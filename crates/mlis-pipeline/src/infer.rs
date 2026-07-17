//! Tier-2 inference backend abstraction: the pipeline gets an [`Extraction`]
//! from *some* backend. [`NativeInferer`] (feature `inferer-native`, default)
//! runs the Qwen GGUF in-process via `mlis-llm` / `llama-cpp-2`. [`GrpcInferer`]
//! (feature `inferer-grpc`) talks to the legacy Python sidecar over gRPC. The
//! backend is chosen at runtime by `MLIS_INFERER` (`native` | `grpc`).
//!
//! Concurrency control (the single-flight semaphore and queue-depth counter)
//! lives in [`crate::Pipeline`], not here — every backend is called through
//! the same guarded seam so the "one concurrent Tier-2 call" invariant holds
//! regardless of which backend is active.

use crate::ProcessEvent;
use async_trait::async_trait;
use mlis_core::Extraction;
use tokio::sync::mpsc;

#[cfg(not(any(feature = "inferer-native", feature = "inferer-grpc")))]
compile_error!(
    "mlis-pipeline requires at least one of the `inferer-native` or `inferer-grpc` features"
);

/// Produces an [`Extraction`] from OCR Markdown (Tier 2 — the LLM fallback).
#[async_trait]
pub trait InferBackend: Send + Sync {
    async fn extract(&self, markdown: &str) -> Result<Extraction, String>;
    /// Like [`Self::extract`], but forwards incremental text on `tx` as
    /// [`ProcessEvent::Delta`] while the model generates. Implementations
    /// must use non-blocking sends (`try_send`) — a stalled receiver must
    /// never extend how long the caller's concurrency permit is held.
    async fn extract_stream(
        &self,
        markdown: &str,
        tx: &mpsc::Sender<ProcessEvent>,
    ) -> Result<Extraction, String>;
    /// Short human-readable identity for logs.
    fn describe(&self) -> String;
    /// Preflight check for `mlis doctor`: `Ok(status)` on success, `Err(reason)`
    /// otherwise. Must not panic and should be cheap (no full model load).
    async fn health(&self) -> Result<String, String>;
}

#[cfg(feature = "inferer-grpc")]
mod grpc {
    use super::*;
    use crate::extraction_from_response;
    use crate::inferer::inferer_client::InfererClient;
    use crate::inferer::{ExtractRequest, HealthRequest};

    /// Legacy backend: the persistent Python sidecar, reached over gRPC (see
    /// `proto/inferer.proto`). Kept as a fallback through one release past
    /// `NativeInferer` shipping; scheduled for deletion once pure-Rust OCR
    /// lands and the sidecar has no remaining reason to exist.
    pub struct GrpcInferer {
        addr: String,
    }

    impl GrpcInferer {
        pub fn new(addr: impl Into<String>) -> Self {
            Self { addr: addr.into() }
        }

        pub fn addr(&self) -> &str {
            &self.addr
        }
    }

    #[async_trait]
    impl InferBackend for GrpcInferer {
        async fn extract(&self, markdown: &str) -> Result<Extraction, String> {
            let mut client = InfererClient::connect(self.addr.clone())
                .await
                .map_err(|e| format!("cannot reach inferer at {}: {e}", self.addr))?;
            let resp = client
                .extract(ExtractRequest {
                    markdown: markdown.to_string(),
                    image_roi: Vec::new(),
                })
                .await
                .map_err(|e| format!("inferer Extract RPC failed: {e}"))?
                .into_inner();
            Ok(extraction_from_response(resp))
        }

        async fn extract_stream(
            &self,
            markdown: &str,
            tx: &mpsc::Sender<ProcessEvent>,
        ) -> Result<Extraction, String> {
            let mut client = InfererClient::connect(self.addr.clone())
                .await
                .map_err(|e| format!("cannot reach inferer at {}: {e}", self.addr))?;
            let mut stream = client
                .extract_stream(ExtractRequest {
                    markdown: markdown.to_string(),
                    image_roi: Vec::new(),
                })
                .await
                .map_err(|e| format!("inferer ExtractStream RPC failed: {e}"))?
                .into_inner();

            loop {
                match stream.message().await {
                    Ok(Some(chunk)) => {
                        if !chunk.delta.is_empty() {
                            // Best-effort UI progress — a stalled browser must
                            // never extend how long the caller's permit is held.
                            let _ = tx.try_send(ProcessEvent::Delta(chunk.delta));
                        }
                        if chunk.done {
                            return match chunk.result {
                                Some(result) => Ok(extraction_from_response(result)),
                                None => Err("inferer stream finished without a result".into()),
                            };
                        }
                    }
                    Ok(None) => return Err("inferer stream ended before a final chunk".into()),
                    Err(e) => return Err(format!("inferer stream error: {e}")),
                }
            }
        }

        fn describe(&self) -> String {
            format!("gRPC inferer @ {}", self.addr)
        }

        async fn health(&self) -> Result<String, String> {
            let mut client = InfererClient::connect(self.addr.clone())
                .await
                .map_err(|e| format!("inferer NOT reachable at {}: {e}", self.addr))?;
            let resp = client
                .health(HealthRequest {})
                .await
                .map_err(|e| format!("connected but Health RPC failed: {e}"))?
                .into_inner();
            Ok(format!(
                "reachable at {} (model_loaded: {})",
                self.addr, resp.model_loaded
            ))
        }
    }
}
#[cfg(feature = "inferer-grpc")]
pub use grpc::GrpcInferer;

#[cfg(feature = "inferer-native")]
mod native {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::OnceCell;

