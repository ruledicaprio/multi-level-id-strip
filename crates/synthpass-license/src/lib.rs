//! Offline cryptographic licensing: Ed25519-signed license files so a
//! shipped `synthpass`/`synthpass-serve` binary can be sold and metered for air-gapped
//! enterprise distribution without ever phoning home.
//!
//! **Threat model, stated plainly (matches this project's house style of
//! documenting limitations rather than overselling — see
//! `docs/ARCHITECTURE.md` §7):** the source is public, so anyone who
//! rebuilds from source can strip this check entirely. This meters and gates
//! the *official pre-built binary*, deters casual license-sharing, and
//! produces a compliance artifact — it is **not DRM** and is not sold as
//! tamper-proof. See [`fingerprint`] for the hardware-binding caveat
//! specifically.
//!
//! Format: a license file (`license.mlis`) is a small JSON envelope
//! ([`SignedLicense`]) whose `payload` field is the base64 of the *exact*
//! bytes that were signed. The verifier checks the signature over those
//! literal bytes and only deserializes into [`LicensePayload`] afterward —
//! so, unlike re-serializing the payload before verifying, a valid license
//! can never fail to verify due to field-order/whitespace drift between
//! signer and verifier.

mod fingerprint;
mod keys;
#[cfg(feature = "vendor")]
pub mod sign;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;
use std::str::FromStr;

pub use fingerprint::machine_fingerprint;

/// Single-document extraction — the core capability. Named here for
/// completeness (and so a preset can list it), but deliberately **never**
/// gated in `synthpass-serve`: `docs/BRANDING.md` §5 draws the paid boundary
/// at capacity, support, and enterprise-integration surfaces, never at the
/// core.
pub const FEATURE_EXTRACT: &str = "extract";
/// Batch submission / job endpoints — a capacity surface.
pub const FEATURE_BATCH: &str = "batch";
/// More than one concurrent LLM context — a capacity surface.
pub const FEATURE_MULTI_CONTEXT: &str = "multi-context";
/// Prometheus `/metrics` — the "enhanced reporting" surface.
pub const FEATURE_METRICS: &str = "metrics";

/// The commercial tiers of `docs/BRANDING.md` §5, ordered so that a higher
/// tier is a superset of a lower one.
///
/// The tier is *descriptive*: gating decisions are made per-feature by
/// [`check_feature`], because a license's `features` list is what the issuer
/// actually signed. `Tier` exists so the issuer can stamp a coherent preset
/// ([`Tier::default_features`]) in one step and so startup logging can say
/// something meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Trial,
    Pro,
    Enterprise,
}

impl Tier {
    /// The feature set an issuer stamps for this tier. Mirrors the tier table
    /// in `docs/BRANDING.md` §5: Professional adds capacity knobs, Enterprise
    /// adds reporting on top.
    pub fn default_features(self) -> Vec<String> {
        let names: &[&str] = match self {
            Self::Trial => &[FEATURE_EXTRACT],
            Self::Pro => &[FEATURE_EXTRACT, FEATURE_BATCH, FEATURE_MULTI_CONTEXT],
            Self::Enterprise => &[
                FEATURE_EXTRACT,
                FEATURE_BATCH,
                FEATURE_MULTI_CONTEXT,
                FEATURE_METRICS,
            ],
        };
        names.iter().map(|s| (*s).to_string()).collect()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trial => "trial",
            Self::Pro => "pro",
            Self::Enterprise => "enterprise",
        }
    }
}

impl FromStr for Tier {
    type Err = ();

