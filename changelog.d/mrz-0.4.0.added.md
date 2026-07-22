- **`mrz` 0.4.0: document-number overflow, which check digit failed, and a tunable century
  pivot.** Document numbers longer than the 9-character field (ICAO 9303 part 4 §4.2.2.2) are now
  a first-class TD1/TD2/TD3 feature instead of a documented gap: `format_td3`/`format_td2`/
  `format_td1` reassemble the overflow automatically whenever the remainder fits the format's
  optional-data field (TD3 personal number 14 chars, TD2 optional data 7, TD1 optional data 1 15),
  and the parsers surface it on the new `MrzData::document_number_full` / `full_document_number()`.
  A remainder that still doesn't fit truncates to 9 characters exactly as before. MRV-A/MRV-B
  visas have no overflow encoding in part 7, so they're unaffected. Separately, a new `Field` enum
  and `Checks::failed() -> Vec<Field>` name which check digit(s) disagreed instead of leaving
  callers to infer it from five booleans, backing a new `MrzError::BadChecksum { field, position }`
  variant for callers that want to turn a failed `Checks` into an error of their own (`MrzError` is
  now `#[non_exhaustive]`). A new `ParseOptions { pivot_yy }` plus `parse_td3_with`/`parse_td2_with`/
  `parse_td1_with`/`parse_mrv_a_with`/`parse_mrv_b_with`/`find_and_parse_with` let a caller pin the
  two-digit-year century pivot explicitly instead of inheriting the constant the crate was built
  with; the existing `parse_*`/`find_and_parse` entry points are unchanged, delegating to the
  default. `find_and_parse`'s no-fully-valid-candidate fallback now returns the *best-scoring*
  reading (most passing check digits) instead of the first candidate found — same signature, same
  `Ok(invalid MrzData)` contract, a strictly better answer when nothing validates outright. And
  `aggressive_defiller`'s repair loop now has an explicit `MAX_DEFILL_PASSES` bound instead of
  relying on convergence.
