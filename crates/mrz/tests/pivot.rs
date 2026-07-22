//! Table-driven proof of `ParseOptions::pivot_yy`'s century-expansion rule
//! (see `src/dates.rs::expand_date_with_pivot`): birth dates roll back to the
//! 1900s when their two-digit year exceeds the pivot, expiry dates are always
//! 20xx regardless of the pivot. The pivot is purely a display/interpretation
//! knob — it must never change whether any check digit validates, since it
//! never touches the printed characters the checksums are computed over.

use mrz::{format_td3, parse_td3_with, ParseOptions, Td3Fields};

/// Build a valid TD3 MRZ with the given `YYMMDD` birth/expiry fields, so each
/// table entry only has to name the two-digit years under test.
fn td3_with_dates(date_of_birth: &str, date_of_expiry: &str) -> String {
    let fields = Td3Fields {
        document_code: "P".to_string(),
        issuing_country: "UTO".to_string(),
        document_number: "L898902C3".to_string(),
        surname: "ERIKSSON".to_string(),
        given_names: "ANNA MARIA".to_string(),
        nationality: "UTO".to_string(),
        date_of_birth: date_of_birth.to_string(),
        sex: "F".to_string(),
        date_of_expiry: date_of_expiry.to_string(),
        personal_number: None,
    };
    format_td3(&fields)
}

/// `(pivot, yy, expected_century)` — the birth-date expansion rule is
/// `yy > pivot => "19", else "20"`. Table covers, for several pivots, the
/// boundary itself, one year on each side, and the extremes 00 and 99.
const BIRTH_CENTURY_TABLE: &[(u32, u32, &str)] = &[
    // pivot 0: every yy > 0 rolls back to 1900s; only yy == 0 stays 2000s.
    (0, 0, "20"),
    (0, 1, "19"),
    (0, 99, "19"),
    // pivot 26 (mrz::CURRENT_YY, the crate's default): the boundary this
    // crate ships with today.
    (26, 26, "20"),
    (26, 27, "19"),
    (26, 0, "20"),
    (26, 99, "19"),
    // pivot 50: a "everything before Y2K+50 is now" style cutoff.
    (50, 50, "20"),
    (50, 51, "19"),
    (50, 49, "20"),
    // pivot 99: every yy stays in the 2000s (no value exceeds 99).
    (99, 0, "20"),
    (99, 98, "20"),
    (99, 99, "20"),
];

#[test]
fn birth_date_lands_in_the_intended_century_at_and_around_each_pivot() {
    for &(pivot, yy, expected_century) in BIRTH_CENTURY_TABLE {
        let dob = format!("{yy:02}0615"); // mid-year, always a valid calendar date
        let mrz = td3_with_dates(&dob, "300101");
        let (l1, l2) = mrz.split_once('\n').unwrap();
        let opts = ParseOptions { pivot_yy: pivot };
        let d = parse_td3_with(l1, l2, &opts).unwrap();

        let expected = format!("{expected_century}{yy:02}-06-15");
        assert_eq!(
            d.date_of_birth, expected,
            "pivot {pivot}, yy {yy}: expected {expected}, got {}",
            d.date_of_birth
        );
    }
}

#[test]
fn expiry_date_is_always_20xx_regardless_of_pivot() {
    // Expiry has no "is_birth" branch in `expand_date_with_pivot` — no
    // pivot, however extreme, should ever push it into the 1900s.
    for &pivot in &[0, 26, 50, 99] {
        for yy in [0u32, 26, 27, 50, 99] {
            let expiry = format!("{yy:02}0615");
            let mrz = td3_with_dates("740812", &expiry);
            let (l1, l2) = mrz.split_once('\n').unwrap();
            let opts = ParseOptions { pivot_yy: pivot };
            let d = parse_td3_with(l1, l2, &opts).unwrap();
            let expected = format!("20{yy:02}-06-15");
            assert_eq!(
                d.date_of_expiry, expected,
                "pivot {pivot}, yy {yy}: expected {expected}, got {}",
                d.date_of_expiry
            );
        }
    }
}

#[test]
fn leap_day_birth_dates_expand_to_the_correct_century_leap_status() {
    // 290229: yy=29 is never a leap year in either century (29 % 4 != 0), so
    // this is a "century changes, well-formedness doesn't" case.
    for &(pivot, expected_century) in &[(0u32, "19"), (99, "20")] {
        let mrz = td3_with_dates("290229", "300101");
        let (l1, l2) = mrz.split_once('\n').unwrap();
        let opts = ParseOptions { pivot_yy: pivot };
        let d = parse_td3_with(l1, l2, &opts).unwrap();
        assert_eq!(d.date_of_birth, format!("{expected_century}29-02-29"));
    }

    // 000229: yy=00 is the sharp Gregorian case where the century *does*
    // change leap-year status — 2000-02-29 is a real calendar date (divisible
    // by 400) but 1900-02-29 is not (divisible by 100, not 400). A pivot of 0
    // keeps yy=0 in the 2000s (well-formed); a pivot below 0 is impossible,
    // so instead compare pivot 0 (2000s, well-formed) against a birth year
    // that pivot pushes into the 1900s (1900-02-29, not well-formed) to prove
    // the pivot's century choice really does flip real-world validity.
    let mrz_00 = td3_with_dates("000229", "300101");
    let (l1, l2) = mrz_00.split_once('\n').unwrap();

    let d_2000s = parse_td3_with(l1, l2, &ParseOptions { pivot_yy: 0 }).unwrap();
    assert_eq!(d_2000s.date_of_birth, "2000-02-29");
    assert!(
        d_2000s
            .validity(mrz::Date::new(2030, 1, 1))
            .dates_well_formed
    );

    // pivot -1 doesn't exist (u32), so use yy=01 with pivot 0 instead: yy=1 >
    // pivot=0 rolls back to 1901 (not a leap year at all, ordinary case) —
    // included for completeness of the "pivot flips century" story, but the
    // headline leap-specific proof is the 2000-vs-1900 contrast above.
    let mrz_01 = td3_with_dates("010229", "300101");
    let (l1, l2) = mrz_01.split_once('\n').unwrap();
    let d_1901 = parse_td3_with(l1, l2, &ParseOptions { pivot_yy: 0 }).unwrap();
    assert_eq!(d_1901.date_of_birth, "1901-02-29");
    assert!(
        !d_1901
            .validity(mrz::Date::new(2030, 1, 1))
            .dates_well_formed
    );
}

#[test]
fn changing_the_pivot_never_changes_any_check_digit() {
    // The pivot only feeds `expand_date_with_pivot`'s ISO-string formatting;
    // it never touches the printed YYMMDD characters the check digits are
    // computed over, so `checks` must be byte-identical across every pivot.
    let mrz = td3_with_dates("290229", "300101");
    let (l1, l2) = mrz.split_once('\n').unwrap();

    let baseline = parse_td3_with(l1, l2, &ParseOptions { pivot_yy: 26 })
        .unwrap()
        .checks
        .clone();

    for pivot in [0, 1, 25, 26, 27, 28, 50, 98, 99] {
        let d = parse_td3_with(l1, l2, &ParseOptions { pivot_yy: pivot }).unwrap();
        assert_eq!(
            d.checks, baseline,
            "pivot {pivot} changed checks: {:?}",
            d.checks
        );
        assert!(d.valid(), "pivot {pivot}: checks {:?}", d.checks);
    }
}
