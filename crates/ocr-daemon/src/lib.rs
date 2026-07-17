//! Native OCR engine (Linux/WSL): Tesseract + Leptonica via `leptess`, with a
//! preprocessing pipeline (DPI normalization, orientation correction,
//! deskew, grayscale + Otsu binarization) tuned for phone-camera document
//! photos.
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
/// Normalizes DPI, corrects orientation, deskews, then grayscales +
/// Otsu-binarizes the image before running Tesseract in `lang` (e.g. `"eng"`).
/// Tesseract locates its trained data via the standard install path /
/// `TESSDATA_PREFIX`.
pub fn image_to_text(path: &Path, lang: &str) -> Result<String, OcrError> {
    let img = image::open(path)?;
    let normalized = preprocess::normalize_dpi(&img);
    let oriented = preprocess::correct_orientation(&normalized, lang);
    let deskewed = preprocess::deskew(&oriented);
    let binarized = preprocess::binarize(&deskewed);

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

    // Confirms the full pipeline (orientation correction in particular) still
    // recovers readable text from a 90-degree-rotated version of the same
    // specimen. Ignored by default — needs real tessdata, same as above.
    #[test]
    #[ignore]
    fn ocr_reads_specimen_image_rotated_90_degrees() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../samples/Croatian_passport_data_page.jpg"
        );
        let img = image::open(path).expect("open specimen");
        let rotated = image::DynamicImage::ImageRgb8(image::imageops::rotate90(&img.to_rgb8()));

        let tmp = std::env::temp_dir().join(format!(
            "ocr-daemon-rotated-specimen-{}.png",
            std::process::id()
        ));
        rotated.save(&tmp).expect("save rotated specimen");

        let text = image_to_text(&tmp, "eng");
        std::fs::remove_file(&tmp).ok();
        let text = text.expect("OCR should run on rotated input");
        println!("--- OCR output (rotated input) ---\n{text}\n--- end ---");
        assert!(
            !text.trim().is_empty(),
            "expected orientation correction to recover readable text"
        );
    }
}
