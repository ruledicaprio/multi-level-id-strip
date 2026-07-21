//! Measurement harness for Phase A of the line-1 integrity work
//! (`docs/document-layout-survey.jsonl`'s sibling): runs every specimen under
//! `samples/` through the OCR engine, reports per-extension decode/OCR
//! timing, and evaluates three **candidate** integrity findings that are not
//! yet promoted into `synthpass_core::fusion::check_line1_integrity`.
//!
//! This script exists to answer "does a candidate check ever fire, and if
//! so, on real corruption or a false positive" *before* it's added to
//! `fusion.rs` — see `docs/document-layout-survey.jsonl` and the plan this
//! implements. Nothing here changes production behavior.
//!
//! Run from the repo root (models auto-resolve there):
//! ```powershell
//! cargo run -p synthpass-ocr --release --example integrity_survey -- --batch 5
//! ```

use image::GenericImageView;
use mrz::{find_and_parse, Format, MrzData};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use synthpass_core::fusion::check_line1_integrity;
use synthpass_ocr::NativeOcr;

#[derive(Debug, Default)]
struct ExtStats {
    n: usize,
    total_ms: Vec<f64>,
    ms_per_mp: Vec<f64>,
}

/// Candidate findings not yet in `fusion.rs`. Evaluated here, over real OCR
/// output, purely to measure fire rate before promotion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Candidate {
    UnrecognizedNationality,
    NonAlphabeticName,
    NonCanonicalNameField,
}

impl Candidate {
    fn label(self) -> &'static str {
        match self {
            Self::UnrecognizedNationality => "unrecognized_nationality",
            Self::NonAlphabeticName => "non_alphabetic_name",
            Self::NonCanonicalNameField => "non_canonical_name_field",
        }
    }
}

/// Mirrors `mrz::emit`'s private `clean`/`field`/`name_field` exactly (see
/// `crates/mrz/src/emit.rs`), duplicated here because those helpers aren't
/// `pub` yet — Phase B exposes `name_field` for real, once a candidate earns
/// promotion. Kept in lockstep with the emitter deliberately: any mismatch
/// makes this survey lie about `NonCanonicalNameField`'s fire rate.
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| {
            let u = c.to_ascii_uppercase();
            if u.is_ascii_alphanumeric() {
                u
            } else {
                '<'
            }
        })
        .collect()
}

fn field(s: &str, width: usize) -> String {
    let cleaned = clean(s);
    let mut chars = cleaned.chars();
    let mut out = String::with_capacity(width);
    for _ in 0..width {
        out.push(chars.next().unwrap_or('<'));
    }
    out
}

fn canonical_name_field(surname: &str, given_names: &str) -> String {
    let combined = format!("{}<<{}", clean(surname), clean(given_names));
    field(&combined, 39)
}

