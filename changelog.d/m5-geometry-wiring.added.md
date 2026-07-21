- **OCR geometry now reaches the extraction record, and Tier-2 output is normalized.** The
  `OcrEngine` trait gained `recognize_detailed` alongside `to_markdown` — **additive, not
  breaking**: the default body wraps `to_markdown`, so every existing implementation, in-tree or
  out-of-tree, keeps compiling and behaving identically without a line changed. Only the
  pure-Rust engine overrides it. `OcrResult`/`BBox` are owned by `synthpass-pipeline` rather than
  re-exported from `synthpass-ocr`, which is an optional dependency of that crate — leaking its
  types into a public signature would break every other feature combination. `ExtractionV2.portrait`
  is finally populated from the detected region; as always this is **crop coordinates only**,
  never face recognition or biometric matching (`VISION.md` §2). Tier-2 extractions are run
  through `synthpass_core::normalize::extraction` where they leave the inferer, so every consumer
  including the batch job queue gets `"CROATIA"` → `HRV` and `"JAAK-KRISTJAN"` → `JAAK KRISTJAN`
  for free. Tier-1 output is deliberately left alone: it comes from the checksum-verified MRZ and
  is canonical by construction.