    /// Lenient by design: licenses in the wild carry a free-form `tier`
    /// string, so parsing accepts the obvious spellings and is
    /// case/whitespace-insensitive. An unrecognised tier is an `Err` rather
    /// than a silent default — callers decide what to do with it, and none of
    /// them grant access on the strength of the tier alone.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "trial" | "eval" | "evaluation" => Ok(Self::Trial),
            "pro" | "professional" => Ok(Self::Pro),
            "enterprise" => Ok(Self::Enterprise),
            _ => Err(()),
        }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The terms of a license, signed as-is (see module docs on the byte-exact
/// signing scheme).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicensePayload {
    pub license_id: String,
    pub customer: String,
    /// Empty ⇒ unbound (site/trial license) — [`check`] skips the
    /// fingerprint comparison entirely when this is empty.
    #[serde(default)]
    pub hw_fingerprint: String,
    pub issued_unix: u64,
    pub expires_unix: u64,
    /// Free-form on the wire; parse with [`Tier`] when a structured view is
    /// wanted. Descriptive only — [`check_feature`] gates on `features`.
    pub tier: String,
    /// The gateable surfaces this license unlocks. **Empty means all of
    /// them** — see [`check_feature`] on grandfathering.
    #[serde(default)]
    pub features: Vec<String>,
    /// A license can refuse to unlock a `synthpass` build older than it was
    /// issued for. [`check`] enforces this against `CARGO_PKG_VERSION` when
    /// present.
    #[serde(default)]
    pub mlis_min_version: Option<String>,
    /// Optional cap on concurrent Tier-2 LLM contexts. The environment
    /// *asks*, the license *permits* — see [`effective_llm_contexts`].
    /// Absent ⇒ uncapped.
    #[serde(default)]
    pub max_llm_contexts: Option<usize>,
}

/// The on-disk license file format: `payload` is base64 of the *exact*
/// signed [`LicensePayload`] JSON bytes; `signature` is the base64 Ed25519
/// signature over those same bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedLicense {
    pub format: u32,
    pub payload: String,
    pub signature: String,
}

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug)]
pub enum LicenseError {
    Io(std::io::Error),
    /// The envelope (outer JSON) or its base64 fields didn't parse.
    Malformed(String),
    InvalidPublicKey,
    /// Signature didn't verify — tampered, corrupted, or signed by a
    /// different key entirely.
    InvalidSignature,
    /// Signature verified but the payload bytes underneath weren't valid
    /// `LicensePayload` JSON (shouldn't happen for a genuinely-issued
    /// license; would mean the issuer and verifier disagree on the schema).
    InvalidPayload(String),
    Expired {
        expires_unix: u64,
    },
    FingerprintMismatch,
    /// The running binary's `CARGO_PKG_VERSION` doesn't satisfy the
    /// license's `mlis_min_version`. Also raised (fail closed) when
    /// `mlis_min_version` itself isn't a parsable `major[.minor[.patch]]`
    /// string — an unreadable requirement is not a satisfied one.
    MinVersionUnmet {
        required: String,
    },
    /// The license verified fine but doesn't name the feature backing the
    /// surface being reached for. Fails closed and names the missing feature
    /// so the operator knows exactly what to ask their vendor for.
    FeatureNotLicensed {
        feature: String,
    },
}

impl fmt::Display for LicenseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "could not read license file: {e}"),
            Self::Malformed(e) => write!(f, "malformed license file: {e}"),
            Self::InvalidPublicKey => write!(f, "invalid embedded/override public key"),
            Self::InvalidSignature => {
                write!(f, "license signature is invalid (tampered or wrong key)")
            }
            Self::InvalidPayload(e) => write!(f, "license payload did not parse: {e}"),
            Self::Expired { expires_unix } => write!(f, "license expired at {expires_unix}"),
            Self::FingerprintMismatch => {
                write!(f, "license is bound to a different machine")
            }
            Self::MinVersionUnmet { required } => write!(
                f,
                "license requires synthpass >= {required} (running {})",
                env!("CARGO_PKG_VERSION")
            ),
            Self::FeatureNotLicensed { feature } => {
                write!(f, "license does not include the '{feature}' feature")
            }
        }
    }
}

impl std::error::Error for LicenseError {}

impl From<std::io::Error> for LicenseError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// A license that has passed signature verification (and, via
/// [`load_and_check`], expiry/fingerprint checks too).
#[derive(Debug, Clone)]
pub struct LicenseStatus {
    pub payload: LicensePayload,
}

