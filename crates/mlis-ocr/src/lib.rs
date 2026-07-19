//! In-process pure-Rust OCR for Tier 1 via `ocrs`/`rten` — the default engine
//! since v0.7.0 (it replaced the `docling-serve` Docker OCR service that
//! version), mirroring `mlis-llm`'s `NativeLlm` naming/lifecycle pattern but
//! for text detection+recognition instead of generation.
//!
//! [`NativeOcr`] loads both `.rten` weight files once and is kept warm for
//! the process lifetime; `recognize` is blocking — callers on an async
//! runtime (see `mlis-pipeline`) must run it via `spawn_blocking`, mirroring
//! how the native LLM inferer is wrapped.
//!
//! Image-only: `ocrs` has no PDF parsing, and as of v0.7.5 there is no other
//! engine to route PDF input to — PDF is rejected outright at the
//! `mlis-pipeline` layer (see `crates/mlis-pipeline/src/ocr.rs`).

pub mod download;
#[cfg(feature = "embedded-models")]
pub mod embedded;
pub mod verify;

use ocrs::{ImageSource, OcrEngine as OcrsEngine, OcrEngineParams};
use rten::Model;
use std::path::Path;

pub struct NativeOcr {
    engine: OcrsEngine,
}

impl NativeOcr {
    /// Load both `.rten` model files and build a warm, reusable engine.
    pub fn load(detection_path: &Path, recognition_path: &Path) -> Result<Self, String> {
        let detection_model = Model::load_file(detection_path).map_err(|e| {
            format!(
                "failed to load detection model at {}: {e}",
                detection_path.display()
            )
        })?;
        let recognition_model = Model::load_file(recognition_path).map_err(|e| {
            format!(
                "failed to load recognition model at {}: {e}",
                recognition_path.display()
            )
        })?;
        let engine = OcrsEngine::new(OcrEngineParams {
            detection_model: Some(detection_model),
            recognition_model: Some(recognition_model),
            ..Default::default()
        })
        .map_err(|e| format!("failed to build ocrs engine: {e}"))?;
        Ok(Self { engine })
    }

    /// Build a warm engine from the models baked into the binary at compile
    /// time (`embedded-models` feature) — no filesystem or network access,
    /// for a true single-file air-gapped deployment (see
    /// docs/ARCHITECTURE.md §10).
    #[cfg(feature = "embedded-models")]
    pub fn load_embedded() -> Result<Self, String> {
        let detection_model = Model::load_static_slice(embedded::DETECTION_BYTES)
            .map_err(|e| format!("failed to load embedded detection model: {e}"))?;
        let recognition_model = Model::load_static_slice(embedded::RECOGNITION_BYTES)
            .map_err(|e| format!("failed to load embedded recognition model: {e}"))?;
        let engine = OcrsEngine::new(OcrEngineParams {
            detection_model: Some(detection_model),
            recognition_model: Some(recognition_model),
            ..Default::default()
        })
        .map_err(|e| format!("failed to build ocrs engine: {e}"))?;
        Ok(Self { engine })
    }

    /// Run OCR on the image at `image_path`, returning all recognized text as
    /// a single string — exactly what Tier 1's MRZ pattern search and the
    /// Tier-2 LLM prompt both need; no requirement for structured layout.
    pub fn recognize(&self, image_path: &Path) -> Result<String, String> {
        let image = image::open(image_path)
            .map_err(|e| format!("failed to open image {}: {e}", image_path.display()))?
            .into_rgb8();
        let source = ImageSource::from_bytes(image.as_raw(), image.dimensions())
            .map_err(|e| format!("failed to prepare image source: {e}"))?;
        let input = self
            .engine
            .prepare_input(source)
            .map_err(|e| format!("failed to prepare ocr input: {e}"))?;
        self.engine
            .get_text(&input)
            .map_err(|e| format!("ocr text extraction failed: {e}"))
    }
}
