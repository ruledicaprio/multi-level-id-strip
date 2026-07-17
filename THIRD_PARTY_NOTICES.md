# Third-Party Notices

`multi-level-id-strip` (MIT) bundles and builds on third-party open-source components.
All are permissively licensed (MIT / Apache-2.0 / BSD / ISC) — no copyleft.

> This file is hand-curated from the dependency set at v0.4.0. Regenerate the
> authoritative, version-exact list before a release:
>
> ```bash
> # Rust
> cargo install cargo-about cargo-deny
> cargo about generate about.hbs > THIRD_PARTY_NOTICES.md
> cargo deny check licenses           # fail the build on any surprise copyleft
>
> # Python (from ./python)
> pip install pip-licenses && pip-licenses --format=markdown
> ```

## Rust crates

| License | Crates (direct) |
| --- | --- |
| MIT OR Apache-2.0 | `tokio`, `tokio-stream`, `axum`, `axum-server`, `tower`, `tower-http`, `hyper`, `hyper-util`, `serde`, `serde_json`, `tonic`, `tonic-build`, `prost`, `rustls`, `image`, `thiserror`, `async-trait`, `uuid`, `getrandom` |
| MIT OR Apache-2.0 (RustCrypto) | `sha2`, `aes-gcm`, `base64` |
| ISC AND (Apache-2.0 OR ISC) AND OpenSSL | `aws-lc-rs` / `aws-lc-sys` (default rustls crypto provider) |
| Apache-2.0 (wraps C libs) | `leptess` → **Tesseract** (Apache-2.0) + **Leptonica** (BSD-2-Clause) |
| MIT | `docling_rs` |
| MIT / Apache-2.0 | `wasm-bindgen` (WASM demo) |

The Rust toolchain and `std` are dual MIT/Apache-2.0 (© The Rust Project Developers).

## Python packages (inferer sidecar)

| Package | License |
| --- | --- |
| `grpcio`, `grpcio-tools` | Apache-2.0 |
| `protobuf` | BSD-3-Clause |
| `pydantic` | MIT |
| `llama-cpp-python` → **llama.cpp** | MIT |

## Frontend & models

| Component | License / Attribution |
| --- | --- |
| `tesseract.js` (browser OCR, loaded via CDN in the demo) | Apache-2.0 |
| Qwen 2.5 1.5B Instruct GGUF (downloaded at runtime, not vendored) | Apache-2.0 © Alibaba Cloud |
| **OCR-B model `web/tessdata/mrz.traineddata`** | **BSD-3-Clause © DoubangoTelecom** — see below |

### DoubangoTelecom OCR-B trained data (BSD-3-Clause)

The vendored MRZ OCR model (`web/tessdata/mrz.traineddata`) is redistributed from
[DoubangoTelecom/tesseractMRZ](https://github.com/DoubangoTelecom/tesseractMRZ) under the
BSD-3-Clause license. The full license text is preserved at
[`web/tessdata/LICENSE`](web/tessdata/LICENSE) and attributed in the live-demo footer.

---

Apache-2.0 components require their `NOTICE` files (where present) to be preserved; the
generated `cargo about` / `pip-licenses` output covers the complete, verbatim texts.
