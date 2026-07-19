# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.8.0] — 2026-07-19

Offline cryptographic licensing: the shipped `mlis`/`mlis-serve` binaries now require an
Ed25519-signed license to run their extraction path, so the software can be sold and metered for
air-gapped enterprise distribution without ever phoning home. This is the fourth step on the road
to a single static musl binary — see `docs/ARCHITECTURE.md` §10.

### Added
- **`mlis-license`** (new crate): `LicensePayload`/`SignedLicense` types, `verify()` (Ed25519
  `verify_strict` over the exact signed payload bytes — never a re-serialized copy, so signer and
  verifier can never desync on field-order/whitespace), `check()` (expiry + optional fingerprint
  match, pure/deterministic), `load_and_check()` (the file-reading convenience used at
  CLI/serve startup). `fingerprint::machine_fingerprint()` hashes `/etc/machine-id` (Linux) via
  the same `sha256_hex` the audit log uses — not the weaker OS+hostname+CPU-brand approach some
  designs use. `keys::verifying_key()` reads the embedded `pubkey.b64` (or `MLIS_LICENSE_PUBKEY`
  override, mirroring the `MLIS_MODEL_SHA256` convention).
- **`mlis-license-issuer`** (new binary, `vendor` feature, never shipped to customers): `keygen`
  and `issue-license` subcommands. Keeping signing/keygen entirely off the customer-facing binary
  (rather than gating it by an env var inside the same binary, as a naive design would) means
  `mlis`/`mlis-serve` carry no private-key handling and no keygen RNG dependency at all.
- **`mlis fingerprint`** and **`mlis verify-license [path]`** CLI subcommands (hand-rolled
  dispatch, no `clap` — both are flag-light; the flag-heavy `issue-license` lives only in the
  off-binary issuer). Both stay usable without a valid license, since you need `fingerprint` to
  get one.
- **License enforcement:** `mlis`'s extraction path (`mlis <file>`) checks once at startup;
  `decrypt`/`doctor`/`fingerprint`/`verify-license` are exempt. `mlis-serve` checks once at boot
  (`license_refusal()`, mirrors the existing `startup_refusal()` non-loopback gate) plus a cheap
  expiry-only re-check (no signature re-verification) on every `/api/extract` request, so a
  long-running server stops serving once its license expires. `MLIS_LICENSE_SKIP=1` bypasses
  enforcement for local development/CI, mirroring `MLIS_MODEL_SKIP_VERIFY`.
- **`mlis doctor`** gained a license block: required unless skipped, shows tier/expiry/days
  remaining, ⚠️ under 30 days remaining.
- New env vars: `MLIS_LICENSE_PATH` (default `license.mlis`), `MLIS_LICENSE_SKIP`,
  `MLIS_LICENSE_PUBKEY`; vendor-only `MLIS_LICENSE_PRIVKEY`.
