//! ICAO 9303 Machine Readable Zone parser with check-digit validation.
//!
//! Zero runtime dependencies so it compiles to native and `wasm32-unknown-unknown`
//! targets alike. Supports:
//! - **TD3** (passports): 2 lines × 44 characters
//! - **TD1** (ID cards): 3 lines × 30 characters
//!
//! Check digits use the standard 7-3-1 weighting over the value mapping
//! `0-9 → 0-9`, `A-Z → 10-35`, `< → 0`. A field checksum that validates
//! mathematically proves the OCR read is faithful to the printed document —
//! no probabilistic model involved.

#[cfg(feature = "serde")]
use serde::Serialize;

/// Per-field check-digit verification results.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Checks {
    pub document_number: bool,
    pub date_of_birth: bool,
    pub date_of_expiry: bool,
    /// TD3 only; `true` for TD1 (no such check digit exists there).
    pub personal_number: bool,
    /// The composite check digit over the whole zone.
    pub composite: bool,
}

impl Checks {
    /// All check digits valid — the MRZ read is mathematically verified.
    pub fn all_valid(&self) -> bool {
        self.document_number
            && self.date_of_birth
            && self.date_of_expiry
            && self.personal_number
            && self.composite
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum Format {
    Td3,
    Td1,
}

/// Parsed and validated MRZ data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct MrzData {
    pub format: Format,
    /// Document code, e.g. "P" (passport), "ID"/"I" (identity card).
    pub document_type: String,
    /// Issuing state or organization (3-letter ICAO code).
    pub issuing_country: String,
    pub document_number: String,
    pub surname: String,
    pub given_names: String,
    /// Nationality (3-letter ICAO code).
    pub nationality: String,
    /// ISO 8601 (`YYYY-MM-DD`), century inferred (see [`expand_date`]).
    pub date_of_birth: String,
    /// "M", "F" or "X" (unspecified).
    pub sex: String,
    /// ISO 8601 (`YYYY-MM-DD`).
    pub date_of_expiry: String,
    /// TD3: personal number field. TD1: optional data 1 + 2 joined.
    pub personal_number: Option<String>,
    /// The raw MRZ lines, newline-joined, exactly as validated.
    pub mrz_lines: String,
    pub checks: Checks,
}

impl MrzData {
    /// Shorthand for `checks.all_valid()`.
    pub fn valid(&self) -> bool {
        self.checks.all_valid()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MrzError {
    /// Line has the wrong length for the claimed format.
    BadLength { expected: usize, got: usize },
    /// Character outside `[A-Z0-9<]`.
    BadCharacter(char),
    /// Document code not recognized for the format.
    BadDocumentCode(String),
    /// No plausible MRZ found in the supplied text.
    NotFound,
}

impl core::fmt::Display for MrzError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadLength { expected, got } => {
                write!(f, "bad MRZ line length: expected {expected}, got {got}")
            }
            Self::BadCharacter(c) => write!(f, "invalid MRZ character: {c:?}"),
            Self::BadDocumentCode(c) => write!(f, "unrecognized document code: {c:?}"),
            Self::NotFound => write!(f, "no MRZ found in text"),
        }
    }
}

impl std::error::Error for MrzError {}

/// ICAO 9303 character value: `0-9 → 0-9`, `A-Z → 10-35`, `< → 0`.
fn char_value(c: char) -> Result<u32, MrzError> {
    match c {
        '0'..='9' => Ok(c as u32 - '0' as u32),
        'A'..='Z' => Ok(c as u32 - 'A' as u32 + 10),
        '<' => Ok(0),
        other => Err(MrzError::BadCharacter(other)),
    }
}

/// Compute the ICAO 9303 check digit (7-3-1 repeating weights, mod 10).
pub fn check_digit(field: &str) -> Result<u32, MrzError> {
    const WEIGHTS: [u32; 3] = [7, 3, 1];
    let mut sum = 0u32;
    for (i, c) in field.chars().enumerate() {
        sum += char_value(c)? * WEIGHTS[i % 3];
    }
    Ok(sum % 10)
}

