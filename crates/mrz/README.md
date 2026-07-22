# mrz

[![crates.io](https://img.shields.io/crates/v/mrz.svg)](https://crates.io/crates/mrz)
[![docs.rs](https://docs.rs/mrz/badge.svg)](https://docs.rs/mrz)
[![MSRV](https://img.shields.io/badge/MSRV-1.82-blue.svg)](#minimum-supported-rust-version)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](#license)

A zero-dependency [ICAO 9303](https://www.icao.int/publications/pages/publication.aspx?docnum=9303)
Machine Readable Zone (MRZ) parser and check-digit validator for Rust.

A valid composite check digit is **mathematical proof** that an OCR read is
faithful to the printed document — not a probability, not a model score. `mrz`
verifies every printed check digit under the standard 7-3-1 weighting and
exposes the result per field, so you always know *which* digit proved (or
failed) the read.

The core crate has **no runtime dependencies** and compiles to
`wasm32-unknown-unknown` as readily as to native targets.

**▶ [Try it in your browser](https://ruledicaprio.github.io/SynthPass/)** — live WASM MRZ validator.

## Supported formats

| Format | Document                          | Layout            | Status |
| ------ | --------------------------------- | ----------------- | ------ |
| TD3    | Passports                         | 2 lines × 44      | ✅ parse + emit |
| TD2    | Official travel documents / IDs   | 2 lines × 36      | ✅ parse + emit |
| TD1    | ID cards                          | 3 lines × 30      | ✅ parse + emit |
| MRV-A  | Visas (passport-book)             | 2 lines × 44      | ✅ parse + emit |
| MRV-B  | Visas (smaller)                   | 2 lines × 36      | ✅ parse + emit |

Document-number overflow (ICAO 9303 part 4 §4.2.2.2) is supported for TD1/TD2/
TD3: when a document number is longer than the 9-character field, `format_td3`
/ `format_td2` / `format_td1` reassemble it automatically as long as the
remainder fits the format's optional-data field (TD3 personal number, 14
chars; TD2/TD1 optional data, 7/15 chars), and the parser reads it back into
`MrzData::document_number_full`:

```rust
use mrz::{format_td3, Td3Fields};

let lines = format_td3(&Td3Fields {
    issuing_country: "UTO".into(),
    document_number: "L898902C31234".into(), // 13 chars, overflows the 9-char field
    surname: "ERIKSSON".into(),
    given_names: "ANNA MARIA".into(),
    nationality: "UTO".into(),
    date_of_birth: "740812".into(),
    sex: "F".into(),
    date_of_expiry: "120415".into(),
    ..Default::default()
});

let (l1, l2) = lines.split_once('\n').unwrap();
let doc = mrz::parse_td3(l1, l2).unwrap();
assert!(doc.valid());
assert_eq!(doc.document_number, "L898902C"); // the printed 9-char field
assert_eq!(doc.full_document_number(), "L898902C31234"); // the reassembled number
```

When the remainder doesn't fit the optional field, the number still truncates
to 9 characters as before (unchanged, pre-existing behavior). MRV-A/MRV-B
visas have no overflow encoding in ICAO 9303 part 7, so this only applies to
TD1/TD2/TD3.

### Compared to other Rust MRZ crates

The closest published alternative is [`mrtd`](https://docs.rs/mrtd). Verified
against its docs.rs page at the time of writing:

|                            | `mrz`                     | `mrtd`             |
| -------------------------- | ------------------------- | ------------------ |
| Zero-dependency core       | ✅                        | ❌ chrono, regex, lazy_static |
| Emit (format) MRZ lines    | ✅ all five formats       | ❌ parse only      |
| MRV-A / MRV-B visas        | ✅                        | ❌                 |
| Document-number overflow   | ✅ TD1/TD2/TD3            | ❌                 |
| OCR repair heuristics      | ✅ checksum-guided        | ❌                 |
| `wasm32-unknown-unknown`   | ✅ default build          | not documented     |

## Usage

Add it to `Cargo.toml`:

```toml
[dependencies]
mrz = "0.5"
```

### Minimum supported Rust version

**1.82.** The floor is set by `std::iter::repeat_n` and `Option::is_none_or`
in the OCR repair helpers, both stabilized in that release. It applies to the
zero-dependency default build and is enforced by CI on every push; the
optional `serde`/`zeroize` features and the dev-dependency test suite follow
their own upstream MSRVs, which are higher. The MSRV is a floor, not a pin —
raising it is a minor version bump.

### Scan free-form OCR text

`find_and_parse` locates an MRZ inside noisy OCR output (it tolerates
HTML-escaped fillers and lines merged onto one physical line) and drives a
check-digit-guided repair pass — a candidate reading is accepted only when its
composite check digit proves it.

```rust
let text = "## REPUBLIC OF UTOPIA\n\
            P<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<\n\
            L898902C36UTO7408122F1204159ZE184226B<<<<<10";

let doc = mrz::find_and_parse(text).expect("an MRZ");
assert_eq!(doc.surname, "ERIKSSON");
assert_eq!(doc.given_names, "ANNA MARIA");
assert!(doc.valid()); // every check digit verified
```

### Parse known lines directly

```rust
let doc = mrz::parse_td3(
    "P<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<",
    "L898902C36UTO7408122F1204159ZE184226B<<<<<10",
).unwrap();

// Per-field proof, not a single boolean.
assert!(doc.checks.document_number);
assert!(doc.checks.date_of_birth);
assert!(doc.checks.composite);
```

`parse_td1` and `parse_td2` cover the other two formats.

### Emit a valid MRZ

All three formats emit — `format_td3` / `format_td2` / `format_td1`, each taking
its own `Td3Fields` / `Td2Fields` / `Td1Fields` in MRZ-native form (`YYMMDD`
dates, uppercase `[A-Z0-9]`). Every check digit is computed for you, so the
output always round-trips back through the matching parser as `valid()`.

```rust
use mrz::{format_td3, Td3Fields};

let lines = format_td3(&Td3Fields {
    issuing_country: "UTO".into(),
    document_number: "L898902C3".into(),
    surname: "ERIKSSON".into(),
    given_names: "ANNA MARIA".into(),
    nationality: "UTO".into(),
    date_of_birth: "740812".into(), // YYMMDD, MRZ-native
    sex: "F".into(),
    date_of_expiry: "120415".into(),
    personal_number: Some("ZE184226B".into()),
    ..Default::default()
});
// Round-trips: parse_td3(&lines) is valid() == true.
```

`Td2Fields` and `Td1Fields` follow the same shape (with `optional_data` /
`optional_data_1` + `optional_data_2` respectively):

```rust
use mrz::{format_td1, Td1Fields};

let td1 = format_td1(&Td1Fields {
    issuing_country: "UTO".into(),
    document_number: "D23145890".into(),
    surname: "ERIKSSON".into(),
    given_names: "ANNA MARIA".into(),
    nationality: "UTO".into(),
    date_of_birth: "740812".into(),
    sex: "F".into(),
    date_of_expiry: "120415".into(),
    ..Default::default() // document_code defaults to "I"
});
// Three 30-char lines; parse_td1 rebuilds them as valid().
```

## A valid read is not an in-date document

A verified composite check digit proves the *read* is faithful — it says
nothing about whether the document is expired or its dates are internally
consistent. That separate, non-cryptographic judgement is
`MrzData::validity(today)`:

```rust
use mrz::{parse_td3, Date};

// The ICAO 9303 part 4 specimen: expires 2012-04-15.
let doc = parse_td3(
    "P<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<",
    "L898902C36UTO7408122F1204159ZE184226B<<<<<10",
)
.unwrap();
assert!(doc.valid()); // the read is checksum-proven ...

let report = doc.validity(Date::new(2010, 1, 1));
assert!(report.in_date); // ... and, as of 2010, still in date
assert!(report.dob_before_expiry);

// The same faithful read, judged against a later day: still valid(), expired.
assert!(!doc.validity(Date::new(2020, 1, 1)).in_date);
```

## What check digits cannot prove

The same honest-limits argument, applied to characters rather than dates. A
check digit is a 7-3-1 weighted sum taken **mod 10** (ICAO 9303 part 3 §4.9),
so it sees only each character's value mod 10. Two characters are
indistinguishable to *every* check digit exactly when their values are
congruent mod 10 — and `blindspot` states that outright:

```rust
use mrz::{blindspot, collisions, Blindspot};

// The OCR confusion everyone worries about is CAUGHT ...
assert!(matches!(blindspot('O', '0'), Blindspot::Caught { delta_mod10: 4 }));
// ... and the quiet one nobody mentions is not.
assert!(blindspot('K', '<').is_blind());

// The complete blind set for a character, not a sample of it.
assert_eq!(collisions('K'), vec!['0', 'A', 'U', '<']);
```

That inversion is the point:

| Pair    | Verdict    | Why                         |
| ------- | ---------- | --------------------------- |
| O ↔ 0   | **caught** | 24 vs 0 — differ mod 10     |
| I ↔ 1   | **caught** | 18 vs 1                     |
| B ↔ 8   | **caught** | 11 vs 8                     |
| S ↔ 5   | **caught** | 28 vs 5                     |
| Z ↔ 2   | **caught** | 35 vs 2                     |
| K ↔ `<` | **blind**  | 20 vs 0 — congruent mod 10  |
| I ↔ S   | **blind**  | 18 vs 28                    |
| B ↔ L   | **blind**  | 11 vs 21                    |
| A ↔ K   | **blind**  | 10 vs 20                    |

`mrz::CLASSES` is the whole atlas: ten residue classes partitioning the
37-character alphabet. Class 0 is `0 A K U <` — five members, because the
filler and the digit zero share a value. Any substitution *within* a row is
provably undetectable; any substitution *across* rows is caught. This is
precisely why the crate layers structural guards on top of the arithmetic
(recognized country codes, date plausibility, name charset).

`cargo run -p mrz --example checksum_blindspots` demonstrates the law
empirically — it mutates the ICAO specimen character by character and asks the
real parser, rather than asserting the algebra at you.

## Feature flags

Both are off by default, keeping the base crate zero-dependency (and
wasm-clean):

- **`serde`** — derives `Serialize` + `Deserialize` on `MrzData`, `Checks`,
  `Format`, `Td3Fields`, `Date`, and `DateValidity`.
- **`zeroize`** — derives `ZeroizeOnDrop` on `MrzData`, wiping the PII-bearing
  `String` fields from memory when the value is dropped.

## License

MIT
