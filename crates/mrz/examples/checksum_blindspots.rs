//! Map the honest boundary of the check-digit oracle: *what a valid MRZ
//! composite provably cannot catch.*
//!
//! The whole deterministic Tier-1 thesis is that a valid check digit is
//! mathematical proof of a faithful read — no probability involved. True, but
//! a check digit sees only the weighted sum of character *values mod 10*. Any
//! single-character substitution that preserves that sum is **invisible** to
//! it. This example doesn't reason about that abstractly — it mutates a known-
//! good ICAO specimen one character at a time and asks the *real* `parse_td3`
//! whether `valid()` still holds, so the blind spots are demonstrated, not
//! asserted.
//!
//! The surprise: the OCR confusions people worry about most (O↔0, I↔1, B↔8)
//! are *caught* by the checksum, while quieter same-value-class pairs
//! (K↔<, I↔S, B↔L) slip straight through. Knowing which is which is exactly
//! the kind of limit a validation library should be able to state out loud.
//!
//! Since 0.5.0 the law this example demonstrates is a first-class API —
//! `mrz::blindspot`, `mrz::collisions`, `mrz::CLASSES` — and sections 2 and 3
//! below call it rather than doing their own arithmetic. Section 1 still
//! mutates the specimen and asks the real parser, so the example doubles as a
//! live cross-check that the algebra agrees with the engine.
//!
//! Run: `cargo run -p mrz --example checksum_blindspots`

use mrz::{blindspot, parse_td3, Blindspot, CLASSES};

// Official ICAO 9303 Part 4 specimen (Utopia / Anna Maria Eriksson) — every
// check digit valid.
const L1: &str = "P<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<";
const L2: &str = "L898902C36UTO7408122F1204159ZE184226B<<<<<10";

const CHARSET: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ<";

/// Which of the ten residue classes `c` falls in, via the public table.
fn residue(c: char) -> usize {
    CLASSES
        .iter()
        .position(|class| class.contains(&c))
        .expect("MRZ alphabet character")
}

/// Substitute a single byte of line 2 and ask the real parser whether the
/// (unchanged) printed check digits still prove the read. `true` == the
/// checksum was blind to the swap.
fn checksum_blind_to(pos: usize, cand: char) -> bool {
    let mut bytes = L2.as_bytes().to_vec();
    bytes[pos] = cand as u8;
    match String::from_utf8(bytes) {
        Ok(mutated) => parse_td3(L1, &mutated).map(|d| d.valid()).unwrap_or(false),
        Err(_) => false,
    }
}

fn main() {
    let baseline = parse_td3(L1, L2).expect("specimen parses");
    assert!(baseline.valid(), "specimen must start valid");
    println!("baseline specimen: valid() = {}\n", baseline.valid());

    // 1) Sweep one data character across the whole MRZ alphabet.
    //    Position 0 of line 2 is the first char of the document number
    //    ('L'), covered by both the document-number and composite digits.
    let pos = 0;
    let orig = L2.as_bytes()[pos] as char;
    println!(
        "=== single-character sweep at line2[{pos}] (document number, printed '{orig}', class ≡{}) ===",
        residue(orig)
    );
    let mut blind = Vec::new();
    let mut caught = 0;
    for cand in CHARSET.chars() {
        if cand == orig {
            continue;
        }
        if checksum_blind_to(pos, cand) {
            blind.push(cand);
        } else {
            caught += 1;
        }
    }
    println!(
        "  BLIND  (checksum cannot distinguish from '{orig}'): {}",
        blind
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join("  ")
    );
    println!("  CAUGHT (checksum flips valid()→false):        {caught} of the other 35 chars");
    // The empirical blind set the parser just produced must be exactly what
    // the public API predicts — the whole point of promoting it to an API.
    assert_eq!(
        blind,
        mrz::collisions(orig),
        "the parser and mrz::collisions must agree"
    );
    println!(
        "  → every BLIND char has value ≡ {} (mod 10), same as '{orig}'. That is the entire\n    \
         blind set, and it is exactly one residue class — nothing more slips through.\n    \
         mrz::collisions('{orig}') returns precisely this set.\n",
        residue(orig)
    );

    // 2) The counterintuitive part: real-world OCR confusion pairs, classified
    //    by the same value-mod-10 law section 1 just demonstrated against the
    //    engine. A pair is invisible to *every* check digit iff its two
    //    characters share a value residue mod 10.
    println!("=== common OCR confusions: caught or blind? ===");
    let pairs = [
        ('O', '0', "the classic — surprisingly CAUGHT"),
        ('I', '1', "CAUGHT"),
        ('B', '8', "CAUGHT"),
        ('S', '5', "CAUGHT"),
        ('Z', '2', "CAUGHT"),
        ('K', '<', "the one the parser documents — BLIND"),
        ('I', 'S', "quiet BLIND spot"),
        ('B', 'L', "quiet BLIND spot"),
        ('A', 'K', "BLIND"),
    ];
    println!("  pair      Δ mod10  verdict   note");
    for (a, b, note) in pairs {
        let (delta, verdict) = match blindspot(a, b) {
            Blindspot::Blind { .. } => (0, "BLIND"),
            Blindspot::Caught { delta_mod10 } => (delta_mod10, "caught"),
            _ => unreachable!("all pairs are distinct MRZ characters"),
        };
        println!("  {a} ↔ {b}      {delta:<8} {verdict:<9} {note}");
    }

    // 3) The full residue-class partition of the MRZ alphabet — the complete
    //    atlas of what a check digit can never separate.
    println!("\n=== the 10 collision classes (a check digit sees only value mod 10) ===");
    for (r, class) in CLASSES.iter().enumerate() {
        let members: Vec<String> = class.iter().map(|c| c.to_string()).collect();
        println!("  ≡{r}: {}", members.join(" "));
    }
    println!(
        "\n  Any single-char misread WITHIN a class is provably undetectable by check digits\n  \
         alone — which is precisely why this crate layers structural guards on top (recognized\n  \
         country codes, date plausibility, name charset) and hands the rest to Tier-2. The\n  \
         oracle is honest about its own edges."
    );
}
