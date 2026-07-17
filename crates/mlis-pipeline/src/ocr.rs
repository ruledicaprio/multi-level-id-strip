//! OCR engine abstraction: the pipeline gets Markdown from *some* engine.
//!
//! [`DoclingEngine`] (the containerized `docling-serve` service) is the portable
//! default and the only option on native Windows. [`NativeEngine`] (Cargo
//! feature `native-ocr`, Linux/WSL only) drives the in-tree `ocr-daemon`
//! (Tesseract + Leptonica) in-process. The engine is chosen at runtime by
//! `MLIS_OCR_ENGINE` (`docling` | `native`).

use crate::PipelineError;
use async_trait::async_trait;
use docling_rs::DoclingClient;
use std::path::Path;

/// Produces Markdown / plain text from a local image or PDF.
#[async_trait]
pub trait OcrEngine: Send + Sync {
    async fn to_markdown(&self, input: &Path) -> Result<String, PipelineError>;
    /// Short human-readable identity for logs.
    fn describe(&self) -> String;
}

/// Default engine: the containerized `docling-serve` OCR service. Layout-aware,
/// handles PDFs, runs on every platform.
pub struct DoclingEngine {
    client: DoclingClient,
    url: String,
}

impl DoclingEngine {
    pub fn new(url: impl Into<String>) -> Self {
        let url = url.into();
        Self {
            client: DoclingClient::new(url.clone()),
            url,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

#[async_trait]
impl OcrEngine for DoclingEngine {
    async fn to_markdown(&self, input: &Path) -> Result<String, PipelineError> {
        let result = self
            .client
            .convert_file(&[input], None, None)
            .await
            .map_err(|e| PipelineError::Ocr(format!("docling-serve: {e:?}")))?;
        result
            .document
            .md_content
            .clone()
            .ok_or_else(|| PipelineError::NoMarkdown(format!("{:?}", result.errors)))
    }

    fn describe(&self) -> String {
        format!("docling-serve @ {}", self.url)
    }
}

/// Native Tesseract + Leptonica engine (Linux/WSL only, `native-ocr` feature).
/// Runs the blocking OCR on a dedicated thread so it never stalls the async
/// runtime. Image-focused (no PDF); use docling for PDFs.
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

/// Build the OCR engine selected by `MLIS_OCR_ENGINE` (`docling` default;
/// `native` for the Linux-only Tesseract engine). Falls back to docling with a
/// warning if `native` is requested from a build without the `native-ocr`
/// feature (e.g. a Windows build).
pub fn engine_from_env() -> Box<dyn OcrEngine> {
    let docling_url =
        std::env::var("DOCLING_URL").unwrap_or_else(|_| "http://localhost:5001".into());
    match std::env::var("MLIS_OCR_ENGINE")
        .unwrap_or_else(|_| "docling".into())
        .as_str()
    {
        "native" => native_or_fallback(docling_url),
        _ => Box::new(DoclingEngine::new(docling_url)),
    }
}

#[cfg(feature = "native-ocr")]
fn native_or_fallback(_docling_url: String) -> Box<dyn OcrEngine> {
    Box::new(NativeEngine::from_env())
}

#[cfg(not(feature = "native-ocr"))]
fn native_or_fallback(docling_url: String) -> Box<dyn OcrEngine> {
    eprintln!(
        "[mlis] MLIS_OCR_ENGINE=native but this build lacks the `native-ocr` feature \
         (Linux/WSL only) — falling back to docling-serve"
    );
    Box::new(DoclingEngine::new(docling_url))
}