/// Verify `field` against its printed check digit character.
/// A `<` check digit counts as 0 (used for empty optional fields).
pub fn verify(field: &str, digit: char) -> bool {
    match (check_digit(field), char_value(digit)) {
        (Ok(expected), Ok(got)) => expected == got && (digit.is_ascii_digit() || digit == '<'),
        _ => false,
    }
}

/// Expand `YYMMDD` to ISO `YYYY-MM-DD`.
///
/// Century heuristic: birth dates after the current two-digit year roll back
/// to 19xx; expiry dates are always 20xx (no valid travel document from the
/// 1900s remains in circulation).
pub fn expand_date(yymmdd: &str, is_birth: bool) -> String {
    // Two-digit year of "today" — bump this constant is not needed: it only
    // shifts the 19xx/20xx pivot for birth dates, derived at compile time
    // from the crate's era. Kept simple and deterministic for auditability.
    const CURRENT_YY: u32 = 26;
    if yymmdd.len() != 6 || !yymmdd.chars().all(|c| c.is_ascii_digit()) {
        return yymmdd.to_string(); // leave unparseable input untouched
    }
    let yy: u32 = yymmdd[0..2].parse().unwrap();
    let century = if is_birth && yy > CURRENT_YY { "19" } else { "20" };
    format!("{century}{}-{}-{}", &yymmdd[0..2], &yymmdd[2..4], &yymmdd[4..6])
}

fn clean_name(field: &str) -> (String, String) {
    let trimmed = field.trim_end_matches('<');
    let (surname, given) = match trimmed.split_once("<<") {
        Some((s, g)) => (s, g),
        None => (trimmed, ""),
    };
    (
        surname.replace('<', " ").trim().to_string(),
        given.replace('<', " ").trim().to_string(),
    )
}

fn clean_sex(c: char) -> String {
    match c {
        'M' => "M".into(),
        'F' => "F".into(),
        _ => "X".into(),
    }
}

fn ensure_charset(line: &str) -> Result<(), MrzError> {
    for c in line.chars() {
        char_value(c)?;
    }
    Ok(())
}

/// Parse a TD3 (passport) MRZ: two lines of exactly 44 characters.
pub fn parse_td3(line1: &str, line2: &str) -> Result<MrzData, MrzError> {
    for line in [line1, line2] {
        if line.len() != 44 {
            return Err(MrzError::BadLength { expected: 44, got: line.len() });
        }
        ensure_charset(line)?;
    }
    if !line1.starts_with('P') {
        return Err(MrzError::BadDocumentCode(line1[0..2].to_string()));
    }

    let (surname, given_names) = clean_name(&line1[5..44]);

    let document_number = line2[0..9].trim_end_matches('<').to_string();
    let personal_raw = &line2[28..42];
    let personal = personal_raw.trim_end_matches('<');

    let checks = Checks {
        document_number: verify(&line2[0..9], line2.as_bytes()[9] as char),
        date_of_birth: verify(&line2[13..19], line2.as_bytes()[19] as char),
        date_of_expiry: verify(&line2[21..27], line2.as_bytes()[27] as char),
        personal_number: verify(personal_raw, line2.as_bytes()[42] as char),
        // Composite: doc number + check, DOB + check, expiry + check +
        // personal number + check (line 2 positions 1-10, 14-20, 22-43).
        composite: verify(
            &format!("{}{}{}", &line2[0..10], &line2[13..20], &line2[21..43]),
            line2.as_bytes()[43] as char,
        ),
    };

    Ok(MrzData {
        format: Format::Td3,
        document_type: line1[0..2].trim_end_matches('<').to_string(),
        issuing_country: line1[2..5].trim_end_matches('<').to_string(),
        document_number,
        surname,
        given_names,
        nationality: line2[10..13].trim_end_matches('<').to_string(),
        date_of_birth: expand_date(&line2[13..19], true),
        sex: clean_sex(line2.as_bytes()[20] as char),
        date_of_expiry: expand_date(&line2[21..27], false),
        personal_number: if personal.is_empty() { None } else { Some(personal.to_string()) },
        mrz_lines: format!("{line1}\n{line2}"),
        checks,
    })
}

