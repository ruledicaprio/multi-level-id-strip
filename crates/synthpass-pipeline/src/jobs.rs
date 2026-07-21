//! Batch-job abstraction (M5 job queue): submit N documents, get a
//! [`JobHandle`] back immediately, poll it (or `wait()` on it) for status and
//! per-document results.
//!
//! The two existing choke points — [`Pipeline::extract_via_inferer`] and
//! [`Pipeline::extract_via_inferer_stream`] — already gate Tier-2 concurrency
//! via `llm_semaphore`, and `ocr_and_tier1` now gates OCR concurrency via
//! `ocr_semaphore` (B2). A job is deliberately *not* a third concurrency
//! gate: [`Pipeline::submit`] just fires every document in the batch as an
//! independent [`Pipeline::process_document`] call, so a batch parallelizes
//! exactly as much as those two semaphores allow — no more, no less — the
//! same as if the caller had fired N single-document requests by hand.
//!
//! **In-memory only.** Nothing here is persisted; a process restart loses
//! every job. That is also why [`JobId`] is a process-local counter rather
//! than a UUID — see its doc.

use crate::{Pipeline, PipelineResult};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::watch;

/// Identifies one submitted batch job, unique within this process's
/// lifetime. Not a UUID: jobs are in-memory only (no persistence across
/// restarts, see the module doc), so a cheap process-local counter is all
/// identity needs — and it keeps `synthpass-pipeline` free of a new
/// dependency. (`synthpass-serve` already carries `uuid` for per-request
/// ids, but pulling it in here just to stringify a counter would be exactly
/// the kind of dependency creep the project's "add no new dependency" rule
/// exists to prevent.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(u64);

impl std::fmt::Display for JobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Lets a caller (e.g. `synthpass-serve`'s `GET /api/jobs/{id}`, which parses
/// the raw path segment with this rather than using `Path<JobId>` directly —
/// `JobId` intentionally doesn't implement `serde::Deserialize`, see that
/// handler's doc) round-trip the string [`Display`](std::fmt::Display) prints
/// back into a `JobId`.
impl std::str::FromStr for JobId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u64>().map(JobId)
    }
}

/// Lifecycle of one submitted job — see [`Pipeline::submit`].
///
/// `Failed` is reserved for a job that could not be *attempted* at all (as
/// of this release, only an empty document list) — a job where some
/// documents individually failed OCR still reaches `Done`; each document's
/// own outcome is recorded on its [`DocumentEntry`], never rolled up into
/// the job-level status. This mirrors [`Pipeline::process_document`] itself:
/// an OCR failure there is an `Err`, but a Tier-2 failure merely *degrades*
/// the result (`llm_error: Some(..)`) rather than failing the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

/// One document's outcome within a job. `Pending` until its
/// [`Pipeline::process_document`] call resolves.
pub enum DocumentStatus {
    Pending,
    /// Boxed for the same reason [`crate::ProcessEvent::Done`] is: keeps
    /// this enum small on the stack regardless of how large
    /// [`PipelineResult`] grows.
    Done(Box<PipelineResult>),
    /// The document's own [`crate::PipelineError`], stringified — an OCR
    /// failure, specifically (see [`JobStatus`]'s doc on why a Tier-2
    /// failure never lands here).
    Failed(String),
}

/// A document within a job, alongside its current [`DocumentStatus`].
pub struct DocumentEntry {
    pub input: PathBuf,
    pub status: DocumentStatus,
}

/// Internal, shared job state — reachable both from the [`JobHandle`]
/// `submit` returns and from a later [`Pipeline::job`] lookup by [`JobId`].
struct JobRecord {
    id: JobId,
    /// `watch`, not `Mutex<JobStatus>` + `Notify`: a `watch::Receiver` always
    /// observes the *current* value on `borrow()`/`changed()`, no matter
    /// when it subscribed relative to the last `send()` — so
    /// [`JobHandle::wait`] can never miss the final status transition the
    /// way a `Notify`-based "check-then-wait" can if the notification fires
    /// between the check and the `.await`.
    status_tx: watch::Sender<JobStatus>,
    documents: Mutex<Vec<DocumentEntry>>,
}