/// Evaluates the three candidates against one parsed record. Returns the
/// subset that fired.
fn candidates_fired(m: &MrzData) -> Vec<Candidate> {
    let mut fired = Vec::new();

    if mrz::country_name(&m.nationality).is_none() {
        fired.push(Candidate::UnrecognizedNationality);
    }

    let name_has_digit = m
        .surname
        .chars()
        .chain(m.given_names.chars())
        .any(|c| c.is_ascii_digit());
    if name_has_digit {
        fired.push(Candidate::NonAlphabeticName);
    }

    if m.format == Format::Td3 {
        if let Some(line1) = m.mrz_lines.lines().next() {
            if line1.len() == 44 {
                let expected = canonical_name_field(&m.surname, &m.given_names);
                if line1[5..44] != expected {
                    if std::env::var("INTEGRITY_SURVEY_DEBUG").is_ok() {
                        eprintln!(
                            "DEBUG non_canonical_name_field: surname={:?} given_names={:?}\n  actual:   {:?}\n  expected: {:?}",
                            m.surname, m.given_names, &line1[5..44], expected
                        );
                    }
                    fired.push(Candidate::NonCanonicalNameField);
                }
            }
        }
    }

    fired
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn walk_samples(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_samples(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_else(|| "(none)".to_string())
}

fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * pct).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn median(sorted: &[f64]) -> f64 {
    percentile(sorted, 0.5)
}

fn main() {
    let batch_size: usize = std::env::args()
        .position(|a| a == "--batch")
        .and_then(|i| std::env::args().nth(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);

    let root = repo_root();
    let ocr = NativeOcr::load(
        &root.join("text-detection.rten"),
        &root.join("text-recognition.rten"),
    )
    .expect("failed to load OCR models — run from the repo root");

    let only: Option<String> = std::env::args()
        .position(|a| a == "--only")
        .and_then(|i| std::env::args().nth(i + 1));

    let mut files = Vec::new();
    walk_samples(&root.join("samples"), &mut files);
    files.sort();
    if let Some(substr) = &only {
        files.retain(|p| {
            p.file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|f| f.contains(substr.as_str()))
        });
    }

    let mut ext_stats: BTreeMap<String, ExtStats> = BTreeMap::new();
    let mut candidate_hits: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let mut jsonl_lines = Vec::new();

    let total = files.len();
    for (batch_idx, batch) in files.chunks(batch_size).enumerate() {
        println!(
            "\n=== batch {} ({}-{} of {total}) ===",
            batch_idx + 1,
            batch_idx * batch_size + 1,
            (batch_idx * batch_size + batch.len()).min(total)
        );

        for path in batch {
            let filename = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("?")
                .to_string();
            let ext = extension_of(path);

            let decode_start = Instant::now();
            let dims = image::open(path).ok().map(|img| img.dimensions());
            let decode_ms = decode_start.elapsed().as_secs_f64() * 1000.0;

            let Some((w, h)) = dims else {
                println!("SKIP  {filename} ({ext}): failed to decode image");
                continue;
            };
            let mp = (w as f64 * h as f64) / 1_000_000.0;

            let ocr_start = Instant::now();
            let text = match ocr.recognize(path) {
                Ok(t) => t,
                Err(e) => {
                    println!(
                        "SKIP  {filename} ({ext}, {w}x{h}, {mp:.2}MP): OCR error: {e} \
                         (decode {decode_ms:.1}ms)"
                    );
                    continue;
                }
            };
            let ocr_ms = ocr_start.elapsed().as_secs_f64() * 1000.0;
            let total_ms = decode_ms + ocr_ms;

            let stats = ext_stats.entry(ext.clone()).or_default();
            stats.n += 1;
            stats.total_ms.push(total_ms);
            if mp > 0.0 {
                stats.ms_per_mp.push(total_ms / mp);
            }

            let parsed = find_and_parse(&text);
            let (checksum_ok, verdict_str, fired) = match &parsed {
                Ok(m) => {
                    let verdict = check_line1_integrity(m);
                    let fired = candidates_fired(m);
                    for c in &fired {
                        candidate_hits
                            .entry(c.label())
                            .or_default()
                            .push(filename.clone());
                    }
                    (m.valid(), format!("{verdict:?}"), fired)
                }
                Err(_) => (false, "no_mrz".to_string(), Vec::new()),
            };

            let fired_labels: Vec<&str> = fired.iter().map(|c| c.label()).collect();
            println!(
                "{filename} ({ext}, {w}x{h}, {mp:.2}MP) decode {decode_ms:.1}ms ocr {ocr_ms:.1}ms \
                 total {total_ms:.1}ms mrz={} checksum_ok={checksum_ok} verdict={verdict_str} \
                 candidates={fired_labels:?}",
                parsed.is_ok(),
            );

            jsonl_lines.push(
                serde_json::json!({
                    "filename": filename,
                    "extension": ext,
                    "width": w,
                    "height": h,
                    "megapixels": mp,
                    "decode_ms": decode_ms,
                    "ocr_ms": ocr_ms,
                    "total_ms": total_ms,
                    "mrz_found": parsed.is_ok(),
                    "checksum_ok": checksum_ok,
                    "line1_verdict": verdict_str,
                    "candidates_fired": fired_labels,
                })
                .to_string(),
            );
        }
    }

    println!("\n=== per-extension timing ===");
    for (ext, stats) in &ext_stats {
        let mut sorted_total = stats.total_ms.clone();
        sorted_total.sort_by(|a, b| a.total_cmp(b));
        println!(
            "{ext:8} n={:<4} mean={:.1}ms median={:.1}ms p95={:.1}ms mean_ms_per_mp={:.1}",
            stats.n,
            mean(&sorted_total),
            median(&sorted_total),
            percentile(&sorted_total, 0.95),
            mean(&stats.ms_per_mp),
        );
    }

    println!("\n=== candidate finding fire rates ===");
    for label in [
        Candidate::UnrecognizedNationality.label(),
        Candidate::NonAlphabeticName.label(),
        Candidate::NonCanonicalNameField.label(),
    ] {
        match candidate_hits.get(label) {
            Some(files) => {
                println!("{label}: {} fired", files.len());
                for f in files {
                    println!("    {f}");
                }
            }
            None => println!("{label}: 0 fired"),
        }
    }

    let out_path = root.join("docs").join("integrity-survey.jsonl");
    if let Err(e) = std::fs::write(&out_path, jsonl_lines.join("\n") + "\n") {
        eprintln!("failed to write {}: {e}", out_path.display());
    } else {
        println!("\nwrote {}", out_path.display());
    }
}