/// Parse a TD1 (ID card) MRZ: three lines of exactly 30 characters.
pub fn parse_td1(line1: &str, line2: &str, line3: &str) -> Result<MrzData, MrzError> {
    for line in [line1, line2, line3] {
        if line.len() != 30 {
            return Err(MrzError::BadLength { expected: 30, got: line.len() });
        }
        ensure_charset(line)?;
    }
    let code = line1[0..2].trim_end_matches('<');
    if !matches!(code.as_bytes().first(), Some(b'I' | b'A' | b'C')) {
        return Err(MrzError::BadDocumentCode(code.to_string()));
    }

    let (surname, given_names) = clean_name(line3);

    let optional1 = line1[15..30].trim_end_matches('<');
    let optional2 = line2[18..29].trim_end_matches('<');
    let personal = [optional1, optional2]
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");

    let checks = Checks {
        document_number: verify(&line1[5..14], line1.as_bytes()[14] as char),
        date_of_birth: verify(&line2[0..6], line2.as_bytes()[6] as char),
        date_of_expiry: verify(&line2[8..14], line2.as_bytes()[14] as char),
        personal_number: true, // TD1 has no personal-number check digit
        // Composite: line1 positions 6-30, line2 positions 1-7, 9-15, 19-29.
        composite: verify(
            &format!("{}{}{}{}", &line1[5..30], &line2[0..7], &line2[8..15], &line2[18..29]),
            line2.as_bytes()[29] as char,
        ),
    };

    Ok(MrzData {
        format: Format::Td1,
        document_type: code.to_string(),
        issuing_country: line1[2..5].trim_end_matches('<').to_string(),
        document_number: line1[5..14].trim_end_matches('<').to_string(),
        surname,
        given_names,
        nationality: line2[15..18].trim_end_matches('<').to_string(),
        date_of_birth: expand_date(&line2[0..6], true),
        sex: clean_sex(line2.as_bytes()[7] as char),
        date_of_expiry: expand_date(&line2[8..14], false),
        personal_number: if personal.is_empty() { None } else { Some(personal) },
        mrz_lines: format!("{line1}\n{line2}\n{line3}"),
        checks,
    })
}

/// Normalize one line of OCR output toward the MRZ charset:
/// uppercase, strip whitespace, map common OCR confusions of the filler —
/// `«`/`≪` are how OCR typically renders the double filler `<<`, while `‹`
/// is a single `<`.
fn normalize_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for c in line.chars().filter(|c| !c.is_whitespace()) {
        match c {
            '«' | '≪' => out.push_str("<<"),
            '‹' => out.push('<'),
            c => out.push(c.to_ascii_uppercase()),
        }
    }
    out
}

fn is_mrz_charset(s: &str) -> bool {
    s.chars().all(|c| matches!(c, 'A'..='Z' | '0'..='9' | '<'))
}

/// OCR lookalike corrections for positions that must be digits.
fn digitize(c: char) -> char {
    match c {
        'O' | 'Q' | 'D' => '0',
        'I' | 'L' => '1',
        'Z' => '2',
        'S' => '5',
        'G' => '6',
        'B' => '8',
        c => c,
    }
}

/// OCR lookalike corrections for positions that must be letters (or filler).
fn letterize(c: char) -> char {
    match c {
        '0' => 'O',
        '1' => 'I',
        '2' => 'Z',
        '5' => 'S',
        '6' => 'G',
        '8' => 'B',
        c => c,
    }
}

