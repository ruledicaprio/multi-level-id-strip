//! Shared document-processing pipeline:
//! OCR → Markdown on disk → **Tier 1** deterministic ICAO 9303 MRZ validation
//! → **Tier 2** LLM fallback (see [`InferBackend`]) → structured JSON.
//!
//! Tier 1 (the [`mrz`] crate) mathematically verifies every MRZ check digit.
//! When the composite checksum validates, the extraction is provably faithful
//! to the printed document and the probabilistic LLM step is skipped entirely.
//! The LLM only runs for documents without a valid MRZ (damaged scans,
//! non-standard documents, technical manuals).
//!
//! Tier 2 is a pluggable [`InferBackend`]: [`NativeInferer`] (feature
//! `inferer-native`, default, and the only backend as of v0.7.5) runs the
//! Qwen GGUF in-process via `mlis-llm`, staying warm for the process
//! lifetime. See `infer::backend_from_env` for how it's constructed.
//!
//! Both binaries (`mlis` CLI and `mlis-serve` web server) are thin
//! wrappers around [`Pipeline::process_document`].

pub use mrz;

mod infer;
mod ocr;
pub use infer::InferBackend;
#[cfg(feature = "inferer-native")]
pub use infer::NativeInferer;
pub use ocr::OcrEngine;
#[cfg(feature = "ocr-native-rust")]
pub use ocr::RustOcrEngine;

use mlis_core::Extraction;
use serde_json::Value;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};
use zeroize::Zeroizing;

pub struct Pipeline {
    /// The OCR engine (pure-Rust `ocrs`/`rten` — the only engine since
    /// v1.2.0).
    ocr: Box<dyn OcrEngine>,
    /// Tier-2 inference backend (native llama.cpp, in-process).
    infer: Box<dyn InferBackend>,
    /// Tier 3: when set, append a PII-free audit record per processed document.
    audit_log: Option<PathBuf>,
    /// Tier 3: when set, encrypt the output JSON (AES-256-GCM) to `.json.enc`.
    /// `Zeroizing` wipes the key from memory when the `Pipeline` is dropped.
    encrypt_key: Option<Zeroizing<[u8; 32]>>,
    /// Consumer GPUs (e.g. GTX 970, 3.5 GB VRAM) fit exactly one GGUF model
    /// instance — LLM inference is serialized so concurrent callers queue
    /// instead of racing the same `llama.cpp` context (also enforced,
    /// defense-in-depth, inside `mlis-llm`'s `NativeLlm` generation lock).
    /// One permit = one concurrent Tier-2 call.
    llm_semaphore: Arc<Semaphore>,
    /// Requests currently queued or in flight against the inferer. Lets a
    /// caller (e.g. `mlis-serve`) reject new work fast under overload instead
    /// of accepting it unboundedly and blocking. Incremented just before
    /// queuing for `llm_semaphore`, decremented when the call completes.
    llm_queue_depth: Arc<AtomicUsize>,
}

/// Bumps `llm_queue_depth` for the lifetime of the guard — from just before
/// queuing for the semaphore permit until the Tier-2 call fully completes.
struct QueueDepthGuard<'a>(&'a AtomicUsize);

impl<'a> QueueDepthGuard<'a> {
    fn enter(counter: &'a AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}

impl Drop for QueueDepthGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub enum PipelineError {
    /// The OCR engine was unreachable or the conversion failed outright.
    Ocr(String),
    /// Conversion succeeded but returned no Markdown content.
    NoMarkdown(String),
    Io(std::io::Error),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ocr(e) => write!(f, "OCR failed: {e}"),
            Self::NoMarkdown(e) => write!(f, "no markdown returned from OCR: {e}"),
            Self::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for PipelineError {}

impl From<std::io::Error> for PipelineError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Which extraction tier produced the final JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Tier 1: ICAO 9303 MRZ with all check digits valid — deterministic,
    /// LLM skipped.
    MrzDeterministic,
    /// Tier 2: LLM sidecar (no MRZ found, or its checksums failed).
    Llm,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MrzDeterministic => "mrz-deterministic",
            Self::Llm => "llm",
        }
    }
}

