//! mlis (CLI) — command-line front-end for the multi-level-id-strip pipeline.
//!
//! Run from the repository root so the inferer sidecar and OCR engine resolve:
//!
//! ```powershell
//! cargo run -p mlis-cli -- samples/Croatian_passport_data_page.jpg
//! ```

use mlis_pipeline::Pipeline;
use std::env;
use std::io::Write;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage:");
        eprintln!("  cargo run -p mlis-cli -- <path_to_image_or_pdf>   extract");
        eprintln!("  cargo run -p mlis-cli -- decrypt <file.json.enc>  decrypt (needs MLIS_KEY)");
        return Ok(());
    }

    // `mlis decrypt <file>` — decrypt an AES-256-GCM payload to stdout.
    if args[1] == "decrypt" {
        return decrypt_command(args.get(2).map(String::as_str));
    }

    let input = Path::new(&args[1]);
    if !input.exists() {
        eprintln!("❌ Error: File not found at {}", input.display());
        return Ok(());
    }

    let pipeline = Pipeline::from_env();
    println!(
        "🔄 [Rust] Uploading and processing local file: {} (ocr: {})...",
        input.display(),
        pipeline.ocr_engine()
    );

    match pipeline.process_document(input).await {
        Ok(result) => {
            println!("✅ [Rust] OCR successful!");
            println!("💾 [Rust] Saved Markdown to: {}", result.md_path.display());
            match result.method {
                mlis_pipeline::Method::MrzDeterministic => {
                    println!("🔐 [Rust] ICAO 9303 checksums valid — deterministic MRZ extraction (LLM skipped)");
                    if let Some(extracted) = &result.extracted {
                        println!("{}", serde_json::to_string_pretty(extracted)?);
                    }
                }
                mlis_pipeline::Method::Llm => {
                    match &result.mrz {
                        Some(m) => println!(
                            "⚠️ [Rust] MRZ found but checksums failed ({:?}) — falling back to LLM",
                            m.checks
                        ),
                        None => println!("ℹ️ [Rust] No MRZ found — using LLM extraction"),
                    }
                    print!("{}", result.sidecar_stdout);
                }
            }
            match result.llm_error {
                None => println!(
                    "🎉 [Rust] Pipeline completed via {}! JSON saved to: {}",
                    result.method.as_str(),
                    result.json_path.display()
                ),
                Some(e) => eprintln!("⚠️ [Rust] LLM extraction failed: {e}"),
            }
        }
        Err(e) => eprintln!("❌ [Rust] {e}"),
    }

    Ok(())
}

/// `mlis decrypt <file.json.enc>` — decrypt an AES-256-GCM payload (written when
/// `MLIS_KEY` is set) to stdout, using the same `MLIS_KEY`.
fn decrypt_command(file: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let Some(file) = file else {
        eprintln!("Usage: mlis decrypt <file.json.enc>   (reads key from MLIS_KEY)");
        return Ok(());
    };
    let key = match env::var("MLIS_KEY") {
        Ok(s) => mlis_core::crypt::key_from_base64(&s)?,
        Err(_) => {
            eprintln!("❌ set MLIS_KEY (base64-encoded 32-byte AES-256 key)");
            return Ok(());
        }
    };
    let data = std::fs::read(file)?;
    match mlis_core::crypt::decrypt(&key, &data) {
        Ok(plain) => std::io::stdout().write_all(&plain)?,
        Err(e) => eprintln!("❌ decrypt failed: {e}"),
    }
    Ok(())
}
