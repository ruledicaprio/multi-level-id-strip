//! Line-1 integrity checks for ICAO 9303 MRZ reads.
//!
//! **The defect this module exists to catch**: TD1/TD2/TD3 check digits cover
//! only `document_number`, `date_of_birth`, `date_of_expiry`, and
//! `personal_number` (verified directly against the real ICAO fixture in
//! `mrz::dates` tests and `mrz::parser`'s composite ranges — the composite
//! excludes `nationality` and `sex` too, matching the published standard, not
//! a bug in this codebase). `document_type`, `issuing_country`, `surname`,
//! `given_names` carry **no check digit at all**. A document can be
//! checksum-proven — `MrzData::valid() == true` — and still have the wrong
//! name, because nothing mathematically ties line 1 to anything.
//!
//! Measured on the synthetic corpus (`synthpass-bench`, `feat/bench-per-field-cer`):
//! of documents passing the Tier-1 gate, the dominant failure is OCR
//! collapsing interior `<` filler runs in line 1 while the trailing filler
//! absorbs the loss, so the line stays the correct length and parses without
//! error while every field boundary shifts left
//! (`P<JPNSTRAND<<ALEKSANDER<<<…` → `PJPNSTRANDALEKSANDER<<<<…`). This module
//! catches that specific, reproducible corruption deterministically, using
//! data that already ships (`mrz::country_name`) rather than a model.
//!
//! What this is not: a posterior probability. See `Support`'s doc comment —
//! the ranking is ordinal on purpose.
//!
//! **`UnrecognizedNationality` and `NonAlphabeticName`** (added after the
//! above) were chosen by measuring candidate checks over ~150 specimens
//! (`crates/synthpass-ocr/examples/integrity_survey.rs`,
//! `docs/integrity-survey.jsonl`) before shipping them: both fired only on
//! records that an existing finding already flagged, never alone on a
//! checksum-valid, otherwise-`Accepted` document. A third candidate —
//! reconstructing the 39-char name field via `mrz::emit`'s canonicalization
//! and comparing against the raw line — was measured and **rejected**: it
//! false-positived on multiple genuine, checksum-valid specimens (e.g.
//! `Spain_Passport_Specimen.png`) because `mrz::parser::clean_name` is lossy
//! — it collapses any interior filler run of 2+ `<` to a single space via
//! `.trim()`, so a name with a wider-than-minimum filler gap can never be
//! byte-reconstructed from the parsed `surname`/`given_names` strings alone.
//! Same failure shape as a naive filler-count check would have: measure
//! before shipping.

use mrz::MrzData;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// Why a field's value is believed, ranked from strongest to weakest.
///
/// Deliberately an ordinal enum, not a float. A calibrated posterior needs a
/// measured likelihood function; SynthPass has never produced a calibration
/// curve (a reliability diagram from `synthpass-gen`'s labeled corpus would
/// be the way to earn one). Inventing likelihoods and combining them with
/// Bayes' rule would launder a guess into a number with decimal places —
/// exactly the failure `FieldConfidence::proven() == 1.0` on unverified line-1
/// fields already demonstrates. An ordinal scale says only "which claim is
/// stronger", which is what the data actually supports today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    /// An ICAO check digit mathematically verifies this exact value.
    CheckDigit,
    /// Two values parsed from different MRZ lines by the same OCR pass agree.
    /// Weaker than a check digit — neither side is proven — but the two
    /// values are not derived from the same bytes, so agreement is a real,
    /// if modest, reduction in correlated risk.
    CrossField,
    /// Parsed at a fixed offset; nothing beyond charset and length checks it.
    Structural,
}

