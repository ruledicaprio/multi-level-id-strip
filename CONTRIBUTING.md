# Contributing

Thanks for your interest in `multi-level-id-strip` (mlis). This is a Rust workspace with a Python
inference sidecar and a WASM browser demo.

## Layout

```
crates/mrz          zero-dep ICAO 9303 engine (TD1/TD2/TD3, checksum-verified OCR repair)
crates/mrz-wasm     wasm-bindgen wrapper for the GitHub Pages demo
crates/mlis-core    canonical Extraction schema + Tier-3 audit/crypto (feature `security`)
crates/mlis-pipeline  OCR trait → Tier 1 MRZ → Tier 2 gRPC inferer → JSON
crates/mlis-cli     CLI (binary `mlis`, + `mlis decrypt`)
crates/mlis-serve   axum web app (auth, TLS)
crates/ocr-daemon   native Tesseract+Leptonica OCR engine (Linux/WSL only)
proto/              inferer.proto — the gRPC contract for Rust + Python
python/inferer      persistent warm LLM sidecar (grpcio)
```

## Building & testing

The default toolchain target is `x86_64-pc-windows-msvc`; if you don't have the MSVC linker (or on
any OS), the simplest reproducible path is a Linux Rust container:

```bash
# needs: cmake, protobuf-compiler, and (for ocr-daemon) clang + libtesseract/libleptonica
cargo test --workspace                 # cross-platform crates
cargo test -p ocr-daemon               # native OCR (Linux/WSL, tesseract installed)
cargo build -p mlis-pipeline --features native-ocr   # wire in the native engine

# Python inferer (from ./python)
python generate_grpc.py && python smoke_test.py       # gRPC stubs + mock-mode smoke
```

`gRPC codegen` needs `protobuf-compiler` (Rust `tonic-build`) and `grpcio-tools` (Python). The
native `ocr-daemon` is **Linux/WSL only** and excluded from `default-members`; CI must not build it
on Windows/macOS.

## Guidelines

- **Tests are the contract.** The `mrz` crate ships ICAO specimen + real-OCR-noise vectors; keep
  them green and add one for any new format/repair path.
- **Match the surrounding style.** Run `cargo fmt` and `cargo clippy --workspace`; keep comment
  density and naming consistent with the file you're editing.
- **The `mrz` crate stays zero-dependency** (it compiles to `wasm32`); don't add runtime deps to it.
- **No PII in logs or fixtures.** Use the public-domain specimens in `samples/`.
- **One logical change per PR**, with a clear description. Reference issues where relevant.

## Git workflow

`main` is a protected branch: no direct pushes, no force-pushes, even for the repo owner. Every
change lands via a pull request:

```bash
git fetch origin && git merge --ff-only origin/main   # start from current main, every time
git checkout -b <topic>-branch
# make changes, commit
git push -u origin <topic>-branch
gh pr create
```

The PR must show 4 green required checks before the merge button unlocks: `Rust (Linux, incl.
native OCR)`, `Rust (macos-latest, default members)`, `Python inferer (gRPC smoke)`, and `Bridge
(Rust client -> real Python inferer)`. No review approval is required (solo maintainer), but a PR
and passing CI always are.

If you work from more than one machine, always `git fetch && git merge --ff-only origin/main`
(or `git pull --ff-only`) before branching off — this fails loudly instead of silently diverging
if another client already pushed work you don't have yet.

## Commit sign-off

Contributions are accepted under the project's [MIT license](LICENSE). By submitting a PR you certify
you have the right to contribute the code (DCO-style `Signed-off-by` is welcome but not required).