impl LicenseStatus {
    /// Whole days remaining until expiry, relative to `now_unix`. Negative
    /// once expired (callers that reach this far already passed [`check`],
    /// so in practice this is used only for the "expires in N days" /
    /// "⚠️ expiring soon" UI, not as an expiry check itself).
    pub fn days_until_expiry(&self, now_unix: u64) -> i64 {
        (self.payload.expires_unix as i64 - now_unix as i64) / 86_400
    }
}

/// Verifies `signed`'s Ed25519 signature over its exact payload bytes
/// against `SYNTHPASS_LICENSE_PUBKEY` if set, else the embedded key (see
/// [`verify_with_key`] to verify against an explicit key instead — used by
/// this crate's own tests to avoid mutating the process-global env var,
/// which isn't safe across parallel test threads).
pub fn verify(signed: &SignedLicense) -> Result<LicensePayload, LicenseError> {
    verify_with_key(signed, &keys::verifying_key()?)
}

/// Verifies `signed`'s Ed25519 signature over its exact payload bytes
/// against an explicit key and, only once that succeeds, parses those bytes
/// as [`LicensePayload`]. Does not check expiry or fingerprint — see
/// [`check`] / [`load_and_check`].
pub fn verify_with_key(
    signed: &SignedLicense,
    key: &ed25519_dalek::VerifyingKey,
) -> Result<LicensePayload, LicenseError> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use ed25519_dalek::Signature;

    let payload_bytes = STANDARD
        .decode(&signed.payload)
        .map_err(|e| LicenseError::Malformed(format!("payload not valid base64: {e}")))?;
    let sig_bytes = STANDARD
        .decode(&signed.signature)
        .map_err(|e| LicenseError::Malformed(format!("signature not valid base64: {e}")))?;
    let sig_bytes: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| LicenseError::Malformed("signature must be 64 bytes".into()))?;
    let signature = Signature::from_bytes(&sig_bytes);

    // `verify_strict` (not the plain `Verifier::verify`) rejects
    // non-canonical/cofactored signature malleability — the conservative,
    // recommended default per RFC 8032, and cheap insurance for a scheme
    // this security-sensitive.
    key.verify_strict(&payload_bytes, &signature)
        .map_err(|_| LicenseError::InvalidSignature)?;

    serde_json::from_slice(&payload_bytes).map_err(|e| LicenseError::InvalidPayload(e.to_string()))
}

/// Expiry + (if bound) fingerprint + (if set) minimum-version checks against
/// an already-verified payload. Pure and deterministic — `now_unix`/
/// `fingerprint` are passed in rather than read from the clock/machine here,
/// so callers can test it exactly the way `synthpass-serve`'s existing
/// `startup_refusal` is tested.
pub fn check(
    payload: &LicensePayload,
    now_unix: u64,
    fingerprint: &str,
) -> Result<(), LicenseError> {
    if now_unix > payload.expires_unix {
        return Err(LicenseError::Expired {
            expires_unix: payload.expires_unix,
        });
    }
    if !payload.hw_fingerprint.is_empty() && payload.hw_fingerprint != fingerprint {
        return Err(LicenseError::FingerprintMismatch);
    }
    if let Some(required) = &payload.mlis_min_version {
        if !version_satisfies(env!("CARGO_PKG_VERSION"), required) {
            return Err(LicenseError::MinVersionUnmet {
                required: required.clone(),
            });
        }
    }
    Ok(())
}

/// `true` when `payload` predates feature gating and is therefore
/// grandfathered into every feature (design record §9, break B6).
///
/// Kept separate from [`check_feature`] so startup can log the fact **once**
/// rather than silently waving every request through.
pub fn features_grandfathered(payload: &LicensePayload) -> bool {
    payload.features.is_empty()
}

/// Does this license unlock `feature`?
///
/// Fails closed with [`LicenseError::FeatureNotLicensed`] — with one
/// deliberate exception: a payload with an **empty** `features` list is
/// grandfathered into everything, because licenses issued before gating
/// existed carry no list and must not stop working on upgrade. Those are
/// flagged by [`features_grandfathered`] so the operator is told once.
///
/// Pure, like [`check`]: no clock, no environment, no I/O.
pub fn check_feature(payload: &LicensePayload, feature: &str) -> Result<(), LicenseError> {
    if features_grandfathered(payload) || payload.features.iter().any(|f| f == feature) {
        return Ok(());
    }
    Err(LicenseError::FeatureNotLicensed {
        feature: feature.to_string(),
    })
}

