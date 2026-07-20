# Offline Licensing — CLI Walkthrough

Extraction (`mlis <file>` and `mlis-serve`'s `/api/extract`) requires an offline,
Ed25519-signed license — set once and checked with no network call. `mlis doctor`, `mlis
decrypt`, `mlis fingerprint`, and `mlis verify-license` all keep working without one (you need
`fingerprint` to get one in the first place). For local development, skip enforcement
entirely:

```powershell
$env:MLIS_LICENSE_SKIP = "1"
```

For the cryptographic design — signed-bytes format, `verify_strict`, the fingerprint scheme,
and the threat model stated plainly — see
[ARCHITECTURE.md §6](ARCHITECTURE.md#6-offline-cryptographic-licensing-v080). This document is
just the CLI steps.

## Customer flow

```powershell
cargo run -p mlis-cli -- fingerprint                    # send this string to your vendor
# ...vendor emails back license.mlis...
cargo run -p mlis-cli -- verify-license license.mlis     # confirm it before relying on it
```

## Vendor flow

The `mlis-license-issuer` binary (`vendor` feature, never shipped to customers):

```powershell
cargo run -p mlis-license --features vendor --bin mlis-license-issuer -- keygen
# keep the private key offline; embed the printed public key in crates/mlis-license/pubkey.b64

$env:MLIS_LICENSE_PRIVKEY = "<private key from keygen>"
cargo run -p mlis-license --features vendor --bin mlis-license-issuer -- `
  issue-license --customer "Acme Hospital" --tier enterprise --expires-in-days 365 `
  --hw <fingerprint from the customer> --out license.mlis
```

An empty `--hw` issues an unbound (site/trial) license instead of a machine-locked one.

## Configuration (environment)

| Variable | Default | Purpose |
| --- | --- | --- |
| `MLIS_LICENSE_PATH` | `license.mlis` | path to the signed license file |
| `MLIS_LICENSE_SKIP` | *(unset)* | `1` bypasses license enforcement entirely (local development/CI) |
| `MLIS_LICENSE_PUBKEY` | *(embedded)* | override the embedded verifying key (base64), for testing |
