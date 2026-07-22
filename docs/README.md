# Docs index

Start at the top-level [README.md](../README.md) for install/usage. This folder holds everything
that's too long-form for there.

## Current & forward-looking

- **[VISION.md](VISION.md)** — the dual mission (technical sovereignty + compliance-by-design)
  and long-term direction.
- **[ROADMAP.md](ROADMAP.md)** — the authoritative M1–M6 milestone spine, Definition of Done per
  phase, what's shipped vs. planned. If another doc's roadmap section disagrees with this one,
  this one wins.
- **[BRANDING.md](BRANDING.md)** — the Identra/SynthPass naming model, messaging guardrails,
  commercial tiers.
- **[ARCHITECTURE.md](ARCHITECTURE.md)** — engineering rationale, trade-offs, and the
  version-by-version design history (why Tier 1/Tier 2 exist, what got deleted and when).
- **[LICENSING.md](LICENSING.md)** — the full customer (`fingerprint` → `verify-license`) and
  vendor (`keygen` → `issue-license`) CLI walkthrough for offline licensing.
- **[CORPUS_COVERAGE.md](CORPUS_COVERAGE.md)** — per-country OCR corpus status and the checklist
  for adding a new specimen.
- **[SYNTHPASS.md](SYNTHPASS.md)** — how to run the `synthpass-bench` corpus runner locally, its
  CLI flags, report format, and how the CI accuracy gate works (M4).
- **[ADVERSARIAL.md](ADVERSARIAL.md)** — the degraded capture profiles (mobile/scanner/worn/
  border-kiosk) as the adversarial/stress corpus: what each simulates, and gate status.

## Historical / origin records

`FOUNDATIONAL_STRATEGY.md` and `synthpass_v2_0.md` are the source notes VISION/ROADMAP/BRANDING
were distilled from. (The earlier `rebranding_identra_synthpass.md` and
`mlis_v2_0_0_preliminary_design.md` scratch notes and the `REBRAND_MIGRATION.md` execution record
have since been removed now that the rename is long complete and folded into `CHANGELOG.md`.)
Kept for provenance; not maintained as living docs — prefer the current docs above when they
disagree.

Project-wide (not in this folder): [CONTRIBUTING.md](../CONTRIBUTING.md),
[SECURITY.md](../SECURITY.md), [CHANGELOG.md](../CHANGELOG.md).
