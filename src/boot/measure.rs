// CLASSIFICATION: PRIVATE
// Filename: measure.rs · Cohesix boot module
// Date Modified: 2025-05-31
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Secure‑boot measurement helpers
//
// Provides a minimal TPM‑style PCR extension primitive using
// SHA‑256.  No direct TPM interaction is performed here; callers
// are responsible for persisting PCR slots or forwarding them to
// hardware.
//
// # Public API
// * [`extend_pcr`] – in‑place SHA‑256 extension of a 32‑byte PCR.
// ─────────────────────────────────────────────────────────────

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use sha2::{Digest, Sha256};

/// Extend a 32‑byte Platform Configuration Register **in place**.
///
/// Pseudocode: `PCR := SHA256(PCR || data)`
pub fn extend_pcr(pcr: &mut [u8; 32], data: &[u8]) {
    let mut hasher = Sha256::new();
    hasher.update(pcr);
    hasher.update(data);
    *pcr = hasher.finalize().into();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcr_is_not_zero_after_extend() {
        let mut pcr = [0u8; 32];
        extend_pcr(&mut pcr, b"cohesix");
        assert!(pcr.iter().any(|&b| b != 0));
    }

    #[test]
    fn second_extend_changes_value() {
        let mut pcr = [0u8; 32];
        extend_pcr(&mut pcr, b"first");
        let first = pcr;
        extend_pcr(&mut pcr, b"second");
        assert_ne!(pcr, first);
    }
}
