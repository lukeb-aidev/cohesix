// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Enforce manifest-gated local diagnostics seat policy and bounds.
// Author: Lukas Bower

//! Local diagnostics seat policy helpers (Milestone 26).

extern crate alloc;

#[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
use crate::bootstrap::log as boot_log;
use crate::console::{Command, CommandParser, ConsoleError};
use crate::generated::{self, HardwareDeviceKind};
#[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
use crate::local_seat_pi4::{Pi4FramebufferHint, Pi4LocalSeat, Pi4LocalSeatHints, Pi4SeatError};
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
#[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
static LOCAL_SEAT_POLL_LOGGED: AtomicBool = AtomicBool::new(false);
#[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
static LOCAL_SEAT_DATA_LOGGED: AtomicBool = AtomicBool::new(false);

/// Maximum number of queued keyboard bytes retained by the local-seat runtime.
pub const KEYBOARD_QUEUE_MAX_BYTES: usize = 4_096;

/// Maximum keyboard bytes drained from the runtime in one event-pump cycle.
pub const KEYBOARD_POLL_CHUNK_BYTES: usize = 128;

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

/// Runtime state for local-seat keyboard ingress and mirrored line egress.
///
/// This state is bounded by manifest values (`line_bytes`, `buffer_lines`) and
/// is transport-agnostic so HAL-owned keyboard/display backends can wire bytes
/// in/out without affecting parser semantics.
#[derive(Debug)]
pub struct LocalSeatRuntime {
    status: LocalSeatStatus,
    keyboard_queue: VecDeque<u8>,
    input_echo_preview: String,
    mirrored_lines: VecDeque<String>,
    dropped_keyboard_bytes: u64,
    dropped_mirrored_lines: u64,
    backend_keyboard_polling_enabled: bool,
    backend_keyboard_poll_deferred_logged: bool,
    #[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
    backend: Option<Pi4LocalSeat>,
}

/// Optional DT/firmware display mapping hint for local-seat HDMI output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalSeatDisplayHint {
    /// Physical base of the framebuffer allocation.
    pub paddr: usize,
    /// Visible width in pixels.
    pub width: usize,
    /// Visible height in pixels.
    pub height: usize,
    /// Bytes per rendered scanline.
    pub pitch: usize,
}

/// Optional bootloader-provided xHCI capability snapshot for local-seat handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalSeatXhciCapabilitySnapshot {
    /// Capability register length.
    pub cap_length: u8,
    /// xHCI interface version.
    pub hci_version: u16,
    /// Structural Parameters 1.
    pub hcs1: u32,
    /// Structural Parameters 2.
    pub hcs2: u32,
    /// Capability Parameters 1.
    pub hccparams1: u32,
    /// Doorbell offset.
    pub db_offset: u32,
    /// Runtime space offset.
    pub rts_offset: u32,
}

/// Optional bootloader-provided xHCI stop-state snapshot for local-seat handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalSeatXhciStopStateSnapshot {
    /// Operational `USBCMD` captured before handoff.
    pub usbcmd: Option<u32>,
    /// Operational `USBSTS` captured before handoff.
    pub usbsts: Option<u32>,
    /// Interrupter 0 `IMAN` captured before handoff.
    pub iman0: Option<u32>,
}

/// Optional platform-specific hints for local-seat backend attachment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalSeatPlatformHints {
    /// Optional MMIO base for Pi4 xHCI.
    pub xhci_mmio_hint: Option<usize>,
    /// Optional bootloader-exported PCI command state for Pi4 xHCI handoff.
    pub xhci_pci_cmd: Option<u16>,
    /// Whether the bootloader marked the Pi4 xHCI BAR safe for cold-start handoff.
    pub xhci_handoff_ready: bool,
    /// Whether the bootloader quiesced xHCI interrupt delivery before handoff.
    pub xhci_irq_quiesced: bool,
    /// Optional validated capability snapshot exported by the bootloader.
    pub xhci_capability_snapshot: Option<LocalSeatXhciCapabilitySnapshot>,
    /// Optional stop-state snapshot exported by the bootloader.
    pub xhci_stop_state_snapshot: Option<LocalSeatXhciStopStateSnapshot>,
    /// Optional DT/firmware framebuffer hint for HDMI rendering.
    pub display_hint: Option<LocalSeatDisplayHint>,
}

