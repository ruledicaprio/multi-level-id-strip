# Third-Party Notices

`multi-level-id-strip` (MIT) bundles and builds on third-party open-source components.
All are permissively licensed (MIT / Apache-2.0 / BSD / ISC) — no copyleft.

> This file is hand-curated and, as of v0.7.5, still not fully current for the native LLM/OCR
> deps added in v0.6.0/v0.7.0 (`llama-cpp-2`, `ocrs`, `rten`) — verify their licenses before
> relying on this list. Regenerate the authoritative, version-exact list before a release:
>
> ```bash
> cargo install cargo-about cargo-deny
> cargo about generate about.hbs > THIRD_PARTY_NOTICES.md
> cargo deny check licenses           # fail the build on any surprise copyleft
> ```
>
> The legacy Python inferer sidecar (`grpcio`, `pydantic`, `llama-cpp-python`) and `docling_rs`
> were removed in v0.7.5 along with the gRPC backend and the `docling-serve` OCR engine — no
> Python dependencies remain in this project at all.

## Rust crates

| License | Crates (direct) |
| --- | --- |
| MIT OR Apache-2.0 | `tokio`, `axum`, `axum-server`, `tower`, `tower-http`, `hyper`, `hyper-util`, `serde`, `serde_json`, `rustls`, `image`, `thiserror`, `async-trait`, `uuid`, `getrandom` |
| MIT OR Apache-2.0 (RustCrypto) | `sha2`, `aes-gcm`, `base64` |
| ISC AND (Apache-2.0 OR ISC) AND OpenSSL | `aws-lc-rs` / `aws-lc-sys` (default rustls crypto provider) |
| Apache-2.0 (wraps C libs) | `leptess` → **Tesseract** (Apache-2.0) + **Leptonica** (BSD-2-Clause) |
| *(unverified — see note above)* | `llama-cpp-2` → **llama.cpp**, `ocrs`, `rten` |
| MIT / Apache-2.0 | `wasm-bindgen` (WASM demo) |

The Rust toolchain and `std` are dual MIT/Apache-2.0 (© The Rust Project Developers).

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
