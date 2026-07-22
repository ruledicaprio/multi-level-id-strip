//! What a check digit provably *cannot* catch.
//!
//! An ICAO 9303 check digit (part 3 §4.9) is the 7-3-1 weighted sum of the
//! character values `0-9 → 0-9`, `A-Z → 10-35`, `< → 0`, taken **mod 10**.
//! Every weight is odd and coprime to nothing in particular — what matters is
//! the final `mod 10`. A single-character substitution therefore shifts the
//! sum by `weight × (value(a) - value(b))`, and that shift vanishes mod 10 for
//! *every* weight exactly when the two values are congruent mod 10.
//!
//! That gives a complete, closed-form law:
//!
//! > Two MRZ characters are indistinguishable to every check digit **iff**
//! > their ICAO values are congruent mod 10.
//!
//! The consequence inverts most people's intuition. The OCR confusions that
//! get worried about most — O↔0, I↔1, B↔8, S↔5, Z↔2 — are all **caught**,
//! because letters sit 10..35 and their digit lookalikes do not line up mod
//! 10. The quiet pairs K↔`<`, I↔S, B↔L and A↔K are **blind**, and no amount
//! of check-digit arithmetic will ever separate them.
//!
//! This is why the crate layers structural guards on top of the arithmetic
//! (recognized country codes, date plausibility, name charset): the oracle is
//! exact, and it is exact about its own edges too.
//!
//! ```
//! use mrz::{blindspot, Blindspot};
//!
//! // The classic OCR worry is not the dangerous one.
//! assert!(matches!(blindspot('O', '0'), Blindspot::Caught { .. }));
//! // This one is invisible to every check digit ever printed.
//! assert!(blindspot('K', '<').is_blind());
//! ```
//!
//! See `cargo run -p mrz --example checksum_blindspots` for the same law
//! demonstrated empirically against the real parser.

use crate::checksum::char_value;

/// Whether a check digit can distinguish two MRZ characters.
///
/// Obtain one from [`blindspot`]. `#[non_exhaustive]`: a future variant (e.g.
/// for a format whose check digit is not mod 10) must not be a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Blindspot {
    /// Same character — there is nothing to distinguish.
    Identical,
    /// Values differ mod 10: any check digit covering the position rejects
    /// the swap. `delta_mod10` is the (non-zero) shift it would induce.
    Caught { delta_mod10: u32 },
    /// Values are congruent mod 10: the swap is provably undetectable by
    /// check digits alone. `residue` is the shared value class.
    Blind { residue: u32 },
    /// One or both characters lie outside the ICAO 9303 MRZ alphabet.
    NotMrzCharset,
}

impl Blindspot {
    /// `true` only for [`Blindspot::Blind`] — a substitution no check digit
    /// can see. [`Identical`](Blindspot::Identical) is deliberately *not*
    /// blind: nothing was substituted.
    pub fn is_blind(self) -> bool {
        matches!(self, Self::Blind { .. })
    }
}

/// Classify a single-character substitution: can any ICAO check digit tell
/// `a` from `b`?
///
/// ```
/// use mrz::{blindspot, Blindspot};
///
/// assert_eq!(blindspot('A', 'A'), Blindspot::Identical);
/// assert_eq!(blindspot('A', 'K'), Blindspot::Blind { residue: 0 });
/// assert_eq!(blindspot('O', '0'), Blindspot::Caught { delta_mod10: 4 });
/// assert_eq!(blindspot('a', 'A'), Blindspot::NotMrzCharset);
/// ```
pub fn blindspot(a: char, b: char) -> Blindspot {
    let (va, vb) = match (char_value(a), char_value(b)) {
        (Ok(va), Ok(vb)) => (va, vb),
        _ => return Blindspot::NotMrzCharset,
    };
    if a == b {
        return Blindspot::Identical;
    }
    let delta = (va % 10 + 10 - vb % 10) % 10;
    if delta == 0 {
        Blindspot::Blind { residue: va % 10 }
    } else {
        Blindspot::Caught { delta_mod10: delta }
    }
}

