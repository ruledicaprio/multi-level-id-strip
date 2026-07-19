//! OCR engine abstraction: the pipeline gets Markdown from *some* engine.
//! Image-only as of v0.7.5 — the Docker-based `docling-serve` engine (the
//! project's only PDF-capable path) was retired along with PDF input support;
//! see `CHANGELOG.md`.
//!
//! [`RustOcrEngine`] (Cargo feature `ocr-native-rust`, **default**) runs the
//! pure-Rust `ocrs`/`rten` engine in-process — no Docker, no Python, no C
//! libraries, works unchanged on native Windows. [`NativeEngine`] (Cargo
//! feature `native-ocr`, Linux/WSL only) drives the in-tree `ocr-daemon`
//! (Tesseract + Leptonica) in-process, kept as an accuracy fallback with
//! proven confidence-scored orientation correction. The engine is chosen at
//! runtime by `MLIS_OCR_ENGINE` (`rust` | `native`).
//!
//! Supported input: JPEG, PNG, WebP, TIFF, BMP, GIF (whatever the `image`
//! crate's default features decode). Not supported: PDF (see above) and
//! HEIC/HEIF (Apple's default photo format) — no permissively-licensed
//! pure-Rust decoder exists yet (the two that do are AGPL-3.0), so HEIC/HEIF
//! input is rejected with a clear message rather than silently failing or
//! pulling in AGPL code. See `docs/ARCHITECTURE.md`'s "Supported input
//! formats" note.

use crate::PipelineError;
use async_trait::async_trait;
use std::path::Path;

#[cfg(not(any(feature = "ocr-native-rust", feature = "native-ocr")))]
compile_error!(
    "mlis-pipeline requires at least one of the `ocr-native-rust` or `native-ocr` features"
);

/// Produces Markdown / plain text from a local image.
#[async_trait]
pub trait OcrEngine: Send + Sync {
    async fn to_markdown(&self, input: &Path) -> Result<String, PipelineError>;
    /// Short human-readable identity for logs.
    fn describe(&self) -> String;
}

/// Native Tesseract + Leptonica engine (Linux/WSL only, `native-ocr` feature).
/// Runs the blocking OCR on a dedicated thread so it never stalls the async
/// runtime. Image-focused, which is no longer a limitation — mlis is
/// image-only as of v0.7.5.
#[cfg(feature = "native-ocr")]
pub struct NativeEngine {
    lang: String,
}

#[cfg(feature = "native-ocr")]
impl NativeEngine {
    pub fn new(lang: impl Into<String>) -> Self {
        Self { lang: lang.into() }
    }

    pub fn from_env() -> Self {
        Self::new(std::env::var("MLIS_OCR_LANG").unwrap_or_else(|_| "eng".into()))
    }
}

#[cfg(feature = "native-ocr")]
#[async_trait]
impl OcrEngine for NativeEngine {
    async fn to_markdown(&self, input: &Path) -> Result<String, PipelineError> {
        let path = input.to_path_buf();
        let lang = self.lang.clone();
        tokio::task::spawn_blocking(move || ocr_daemon::image_to_text(&path, &lang))
            .await
            .map_err(|e| PipelineError::Ocr(format!("native OCR task panicked: {e}")))?
            .map_err(|e| PipelineError::Ocr(format!("native OCR: {e}")))
    }

    fn describe(&self) -> String {
        format!("native ocr-daemon (tesseract, lang={})", self.lang)
    }
}

/// Pure-Rust `ocrs`/`rten` engine (feature `ocr-native-rust`, **default**).
/// Lazy-loads both `.rten` weight files on first call and keeps the engine
/// warm for the process lifetime, mirroring `NativeInferer` in `infer.rs`.
/// PDF and HEIC/HEIF input are rejected with a clear, actionable message
/// (see [`OcrEngine::to_markdown`] below) rather than a generic failure.
#[cfg(feature = "ocr-native-rust")]
mod rust_ocr {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::OnceCell;

