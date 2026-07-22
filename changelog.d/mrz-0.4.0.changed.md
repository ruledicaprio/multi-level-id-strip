- **`mrz` 0.4.0 makes the output types `#[non_exhaustive]`, so that future additions stop being
  breaking changes.** `MrzData`, `Checks` and `Format` can no longer be constructed by literal or
  matched exhaustively from outside the crate: obtain them from a `parse_*` function, and give a
  `match` on `Format` a `_` arm. This is a one-time cost paid deliberately inside an
  already-breaking release. Every breaking change this crate has shipped was an addition to one of
  those three types — `Format` gained `MrvA`/`MrvB` in 0.3.0, `MrzData` gains
  `document_number_full` here — and each forced a minor bump that downstream `Cargo.toml`s had to
  chase. From 0.4.0 on the same additions are patch releases. The emit-side input structs
  (`Td3Fields` and friends) are deliberately left exhaustive: `#[non_exhaustive]` forbids
  `..Default::default()` as well as full literals, which would leave callers no way to build one
  short of field-by-field mutation — too high a price on a type whose whole job is being filled in.
- **`MrzData` gained the public field `document_number_full: Option<String>`,** and `MrzError`
  gained a `BadChecksum` variant alongside becoming `#[non_exhaustive]`, so an exhaustive match
  over it needs a `_` arm. Code that only *reads* these types — the overwhelmingly common case —
  needs no change at all.
