- **`mrz` 0.5.0: the check-digit oracle's blind spots are now a public API, and the crate has a
  CI-enforced MSRV.** An ICAO 9303 check digit (part 3 §4.9) is a 7-3-1 weighted sum taken mod 10,
  so it sees only each character's value mod 10 — meaning two characters are indistinguishable to
  *every* check digit exactly when their values are congruent mod 10. That law was previously
  trapped in `examples/checksum_blindspots.rs`, computed with the example's own private `value()`
  helper and printed to stdout, where no downstream OCR pipeline could consume it. It is now
  `blindspot(a, b) -> Blindspot` (`Identical` / `Caught { delta_mod10 }` / `Blind { residue }` /
  `NotMrzCharset`, with `is_blind()`), `collisions(c) -> Vec<char>` for the complete blind set of a
  character, `class_of(c) -> Option<&'static [char]>` for the allocation-free form, and `CLASSES`,
  the ten residue classes partitioning the 37-character MRZ alphabet. The result inverts most
  intuitions and is worth stating out loud: O↔0, I↔1, B↔8, S↔5 and Z↔2 are all **caught**, while
  K↔`<`, I↔S, B↔L and A↔K are **blind**. A new integration test sweeps every data position of the
  ICAO TD3 specimen across the whole alphabet and asserts the algebra agrees with what `parse_td3`
  actually does, so the API is proven against the engine rather than restating it; the example now
  builds on the same API and cross-checks `collisions` against the parser at runtime. Separately,
  `crates/mrz/Cargo.toml` now declares `rust-version = "1.82"` — the verified floor for the
  zero-dependency default build, set by `std::iter::repeat_n` and `Option::is_none_or` in the OCR
  repair helpers — enforced by a new lightweight `msrv` CI job, with the policy documented in the
  README alongside new badges and a verified feature comparison against `mrtd`.