    pub struct RustOcrEngine {
        model_dir: PathBuf,
        // Only read on the non-`ocr-embedded` load path (see `get_or_load`
        // below) — genuinely unused when models are compiled in instead.
        #[cfg_attr(feature = "ocr-embedded", allow(dead_code))]
        auto_download: bool,
        inner: OnceCell<Arc<mlis_ocr::NativeOcr>>,
    }

    impl RustOcrEngine {
        pub fn new(model_dir: impl Into<PathBuf>, auto_download: bool) -> Self {
            Self {
                model_dir: model_dir.into(),
                auto_download,
                inner: OnceCell::new(),
            }
        }

        pub fn model_dir(&self) -> &std::path::Path {
            &self.model_dir
        }

        async fn get_or_load(&self) -> Result<Arc<mlis_ocr::NativeOcr>, String> {
            self.inner
                .get_or_try_init(|| async {
                    #[cfg(feature = "ocr-embedded")]
                    {
                        // No filesystem/network access at all — the models
                        // are already in the binary (see mlis-ocr/build.rs).
                        return tokio::task::spawn_blocking(mlis_ocr::NativeOcr::load_embedded)
                            .await
                            .map_err(|e| format!("ocr model load task panicked: {e}"))?
                            .map(Arc::new);
                    }
                    #[cfg(not(feature = "ocr-embedded"))]
                    {
                        let model_dir = self.model_dir.clone();
                        let auto_download = self.auto_download;
                        tokio::task::spawn_blocking(move || {
                            let (detection, recognition) = if auto_download {
                                mlis_ocr::download::ensure_models(&model_dir)?
                            } else {
                                (
                                    model_dir.join(mlis_ocr::download::DETECTION_FILENAME),
                                    model_dir.join(mlis_ocr::download::RECOGNITION_FILENAME),
                                )
                            };
                            // Verify on the actual load path, not just in `mlis doctor` —
                            // a tampered or corrupted-but-complete download (whether
                            // fetched just now or cached from a previous run) must fail
                            // closed before it's ever loaded into the OCR engine.
                            if !mlis_ocr::verify::skip_verify() {
                                mlis_ocr::verify::verify_detection_model(&detection)
                                    .map_err(|e| e.to_string())?;
                                mlis_ocr::verify::verify_recognition_model(&recognition)
                                    .map_err(|e| e.to_string())?;
                            }
                            mlis_ocr::NativeOcr::load(&detection, &recognition)
                        })
                        .await
                        .map_err(|e| format!("ocr model load task panicked: {e}"))?
                        .map(Arc::new)
                    }
                })
                .await
                .cloned()
        }
    }

    /// True if `path`'s extension looks like a raster image `ocrs` can read.
    fn looks_like_image(path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .is_some_and(|e| {
                matches!(
                    e.as_str(),
                    "png" | "jpg" | "jpeg" | "webp" | "tif" | "tiff" | "bmp" | "gif"
                )
            })
    }

    /// True if `path`'s extension is HEIC/HEIF — checked ahead of the general
    /// image allowlist so the rejection message names the real reason (no
    /// permissively-licensed pure-Rust decoder) instead of a generic
    /// "unsupported format" error.
    fn looks_like_heic(path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .is_some_and(|e| matches!(e.as_str(), "heic" | "heif"))
    }

    #[async_trait]
    impl OcrEngine for RustOcrEngine {
        async fn to_markdown(&self, input: &Path) -> Result<String, PipelineError> {
            if looks_like_heic(input) {
                return Err(PipelineError::Ocr(
                    "HEIC/HEIF input is not yet supported — no permissively-licensed \
                     pure-Rust decoder exists (the available ones are AGPL-3.0). Convert to \
                     JPEG or PNG first."
                        .into(),
                ));
            }
            if !looks_like_image(input) {
                return Err(PipelineError::Ocr(
                    "PDF input is not supported — mlis is image-only as of v0.7.5. Convert \
                     to an image first."
                        .into(),
                ));
            }
            let ocr = self.get_or_load().await.map_err(PipelineError::Ocr)?;
            let path = input.to_path_buf();
            tokio::task::spawn_blocking(move || ocr.recognize(&path))
                .await
                .map_err(|e| PipelineError::Ocr(format!("ocr task panicked: {e}")))?
                .map_err(PipelineError::Ocr)
        }