/// Reconcile a *requested* Tier-2 context count with what the license
/// permits: the environment asks, the license caps, and the effective value
/// is the lesser of the two (floored at 1 — zero contexts would mean no
/// Tier-2 at all, which is a misconfiguration, not a license tier).
///
/// Returns `(effective, capped)`; `capped` is `true` when the license
/// actually lowered the request, so the caller can say so out loud instead of
/// leaving an operator wondering why their env var did nothing.
pub fn effective_llm_contexts(payload: &LicensePayload, requested: usize) -> (usize, bool) {
    let requested = requested.max(1);
    match payload.max_llm_contexts {
        Some(cap) if cap.max(1) < requested => (cap.max(1), true),
        _ => (requested, false),
    }
}

/// `true` iff `actual` is >= `required`, both `major[.minor[.patch]]`
/// (any missing component pads as `0`; a trailing `-pre`/`+build` suffix on
/// either string is stripped before comparing). Fails closed — an
/// unparsable `required` string (e.g. non-numeric components) is treated as
/// unmet, since an unreadable requirement can't be verified as satisfied.
fn version_satisfies(actual: &str, required: &str) -> bool {
    let Some(required) = parse_version(required) else {
        return false;
    };
    // `actual` is always this crate's own `CARGO_PKG_VERSION`, so a parse
    // failure there would mean a broken build, not a license problem —
    // still fail closed rather than panic.
    let Some(actual) = parse_version(actual) else {
        return false;
    };
    actual >= required
}

/// Parses the leading `major[.minor[.patch]]` numeric components of a
/// version string, ignoring any `-`/`+` suffix (pre-release/build
/// metadata). Missing trailing components default to `0`.
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.split(['-', '+']).next().unwrap_or(v);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().map(str::parse).transpose().ok()?.unwrap_or(0);
    let patch = parts.next().map(str::parse).transpose().ok()?.unwrap_or(0);
    Some((major, minor, patch))
}

/// Convenience used at CLI/serve startup: read `path`, verify the
/// signature, and run the expiry/fingerprint checks against the real clock
/// and this machine's real fingerprint.
pub fn load_and_check(path: &Path) -> Result<LicenseStatus, LicenseError> {
    let data = std::fs::read_to_string(path)?;
    let signed: SignedLicense =
        serde_json::from_str(&data).map_err(|e| LicenseError::Malformed(e.to_string()))?;
    let payload = verify(&signed)?;
    check(&payload, current_unix(), &machine_fingerprint())?;
    Ok(LicenseStatus { payload })
}

