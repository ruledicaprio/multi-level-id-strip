//! The blindspot API's algebra must agree with the actual parser.
//!
//! `src/blindspot.rs` carries the unit tests that need `checksum::char_value`
//! (crate-private). These are the tests that only need the public surface —
//! above all the empirical sweep, which mutates the ICAO 9303 specimen one
//! character at a time and asks the *real* `parse_td3` whether `valid()` still
//! holds. That proves the mod-10 law matches the engine rather than restating
//! it.

use mrz::{blindspot, collisions, parse_td3, Blindspot, CLASSES};

// Official ICAO 9303 part 4 specimen (Utopia / Anna Maria Eriksson).
const L1: &str = "P<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<";
const L2: &str = "L898902C36UTO7408122F1204159ZE184226B<<<<<10";

const CHARSET: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ<";

/// Substitute one character of line 2 and ask the parser whether the printed
/// check digits still prove the read. `true` == the checksum was blind.
fn parser_blind_to(pos: usize, cand: char) -> bool {
    let mut bytes = L2.as_bytes().to_vec();
    bytes[pos] = cand as u8;
    let mutated = String::from_utf8(bytes).expect("ascii in, ascii out");
    parse_td3(L1, &mutated).map(|d| d.valid()).unwrap_or(false)
}

/// TD3 line-2 *data* positions covered by a check digit, from the part 4
/// §4.2.2 field layout: document number `0..9`, date of birth `13..19`,
/// date of expiry `21..27`, optional (personal number) data `28..42`.
///
/// The check-digit characters themselves (9, 19, 27, 42, 43) are deliberately
/// excluded — see `check_digit_positions_are_not_pure_arithmetic` below.
fn data_positions() -> Vec<usize> {
    (0..9).chain(13..19).chain(21..27).chain(28..42).collect()
}

#[test]
fn specimen_starts_valid() {
    assert!(parse_td3(L1, L2).unwrap().valid());
}

#[test]
fn blindspot_agrees_with_the_real_parser() {
    for pos in data_positions() {
        let orig = L2.as_bytes()[pos] as char;
        for cand in CHARSET.chars() {
            if cand == orig {
                continue;
            }
            let predicted = blindspot(orig, cand).is_blind();
            let observed = parser_blind_to(pos, cand);
            assert_eq!(
                predicted, observed,
                "disagreement at line2[{pos}]: '{orig}' → '{cand}' \
                 (blindspot says blind={predicted}, parser says valid={observed})"
            );
        }
    }
}

#[test]
fn collisions_match_the_parser_at_every_data_position() {
    for pos in data_positions() {
        let orig = L2.as_bytes()[pos] as char;
        let observed: Vec<char> = CHARSET
            .chars()
            .filter(|&c| c != orig && parser_blind_to(pos, c))
            .collect();
        assert_eq!(observed, collisions(orig), "at line2[{pos}]");
    }
}

/// A check digit *position* is not governed by the mod-10 law alone: ICAO
/// requires the printed check digit to be a digit (or `<` for an empty
/// optional field), and `mrz::verify` enforces that. So `'9'` → `'J'` is
/// algebraically blind but still rejected by the parser. This documents the
/// carve-out that `data_positions()` excludes, so the exclusion above reads as
/// a deliberate boundary rather than a convenient omission.
#[test]
fn check_digit_positions_are_not_pure_arithmetic() {
    let pos = 9; // document-number check digit, printed '6'
    let orig = L2.as_bytes()[pos] as char;
    assert_eq!(orig, '6');
    let same_class = 'G'; // value 16 ≡ 6 (mod 10)
    assert!(blindspot(orig, same_class).is_blind());
    assert!(
        !parser_blind_to(pos, same_class),
        "a letter in a check-digit position must be rejected despite the residue match"
    );
}

#[test]
fn common_ocr_pairs_classified_correctly() {
    // The README's table, encoded as assertions. The pairs people worry about
    // are caught; the quiet ones are not.
    for (a, b) in [('O', '0'), ('I', '1'), ('B', '8'), ('S', '5'), ('Z', '2')] {
        assert!(
            matches!(blindspot(a, b), Blindspot::Caught { .. }),
            "{a} ↔ {b} should be caught"
        );
    }
    for (a, b) in [('K', '<'), ('I', 'S'), ('B', 'L'), ('A', 'K')] {
        assert!(blindspot(a, b).is_blind(), "{a} ↔ {b} should be blind");
    }
}

#[test]
fn classes_cover_the_alphabet_exactly_once() {
    let total: usize = CLASSES.iter().map(|c| c.len()).sum();
    assert_eq!(total, 37);
    for c in CHARSET.chars() {
        let hits = CLASSES.iter().filter(|class| class.contains(&c)).count();
        assert_eq!(hits, 1, "{c:?} appears in {hits} classes");
    }
}