- CI: `mlis-license`'s default (no `vendor`) and `--features vendor` builds each checked alone;
  the vendor-gated signing/keygen tests run as their own step (not covered by the default
  workspace test run, since `vendor` isn't a default feature).

### Changed
- `crates/mlis-license/pubkey.b64` ships with a **placeholder** keypair generated during
  development. A real deployment must run `mlis-license-issuer keygen` and replace this file
  before issuing real licenses — see `docs/ARCHITECTURE.md` §6.

### Known limitations
- The license binds to an OS *installation* via `/etc/machine-id`, not physically to hardware:
  root can read/copy it, it survives a disk clone, and expiry trusts the system clock (which an
  air-gapped operator can roll back). Because the source is public, anyone who rebuilds from
  source strips the check entirely. This meters and gates the *official pre-built binary* and
  deters casual sharing — it is explicitly **not DRM** and not sold as tamper-proof. True hardware
  attestation would need a TPM/HSM, out of scope here. Stated in full in `docs/ARCHITECTURE.md` §6.

## [0.7.5] — 2026-07-19

Pure-Rust, image-only pipeline: the legacy gRPC Tier-2 backend and the Docker-based
`docling-serve` OCR engine are both **deleted outright**, not just made non-default. Docker and
Python are no longer required for any functional code path. **This also means PDF input is no
longer supported** — `docling-serve` was the only engine that parsed it, and `ocrs` (the default
OCR engine since v0.7.0) is image-only. The `inferer-grpc` feature's own code comment stated it
would be kept "for one release past `inferer-native` shipping" (v0.6.0); v0.7.0 was that release,
so this milestone is the follow-through on that stated plan, not a surprise removal. Shrinks the
dependency/attack surface the v0.8.0 licensing work and the eventual v1.0.0 musl static build must
both carry.

### Removed
- **gRPC Tier-2 backend** (`GrpcInferer`, Cargo feature `inferer-grpc`): `tonic`/`prost` deps,
  `proto/inferer.proto`, the entire `python/inferer` sidecar package and its gRPC codegen tooling,
  `docker/Dockerfile.inferer` + `entrypoint-inferer.sh`, and the `bridge_e2e.rs` cross-language
  test. **API break:** the `GrpcInferer` type and `inferer-grpc` feature no longer exist; anyone
  building against them directly needs to switch to the (now sole) native backend.
- **`docling-serve` OCR engine** (`DoclingEngine`, `MLIS_OCR_ENGINE=docling`) and its
  `docling_rs` dependency. **This removes PDF input support entirely** — no remaining OCR engine
  parses PDF. The OCR engine choice is now a two-way `rust` (default) / `native` match; a build
  with neither OCR feature enabled now fails to compile with a clear `compile_error!` instead of
  silently falling back to a Docker service that no longer exists.
- CI: the `python` (gRPC smoke) and `bridge` (cross-language e2e) jobs, and their required-status-
  check entries in branch protection. `protobuf-compiler`/`protobuf` dropped from the Linux and
  macOS job dependency installs (no longer needed without `tonic-build`).
- `docker/docker-compose.yml`'s `docling` and `inferer` services; `docker/Dockerfile.serve` no
  longer installs `protobuf-compiler`. The compose file now only builds/runs `mlis-serve` — Docker
  is entirely optional packaging, not a functional dependency, as of this release.

### Added
- **HEIC/HEIF rejection.** Detected by extension and rejected with a clear, actionable error
  ("convert to JPEG or PNG first") rather than a silent or generic OCR failure. No permissively-
  licensed pure-Rust HEIC/HEIF decoder exists — the two that do (`imazen/heic`,
  `ente-io/heic-decoder`) are both AGPL-3.0, which would force this MIT-licensed project (and the
  offline-licensing model planned for v0.8.0) to AGPL too. Revisiting this via a commercial
  license or an in-house permissive decoder is an open follow-up, not a rejected idea.
- **Format-coverage tests.** `mlis-pipeline`'s OCR smoke test now also proves JPEG, PNG, and WebP
  all flow through the pure-Rust OCR engine end-to-end (previously only JPEG was exercised); fast
  unit tests cover the PDF/HEIC rejection paths without needing the real `.rten` model files.

### Known limitations
- No PDF support (see Removed, above) and no HEIC/HEIF support (see Added, above). Both are
  deliberate scope cuts for this release, not oversights.

## [0.7.0] — 2026-07-18

Native Tier-1 OCR: a pure-Rust `ocrs`/`rten` engine now runs in-process, no `docling-serve` Docker
container required by default. Second step on the road to a single static musl binary — `docling`
stays available for PDF input (which the native engine can't parse) and the Tesseract-based
`native-ocr` engine stays as an accuracy fallback.

### Added
- **`mlis-ocr`** (new crate): `NativeOcr` loads two `.rten` weight files (text detection +
  recognition) via `ocrs`/`rten`, `get_text()` convenience API for full-text extraction. SHA-256
  integrity check for both files (`mlis-core::audit::verify_file_sha256`, shared with `mlis-llm`'s
  model check), auto-download from the fixed `ocrs-models` S3 bucket into `MLIS_OCR_MODEL_DIR`
  (`MLIS_OCR_AUTO_DOWNLOAD=0` to require pre-staged files).
- **`RustOcrEngine`** (`mlis-pipeline`, feature `ocr-native-rust`, **default**): the OCR seam is now
  a three-way `MLIS_OCR_ENGINE` choice (`rust` | `docling` | `native`), mirroring v0.6.0's
  `InferBackend` pattern. `rust` is image-only — PDF input returns a clear error pointing at
  `MLIS_OCR_ENGINE=docling` instead of failing confusingly inside `ocrs`.
- **CI**: `.rten` model files (cached, ~12 MB) downloaded and checksum-verified on every push (not
  opt-in, unlike the ~1 GB GGUF); `ocr-native-rust`/`native-ocr` feature-combination build checks; a
  real-model e2e test (`mlis-ocr`) and a pipeline smoke test (`mlis-pipeline`) both run `--ignored`
  in CI. `mlis-ocr` is a default workspace member, so the macOS `rust-portable` job now also proves
  the "genuinely pure Rust, cross-platform OCR" claim, unlike `ocr-daemon`.

### Changed
- `mlis doctor`'s OCR check is now a three-way match: `rust` verifies both model files' presence and
  checksum, `docling` keeps the existing TCP reachability check, `native` keeps its no-network-check
  message.
- `docker/docker-compose.yml`: `docling` service commented as legacy/PDF-only; `serve`'s
  `MLIS_OCR_ENGINE` is now pinned to `docling` explicitly (previously implicit, since `docling` used
  to be the only default) to keep that compose file's legacy full-stack demo behavior unchanged.
- `mlis-core::audit`: `verify_file_sha256`/`Sha256MismatchError` factored out of `mlis-llm::verify`
  (now a thin wrapper/type-alias over it) so `mlis-ocr` doesn't duplicate the same ~20-line pattern a
  second and third time (it needs the check for two files).

### Fixed
- **Both native backends now verify model integrity on the actual load path, not just in `mlis
  doctor`.** Previously `NativeInferer::get_or_load` (`mlis-llm`'s GGUF) and `RustOcrEngine::get_or_load`
  (`mlis-ocr`'s two `.rten` files) loaded weights straight into memory with no SHA-256 check at all —
  verification only ran as an optional, separate preflight command. A tampered or corrupted-but-complete
  file (or download) is now rejected before it's ever mapped into the process, honoring
  `MLIS_MODEL_SKIP_VERIFY`/`MLIS_OCR_MODEL_SKIP_VERIFY` the same way `mlis doctor` already did. Found by
  an Opus-assisted code review of this milestone's diff.

### Known limitations
- `ocrs`'s out-of-the-box text recognition is not yet verified against this project's specimen
  corpus and, on the workspace's own low/medium-resolution samples, is not always clean enough to
  reconstruct a checksum-valid MRZ line — see `docs/ARCHITECTURE.md` §7. Tier 2 still runs as
  designed when this happens.

## [0.6.0] — 2026-07-17

Native Tier-2 inference: the Qwen GGUF now runs in-process via `llama-cpp-2`, no Python sidecar
required. First step on the road to a single static musl binary — the legacy gRPC path stays as a
one-release fallback while the pure-Rust OCR and licensing milestones land.

### Added
- **`mlis-llm`** (new crate): loads and runs the Qwen2.5 GGUF in-process via `llama-cpp-2`, ChatML
  prompting, deterministic (temp-0, greedy) sampling, and JSON repair/parsing ported from
  `python/inferer/{prompts,adapter}.py`. SHA-256 model integrity check (`MLIS_MODEL_SHA256` override,
  `MLIS_MODEL_SKIP_VERIFY=1` to bypass) reusing `mlis_core::audit::sha256_hex`.
- **`InferBackend` trait** (`mlis-pipeline`): the Tier-2 seam is now pluggable — `NativeInferer`
  (feature `inferer-native`, **default**) wraps `mlis-llm`; `GrpcInferer` (feature `inferer-grpc`,
  still default this release) is the existing Python sidecar path, scheduled for removal once the
  pure-Rust OCR milestone lands and the sidecar has no remaining reason to exist. Selected at runtime
  via `MLIS_INFERER=native|grpc` (defaults to `native` when compiled in). Concurrency control (the
  single-flight semaphore + queue-depth counter) stays centralized in `Pipeline`, backend-agnostic.
- **Field-accuracy parity harness** (`crates/mlis-llm/tests/parity.rs`, `--ignored`): runs the native
  backend against `samples/*.md` and compares against the deterministic-MRZ ground truth in the
  matching `samples/*.json`, asserting a floor on the per-field match rate as a regression guard (not
  an accuracy gate — a 1.5B model reading OCR'd Markdown won't hit 100%, and Tier 1 exists precisely
  for the documents where it wouldn't need to).
- **CI**: `inferer-native`/`inferer-grpc` feature-combination build checks on every push; an opt-in
  `native-llm` job (`workflow_dispatch`) downloads the real GGUF and runs the e2e smoke test plus the
  parity harness.

### Changed
- `mlis doctor` now reports Tier-2 health via `Pipeline::infer_describe()`/`infer_health()` instead of
  hardcoding a gRPC `Health` RPC call — works against whichever backend is active.
- `docker/docker-compose.yml`'s `serve` service pins `MLIS_INFERER=grpc` for this release, so the
  existing `inferer` sidecar container keeps being used by default until compose gains a native
  profile.

## [0.5.1] — 2026-07-17

Bounded, observable inference queue — plus a safety gap the redesign surfaced.

### Added
- **`MLIS_MAX_QUEUE_DEPTH`** (`mlis-serve`, default `4`): reject uploads with `503` once this many
  Tier-2 requests are queued/in-flight, instead of accepting them unboundedly and blocking behind
  the single-GPU semaphore. `Pipeline::llm_queue_depth()` exposes the live count.
- **`python/bench_inferer.py`**: a minimal concurrency benchmark (mock-mode, in-process gRPC server)
  reporting per-request latency and simulated rejection counts across candidate queue-depth caps —
  the "benchmark first" groundwork for any future batched-gRPC work, which remains unimplemented
  pending evidence it's needed.

### Changed
- The single-flight Tier-2 serialization primitive moved from a bare `Mutex<()>` to a `Semaphore` +
  atomic depth counter (same one-concurrent-call guarantee); streaming `delta` events are now
  forwarded best-effort (`try_send`) so a stalled browser connection no longer extends how long the
  GPU permit is held.

### Fixed
- `python/inferer/loader.py`'s `Llama` instance had **no serialization of its own** — the Rust-side
  lock was the only thing preventing the gRPC server's `ThreadPoolExecutor(max_workers=4)` from
  making concurrent calls into the shared `llama.cpp` context, which risks corrupted output or a
  crash, not just VRAM exhaustion. Added a `threading.Lock` around every model call as defense-in-depth.

## [0.5.0] — 2026-07-17

Throughput & robustness: the Tier-2 UI freeze and the native OCR path's biggest accuracy gap.

### Added
- **Streaming Tier-2 inference (SSE)**: `proto/inferer.proto` gains a server-streaming `ExtractStream`
  RPC (`ExtractChunk{delta, done, result}`) alongside the existing unary `Extract`. The Python inferer
  uses `llama-cpp-python`'s native `stream=True` support to emit token deltas as the model generates;
  `mlis-serve`'s `/api/extract` now returns a Server-Sent Events stream that forwards those deltas to
  the browser in real time, so the upload page shows live "Extracting…" progress instead of a frozen
  status line for the ~few-second-to-30s Tier-2 wait. The Tier-1 (deterministic MRZ) fast path is
  unaffected — it still resolves in a single SSE `result` event with no visible change in behavior.
- **Native OCR preprocessing** (`ocr-daemon`, Linux/WSL): three new steps ahead of the existing Otsu
  binarization — DPI normalization (upscales small images to a ~300-DPI-equivalent floor), orientation
  correction (0/90/180/270°, scored by Tesseract's own confidence), and deskew (projection-profile
  method, ±15° search). Targets the main failure modes of phone-camera document photos that the native
  path previously had no defense against.

### Changed
- `Pipeline::process_document` is unchanged (CLI keeps its existing unary behavior); the OCR + Tier-1
  logic it shares with the new streaming path was extracted into a private `ocr_and_tier1` helper.

## [0.4.1] — 2026-07-17

Coverage hardening: closes the one untested boundary in v0.4.0 (the Rust↔Python gRPC bridge)
and the one untested surface (`mlis-serve` auth), plus a correctness fix each found.

### Added
- **Cross-language bridge test** (`crates/mlis-pipeline/tests/bridge_e2e.rs`): drives the real
  Python inferer (mock mode) from the real Rust `InfererClient`, catching drift across
  `proto/inferer.proto` / `tonic` / `grpcio` that no other test could — each other test mocks
  one side of the boundary. Wired into CI as a new `bridge` job.
- **`mlis-serve` auth test coverage** (was 0 tests): `is_loopback`, the non-loopback-without-token
  startup refusal, and the bearer-auth middleware (missing/wrong/non-bearer/correct token) are now
  unit-tested via `tower::ServiceExt::oneshot`.

### Fixed
- `is_loopback` used a string-prefix check, so an address like `127.0.0.1.evil.example:8080`
  was wrongly treated as loopback; now parses and exact-matches the host. Caught by the new tests.
- Tier-2 LLM output could echo HTML-escaped MRZ chevrons (`&lt;` instead of `<`) into fields like
  `mrz_line`, because docling's Markdown HTML-escapes them and the prompt passed it through
  verbatim. `python/inferer/prompts.py` now unescapes the Markdown before prompting the model.

## [0.4.0] — 2026-07-17

Rebrand of `docs-to-md` → **multi-level-id-strip (mlis)** plus a full architecture restructure and
the Tier-3 security milestone.

### Added
- **TD2 MRZ support** (2×36, ICAO 9303 Part 6) alongside TD1/TD3, verified against the official
  specimen, with checksum-verified OCR repair.
- **Date plausibility** (`mrz::DateValidity`): expiry-vs-today and DOB-before-expiry checks, computed
  clock-free (the caller supplies "today"), distinct from the check digits.
- **ISO 3166-1 / ICAO country names** (`mrz::country_name`), including the stateless/refugee/
  organization and specimen codes — the table follows the standard verbatim and neutrally.
- **`mlis-core`** crate: one canonical `Extraction` schema shared by every tier and the WASM demo.
- **Persistent gRPC inferer**: `proto/inferer.proto` + a warm Python sidecar (`python/inferer`,
  grpcio) reached over tonic — replaces the per-document `python extract_json.py` cold-load.
- **Pluggable OCR** via an `OcrEngine` trait: docling-serve (default, all platforms) and a native
  Tesseract+Leptonica engine (`ocr-daemon`, Linux/WSL, `--features native-ocr`).
- **Tier 3 security**: PII-free SHA-256 audit trail (`MLIS_AUDIT_LOG`), AES-256-GCM output
  encryption (`MLIS_KEY` → `.json.enc`, `mlis decrypt`), bearer-token auth with a hard refusal to
  bind non-loopback without a token, and optional rustls TLS.
- **Docker orchestration**: `docker/` Dockerfiles + `docker-compose.yml` (OCR + inferer + web);
  release LTO profile.

### Changed
- Crates/binaries renamed and regrouped under `crates/`: `pipeline`→`mlis-pipeline`,
  `docling-client`→`mlis-cli` (binary `mlis`), `docling-app`→`mlis-serve`.
- `mrz` split into `checksum` / `parser` / `dates` / `countries` modules (behavior preserved; all
  original tests kept as the regression gate).
- Tier-1 output enriched with resolved country names and the date-plausibility summary.

### Removed
- `extract_json.py` and root `requirements.txt` (superseded by `python/inferer` + `python/pyproject.toml`).

### Security
- The web app now enforces authentication and refuses wide-open binds by default; see
  [SECURITY.md](SECURITY.md).

## [0.3.0]
- Hybrid deterministic extraction: pure-Rust ICAO 9303 MRZ (TD1/TD3) with checksum-verified OCR
  repair (Tier 1), local Qwen 2.5 GGUF fallback (Tier 2), and a client-side WASM MRZ demo.

## [0.2.0]
- Air-gapped document pipeline: workspace, shared pipeline crate, CLI + axum web app.