/// Default number of **completed** jobs retained for [`Pipeline::job`]
/// lookups before the oldest is evicted from the ring buffer — see
/// [`queue_capacity_from_env`] for how an operator overrides this.
pub(crate) const DEFAULT_QUEUE_CAPACITY: usize = 100;

/// Parses `SYNTHPASS_QUEUE_CAPACITY`'s raw value into a completed-job
/// retention count. Same fallback-on-garbage discipline as
/// [`crate::parse_llm_contexts`] (unset, unparsable, or non-positive all
/// fall back to a safe value) adapted to this setting's own floor: the
/// invariant that matters here is the same one `parse_llm_contexts`
/// protects — never produce 0, which would mean a job evicts itself from the
/// registry the instant it completes, breaking `GET /api/jobs/{id}` for the
/// job the caller just submitted. Unlike `parse_llm_contexts`, the fallback
/// value is [`DEFAULT_QUEUE_CAPACITY`] rather than `1`: a retention ring of
/// exactly one job would make that lookup useless for anything but the
/// single most recent batch.
fn parse_queue_capacity(raw: Option<&str>) -> usize {
    raw.and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(DEFAULT_QUEUE_CAPACITY)
}

/// Resolves the job-queue retention size from the environment:
/// `SYNTHPASS_QUEUE_CAPACITY`, or — for one release — the older
/// `SYNTHPASS_MAX_QUEUE_DEPTH` name, with a deprecation warning.
///
/// **This is not the same setting `synthpass-serve` already reads
/// `SYNTHPASS_MAX_QUEUE_DEPTH` for.** That one (`main.rs`'s
/// `max_queue_depth`) gates `/api/extract`'s 503 on
/// [`Pipeline::llm_queue_depth`] and is untouched by this function — it
/// keeps its exact original meaning. The fallback here exists only because
/// an operator who already set `SYNTHPASS_MAX_QUEUE_DEPTH` to mean "how much
/// queued work this server tolerates" reasonably expects it to keep doing
/// *something* on upgrade rather than being silently ignored for the new
/// job queue, hence honouring it here too, loudly, with a named migration
/// path.
pub fn queue_capacity_from_env() -> usize {
    if let Ok(raw) = std::env::var("SYNTHPASS_QUEUE_CAPACITY") {
        return parse_queue_capacity(Some(&raw));
    }
    if let Ok(raw) = std::env::var("SYNTHPASS_MAX_QUEUE_DEPTH") {
        tracing::warn!(
            "SYNTHPASS_MAX_QUEUE_DEPTH is deprecated for the job queue's completed-job \
             retention size — set SYNTHPASS_QUEUE_CAPACITY instead. (SYNTHPASS_MAX_QUEUE_DEPTH \
             keeps its original meaning in synthpass-serve — the /api/extract queue-full \
             threshold — unchanged; this is a separate, additional read of the same name, and \
             will stop being honoured here in a future release.)"
        );
        return parse_queue_capacity(Some(&raw));
    }
    parse_queue_capacity(None)
}

/// In-memory job store. Lives behind an `Arc` inside [`Pipeline`] and is
/// shared — never re-created — across every `Clone` of that `Pipeline`
/// (see `Pipeline::jobs`'s field doc): a job submitted through one clone
/// (e.g. the background task [`Pipeline::submit`] spawns) must stay visible
/// to a [`Pipeline::job`] lookup made through any other clone.
///
/// Only **completed** jobs count against `capacity` — a job that is still
/// `Queued`/`Running` stays reachable for as long as it takes to finish, no
/// matter how many other jobs complete meanwhile. This is a bound on
/// *history*, not an admission-control gate: batch submissions are
/// throttled indirectly, the same way `/api/extract` already is — see
/// `synthpass-serve`'s reuse of `queue_full_error` against
/// [`Pipeline::llm_queue_depth`] on the batch endpoint too.
pub(crate) struct JobRegistry {
    jobs: Mutex<HashMap<JobId, Arc<JobRecord>>>,
    completed_order: Mutex<VecDeque<JobId>>,
    capacity: usize,
    next_id: AtomicU64,
}