impl LocalSeatRuntime {
    /// Create a new runtime buffer set for the active local-seat manifest.
    #[must_use]
    pub fn new(status: LocalSeatStatus) -> Self {
        Self {
            status,
            keyboard_queue: VecDeque::new(),
            input_echo_preview: String::new(),
            mirrored_lines: VecDeque::new(),
            dropped_keyboard_bytes: 0,
            dropped_mirrored_lines: 0,
            // Keep boot fail-open: the root shell must stay reachable even if
            // a platform keyboard backend can still wedge during first probe.
            backend_keyboard_polling_enabled: false,
            backend_keyboard_poll_deferred_logged: false,
            #[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
            backend: None,
        }
    }

    /// Return manifest-derived runtime limits.
    #[must_use]
    pub const fn status(&self) -> LocalSeatStatus {
        self.status
    }

    /// Queue keyboard bytes received from a HAL-owned input backend.
    ///
    /// Returns the number of bytes accepted into the bounded queue.
    pub fn enqueue_keyboard_bytes(&mut self, bytes: &[u8]) -> usize {
        let mut accepted = 0usize;
        for &byte in bytes {
            if self.keyboard_queue.len() >= KEYBOARD_QUEUE_MAX_BYTES {
                self.dropped_keyboard_bytes = self.dropped_keyboard_bytes.saturating_add(1);
                continue;
            }
            self.keyboard_queue.push_back(byte);
            accepted = accepted.saturating_add(1);
        }
        accepted
    }

    /// Drain queued keyboard bytes into `out` and return bytes written.
    pub fn drain_keyboard_bytes(&mut self, out: &mut [u8]) -> usize {
        let mut written = 0usize;
        for slot in out.iter_mut() {
            match self.keyboard_queue.pop_front() {
                Some(byte) => {
                    *slot = byte;
                    written = written.saturating_add(1);
                }
                None => break,
            }
        }
        written
    }

    /// Mirror a console line into the bounded local-seat output ring.
    pub fn mirror_line(&mut self, line: &str) {
        let truncated = truncate_for_display(line, self.status.line_bytes);
        let mut mirrored = String::new();
        mirrored.push_str(truncated);

        while self.mirrored_lines.len() >= usize::from(self.status.buffer_lines) {
            if self.mirrored_lines.pop_front().is_none() {
                break;
            }
            self.dropped_mirrored_lines = self.dropped_mirrored_lines.saturating_add(1);
        }
        self.mirrored_lines.push_back(mirrored);

        #[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
        if let Some(backend) = self.backend.as_mut() {
            backend.write_line(truncated);
        }
    }

    /// Snapshot mirrored lines for diagnostics/tests.
    #[must_use]
    pub fn mirrored_lines_snapshot(&self) -> Vec<String> {
        self.mirrored_lines.iter().cloned().collect()
    }

    /// Count of keyboard bytes dropped due to queue saturation.
    #[must_use]
    pub const fn dropped_keyboard_bytes(&self) -> u64 {
        self.dropped_keyboard_bytes
    }

    /// Count of mirrored lines dropped due to ring saturation.
    #[must_use]
    pub const fn dropped_mirrored_lines(&self) -> u64 {
        self.dropped_mirrored_lines
    }

    /// Returns whether backend keyboard polling is currently enabled.
    #[must_use]
    pub const fn backend_keyboard_polling_enabled(&self) -> bool {
        self.backend_keyboard_polling_enabled
    }

    /// Enable backend keyboard polling after boot has reached a safe manual
    /// control point.
    pub fn enable_backend_keyboard_polling(&mut self) {
        self.backend_keyboard_polling_enabled = true;
    }

