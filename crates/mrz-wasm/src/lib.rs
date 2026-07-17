//! WebAssembly bindings for the `mrz` crate — powers the GitHub Pages demo.
//!
//! Everything runs inside the visitor's browser: the image is OCR'd by
//! tesseract.js locally, and this module validates the MRZ. No document data
//! ever leaves the page.

use wasm_bindgen::prelude::*;

/// Scan free-form OCR text for an ICAO 9303 MRZ (TD3 passport or TD1 ID card),
/// parse it and verify every check digit.
///
/// Returns a JSON string: `{"ok": true, ...MrzData}` on success or
/// `{"ok": false, "error": "..."}` when no valid MRZ is found.
#[wasm_bindgen]
pub fn parse_mrz_text(text: &str) -> String {
    match mrz::find_and_parse(text) {
        Ok(data) => {
            let mut v = serde_json::to_value(&data).expect("MrzData serializes");
            v["ok"] = serde_json::Value::Bool(true);
            v["valid"] = serde_json::Value::Bool(data.valid());
            v.to_string()
        }
        Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }).to_string(),
    }
}

/// Compute a single ICAO 9303 check digit for a field (utility/debug helper).
#[wasm_bindgen]
pub fn icao_check_digit(field: &str) -> i32 {
    mrz::check_digit(field).map(|d| d as i32).unwrap_or(-1)
}