/// The current Unix timestamp, clamped to 0 on a pre-epoch clock. Exposed so
/// callers computing "days until expiry" for display, or a vendor issuer
/// stamping `issued_unix`, all agree on one clock-reading convention.
pub fn current_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};

    /// A deterministic test keypair from a fixed seed — avoids needing an
    /// RNG dependency (the `vendor` feature's `SigningKey::generate`) just
    /// for tests; `from_bytes` is plain key-material construction, no
    /// keygen involved.
    fn keypair_from_seed(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn sign_payload(signing_key: &SigningKey, payload: &LicensePayload) -> SignedLicense {
        let payload_bytes = serde_json::to_vec(payload).unwrap();
        let signature = signing_key.sign(&payload_bytes);
        SignedLicense {
            format: FORMAT_VERSION,
            payload: STANDARD.encode(&payload_bytes),
            signature: STANDARD.encode(signature.to_bytes()),
        }
    }

    fn sample_payload(hw_fingerprint: &str, expires_unix: u64) -> LicensePayload {
        LicensePayload {
            license_id: "test-license-1".into(),
            customer: "Test Customer".into(),
            hw_fingerprint: hw_fingerprint.into(),
            issued_unix: 1_700_000_000,
            expires_unix,
            tier: "enterprise".into(),
            features: vec!["extract".into()],
            mlis_min_version: None,
            max_llm_contexts: None,
        }
    }

    #[test]
    fn sign_verify_round_trips() {
        let signing_key = keypair_from_seed(1);
        let payload = sample_payload("", 4_000_000_000);
        let signed = sign_payload(&signing_key, &payload);

        let verified = verify_with_key(&signed, &signing_key.verifying_key())
            .expect("valid signature should verify");
        assert_eq!(verified, payload);
    }

    #[test]
    fn expired_license_fails_check() {
        let payload = sample_payload("", 100); // expired long ago
        let err = check(&payload, 200, "").expect_err("must reject expired license");
        assert!(matches!(err, LicenseError::Expired { expires_unix: 100 }));
    }

    #[test]
    fn wrong_fingerprint_fails_check() {
        let payload = sample_payload("machine-a", 4_000_000_000);
        let err = check(&payload, 1, "machine-b").expect_err("must reject fingerprint mismatch");
        assert!(matches!(err, LicenseError::FingerprintMismatch));
    }

    #[test]
    fn unbound_license_accepted_on_any_fingerprint() {
        let payload = sample_payload("", 4_000_000_000);
        check(&payload, 1, "any-machine-at-all").expect("empty hw_fingerprint skips the check");
    }

    #[test]
    fn absent_min_version_skips_the_check() {
        let payload = sample_payload("", 4_000_000_000); // mlis_min_version: None
        check(&payload, 1, "").expect("no min-version requirement means nothing to enforce");
    }

    #[test]
    fn satisfied_min_version_passes_check() {
        let payload = LicensePayload {
            mlis_min_version: Some("0.0.1".into()), // trivially satisfied by any real build
            ..sample_payload("", 4_000_000_000)
        };
        check(&payload, 1, "").expect("running version should satisfy a low minimum");
    }

    #[test]
    fn unmet_min_version_fails_check() {
        let payload = LicensePayload {
            mlis_min_version: Some("999.0.0".into()),
            ..sample_payload("", 4_000_000_000)
        };
        let err = check(&payload, 1, "").expect_err("must reject an unmet minimum version");
        assert!(matches!(
            err,
            LicenseError::MinVersionUnmet { required } if required == "999.0.0"
        ));
    }

    #[test]
    fn malformed_min_version_fails_closed() {
        let payload = LicensePayload {
            mlis_min_version: Some("not-a-version".into()),
            ..sample_payload("", 4_000_000_000)
        };
        let err = check(&payload, 1, "").expect_err("unparsable requirement must fail closed");
        assert!(matches!(err, LicenseError::MinVersionUnmet { .. }));
    }

    #[test]
    fn version_satisfies_compares_numeric_components() {
        assert!(
            version_satisfies("1.2.3", "1.2.3"),
            "equal versions satisfy"
        );
        assert!(
            version_satisfies("2.0.0", "1.9.9"),
            "greater major satisfies"
        );
        assert!(
            version_satisfies("1.3.0", "1.2.9"),
            "greater minor satisfies"
        );
        assert!(
            version_satisfies("1.2.4", "1.2.3"),
            "greater patch satisfies"
        );
        assert!(!version_satisfies("1.2.3", "1.2.4"), "lesser patch fails");
        assert!(!version_satisfies("1.1.9", "1.2.0"), "lesser minor fails");
        assert!(!version_satisfies("0.9.9", "1.0.0"), "lesser major fails");
        assert!(
            version_satisfies("1.2", "1.2.0"),
            "missing components pad as 0"
        );
        assert!(
            version_satisfies("1.2.0", "1.2.0-beta"),
            "pre-release suffix on the requirement is stripped before comparing"
        );
        assert!(
            !version_satisfies("not-a-version", "1.0.0"),
            "unparsable actual fails closed"
        );
        assert!(
            !version_satisfies("1.0.0", "not-a-version"),
            "unparsable required fails closed"
        );
    }

    #[test]
    fn listed_feature_is_allowed() {
        let payload = sample_payload("", 4_000_000_000); // features: ["extract"]
        check_feature(&payload, FEATURE_EXTRACT).expect("a listed feature is licensed");
    }

    #[test]
    fn unlisted_feature_fails_closed_and_names_itself() {
        let payload = sample_payload("", 4_000_000_000); // features: ["extract"] only
        let err =
            check_feature(&payload, FEATURE_BATCH).expect_err("an unlisted feature must fail");
        assert!(matches!(
            err,
            LicenseError::FeatureNotLicensed { ref feature } if feature == FEATURE_BATCH
        ));
        assert!(
            err.to_string().contains(FEATURE_BATCH),
            "the operator must be told which feature is missing: {err}"
        );
    }

    #[test]
    fn empty_feature_list_is_grandfathered_into_everything() {
        // Break B6: licenses issued before gating existed carry no `features`
        // list and must keep working across the upgrade.
        let payload = LicensePayload {
            features: vec![],
            ..sample_payload("", 4_000_000_000)
        };
        assert!(features_grandfathered(&payload));
        for feature in [
            FEATURE_EXTRACT,
            FEATURE_BATCH,
            FEATURE_MULTI_CONTEXT,
            FEATURE_METRICS,
        ] {
            check_feature(&payload, feature).expect("a feature-less license unlocks everything");
        }
    }

    #[test]
    fn a_populated_feature_list_is_not_grandfathered() {
        let payload = sample_payload("", 4_000_000_000);
        assert!(
            !features_grandfathered(&payload),
            "naming any feature opts the license into real gating"
        );
    }

    #[test]
    fn license_cap_lowers_the_requested_context_count() {
        let payload = LicensePayload {
            max_llm_contexts: Some(1),
            ..sample_payload("", 4_000_000_000)
        };
        assert_eq!(
            effective_llm_contexts(&payload, 4),
            (1, true),
            "the env asks for 4, the license permits 1"
        );
    }

    #[test]
    fn license_cap_never_raises_the_request() {
        let payload = LicensePayload {
            max_llm_contexts: Some(8),
            ..sample_payload("", 4_000_000_000)
        };
        assert_eq!(
            effective_llm_contexts(&payload, 2),
            (2, false),
            "a generous cap doesn't conjure contexts nobody asked for"
        );
    }

    #[test]
    fn absent_cap_leaves_the_request_alone() {
        let payload = sample_payload("", 4_000_000_000); // max_llm_contexts: None
        assert_eq!(effective_llm_contexts(&payload, 3), (3, false));
    }

    #[test]
    fn context_count_is_floored_at_one() {
        let zero_cap = LicensePayload {
            max_llm_contexts: Some(0),
            ..sample_payload("", 4_000_000_000)
        };
        // Zero contexts would mean no Tier 2 at all — a misconfiguration, not
        // a tier. Both sides of the min() floor at 1.
        assert_eq!(effective_llm_contexts(&zero_cap, 4), (1, true));
        assert_eq!(
            effective_llm_contexts(&sample_payload("", 4_000_000_000), 0),
            (1, false)
        );
    }

    #[test]
    fn tier_parses_leniently_and_orders() {
        assert_eq!(Tier::from_str("Trial "), Ok(Tier::Trial));
        assert_eq!(Tier::from_str("PROFESSIONAL"), Ok(Tier::Pro));
        assert_eq!(Tier::from_str("enterprise"), Ok(Tier::Enterprise));
        assert_eq!(Tier::from_str("platinum"), Err(()));
        assert!(Tier::Trial < Tier::Pro && Tier::Pro < Tier::Enterprise);
    }

    #[test]
    fn tier_presets_are_supersets_going_up() {
        let trial = Tier::Trial.default_features();
        let pro = Tier::Pro.default_features();
        let enterprise = Tier::Enterprise.default_features();

        assert!(trial.iter().all(|f| pro.contains(f)), "pro ⊇ trial");
        assert!(
            pro.iter().all(|f| enterprise.contains(f)),
            "enterprise ⊇ pro"
        );
        // The core capability is in every tier — BRANDING.md §5 draws the
        // paid boundary at capacity and reporting, never at the core.
        for tier in [Tier::Trial, Tier::Pro, Tier::Enterprise] {
            assert!(tier
                .default_features()
                .contains(&FEATURE_EXTRACT.to_string()));
        }
        // ...and the capacity/reporting surfaces are genuinely withheld from
        // the bottom tier, or the gate would be decorative.
        assert!(!trial.contains(&FEATURE_BATCH.to_string()));
        assert!(!pro.contains(&FEATURE_METRICS.to_string()));
    }

    #[test]
    fn new_optional_fields_deserialize_from_a_pre_gating_payload() {
        // A license issued before this change has neither `features` nor
        // `max_llm_contexts` in its signed bytes. Since verification checks
        // the *signature over those exact bytes* and only then deserializes,
        // the new fields must default rather than fail to parse.
        let json = r#"{
            "license_id": "old-1",
            "customer": "Legacy Customer",
            "hw_fingerprint": "",
            "issued_unix": 1700000000,
            "expires_unix": 4000000000,
            "tier": "enterprise"
        }"#;
        let payload: LicensePayload =
            serde_json::from_str(json).expect("a pre-gating payload still parses");
        assert!(payload.features.is_empty());
        assert_eq!(payload.max_llm_contexts, None);
        assert!(features_grandfathered(&payload));
    }

    #[test]
    fn tampered_payload_fails_closed() {
        let signing_key = keypair_from_seed(1);
        let payload = sample_payload("", 4_000_000_000);
        let mut signed = sign_payload(&signing_key, &payload);

        // Flip one byte of the (base64-decoded) payload.
        let mut raw = STANDARD.decode(&signed.payload).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0xff;
        signed.payload = STANDARD.encode(raw);

        let err = verify_with_key(&signed, &signing_key.verifying_key())
            .expect_err("tampered payload must fail closed");
        assert!(matches!(err, LicenseError::InvalidSignature));
    }

    #[test]
    fn tampered_signature_fails_closed() {
        let signing_key = keypair_from_seed(1);
        let payload = sample_payload("", 4_000_000_000);
        let mut signed = sign_payload(&signing_key, &payload);

        let mut raw = STANDARD.decode(&signed.signature).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0xff;
        signed.signature = STANDARD.encode(raw);

        let err = verify_with_key(&signed, &signing_key.verifying_key())
            .expect_err("tampered signature must fail closed");
        assert!(matches!(err, LicenseError::InvalidSignature));
    }

    #[test]
    fn signed_by_a_different_key_fails_closed() {
        let signing_key = keypair_from_seed(1);
        let other_signing_key = keypair_from_seed(2); // a different keypair entirely
        let payload = sample_payload("", 4_000_000_000);
        let signed = sign_payload(&signing_key, &payload);

        let err = verify_with_key(&signed, &other_signing_key.verifying_key())
            .expect_err("wrong verifying key must fail closed");
        assert!(matches!(err, LicenseError::InvalidSignature));
    }

    #[test]
    fn reserialization_cannot_desync_signer_and_verifier() {
        // Proves the canonical-bytes design: even if something re-serializes
        // the payload differently (e.g. a future serde_json version changes
        // formatting), verification still succeeds because the signature
        // covers the *exact stored bytes*, never a freshly re-serialized copy.
        let signing_key = keypair_from_seed(1);
        let payload = sample_payload("", 4_000_000_000);
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let signature = signing_key.sign(&payload_bytes);

        // Re-serialize independently (simulating formatting drift) and
        // confirm it's byte-different from the original, yet the ORIGINAL
        // signed bytes (not this re-serialized copy) are what verification
        // actually checks against.
        let reserialized = serde_json::to_string(&payload).unwrap().into_bytes();
        let signed = SignedLicense {
            format: FORMAT_VERSION,
            payload: STANDARD.encode(&payload_bytes), // the ORIGINAL bytes, not `reserialized`
            signature: STANDARD.encode(signature.to_bytes()),
        };

        verify_with_key(&signed, &signing_key.verifying_key())
            .expect("verification uses the stored bytes, not a re-serialization");
        // Sanity: to_vec and to_string+into_bytes happen to agree here, but
        // the point stands regardless — verify() never re-serializes at all.
        assert_eq!(payload_bytes, reserialized);
    }
}
