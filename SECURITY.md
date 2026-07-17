# Security Policy

`multi-level-id-strip` processes **personally identifiable information** (passports, ID cards).
Security is a first-class concern; this document describes the posture and how to report issues.

## Supported versions

| Version | Supported |
| --- | --- |
| 0.4.x | ✅ |
| < 0.4 | ❌ |

## Reporting a vulnerability

Please report privately — **do not** open a public issue for a security bug.

- Email **rusmirskopljak@gmail.com** with a description, reproduction steps, and impact.
- Or use GitHub's [private vulnerability reporting](https://docs.github.com/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability) on the repository.

Expect an acknowledgment within a few days. Coordinated disclosure is appreciated; please allow
time for a fix before any public write-up.

## Security posture

- **Air-gapped by design.** No cloud calls in the processing path; all OCR and LLM inference run on
  the local host / loopback. No telemetry.
- **Loopback by default.** `mlis-serve` binds `127.0.0.1`. It **refuses a non-loopback bind unless
  `MLIS_TOKEN` is set**, and then enforces `Authorization: Bearer <token>` on every request.
- **Transport security.** Optional rustls TLS via `MLIS_TLS_CERT` / `MLIS_TLS_KEY`. When exposed
  beyond loopback, terminate TLS (directly or via a reverse proxy) and keep the bearer token secret.
- **PII hygiene.** Uploaded files and intermediate artifacts are deleted after each request
  (`KEEP_WORK=1` retains them only for local debugging).
- **At-rest options.** `MLIS_AUDIT_LOG` writes a **PII-free** SHA-256 audit trail (document
  fingerprint + method + timestamp, never names/numbers). `MLIS_KEY` (base64 32-byte AES-256-GCM)
  encrypts the output JSON to `<input>.json.enc`; read it back with `mlis decrypt`.
- **Deterministic core.** Tier 1 (ICAO 9303 MRZ) is checksum-verified math, not a model — no
  hallucinated identity fields when a valid MRZ is present.

## Hardening checklist for production

- [ ] Set a strong, unique `MLIS_TOKEN`.
- [ ] Enable TLS (`MLIS_TLS_*`) or front with a TLS-terminating reverse proxy.
- [ ] Set `MLIS_KEY` to encrypt outputs at rest; store the key in a secrets manager.
- [ ] Enable `MLIS_AUDIT_LOG` and ship the log to your SIEM.
- [ ] Keep the GGUF model and containers on trusted, access-controlled hosts.
- [ ] Run `cargo deny check advisories` / `pip-audit` in CI.