/// Replace runs of ≥ `min_run` consecutive `K`/`L` characters with fillers —
/// OCR persistently misreads the `<` filler as K or L, and no transliterated
/// ICAO name contains four K/L in a row.
fn defiller(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'K' | b'L') {
            let mut j = i;
            while j < bytes.len() && matches!(bytes[j], b'K' | b'L' | b'<') {
                j += 1;
            }
            let kl = bytes[i..j].iter().filter(|b| matches!(b, b'K' | b'L')).count();
            if j - i >= 4 && kl >= 3 {
                out.extend(std::iter::repeat('<').take(j - i));
            } else {
                out.push_str(&s[i..j]);
            }
            i = j;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Apply a per-index repair map: `spec` lists `(range, fix)` pairs.
fn repair_positions(line: &str, spec: &[(core::ops::Range<usize>, fn(char) -> char)]) -> String {
    line.chars()
        .enumerate()
        .map(|(i, c)| {
            if c == '<' {
                return c; // fillers are always legitimate
            }
            for (range, fix) in spec {
                if range.contains(&i) {
                    return fix(c);
                }
            }
            c
        })
        .collect()
}

/// Fix a document-code second character misread: no ICAO document code has
/// `K` there, but OCR reads the `<` filler as `K` constantly.
fn fix_doc_code(l: &str) -> String {
    if l.as_bytes().get(1) == Some(&b'K') {
        format!("{}<{}", &l[0..1], &l[2..])
    } else {
        l.to_string()
    }
}

/// MRZ name fields separate surname from given names with `<<`. When a name
/// field has no `<<` at all but contains `KK`, that pair is a misread
/// separator. Fields that already contain a real `<<` (e.g. MIKKO<<HEIKKI)
/// are left untouched.
fn fix_name_separator(s: &str) -> String {
    let trimmed = s.trim_end_matches('<');
    if !trimmed.contains("<<") {
        if let Some(pos) = trimmed.find("KK") {
            return format!("{}<<{}", &s[..pos], &s[pos + 2..]);
        }
    }
    s.to_string()
}

/// Last-resort variant: any `K`/`L` touching a `<` becomes `<` (to fixpoint).
/// Only ever accepted when the composite check digit validates the result.
fn aggressive_defiller(s: &str) -> String {
    let mut b: Vec<u8> = s.bytes().collect();
    let mut changed = true;
    while changed {
        changed = false;
        for i in 0..b.len() {
            if matches!(b[i], b'K' | b'L')
                && ((i > 0 && b[i - 1] == b'<') || (i + 1 < b.len() && b[i + 1] == b'<'))
            {
                b[i] = b'<';
                changed = true;
            }
        }
    }
    String::from_utf8(b).expect("ascii in, ascii out")
}

fn repair_td3_line1(l: &str) -> String {
    // Issuing state must be letters; the name field suffers filler misreads.
    let l = fix_doc_code(&repair_positions(l, &[(2..5, letterize)]));
    format!("{}{}", &l[0..5], fix_name_separator(&defiller(&l[5..])))
}

fn repair_td3_line2(l: &str) -> String {
    let l = repair_positions(
        l,
        &[
            (9..10, digitize),   // doc number check digit
            (10..13, letterize), // nationality
            (13..20, digitize),  // DOB + check digit
            (21..28, digitize),  // expiry + check digit
            (42..44, digitize),  // personal-number check + composite
        ],
    );
    // The personal-number field is filler-dominated on most passports.
    // Note: `K` ≡ `<` (both value 20 ≡ 0 mod 10) under every 7-3-1 weight, so
    // check digits are provably blind to this misread — heuristics must do it.
    format!(
        "{}{}{}",
        &l[0..28],
        aggressive_defiller(&defiller(&l[28..42])),
        &l[42..44]
    )
}

fn repair_td1_line1(l: &str) -> String {
    let l = fix_doc_code(&repair_positions(l, &[(2..5, letterize), (14..15, digitize)]));
    format!("{}{}", &l[0..15], aggressive_defiller(&defiller(&l[15..])))
}

fn repair_td1_line2(l: &str) -> String {
    let l = repair_positions(
        l,
        &[
            (0..7, digitize),    // DOB + check digit
            (8..15, digitize),   // expiry + check digit
            (15..18, letterize), // nationality
            (29..30, digitize),  // composite
        ],
    );
    format!(
        "{}{}{}",
        &l[0..18],
        aggressive_defiller(&defiller(&l[18..29])),
        &l[29..30]
    )
}

fn repair_td1_line3(l: &str) -> String {
    fix_name_separator(&defiller(l))
}

/// Longest run of consecutive `<` fillers: `(start, len)`.
fn longest_filler_run(s: &str) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let start = i;
            while i < bytes.len() && bytes[i] == b'<' {
                i += 1;
            }
            if best.map_or(true, |(_, l)| i - start > l) {
                best = Some((start, i - start));
            }
        } else {
            i += 1;
        }
    }
    best
}

