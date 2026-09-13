// Author: Lukas Bower
// Purpose: Enforce honest attestation availability before publishing ticket authority.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

//! Public manifest measurements never substitute for an admitted device issuer.

use crate::generated::{self, AttestationMode};
use heapless::String;

/// Public metadata carrying no device or ticket-key assurance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    /// Selected resolved-manifest identity.
    pub manifest_sha256: String<64>,
}

/// Deterministic pre-authority policy failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttestationError {
    /// Malformed public measurement.
    InvalidManifestHash,
    /// Required signed evidence cannot be supplied.
    RequiredEvidenceUnavailable,
    /// A configured signing mode has no admitted isolated device issuer.
    DeviceProviderUnavailable,
    /// A bounded status cannot be encoded.
    StatusOverflow,
}

impl AttestationError {
    /// Stable failure token for emergency serial diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidManifestHash => "invalid-manifest-hash",
            Self::RequiredEvidenceUnavailable => "required-signed-evidence-unavailable",
            Self::DeviceProviderUnavailable => "attestation-device-provider-unavailable",
            Self::StatusOverflow => "attestation-status-bound",
        }
    }
}

/// Fail required/signed modes before generated development ticket registration.
pub fn evaluate(
    hardware: generated::HardwareConfig,
    manifest_sha256: &str,
) -> Result<Option<Measurement>, AttestationError> {
    let config = hardware.attestation;
    if config.required {
        return Err(AttestationError::RequiredEvidenceUnavailable);
    }
    match config.mode {
        AttestationMode::Disabled => Ok(None),
        AttestationMode::Tpm2Quote | AttestationMode::DiceEvidence => {
            Err(AttestationError::DeviceProviderUnavailable)
        }
        AttestationMode::MeasurementOnly => {
            if manifest_sha256.len() != 64
                || !manifest_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(AttestationError::InvalidManifestHash);
            }
            let manifest_sha256 = String::try_from(manifest_sha256)
                .map_err(|_| AttestationError::InvalidManifestHash)?;
            Ok(Some(Measurement { manifest_sha256 }))
        }
    }
}

/// Stable implementation mode; no automatic TPM/DICE fallback exists.
pub const fn mode_label(mode: AttestationMode) -> &'static str {
    match mode {
        AttestationMode::Disabled => "disabled",
        AttestationMode::MeasurementOnly => "measurement_only",
        AttestationMode::Tpm2Quote => "tpm2_quote",
        AttestationMode::DiceEvidence => "dice_evidence",
    }
}

/// The selected runtime has no device issuer; discovery must make this explicit.
pub const CAPABILITIES: &str = "{\"schema\":\"cohesix-attestation-capabilities/v1\",\"signed_evidence\":false,\"challenge\":false,\"reason\":\"device-provider-unavailable\"}";

/// Bounded status never labels public measurements as a TPM/DICE signature.
pub fn status() -> Result<String<256>, AttestationError> {
    use core::fmt::Write as _;
    let mut out = String::new();
    write!(out, "{{\"schema\":\"cohesix-attestation-status/v1\",\"mode\":\"{}\",\"required\":{},\"signed_evidence\":false,\"ticket_keys\":\"development_static\"}}",
        mode_label(generated::HARDWARE_CONFIG.attestation.mode),
        generated::HARDWARE_CONFIG.attestation.required)
        .map_err(|_| AttestationError::StatusOverflow)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn measurements_cannot_satisfy_required_attestation() {
        let mut hardware = generated::HARDWARE_CONFIG;
        hardware.attestation.mode = AttestationMode::MeasurementOnly;
        hardware.attestation.required = false;
        assert_eq!(
            evaluate(hardware, HASH).unwrap().unwrap().manifest_sha256,
            HASH
        );
        hardware.attestation.required = true;
        assert_eq!(
            evaluate(hardware, HASH),
            Err(AttestationError::RequiredEvidenceUnavailable)
        );
    }

    #[test]
    fn disabled_and_missing_device_modes_are_distinct() {
        let mut hardware = generated::HARDWARE_CONFIG;
        hardware.attestation.required = false;
        hardware.attestation.mode = AttestationMode::Disabled;
        assert_eq!(evaluate(hardware, HASH), Ok(None));
        for mode in [AttestationMode::Tpm2Quote, AttestationMode::DiceEvidence] {
            hardware.attestation.mode = mode;
            assert_eq!(
                evaluate(hardware, HASH),
                Err(AttestationError::DeviceProviderUnavailable)
            );
        }
    }

    #[test]
    fn measurement_input_is_canonical_and_bounded() {
        let mut hardware = generated::HARDWARE_CONFIG;
        hardware.attestation.required = false;
        hardware.attestation.mode = AttestationMode::MeasurementOnly;
        for hash in [
            "",
            "abc",
            "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
        ] {
            assert_eq!(
                evaluate(hardware, hash),
                Err(AttestationError::InvalidManifestHash)
            );
        }
    }
}