impl JobRegistry {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            completed_order: Mutex::new(VecDeque::new()),
            capacity: capacity.max(1),
            next_id: AtomicU64::new(1),
        }
    }

    fn next_id(&self) -> JobId {
        JobId(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    fn insert(&self, record: Arc<JobRecord>) {
        self.jobs
            .lock()
            .expect("jobs registry mutex poisoned")
            .insert(record.id, record);
    }

    fn get(&self, id: JobId) -> Option<Arc<JobRecord>> {
        self.jobs
            .lock()
            .expect("jobs registry mutex poisoned")
            .get(&id)
            .cloned()
    }

    /// Move `id` into the completed ring buffer, evicting the oldest
    /// completed job once `capacity` is exceeded. Must only be called once
    /// per job, after its status has already been set to a terminal value
    /// (see the single call site in [`Pipeline::submit`]).
    fn mark_completed(&self, id: JobId) {
        let evicted = {
            let mut order = self
                .completed_order
                .lock()
                .expect("completed_order mutex poisoned");
            order.push_back(id);
            if order.len() > self.capacity {
                order.pop_front()
            } else {
                None
            }
        };
        if let Some(evicted) = evicted {
            self.jobs
                .lock()
                .expect("jobs registry mutex poisoned")
                .remove(&evicted);
        }
    }
}

/// A handle to one submitted job — returned by [`Pipeline::submit`], and
/// obtainable again for an already-submitted job via [`Pipeline::job`].
/// Cheap to hold: internally just an `Arc` to the shared record.
pub struct JobHandle {
    record: Arc<JobRecord>,
}

impl JobHandle {
    pub fn id(&self) -> JobId {
        self.record.id
    }

    /// Current status. A cheap, synchronous, point-in-time read — never
    /// blocks, unlike [`wait`](Self::wait).
    pub fn status(&self) -> JobStatus {
        *self.record.status_tx.borrow()
    }

    /// Current per-document entries, in submission order. Returns a guard
    /// rather than a clone: `DocumentStatus` holds a full
    /// [`PipelineResult`] once a document completes, and requiring `Clone`
    /// on that just to hand back a snapshot would be a needless copy for
    /// every caller that (like `synthpass-serve`'s `GET /api/jobs/{id}`)
    /// only needs to read the fields once to build a response. Do not hold
    /// this guard across an `.await` point.
    pub fn documents(&self) -> MutexGuard<'_, Vec<DocumentEntry>> {
        self.record
            .documents
            .lock()
            .expect("job documents mutex poisoned")
    }

    /// Block until the job reaches a terminal status (`Done`/`Failed`),
    /// returning it. Every document's [`DocumentEntry`] is guaranteed
    /// populated (no longer `Pending`) by the time this returns — the
    /// worker task only sends the terminal status after every per-document
    /// result has been written.
    ///
    /// This is the CLI's `submit + wait` batch flow (`synthpass batch`
    /// stays synchronous end-to-end). `synthpass-serve`'s async job
    /// endpoints never call this — a server has to stay responsive to other
    /// requests while a batch runs, so it polls [`status`](Self::status) /
    /// [`documents`](Self::documents) from `GET /api/jobs/{id}` instead.
    pub async fn wait(&self) -> JobStatus {
        let mut rx = self.record.status_tx.subscribe();
        loop {
            let status = *rx.borrow();
            if matches!(status, JobStatus::Done | JobStatus::Failed) {
                return status;
            }
            if rx.changed().await.is_err() {
                // The sender side was dropped without ever reaching a
                // terminal state (e.g. the worker task panicked outside the
                // per-document `catch` in `submit`). Treat that as Failed
                // rather than hanging forever.
                return JobStatus::Failed;
            }
        }
    }
}