/// The ten residue classes partitioning the 37-character MRZ alphabet:
/// `CLASSES[r]` lists every character whose ICAO value is ≡ `r` (mod 10).
///
/// This is the complete atlas of undetectable substitutions — any swap
/// *within* a row is invisible to every check digit, and any swap *across*
/// rows is caught. Note class 0 has five members because `<` and `0` share
/// the value 0.
pub const CLASSES: [&[char]; 10] = [
    &['0', 'A', 'K', 'U', '<'],
    &['1', 'B', 'L', 'V'],
    &['2', 'C', 'M', 'W'],
    &['3', 'D', 'N', 'X'],
    &['4', 'E', 'O', 'Y'],
    &['5', 'F', 'P', 'Z'],
    &['6', 'G', 'Q'],
    &['7', 'H', 'R'],
    &['8', 'I', 'S'],
    &['9', 'J', 'T'],
];

/// The residue class `c` belongs to, as a borrowed row of [`CLASSES`], or
/// `None` when `c` is outside the MRZ alphabet. Allocation-free.
pub fn class_of(c: char) -> Option<&'static [char]> {
    char_value(c).ok().map(|v| CLASSES[(v % 10) as usize])
}

/// Every MRZ character a check digit cannot distinguish from `c`, excluding
/// `c` itself. Empty when `c` is outside the alphabet.
///
/// ```
/// use mrz::collisions;
///
/// assert_eq!(collisions('K'), vec!['0', 'A', 'U', '<']);
/// assert!(collisions('@').is_empty());
/// ```
pub fn collisions(c: char) -> Vec<char> {
    class_of(c)
        .map(|class| class.iter().copied().filter(|&x| x != c).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHARSET: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ<";

    #[test]
    fn classes_partition_the_alphabet() {
        let mut seen: Vec<char> = Vec::new();
        for (r, class) in CLASSES.iter().enumerate() {
            for &c in *class {
                assert_eq!(
                    char_value(c).unwrap() % 10,
                    r as u32,
                    "{c:?} is not in class {r}"
                );
                assert!(!seen.contains(&c), "{c:?} appears in two classes");
                seen.push(c);
            }
        }
        assert_eq!(seen.len(), 37, "the MRZ alphabet has 37 characters");
        for c in CHARSET.chars() {
            assert!(seen.contains(&c), "{c:?} is in no class");
        }
    }

    #[test]
    fn identical_is_never_blind() {
        for c in CHARSET.chars() {
            assert_eq!(blindspot(c, c), Blindspot::Identical);
            assert!(!blindspot(c, c).is_blind());
        }
    }

    #[test]
    fn collisions_are_exactly_the_blind_set() {
        for c in CHARSET.chars() {
            let residue = char_value(c).unwrap() % 10;
            let coll = collisions(c);
            assert!(!coll.contains(&c));
            assert_eq!(coll.len(), CLASSES[residue as usize].len() - 1);
            for other in coll {
                assert_eq!(blindspot(c, other), Blindspot::Blind { residue });
                assert_eq!(blindspot(other, c), Blindspot::Blind { residue });
            }
            // Everything outside the class is caught.
            for other in CHARSET
                .chars()
                .filter(|&x| char_value(x).unwrap() % 10 != residue)
            {
                assert!(matches!(blindspot(c, other), Blindspot::Caught { .. }));
            }
        }
    }

    #[test]
    fn non_mrz_characters_are_rejected() {
        for c in ['@', 'a', 'é', ' ', '\0'] {
            assert_eq!(blindspot(c, 'A'), Blindspot::NotMrzCharset);
            assert_eq!(blindspot('A', c), Blindspot::NotMrzCharset);
            assert!(collisions(c).is_empty());
            assert_eq!(class_of(c), None);
        }
    }

    #[test]
    fn class_zero_holds_both_zero_and_filler() {
        assert_eq!(CLASSES[0], &['0', 'A', 'K', 'U', '<']);
        assert!(blindspot('K', '<').is_blind());
    }
}
