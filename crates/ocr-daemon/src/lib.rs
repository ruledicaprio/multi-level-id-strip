//! Native OCR engine (Linux/WSL): Tesseract + Leptonica via `leptess`, with a
//! preprocessing pass (grayscale + Otsu binarization).
//!
//! **Linux/WSL only** — this crate links the system `libtesseract` and
//! `libleptonica` libraries (apt: `libtesseract-dev libleptonica-dev clang
//! tesseract-ocr-eng`). Windows/macOS builds should exclude it and build the
//! cross-platform crates explicitly; the pipeline reaches it only through the
//! `native-ocr` feature of `mlis-pipeline`.
//!
//! It is deliberately image-focused (no PDF): `docling-serve` remains the engine
//! for PDFs and layout-heavy documents. The output is plain recognized text,
//! which the same [`mrz`](../mrz) scanner and LLM tier consume as "Markdown".

pub mod preprocess;

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("image load/encode failed: {0}")]
    Image(#[from] image::ImageError),
    #[error("tesseract: {0}")]
    Tesseract(String),
}

/// OCR an image file into text.
///
/// Grayscales + Otsu-binarizes the image, then runs Tesseract in `lang` (e.g.
/// `"eng"`). Tesseract locates its trained data via the standard install path /
/// `TESSDATA_PREFIX`.
pub fn image_to_text(path: &Path, lang: &str) -> Result<String, OcrError> {
    let img = image::open(path)?;
    let binarized = preprocess::binarize(&img);

    // leptess reads from an in-memory encoded image; hand it a PNG buffer.
    let mut buf = std::io::Cursor::new(Vec::new());
    binarized.write_to(&mut buf, image::ImageFormat::Png)?;

    let mut lt = leptess::LepTess::new(None, lang)
        .map_err(|e| OcrError::Tesseract(format!("init(lang={lang}): {e}")))?;
    lt.set_image_from_mem(buf.get_ref())
        .map_err(|e| OcrError::Tesseract(format!("set_image_from_mem: {e}")))?;
    lt.get_utf8_text()
        .map_err(|e| OcrError::Tesseract(format!("get_utf8_text: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real OCR against a bundled specimen. Ignored by default (needs the
    // tesseract 'eng' trained data installed); run with:
    //   cargo test -p ocr-daemon -- --ignored --nocapture
    #[test]
    #[ignore]
    fn ocr_reads_specimen_image() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../samples/Croatian_passport_data_page.jpg"
        );
        let text = image_to_text(Path::new(path), "eng").expect("OCR should run");
        println!("--- OCR output ---\n{text}\n--- end ---");
        assert!(!text.trim().is_empty(), "expected some recognized text");
    }
}