impl Pipeline {
    /// Submit a batch of documents for background processing, returning a
    /// [`JobHandle`] immediately. The job starts `Queued`, moves to
    /// `Running` once its worker task is scheduled, and reaches `Done` once
    /// every document has a terminal [`DocumentStatus`] (or `Failed`
    /// immediately for an empty `documents` list — see [`JobStatus`]'s
    /// doc).
    ///
    /// Every document is dispatched as an independent
    /// [`process_document`](Self::process_document) call — the exact same
    /// OCR → Tier 1 → Tier 2 path, including both concurrency semaphores, as
    /// a single-document request. Nothing about a document's own processing
    /// is aware it arrived as part of a batch; a batch parallelizes exactly
    /// as much as `ocr_semaphore`/`llm_semaphore` allow.
    pub fn submit(&self, documents: Vec<PathBuf>) -> JobHandle {
        let id = self.jobs.next_id();
        let (status_tx, _status_rx) = watch::channel(JobStatus::Queued);
        let initial_documents = documents
            .iter()
            .cloned()
            .map(|input| DocumentEntry {
                input,
                status: DocumentStatus::Pending,
            })
            .collect();
        let record = Arc::new(JobRecord {
            id,
            status_tx,
            documents: Mutex::new(initial_documents),
        });
        self.jobs.insert(record.clone());

        if documents.is_empty() {
            let _ = record.status_tx.send(JobStatus::Failed);
            self.jobs.mark_completed(id);
            return JobHandle { record };
        }

        // `self.clone()` is cheap (every field is `Arc`) and gives the
        // spawned task an owned, `'static` handle on the same OCR engine,
        // inferer, semaphores, metrics, and — crucially — the same
        // `JobRegistry`, so this job stays visible to `Pipeline::job`
        // lookups made through the original `Pipeline` (or any other
        // clone).
        let pipeline = self.clone();
        let bg_record = record.clone();
        tokio::spawn(async move {
            let _ = bg_record.status_tx.send(JobStatus::Running);

            // Fire every document concurrently; `process_document`'s own
            // `ocr_semaphore`/`llm_semaphore` acquisitions are what actually
            // throttle how much of this runs at once (see the module doc).
            // The `usize` index travels alongside each `JoinHandle` (not
            // through it) so a panicked task — which never returns its
            // value — doesn't lose track of which document it was.
            let mut handles = Vec::with_capacity(documents.len());
            for (idx, input) in documents.into_iter().enumerate() {
                let doc_pipeline = pipeline.clone();
                handles.push((
                    idx,
                    tokio::spawn(async move { doc_pipeline.process_document(&input).await }),
                ));
            }

            for (idx, handle) in handles {
                let outcome = match handle.await {
                    Ok(Ok(result)) => DocumentStatus::Done(Box::new(result)),
                    Ok(Err(e)) => DocumentStatus::Failed(e.to_string()),
                    Err(join_err) => {
                        DocumentStatus::Failed(format!("processing task panicked: {join_err}"))
                    }
                };
                bg_record
                    .documents
                    .lock()
                    .expect("job documents mutex poisoned")[idx]
                    .status = outcome;
            }

            // Retire into the ring buffer *before* publishing the terminal
            // status: `watch::Sender::send` is the one synchronization point
            // a `wait()` caller on another thread actually observes, so
            // anything that must be visible by the time `wait()` returns —
            // both every document's result (written above) and this job's
            // registry bookkeeping — has to happen-before it in program
            // order. Reversing this would let a `wait()` caller race ahead
            // of `mark_completed` on a multi-threaded runtime and observe a
            // registry that hasn't retired this job yet.
            pipeline.jobs.mark_completed(id);
            let _ = bg_record.status_tx.send(JobStatus::Done);
        });

        JobHandle { record }
    }

    /// Look up an already-submitted job by id — `None` if it never existed
    /// or has aged out of the completed-job retention ring (see
    /// [`JobRegistry`]).
    pub fn job(&self, id: JobId) -> Option<JobHandle> {
        self.jobs.get(id).map(|record| JobHandle { record })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OcrEngine, PipelineError, ProcessEvent};
    use async_trait::async_trait;
    use std::path::Path;
    use synthpass_core::Extraction;
    use tokio::sync::mpsc;