pub struct PipelineResult {
    /// OCR output (Markdown/plain text) from the active OCR engine.
    pub markdown: String,
    /// Where the Markdown was written (`<input>.md`).
    pub md_path: PathBuf,
    /// Where the extracted JSON was written (`<input>.json`).
    pub json_path: PathBuf,
    /// Parsed extraction JSON (Tier 1 or Tier 2); `None` when Tier 2 failed.
    pub extracted: Option<Value>,
    /// Tier 2 failure description. OCR `markdown` is still valid when set.
    pub llm_error: Option<String>,
    /// Diagnostic notes from the Tier 2 step (e.g. a persist warning); usually
    /// empty now that the inferer streams no stdout.
    pub sidecar_stdout: String,
    /// Parsed MRZ when one was found in the OCR output — present even when
    /// its checksums failed (see `mrz.checks` for per-field results).
    pub mrz: Option<mrz::MrzData>,
    /// Which tier produced `extracted`.
    pub method: Method,
}

/// Progress/terminal events emitted by [`Pipeline::process_document_stream`].
pub enum ProcessEvent {
    /// Incremental Tier-2 LLM text. Never sent on the Tier-1 fast path.
    Delta(String),
    /// Terminal: the full pipeline result, same shape as
    /// [`Pipeline::process_document`]'s return value.
    Done(Box<PipelineResult>),
    /// Terminal: an OCR-stage failure (mirrors [`PipelineError`]).
    Failed(String),
}