/// One integrity finding about a parsed MRZ record. `NeedsReview`'s `reasons`
/// are these, rendered.
///
/// Carries PII: `got`/`issuing_country`/`nationality` are copies of the same
/// ICAO country codes already stored (and zeroized) in `ExtractionFields` —
/// short, but real. `Zeroize`d field-by-field rather than skipped, matching
/// [`crate::v2::MrzBlock`]'s discipline for its own copy of the raw zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    /// `issuing_country` is not a recognized ICAO/ISO 3166-1 code — the
    /// clearest, cheapest signal of a shifted line 1.
    UnrecognizedIssuingCountry { got: String },
    /// `issuing_country` and `nationality` disagree, and `nationality` *is*
    /// a recognized code (so this isn't just two unrecognized strings talking
    /// past each other). `Support::CrossField` — see the doc comment above.
    IssuingCountryNationalityMismatch {
        issuing_country: String,
        nationality: String,
    },
    /// `given_names` is empty while `surname` is long — the signature of the
    /// collapsed-filler corruption: `parse_td3`'s `<<` split
    /// (`mrz::parser::clean_name`) never fired, so the whole name line landed
    /// in `surname`. `surname_len` is a length, not PII, but carries no
    /// `Zeroize` impl of its own (`usize` isn't `Copy`-zeroizable by derive
    /// without an explicit skip).
    MissingNameSeparator {
        #[zeroize(skip)]
        surname_len: usize,
    },
    /// `nationality` is not a recognized ICAO/ISO 3166-1 code. Separate from
    /// `IssuingCountryNationalityMismatch` — that only fires when
    /// `nationality` *is* recognized but disagrees with `issuing_country`.
    /// Worth checking on its own: the TD3 composite check digit excludes
    /// `nationality` entirely (see the module doc comment), so nothing else
    /// in the parser or the checksum math ever looks at this field.
    UnrecognizedNationality { got: String },
    /// A ASCII digit appears in `surname` or `given_names`. ICAO 9303 names
    /// are alphabetic by convention, but `parser::ensure_charset` accepts
    /// `0-9` across the whole line (it has to — line 2 is mostly digits), so
    /// nothing upstream of this module rejects a digit landing in a name
    /// field. Deliberately doesn't carry the matched character or the name
    /// itself: which field is enough to act on, and it keeps this variant
    /// off the highest-PII field in the record. `field` is always
    /// `"surname"` or `"given_names"` — not PII, but `String` (not
    /// `&'static str`) so `Finding` can keep deriving `Deserialize`.
    NonAlphabeticName { field: String },
}

/// Verdict for an [`MrzData`] record, from the checks in this module.
/// Distinct from [`mrz::MrzData::valid`] — that asks "do the check digits
/// verify", this asks "does the rest of the record look internally
/// consistent". A document can pass one and fail the other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Verdict {
    /// No integrity findings.
    Accepted,
    /// At least one finding, none severe enough to reject outright.
    NeedsReview { reasons: Vec<Finding> },
}

/// Threshold, in characters, above which an empty `given_names` next to a
/// long `surname` is flagged rather than treated as a plausible single-name
/// document (real single-name passports exist; ICAO allows it).
const SUSPICIOUSLY_LONG_UNSPLIT_NAME: usize = 12;

