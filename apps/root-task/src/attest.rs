// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Evaluate manifest-driven device identity attestation policy.
// Author: Lukas Bower

//! Deterministic attestation policy evaluation.
//!
//! Milestone 26 requires boot-time attestation policy checks before tickets
//! are published. This module intentionally provides a bounded, deterministic
//! evaluation path that does not depend on host networking.

use crate::generated::{self, AttestationPolicy, HardwareDeviceKind};
use core::fmt::Write as _;
use heapless::String;
use sha2::{Digest, Sha256};

const SHA256_HEX_BYTES: usize = 64;

/// Attestation back-end selected by policy/device availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttestationMethod {
    /// TPM-backed attestation path.
    Tpm,
    /// DICE fallback path.
    Dice,
}

impl AttestationMethod {
    /// Stable label used in audit lines.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tpm => "tpm",
            Self::Dice => "dice",
        }
    }
}

/// Deterministic attestation evidence snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationEvidence {
    /// Runtime method selected from manifest policy.
    pub method: AttestationMethod,
    /// Manifest policy used during evaluation.
    pub policy: AttestationPolicy,
    /// Manifest fingerprint this evidence is bound to.
    pub manifest_sha256: String<SHA256_HEX_BYTES>,
    /// Evidence digest emitted to boot diagnostics.
    pub evidence_sha256: String<SHA256_HEX_BYTES>,
}

/// Attestation policy evaluation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttestationError {
    /// Manifest fingerprint was not a canonical 64-byte lowercase/uppercase hex string.
    InvalidManifestHash,
    /// Policy requires TPM but no TPM device declaration is present.
    TpmUnavailable,
    /// Internal bounded string formatting failed.
    FormatOverflow,
}

impl AttestationError {
    /// Stable error token for audited boot diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidManifestHash => "invalid-manifest-hash",
            Self::TpmUnavailable => "tpm-unavailable",
            Self::FormatOverflow => "format-overflow",
        }
    }
}

/// Evaluate attestation policy from generated hardware config.
///
/// Returns:
/// - `Ok(None)` when attestation is disabled.
/// - `Ok(Some(_))` with deterministic evidence when enabled.
/// - `Err(_)` when required policy guarantees cannot be satisfied.
pub fn evaluate(
    hardware: generated::HardwareConfig,
    manifest_sha256: &str,
) -> Result<Option<AttestationEvidence>, AttestationError> {
    let config = hardware.attestation;
    if !config.enabled {
        return Ok(None);
    }
    if !is_sha256_hex(manifest_sha256) {
        return Err(AttestationError::InvalidManifestHash);
    }

    let has_tpm = hardware
        .devices
        .iter()
        .any(|device| device.kind == HardwareDeviceKind::Tpm);
    let method = match config.policy {
        AttestationPolicy::TpmOnly => {
            if !has_tpm {
                return Err(AttestationError::TpmUnavailable);
            }
            AttestationMethod::Tpm
        }
        AttestationPolicy::TpmOrDice => {
            if has_tpm {
                AttestationMethod::Tpm
            } else {
                AttestationMethod::Dice
            }
        }
        AttestationPolicy::DiceOnly => AttestationMethod::Dice,
    };

    let mut seed = String::<96>::new();
    write!(
        &mut seed,
        "{}:{}",
        attestation_policy_label(config.policy),
        manifest_sha256
    )
    .map_err(|_| AttestationError::FormatOverflow)?;
    let digest = Sha256::digest(seed.as_bytes());

    let mut manifest_hash = String::<SHA256_HEX_BYTES>::new();
    manifest_hash
        .push_str(manifest_sha256)
        .map_err(|_| AttestationError::FormatOverflow)?;

    let mut evidence_hash = String::<SHA256_HEX_BYTES>::new();
    encode_hex(&digest, &mut evidence_hash)?;

    Ok(Some(AttestationEvidence {
        method,
        policy: config.policy,
        manifest_sha256: manifest_hash,
        evidence_sha256: evidence_hash,
    }))
}

fn encode_hex(bytes: &[u8], out: &mut String<SHA256_HEX_BYTES>) -> Result<(), AttestationError> {
    for byte in bytes {
        write!(out, "{byte:02x}").map_err(|_| AttestationError::FormatOverflow)?;
    }
    Ok(())
}

/// Stable policy label used in boot diagnostics.
#[must_use]
pub const fn attestation_policy_label(policy: AttestationPolicy) -> &'static str {
    match policy {
        AttestationPolicy::TpmOnly => "tpm-only",
        AttestationPolicy::TpmOrDice => "tpm-or-dice",
        AttestationPolicy::DiceOnly => "dice-only",
    }
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == SHA256_HEX_BYTES && value.as_bytes().iter().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::{AttestationConfig, HardwareConfig, LocalSeatConfig};

    #[test]
    fn disabled_attestation_returns_none() {
        let hw = HardwareConfig {
            secure_boot: false,
            no_nic: false,
            attestation: AttestationConfig {
                enabled: false,
                policy: AttestationPolicy::TpmOrDice,
                evidence_max_bytes: 256,
            },
            local_seat: LocalSeatConfig {
                enabled: false,
                required: false,
                keyboard_device: "kbd0",
                display_device: "hdmi0",
                line_bytes: 160,
                buffer_lines: 128,
            },
            devices: &[],
        };

        let evidence = evaluate(
            hw,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect("disabled attestation should not fail");
        assert!(evidence.is_none());
    }

    #[test]
    fn tpm_only_requires_declared_tpm() {
        let hw = HardwareConfig {
            secure_boot: false,
            no_nic: false,
            attestation: AttestationConfig {
                enabled: true,
                policy: AttestationPolicy::TpmOnly,
                evidence_max_bytes: 256,
            },
            local_seat: LocalSeatConfig {
                enabled: false,
                required: false,
                keyboard_device: "kbd0",
                display_device: "hdmi0",
                line_bytes: 160,
                buffer_lines: 128,
            },
            devices: &[],
        };

        let err = evaluate(
            hw,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect_err("tpm-only without TPM must fail");
        assert_eq!(err, AttestationError::TpmUnavailable);
    }
}