    /// Default backend: the Qwen GGUF run in-process via `llama-cpp-2`. The
    /// model is loaded lazily (on first Tier-2 call) and kept warm for the
    /// process lifetime — `llama_cpp_2::llama_backend::LlamaBackend::init()`
    /// is itself a process-wide singleton, so this cell must only ever
    /// populate once.
    pub struct NativeInferer {
        model_path: PathBuf,
        n_ctx: u32,
        llm: OnceCell<Arc<mlis_llm::NativeLlm>>,
    }

    impl NativeInferer {
        pub fn new(model_path: impl Into<PathBuf>, n_ctx: u32) -> Self {
            Self {
                model_path: model_path.into(),
                n_ctx,
                llm: OnceCell::new(),
            }
        }

        pub fn model_path(&self) -> &std::path::Path {
            &self.model_path
        }

        async fn get_or_load(&self) -> Result<Arc<mlis_llm::NativeLlm>, String> {
            self.llm
                .get_or_try_init(|| async {
                    let path = self.model_path.clone();
                    let n_ctx = self.n_ctx;
                    tokio::task::spawn_blocking(move || mlis_llm::NativeLlm::load(&path, n_ctx))
                        .await
                        .map_err(|e| format!("model load task panicked: {e}"))?
                        .map(Arc::new)
                })
                .await
                .cloned()
        }
    }

    #[async_trait]
    impl InferBackend for NativeInferer {
        async fn extract(&self, markdown: &str) -> Result<Extraction, String> {
            let llm = self.get_or_load().await?;
            let markdown = markdown.to_string();
            tokio::task::spawn_blocking(move || llm.extract(&markdown))
                .await
                .map_err(|e| format!("inference task panicked: {e}"))?
        }

        async fn extract_stream(
            &self,
            markdown: &str,
            tx: &mpsc::Sender<ProcessEvent>,
        ) -> Result<Extraction, String> {
            let llm = self.get_or_load().await?;
            let markdown = markdown.to_string();
            let tx = tx.clone();
            tokio::task::spawn_blocking(move || {
                llm.extract_stream(&markdown, |delta| {
                    // Non-blocking: a stalled receiver must never extend how
                    // long the caller's concurrency permit is held.
                    let _ = tx.try_send(ProcessEvent::Delta(delta.to_string()));
                })
            })
            .await
            .map_err(|e| format!("inference task panicked: {e}"))?
        }

        fn describe(&self) -> String {
            format!("native llama.cpp @ {}", self.model_path.display())
        }

        async fn health(&self) -> Result<String, String> {
            if !self.model_path.exists() {
                return Err(format!("model not found at {}", self.model_path.display()));
            }
            if mlis_llm::verify::skip_verify() {
                return Ok(format!(
                    "model present at {} (sha256 verification skipped)",
                    self.model_path.display()
                ));
            }
            let path = self.model_path.clone();
            tokio::task::spawn_blocking(move || mlis_llm::verify::verify_model(&path))
                .await
                .map_err(|e| format!("verify task panicked: {e}"))?
                .map_err(|e| e.to_string())?;
            Ok(format!(
                "model present and sha256-verified at {}",
                self.model_path.display()
            ))
        }
    }
}
#[cfg(feature = "inferer-native")]
pub use native::NativeInferer;

/// Default GGUF path when `MLIS_MODEL_PATH` is unset — the model this
/// workspace ships against, expected at the repository root.
#[cfg(feature = "inferer-native")]
const DEFAULT_MODEL_PATH: &str = "./qwen2.5-1.5b-instruct-q4_k_m.gguf";
#[cfg(feature = "inferer-native")]
const DEFAULT_N_CTX: u32 = 2048;
#[cfg(feature = "inferer-grpc")]
const DEFAULT_GRPC_ADDR: &str = "http://127.0.0.1:50051";

/// Build the Tier-2 backend selected by `MLIS_INFERER` (`native` | `grpc`).
/// Defaults to `native` when this build has the `inferer-native` feature,
/// else `grpc`. Falls back (with a warning) if the requested backend's
/// feature wasn't compiled in.
pub fn backend_from_env() -> Box<dyn InferBackend> {
    let choice = std::env::var("MLIS_INFERER").unwrap_or_else(|_| default_choice().to_string());
    match choice.as_str() {
        "grpc" => grpc_choice(),
        _ => native_choice(),
    }
}

fn default_choice() -> &'static str {
    if cfg!(feature = "inferer-native") {
        "native"
    } else {
        "grpc"
    }
}

#[cfg(feature = "inferer-native")]
fn native_choice() -> Box<dyn InferBackend> {
    let model_path = std::env::var("MLIS_MODEL_PATH").unwrap_or_else(|_| DEFAULT_MODEL_PATH.into());
    let n_ctx = std::env::var("MLIS_MODEL_N_CTX")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_N_CTX);
    Box::new(NativeInferer::new(model_path, n_ctx))
}
#[cfg(not(feature = "inferer-native"))]
fn native_choice() -> Box<dyn InferBackend> {
    eprintln!(
        "[mlis] MLIS_INFERER=native requested but this build lacks the `inferer-native` \
         feature — falling back to grpc"
    );
    grpc_choice()
}

#[cfg(feature = "inferer-grpc")]
fn grpc_choice() -> Box<dyn InferBackend> {
    let addr = std::env::var("MLIS_INFERER_ADDR").unwrap_or_else(|_| DEFAULT_GRPC_ADDR.into());
    Box::new(GrpcInferer::new(addr))
}
#[cfg(not(feature = "inferer-grpc"))]
fn grpc_choice() -> Box<dyn InferBackend> {
    eprintln!(
        "[mlis] MLIS_INFERER=grpc requested but this build lacks the `inferer-grpc` \
         feature — falling back to native"
    );
    native_choice()
}