    /// Fixed Markdown per call, keyed by input filename stem, so a test can
    /// give different documents in one batch different OCR outcomes (e.g. a
    /// checksum-valid MRZ for one, prose with no MRZ for another) without
    /// needing real image files or a real OCR engine.
    struct StemKeyedOcr(std::collections::HashMap<&'static str, &'static str>);

    #[async_trait]
    impl OcrEngine for StemKeyedOcr {
        async fn to_markdown(&self, input: &Path) -> Result<String, PipelineError> {
            let stem = input
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            match self.0.get(stem) {
                Some(md) => Ok(md.to_string()),
                None => Err(PipelineError::Ocr(format!("no fixture for stem {stem}"))),
            }
        }
        fn describe(&self) -> String {
            "stem-keyed-ocr".into()
        }
    }

    struct JobsMockBackend;

    #[async_trait]
    impl crate::InferBackend for JobsMockBackend {
        async fn extract(&self, _markdown: &str) -> Result<Extraction, String> {
            let mut e = Extraction::default();
            e.extraction_method = "llm".into();
            Ok(e)
        }
        async fn extract_stream(
            &self,
            markdown: &str,
            _tx: &mpsc::Sender<ProcessEvent>,
        ) -> Result<Extraction, String> {
            self.extract(markdown).await
        }
        fn describe(&self) -> String {
            "jobs-mock-backend".into()
        }
        async fn health(&self) -> Result<String, String> {
            Ok("ok".into())
        }
    }

    /// The Croatian TD3 specimen MRZ from the `mrz` crate's corpus tests —
    /// every check digit valid, so this lands on Tier 1.
    const TD3_MRZ: &str = "## PUTOVNICA\n\nP<HRVSPECIMEN<<SPECIMEN<<<<<<<<<<<<<<<<<<<<<\n0070070071HRV8212258F1407019<<<<<<<<<<<<<<06\n";

