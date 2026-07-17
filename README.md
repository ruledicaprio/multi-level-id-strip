# 📄 docs-to-md

<!-- Language / backend -->
![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)
![Tokio](https://img.shields.io/badge/Tokio-0B7261?style=flat&logo=rust&logoColor=white)
![Axum](https://img.shields.io/badge/Axum-000000?style=flat&logo=rust&logoColor=white)
<!-- ML / inference -->
![Python](https://img.shields.io/badge/Python-3776AB?style=flat&logo=python&logoColor=white)
![llama.cpp](https://img.shields.io/badge/llama.cpp-GGUF-555555?style=flat)
![Qwen 2.5](https://img.shields.io/badge/Qwen%202.5-1.5B%20q4__k__m-6236FF?style=flat)
![CUDA](https://img.shields.io/badge/CUDA-12.4-76B900?style=flat&logo=nvidia&logoColor=white)
<!-- OCR / runtime -->
![Docker](https://img.shields.io/badge/Docker-2496ED?style=flat&logo=docker&logoColor=white)
![docling-serve](https://img.shields.io/badge/docling--serve-OCR-FF6600?style=flat)
<!-- MRZ / demo -->
![WebAssembly](https://img.shields.io/badge/WebAssembly-654FF0?style=flat&logo=webassembly&logoColor=white)
![ICAO 9303](https://img.shields.io/badge/ICAO%209303-MRZ%20checksums-0B7261?style=flat)
[![Live demo](https://img.shields.io/badge/live%20demo-GitHub%20Pages-222222?style=flat&logo=github&logoColor=white)](https://ruledicaprio.github.io/docs-to-md/)
<!-- Posture -->
![Air-gapped](https://img.shields.io/badge/deploy-air--gapped-10B981?style=flat)
![Cloud calls](https://img.shields.io/badge/cloud%20calls-0-brightgreen?style=flat)
![License](https://img.shields.io/badge/license-MIT-blue?style=flat)

Air-gapped document extraction: passports, ID cards, and technical manuals in — structured JSON out, with **zero cloud calls**. A shared Rust pipeline ships files to a local `docling-serve` OCR container (CPU), validates identity documents **deterministically via ICAO 9303 MRZ check digits** (Tier 1), and only falls back to a quantized Qwen 2.5 GGUF model (GPU, Tier 2) when no valid MRZ exists. Use it from the CLI, a self-hostable axum web app, or the [**browser-only MRZ demo**](https://ruledicaprio.github.io/docs-to-md/) — no PII ever leaves your machine.

## 🔀 Pipeline

```mermaid
flowchart LR
    A[📄 Image / PDF] --> B{Entry point}
    B -->|CLI| C[docling-client]
    B -->|upload page / API| D["docling-app<br/>(axum, :8080)"]
    C --> E["pipeline crate<br/>process_document()"]
    D --> E
    E -->|"POST /v1/convert/file"| F["🐳 docling-serve<br/>OCR · CPU · :5001"]
    F -->|Markdown| E
    E --> T{"🔐 Tier 1<br/>ICAO 9303 checksums<br/>valid?"}
    T -->|"yes — deterministic,<br/>LLM skipped"| H[📦 .md + .json artifacts]
    T -->|"no — fallback,<br/>🔒 mutex"| G["🧠 Tier 2<br/>extract_json.py<br/>Qwen 2.5 GGUF · GPU"]
    G -->|strict JSON| H

    A2[🌐 Browser-only demo] -->|"tesseract.js OCR +<br/>mrz crate as WASM"| W["GitHub Pages<br/>no server at all"]
```

CPU handles OCR, GPU handles LLM inference — a deliberate split that keeps a 3.5 GB-VRAM card (GTX 970) from OOM-ing. Full rationale in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## 🔐 Deterministic MRZ validation (Tier 1)

The [`mrz`](mrz/) crate is a zero-dependency ICAO 9303 parser: TD3 passports and TD1 ID cards, every check digit verified (7-3-1 weighting), with **checksum-verified OCR repair** — common misreads (`B`↔`8`, `O`↔`0`, filler runs read as `K`/`L`, dropped or hallucinated characters) are corrected by generating candidate readings and letting the composite check digit prove which one matches the printed zone. A valid composite is mathematical proof of a faithful read; a failed one flags a bad scan or a tampered document. When Tier 1 validates, the LLM never runs: extraction is instant, deterministic and hallucination-free.

**[Try the live demo →](https://ruledicaprio.github.io/docs-to-md/)** The same Rust code compiled to WebAssembly, with tesseract.js OCR, on a static GitHub Pages site. **No data is persistent on any server — there is no server.** Images are downscaled and processed entirely inside your browser tab; the extracted JSON is shown for 10 seconds with a copy button, then wiped.

## 🚀 Quickstart

**1. Start the OCR engine** (Docker):
```powershell
docker run -d --name docling-serve -p 5001:5001 `
  -e OMP_NUM_THREADS=4 -e MKL_NUM_THREADS=4 `
  ghcr.io/docling-project/docling-serve
```

**2. Set up the Python sidecar:**
```powershell
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -r requirements.txt                # CPU-only
# or, with NVIDIA GPU acceleration (prebuilt CUDA 12.4 wheel):
pip install llama-cpp-python --extra-index-url https://abetlen.github.io/llama-cpp-python/whl/cu124
```

**3. Download the model** (~1 GB, not tracked in git):
```powershell
curl -L -o qwen2.5-1.5b-instruct-q4_k_m.gguf `
  https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf
```

**4. Run** (from the repo root):
```powershell
# CLI — one-shot extraction:
cargo run -p docling-client -- samples/Croatian_passport_data_page.jpg

# Web app — upload page + JSON API on http://127.0.0.1:8080
cargo run -p docling-app
```

```powershell
# API example:
curl -F "file=@samples/Passport_of_Serbia_ID_2009_version.jpg" http://127.0.0.1:8080/api/extract
```

> **Windows note:** the pre-compiled CUDA `llama-cpp-python` bindings require the Microsoft Visual C++ Redistributable.

## 📁 Repository Layout

```
├── mrz/              Zero-dep ICAO 9303 MRZ parser + checksum-verified OCR repair
├── mrz-wasm/         wasm-bindgen wrapper for the browser demo
├── pipeline/         Shared Rust library: OCR → md → Tier 1 MRZ → Tier 2 LLM → JSON
├── docling-client/   CLI front-end (thin wrapper)
├── docling-app/      axum web app: upload page + POST /api/extract (thin wrapper)
├── web/              GitHub Pages demo site (static, client-side only)
├── extract_json.py   Python sidecar: Qwen 2.5 GGUF → structured JSON
├── requirements.txt  Sidecar dependencies
├── samples/          Public-domain specimen documents + example outputs
└── docs/             Architectural manifest & roadmap
```

## 🔒 Security

Everything runs on loopback; the web app binds `127.0.0.1` by default and ships **without authentication**. If you expose it (`BIND_ADDR=0.0.0.0:8080`), put a reverse proxy with TLS + auth in front — it processes identity documents. Uploaded files and intermediate artifacts are deleted after each request. Details and the v0.3.0 roadmap (deterministic ICAO 9303 MRZ validation, payload encryption) live in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