    fn echo_input_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            update_input_echo_preview(
                &mut self.input_echo_preview,
                byte,
                usize::from(self.status.line_bytes),
            );
        }

        #[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
        if let Some(backend) = self.backend.as_mut() {
            backend.write_bytes(bytes);
        }
    }

    /// Poll the platform local-seat input backend and enqueue discovered bytes.
    pub fn poll_backend_keyboard(&mut self) {
        #[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
        {
            if !LOCAL_SEAT_POLL_LOGGED.swap(true, Ordering::AcqRel) {
                boot_log::force_uart_line("[local-seat] runtime keyboard poll active");
            }
            let mut chunk = [0u8; KEYBOARD_POLL_CHUNK_BYTES];
            if let Some(backend) = self.backend.as_mut() {
                if !self.backend_keyboard_polling_enabled {
                    if !self.backend_keyboard_poll_deferred_logged {
                        self.backend_keyboard_poll_deferred_logged = true;
                        boot_log::force_uart_line(
                            "[local-seat] runtime keyboard poll deferred action=serial-shell-first",
                        );
                    }
                    return;
                }
                let read = backend.poll_keyboard_bytes(&mut chunk);
                if read > 0 {
                    if !LOCAL_SEAT_DATA_LOGGED.swap(true, Ordering::AcqRel) {
                        let mut line = heapless::String::<128>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!("[local-seat] runtime keyboard first-byte read={read}"),
                        );
                        boot_log::force_uart_line(line.as_str());
                    }
                    self.echo_input_bytes(&chunk[..read]);
                    let _ = self.enqueue_keyboard_bytes(&chunk[..read]);
                }
            }
        }
    }

    /// Returns whether a physical backend is attached to this runtime.
    #[must_use]
    pub fn backend_attached(&self) -> bool {
        #[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
        {
            return self.backend.is_some();
        }
        #[cfg(not(all(feature = "kernel", target_arch = "aarch64", target_os = "none")))]
        {
            false
        }
    }

    /// Attach a platform backend (HDMI text + keyboard ingress) to this runtime.
    #[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
    pub fn attach_backend(&mut self, backend: Pi4LocalSeat) {
        self.backend = Some(backend);
    }

    /// Publish the attached HDMI sink for boot-progress banners once runtime
    /// storage is stable.
    #[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
    pub fn register_boot_progress_backend(&mut self) {
        if let Some(backend) = self.backend.as_mut() {
            backend.register_boot_progress_display();
        }
    }

    /// Preseed platform keyboard MMIO windows after core boot mappings settle.
    pub fn preseed_backend_keyboard_mmio(&mut self) {
        #[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
        if let Some(backend) = self.backend.as_mut() {
            backend.preseed_keyboard_mmio();
        }
    }
}

/// Runtime local-seat backend initialisation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSeatBackendError {
    /// Platform backend initialisation failed with a Pi4-specific reason.
    #[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
    Pi4(Pi4SeatError),
    /// No local-seat backend is available on this profile/target.
    Unsupported,
}

impl LocalSeatBackendError {
    /// Stable diagnostic token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            #[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
            Self::Pi4(err) => err.as_str(),
            Self::Unsupported => "unsupported",
        }
    }
}

/// Try to attach a concrete platform backend to a local-seat runtime.
#[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
pub fn attach_platform_backend(
    runtime: &mut LocalSeatRuntime,
    hal: &mut crate::hal::KernelHal<'_>,
    hints: LocalSeatPlatformHints,
) -> Result<(), LocalSeatBackendError> {
    let backend_hints = Pi4LocalSeatHints {
        xhci_mmio_hint: hints.xhci_mmio_hint,
        xhci_pci_cmd: hints.xhci_pci_cmd,
        xhci_handoff_ready: hints.xhci_handoff_ready,
        xhci_irq_quiesced: hints.xhci_irq_quiesced,
        xhci_capability_snapshot: hints.xhci_capability_snapshot,
        xhci_stop_state_snapshot: hints.xhci_stop_state_snapshot,
        framebuffer_hint: hints.display_hint.map(|hint| Pi4FramebufferHint {
            paddr: hint.paddr,
            width: hint.width,
            height: hint.height,
            pitch: hint.pitch,
        }),
    };
    let backend = Pi4LocalSeat::new(hal, backend_hints).map_err(LocalSeatBackendError::Pi4)?;
    runtime.attach_backend(backend);
    Ok(())
}