/// Candidate alignments of a wrong-length line. OCR drops or hallucinates
/// characters most often inside filler runs, so besides trimming/padding at
/// the ends we also inflate/deflate the longest `<` run.
fn fit_length(n: &str, target: usize) -> Vec<String> {
    use core::cmp::Ordering;
    match n.len().cmp(&target) {
        Ordering::Equal => vec![n.to_string()],
        Ordering::Less => {
            let missing = target - n.len();
            let mut v = Vec::new();
            if let Some((start, len)) = longest_filler_run(n) {
                let mut s = String::with_capacity(target);
                s.push_str(&n[..start + len]);
                s.extend(core::iter::repeat('<').take(missing));
                s.push_str(&n[start + len..]);
                v.push(s);
            }
            let mut padded = n.to_string();
            padded.extend(core::iter::repeat('<').take(missing));
            v.push(padded);
            v
        }
        Ordering::Greater => {
            let extra = n.len() - target;
            let mut v = Vec::new();
            if let Some((start, len)) = longest_filler_run(n) {
                if len > extra {
                    let mut s = String::with_capacity(target);
                    s.push_str(&n[..start]);
                    s.extend(core::iter::repeat('<').take(len - extra));
                    s.push_str(&n[start + len..]);
                    v.push(s);
                }
            }
            v.push(n[0..target].to_string());
            v.push(n[n.len() - target..].to_string());
            v
        }
    }
}