        fn describe(&self) -> String {
            format!("pure-rust ocr (ocrs/rten) @ {}", self.model_dir.display())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // These don't need real model files: both rejections happen on the
        // extension check, before `get_or_load()` ever runs — the path need
        // not even exist.

        #[tokio::test]
        async fn rejects_pdf_with_an_actionable_message() {
            let engine = RustOcrEngine::new(".", false);
            let err = engine
                .to_markdown(Path::new("document.pdf"))
                .await
                .expect_err("PDF input must be rejected, not silently processed");
            let PipelineError::Ocr(msg) = err else {
                panic!("expected PipelineError::Ocr");
            };
            assert!(msg.contains("PDF"), "message should name PDF: {msg}");
            assert!(
                msg.contains("image-only"),
                "message should explain why: {msg}"
            );
        }

        #[tokio::test]
        async fn rejects_heic_with_an_actionable_message() {
            let engine = RustOcrEngine::new(".", false);
            for ext in ["heic", "heif", "HEIC"] {
                let err = engine
                    .to_markdown(Path::new(&format!("photo.{ext}")))
                    .await
                    .expect_err("HEIC/HEIF input must be rejected, not silently processed");
                let PipelineError::Ocr(msg) = err else {
                    panic!("expected PipelineError::Ocr");
                };
                assert!(
                    msg.contains("HEIC") || msg.contains("HEIF"),
                    "message should name HEIC/HEIF: {msg}"
                );
                assert!(
                    msg.contains("JPEG") || msg.contains("PNG"),
                    "message should suggest a conversion target: {msg}"
                );
            }
        }
    }
}
#[cfg(feature = "ocr-native-rust")]
pub use rust_ocr::RustOcrEngine;

/// Default model directory when `MLIS_OCR_MODEL_DIR` is unset — the repo root.
#[cfg(feature = "ocr-native-rust")]
const DEFAULT_OCR_MODEL_DIR: &str = ".";

/// Build the OCR engine selected by `MLIS_OCR_ENGINE` (`rust` default,
/// pure-Rust `ocrs`/`rten`; `native` for the Linux-only Tesseract engine).
/// Falls back to whichever engine *is* compiled in (with a warning) if the
/// requested one isn't — at least one is always compiled in, enforced by the
/// `compile_error!` at the top of this file.
pub fn engine_from_env() -> Box<dyn OcrEngine> {
    match std::env::var("MLIS_OCR_ENGINE")
        .unwrap_or_else(|_| "rust".into())
        .as_str()
    {
        "native" => native_choice(),
        _ => rust_choice(),
    }
}

#[cfg(feature = "native-ocr")]
fn native_choice() -> Box<dyn OcrEngine> {
    Box::new(NativeEngine::from_env())
}

#[cfg(not(feature = "native-ocr"))]
fn native_choice() -> Box<dyn OcrEngine> {
    eprintln!(
        "[mlis] MLIS_OCR_ENGINE=native requested but this build lacks the `native-ocr` \
         feature (Linux/WSL only) — using the pure-Rust engine instead"
    );
    rust_choice()
}

#[cfg(feature = "ocr-native-rust")]
fn rust_choice() -> Box<dyn OcrEngine> {
    let model_dir =
        std::env::var("MLIS_OCR_MODEL_DIR").unwrap_or_else(|_| DEFAULT_OCR_MODEL_DIR.into());
    let auto_download = std::env::var("MLIS_OCR_AUTO_DOWNLOAD").as_deref() != Ok("0");
    Box::new(RustOcrEngine::new(model_dir, auto_download))
}

#[cfg(not(feature = "ocr-native-rust"))]
fn rust_choice() -> Box<dyn OcrEngine> {
    eprintln!(
        "[mlis] MLIS_OCR_ENGINE=rust (default) requested but this build lacks the \
         `ocr-native-rust` feature — using the native Tesseract engine instead"
    );
    native_choice()
}