/// Host/test profile backend attach path (always unavailable).
#[cfg(not(all(feature = "kernel", target_arch = "aarch64", target_os = "none")))]
#[cfg(feature = "kernel")]
pub fn attach_platform_backend(
    _runtime: &mut LocalSeatRuntime,
    _hal: &mut crate::hal::KernelHal<'_>,
    _hints: LocalSeatPlatformHints,
) -> Result<(), LocalSeatBackendError> {
    Err(LocalSeatBackendError::Unsupported)
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

/// Returns whether the local-seat runtime backend is available.
///
/// The current backend provides bounded keyboard buffering and mirrored output
/// routing while HAL-owned physical USB/HDMI device transports are attached.
#[must_use]
pub const fn runtime_backend_available() -> bool {
    cfg!(all(
        feature = "kernel",
        target_arch = "aarch64",
        target_os = "none"
    ))
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

fn update_input_echo_preview(preview: &mut String, byte: u8, max_bytes: usize) {
    match byte {
        b'\r' | b'\n' => preview.clear(),
        0x08 | 0x7f => {
            let _ = preview.pop();
        }
        byte if byte.is_ascii_control() => {}
        byte => {
            if preview.len() < max_bytes {
                preview.push(byte as char);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::{
        AttestationConfig, AttestationPolicy, DhcpPolicyConfig, HardwareConfig, HardwareDevice,
        HardwareNetworkConfig, LocalSeatConfig, NetworkBackendKind, NetworkInterfacePolicy,
        NetworkMode, StaticIpv4Config,
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
            network: HardwareNetworkConfig {
                enabled: false,
                backend: NetworkBackendKind::Auto,
                mode: NetworkMode::Off,
                interface: NetworkInterfacePolicy::Wired,
                static_ipv4: StaticIpv4Config {
                    ip: [0, 0, 0, 0],
                    prefix_len: 0,
                    gateway: None,
                },
                dhcp: DhcpPolicyConfig {
                    discover_timeout_ms: 1_000,
                    request_timeout_ms: 1_000,
                    max_retries: 4,
                },
            },
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

    #[test]
    fn runtime_queues_keyboard_bytes_with_bounded_capacity() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        let payload = [b'a'; KEYBOARD_QUEUE_MAX_BYTES + 64];
        let accepted = runtime.enqueue_keyboard_bytes(&payload);
        assert_eq!(accepted, KEYBOARD_QUEUE_MAX_BYTES);
        assert_eq!(runtime.dropped_keyboard_bytes(), 64);

        let mut drained = vec![0u8; 32];
        let read = runtime.drain_keyboard_bytes(&mut drained);
        assert_eq!(read, 32);
        assert!(drained.iter().all(|byte| *byte == b'a'));
    }

    #[test]
    fn runtime_mirrors_lines_with_manifest_bounds() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 5,
            buffer_lines: 2,
        });

        runtime.mirror_line("123456");
        runtime.mirror_line("abcdef");
        runtime.mirror_line("xyz");

        let lines = runtime.mirrored_lines_snapshot();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "abcde");
        assert_eq!(lines[1], "xyz");
        assert_eq!(runtime.dropped_mirrored_lines(), 1);
    }

    #[test]
    fn input_echo_preview_tracks_typing_backspace_and_enter() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 4,
            buffer_lines: 2,
        });

        runtime.echo_input_bytes(b"ab");
        assert_eq!(runtime.input_echo_preview, "ab");

        runtime.echo_input_bytes(b"\x08c");
        assert_eq!(runtime.input_echo_preview, "ac");

        runtime.echo_input_bytes(b"def");
        assert_eq!(runtime.input_echo_preview, "acde");

        runtime.echo_input_bytes(b"\n");
        assert!(runtime.input_echo_preview.is_empty());
    }

    #[test]
    fn runtime_backend_keyboard_poll_is_manual_by_default() {
        let runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        assert!(!runtime.backend_keyboard_polling_enabled());
    }

    #[test]
    fn runtime_backend_keyboard_poll_can_be_enabled_explicitly() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.enable_backend_keyboard_polling();

        assert!(runtime.backend_keyboard_polling_enabled());
    }
}