/// Runs the line-1 integrity checks over an already-parsed, checksum-passing
/// [`MrzData`] record. Callers should still gate on [`MrzData::valid`]
/// first — this module says nothing about line 2.
pub fn check_line1_integrity(m: &MrzData) -> Verdict {
    let mut reasons = Vec::new();

    match mrz::country_name(&m.issuing_country) {
        None => reasons.push(Finding::UnrecognizedIssuingCountry {
            got: m.issuing_country.clone(),
        }),
        Some(_) => {
            if mrz::country_name(&m.nationality).is_some() && m.issuing_country != m.nationality {
                reasons.push(Finding::IssuingCountryNationalityMismatch {
                    issuing_country: m.issuing_country.clone(),
                    nationality: m.nationality.clone(),
                });
            }
        }
    }

    if m.given_names.is_empty() && m.surname.len() > SUSPICIOUSLY_LONG_UNSPLIT_NAME {
        reasons.push(Finding::MissingNameSeparator {
            surname_len: m.surname.len(),
        });
    }

    if mrz::country_name(&m.nationality).is_none() {
        reasons.push(Finding::UnrecognizedNationality {
            got: m.nationality.clone(),
        });
    }

    if m.surname.chars().any(|c| c.is_ascii_digit()) {
        reasons.push(Finding::NonAlphabeticName {
            field: "surname".to_string(),
        });
    }
    if m.given_names.chars().any(|c| c.is_ascii_digit()) {
        reasons.push(Finding::NonAlphabeticName {
            field: "given_names".to_string(),
        });
    }

    if reasons.is_empty() {
        Verdict::Accepted
    } else {
        Verdict::NeedsReview { reasons }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> MrzData {
        // The canonical ICAO 9303 worked example, same fixture `mrz::dates`'
        // tests use.
        mrz::parse_td3(
            "P<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<",
            "L898902C36UTO7408122F1204159ZE184226B<<<<<10",
        )
        .expect("fixture is a valid ICAO 9303 specimen")
    }

    #[test]
    fn a_clean_document_is_accepted() {
        assert_eq!(check_line1_integrity(&base()), Verdict::Accepted);
    }

    #[test]
    fn an_unrecognized_issuing_country_is_flagged() {
        let mut m = base();
        "ZZZ".clone_into(&mut m.issuing_country);
        assert_eq!(
            check_line1_integrity(&m),
            Verdict::NeedsReview {
                reasons: vec![Finding::UnrecognizedIssuingCountry { got: "ZZZ".into() }]
            }
        );
    }

    #[test]
    fn issuing_country_disagreeing_with_a_valid_nationality_is_flagged() {
        let mut m = base();
        // UTO (Utopia) is a real specimen code, distinct from the fixture's
        // own UTO nationality, so this is a genuine mismatch, not a typo.
        "HRV".clone_into(&mut m.issuing_country);
        assert_eq!(
            check_line1_integrity(&m),
            Verdict::NeedsReview {
                reasons: vec![Finding::IssuingCountryNationalityMismatch {
                    issuing_country: "HRV".into(),
                    nationality: "UTO".into(),
                }]
            }
        );
    }

    /// Pinned to the actual corpus corruption
    /// (`P<JPNSTRAND<<ALEKSANDER<<<…` -> `PJPNSTRANDALEKSANDER<<<<…`): the
    /// `<<` separator is gone, so `clean_name` puts the whole line into
    /// `surname` and leaves `given_names` empty.
    #[test]
    fn the_collapsed_filler_run_corruption_is_flagged() {
        let mut m = base();
        "TRANDALEKSANDER".clone_into(&mut m.surname);
        String::new().clone_into(&mut m.given_names);
        assert_eq!(
            check_line1_integrity(&m),
            Verdict::NeedsReview {
                reasons: vec![Finding::MissingNameSeparator { surname_len: 15 }]
            }
        );
    }

    #[test]
    fn a_short_single_name_with_no_given_names_is_not_flagged() {
        // Real ICAO documents can legitimately have no given names (mononyms).
        // Only a *long* unsplit name is suspicious.
        let mut m = base();
        "CHER".clone_into(&mut m.surname);
        String::new().clone_into(&mut m.given_names);
        assert_eq!(check_line1_integrity(&m), Verdict::Accepted);
    }

    #[test]
    fn an_unrecognized_nationality_is_flagged() {
        let mut m = base();
        "ZZZ".clone_into(&mut m.nationality);
        assert_eq!(
            check_line1_integrity(&m),
            Verdict::NeedsReview {
                reasons: vec![Finding::UnrecognizedNationality { got: "ZZZ".into() }]
            }
        );
    }

    #[test]
    fn a_digit_in_surname_is_flagged() {
        let mut m = base();
        "ER1KSSON".clone_into(&mut m.surname);
        assert_eq!(
            check_line1_integrity(&m),
            Verdict::NeedsReview {
                reasons: vec![Finding::NonAlphabeticName {
                    field: "surname".to_string()
                }]
            }
        );
    }

    #[test]
    fn a_digit_in_given_names_is_flagged() {
        let mut m = base();
        "ANNA MAR1A".clone_into(&mut m.given_names);
        assert_eq!(
            check_line1_integrity(&m),
            Verdict::NeedsReview {
                reasons: vec![Finding::NonAlphabeticName {
                    field: "given_names".to_string()
                }]
            }
        );
    }

    /// The failure mode a naive filler-count/round-trip check would have hit
    /// (see the module doc comment): a wider-than-minimum internal gap
    /// between given names (parsed as extra spaces, since `clean_name`
    /// converts every interior `<` in the raw MRZ to one space each) must
    /// never be flagged by anything in this module.
    #[test]
    fn a_document_with_a_wide_internal_name_gap_is_not_flagged() {
        let mut m = base();
        "ANNA   MARIA".clone_into(&mut m.given_names);
        assert_eq!(check_line1_integrity(&m), Verdict::Accepted);
    }

    #[test]
    fn multiple_findings_are_all_reported() {
        let mut m = base();
        "ZZZ".clone_into(&mut m.issuing_country);
        "TRANDALEKSANDER".clone_into(&mut m.surname);
        String::new().clone_into(&mut m.given_names);
        assert_eq!(
            check_line1_integrity(&m),
            Verdict::NeedsReview {
                reasons: vec![
                    Finding::UnrecognizedIssuingCountry { got: "ZZZ".into() },
                    Finding::MissingNameSeparator { surname_len: 15 },
                ]
            }
        );
    }
}