    async fn temp_inputs(tag: &str, stems: &[&str]) -> (Vec<PathBuf>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("synthpass-jobs-{tag}-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.expect("temp dir");
        let mut inputs = Vec::new();
        for stem in stems {
            let input = dir.join(format!("{stem}.jpg"));
            tokio::fs::write(&input, b"not a real image - OCR is mocked")
                .await
                .expect("write temp input");
            inputs.push(input);
        }
        (inputs, dir)
    }

    #[tokio::test]
    async fn submit_processes_a_mixed_batch_to_done_with_correct_per_document_tiers() {
        let ocr = StemKeyedOcr(std::collections::HashMap::from([
            ("tier1", TD3_MRZ),
            ("tier2", "just prose — no MRZ anywhere"),
        ]));
        let pipeline = Pipeline::new(Box::new(ocr), Box::new(JobsMockBackend));
        let (inputs, dir) = temp_inputs("mixed", &["tier1", "tier2"]).await;

        let handle = pipeline.submit(inputs);
        assert_eq!(handle.wait().await, JobStatus::Done);
        assert_eq!(handle.status(), JobStatus::Done);

        // Block-scoped (rather than an explicit `drop()`) so the `MutexGuard`
        // unambiguously ends before the `.await` below — clippy's
        // `await_holding_lock` lint goes by lexical scope, not by where a
        // manual `drop()` call sits.
        {
            let docs = handle.documents();
            assert_eq!(docs.len(), 2);
            match &docs[0].status {
                DocumentStatus::Done(result) => {
                    assert_eq!(result.method, crate::Method::MrzDeterministic)
                }
                _ => panic!("expected tier1 doc to be Done, got a different status"),
            }
            match &docs[1].status {
                DocumentStatus::Done(result) => assert_eq!(result.method, crate::Method::Llm),
                _ => panic!("expected tier2 doc to be Done"),
            }
        }

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn submit_records_a_per_document_ocr_failure_without_failing_the_job() {
        // No fixture registered for this stem, so `StemKeyedOcr` errors —
        // exercising the per-document `Failed` path while the job itself
        // still reaches `Done`.
        let ocr = StemKeyedOcr(std::collections::HashMap::new());
        let pipeline = Pipeline::new(Box::new(ocr), Box::new(JobsMockBackend));
        let (inputs, dir) = temp_inputs("failure", &["missing"]).await;

        let handle = pipeline.submit(inputs);
        assert_eq!(handle.wait().await, JobStatus::Done, "the job itself ran");

        {
            let docs = handle.documents();
            assert!(
                matches!(docs[0].status, DocumentStatus::Failed(_)),
                "the individual document's OCR failure must be recorded, not silently dropped"
            );
        }

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn submit_with_no_documents_fails_immediately() {
        let pipeline = Pipeline::new(
            Box::new(StemKeyedOcr(Default::default())),
            Box::new(JobsMockBackend),
        );
        let handle = pipeline.submit(vec![]);
        assert_eq!(handle.status(), JobStatus::Failed);
        assert_eq!(handle.wait().await, JobStatus::Failed);
        assert!(handle.documents().is_empty());
    }

    #[tokio::test]
    async fn job_looks_up_a_submitted_job_by_id() {
        let ocr = StemKeyedOcr(std::collections::HashMap::from([("tier1", TD3_MRZ)]));
        let pipeline = Pipeline::new(Box::new(ocr), Box::new(JobsMockBackend));
        let (inputs, dir) = temp_inputs("lookup", &["tier1"]).await;

        let handle = pipeline.submit(inputs);
        let id = handle.id();
        handle.wait().await;

        let looked_up = pipeline.job(id).expect("job must be findable by id");
        assert_eq!(looked_up.status(), JobStatus::Done);
        assert_eq!(looked_up.documents().len(), 1);

        assert!(
            pipeline.job(JobId(999_999)).is_none(),
            "an id that was never submitted must not be found"
        );

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn completed_jobs_beyond_capacity_are_evicted_oldest_first() {
        let ocr = StemKeyedOcr(std::collections::HashMap::from([("tier1", TD3_MRZ)]));
        let mut pipeline = Pipeline::new(Box::new(ocr), Box::new(JobsMockBackend));
        pipeline.jobs = Arc::new(JobRegistry::new(1));
        let (inputs_a, dir_a) = temp_inputs("evict-a", &["tier1"]).await;
        let (inputs_b, dir_b) = temp_inputs("evict-b", &["tier1"]).await;

        let first = pipeline.submit(inputs_a);
        let first_id = first.id();
        first.wait().await;

        let second = pipeline.submit(inputs_b);
        let second_id = second.id();
        second.wait().await;

        assert!(
            pipeline.job(first_id).is_none(),
            "capacity 1: the older completed job must have been evicted"
        );
        assert!(
            pipeline.job(second_id).is_some(),
            "the newer completed job must still be retained"
        );

        let _ = tokio::fs::remove_dir_all(&dir_a).await;
        let _ = tokio::fs::remove_dir_all(&dir_b).await;
    }

    #[test]
    fn parse_queue_capacity_falls_back_to_the_default_on_bad_input() {
        assert_eq!(parse_queue_capacity(None), DEFAULT_QUEUE_CAPACITY);
        assert_eq!(parse_queue_capacity(Some("0")), DEFAULT_QUEUE_CAPACITY);
        assert_eq!(parse_queue_capacity(Some("abc")), DEFAULT_QUEUE_CAPACITY);
        assert_eq!(parse_queue_capacity(Some("-1")), DEFAULT_QUEUE_CAPACITY);
    }

    #[test]
    fn parse_queue_capacity_accepts_a_positive_count() {
        assert_eq!(parse_queue_capacity(Some("7")), 7);
        assert_eq!(parse_queue_capacity(Some("1")), 1);
    }

    #[test]
    fn job_id_round_trips_through_display_and_from_str() {
        use std::str::FromStr;
        let id = JobId(42);
        assert_eq!(id.to_string(), "42");
        assert_eq!(JobId::from_str("42").unwrap(), id);
        assert!(JobId::from_str("not-a-number").is_err());
    }
}
