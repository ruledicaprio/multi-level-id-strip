- **`mrz` now parses MRV-A and MRV-B machine readable visas (ICAO 9303 part 7).** New
  `parse_mrv_a` (two 44-char lines) and `parse_mrv_b` (two 36-char lines) mirror the TD3/TD2
  field geometry through the expiry check digit, but reject any `line1` not starting with `V`.
  MRVs have no personal-number field and no composite check digit at all, so `Checks::personal_number`
  and `Checks::composite` are vacuously `true` — the same convention TD1/TD2 already use for
  `personal_number`. Two new `Format` variants (`MrvA`, `MrvB`) round out the public enum. Verified
  against externally-checked specimen line-2 vectors whose document-number, date-of-birth and
  expiry check digits are correct under the standard 7-3-1 weighting. `find_and_parse` is
  unchanged — it does not yet scan for `V`-prefixed visa lines.
