//! Machine fingerprint for optionally binding a license to one installation.
//!
//! **Threat model, stated plainly (not oversold):** this binds to an OS
//! *installation*, not physically to hardware. A root user can read/copy the
//! underlying identifier, it survives a full disk clone, and it says nothing
//! about the CPU/motherboard. This deters casual license-sharing and
//! produces a compliance artifact — it is not DRM and isn't tamper-proof.
//! True hardware attestation would need a TPM/HSM, out of scope here.
//!
//! Deliberately does NOT use `sysinfo`(OS name + hostname + CPU brand): the
//! hostname is trivially changed and the CPU brand string is identical
//! across thousands of same-SKU machines, so two identical servers would
//! hash the same and one license would unlock both. `/etc/machine-id` is a
//! real, stable, per-install 128-bit identifier — far more unique.

use mlis_core::audit::sha256_hex;

/// A stable per-machine fingerprint, hex-encoded SHA-256 of the best
/// identifier this platform exposes. Binding to a license is optional (an
/// empty `hw_fingerprint` in the payload skips the check entirely), so this
/// never needs to be perfect — only good enough to deter casual sharing.
pub fn machine_fingerprint() -> String {
    sha256_hex(raw_identifier().as_bytes())
}

#[cfg(target_os = "linux")]
fn raw_identifier() -> String {
    for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(id) = std::fs::read_to_string(path) {
            let id = id.trim();
            if !id.is_empty() {
                return id.to_string();
            }
        }
    }
    "unbound-dev-linux-no-machine-id".to_string()
}

#[cfg(target_os = "windows")]
fn raw_identifier() -> String {
    // Best-effort, dev-only: the musl target this scheme is actually
    // designed for is Linux (see module docs); Windows/macOS builds only
    // need something stable enough for local testing.
    std::env::var("COMPUTERNAME")
        .map(|c| format!("windows-dev-{c}"))
        .unwrap_or_else(|_| "unbound-dev-windows".to_string())
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn raw_identifier() -> String {
    "unbound-dev-unknown-platform".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_stable_across_calls() {
        assert_eq!(machine_fingerprint(), machine_fingerprint());
    }

    #[test]
    fn is_a_sha256_hex_string() {
        let fp = machine_fingerprint();
        assert_eq!(fp.len(), 64);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
