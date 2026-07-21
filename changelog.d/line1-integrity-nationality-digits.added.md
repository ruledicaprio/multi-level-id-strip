- **`synthpass_core::fusion::check_line1_integrity` gained two new findings, chosen by measuring
  candidates over ~150 specimens first rather than shipping on intuition**
  (`crates/synthpass-ocr/examples/integrity_survey.rs`, `docs/integrity-survey.jsonl`):
  - `UnrecognizedNationality` — `nationality` isn't a recognized ICAO/ISO 3166-1 code. The TD3
    composite check digit excludes `nationality` entirely, so nothing else in the parser or the
    checksum math ever looks at this field.
  - `NonAlphabeticName` — a digit appears in `surname` or `given_names`. ICAO 9303 names are
    alphabetic by convention, but the parser's charset check accepts `0-9` across the whole line
    (line 2 needs it), so a digit landing in a name field went uncaught.

  Both were measured before shipping: across the survey corpus, each fired only on records an
  existing finding had already flagged — never alone on a checksum-valid, otherwise-`Accepted`
  document. A third candidate — reconstructing the 39-char name field from the parsed
  `surname`/`given_names` and comparing against the raw MRZ line — was measured and **rejected**:
  it false-positived on genuine, checksum-valid specimens (e.g. `Spain_Passport_Specimen.png`)
  because `mrz::parser::clean_name` is lossy — any interior filler run of 2+ `<` collapses to a
  single space via `.trim()`, so a name with a wider-than-minimum filler gap can never be
  byte-reconstructed from the parsed strings alone.