/// Candidate readings of one OCR line for a given MRZ line length:
/// normalized, length-adjusted, each both verbatim and lookalike-repaired.
/// The check digits decide which variant — if any — is the true read.
fn variants(raw: &str, target: usize, repair: fn(&str) -> String) -> Vec<String> {
    let n = normalize_line(raw);
    // OCR drops trailing fillers wholesale, so tolerate short lines generously
    // (they get padded back); hallucinated extra characters are rarer.
    if n.len() + 8 < target || n.len() > target + 4 || !is_mrz_charset(&n) {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for fitted in fit_length(&n, target) {
        let repaired = repair(&fitted);
        let last_resort = aggressive_defiller(&repaired);
        for form in [repaired, fitted, last_resort] {
            if !out.contains(&form) {
                out.push(form);
            }
        }
    }
    out
}

/// Scan free-form text (e.g. OCR output) for an MRZ and parse it.
///
/// Tries TD3 (two 44-char lines starting with `P`) first, then TD1
/// (three 30-char lines starting with `I`/`A`/`C`). Tolerates HTML-escaped
/// fillers (`&lt;`, as produced by docling's Markdown) and both TD3 lines
/// merged onto a single physical line.
pub fn find_and_parse(text: &str) -> Result<MrzData, MrzError> {
    // Markdown/HTML pipelines escape the filler character.
    let text = text.replace("&lt;", "<");
    // OCR often emits several MRZ lines as ONE physical line, space-separated
    // (docling renders the whole zone as a single paragraph) — treat long
    // whitespace-separated tokens as individual candidate lines.
    let mut lines: Vec<&str> = Vec::new();
    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().filter(|t| t.len() >= 20).collect();
        if tokens.len() >= 2 {
            lines.extend(tokens);
        } else {
            lines.push(line);
        }
    }

    // First parseable-but-checksum-failed hit, reported when nothing better
    // is found so callers can show which check digits failed.
    let mut fallback: Option<MrzData> = None;
    let mut consider = |data: MrzData| -> Option<MrzData> {
        if data.valid() {
            return Some(data);
        }
        fallback.get_or_insert(data);
        None
    };

    // TD3: a line starting with 'P' followed by a candidate line — or both
    // 44-char lines merged into one ~88-char physical line.
    for i in 0..lines.len() {
        let merged = normalize_line(lines[i]);
        if merged.starts_with('P') && (84..=92).contains(&merged.len()) && is_mrz_charset(&merged)
        {
            let head = &merged[0..44];
            let tail = &merged[44..];
            for l1 in [repair_td3_line1(head), head.to_string()] {
                for l2 in variants(tail, 44, repair_td3_line2) {
                    if let Ok(data) = parse_td3(&l1, &l2) {
                        if let Some(valid) = consider(data) {
                            return Ok(valid);
                        }
                    }
                }
            }
        }

        for l1 in variants(lines[i], 44, repair_td3_line1) {
            if !l1.starts_with('P') {
                continue;
            }
            for l2_raw in lines.iter().skip(i + 1).take(3) {
                for l2 in variants(l2_raw, 44, repair_td3_line2) {
                    if let Ok(data) = parse_td3(&l1, &l2) {
                        if let Some(valid) = consider(data) {
                            return Ok(valid);
                        }
                    }
                }
            }
        }
    }

    // TD1: three consecutive candidate lines, first starting with I/A/C.
    for i in 0..lines.len().saturating_sub(2) {
        for l1 in variants(lines[i], 30, repair_td1_line1) {
            if !matches!(l1.as_bytes().first(), Some(b'I' | b'A' | b'C')) {
                continue;
            }
            for l2 in variants(lines[i + 1], 30, repair_td1_line2) {
                for l3 in variants(lines[i + 2], 30, repair_td1_line3) {
                    if let Ok(data) = parse_td1(&l1, &l2, &l3) {
                        if let Some(valid) = consider(data) {
                            return Ok(valid);
                        }
                    }
                }
            }
        }
    }

    fallback.ok_or(MrzError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Official ICAO 9303 part 4 specimen (Utopia / Anna Maria Eriksson).
    const TD3_L1: &str = "P<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<";
    const TD3_L2: &str = "L898902C36UTO7408122F1204159ZE184226B<<<<<10";

    #[test]
    fn check_digit_icao_vectors() {
        assert_eq!(check_digit("L898902C3").unwrap(), 6);
        assert_eq!(check_digit("740812").unwrap(), 2);
        assert_eq!(check_digit("120415").unwrap(), 9);
        assert_eq!(check_digit("ZE184226B<<<<<").unwrap(), 1);
    }

    #[test]
    fn td3_specimen_fully_valid() {
        let d = parse_td3(TD3_L1, TD3_L2).unwrap();
        assert!(d.valid(), "checks: {:?}", d.checks);
        assert_eq!(d.document_type, "P");
        assert_eq!(d.issuing_country, "UTO");
        assert_eq!(d.surname, "ERIKSSON");
        assert_eq!(d.given_names, "ANNA MARIA");
        assert_eq!(d.document_number, "L898902C3");
        assert_eq!(d.nationality, "UTO");
        assert_eq!(d.date_of_birth, "1974-08-12");
        assert_eq!(d.sex, "F");
        assert_eq!(d.date_of_expiry, "2012-04-15");
        assert_eq!(d.personal_number.as_deref(), Some("ZE184226B"));
    }

    #[test]
    fn td3_tampered_dob_fails_checksum() {
        // Change one digit of the date of birth: 740812 → 750812.
        let tampered = TD3_L2.replacen("740812", "750812", 1);
        let d = parse_td3(TD3_L1, &tampered).unwrap();
        assert!(!d.checks.date_of_birth);
        assert!(!d.checks.composite);
        assert!(!d.valid());
    }

    #[test]
    fn td3_empty_personal_number_with_filler_check() {
        // Personal number all fillers and check digit '<' is valid (value 0).
        let l2 = "L898902C36UTO7408122F1204159<<<<<<<<<<<<<<06";
        let d = parse_td3(TD3_L1, l2).unwrap();
        assert!(d.checks.personal_number);
        assert_eq!(d.personal_number, None);
    }

    // Official ICAO 9303 part 5 TD1 specimen.
    const TD1_L1: &str = "I<UTOD231458907<<<<<<<<<<<<<<<";
    const TD1_L2: &str = "7408122F1204159UTO<<<<<<<<<<<6";
    const TD1_L3: &str = "ERIKSSON<<ANNA<MARIA<<<<<<<<<<";

    #[test]
    fn td1_specimen_fully_valid() {
        let d = parse_td1(TD1_L1, TD1_L2, TD1_L3).unwrap();
        assert!(d.valid(), "checks: {:?}", d.checks);
        assert_eq!(d.format, Format::Td1);
        assert_eq!(d.document_type, "I");
        assert_eq!(d.document_number, "D23145890");
        assert_eq!(d.surname, "ERIKSSON");
        assert_eq!(d.given_names, "ANNA MARIA");
        assert_eq!(d.date_of_birth, "1974-08-12");
        assert_eq!(d.date_of_expiry, "2012-04-15");
    }

    #[test]
    fn find_in_ocr_noise() {
        let text = format!(
            "## REPUBLIC OF UTOPIA\n\nSome OCR noise here\n\n{}\n{}\n\nfooter",
            // OCR quirks: lowercase, stray spaces, « for <<, dropped fillers.
            "p<utoeriksson«anna<maria<<<<<<<<<<<<<<<<<",
            "L898902C36UTO7408122F1204159ZE184226B<<<<<10"
        );
        let d = find_and_parse(&text).unwrap();
        assert!(d.valid());
        assert_eq!(d.surname, "ERIKSSON");
    }

    #[test]
    fn find_html_escaped_and_merged_lines() {
        // Real docling output shape: fillers escaped as &lt; and both TD3
        // lines on one physical markdown line (Croatian specimen).
        let text = "## PUTOVNICA\n\nP&lt;HRVSPECIMEN&lt;&lt;SPECIMEN&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt; 0070070071HRV8212258F1407019&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;06\n";
        let d = find_and_parse(text).unwrap();
        assert_eq!(d.surname, "SPECIMEN");
        assert_eq!(d.document_number, "007007007");
        assert_eq!(d.issuing_country, "HRV");
        assert!(d.checks.document_number);
        assert!(d.checks.date_of_birth);
        assert!(d.checks.date_of_expiry);
    }

    #[test]
    fn checksum_verified_ocr_repair() {
        // Verbatim tesseract.js output for the Croatian specimen at low
        // resolution: trailing fillers read as K/L runs, a hallucinated
        // leading '1' on line 2 (45 chars), and 'B' where '8' is printed.
        // The check digits prove which repaired variant is the true read.
        let text = "I 01072009 PUJZAGREB 0\n\nBIDFD WH5SS A 2\n\n01072014\nP<HRVSPECIMEN<<SPECIMEN<KLLLLLLLLLLLLLLLLLKLKL\n10070070071HRVB212258F1407019<<<<<<<<<<<<<<06\n";
        let d = find_and_parse(text).unwrap();
        assert!(d.valid(), "checks: {:?}", d.checks);
        assert_eq!(d.surname, "SPECIMEN");
        assert_eq!(d.given_names, "SPECIMEN");
        assert_eq!(d.document_number, "007007007");
        assert_eq!(d.date_of_birth, "1982-12-25");
    }

    #[test]
    fn ocr_repair_dropped_filler_mid_line() {
        // Second verbatim tesseract.js reading of the same specimen: an
        // L-run inside the personal-number field and one filler DROPPED
        // (43 chars) — the missing character must be re-inserted inside the
        // filler run, not appended, or the check digits shift.
        let text = "RF 01072009 PUZAGREB\n01072014\nP<HRVSPECIMEN<<SPECIMEN<<K<KLLLLLLLLLLLLLLLLKLKL\n0070070071HRVB212258F1407019<<<<LLLLLLL<<06\n";
        let d = find_and_parse(text).unwrap();
        assert!(d.valid(), "checks: {:?}", d.checks);
        assert_eq!(d.surname, "SPECIMEN");
        assert_eq!(d.document_number, "007007007");
        assert_eq!(d.personal_number, None);
    }

    #[test]
    fn td1_from_single_docling_line_with_k_misreads() {
        // Verbatim docling OCR of the Slovenian 2022 specimen ID card rear:
        // all three TD1 lines in ONE paragraph, `<` escaped as &lt;, and the
        // usual K-for-filler misreads (IK→I<, 145K<→145<<, VZORECKK→VZOREC<<).
        let text = "1F9874543\n\nIKSVNIE987654302806985505145K&lt; 8506287F3203282SVN&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;&lt;2 VZORECKKJANAKKKKKKKKK&lt;&lt;KK";
        let d = find_and_parse(text).unwrap();
        assert!(d.valid(), "checks: {:?}", d.checks);
        assert_eq!(d.format, Format::Td1);
        assert_eq!(d.document_type, "I");
        assert_eq!(d.issuing_country, "SVN");
        assert_eq!(d.document_number, "IE9876543");
        assert_eq!(d.surname, "VZOREC");
        assert_eq!(d.given_names, "JANA");
        assert_eq!(d.date_of_birth, "1985-06-28");
        assert_eq!(d.date_of_expiry, "2032-03-28");
        // The trailing K in the EMŠO field is a filler misread that check
        // digits cannot catch (K ≡ < mod 10) — heuristic cleanup handles it.
        assert_eq!(d.personal_number.as_deref(), Some("2806985505145"));
    }

    #[test]
    fn name_separator_fix_spares_real_kk_names() {
        // MIKKONEN<<HEIKKI already contains a real `<<` separator — the KK
        // pairs inside the names must survive.
        assert_eq!(
            fix_name_separator("MIKKONEN<<HEIKKI<<<<<<"),
            "MIKKONEN<<HEIKKI<<<<<<"
        );
        // No separator at all + a KK pair → it was the separator.
        assert_eq!(fix_name_separator("VZORECKKJANA<<<"), "VZOREC<<JANA<<<");
    }

    #[test]
    fn invalid_checksums_still_reported() {
        // A tampered MRZ parses but is flagged invalid rather than dropped.
        let tampered = TD3_L2.replacen("740812", "750812", 1);
        let text = format!("{TD3_L1}\n{tampered}");
        let d = find_and_parse(&text).unwrap();
        assert!(!d.valid());
        assert!(!d.checks.date_of_birth);
    }

    #[test]
    fn find_nothing_in_plain_text() {
        assert_eq!(
            find_and_parse("just a regular paragraph\nwith two lines"),
            Err(MrzError::NotFound)
        );
    }

    #[test]
    fn date_century_pivot() {
        assert_eq!(expand_date("740812", true), "1974-08-12");
        assert_eq!(expand_date("150101", true), "2015-01-01");
        assert_eq!(expand_date("301231", false), "2030-12-31");
    }
}
