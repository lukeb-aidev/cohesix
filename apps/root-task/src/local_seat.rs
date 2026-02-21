// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Enforce manifest-gated local diagnostics seat policy and bounds.
// Author: Lukas Bower

//! Local diagnostics seat policy helpers (Milestone 26).

use crate::console::{Command, CommandParser, ConsoleError};
use crate::generated::{self, HardwareDeviceKind};

/// Deterministic local-seat initialisation outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSeatInit {
    /// Local seat not requested by manifest.
    Disabled,
    /// Local seat is active and can mirror I/O.
    Active(LocalSeatStatus),
    /// Manifest allowed degradation to serial-only diagnostics.
    Degraded(LocalSeatDegradedReason),
}

/// Local-seat readiness details when active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalSeatStatus {
    /// Declared keyboard device identifier.
    pub keyboard_device: &'static str,
    /// Declared display device identifier.
    pub display_device: &'static str,
    /// Maximum mirrored line width in bytes.
    pub line_bytes: u16,
    /// Ring depth for mirrored lines.
    pub buffer_lines: u16,
}

/// Non-fatal degraded modes when `required=false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSeatDegradedReason {
    /// Manifest keyboard declaration is missing or mismatched.
    MissingKeyboard,
    /// Manifest display declaration is missing or mismatched.
    MissingDisplay,
    /// Runtime backend for USB keyboard/HDMI text is unavailable.
    BackendUnavailable,
}

impl LocalSeatDegradedReason {
    /// Stable diagnostic token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingKeyboard => "missing-keyboard",
            Self::MissingDisplay => "missing-display",
            Self::BackendUnavailable => "backend-unavailable",
        }
    }
}

/// Fatal local-seat error for `required=true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSeatError {
    /// Manifest keyboard declaration is missing or mismatched.
    MissingKeyboard,
    /// Manifest display declaration is missing or mismatched.
    MissingDisplay,
    /// Runtime backend for USB keyboard/HDMI text is unavailable.
    BackendUnavailable,
}

impl LocalSeatError {
    /// Stable diagnostic token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingKeyboard => "missing-keyboard",
            Self::MissingDisplay => "missing-display",
            Self::BackendUnavailable => "backend-unavailable",
        }
    }
}

/// Evaluate local-seat initialisation policy.
///
/// `backend_available` should reflect whether the runtime has a HAL-backed local
/// seat implementation for this platform profile.
pub fn evaluate(
    hardware: generated::HardwareConfig,
    backend_available: bool,
) -> Result<LocalSeatInit, LocalSeatError> {
    let config = hardware.local_seat;
    if !config.enabled {
        return Ok(LocalSeatInit::Disabled);
    }

    let has_keyboard = hardware.devices.iter().any(|device| {
        device.kind == HardwareDeviceKind::Keyboard && device.id == config.keyboard_device
    });
    if !has_keyboard {
        return missing(config.required, LocalSeatError::MissingKeyboard);
    }

    let has_display = hardware.devices.iter().any(|device| {
        device.kind == HardwareDeviceKind::Display && device.id == config.display_device
    });
    if !has_display {
        return missing(config.required, LocalSeatError::MissingDisplay);
    }

    if !backend_available {
        return missing(config.required, LocalSeatError::BackendUnavailable);
    }

    Ok(LocalSeatInit::Active(LocalSeatStatus {
        keyboard_device: config.keyboard_device,
        display_device: config.display_device,
        line_bytes: config.line_bytes,
        buffer_lines: config.buffer_lines,
    }))
}

fn missing(required: bool, error: LocalSeatError) -> Result<LocalSeatInit, LocalSeatError> {
    if required {
        Err(error)
    } else {
        let reason = match error {
            LocalSeatError::MissingKeyboard => LocalSeatDegradedReason::MissingKeyboard,
            LocalSeatError::MissingDisplay => LocalSeatDegradedReason::MissingDisplay,
            LocalSeatError::BackendUnavailable => LocalSeatDegradedReason::BackendUnavailable,
        };
        Ok(LocalSeatInit::Degraded(reason))
    }
}

/// Feed keyboard bytes through the canonical root-console parser.
///
/// This intentionally shares the same parser implementation used by serial/TCP
/// paths so local-seat input does not introduce a new grammar surface.
pub fn feed_keyboard_bytes(
    parser: &mut CommandParser,
    bytes: &[u8],
) -> Result<Option<Command>, ConsoleError> {
    for &byte in bytes {
        if let Some(command) = parser.push_byte(byte)? {
            return Ok(Some(command));
        }
    }
    Ok(None)
}

/// Truncate a mirrored display line to the manifest-declared byte bound.
#[must_use]
pub fn truncate_for_display(line: &str, line_bytes: u16) -> &str {
    let limit = usize::from(line_bytes);
    if line.len() <= limit {
        return line;
    }
    let mut idx = limit;
    while idx > 0 && !line.is_char_boundary(idx) {
        idx -= 1;
    }
    &line[..idx]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::{
        AttestationConfig, AttestationPolicy, HardwareConfig, HardwareDevice, LocalSeatConfig,
    };

    const KEYBOARD: HardwareDevice = HardwareDevice {
        kind: HardwareDeviceKind::Keyboard,
        id: "usb-kbd0",
        required: true,
    };
    const DISPLAY: HardwareDevice = HardwareDevice {
        kind: HardwareDeviceKind::Display,
        id: "hdmi0",
        required: true,
    };
    const DEVICES_KEYBOARD_DISPLAY: [HardwareDevice; 2] = [KEYBOARD, DISPLAY];

    fn local_seat_hw(required: bool, devices: &'static [HardwareDevice]) -> HardwareConfig {
        HardwareConfig {
            secure_boot: false,
            no_nic: false,
            attestation: AttestationConfig {
                enabled: false,
                policy: AttestationPolicy::TpmOrDice,
                evidence_max_bytes: 256,
            },
            local_seat: LocalSeatConfig {
                enabled: true,
                required,
                keyboard_device: "usb-kbd0",
                display_device: "hdmi0",
                line_bytes: 16,
                buffer_lines: 8,
            },
            devices,
        }
    }

    #[test]
    fn required_local_seat_fails_without_backend() {
        let err = evaluate(local_seat_hw(true, &DEVICES_KEYBOARD_DISPLAY), false)
            .expect_err("required local seat must fail when backend is unavailable");
        assert_eq!(err, LocalSeatError::BackendUnavailable);
    }

    #[test]
    fn optional_local_seat_degrades_without_backend() {
        let state = evaluate(local_seat_hw(false, &DEVICES_KEYBOARD_DISPLAY), false)
            .expect("optional local seat should degrade");
        assert_eq!(
            state,
            LocalSeatInit::Degraded(LocalSeatDegradedReason::BackendUnavailable)
        );
    }

    #[test]
    fn keyboard_input_uses_canonical_parser() {
        let mut parser = CommandParser::new();
        let command = feed_keyboard_bytes(&mut parser, b"help\n")
            .expect("help command must parse")
            .expect("help should yield a command");
        assert_eq!(command, Command::Help);
    }

    #[test]
    fn mirror_truncation_respects_configured_bound() {
        let truncated = truncate_for_display("0123456789abcdef", 8);
        assert_eq!(truncated, "01234567");
    }
}