impl Pipeline {
    /// Construct with an explicit OCR engine and Tier-2 inference backend.
    /// Tier-3 security (audit log, encryption) is off; enable it via
    /// [`from_env`] or the `with_*` builders.
    ///
    /// [`from_env`]: Pipeline::from_env
    pub fn new(ocr: Box<dyn OcrEngine>, infer: Box<dyn InferBackend>) -> Self {
        Self {
            ocr,
            infer,
            audit_log: None,
            encrypt_key: None,
            llm_semaphore: Arc::new(Semaphore::new(1)),
            llm_queue_depth: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Append a PII-free audit record (SHA-256 fingerprint + metadata) per
    /// document to `path`.
    pub fn with_audit_log(mut self, path: impl Into<PathBuf>) -> Self {
        self.audit_log = Some(path.into());
        self
    }

    /// Encrypt the output JSON with the given AES-256 key.
    pub fn with_encrypt_key(mut self, key: [u8; 32]) -> Self {
        self.encrypt_key = Some(Zeroizing::new(key));
        self
    }

    /// Configure from the environment: `MLIS_OCR_ENGINE` (`rust` | `native`);
    /// `MLIS_MODEL_PATH` / `MLIS_MODEL_N_CTX` for the Tier-2 model; and
    /// Tier-3 `MLIS_AUDIT_LOG` / `MLIS_KEY` (base64 32-byte AES-256 key).
    pub fn from_env() -> Self {
        let mut pipeline = Self::new(ocr::engine_from_env(), infer::backend_from_env());
        pipeline.audit_log = std::env::var("MLIS_AUDIT_LOG").ok().map(PathBuf::from);
        pipeline.encrypt_key = match std::env::var("MLIS_KEY") {
            Ok(s) => match mlis_core::crypt::key_from_base64(&s) {
                Ok(key) => Some(key),
                Err(e) => {
                    eprintln!("[mlis] ignoring MLIS_KEY: {e}");
                    None
                }
            },
            Err(_) => None,
        };
        pipeline
    }

    /// Human-readable description of the active OCR engine.
    pub fn ocr_engine(&self) -> String {
        self.ocr.describe()
    }

    /// Human-readable description of the active Tier-2 inference backend.
    pub fn infer_describe(&self) -> String {
        self.infer.describe()
    }

    /// Preflight check for `mlis doctor`: `Ok(status)` on success, `Err(reason)`
    /// otherwise.
    pub async fn infer_health(&self) -> Result<String, String> {
        self.infer.health().await
    }

    /// Tier-2 requests currently queued or in flight against the inferer.
    /// Compare against a configured cap to reject new work fast instead of
    /// accepting it unboundedly.
    pub fn llm_queue_depth(&self) -> usize {
        self.llm_queue_depth.load(Ordering::Relaxed)
    }

    /// Tier 2: call the active inference backend and get back the canonical
    /// [`Extraction`] schema. Serialized behind `llm_semaphore` so this
    /// process never fires overlapping Tier-2 calls, regardless of backend.
    pub async fn extract_via_inferer(&self, markdown: &str) -> Result<Extraction, String> {
        let _depth = QueueDepthGuard::enter(&self.llm_queue_depth);
        let _permit = self
            .llm_semaphore
            .acquire()
            .await
            .expect("llm_semaphore is never closed");
        self.infer.extract(markdown).await
    }

    /// Stage 1-2: OCR the input, write `<input>.md`, and check for a
    /// checksum-valid MRZ (Tier 1). Shared by [`process_document`] and
    /// [`process_document_stream`] — the two differ only in how they handle a
    /// Tier-1 miss (unary vs. streaming Tier-2 fallback).
    ///
    /// [`process_document`]: Pipeline::process_document
    /// [`process_document_stream`]: Pipeline::process_document_stream
    async fn ocr_and_tier1(
        &self,
        input: &Path,
    ) -> Result<(String, PathBuf, Option<mrz::MrzData>, Option<Value>), PipelineError> {
        let markdown = self.ocr.to_markdown(input).await?;

        let md_path = input.with_extension("md");
        tokio::fs::write(&md_path, &markdown).await?;

        // Tier 1: deterministic ICAO 9303 MRZ validation. A valid composite
        // checksum mathematically proves the read — no LLM needed.
        let mrz_data = mrz::find_and_parse(&markdown).ok();
        let tier1 = mrz_data
            .as_ref()
            .filter(|m| m.valid())
            .map(|m| serde_json::to_value(extraction_from_mrz(m)).expect("Extraction serializes"));
        Ok((markdown, md_path, mrz_data, tier1))
    }

    /// Run the full pipeline on a local image/PDF. Writes `<input>.md` and
    /// `<input>.json` next to the input file.
    ///
    /// An OCR failure is an `Err`; an LLM-inferer failure is *not* — it
    /// degrades to `llm_error: Some(..)` with the OCR Markdown still returned.
    pub async fn process_document(&self, input: &Path) -> Result<PipelineResult, PipelineError> {
        let (markdown, md_path, mrz_data, tier1) = self.ocr_and_tier1(input).await?;
        let mut json_path = md_path.with_extension("json");

        // Tier 2: semantic JSON extraction via the in-process LLM inferer,
        // used only when Tier 1 found no checksum-valid MRZ.
        let (extracted, method, llm_error) = if let Some(value) = tier1 {
            (Some(value), Method::MrzDeterministic, None)
        } else {
            match self.extract_via_inferer(&markdown).await {
                Ok(extraction) => {
                    let value = serde_json::to_value(&extraction).expect("Extraction serializes");
                    (Some(value), Method::Llm, None)
                }
                Err(e) => (None, Method::Llm, Some(e)),
            }
        };

        // Persist (encrypting if a key is configured) + append a PII-free audit
        // record. A persistence failure never invalidates the in-memory result.
        let mut sidecar_stdout = String::new();
        if let Some(value) = &extracted {
            match self
                .write_outputs(input, &json_path, value, method, mrz_data.as_ref())
                .await
            {
                Ok(written) => json_path = written,
                Err(e) => sidecar_stdout = format!("warning: could not persist output: {e}"),
            }
        }

        Ok(PipelineResult {
            markdown,
            md_path,
            json_path,
            extracted,
            llm_error,
            sidecar_stdout,
            mrz: mrz_data,
            method,
        })
    }

    /// Streaming variant of [`process_document`] for callers (`mlis-serve`)
    /// that want to render Tier-2 LLM progress instead of freezing during the
    /// wait. Emits `Delta` events as the model generates, then exactly one
    /// terminal `Done` or `Failed` event on `tx`.
    ///
    /// [`process_document`]: Pipeline::process_document
    pub async fn process_document_stream(&self, input: &Path, tx: mpsc::Sender<ProcessEvent>) {
        let (markdown, md_path, mrz_data, tier1) = match self.ocr_and_tier1(input).await {
            Ok(v) => v,
            Err(e) => {
                let _ = tx.send(ProcessEvent::Failed(e.to_string())).await;
                return;
            }
        };
        let mut json_path = md_path.with_extension("json");

        let (extracted, method, llm_error) = if let Some(value) = tier1 {
            (Some(value), Method::MrzDeterministic, None)
        } else {
            match self.extract_via_inferer_stream(&markdown, &tx).await {
                Ok(extraction) => {
                    let value = serde_json::to_value(&extraction).expect("Extraction serializes");
                    (Some(value), Method::Llm, None)
                }
                Err(e) => (None, Method::Llm, Some(e)),
            }
        };

        let mut sidecar_stdout = String::new();
        if let Some(value) = &extracted {
            match self
                .write_outputs(input, &json_path, value, method, mrz_data.as_ref())
                .await
            {
                Ok(written) => json_path = written,
                Err(e) => sidecar_stdout = format!("warning: could not persist output: {e}"),
            }
        }

        let _ = tx
            .send(ProcessEvent::Done(Box::new(PipelineResult {
                markdown,
                md_path,
                json_path,
                extracted,
                llm_error,
                sidecar_stdout,
                mrz: mrz_data,
                method,
            })))
            .await;
    }

    /// Tier 2 streaming: like [`extract_via_inferer`] but forwards incremental
    /// text on `tx` as [`ProcessEvent::Delta`] while the model generates.
    ///
    /// [`extract_via_inferer`]: Pipeline::extract_via_inferer
    async fn extract_via_inferer_stream(
        &self,
        markdown: &str,
        tx: &mpsc::Sender<ProcessEvent>,
    ) -> Result<Extraction, String> {
        let _depth = QueueDepthGuard::enter(&self.llm_queue_depth);
        let _permit = self
            .llm_semaphore
            .acquire()
            .await
            .expect("llm_semaphore is never closed");
        self.infer.extract_stream(markdown, tx).await
    }

    /// Write the extraction JSON (encrypted to `<input>.json.enc` when a key is
    /// set, else plaintext `<input>.json`) and append an audit record when a log
    /// is configured. Returns the path actually written.
    async fn write_outputs(
        &self,
        input: &Path,
        json_path: &Path,
        value: &Value,
        method: Method,
        mrz: Option<&mrz::MrzData>,
    ) -> std::io::Result<PathBuf> {
        let pretty = Zeroizing::new(serde_json::to_string_pretty(value).expect("Value serializes"));

        let written = if let Some(key) = &self.encrypt_key {
            let blob =
                mlis_core::crypt::encrypt(key, pretty.as_bytes()).map_err(std::io::Error::other)?;
            let enc_path = json_path.with_extension("json.enc");
            tokio::fs::write(&enc_path, blob).await?;
            enc_path
        } else {
            tokio::fs::write(json_path, pretty.as_bytes()).await?;
            json_path.to_path_buf()
        };

        if let Some(log) = &self.audit_log {
            // Fingerprint the input bytes; never log the PII itself.
            let input_bytes = tokio::fs::read(input).await.unwrap_or_default();
            let record = mlis_core::audit::AuditRecord::new(
                mlis_core::audit::sha256_hex(&input_bytes),
                method.as_str(),
                mrz.map(|m| m.valid()),
                mrz.map(|m| m.document_type.clone()),
            );
            let _ = mlis_core::audit::append(log, &record);
        }

        Ok(written)
    }
}

/// The current UTC date, for date-plausibility checks. Derived from the system
/// clock with pure arithmetic (via [`mrz::Date::from_epoch_days`]) so the `mrz`
/// crate itself stays clock-free.
fn today() -> mrz::Date {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    mrz::Date::from_epoch_days((secs / 86_400) as i64)
}

/// Map validated MRZ data onto the canonical [`Extraction`] schema — the same
/// shape Tier 2 and the WASM demo produce. Enriches with the resolved country
/// names and a date-plausibility summary (checksum-valid does not imply
/// in-date — see [`mrz::MrzData::validity`]).
fn extraction_from_mrz(m: &mrz::MrzData) -> Extraction {
    let v = m.validity(today());
    Extraction {
        document_type: Some(m.document_type.clone()),
        issuing_country: Some(m.issuing_country.clone()),
        issuing_country_name: m.issuing_country_name().map(str::to_string),
        document_number: Some(m.document_number.clone()),
        surname: Some(m.surname.clone()),
        given_names: Some(m.given_names.clone()),
        nationality: Some(m.nationality.clone()),
        nationality_name: m.nationality_name().map(str::to_string),
        date_of_birth: Some(m.date_of_birth.clone()),
        sex: Some(m.sex.clone()),
        date_of_expiry: Some(m.date_of_expiry.clone()),
        personal_number: m.personal_number.clone(),
        mrz_line: Some(m.mrz_lines.clone()),
        mrz_checksums_valid: Some(true),
        validity: Some(mlis_core::Validity {
            dates_well_formed: v.dates_well_formed,
            in_date: v.in_date,
            dob_before_expiry: v.dob_before_expiry,
            days_until_expiry: v.days_until_expiry,
        }),
        extraction_method: Method::MrzDeterministic.as_str().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal `OcrEngine` used only to satisfy `Pipeline::new`'s type in
    /// tests that exercise Tier 2 directly — its `to_markdown` is never called.
    struct NoopOcr;

    #[async_trait::async_trait]
    impl OcrEngine for NoopOcr {
        async fn to_markdown(&self, _input: &Path) -> Result<String, PipelineError> {
            unreachable!("these tests call extract_via_inferer directly, not to_markdown")
        }
        fn describe(&self) -> String {
            "noop".into()
        }
    }

    /// A mock Tier-2 backend, replacing the pre-v0.7.5 gRPC mock server —
    /// `Pipeline`'s queue/stream behavior is backend-agnostic by design (the
    /// whole point of [`InferBackend`]), so a plain in-process trait impl
    /// exercises it just as well as a real network round-trip did.
    struct MockBackend;

    #[async_trait::async_trait]
    impl InferBackend for MockBackend {
        async fn extract(&self, markdown: &str) -> Result<Extraction, String> {
            Ok(mock_extraction(markdown))
        }

        async fn extract_stream(
            &self,
            markdown: &str,
            tx: &mpsc::Sender<ProcessEvent>,
        ) -> Result<Extraction, String> {
            let _ = tx.try_send(ProcessEvent::Delta("mock-delta".into()));
            Ok(mock_extraction(markdown))
        }

        fn describe(&self) -> String {
            "mock".into()
        }

        async fn health(&self) -> Result<String, String> {
            Ok("mock healthy".into())
        }
    }

    fn mock_extraction(markdown: &str) -> Extraction {
        // Struct-update syntax (`..Extraction::default()`) can't be used
        // here: `Extraction` implements `Drop` (via `ZeroizeOnDrop`), and
        // Rust disallows partial moves out of a base value of a `Drop` type.
        let mut e = Extraction::default();
        e.document_type = Some("P".into());
        e.surname = Some("DOE".into());
        e.given_names = Some("JOHN".into());
        e.document_number = Some("X1234567".into());
        e.mrz_line = Some(markdown.chars().take(4).collect());
        e.extraction_method = Method::Llm.as_str().to_string();
        e
    }

    fn mock_pipeline() -> Pipeline {
        Pipeline::new(Box::new(NoopOcr), Box::new(MockBackend))
    }

    #[tokio::test]
    async fn tier2_returns_backend_extraction() {
        let pipeline = mock_pipeline();

        let e = pipeline
            .extract_via_inferer("P<UTO passport markdown")
            .await
            .expect("inferer extract");

        assert_eq!(e.surname.as_deref(), Some("DOE"));
        assert_eq!(e.given_names.as_deref(), Some("JOHN"));
        assert_eq!(e.document_number.as_deref(), Some("X1234567"));
        assert_eq!(e.mrz_line.as_deref(), Some("P<UT")); // proves markdown reached the backend
        assert_eq!(e.extraction_method, "llm");
    }

    #[tokio::test]
    async fn extract_via_inferer_stream_forwards_deltas_and_result() {
        let pipeline = mock_pipeline();

        let (tx, mut rx) = mpsc::channel(16);
        let extraction = pipeline
            .extract_via_inferer_stream("P<UTO passport markdown", &tx)
            .await
            .expect("inferer extract_stream");
        drop(tx);

        let mut deltas = Vec::new();
        while let Some(event) = rx.recv().await {
            match event {
                ProcessEvent::Delta(d) => deltas.push(d),
                _ => panic!("extract_via_inferer_stream should only send Delta events"),
            }
        }

        assert_eq!(deltas, vec!["mock-delta".to_string()]);
        assert_eq!(extraction.surname.as_deref(), Some("DOE"));
        assert_eq!(extraction.document_number.as_deref(), Some("X1234567"));
        assert_eq!(extraction.extraction_method, "llm");
    }

    #[tokio::test]
    async fn llm_queue_depth_tracks_in_flight_calls_and_returns_to_zero() {
        let pipeline = mock_pipeline();

        assert_eq!(pipeline.llm_queue_depth(), 0, "idle before any call");

        pipeline
            .extract_via_inferer("P<UTO passport markdown")
            .await
            .expect("inferer extract");
        assert_eq!(
            pipeline.llm_queue_depth(),
            0,
            "guard releases after completion"
        );

        let (tx, _rx) = mpsc::channel(16);
        pipeline
            .extract_via_inferer_stream("P<UTO passport markdown", &tx)
            .await
            .expect("inferer extract_stream");
        assert_eq!(
            pipeline.llm_queue_depth(),
            0,
            "guard releases after streaming completion too"
        );
    }
}
