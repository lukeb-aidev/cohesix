// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Event pump coordinating serial, timer, networking, and IPC work for the root task.
// Author: Lukas Bower

//! Cooperative event pump coordinating serial, timer, networking, and IPC work.
//!
//! The pump intentionally avoids dynamic allocation so it can operate in the
//! seL4 environment while remaining testable under `cargo test`. Each polling
//! cycle progresses the serial console, dispatches timer ticks, advances the
//! networking stack (when enabled), and finally services IPC queues.
//!
//! Tracing: enable the `timer-trace` feature to log periodic timer ticks for
//! debugging long-running workloads. The default `dev-virt` profile keeps timers
//! silent to prioritise network instrumentation.

#[cfg(feature = "kernel")]
pub mod dispatch;
#[cfg(feature = "kernel")]
pub mod handlers;
#[cfg(feature = "kernel")]
pub mod op;

extern crate alloc;

#[cfg(feature = "kernel")]
pub use dispatch::{dispatch_message, DispatchOutcome};
#[cfg(feature = "kernel")]
pub use handlers::{call_handler, Handler, HandlerError, HandlerResult, HandlerTable};
#[cfg(feature = "kernel")]
pub use op::BootstrapOp;

use core::cmp::min;
use core::fmt::{self, Write as FmtWrite};

use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[cfg(feature = "kernel")]
use crate::affinity;
#[cfg(feature = "kernel")]
use crate::lifecycle;

#[cfg(not(feature = "kernel"))]
mod lifecycle {
    use crate::generated::LifecycleState;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum RootCutReason {
        SessionRevoked,
        NetworkUnreachable,
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Gate {
        pub name: &'static str,
    }

    pub const GATE_WORKER_ATTACH: Gate = Gate {
        name: "worker-attach",
    };

    #[derive(Clone, Copy, Debug)]
    pub struct RootSnapshot {
        pub reachable: bool,
    }

    #[inline]
    pub fn root_snapshot() -> RootSnapshot {
        RootSnapshot { reachable: true }
    }

    #[inline]
    pub fn gate_allows(_: Gate) -> bool {
        true
    }

    #[inline]
    pub fn state() -> LifecycleState {
        LifecycleState::Online
    }

    #[inline]
    pub fn state_label(_state: LifecycleState) -> &'static str {
        "ONLINE"
    }

    #[inline]
    pub fn root_record_activity(_now_ms: u64) {}

    #[inline]
    pub fn root_mark_cut(_reason: RootCutReason) {}

    #[inline]
    pub fn root_mark_policy_denied() {}

    #[inline]
    pub fn root_mark_session_active(_now_ms: u64) {}
}
use cohesix_ticket::{Role, TicketClaims, TicketQuotas, TicketToken, TicketVerb};
use cohsh_core::{ConsoleVerb, RoleParseMode};
use heapless::{String as HeaplessString, Vec as HeaplessVec};

#[cfg(feature = "kernel")]
use crate::bootstrap::log as boot_log;
use crate::console::proto::{render_ack, AckLine, AckStatus, LineFormatError};
use crate::console::{Command, CommandParser, ConsoleError, SmpMode, MAX_ROLE_LEN, MAX_TICKET_LEN};
#[cfg(feature = "kernel")]
use crate::debug_uart::debug_uart_str;
#[cfg(feature = "net-console")]
use crate::hal::driver_task::DriverServiceBudget;
#[cfg(feature = "kernel")]
use crate::hal::{
    SdioBusWidth, WifiControlPlaneTrace, WifiDebugOps, WifiDebugSnapshot,
    WifiFirmwareContractTrace, WifiPowerState, WifiResetState, WifiSdhciContractTrace,
};
#[cfg(feature = "net-console")]
use crate::local_seat::LocalSeatDisplayTrace;
use crate::local_seat::{LocalSeatRuntime, KEYBOARD_POLL_CHUNK_BYTES};
#[cfg(feature = "kernel")]
use crate::log_buffer;
#[cfg(feature = "net-console")]
use crate::net::NetSelfTestStartResult;
#[cfg(feature = "net-console")]
use crate::net::{
    ConsoleLine, NetConsoleDisconnectReason, NetConsoleEvent, NetCounters, NetDiagSnapshot,
    NetPoller, NetStatusReport, NetTelemetry, CONSOLE_DISPATCH_BURST, NET_DIAG, NET_DIAG_FEATURED,
};
#[cfg(feature = "kernel")]
use crate::ninedoor::TelemetryTailMeta;
#[cfg(feature = "kernel")]
use crate::ninedoor::{NineDoorBridge, NineDoorBridgeError};
#[cfg(feature = "net-console")]
use crate::observe::IngestSnapshot;
use crate::observe::PressureKind;
#[cfg(feature = "kernel")]
use crate::sel4;
#[cfg(feature = "kernel")]
use crate::sel4::{BootInfoExt, BootInfoView};
use crate::serial::{SerialDriver, SerialPort, SerialTelemetry, DEFAULT_LINE_CAPACITY};
#[cfg(feature = "net-console")]
use crate::trace::{RateLimitKey, RateLimiter};
#[cfg(feature = "kernel")]
use sel4_sys::seL4_CPtr;

#[cfg(not(feature = "kernel"))]
fn debug_uart_str(_message: &str) {}

fn format_message(args: fmt::Arguments<'_>) -> HeaplessString<DEFAULT_LINE_CAPACITY> {
    let mut buf = HeaplessString::new();
    if FmtWrite::write_fmt(&mut buf, args).is_err() {
        // Truncated diagnostic; best-effort only.
    }
    buf
}

#[cfg(feature = "kernel")]
#[derive(Clone)]
struct WifiLiveNetFrontier {
    active_interface: &'static str,
    address_source: &'static str,
    dhcp_phase: &'static str,
    ip: HeaplessString<32>,
    wifi_assoc: u64,
    wifi_link_up: u64,
    wifi_host_eapol_secure: u64,
}

/// Trait used by the event pump to emit audit records.
pub trait AuditSink {
    /// Informational message emitted during pump initialisation or state changes.
    fn info(&mut self, message: &str);

    /// Audit entry emitted when a privileged action is denied.
    fn denied(&mut self, message: &str);
}

/// Tick emitted by a [`TimerSource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickEvent {
    /// Sequential tick identifier.
    pub tick: u64,
    /// Monotonic time of the tick in milliseconds.
    pub now_ms: u64,
}

#[cfg(feature = "kernel")]
const MAX_BOOTSTRAP_WORDS: usize = crate::sel4::MSG_MAX_WORDS;

#[cfg(feature = "kernel")]
const BOOTSTRAP_IDLE_SPINS: usize = 512;

const CONSOLE_BANNER: &str = "[Cohesix] Root console starting (type 'help' for commands)";
const CONSOLE_PROMPT: &str = "cohesix> ";
const QUEEN_CTL_PATH: &str = "/queen/ctl";
#[cfg(feature = "kernel")]
const WIFI_DEBUG_ACK_LABEL: &str = "WIFI";
#[cfg(feature = "kernel")]
const USB_DEBUG_ACK_LABEL: &str = "USB";
#[cfg(feature = "kernel")]
const WIFI_CHIPCLKCSR_FORCE_HT: u8 = 0x02;
#[cfg(feature = "kernel")]
const WIFI_CHIPCLKCSR_ALP_AVAIL_REQ: u8 = 0x08;
#[cfg(feature = "kernel")]
const WIFI_CHIPCLKCSR_HT_AVAIL_REQ: u8 = 0x10;
#[cfg(feature = "kernel")]
const WIFI_CHIPCLKCSR_ALP_AVAIL: u8 = 0x40;
#[cfg(feature = "kernel")]
const WIFI_CHIPCLKCSR_HT_AVAIL: u8 = 0x80;
#[cfg(feature = "kernel")]
const WIFI_WAKE_TILL_HT_AVAIL: u8 = 0x02;
#[cfg(feature = "kernel")]
const WIFI_SLEEPCSR_KSO: u8 = 0x01;
#[cfg(feature = "kernel")]
const WIFI_SLEEPCSR_DEVON: u8 = 0x02;
#[cfg(feature = "net-console")]
const NET_DIAG_RATE_LIMIT_MS: u64 = 15_000;
#[cfg(feature = "net-console")]
const NET_DIAG_RATE_KINDS: usize = 1;
#[cfg(feature = "net-console")]
const NET_DIAG_STUCK_MS: u64 = 3_000;
#[cfg(feature = "net-console")]
const WIFI_HOST_EAPOL_PRE_ROOT_BURST_POLLS: usize = 96;
#[cfg(feature = "net-console")]
const WIFI_HOST_EAPOL_RUNTIME_BURST_POLLS: usize = 8;
#[cfg(feature = "net-console")]
// Network-origin NineDoor commands may enqueue multiple TCP response segments;
// keep a small bounded flush window so replies are not deferred behind later
// event-loop work during Genet bursts.
const NET_POST_DISPATCH_FLUSH_POLLS: usize = 8;
#[cfg(feature = "net-console")]
const NET_POST_DISPATCH_BACKLOG_FLUSH_POLLS: usize = 16;
#[cfg(feature = "net-console")]
const NET_LINKED_RUNTIME_HOT_DISPATCH_ROUNDS: usize = 3;
#[cfg(feature = "net-console")]
const NET_CYW43_HOT_DISPATCH_ROUNDS: usize = 6;
#[cfg(feature = "net-console")]
const NET_CYW43_POST_DISPATCH_FLUSH_POLLS: usize = 12;
#[cfg(feature = "net-console")]
const NET_CYW43_POST_DISPATCH_BACKLOG_FLUSH_POLLS: usize = 24;
const LOCAL_SEAT_BACKEND_POLL_PASSES_PER_TURN: usize = 1;
const LOCAL_SEAT_BURST_DRAIN_PASSES_PER_TURN: usize = 4;
const LOCAL_SEAT_EMPTY_POLLS_BEFORE_YIELD: usize = 1;
const LOCAL_SEAT_OUTPUT_KEYBOARD_POLL_PASSES: usize = 1;
const LOCAL_SEAT_HDMI_PUMP_PASSES_PER_TURN: usize = 3;
#[cfg(feature = "net-console")]
const LOCAL_SEAT_HDMI_PUMP_PASSES_UNDER_NET_PRESSURE: usize = 1;
const LOCAL_SEAT_NET_MIRROR_INITIAL_LINES: u64 = 16;
const LOCAL_SEAT_NET_MIRROR_SAMPLE_STRIDE: u64 = 256;
const CONSOLE_OUTPUT_BACKLOG_LINES: usize = 32;
const CONSOLE_OUTPUT_LINES_PER_IDLE_TURN: usize = 2;
const CONSOLE_INPUT_TURN_IMMEDIATE_OUTPUT_LINES: usize = 1;
#[cfg(feature = "kernel")]
const SERIAL_INPUT_IDLE_TRACE_INTERVAL_MS: u64 = 10_000;
#[cfg(feature = "kernel")]
const SERIAL_INPUT_IDLE_TRACE_LIMIT: u8 = 2;
#[cfg(feature = "kernel")]
const SERIAL_RAW_UART_PREFLUSH_TURNS: usize = 8;
#[cfg(all(feature = "kernel", feature = "usb"))]
const POST_PROMPT_LOCAL_SEAT_ATTACH_IDLE_GRACE_MS: u64 = 750;
#[cfg(all(feature = "kernel", feature = "usb"))]
const POST_PROMPT_LOCAL_SEAT_ATTACH_IDLE_TURNS: u8 = 2;
#[cfg(all(feature = "kernel", feature = "usb"))]
const POST_PROMPT_LOCAL_SEAT_ATTACH_RETRY_MS: u64 = 10_000;
#[cfg(all(feature = "kernel", feature = "usb"))]
const POST_PROMPT_LOCAL_SEAT_ATTACH_RETRY_IDLE_TURNS: u16 = 1024;
#[cfg(all(feature = "kernel", feature = "usb"))]
const POST_PROMPT_LOCAL_SEAT_ATTACH_ACTIVE_USB_RETRY_MS: u64 = 0;
#[cfg(all(feature = "kernel", feature = "usb"))]
const POST_PROMPT_LOCAL_SEAT_ATTACH_ACTIVE_USB_RETRY_IDLE_TURNS: u16 = 0;
#[cfg(all(feature = "kernel", feature = "usb"))]
const POST_PROMPT_LOCAL_SEAT_ATTACH_VERBOSE_ATTEMPTS: u16 = 1;

const fn local_seat_usb_burst_proof(
    accepted_bytes: u64,
    drained_bytes: u64,
    echoed_bytes: u64,
    dropped_bytes: u64,
) -> bool {
    let burst_floor = (LOCAL_SEAT_BURST_DRAIN_PASSES_PER_TURN * KEYBOARD_POLL_CHUNK_BYTES) as u64;
    accepted_bytes >= burst_floor
        && accepted_bytes == drained_bytes
        && accepted_bytes == echoed_bytes
        && dropped_bytes == 0
}

const fn local_seat_network_mirror_sample_allowed(ordinal: u64) -> bool {
    ordinal < LOCAL_SEAT_NET_MIRROR_INITIAL_LINES
        || ordinal % LOCAL_SEAT_NET_MIRROR_SAMPLE_STRIDE == LOCAL_SEAT_NET_MIRROR_SAMPLE_STRIDE - 1
}

const LOCAL_SEAT_SERIAL_LINES_PER_TURN: usize = 1;
const LOCAL_SEAT_SERIAL_OUTPUT_CHUNK_BYTES: usize = 32;

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn post_prompt_local_seat_attach_retry_policy(
    usb_no_reply: bool,
    usb_active: bool,
) -> Option<(u64, u16, &'static str)> {
    if usb_active {
        Some((
            POST_PROMPT_LOCAL_SEAT_ATTACH_ACTIVE_USB_RETRY_MS,
            POST_PROMPT_LOCAL_SEAT_ATTACH_ACTIVE_USB_RETRY_IDLE_TURNS,
            "serial-safe-active-usb-progress",
        ))
    } else if usb_no_reply {
        None
    } else {
        Some((
            POST_PROMPT_LOCAL_SEAT_ATTACH_RETRY_MS,
            POST_PROMPT_LOCAL_SEAT_ATTACH_RETRY_IDLE_TURNS,
            "serial-safe-usb-progress",
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LocalSeatConsumePhase {
    PreRuntime,
    PriorityFollowup,
    PostRuntime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LocalSeatEscapeState {
    Idle,
    Esc,
    Csi,
}

#[cfg(test)]
const fn local_seat_input_drain_contract_for_test(
) -> (usize, usize, usize, usize, usize, usize, usize) {
    (
        LOCAL_SEAT_BACKEND_POLL_PASSES_PER_TURN,
        LOCAL_SEAT_BURST_DRAIN_PASSES_PER_TURN,
        LOCAL_SEAT_EMPTY_POLLS_BEFORE_YIELD,
        LOCAL_SEAT_OUTPUT_KEYBOARD_POLL_PASSES,
        LOCAL_SEAT_HDMI_PUMP_PASSES_PER_TURN,
        LOCAL_SEAT_SERIAL_LINES_PER_TURN,
        LOCAL_SEAT_SERIAL_OUTPUT_CHUNK_BYTES,
    )
}

#[cfg(feature = "net-console")]
fn net_status_allows_root_console(status: &NetStatusReport) -> bool {
    matches!(
        status.address_source,
        "manifest-static" | "dev-virt" | "dhcp-lease"
    )
}

#[cfg(feature = "net-console")]
fn net_status_terminal_failure_reason(status: &NetStatusReport) -> Option<&'static str> {
    match status.address_source {
        "wifi-host-eapol-required" | "wifi-association-failed" => Some(status.address_source),
        _ => None,
    }
}

#[cfg(feature = "net-console")]
fn net_status_pre_root_serial_release_reason(status: &NetStatusReport) -> Option<&'static str> {
    net_status_terminal_failure_reason(status)
}

#[cfg(feature = "net-console")]
fn net_status_needs_host_eapol_burst(status: &NetStatusReport) -> bool {
    status.address_source == "wifi-host-eapol-pending"
}

#[cfg(feature = "net-console")]
fn net_status_active_interface_is_wifi(status: &NetStatusReport) -> bool {
    status.active_interface == "wifi"
}

#[cfg(feature = "net-console")]
fn net_status_wifi_relevant(status: &NetStatusReport) -> bool {
    matches!(status.interface_policy, "wifi" | "auto")
        || status.active_interface == "wifi"
        || status.standby_interface == "wifi"
        || status.address_source.starts_with("wifi-")
        || status.dhcp_phase.starts_with("wifi-")
}

#[cfg(feature = "net-console")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WifiCredentialWarning {
    code: &'static str,
    detail: &'static str,
    action: &'static str,
}

#[cfg(feature = "net-console")]
fn wifi_credential_warning_from_reason(reason: &str) -> Option<WifiCredentialWarning> {
    match reason {
        "wifi-psk-invalid" | "invalid-wifi-psk" => Some(WifiCredentialWarning {
            code: "invalid-config",
            detail: "psk-format-invalid",
            action: "check-wifi-password-format",
        }),
        "wifi-psk-too-short" => Some(WifiCredentialWarning {
            code: "invalid-config",
            detail: "psk-too-short",
            action: "check-wifi-password-length",
        }),
        "wifi-psk-too-long" => Some(WifiCredentialWarning {
            code: "invalid-config",
            detail: "psk-too-long",
            action: "check-wifi-password-length",
        }),
        "wifi-ssid-empty" | "wifi-ssid-missing" | "invalid-wifi-ssid" => {
            Some(WifiCredentialWarning {
                code: "invalid-config",
                detail: "ssid-missing-or-invalid",
                action: "check-wifi-ssid",
            })
        }
        "wifi-ssid-too-long" => Some(WifiCredentialWarning {
            code: "invalid-config",
            detail: "ssid-too-long",
            action: "check-wifi-ssid",
        }),
        "host-eapol-m3-mic" | "host-eapol-group-mic" => Some(WifiCredentialWarning {
            code: "password-or-security-mismatch",
            detail: "wpa2-key-mic-failed",
            action: "check-ssid-password-and-wpa2-security",
        }),
        "cyw43-association-auth-timeout" => Some(WifiCredentialWarning {
            code: "ssid-or-security-unavailable",
            detail: "association-auth-timeout",
            action: "check-ssid-password-security-and-range",
        }),
        "cyw43-association-set-ssid-failed" => Some(WifiCredentialWarning {
            code: "ssid-or-security-unavailable",
            detail: "set-ssid-failed",
            action: "check-ssid-and-ap-availability",
        }),
        "cyw43-association-not-associated"
        | "cyw43-association-event-missing"
        | "wifi-association-failed" => Some(WifiCredentialWarning {
            code: "ssid-or-ap-unavailable",
            detail: "association-not-complete",
            action: "check-ssid-ap-range-and-security",
        }),
        _ => None,
    }
}

#[cfg(feature = "net-console")]
fn wifi_credentials_already_proven(status: &NetStatusReport, stats: &NetCounters) -> bool {
    status.address_source == "dhcp-lease"
        || status.dhcp_phase == "bound"
        || (stats.wifi_assoc != 0 && stats.wifi_link_up != 0 && stats.wifi_host_eapol_secure != 0)
}

#[cfg(feature = "net-console")]
fn wifi_credential_warning_for_status(
    status: &NetStatusReport,
    stats: &NetCounters,
    exact_reason: Option<&'static str>,
) -> Option<WifiCredentialWarning> {
    if !net_status_wifi_relevant(status) || wifi_credentials_already_proven(status, stats) {
        return None;
    }
    exact_reason
        .and_then(wifi_credential_warning_from_reason)
        .or_else(|| wifi_credential_warning_from_reason(status.address_source))
        .or_else(|| wifi_credential_warning_from_reason(status.dhcp_phase))
}

#[cfg(all(feature = "kernel", feature = "net-console"))]
fn wifi_runtime_credential_warning_reason() -> Option<&'static str> {
    crate::drivers::driver_task_net::latest_cyw43_wifi_credential_warning()
}

#[cfg(all(not(feature = "kernel"), feature = "net-console"))]
const fn wifi_runtime_credential_warning_reason() -> Option<&'static str> {
    None
}

#[cfg(feature = "net-console")]
fn net_status_active_interface_is_wired(status: &NetStatusReport) -> bool {
    status.active_interface == "wired"
}

#[cfg(feature = "net-console")]
const fn net_physical_input_pressure_for_status(
    physical_input_active: bool,
    local_seat_first_report_pending: bool,
    host_eapol_pending: bool,
) -> bool {
    physical_input_active || (local_seat_first_report_pending && !host_eapol_pending)
}

#[cfg(feature = "net-console")]
fn net_status_should_yield_to_physical_input(status: &NetStatusReport) -> bool {
    matches!(
        status.address_source,
        "wifi-host-eapol-pending" | "wifi-host-eapol-required"
    )
}

#[cfg(feature = "net-console")]
fn net_status_needs_physical_pressure_service(status: &NetStatusReport) -> bool {
    net_status_active_interface_is_wired(status) && status.address_source == "dhcp-lease"
}

#[cfg(feature = "net-console")]
fn net_status_linked_runtime_data_ready(status: &NetStatusReport) -> bool {
    matches!(status.backend, "bcmgenet-v5" | "cyw43")
        && matches!(status.active_interface, "wired" | "wifi")
        && status.address_source == "dhcp-lease"
        && status.dhcp_phase == "bound"
}

#[cfg(feature = "net-console")]
fn net_status_cyw43_data_ready(status: &NetStatusReport) -> bool {
    status.backend == "cyw43"
        && status.active_interface == "wifi"
        && status.address_source == "dhcp-lease"
        && status.dhcp_phase == "bound"
}

#[cfg(feature = "net-console")]
fn net_hot_dispatch_rounds_for_status(status: &NetStatusReport) -> usize {
    if net_status_cyw43_data_ready(status) {
        NET_CYW43_HOT_DISPATCH_ROUNDS
    } else if net_status_linked_runtime_data_ready(status) {
        NET_LINKED_RUNTIME_HOT_DISPATCH_ROUNDS
    } else {
        1
    }
}

#[cfg(feature = "net-console")]
const fn net_post_dispatch_flush_limit_for_display(
    display: Option<LocalSeatDisplayTrace>,
) -> usize {
    match display {
        Some(trace)
            if trace.pending_bytes != 0
                || trace.pending_redraw
                || trace.no_reply_cooldown_turns != 0
                || trace.stale_after_retry_exhaustion
                || (trace.no_reply_frames != 0
                    && trace.deferred_frames > trace.submitted_frames) =>
        {
            NET_POST_DISPATCH_BACKLOG_FLUSH_POLLS
        }
        _ => NET_POST_DISPATCH_FLUSH_POLLS,
    }
}

#[cfg(feature = "net-console")]
fn net_post_dispatch_flush_limit_for_status(
    status: &NetStatusReport,
    display: Option<LocalSeatDisplayTrace>,
) -> usize {
    if net_status_cyw43_data_ready(status) {
        if net_post_dispatch_flush_limit_for_display(display)
            == NET_POST_DISPATCH_BACKLOG_FLUSH_POLLS
        {
            NET_CYW43_POST_DISPATCH_BACKLOG_FLUSH_POLLS
        } else {
            NET_CYW43_POST_DISPATCH_FLUSH_POLLS
        }
    } else {
        net_post_dispatch_flush_limit_for_display(display)
    }
}

#[cfg_attr(not(any(test, feature = "kernel")), allow(dead_code))]
#[derive(Debug, Default)]
struct BootstrapBackoff {
    idle_spins: usize,
    limit: usize,
}

#[cfg_attr(not(any(test, feature = "kernel")), allow(dead_code))]
impl BootstrapBackoff {
    fn new(limit: usize) -> Self {
        Self {
            idle_spins: 0,
            limit,
        }
    }

    fn observe(&mut self, has_staged: bool) -> Option<usize> {
        if has_staged {
            self.idle_spins = 0;
            return None;
        }
        self.idle_spins = self.idle_spins.saturating_add(1);
        if self.idle_spins >= self.limit {
            Some(self.idle_spins)
        } else {
            None
        }
    }
}

#[cfg(feature = "kernel")]
#[derive(Clone)]
/// IPC message staged during bootstrap and replayed once the dispatcher is ready.
pub struct BootstrapMessage {
    /// Badge attached to the message capability.
    pub badge: sel4_sys::seL4_Word,
    /// Raw message info describing the word and capability counts.
    pub info: sel4_sys::seL4_MessageInfo,
    /// Payload words staged from the IPC buffer.
    pub payload: HeaplessVec<sel4_sys::seL4_Word, { MAX_BOOTSTRAP_WORDS }>,
}

#[cfg(feature = "kernel")]
impl fmt::Debug for BootstrapMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BootstrapMessage")
            .field("badge", &self.badge)
            .field("info_raw", &self.info.words)
            .field("payload", &self.payload)
            .finish()
    }
}

#[cfg(feature = "kernel")]
impl PartialEq for BootstrapMessage {
    fn eq(&self, other: &Self) -> bool {
        self.badge == other.badge
            && self.info.words == other.info.words
            && self.payload == other.payload
    }
}

#[cfg(feature = "kernel")]
impl Eq for BootstrapMessage {}

#[cfg(feature = "kernel")]
impl BootstrapMessage {
    /// Returns `true` when the staged payload contained no words.
    pub fn payload_is_empty(&self) -> bool {
        self.payload.is_empty()
    }
}

/// Timer abstraction used by the event pump.
pub trait TimerSource {
    /// Poll the timer for the next tick, if any.
    fn poll(&mut self, now_ms: u64) -> Option<TickEvent>;
}

/// IPC dispatcher invoked once per pump cycle.
pub trait IpcDispatcher {
    /// Service pending IPC messages.
    fn dispatch(&mut self, now_ms: u64);

    /// Called once the event pump has registered bootstrap handlers.
    fn handlers_ready(&mut self) {}

    #[cfg(feature = "kernel")]
    /// Retrieve the next staged bootstrap message, if any.
    fn take_bootstrap_message(&mut self) -> Option<BootstrapMessage> {
        None
    }

    #[cfg(feature = "kernel")]
    /// Poll the bootstrap endpoint, returning `true` when a message was staged.
    fn bootstrap_poll(&mut self, now_ms: u64) -> bool {
        let _ = now_ms;
        false
    }

    #[cfg(feature = "kernel")]
    /// Return `true` when a bootstrap message is currently staged.
    fn has_staged_bootstrap(&self) -> bool {
        false
    }
}

#[cfg(feature = "kernel")]
/// Handler invoked when the pump observes a staged bootstrap IPC message.
pub trait BootstrapMessageHandler {
    /// Process the staged message once it has been drained from the dispatcher.
    fn handle(&mut self, message: &BootstrapMessage, audit: &mut dyn AuditSink);
}

/// Capability validator consulted when privileged verbs execute.
pub trait CapabilityValidator {
    /// Validate that `ticket` grants the requested `role`.
    fn validate(&self, role: Role, ticket: Option<&str>) -> bool;
}

/// Error raised when registering capability tickets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketRegistryError {
    /// The ticket table reached its capacity.
    Capacity,
    /// Provided secret exceeded the allowed size.
    SecretTooLong,
}

#[derive(Debug)]
struct TicketRecord {
    role: Role,
    key: cohesix_ticket::TicketKey,
}

/// Deterministic capability table used by the authenticated console.
#[derive(Debug)]
pub struct TicketTable<const N: usize> {
    entries: HeaplessVec<TicketRecord, N>,
}

impl<const N: usize> TicketTable<N> {
    /// Create an empty ticket table.
    pub const fn new() -> Self {
        Self {
            entries: HeaplessVec::new(),
        }
    }

    /// Register a new ticket secret.
    pub fn register(&mut self, role: Role, secret: &str) -> Result<(), TicketRegistryError> {
        if secret.len() > MAX_TICKET_LEN {
            return Err(TicketRegistryError::SecretTooLong);
        }
        self.register_key(role, cohesix_ticket::TicketKey::from_secret(secret))
    }

    /// Register a manifest-generated ticket key without deriving it during boot.
    pub fn register_key(
        &mut self,
        role: Role,
        key: cohesix_ticket::TicketKey,
    ) -> Result<(), TicketRegistryError> {
        if self.entries.is_full() {
            return Err(TicketRegistryError::Capacity);
        }
        self.entries
            .push(TicketRecord { role, key })
            .map_err(|_| TicketRegistryError::Capacity)
    }
}

impl TicketRegistryError {
    /// Stable boot-log label for ticket registration failures.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Capacity => "capacity",
            Self::SecretTooLong => "secret-too-long",
        }
    }
}

impl<const N: usize> Default for TicketTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> CapabilityValidator for TicketTable<N> {
    fn validate(&self, role: Role, ticket: Option<&str>) -> bool {
        let ticket = ticket.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        if role == Role::Queen && ticket.is_none() {
            return true;
        }
        let Some(ticket) = ticket else { return false };
        let key = self
            .entries
            .iter()
            .find_map(|record| (record.role == role).then_some(&record.key));
        let Some(key) = key else { return false };
        let Ok(decoded) = cohesix_ticket::TicketToken::decode(ticket, key) else {
            return false;
        };
        decoded.claims().role == role
    }
}

const TICKET_RATE_WINDOW_MS: u64 = 1_000;
const REBOOT_ACK_FLUSH_TURNS: u8 = 1;

/// Validation error when a ticket exceeds manifest limits.
#[derive(Debug, Clone)]
enum TicketClaimError {
    ScopeCount { count: usize, max: u16 },
    ScopePath { path: String, max_len: u16 },
    ScopeRate { rate: u32, max: u32 },
    Bandwidth { value: u64, max: u64 },
    CursorResumes { value: u32, max: u32 },
    CursorAdvances { value: u32, max: u32 },
}

impl fmt::Display for TicketClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TicketClaimError::ScopeCount { count, max } => {
                write!(f, "scope count {count} exceeds {max}")
            }
            TicketClaimError::ScopePath { path, max_len } => {
                write!(f, "scope path '{path}' exceeds {max_len} bytes")
            }
            TicketClaimError::ScopeRate { rate, max } => {
                write!(f, "scope rate {rate} exceeds {max}")
            }
            TicketClaimError::Bandwidth { value, max } => {
                write!(f, "bandwidth quota {value} exceeds {max}")
            }
            TicketClaimError::CursorResumes { value, max } => {
                write!(f, "cursor resume quota {value} exceeds {max}")
            }
            TicketClaimError::CursorAdvances { value, max } => {
                write!(f, "cursor advance quota {value} exceeds {max}")
            }
        }
    }
}

/// Denial outcome for ticket enforcement.
#[derive(Debug, Clone, Copy)]
enum TicketDeny {
    Scope,
    Rate {
        limit_per_s: u32,
    },
    Bandwidth {
        limit_bytes: u64,
        remaining_bytes: u64,
        requested_bytes: u64,
    },
    CursorResume {
        limit: u32,
    },
    CursorAdvance {
        limit: u32,
    },
}

#[derive(Debug, Clone, Copy)]
struct CursorCheck {
    is_resume: bool,
}

#[derive(Debug, Clone)]
struct TicketScopeState {
    path: Vec<String>,
    verb: TicketVerb,
    rate_limit: Option<u32>,
    window_start_ms: u64,
    window_count: u32,
}

impl TicketScopeState {
    fn allows_verb(&self, verb: TicketVerb) -> bool {
        match self.verb {
            TicketVerb::Read => matches!(verb, TicketVerb::Read),
            TicketVerb::Write => matches!(verb, TicketVerb::Write),
            TicketVerb::ReadWrite => true,
        }
    }

    fn matches_path(&self, path: &[String], allow_ancestor: bool) -> bool {
        if path.starts_with(self.path.as_slice()) {
            return true;
        }
        if allow_ancestor && self.path.starts_with(path) {
            return true;
        }
        false
    }

    fn check_rate(&mut self, now_ms: u64) -> Result<(), TicketDeny> {
        let Some(limit) = self.rate_limit else {
            return Ok(());
        };
        if now_ms.saturating_sub(self.window_start_ms) >= TICKET_RATE_WINDOW_MS {
            self.window_start_ms = now_ms;
            self.window_count = 0;
        }
        if self.window_count >= limit {
            return Err(TicketDeny::Rate { limit_per_s: limit });
        }
        self.window_count = self.window_count.saturating_add(1);
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct TicketQuotaState {
    bandwidth_limit: Option<u64>,
    bandwidth_remaining: Option<u64>,
    cursor_resume_limit: Option<u32>,
    cursor_resume_remaining: Option<u32>,
    cursor_advance_limit: Option<u32>,
    cursor_advance_remaining: Option<u32>,
}

impl TicketQuotaState {
    fn bandwidth_limit(&self) -> Option<u64> {
        self.bandwidth_limit
    }

    fn cursor_resume_limit(&self) -> Option<u32> {
        self.cursor_resume_limit
    }

    fn cursor_advance_limit(&self) -> Option<u32> {
        self.cursor_advance_limit
    }

    fn check_bandwidth(&self, requested: u64) -> Result<(), TicketDeny> {
        let Some(remaining) = self.bandwidth_remaining else {
            return Ok(());
        };
        if requested > remaining {
            let limit = self.bandwidth_limit.unwrap_or(remaining);
            return Err(TicketDeny::Bandwidth {
                limit_bytes: limit,
                remaining_bytes: remaining,
                requested_bytes: requested,
            });
        }
        Ok(())
    }

    fn consume_bandwidth(&mut self, consumed: u64) {
        if let Some(remaining) = &mut self.bandwidth_remaining {
            *remaining = remaining.saturating_sub(consumed);
        }
    }

    fn check_cursor(&self, is_resume: bool) -> Result<(), TicketDeny> {
        if let Some(remaining) = self.cursor_advance_remaining {
            if remaining == 0 {
                let limit = self.cursor_advance_limit.unwrap_or(0);
                return Err(TicketDeny::CursorAdvance { limit });
            }
        }
        if is_resume {
            if let Some(remaining) = self.cursor_resume_remaining {
                if remaining == 0 {
                    let limit = self.cursor_resume_limit.unwrap_or(0);
                    return Err(TicketDeny::CursorResume { limit });
                }
            }
        }
        Ok(())
    }

    fn consume_cursor(&mut self, is_resume: bool) {
        if let Some(remaining) = &mut self.cursor_advance_remaining {
            *remaining = remaining.saturating_sub(1);
        }
        if is_resume {
            if let Some(remaining) = &mut self.cursor_resume_remaining {
                *remaining = remaining.saturating_sub(1);
            }
        }
    }

    fn has_limits(&self) -> bool {
        self.bandwidth_limit.is_some()
            || self.cursor_resume_limit.is_some()
            || self.cursor_advance_limit.is_some()
    }
}

#[derive(Debug, Clone)]
struct TicketUsage {
    scopes: Vec<TicketScopeState>,
    quotas: TicketQuotaState,
    cursor_offsets: BTreeMap<String, u64>,
}

impl TicketUsage {
    fn from_claims(
        claims: &TicketClaims,
        limits: crate::generated::TicketLimits,
        now_ms: u64,
    ) -> Result<Self, TicketClaimError> {
        if claims.scopes.len() > limits.max_scopes as usize {
            return Err(TicketClaimError::ScopeCount {
                count: claims.scopes.len(),
                max: limits.max_scopes,
            });
        }
        let mut scopes = Vec::with_capacity(claims.scopes.len());
        for scope in &claims.scopes {
            let path = scope.path.trim().to_owned();
            if path.len() > limits.max_scope_path_len as usize
                || (!path.is_empty() && !path.starts_with('/'))
            {
                return Err(TicketClaimError::ScopePath {
                    path,
                    max_len: limits.max_scope_path_len,
                });
            }
            if limits.max_scope_rate_per_s > 0 && scope.rate_per_s > limits.max_scope_rate_per_s {
                return Err(TicketClaimError::ScopeRate {
                    rate: scope.rate_per_s,
                    max: limits.max_scope_rate_per_s,
                });
            }
            let components = split_scope_path(&path, limits.max_scope_path_len)?;
            let rate_limit = (scope.rate_per_s > 0).then_some(scope.rate_per_s);
            scopes.push(TicketScopeState {
                path: components,
                verb: scope.verb,
                rate_limit,
                window_start_ms: now_ms,
                window_count: 0,
            });
        }
        let quotas = resolve_quotas(claims.quotas, limits)?;
        Ok(Self {
            scopes,
            quotas,
            cursor_offsets: BTreeMap::new(),
        })
    }

    fn has_enforcement(&self) -> bool {
        !self.scopes.is_empty() || self.quotas.has_limits()
    }

    fn check_scope(
        &mut self,
        path: &[String],
        verb: TicketVerb,
        allow_ancestor: bool,
        now_ms: u64,
    ) -> Result<(), TicketDeny> {
        if self.scopes.is_empty() {
            return Ok(());
        }
        let Some(idx) = self.best_scope_index(path, verb, allow_ancestor) else {
            return Err(TicketDeny::Scope);
        };
        self.scopes[idx].check_rate(now_ms)
    }

    fn check_bandwidth(&self, requested: u64) -> Result<(), TicketDeny> {
        self.quotas.check_bandwidth(requested)
    }

    fn consume_bandwidth(&mut self, consumed: u64) {
        self.quotas.consume_bandwidth(consumed);
    }

    fn check_cursor(&self, path_key: &str, offset: u64) -> Result<CursorCheck, TicketDeny> {
        let last = self.cursor_offsets.get(path_key).copied();
        let is_resume = last.is_some_and(|last| offset < last);
        self.quotas.check_cursor(is_resume)?;
        Ok(CursorCheck { is_resume })
    }

    fn cursor_offset(&self, path_key: &str) -> Option<u64> {
        self.cursor_offsets.get(path_key).copied()
    }

    fn record_cursor(&mut self, path_key: String, offset: u64, len: usize, check: CursorCheck) {
        let next = offset.saturating_add(len as u64);
        self.cursor_offsets.insert(path_key, next);
        self.quotas.consume_cursor(check.is_resume);
    }

    fn bandwidth_limit(&self) -> Option<u64> {
        self.quotas.bandwidth_limit()
    }

    fn cursor_resume_limit(&self) -> Option<u32> {
        self.quotas.cursor_resume_limit()
    }

    fn cursor_advance_limit(&self) -> Option<u32> {
        self.quotas.cursor_advance_limit()
    }

    fn best_scope_index(
        &self,
        path: &[String],
        verb: TicketVerb,
        allow_ancestor: bool,
    ) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (idx, scope) in self.scopes.iter().enumerate() {
            if !scope.allows_verb(verb) {
                continue;
            }
            if !scope.matches_path(path, allow_ancestor) {
                continue;
            }
            let match_len = scope.path.len();
            if best.is_none_or(|(_, best_len)| match_len > best_len) {
                best = Some((idx, match_len));
            }
        }
        best.map(|(idx, _)| idx)
    }
}

fn split_scope_path(path: &str, max_len: u16) -> Result<Vec<String>, TicketClaimError> {
    if path.is_empty() || path == "/" {
        return Ok(Vec::new());
    }
    let components: Vec<String> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect();
    if components.iter().any(|segment| segment == "..") {
        return Err(TicketClaimError::ScopePath {
            path: path.to_owned(),
            max_len,
        });
    }
    Ok(components)
}

fn split_request_path(path: &str) -> Option<Vec<String>> {
    if path.is_empty() {
        return Some(Vec::new());
    }
    if !path.starts_with('/') {
        return None;
    }
    let components: Vec<String> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect();
    if components.iter().any(|segment| segment == "..") {
        return None;
    }
    Some(components)
}

fn resolve_quotas(
    quotas: TicketQuotas,
    limits: crate::generated::TicketLimits,
) -> Result<TicketQuotaState, TicketClaimError> {
    let bandwidth_limit = resolve_quota_u64(
        quotas.bandwidth_bytes,
        limits.bandwidth_bytes,
        |value, max| TicketClaimError::Bandwidth { value, max },
    )?;
    let cursor_resume_limit = resolve_quota_u32(
        quotas.cursor_resumes,
        limits.cursor_resumes,
        |value, max| TicketClaimError::CursorResumes { value, max },
    )?;
    let cursor_advance_limit = resolve_quota_u32(
        quotas.cursor_advances,
        limits.cursor_advances,
        |value, max| TicketClaimError::CursorAdvances { value, max },
    )?;
    Ok(TicketQuotaState {
        bandwidth_limit,
        bandwidth_remaining: bandwidth_limit,
        cursor_resume_limit,
        cursor_resume_remaining: cursor_resume_limit,
        cursor_advance_limit,
        cursor_advance_remaining: cursor_advance_limit,
    })
}

fn resolve_quota_u64<F>(
    value: Option<u64>,
    max: u64,
    err: F,
) -> Result<Option<u64>, TicketClaimError>
where
    F: FnOnce(u64, u64) -> TicketClaimError,
{
    match value {
        Some(value) => {
            if max > 0 && value > max {
                return Err(err(value, max));
            }
            Ok(Some(value))
        }
        None => Ok((max > 0).then_some(max)),
    }
}

fn resolve_quota_u32<F>(
    value: Option<u32>,
    max: u32,
    err: F,
) -> Result<Option<u32>, TicketClaimError>
where
    F: FnOnce(u32, u32) -> TicketClaimError,
{
    match value {
        Some(value) => {
            if max > 0 && value > max {
                return Err(err(value, max));
            }
            Ok(Some(value))
        }
        None => Ok((max > 0).then_some(max)),
    }
}

fn ticket_verb_label(verb: TicketVerb) -> &'static str {
    match verb {
        TicketVerb::Read => "read",
        TicketVerb::Write => "write",
        TicketVerb::ReadWrite => "read-write",
    }
}

fn ticket_deny_reason(deny: TicketDeny) -> &'static str {
    match deny {
        TicketDeny::Scope => "scope",
        TicketDeny::Rate { .. } => "rate",
        TicketDeny::Bandwidth { .. } => "bandwidth",
        TicketDeny::CursorResume { .. } => "cursor-resume",
        TicketDeny::CursorAdvance { .. } => "cursor-advance",
    }
}

fn is_telemetry_path(path: &str) -> bool {
    path.ends_with("/telemetry")
}

/// Snapshot of event pump metrics used for diagnostics.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PumpMetrics {
    /// Number of console lines processed across serial and TCP transports.
    pub console_lines: u64,
    /// Commands rejected due to missing authentication.
    pub denied_commands: u64,
    /// Commands executed successfully.
    pub accepted_commands: u64,
    /// UI-oriented reads (tail/cat) accepted by the console.
    pub ui_reads: u64,
    /// UI-oriented denials (unauthenticated reads).
    pub ui_denies: u64,
    /// Timer ticks processed.
    pub timer_ticks: u64,
    /// Poll turns where local-seat keyboard input was consumed before runtime work.
    pub local_seat_keyboard_priority_turns: u64,
    /// Poll turns where runtime work was skipped because keyboard input was ready.
    pub local_seat_runtime_skipped_turns: u64,
    /// Poll turns where the serial dispatch slot yielded to local-seat input.
    pub local_seat_serial_dispatch_yielded_turns: u64,
    /// Poll turns where keyboard input arrived immediately after runtime work.
    pub local_seat_post_runtime_hits: u64,
    /// Backend keyboard polls issued while console output was being emitted.
    pub local_seat_output_keyboard_polls: u64,
    /// Queued HDMI mirror frames submitted during idle local-seat display turns.
    pub local_seat_hdmi_pump_turns: u64,
    /// Network-origin lines accepted for best-effort HDMI mirroring.
    pub local_seat_net_mirror_lines: u64,
    /// Network-origin lines skipped because HDMI was already pressured.
    pub local_seat_net_mirror_suppressed: u64,
    /// Post-dispatch TCP flush polls run after network-origin commands.
    pub net_post_dispatch_flush_polls: u64,
    /// Network-origin command batches that still had TCP work after the flush cap.
    pub net_post_dispatch_flush_exhaustions: u64,
    /// Physical-console output records deferred behind active keyboard input.
    pub physical_console_output_deferred: u64,
    /// Deferred physical-console output records flushed during idle turns.
    pub physical_console_output_flushed: u64,
    /// Physical-console output records that could not fit in the deferred ring.
    pub physical_console_output_backpressure: u64,
    #[cfg(feature = "kernel")]
    /// Bootstrap IPC messages processed.
    pub bootstrap_messages: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SmpActivitySnapshot {
    now_ms: u64,
    metrics: PumpMetrics,
    serial: SerialTelemetry,
    local_seat: Option<SmpLocalSeatActivitySnapshot>,
    #[cfg(feature = "net-console")]
    net: Option<SmpNetActivitySnapshot>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SmpLocalSeatActivitySnapshot {
    backend_poll_calls: u64,
    drained_bytes: u64,
    echoed_bytes: u64,
    dropped_bytes: u64,
    mirrored_line_drops: u64,
    budget_overruns: u64,
    no_replies: u64,
    display_no_reply_frames: u64,
    display_backpressure_bytes: u64,
}

#[cfg(feature = "net-console")]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SmpNetActivitySnapshot {
    counters: crate::net::NetCounters,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SmpActivityRates {
    command_per_s: u64,
    line_per_s: u64,
    tick_per_s: u64,
    serial_drop_per_s: u64,
    seat_drop_per_s: u64,
    seat_no_reply_per_s: u64,
    hdmi_drop_per_s: u64,
    net_drop_per_s: u64,
    seat_poll_per_s: u64,
    keyboard_bytes_per_s: u64,
    display_bytes_per_s: u64,
    net_rx_per_s: u64,
    net_tx_per_s: u64,
    tcp_bytes_per_s: u64,
    drop_per_s: u64,
}

impl SmpActivityRates {
    fn from_snapshots(
        previous: SmpActivitySnapshot,
        current: SmpActivitySnapshot,
        window_ms: u64,
        include_authority: bool,
        include_serial: bool,
        include_local_seat: bool,
        include_net: bool,
    ) -> Self {
        let mut rates = Self::default();
        if include_authority {
            let previous_commands = previous
                .metrics
                .accepted_commands
                .saturating_add(previous.metrics.denied_commands);
            let current_commands = current
                .metrics
                .accepted_commands
                .saturating_add(current.metrics.denied_commands);
            rates.command_per_s = rate_per_second(
                current_commands.saturating_sub(previous_commands),
                window_ms,
            );
            rates.line_per_s = rate_per_second(
                current
                    .metrics
                    .console_lines
                    .saturating_sub(previous.metrics.console_lines),
                window_ms,
            );
            rates.tick_per_s = rate_per_second(
                current
                    .metrics
                    .timer_ticks
                    .saturating_sub(previous.metrics.timer_ticks),
                window_ms,
            );
        }
        if include_serial {
            rates.serial_drop_per_s = rate_per_second(
                delta_u32(
                    current.serial.rx_backpressure,
                    previous.serial.rx_backpressure,
                )
                .saturating_add(delta_u32(
                    current.serial.tx_backpressure,
                    previous.serial.tx_backpressure,
                ))
                .saturating_add(delta_u32(
                    current.serial.utf8_dropped,
                    previous.serial.utf8_dropped,
                ))
                .saturating_add(delta_u32(
                    current.serial.driver_task_budget_overruns,
                    previous.serial.driver_task_budget_overruns,
                )),
                window_ms,
            );
        }
        if include_local_seat {
            let previous_local = previous.local_seat.unwrap_or_default();
            let current_local = current.local_seat.unwrap_or_default();
            rates.seat_poll_per_s = rate_per_second(
                current_local
                    .backend_poll_calls
                    .saturating_sub(previous_local.backend_poll_calls),
                window_ms,
            );
            rates.keyboard_bytes_per_s = rate_per_second(
                current_local
                    .drained_bytes
                    .saturating_sub(previous_local.drained_bytes),
                window_ms,
            );
            rates.display_bytes_per_s = rate_per_second(
                current_local
                    .echoed_bytes
                    .saturating_sub(previous_local.echoed_bytes),
                window_ms,
            );
            rates.seat_drop_per_s = rate_per_second(
                current_local
                    .dropped_bytes
                    .saturating_sub(previous_local.dropped_bytes)
                    .saturating_add(
                        current_local
                            .mirrored_line_drops
                            .saturating_sub(previous_local.mirrored_line_drops),
                    )
                    .saturating_add(
                        current_local
                            .budget_overruns
                            .saturating_sub(previous_local.budget_overruns),
                    ),
                window_ms,
            );
            rates.seat_no_reply_per_s = rate_per_second(
                current_local
                    .no_replies
                    .saturating_sub(previous_local.no_replies),
                window_ms,
            );
            rates.hdmi_drop_per_s = rate_per_second(
                current_local
                    .display_no_reply_frames
                    .saturating_sub(previous_local.display_no_reply_frames)
                    .saturating_add(
                        current_local
                            .display_backpressure_bytes
                            .saturating_sub(previous_local.display_backpressure_bytes),
                    ),
                window_ms,
            );
            rates.drop_per_s = rates
                .drop_per_s
                .saturating_add(rates.seat_drop_per_s)
                .saturating_add(rates.hdmi_drop_per_s);
        }
        if include_net {
            #[cfg(feature = "net-console")]
            if let (Some(previous_net), Some(current_net)) = (previous.net, current.net) {
                rates.net_rx_per_s = rate_per_second(
                    current_net
                        .counters
                        .rx_packets
                        .saturating_sub(previous_net.counters.rx_packets),
                    window_ms,
                );
                rates.net_tx_per_s = rate_per_second(
                    current_net
                        .counters
                        .tx_packets
                        .saturating_sub(previous_net.counters.tx_packets),
                    window_ms,
                );
                rates.tcp_bytes_per_s = rate_per_second(
                    current_net
                        .counters
                        .tcp_rx_bytes
                        .saturating_sub(previous_net.counters.tcp_rx_bytes)
                        .saturating_add(
                            current_net
                                .counters
                                .tcp_tx_bytes
                                .saturating_sub(previous_net.counters.tcp_tx_bytes),
                        ),
                    window_ms,
                );
                rates.net_drop_per_s = rate_per_second(
                    current_net
                        .counters
                        .dropped_zero_len_tx
                        .saturating_sub(previous_net.counters.dropped_zero_len_tx)
                        .saturating_add(
                            current_net
                                .counters
                                .tx_double_submit
                                .saturating_sub(previous_net.counters.tx_double_submit),
                        ),
                    window_ms,
                );
                rates.drop_per_s = rates.drop_per_s.saturating_add(rates.net_drop_per_s);
            }
        }
        rates
    }
}

fn delta_u32(current: u32, previous: u32) -> u64 {
    u64::from(current.saturating_sub(previous))
}

fn rate_per_second(delta: u64, window_ms: u64) -> u64 {
    if window_ms == 0 {
        return 0;
    }
    delta.saturating_mul(1_000).saturating_add(window_ms / 2) / window_ms
}

/// Authenticated session state maintained by the pump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionRole {
    Queen,
    Worker,
}

impl SessionRole {
    fn from_role(role: Role) -> Option<Self> {
        match role {
            Role::Queen => Some(Self::Queen),
            Role::WorkerHeartbeat | Role::WorkerGpu | Role::WorkerBus | Role::WorkerLora => {
                Some(Self::Worker)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefusalReason {
    Busy,
    Quota,
    Cut,
    Policy,
}

impl RefusalReason {
    fn as_str(self) -> &'static str {
        match self {
            RefusalReason::Busy => "busy",
            RefusalReason::Quota => "quota",
            RefusalReason::Cut => "cut",
            RefusalReason::Policy => "policy",
        }
    }

    fn pressure_kind(self) -> PressureKind {
        match self {
            RefusalReason::Busy => PressureKind::Busy,
            RefusalReason::Quota => PressureKind::Quota,
            RefusalReason::Cut => PressureKind::Cut,
            RefusalReason::Policy => PressureKind::Policy,
        }
    }
}

/// Exponential back-off helper used when authentication repeatedly fails.
#[derive(Debug, Default, Clone, Copy)]
struct AuthThrottle {
    failures: u32,
    blocked_until_ms: u64,
}

impl AuthThrottle {
    const BASE_BACKOFF_MS: u64 = 250;
    const MAX_SHIFT: u32 = 8;

    fn register_failure(&mut self, now_ms: u64) {
        let shift = min(self.failures, Self::MAX_SHIFT);
        let delay = Self::BASE_BACKOFF_MS.saturating_mul(1u64 << shift);
        self.failures = self.failures.saturating_add(1);
        self.blocked_until_ms = now_ms.saturating_add(delay);
    }

    fn register_success(&mut self) {
        self.failures = 0;
        self.blocked_until_ms = 0;
    }

    fn check(&self, now_ms: u64) -> Result<(), u64> {
        if now_ms < self.blocked_until_ms {
            Err(self.blocked_until_ms.saturating_sub(now_ms))
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "net-console")]
#[derive(Clone, Copy)]
struct NetDiagLogSnapshot {
    snapshot: NetDiagSnapshot,
    link_up: bool,
    tx_drops: u32,
}

#[cfg(feature = "net-console")]
#[repr(u8)]
#[derive(Clone, Copy, Debug)]
enum NetDiagRateKind {
    Summary = 0,
}

#[cfg(feature = "net-console")]
impl RateLimitKey for NetDiagRateKind {
    const COUNT: usize = NET_DIAG_RATE_KINDS;

    fn index(self) -> usize {
        self as usize
    }
}

/// Networking integration exposed to the pump when the `net` feature is enabled.
/// Event pump orchestrating serial, timer, IPC, and optional networking work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConsoleInputSource {
    Serial,
    LocalSeat,
    Net,
}

impl ConsoleInputSource {
    const fn label(self) -> &'static str {
        match self {
            Self::Serial => "serial",
            Self::LocalSeat => "local-seat",
            Self::Net => "net",
        }
    }

    const fn is_physical_console(self) -> bool {
        matches!(self, Self::Serial | Self::LocalSeat)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingConsoleOutputKind {
    Line,
    Prompt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingConsoleOutput {
    kind: PendingConsoleOutputKind,
    text: HeaplessString<DEFAULT_LINE_CAPACITY>,
}

impl PendingConsoleOutput {
    fn from_str(kind: PendingConsoleOutputKind, text: &str) -> Self {
        let mut buffered = HeaplessString::new();
        for ch in text.chars() {
            if buffered.push(ch).is_err() {
                break;
            }
        }
        Self {
            kind,
            text: buffered,
        }
    }
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WifiDebugCommand {
    Help,
    DumpState,
    ProbeHt,
    Diag,
    LoadFirmware,
    Retry,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UsbDebugCommand {
    Help,
    Status,
    DumpState,
    Diag,
    EnableKeyboard,
    ProbeKeyboard,
}

#[cfg(feature = "kernel")]
#[derive(Debug)]
struct PendingCursor {
    path_key: String,
    offset: u64,
    len: usize,
    check: CursorCheck,
}

#[cfg(feature = "kernel")]
#[derive(Debug)]
struct PendingStream {
    lines:
        HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, { log_buffer::LOG_EXPORT_BATCH_LINES }>,
    next_line: usize,
    bandwidth_bytes: u64,
    cursor: Option<PendingCursor>,
    log_cursor: Option<log_buffer::LogCursor>,
}

#[cfg(feature = "kernel")]
const NET_PENDING_STREAM_FLUSH_LINES_PER_TURN: usize = 48;
#[cfg(feature = "kernel")]
const NET_PENDING_STREAM_FLUSH_BYTES_PER_TURN: usize = 16 * 1024;
#[cfg(feature = "kernel")]
const LOCAL_PENDING_STREAM_FLUSH_LINES_PER_TURN: usize = log_buffer::LOG_EXPORT_BATCH_LINES;
#[cfg(feature = "kernel")]
const LOCAL_PENDING_STREAM_FLUSH_BYTES_PER_TURN: usize = 16 * 1024;

#[cfg(feature = "kernel")]
impl PendingStream {
    fn new() -> Self {
        Self {
            lines: HeaplessVec::new(),
            next_line: 0,
            bandwidth_bytes: 0,
            cursor: None,
            log_cursor: None,
        }
    }

    fn reset(&mut self) {
        self.lines.clear();
        self.next_line = 0;
        self.bandwidth_bytes = 0;
        self.cursor = None;
        self.log_cursor = None;
    }
}

pub struct EventPump<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>
where
    D: SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    serial: SerialPort<D, RX, TX, LINE>,
    parser: CommandParser,
    local_line: HeaplessString<LINE>,
    timer: T,
    ipc: I,
    validator: V,
    audit: &'a mut dyn AuditSink,
    metrics: PumpMetrics,
    now_ms: u64,
    session: Option<SessionRole>,
    session_role: Option<Role>,
    session_ticket: Option<String>,
    ticket_usage: Option<TicketUsage>,
    session_id: Option<u64>,
    session_origin: Option<ConsoleInputSource>,
    next_session_id: u64,
    last_input_source: ConsoleInputSource,
    stream_end_pending: bool,
    tail_active: bool,
    throttle: AuthThrottle,
    #[cfg(feature = "kernel")]
    pending_stream: Option<PendingStream>,
    #[cfg(feature = "net-console")]
    net: Option<&'a mut dyn NetPoller>,
    #[cfg(feature = "net-console")]
    net_unavailable_detail: Option<HeaplessString<192>>,
    #[cfg(feature = "net-console")]
    net_conn_id: Option<u64>,
    #[cfg(feature = "net-console")]
    last_net_diag_log_ms: Option<u64>,
    #[cfg(feature = "net-console")]
    last_net_diag_emitted: Option<NetDiagLogSnapshot>,
    #[cfg(feature = "net-console")]
    last_net_diag_snapshot: Option<NetDiagSnapshot>,
    #[cfg(feature = "net-console")]
    net_diag_limiter: RateLimiter<NET_DIAG_RATE_KINDS>,
    #[cfg(feature = "net-console")]
    net_diag_stuck_logged: bool,
    #[cfg(feature = "net-console")]
    wifi_credential_warning_emitted: bool,
    #[cfg(feature = "kernel")]
    ninedoor: Option<&'a mut NineDoorBridge>,
    #[cfg(feature = "kernel")]
    bootstrap_handler: Option<&'a mut dyn BootstrapMessageHandler>,
    #[cfg(feature = "kernel")]
    console_context: Option<ConsoleContext>,
    #[cfg(feature = "kernel")]
    wifi_debug: Option<&'a mut dyn WifiDebugOps>,
    local_seat: Option<&'a mut LocalSeatRuntime>,
    #[cfg(test)]
    test_pi4_debug_commands: bool,
    banner_emitted: bool,
    serial_console_turn_active: bool,
    console_input_turn_active: bool,
    console_input_turn_output_budget: usize,
    local_seat_chunk_input_pending: bool,
    console_output_flush_active: bool,
    local_seat_mirror_suppressed: bool,
    reboot_pending: bool,
    reboot_flush_turns: u8,
    #[cfg(feature = "kernel")]
    pending_usb_debug_hdmi_frontier: Option<HeaplessString<DEFAULT_LINE_CAPACITY>>,
    local_seat_escape_state: LocalSeatEscapeState,
    pending_console_output: HeaplessVec<PendingConsoleOutput, CONSOLE_OUTPUT_BACKLOG_LINES>,
    #[cfg(feature = "kernel")]
    serial_input_idle_trace_next_ms: u64,
    #[cfg(feature = "kernel")]
    serial_input_idle_trace_count: u8,
    #[cfg(feature = "kernel")]
    serial_input_idle_trace_waiting_for_quiet_output: bool,
    #[cfg(all(feature = "kernel", feature = "usb"))]
    post_prompt_local_seat_attach_pending: bool,
    #[cfg(all(feature = "kernel", feature = "usb"))]
    post_prompt_local_seat_attach_not_before_ms: u64,
    #[cfg(all(feature = "kernel", feature = "usb"))]
    post_prompt_local_seat_attach_idle_turns: u8,
    #[cfg(all(feature = "kernel", feature = "usb"))]
    post_prompt_local_seat_attach_blocked_traces: u8,
    #[cfg(all(feature = "kernel", feature = "usb"))]
    post_prompt_local_seat_attach_retry_turns: u16,
    #[cfg(all(feature = "kernel", feature = "usb"))]
    post_prompt_local_seat_attach_attempts: u16,
    #[cfg(all(feature = "kernel", feature = "usb"))]
    post_prompt_local_seat_attach_active_usb_traced: bool,
    #[cfg(all(test, feature = "kernel", feature = "usb"))]
    post_prompt_local_seat_attach_usb_active_override: Option<bool>,
    last_smp_activity_snapshot: Option<SmpActivitySnapshot>,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy)]
struct ConsoleContext {
    bootinfo: BootInfoView,
    ep_slot: seL4_CPtr,
    uart_slot: Option<seL4_CPtr>,
}

impl<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>
    EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    /// Create a new event pump backed by the supplied subsystems.
    pub fn new(
        serial: SerialPort<D, RX, TX, LINE>,
        timer: T,
        ipc: I,
        validator: V,
        audit: &'a mut dyn AuditSink,
    ) -> Self {
        audit.info("event-pump: init serial");
        audit.info("event-pump: init timer");
        audit.info("event-pump: init ipc");
        Self {
            serial,
            parser: CommandParser::new(),
            local_line: HeaplessString::new(),
            timer,
            ipc,
            validator,
            audit,
            metrics: PumpMetrics::default(),
            now_ms: 0,
            session: None,
            session_role: None,
            session_ticket: None,
            ticket_usage: None,
            session_id: None,
            session_origin: None,
            next_session_id: 1,
            last_input_source: ConsoleInputSource::Serial,
            stream_end_pending: false,
            tail_active: false,
            throttle: AuthThrottle::default(),
            #[cfg(feature = "kernel")]
            pending_stream: None,
            #[cfg(feature = "net-console")]
            net: None,
            #[cfg(feature = "net-console")]
            net_unavailable_detail: None,
            #[cfg(feature = "net-console")]
            net_conn_id: None,
            #[cfg(feature = "net-console")]
            last_net_diag_log_ms: None,
            #[cfg(feature = "net-console")]
            last_net_diag_emitted: None,
            #[cfg(feature = "net-console")]
            last_net_diag_snapshot: None,
            #[cfg(feature = "net-console")]
            net_diag_limiter: RateLimiter::<NET_DIAG_RATE_KINDS>::new(NET_DIAG_RATE_LIMIT_MS),
            #[cfg(feature = "net-console")]
            net_diag_stuck_logged: false,
            #[cfg(feature = "net-console")]
            wifi_credential_warning_emitted: false,
            #[cfg(feature = "kernel")]
            ninedoor: None,
            #[cfg(feature = "kernel")]
            bootstrap_handler: None,
            #[cfg(feature = "kernel")]
            console_context: None,
            #[cfg(feature = "kernel")]
            wifi_debug: None,
            local_seat: None,
            #[cfg(test)]
            test_pi4_debug_commands: false,
            banner_emitted: false,
            serial_console_turn_active: false,
            console_input_turn_active: false,
            console_input_turn_output_budget: 0,
            local_seat_chunk_input_pending: false,
            console_output_flush_active: false,
            local_seat_mirror_suppressed: false,
            reboot_pending: false,
            reboot_flush_turns: 0,
            #[cfg(feature = "kernel")]
            pending_usb_debug_hdmi_frontier: None,
            local_seat_escape_state: LocalSeatEscapeState::Idle,
            pending_console_output: HeaplessVec::new(),
            #[cfg(feature = "kernel")]
            serial_input_idle_trace_next_ms: 0,
            #[cfg(feature = "kernel")]
            serial_input_idle_trace_count: 0,
            #[cfg(feature = "kernel")]
            serial_input_idle_trace_waiting_for_quiet_output: false,
            #[cfg(all(feature = "kernel", feature = "usb"))]
            post_prompt_local_seat_attach_pending: false,
            #[cfg(all(feature = "kernel", feature = "usb"))]
            post_prompt_local_seat_attach_not_before_ms: 0,
            #[cfg(all(feature = "kernel", feature = "usb"))]
            post_prompt_local_seat_attach_idle_turns: 0,
            #[cfg(all(feature = "kernel", feature = "usb"))]
            post_prompt_local_seat_attach_blocked_traces: 0,
            #[cfg(all(feature = "kernel", feature = "usb"))]
            post_prompt_local_seat_attach_retry_turns: 0,
            #[cfg(all(feature = "kernel", feature = "usb"))]
            post_prompt_local_seat_attach_attempts: 0,
            #[cfg(all(feature = "kernel", feature = "usb"))]
            post_prompt_local_seat_attach_active_usb_traced: false,
            #[cfg(all(test, feature = "kernel", feature = "usb"))]
            post_prompt_local_seat_attach_usb_active_override: None,
            last_smp_activity_snapshot: None,
        }
    }

    /// Attach a networking poller to the event pump.
    #[cfg(feature = "net-console")]
    pub fn with_network(mut self, net: &'a mut dyn NetPoller) -> Self {
        self.audit.info("event-pump: init network");
        self.net = Some(net);
        self
    }

    /// Attach the preserved reason why the network stack was unavailable.
    #[cfg(feature = "net-console")]
    pub fn with_network_unavailable_detail(mut self, detail: Option<HeaplessString<192>>) -> Self {
        self.net_unavailable_detail = detail;
        self
    }

    /// Attach a NineDoor handler to the event pump.
    #[cfg(feature = "kernel")]
    pub fn with_ninedoor(mut self, bridge: &'a mut NineDoorBridge) -> Self {
        self.ninedoor = Some(bridge);
        self
    }

    /// Attach a local-seat runtime for keyboard ingress and mirrored egress.
    pub fn with_local_seat(mut self, runtime: &'a mut LocalSeatRuntime) -> Self {
        runtime.register_boot_progress_backend();
        self.local_seat = Some(runtime);
        self
    }

    /// Route a pre-prompt high-impact boot progress line to HDMI when the
    /// linked display runtime is available.
    pub fn publish_pre_root_boot_progress(&mut self, line: &str) {
        if let Some(runtime) = self.local_seat.as_mut() {
            runtime.register_boot_progress_backend();
            runtime.mirror_high_impact_line(line);
        }
    }

    #[cfg(feature = "kernel")]
    /// Attach boot-time console metadata for diagnostic commands.
    pub fn with_console_context(
        mut self,
        bootinfo: BootInfoView,
        ep_slot: seL4_CPtr,
        uart_slot: Option<seL4_CPtr>,
    ) -> Self {
        self.console_context = Some(ConsoleContext {
            bootinfo,
            ep_slot,
            uart_slot,
        });
        self
    }

    #[cfg(feature = "kernel")]
    /// Attach a bootstrap IPC handler that consumes staged messages.
    pub fn with_bootstrap_handler(mut self, handler: &'a mut dyn BootstrapMessageHandler) -> Self {
        self.bootstrap_handler = Some(handler);
        self.ipc.handlers_ready();
        self
    }

    #[cfg(feature = "kernel")]
    /// Attach a serial/local-seat Wi-Fi bring-up debug surface.
    pub fn with_wifi_debug(mut self, wifi_debug: &'a mut dyn WifiDebugOps) -> Self {
        self.wifi_debug = Some(wifi_debug);
        self
    }

    #[cfg(test)]
    fn with_test_pi4_debug_commands(mut self) -> Self {
        self.test_pi4_debug_commands = true;
        self
    }

    /// Execute a single cooperative polling cycle.
    pub fn poll(&mut self) {
        let serial_rx_activity = self.serial.poll_io();
        let local_seat_input_waiting = self
            .local_seat
            .as_ref()
            .is_some_and(|runtime| runtime.keyboard_trace().queued_bytes != 0);
        if serial_rx_activity
            || (self.serial.interactive_input_active() && !local_seat_input_waiting)
        {
            self.serial_console_turn_active = true;
            let serial_input = self.consume_serial();
            self.serial.flush_tx();
            let serial_output_pending = self.serial.tx_pending();
            self.poll_runtime(false, true);
            self.serial.poll_io();
            let serial_followup_input = if self.local_seat.is_some() {
                false
            } else {
                self.consume_serial()
            };
            self.serial.flush_tx();
            let serial_turn_should_return = serial_rx_activity
                || serial_input
                || serial_followup_input
                || serial_output_pending
                || self.serial.interactive_input_active()
                || self.serial.tx_pending();
            self.serial_console_turn_active = false;
            if serial_turn_should_return {
                return;
            }
        }
        let local_input = self.consume_local_seat(LocalSeatConsumePhase::PreRuntime, true);
        if local_input {
            self.serial.flush_tx();
            self.serial.poll_io();
            self.consume_local_seat(LocalSeatConsumePhase::PriorityFollowup, false);
            self.pump_local_seat_display_after_local_input();
            self.serial.flush_tx();
            return;
        }
        let serial_input = self.consume_serial();
        self.serial.flush_tx();
        let serial_output_pending = self.serial.tx_pending();
        self.poll_runtime(
            false,
            serial_rx_activity || serial_input || local_input || serial_output_pending,
        );
        self.serial.poll_io();
        let post_runtime_local_input =
            self.consume_local_seat(LocalSeatConsumePhase::PostRuntime, false);
        if !local_input && !post_runtime_local_input && !serial_input {
            let followup_serial_input = self.consume_serial();
            self.serial.flush_tx();
            self.flush_pending_console_output_if_idle();
            self.retry_pending_usb_debug_hdmi_frontier();
            self.pump_local_seat_display_if_idle();
            self.maybe_run_post_prompt_local_seat_attach(
                serial_rx_activity
                    || serial_input
                    || followup_serial_input
                    || local_input
                    || post_runtime_local_input,
            );
            self.maybe_emit_serial_input_idle_trace(
                serial_rx_activity
                    || serial_input
                    || followup_serial_input
                    || local_input
                    || post_runtime_local_input,
            );
            return;
        }
        self.serial.flush_tx();
        self.flush_pending_console_output_if_idle();
        self.retry_pending_usb_debug_hdmi_frontier();
        self.pump_local_seat_display_if_idle();
        self.maybe_run_post_prompt_local_seat_attach(
            serial_rx_activity || serial_input || local_input || post_runtime_local_input,
        );
        self.maybe_emit_serial_input_idle_trace(
            serial_rx_activity || serial_input || local_input || post_runtime_local_input,
        );
    }

    #[cfg(feature = "net-console")]
    /// Execute the pre-root network/timer portion of the pump without accepting
    /// console input.
    pub fn poll_pre_root_network(&mut self) {
        self.poll_runtime(true, false);
    }

    fn poll_runtime(&mut self, suppress_console_input: bool, physical_input_active: bool) {
        #[cfg(not(feature = "net-console"))]
        let _ = suppress_console_input;
        #[cfg(not(feature = "net-console"))]
        let _ = physical_input_active;

        #[cfg(feature = "kernel")]
        let timebase_now_ms = crate::hal::timebase().now_ms();
        #[cfg(not(feature = "kernel"))]
        let timebase_now_ms = self.now_ms;

        if let Some(tick) = self.timer.poll(timebase_now_ms) {
            self.now_ms = tick.now_ms;
            self.metrics.timer_ticks = self.metrics.timer_ticks.saturating_add(1);
            crate::hal::set_timebase_now_ms(self.now_ms);
            #[cfg(feature = "timer-trace")]
            if tick.tick % 8_000 == 0 {
                let message = format_message(format_args!(
                    "timer: tick {} (now_ms={})",
                    tick.tick, self.now_ms
                ));
                self.audit.info(message.as_str());
            }
        } else {
            self.now_ms = timebase_now_ms;
        }

        #[cfg(feature = "net-console")]
        let local_seat_first_report_pending = self.linked_local_seat_first_report_pending();
        #[cfg(feature = "net-console")]
        let net_poll = if let Some(net) = self.net.as_mut() {
            let status_before = net.status_report();
            let should_yield_before = net_status_should_yield_to_physical_input(&status_before);
            let host_eapol_pending_before = net_status_needs_host_eapol_burst(&status_before);
            let service_under_physical_pressure =
                net_status_needs_physical_pressure_service(&status_before);
            let local_seat_input_pressure = net_physical_input_pressure_for_status(
                physical_input_active,
                local_seat_first_report_pending,
                host_eapol_pending_before,
            );
            let net_contract = net.driver_task_contract();
            let network_data_yields_to_input = net_contract
                .validate()
                .map(|_| !net_contract.preempts_network_data())
                .unwrap_or(true);
            let yield_for_physical_input = local_seat_input_pressure
                && !suppress_console_input
                && !service_under_physical_pressure
                && (should_yield_before || network_data_yields_to_input);
            let mut activity = false;
            let mut net_budget = DriverServiceBudget::new(net_contract).ok();
            // Host-EAPOL pending/required has no DHCP/data progress yet; once
            // the root console is available, yield those SDIO polls to active
            // keyboards. The same contract rule applies to all NIC data paths:
            // active serial/USB input or pending serial output owns this
            // event-pump turn.
            if !yield_for_physical_input {
                if let Some(budget) = net_budget.as_mut() {
                    match net.poll_with_budget(self.now_ms, budget) {
                        Ok(polled) => activity = polled,
                        Err(err) => {
                            let message = format_message(format_args!(
                                "BUDGET_OVERRUN contract={} budget_overrun=1 reason={} service_us={}",
                                net_contract.name,
                                err.reason(),
                                net_contract.max_service_us(),
                            ));
                            self.audit.denied(message.as_str());
                        }
                    }
                }
            }
            let host_eapol_pending = if yield_for_physical_input {
                host_eapol_pending_before
            } else {
                net_status_needs_host_eapol_burst(&net.status_report())
            };
            let burst_limit = if host_eapol_pending && !yield_for_physical_input {
                if suppress_console_input {
                    WIFI_HOST_EAPOL_PRE_ROOT_BURST_POLLS
                } else {
                    WIFI_HOST_EAPOL_RUNTIME_BURST_POLLS
                }
            } else {
                0
            };
            for _ in 0..burst_limit {
                if !net_status_needs_host_eapol_burst(&net.status_report()) {
                    break;
                }
                let mut burst_budget = match DriverServiceBudget::new(net_contract) {
                    Ok(budget) => budget,
                    Err(err) => {
                        let message = format_message(format_args!(
                            "BUDGET_OVERRUN contract={} budget_overrun=1 reason={} service_us={}",
                            net_contract.name,
                            err.reason(),
                            net_contract.max_service_us(),
                        ));
                        self.audit.denied(message.as_str());
                        break;
                    }
                };
                match net.poll_with_budget(self.now_ms, &mut burst_budget) {
                    Ok(polled) => activity |= polled,
                    Err(err) => {
                        let message = format_message(format_args!(
                            "BUDGET_OVERRUN contract={} budget_overrun=1 reason={} service_us={}",
                            net_contract.name,
                            err.reason(),
                            net_contract.max_service_us(),
                        ));
                        self.audit.denied(message.as_str());
                        break;
                    }
                }
            }
            let telemetry = net.telemetry();
            let conn_id = net.active_console_conn_id();
            let hot_dispatch_rounds = net_hot_dispatch_rounds_for_status(&net.status_report());
            let mut buffered: HeaplessVec<ConsoleLine, { CONSOLE_DISPATCH_BURST }> =
                HeaplessVec::new();
            if !yield_for_physical_input {
                let _ = net.drain_console_lines_bounded(
                    self.now_ms,
                    CONSOLE_DISPATCH_BURST,
                    &mut |line| {
                        let _ = buffered.push(line);
                    },
                );
            }
            let ingest_snapshot: IngestSnapshot = net.ingest_snapshot();
            Some((
                activity,
                telemetry,
                buffered,
                conn_id,
                ingest_snapshot,
                hot_dispatch_rounds,
            ))
        } else {
            None
        };

        #[cfg(feature = "net-console")]
        if let Some((
            activity,
            telemetry,
            buffered,
            conn_id,
            _ingest_snapshot,
            hot_dispatch_rounds,
        )) = net_poll
        {
            self.net_conn_id = conn_id;
            if NET_DIAG_FEATURED {
                self.log_net_diag(telemetry);
            } else if activity {
                let message = format_message(format_args!(
                    "net: poll link_up={} tx_drops={}",
                    telemetry.link_up, telemetry.tx_drops
                ));
                self.audit.info(message.as_str());
            }
            let mut ingest_snapshot = _ingest_snapshot;
            for line in buffered {
                if !suppress_console_input {
                    self.handle_network_line(line.text);
                    if let Some(snapshot) = self.poll_net_after_network_dispatch() {
                        ingest_snapshot = snapshot;
                    }
                }
            }
            let mut remaining_hot_rounds = hot_dispatch_rounds.saturating_sub(1);
            while !suppress_console_input && remaining_hot_rounds != 0 {
                let mut buffered: HeaplessVec<ConsoleLine, { CONSOLE_DISPATCH_BURST }> =
                    HeaplessVec::new();
                let drained = if let Some(net) = self.net.as_mut() {
                    net.drain_console_lines_bounded(
                        self.now_ms,
                        CONSOLE_DISPATCH_BURST,
                        &mut |line| {
                            let _ = buffered.push(line);
                        },
                    )
                } else {
                    0
                };
                if drained == 0 {
                    break;
                }
                for line in buffered {
                    self.handle_network_line(line.text);
                    if let Some(snapshot) = self.poll_net_after_network_dispatch() {
                        ingest_snapshot = snapshot;
                    }
                }
                remaining_hot_rounds = remaining_hot_rounds.saturating_sub(1);
            }
            #[cfg(not(feature = "kernel"))]
            let _ = ingest_snapshot;
            #[cfg(feature = "kernel")]
            if let Some(bridge) = self.ninedoor.as_mut() {
                bridge.update_ingest_snapshot(ingest_snapshot);
            }
            self.drain_net_console_events();
        }

        self.ipc.dispatch(self.now_ms);
        #[cfg(feature = "kernel")]
        self.drain_bootstrap_ipc();
        #[cfg(feature = "kernel")]
        self.flush_pending_stream();
        self.service_pending_reboot();
    }

    #[cfg(feature = "net-console")]
    fn poll_net_after_network_dispatch(&mut self) -> Option<IngestSnapshot> {
        let display_trace = self
            .local_seat
            .as_ref()
            .map(|local_seat| local_seat.display_trace());
        let Some(net) = self.net.as_mut() else {
            return None;
        };
        let flush_limit =
            net_post_dispatch_flush_limit_for_status(&net.status_report(), display_trace);
        let net_contract = net.driver_task_contract();
        let mut flush_polls = 0u64;
        let mut flush_exhausted = false;
        for _ in 0..flush_limit {
            let mut net_budget = match DriverServiceBudget::new(net_contract) {
                Ok(budget) => budget,
                Err(err) => {
                    let message = format_message(format_args!(
                        "BUDGET_OVERRUN contract={} budget_overrun=1 reason={} service_us={}",
                        net_contract.name,
                        err.reason(),
                        net_contract.max_service_us(),
                    ));
                    self.audit.denied(message.as_str());
                    return Some(net.ingest_snapshot());
                }
            };
            match net.flush_tcp_with_budget(self.now_ms, &mut net_budget) {
                Ok(polled) => {
                    flush_polls = flush_polls.saturating_add(1);
                    if !polled {
                        flush_exhausted = false;
                        break;
                    }
                    flush_exhausted = true;
                }
                Err(err) => {
                    let message = format_message(format_args!(
                        "BUDGET_OVERRUN contract={} budget_overrun=1 reason={} service_us={}",
                        net_contract.name,
                        err.reason(),
                        net_contract.max_service_us(),
                    ));
                    self.audit.denied(message.as_str());
                    flush_polls = flush_polls.saturating_add(1);
                    flush_exhausted = false;
                    break;
                }
            }
        }
        self.metrics.net_post_dispatch_flush_polls = self
            .metrics
            .net_post_dispatch_flush_polls
            .saturating_add(flush_polls);
        if flush_exhausted && flush_polls >= flush_limit as u64 {
            self.metrics.net_post_dispatch_flush_exhaustions = self
                .metrics
                .net_post_dispatch_flush_exhaustions
                .saturating_add(1);
        }

        Some(net.ingest_snapshot())
    }

    fn service_pending_reboot(&mut self) {
        if !self.reboot_pending {
            return;
        }
        if self.reboot_flush_turns != 0 {
            self.reboot_flush_turns = self.reboot_flush_turns.saturating_sub(1);
            return;
        }
        let line = format_message(format_args!(
            "console: reboot firing source={}",
            self.last_input_source.label()
        ));
        self.audit.info(line.as_str());
        #[cfg(feature = "kernel")]
        boot_log::force_uart_line_raw("[reboot] platform reset request firing");
        match crate::reboot::request_reboot() {
            Ok(()) => {
                self.reboot_pending = false;
            }
            Err(err) => {
                self.reboot_pending = false;
                self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
                let message =
                    format_message(format_args!("reboot request failed: {}", err.detail()));
                self.audit.denied(message.as_str());
                let detail = format_message(format_args!("detail={}", err.detail()));
                self.emit_refusal("REBOOT", RefusalReason::Policy, Some(detail.as_str()));
            }
        }
    }

    #[cfg(feature = "net-console")]
    // Activity-only logging to prevent endless spam in steady state.
    fn should_log_net_diag(&self, snapshot: NetDiagSnapshot, telemetry: NetTelemetry) -> bool {
        if Self::net_diag_idle(snapshot, telemetry) {
            return false;
        }
        self.last_net_diag_emitted.map_or(true, |prev| {
            Self::net_diag_changed(prev.snapshot, snapshot)
                || prev.link_up != telemetry.link_up
                || prev.tx_drops != telemetry.tx_drops
        })
    }

    #[cfg(feature = "net-console")]
    fn net_diag_changed(prev: NetDiagSnapshot, curr: NetDiagSnapshot) -> bool {
        let mut prev = prev;
        let mut curr = curr;
        // Ignore per-packet throughput counters here. NETDIAG remains useful for
        // structural changes and backpressure, while healthy benchmark traffic
        // should not keep adding diagnostic lines to the measured log path.
        prev.rx_irq_count = 0;
        curr.rx_irq_count = 0;
        prev.rx_kicks = 0;
        curr.rx_kicks = 0;
        prev.rx_desc_posted = 0;
        curr.rx_desc_posted = 0;
        prev.rx_used_seen = 0;
        curr.rx_used_seen = 0;
        prev.rx_frames_to_stack = 0;
        curr.rx_frames_to_stack = 0;
        prev.poll_calls = 0;
        curr.poll_calls = 0;
        prev.rx_frames_into_smoltcp = 0;
        curr.rx_frames_into_smoltcp = 0;
        prev.accept_attempts = 0;
        curr.accept_attempts = 0;
        prev.bytes_read = 0;
        curr.bytes_read = 0;
        prev.bytes_written = 0;
        curr.bytes_written = 0;
        prev.rx_cache_clean = 0;
        curr.rx_cache_clean = 0;
        prev.rx_cache_invalidate = 0;
        curr.rx_cache_invalidate = 0;
        prev.tx_submits = 0;
        curr.tx_submits = 0;
        prev.tx_kicks = 0;
        curr.tx_kicks = 0;
        prev.tx_used_seen = 0;
        curr.tx_used_seen = 0;
        prev.tx_completions = 0;
        curr.tx_completions = 0;
        prev.tx_frames_from_smoltcp = 0;
        curr.tx_frames_from_smoltcp = 0;
        prev.outbound_frames = 0;
        curr.outbound_frames = 0;
        prev.outbound_bytes = 0;
        curr.outbound_bytes = 0;
        prev != curr
    }

    #[cfg(feature = "net-console")]
    #[cfg_attr(not(test), allow(dead_code))]
    fn net_diag_idle(snapshot: NetDiagSnapshot, telemetry: NetTelemetry) -> bool {
        telemetry.tx_drops == 0
            && snapshot.bytes_read == 0
            && snapshot.bytes_written == 0
            && snapshot.accept_attempts == 0
            && snapshot.accept_success == 0
            && snapshot.rx_frames_to_stack == 0
            && snapshot.rx_frames_into_smoltcp == 0
            && snapshot.tx_frames_from_smoltcp == 0
            && snapshot.outbound_queued_lines == 0
            && snapshot.outbound_queued_bytes == 0
            && snapshot.outbound_drops == 0
            && snapshot.outbound_would_block == 0
            && snapshot.tx_submits == 0
            && snapshot.tx_completions == 0
    }

    #[cfg(feature = "net-console")]
    fn log_net_diag(&mut self, telemetry: NetTelemetry) {
        if !NET_DIAG_FEATURED {
            return;
        }
        let snapshot = NET_DIAG.snapshot();
        if self.should_log_net_diag(snapshot, telemetry) {
            if let Some(suppressed) = self
                .net_diag_limiter
                .check(NetDiagRateKind::Summary, self.now_ms)
            {
                let line = format_message(format_args!(
                    "NETDIAG in_bytes={} out_bytes={} rx_stack={} rx_smoltcp={} tx_smoltcp={} tx_drops={} link={} q_lines={} q_bytes={} q_drops={} q_wblk={} suppressed={}",
                    snapshot.bytes_read,
                    snapshot.bytes_written,
                    snapshot.rx_frames_to_stack,
                    snapshot.rx_frames_into_smoltcp,
                    snapshot.tx_frames_from_smoltcp,
                    telemetry.tx_drops,
                    telemetry.link_up,
                    snapshot.outbound_queued_lines,
                    snapshot.outbound_queued_bytes,
                    snapshot.outbound_drops,
                    snapshot.outbound_would_block,
                    suppressed,
                ));
                self.audit.info(line.as_str());
                self.last_net_diag_log_ms = Some(self.now_ms);
                self.last_net_diag_emitted = Some(NetDiagLogSnapshot {
                    snapshot,
                    link_up: telemetry.link_up,
                    tx_drops: telemetry.tx_drops,
                });
            }
        }
        self.check_net_diag_progress(snapshot);
        self.last_net_diag_snapshot = Some(snapshot);
    }

    #[cfg(feature = "net-console")]
    fn check_net_diag_progress(&mut self, snapshot: NetDiagSnapshot) {
        if let Some(prev) = self.last_net_diag_snapshot {
            if snapshot.rx_used_seen != prev.rx_used_seen {
                self.net_diag_stuck_logged = false;
            }
            let poll_delta = snapshot.poll_calls.saturating_sub(prev.poll_calls);
            let irq_delta = snapshot.rx_irq_count.saturating_sub(prev.rx_irq_count);
            let last_progress_ms = NET_DIAG.last_rx_used_change_ms();
            if poll_delta > 0
                && irq_delta > 0
                && last_progress_ms > 0
                && self.now_ms.saturating_sub(last_progress_ms) >= NET_DIAG_STUCK_MS
                && !self.net_diag_stuck_logged
            {
                let warn_line = format_message(format_args!(
                    "NETDIAG warn: rx_used_stuck ms={} poll_delta={} irq_delta={} rx_used={}",
                    self.now_ms.saturating_sub(last_progress_ms),
                    poll_delta,
                    irq_delta,
                    snapshot.rx_used_seen
                ));
                self.audit.info(warn_line.as_str());
                self.net_diag_stuck_logged = true;
                NET_DIAG.mark_stuck_warned();
            }
        }
    }

    #[cfg(feature = "kernel")]
    /// Run the bootstrap probe loop until an IPC message has been staged.
    pub fn bootstrap_probe(&mut self) {
        log::trace!("B5: entering bootstrap probe loop");
        let mut backoff = BootstrapBackoff::new(BOOTSTRAP_IDLE_SPINS);
        loop {
            let handled_before = self.metrics.bootstrap_messages;
            if self.ipc.bootstrap_poll(self.now_ms) {
                self.drain_bootstrap_ipc();
            }
            self.poll();
            if self.metrics.bootstrap_messages != handled_before {
                break;
            }
            if let Some(spins) = backoff.observe(self.ipc.has_staged_bootstrap()) {
                let summary = format_message(format_args!(
                    "bootstrap-ipc: idle after {spins} polls; continuing"
                ));
                self.audit.info(summary.as_str());
                break;
            }
            crate::sel4::yield_now();
        }
    }

    /// Emit console audit messages once the UART bridge is connected.
    pub fn announce_console_ready(&mut self) {
        self.emit_serial_line("Cohesix console ready");
        self.emit_help_serial_only();
        #[cfg(feature = "net-console")]
        self.emit_wifi_credential_warning_current_before_prompt();
        debug_uart_str("[dbg] console: writing 'cohesix>' prompt\n");
        #[cfg(feature = "kernel")]
        boot_log::force_uart_line_raw_without_prompt_refresh(
            "[mark] root-console.prompt.write.begin",
        );
        self.emit_prompt();
        #[cfg(feature = "kernel")]
        boot_log::force_uart_line_raw_without_prompt_refresh("[mark] root-console.prompt.write.ok");
        self.serial.poll_io();
        #[cfg(feature = "kernel")]
        boot_log::set_serial_prompt_refresh_after_logs(
            crate::generated::hardware_config().local_seat.enabled,
        );
        if let Some(runtime) = self.local_seat.as_mut() {
            runtime.mark_root_console_ready();
            runtime.mirror_line("Cohesix console ready");
            runtime.mirror_prompt(CONSOLE_PROMPT);
        }
        #[cfg(feature = "kernel")]
        if self.ninedoor.is_some() {
            if boot_log::switch_logger_to_log_buffer() {
                boot_log::force_uart_line_raw_without_prompt_refresh(
                    "[trace] log channel switched to /log/queen.log; raw driver blockers remain on serial",
                );
            } else {
                boot_log::force_uart_line_raw_without_prompt_refresh(
                    "[trace] log channel remains on serial; raw driver blockers preserved",
                );
            }
        }
        self.schedule_post_prompt_local_seat_attach();
        self.audit.info("console: attach uart");
        #[cfg(feature = "kernel")]
        if let Some(bridge) = self.ninedoor.as_mut() {
            match bridge.log_stream(&mut *self.audit) {
                Ok(()) => {
                    self.audit.info("console: log stream start");
                }
                Err(err) => {
                    let summary =
                        format_message(format_args!("console: log stream failed: {}", err));
                    self.audit.info(summary.as_str());
                }
            }
        } else {
            self.audit
                .info("console: log stream deferred (bridge unavailable)");
        }
    }

    #[cfg(feature = "kernel")]
    fn drain_bootstrap_ipc(&mut self) {
        while let Some(message) = self.ipc.take_bootstrap_message() {
            self.metrics.bootstrap_messages = self.metrics.bootstrap_messages.saturating_add(1);
            if let Some(handler) = self.bootstrap_handler.as_mut() {
                handler.handle(&message, &mut *self.audit);
            } else {
                let summary = format_message(format_args!(
                    "bootstrap-ipc: badge=0x{badge:016x} label=0x{label:08x} words={words}",
                    badge = message.badge,
                    label = message.info.words[0],
                    words = message.payload.len(),
                ));
                self.audit.info(summary.as_str());
            }
        }
    }

    /// Emit the interactive banner and publish the initial prompt before any
    /// deferred local-seat settle attempt.
    pub fn start_cli(&mut self) {
        debug_uart_str("[dbg] console: root console task entry\n");
        #[cfg(feature = "kernel")]
        if let Some(context) = self.console_context {
            log::info!(
                target: "root_task::console",
                "[console] starting root shell ep=0x{ep:04x} uart=0x{uart:04x}",
                ep = context.ep_slot,
                uart = context.uart_slot.unwrap_or(crate::sel4::seL4_CapNull),
            );
        }
        self.emit_serial_line(CONSOLE_BANNER);
        self.emit_serial_line("Cohesix console starting");
        #[cfg(feature = "net-console")]
        if let Some(net) = self.net.as_mut() {
            let _ = net.send_console_line(
                "[net-console] authenticate using AUTH <role> <token> to receive console output",
            );
        }
        #[cfg(feature = "kernel")]
        {
            if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
                && crate::serial::resume_serial_driver_task_runtime_after_prompt()
            {
                if crate::serial::probe_driver_task_rx_after_attach()
                    && crate::serial::serial_driver_task_interactive_cutover_allowed()
                    && self.serial.use_driver_task_client_after_attach()
                {
                    boot_log::force_uart_line_raw(
                        "[uart] serial console cutover backend=driver-task-serial-client owner=serial",
                    );
                    boot_log::force_uart_line_raw(
                        "SERIAL_RUNTIME_STATE owner=root stage=serial-runtime-init status=cutover acceptance=green reason=driver-task-attached",
                    );
                    crate::serial::emit_serial_input_route_trace(
                        "root-console-start",
                        "driver-task-rx-proven",
                    );
                    self.serial.poll_io();
                } else {
                    boot_log::force_uart_line_raw(
                        "[uart] serial console cutover deferred backend=bcm2711-mini-uart reason=driver-task-rx-proof-missing action=root-uart-console",
                    );
                    crate::serial::emit_serial_runtime_cutover_deferred(
                        "driver-task-rx-proof-missing",
                    );
                    crate::serial::emit_serial_input_route_trace(
                        "root-console-start",
                        "driver-task-rx-proof-missing",
                    );
                }
            }
        }
        if !self.banner_emitted {
            log::info!(target: "event", "[event] root console banner emitted");
            self.banner_emitted = true;
        }
    }

    /// Run the cooperative pump until shutdown.
    pub fn run(mut self) -> ! {
        log::info!(
            target: "event",
            "[event] pump starting: root_console={}, net_console_enabled={}, ninedoor_enabled={}",
            self.has_root_console(),
            self.net_console_enabled(),
            self.ninedoor_enabled(),
        );

        loop {
            self.poll();
            #[cfg(feature = "kernel")]
            sel4::yield_now();
            #[cfg(not(feature = "kernel"))]
            core::hint::spin_loop();
        }
    }

    /// Returns whether the root console is attached.
    pub fn has_root_console(&self) -> bool {
        true
    }

    /// Returns whether net-console handling is enabled.
    pub fn net_console_enabled(&self) -> bool {
        #[cfg(feature = "net-console")]
        {
            return self.net.is_some();
        }
        #[cfg(not(feature = "net-console"))]
        {
            false
        }
    }

    /// Returns the selected network interface label for net-console status messages.
    #[cfg(feature = "net-console")]
    pub fn net_console_active_interface(&self) -> Option<&'static str> {
        self.net
            .as_ref()
            .map(|net| net.status_report().active_interface)
    }

    /// Returns whether the active network transport is ready to carry the root console.
    #[cfg(feature = "net-console")]
    pub fn net_console_ready_for_root(&self) -> bool {
        match self.net.as_ref() {
            Some(net) => net_status_allows_root_console(&net.status_report()),
            None => false,
        }
    }

    #[cfg(feature = "net-console")]
    pub fn net_console_terminal_failure_reason(&self) -> Option<&'static str> {
        self.net
            .as_ref()
            .and_then(|net| net_status_terminal_failure_reason(&net.status_report()))
    }

    #[cfg(feature = "net-console")]
    pub fn net_console_pre_root_serial_release_reason(&self) -> Option<&'static str> {
        self.net
            .as_ref()
            .and_then(|net| net_status_pre_root_serial_release_reason(&net.status_report()))
    }

    /// Returns whether the NineDoor bridge is enabled.
    pub fn ninedoor_enabled(&self) -> bool {
        #[cfg(feature = "kernel")]
        {
            return self.ninedoor.is_some();
        }
        #[cfg(not(feature = "kernel"))]
        {
            false
        }
    }

    /// Retrieve a snapshot of the current pump metrics.
    #[must_use]
    pub fn metrics(&self) -> PumpMetrics {
        self.metrics
    }

    /// Obtain the most recent serial telemetry.
    #[must_use]
    pub fn serial_telemetry(&self) -> SerialTelemetry {
        self.serial.telemetry()
    }

    /// Emit a console line to the serial console and any attached TCP clients.
    pub fn emit_console_line(&mut self, line: &str) {
        if !self.try_emit_console_line(line) {
            #[cfg(feature = "cohesix-dev")]
            {
                let source = self.last_input_source.label();
                let message = format_message(format_args!(
                    "audit console.emit.failed source={} line={}",
                    source, line
                ));
                crate::debug_uart::debug_uart_line(message.as_str());
            }
        }
    }

    fn service_local_seat_keyboard_during_output(&mut self) {
        if self.serial_console_turn_active || self.serial.interactive_input_active() {
            return;
        }
        if let Some(runtime) = self.local_seat.as_mut() {
            if !runtime.root_console_ready() {
                return;
            }
            for _ in 0..LOCAL_SEAT_OUTPUT_KEYBOARD_POLL_PASSES {
                runtime.poll_backend_keyboard();
                runtime.drain_display_control_bytes_during_output(KEYBOARD_POLL_CHUNK_BYTES);
                self.metrics.local_seat_output_keyboard_polls = self
                    .metrics
                    .local_seat_output_keyboard_polls
                    .saturating_add(1);
            }
        }
    }

    fn physical_console_input_pending_for_output(&self) -> bool {
        self.serial.interactive_input_active()
            || !self.local_line.is_empty()
            || self.local_seat_chunk_input_pending
            || self
                .local_seat
                .as_ref()
                .is_some_and(|runtime| runtime.keyboard_trace().queued_bytes != 0)
    }

    fn physical_console_input_pending_for_display_pump(&self) -> bool {
        self.serial.interactive_input_active()
            || self.local_seat_chunk_input_pending
            || self
                .local_seat
                .as_ref()
                .is_some_and(|runtime| runtime.keyboard_trace().queued_bytes != 0)
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn linked_local_seat_first_report_pending(&self) -> bool {
        crate::local_seat::linked_local_seat_usb_keyboard_ready()
            && !crate::local_seat::linked_local_seat_usb_first_report_ready()
    }

    #[cfg(not(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )))]
    const fn linked_local_seat_first_report_pending(&self) -> bool {
        false
    }

    fn should_defer_physical_console_output(&mut self) -> bool {
        if self.console_output_flush_active {
            return false;
        }
        if self.serial.tx_pending() && self.last_input_source == ConsoleInputSource::LocalSeat {
            return true;
        }
        if !self.physical_console_input_pending_for_output() {
            return false;
        }
        if self.console_input_turn_active && self.console_input_turn_output_budget != 0 {
            self.console_input_turn_output_budget =
                self.console_input_turn_output_budget.saturating_sub(1);
            return false;
        }
        true
    }

    fn queue_physical_console_output(
        &mut self,
        kind: PendingConsoleOutputKind,
        text: &str,
    ) -> bool {
        let output = PendingConsoleOutput::from_str(kind, text);
        if self.pending_console_output.push(output).is_ok() {
            self.metrics.physical_console_output_deferred = self
                .metrics
                .physical_console_output_deferred
                .saturating_add(1);
            true
        } else {
            self.metrics.physical_console_output_backpressure = self
                .metrics
                .physical_console_output_backpressure
                .saturating_add(1);
            false
        }
    }

    fn flush_pending_console_output_if_idle(&mut self) {
        if self.pending_console_output.is_empty()
            || self.serial.tx_pending()
            || self.physical_console_input_pending_for_output()
        {
            return;
        }
        self.console_output_flush_active = true;
        let mut flushed = 0usize;
        while flushed < CONSOLE_OUTPUT_LINES_PER_IDLE_TURN
            && !self.pending_console_output.is_empty()
            && !self.serial.tx_pending()
            && !self.physical_console_input_pending_for_output()
        {
            let output = self.pending_console_output.remove(0);
            self.with_local_seat_mirror_suppressed(|this| match output.kind {
                PendingConsoleOutputKind::Line => this.emit_serial_line_now(output.text.as_str()),
                PendingConsoleOutputKind::Prompt => this.emit_prompt_now(),
            });
            self.metrics.physical_console_output_flushed = self
                .metrics
                .physical_console_output_flushed
                .saturating_add(1);
            flushed = flushed.saturating_add(1);
            let _ = self.serial.poll_io();
            self.service_local_seat_keyboard_during_output();
        }
        self.console_output_flush_active = false;
    }

    fn pump_local_seat_display_if_idle(&mut self) {
        if self.serial.tx_pending()
            || self.serial.interactive_input_active()
            || self.console_output_flush_active
            || !self.pending_console_output.is_empty()
            || self.physical_console_input_pending_for_display_pump()
        {
            return;
        }
        self.pump_local_seat_display_once();
    }

    fn pump_local_seat_display_after_local_input(&mut self) {
        if self.serial.interactive_input_active()
            || self.console_output_flush_active
            || self.physical_console_input_pending_for_display_pump()
        {
            return;
        }
        self.pump_local_seat_display_once();
    }

    fn pump_local_seat_display_once(&mut self) {
        let pass_limit = self.local_seat_hdmi_pump_pass_limit();
        let Some(runtime) = self.local_seat.as_mut() else {
            return;
        };
        let mut passes = 0usize;
        while passes < pass_limit && runtime.linked_hdmi_pending_work() {
            if !runtime.pump_linked_hdmi_once() {
                break;
            }
            self.metrics.local_seat_hdmi_pump_turns =
                self.metrics.local_seat_hdmi_pump_turns.saturating_add(1);
            passes = passes.saturating_add(1);
        }
    }

    fn local_seat_hdmi_pump_pass_limit(&self) -> usize {
        #[cfg(feature = "net-console")]
        {
            if self.last_input_source == ConsoleInputSource::Net
                && self.metrics.local_seat_net_mirror_suppressed
                    > self.metrics.local_seat_net_mirror_lines
            {
                return LOCAL_SEAT_HDMI_PUMP_PASSES_UNDER_NET_PRESSURE;
            }
        }
        LOCAL_SEAT_HDMI_PUMP_PASSES_PER_TURN
    }

    #[cfg(feature = "kernel")]
    fn maybe_emit_serial_input_idle_trace(&mut self, physical_input_active: bool) {
        if !self.banner_emitted {
            return;
        }
        if physical_input_active || self.serial.interactive_input_active() {
            self.serial_input_idle_trace_next_ms = self
                .now_ms
                .saturating_add(SERIAL_INPUT_IDLE_TRACE_INTERVAL_MS);
            return;
        }
        if self.serial.tx_pending()
            || self.console_output_flush_active
            || !self.pending_console_output.is_empty()
            || self.physical_console_input_pending_for_output()
        {
            self.serial_input_idle_trace_waiting_for_quiet_output = true;
            self.serial_input_idle_trace_next_ms = self
                .now_ms
                .saturating_add(SERIAL_INPUT_IDLE_TRACE_INTERVAL_MS);
            return;
        }
        if self.serial_input_idle_trace_waiting_for_quiet_output {
            self.serial_input_idle_trace_waiting_for_quiet_output = false;
            self.serial_input_idle_trace_next_ms = self
                .now_ms
                .saturating_add(SERIAL_INPUT_IDLE_TRACE_INTERVAL_MS);
            return;
        }
        if self.serial_input_idle_trace_count >= SERIAL_INPUT_IDLE_TRACE_LIMIT {
            return;
        }
        if self.now_ms < self.serial_input_idle_trace_next_ms {
            return;
        }
        crate::serial::emit_serial_input_idle_trace(self.now_ms, self.serial.tx_pending());
        self.serial_input_idle_trace_count = self.serial_input_idle_trace_count.saturating_add(1);
        self.serial_input_idle_trace_next_ms = self
            .now_ms
            .saturating_add(SERIAL_INPUT_IDLE_TRACE_INTERVAL_MS);
    }

    #[cfg(not(feature = "kernel"))]
    fn maybe_emit_serial_input_idle_trace(&mut self, _physical_input_active: bool) {}

    fn mirror_local_seat_line_if_ready(&mut self, line: &str) {
        if self.local_seat_mirror_suppressed {
            return;
        }
        if self.serial.interactive_input_active() {
            return;
        }
        if let Some(runtime) = self.local_seat.as_mut() {
            if runtime.root_console_ready() {
                runtime.mirror_line(line);
            }
        }
    }

    fn mirror_local_seat_network_line_if_ready(&mut self, line: &str) {
        if self.local_seat_mirror_suppressed {
            return;
        }
        if self.serial.interactive_input_active() {
            return;
        }
        let Some(runtime) = self.local_seat.as_mut() else {
            return;
        };
        if !runtime.root_console_ready() {
            return;
        }
        let mirror_ordinal = self
            .metrics
            .local_seat_net_mirror_lines
            .saturating_add(self.metrics.local_seat_net_mirror_suppressed);
        if !local_seat_network_mirror_sample_allowed(mirror_ordinal) {
            self.metrics.local_seat_net_mirror_suppressed = self
                .metrics
                .local_seat_net_mirror_suppressed
                .saturating_add(1);
            return;
        }
        if runtime.can_accept_network_origin_mirror() {
            self.metrics.local_seat_net_mirror_lines =
                self.metrics.local_seat_net_mirror_lines.saturating_add(1);
            runtime.mirror_line(line);
        } else {
            self.metrics.local_seat_net_mirror_suppressed = self
                .metrics
                .local_seat_net_mirror_suppressed
                .saturating_add(1);
        }
    }

    fn mirror_local_seat_prompt_if_ready(&mut self) {
        if self.local_seat_mirror_suppressed {
            return;
        }
        if self.serial.interactive_input_active() {
            return;
        }
        if let Some(runtime) = self.local_seat.as_mut() {
            if runtime.root_console_ready() {
                runtime.mirror_prompt(CONSOLE_PROMPT);
            }
        }
    }

    fn with_local_seat_mirror_suppressed(&mut self, f: impl FnOnce(&mut Self)) {
        let previous = self.local_seat_mirror_suppressed;
        self.local_seat_mirror_suppressed = true;
        f(self);
        self.local_seat_mirror_suppressed = previous;
    }

    fn schedule_post_prompt_local_seat_attach(&mut self) {
        #[cfg(all(feature = "kernel", feature = "usb"))]
        {
            if self.local_seat.is_none() {
                return;
            }
            self.post_prompt_local_seat_attach_pending = true;
            self.post_prompt_local_seat_attach_idle_turns = 0;
            self.post_prompt_local_seat_attach_blocked_traces = 0;
            self.post_prompt_local_seat_attach_retry_turns = 0;
            self.post_prompt_local_seat_attach_attempts = 0;
            self.post_prompt_local_seat_attach_active_usb_traced = false;
            self.post_prompt_local_seat_attach_not_before_ms = self
                .now_ms
                .saturating_add(POST_PROMPT_LOCAL_SEAT_ATTACH_IDLE_GRACE_MS);
            #[cfg(all(target_arch = "aarch64", target_os = "none"))]
            boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(
                "[local-seat] prompt-settle attach scheduled action=idle-cooperative",
            );
        }
    }

    fn maybe_run_post_prompt_local_seat_attach(&mut self, physical_input_active: bool) {
        #[cfg(all(feature = "kernel", feature = "usb"))]
        {
            if !self.post_prompt_local_seat_attach_pending {
                return;
            }
            let usb_runtime_active = self.post_prompt_local_seat_attach_usb_runtime_active();
            let blocked_reason = if physical_input_active && !usb_runtime_active {
                Some("physical-input-active")
            } else if self.serial.interactive_input_active() {
                Some("serial-input-active")
            } else if self.now_ms < self.post_prompt_local_seat_attach_not_before_ms {
                Some("idle-grace")
            } else {
                None
            };
            if let Some(reason) = blocked_reason {
                self.emit_post_prompt_local_seat_attach_blocked(reason);
                self.post_prompt_local_seat_attach_idle_turns = 0;
                return;
            }
            if self.post_prompt_local_seat_attach_retry_turns != 0 {
                self.post_prompt_local_seat_attach_retry_turns = self
                    .post_prompt_local_seat_attach_retry_turns
                    .saturating_sub(1);
                self.emit_post_prompt_local_seat_attach_blocked("retry-turn-cooldown");
                self.post_prompt_local_seat_attach_idle_turns = 0;
                return;
            }
            self.post_prompt_local_seat_attach_idle_turns = self
                .post_prompt_local_seat_attach_idle_turns
                .saturating_add(1);
            if self.post_prompt_local_seat_attach_idle_turns
                < POST_PROMPT_LOCAL_SEAT_ATTACH_IDLE_TURNS
            {
                return;
            }
            if usb_runtime_active {
                self.trace_post_prompt_local_seat_attach_active_usb();
            }
            self.post_prompt_local_seat_attach_pending = false;
            self.post_prompt_local_seat_attach_idle_turns = 0;
            self.arm_post_prompt_local_seat_once();
        }
        #[cfg(not(all(feature = "kernel", feature = "usb")))]
        let _ = physical_input_active;
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    fn emit_post_prompt_local_seat_attach_blocked(&mut self, _reason: &'static str) {
        if self.post_prompt_local_seat_attach_blocked_traces >= 1 {
            return;
        }
        self.post_prompt_local_seat_attach_blocked_traces = self
            .post_prompt_local_seat_attach_blocked_traces
            .saturating_add(1);
        #[cfg(all(target_arch = "aarch64", target_os = "none"))]
        {
            let reason = _reason;
            let mut line = HeaplessString::<160>::new();
            let _ = write!(
                line,
                "[local-seat] post-prompt attach deferred reason={} idle_turns={} now_ms={}",
                reason, self.post_prompt_local_seat_attach_idle_turns, self.now_ms
            );
            boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(line.as_str());
        }
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    fn post_prompt_local_seat_attach_usb_runtime_active(&self) -> bool {
        #[cfg(test)]
        if let Some(active) = self.post_prompt_local_seat_attach_usb_active_override {
            return active;
        }
        crate::hal::driver_task::driver_task_ring_command_active(
            crate::hal::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
        )
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    fn trace_post_prompt_local_seat_attach_active_usb(&mut self) {
        if self.post_prompt_local_seat_attach_active_usb_traced {
            return;
        }
        self.post_prompt_local_seat_attach_active_usb_traced = true;
        #[cfg(all(target_arch = "aarch64", target_os = "none"))]
        {
            let mut line = HeaplessString::<160>::new();
            let _ = write!(
                line,
                "[local-seat] prompt-settle attach active-usb action=arm-cooperative retry_ms={}",
                POST_PROMPT_LOCAL_SEAT_ATTACH_ACTIVE_USB_RETRY_MS
            );
            boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(line.as_str());
        }
    }

    fn arm_post_prompt_local_seat_once(&mut self) {
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            #[cfg(feature = "net-console")]
            let wifi_detail = self.net_disabled_refusal_detail();
            #[cfg(feature = "net-console")]
            let wifi_debug_enabled = self.wifi_debug_commands_enabled();
            if let Some(runtime) = self.local_seat.as_mut() {
                let attempt = self.post_prompt_local_seat_attach_attempts;
                self.post_prompt_local_seat_attach_attempts = self
                    .post_prompt_local_seat_attach_attempts
                    .saturating_add(1);
                let verbose_attempt = attempt < POST_PROMPT_LOCAL_SEAT_ATTACH_VERBOSE_ATTEMPTS;
                if verbose_attempt {
                    boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(
                        "[local-seat] prompt-settle attach begin action=arm-cooperative",
                    );
                }
                let linked_display_ready = runtime.ensure_prompt_linked_display_ready();
                runtime.enable_backend_keyboard_polling();
                let usb_overruns_before = runtime.keyboard_trace().driver_task_budget_overruns;
                let keyboard_probe = runtime.probe_backend_keyboard_once();
                let usb_no_reply = !keyboard_probe.attached()
                    && runtime.keyboard_trace().driver_task_budget_overruns > usb_overruns_before;
                let usb_active = crate::hal::driver_task::driver_task_ring_command_active(
                    crate::hal::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
                );
                if verbose_attempt || keyboard_probe.attached() {
                    let usb_frontier = "[drivers] USB frontier: prompt-settle linked-runtime probe armed; xHCI/keyboard state preserved";
                    boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(usb_frontier);
                }
                #[cfg(feature = "net-console")]
                {
                    if verbose_attempt && wifi_debug_enabled {
                        let wifi_frontier = format_message(format_args!(
                            "[drivers] WiFi frontier: driver-task replay state preserved {}",
                            wifi_detail
                        ));
                        boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(
                            wifi_frontier.as_str(),
                        );
                    }
                }
                if verbose_attempt || keyboard_probe.attached() {
                    let mut line = HeaplessString::<160>::new();
                    let _ = write!(
                        line,
                        "[local-seat] prompt-settle attach end result=armed-cooperative linked_display={}",
                        if linked_display_ready {
                            "ready"
                        } else {
                            "deferred"
                        }
                    );
                    boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(line.as_str());
                    let mut probe_line = HeaplessString::<128>::new();
                    let _ = write!(
                        probe_line,
                        "[local-seat] prompt-settle usb probe result={}",
                        keyboard_probe.as_str()
                    );
                    boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(
                        probe_line.as_str(),
                    );
                }
                let retry_policy = if keyboard_probe.attached() {
                    None
                } else {
                    post_prompt_local_seat_attach_retry_policy(usb_no_reply, usb_active)
                };
                if let Some((retry_ms, retry_turns, retry_action)) = retry_policy {
                    self.post_prompt_local_seat_attach_pending = true;
                    self.post_prompt_local_seat_attach_idle_turns = 0;
                    self.post_prompt_local_seat_attach_retry_turns = retry_turns;
                    self.post_prompt_local_seat_attach_not_before_ms =
                        self.now_ms.saturating_add(retry_ms);
                    if verbose_attempt {
                        let mut retry_line = HeaplessString::<128>::new();
                        let _ = write!(
                            retry_line,
                            "[local-seat] prompt-settle attach retry scheduled action={} retry_ms={}",
                            retry_action, retry_ms
                        );
                        boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(
                            retry_line.as_str(),
                        );
                    }
                } else if usb_no_reply {
                    boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(
                        "[local-seat] prompt-settle attach suspended reason=driver-task-no-reply action=serial-shell explicit=usb-probe-kbd",
                    );
                }
            }
        }
    }

    fn try_emit_console_line(&mut self, line: &str) -> bool {
        if self.last_input_source.is_physical_console() {
            self.emit_serial_line(line);
            return true;
        }
        #[cfg(feature = "net-console")]
        if self.last_input_source == ConsoleInputSource::Net {
            let sent = if let Some(net) = self.net.as_mut() {
                net.send_console_line(line)
            } else {
                false
            };
            if sent {
                self.mirror_local_seat_network_line_if_ready(line);
            }
            return sent;
        }
        self.service_local_seat_keyboard_during_output();
        self.mirror_local_seat_line_if_ready(line);
        self.service_local_seat_keyboard_during_output();
        false
    }

    fn emit_serial_line(&mut self, line: &str) {
        if self.should_defer_physical_console_output()
            && self.queue_physical_console_output(PendingConsoleOutputKind::Line, line)
        {
            self.mirror_local_seat_line_if_ready(line);
            return;
        }
        self.emit_serial_line_now(line);
    }

    fn emit_serial_line_now(&mut self, line: &str) {
        self.service_local_seat_keyboard_during_output();
        if self.last_input_source == ConsoleInputSource::LocalSeat {
            self.serial.flush_tx();
            self.serial.enqueue_tx_best_effort(line.as_bytes());
            self.serial.enqueue_tx_best_effort(b"\r\n");
            self.serial.flush_tx();
        } else {
            self.emit_serial_bytes_cooperative(line.as_bytes());
            self.emit_serial_bytes_cooperative(b"\r\n");
        }
        self.service_local_seat_keyboard_during_output();
        self.mirror_local_seat_line_if_ready(line);
        self.service_local_seat_keyboard_during_output();
    }

    fn emit_prompt(&mut self) {
        if self.should_defer_physical_console_output()
            && self.queue_physical_console_output(PendingConsoleOutputKind::Prompt, CONSOLE_PROMPT)
        {
            self.mirror_local_seat_prompt_if_ready();
            return;
        }
        self.emit_prompt_now();
    }

    fn emit_prompt_now(&mut self) {
        self.service_local_seat_keyboard_during_output();
        if self.last_input_source == ConsoleInputSource::LocalSeat {
            self.serial.flush_tx();
            self.serial
                .enqueue_tx_best_effort(CONSOLE_PROMPT.as_bytes());
            self.serial.flush_tx();
        } else {
            self.emit_serial_bytes_cooperative(CONSOLE_PROMPT.as_bytes());
        }
        self.service_local_seat_keyboard_during_output();
        self.mirror_local_seat_prompt_if_ready();
        self.service_local_seat_keyboard_during_output();
    }

    fn emit_serial_bytes_cooperative(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(LOCAL_SEAT_SERIAL_OUTPUT_CHUNK_BYTES) {
            self.serial.enqueue_tx(chunk);
            self.serial.flush_tx();
            let _ = self.serial.poll_io();
            self.service_local_seat_keyboard_during_output();
        }
    }

    #[cfg(feature = "kernel")]
    fn preflush_serial_before_raw_uart(&mut self) {
        for _ in 0..SERIAL_RAW_UART_PREFLUSH_TURNS {
            if !self.serial.tx_pending() {
                break;
            }
            self.serial.flush_tx();
        }
    }

    fn emit_help(&mut self) {
        self.emit_console_line("Commands:");
        self.emit_console_line("  help  - Show this help");
        self.emit_console_line("  bi    - Show bootinfo summary");
        self.emit_console_line("  caps  - Show capability slots");
        self.emit_console_line("  smp [activity] - Show SMP scheduler info or userspace activity");
        self.emit_console_line("  mem   - Show untyped summary");
        self.emit_console_line("  ping  - Respond with pong");
        self.emit_console_line("  test  - Self-test (host-only; use cohsh)");
        self.emit_console_line("  nettest  - Run network self-test");
        self.emit_console_line("  netstats - Show network counters");
        #[cfg(feature = "kernel")]
        self.emit_usb_debug_help(false);
        #[cfg(feature = "kernel")]
        self.emit_wifi_debug_help(false);
        self.emit_console_line("  quit  - Exit the console session");
    }

    fn emit_help_serial_only(&mut self) {
        self.emit_serial_line("Commands:");
        self.emit_serial_line("  help  - Show this help");
        self.emit_serial_line("  bi    - Show bootinfo summary");
        self.emit_serial_line("  caps  - Show capability slots");
        self.emit_serial_line("  smp [activity] - Show SMP scheduler info or userspace activity");
        self.emit_serial_line("  mem   - Show untyped summary");
        self.emit_serial_line("  ping  - Respond with pong");
        self.emit_serial_line("  test  - Self-test (host-only; use cohsh)");
        self.emit_serial_line("  nettest  - Run network self-test");
        self.emit_serial_line("  netstats - Show network counters");
        #[cfg(feature = "kernel")]
        self.emit_usb_debug_help(true);
        #[cfg(feature = "kernel")]
        self.emit_wifi_debug_help(true);
        self.emit_serial_line("  quit  - Exit the console session");
    }

    #[cfg(feature = "kernel")]
    fn emit_usb_debug_help(&mut self, serial_only: bool) {
        if !self.usb_debug_commands_enabled() || self.last_input_source == ConsoleInputSource::Net {
            return;
        }
        let line = "  usb <help|status|dump-state|diag|enable-kbd|probe-kbd> - USB local-seat diagnostics (serial/local only)";
        if serial_only {
            self.emit_serial_line(line);
        } else {
            self.emit_console_line(line);
        }
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_debug_help(&mut self, serial_only: bool) {
        if !self.wifi_debug_commands_enabled() || self.last_input_source == ConsoleInputSource::Net
        {
            return;
        }
        let line = "  wifi <help|dump-state|probe-ht|diag|load-fw|retry> - WiFi bring-up diagnostics (serial/local only)";
        if serial_only {
            self.emit_serial_line(line);
        } else {
            self.emit_console_line(line);
        }
    }

    #[cfg(feature = "kernel")]
    fn emit_log_snapshot(&mut self) {
        let lines = log_buffer::snapshot_lines::<
            DEFAULT_LINE_CAPACITY,
            { log_buffer::LOG_SNAPSHOT_LINES },
        >();
        for line in lines {
            self.emit_console_line(line.as_str());
        }
    }

    #[cfg(feature = "kernel")]
    fn prepare_log_pending_stream(&mut self) -> u64 {
        let mut cursor = log_buffer::export_cursor();
        let stream_bytes = cursor.bytes();
        let pending = self.pending_stream.get_or_insert_with(PendingStream::new);
        pending.reset();
        let exhausted = log_buffer::read_cursor_lines_into(&mut cursor, &mut pending.lines);
        pending.log_cursor = if exhausted { None } else { Some(cursor) };
        pending.bandwidth_bytes = stream_bytes;
        stream_bytes
    }

    #[cfg(feature = "kernel")]
    fn prepare_log_tail_pending_stream(&mut self, requested_lines: Option<u16>) -> u64 {
        let default_lines =
            log_buffer::LOG_SNAPSHOT_LINES.min(usize::from(cohsh_core::command::MAX_TAIL_LINES));
        let lines = requested_lines
            .map(usize::from)
            .unwrap_or(default_lines)
            .clamp(1, usize::from(cohsh_core::command::MAX_TAIL_LINES));
        let mut cursor = log_buffer::tail_cursor(lines);
        let stream_bytes = cursor.bytes();
        let pending = self.pending_stream.get_or_insert_with(PendingStream::new);
        pending.reset();
        let exhausted = log_buffer::read_cursor_lines_into(&mut cursor, &mut pending.lines);
        pending.log_cursor = if exhausted { None } else { Some(cursor) };
        pending.bandwidth_bytes = stream_bytes;
        stream_bytes
    }

    #[cfg(feature = "kernel")]
    fn refill_log_pending_stream(pending: &mut PendingStream) -> bool {
        if pending.next_line < pending.lines.len() {
            return true;
        }
        let Some(mut cursor) = pending.log_cursor.take() else {
            return false;
        };
        pending.lines.clear();
        pending.next_line = 0;
        let exhausted = log_buffer::read_cursor_lines_into(&mut cursor, &mut pending.lines);
        pending.log_cursor = if exhausted { None } else { Some(cursor) };
        !pending.lines.is_empty()
    }

    #[cfg(feature = "kernel")]
    fn emit_bootinfo(&mut self) -> bool {
        let context = match self.console_context {
            Some(context) => context,
            None => return false,
        };
        let header = context.bootinfo.header();
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let _ = write!(
            line,
            "[bi] node_bits={} empty=[0x{:04x}..0x{:04x}) ",
            header.initThreadCNodeSizeBits, header.empty.start, header.empty.end,
        );
        if let Some(ptr) = header.ipc_buffer_ptr() {
            let addr = ptr.as_ptr() as usize;
            let width = core::mem::size_of::<usize>() * 2;
            let _ = write!(line, "ipc=0x{addr:0width$x}");
        } else {
            let _ = line.push_str("ipc=<none>");
        }
        self.emit_console_line(line.as_str());
        true
    }

    #[cfg(not(feature = "kernel"))]
    fn emit_bootinfo(&mut self) -> bool {
        let _ = self;
        false
    }

    #[cfg(feature = "kernel")]
    fn emit_caps(&mut self) -> bool {
        let context = match self.console_context {
            Some(context) => context,
            None => return false,
        };
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let _ = write!(
            line,
            "[caps] root=0x{:04x} ep=0x{:04x} uart=0x{:04x}",
            context.bootinfo.root_cnode_cap(),
            context.ep_slot,
            context.uart_slot.unwrap_or(sel4_sys::seL4_CapNull),
        );
        self.emit_console_line(line.as_str());
        true
    }

    #[cfg(not(feature = "kernel"))]
    fn emit_caps(&mut self) -> bool {
        let _ = self;
        false
    }

    fn emit_smp(&mut self, mode: SmpMode) -> Option<&'static str> {
        match mode {
            SmpMode::Snapshot => self.emit_smp_snapshot(),
            SmpMode::Activity => {
                self.emit_smp_activity();
                Some("mode=activity")
            }
        }
    }

    #[allow(unsafe_code)]
    #[cfg(all(feature = "kernel", sel4_config_debug_build))]
    fn emit_smp_snapshot(&mut self) -> Option<&'static str> {
        self.preflush_serial_before_raw_uart();
        self.emit_console_line("[smp] debug scheduler dump begin");
        self.serial.flush_tx();
        let policy = crate::affinity::policy();
        crate::affinity::debug_dump_per_core(&policy, |line| {
            self.emit_console_line(line);
            self.serial.flush_tx();
        });
        self.serial.flush_tx();
        self.emit_console_line("[smp] debug scheduler dump end");
        self.serial.flush_tx();
        Some("mode=snapshot")
    }

    #[cfg(not(all(feature = "kernel", sel4_config_debug_build)))]
    fn emit_smp_snapshot(&mut self) -> Option<&'static str> {
        self.emit_console_line("ERR reason=unsupported");
        None
    }

    fn emit_smp_activity(&mut self) {
        let snapshot = self.smp_activity_snapshot();
        let previous = self.last_smp_activity_snapshot;
        self.emit_console_line(
            "[smp] activity begin source=userspace benchmark=off hdmi=high-impact-only",
        );
        self.emit_smp_activity_pump();
        self.emit_smp_activity_local_seat();
        self.emit_smp_activity_net();
        self.emit_smp_activity_rates(previous, snapshot);
        self.emit_smp_activity_driver_contracts();
        self.emit_smp_activity_affinity();
        self.emit_console_line("[smp] activity end");
        self.last_smp_activity_snapshot = Some(snapshot);
    }

    fn smp_activity_snapshot(&self) -> SmpActivitySnapshot {
        let local_seat = self.local_seat.as_ref().map(|runtime| {
            let trace = runtime.keyboard_trace();
            let display = runtime.display_trace();
            SmpLocalSeatActivitySnapshot {
                backend_poll_calls: trace.backend_poll_calls,
                drained_bytes: trace.drained_bytes,
                echoed_bytes: trace.echoed_bytes,
                dropped_bytes: trace.dropped_bytes,
                mirrored_line_drops: runtime.dropped_mirrored_lines(),
                budget_overruns: trace.driver_task_budget_overruns,
                no_replies: trace.driver_task_no_replies,
                display_no_reply_frames: display.no_reply_frames,
                display_backpressure_bytes: display.backpressure_bytes,
            }
        });
        #[cfg(feature = "net-console")]
        let net = self.net.as_ref().map(|net| SmpNetActivitySnapshot {
            counters: net.stats(),
        });

        SmpActivitySnapshot {
            now_ms: self.now_ms,
            metrics: self.metrics,
            serial: self.serial_telemetry(),
            local_seat,
            #[cfg(feature = "net-console")]
            net,
        }
    }

    fn emit_smp_activity_pump(&mut self) {
        let metrics = self.metrics;
        let serial = self.serial_telemetry();
        let line = format_message(format_args!(
            "[smp] activity pump now_ms={} input={} lines={} ok={} denied={} ticks={} serial_rx_drop={} serial_tx_drop={} utf8_drop={} serial_budget_overruns={} serial_rx_backpressure={} serial_tx_backpressure={} serial_pressure_source=uart-output",
            self.now_ms,
            self.last_input_source.label(),
            metrics.console_lines,
            metrics.accepted_commands,
            metrics.denied_commands,
            metrics.timer_ticks,
            serial.rx_backpressure,
            serial.tx_backpressure,
            serial.utf8_dropped,
            serial.driver_task_budget_overruns,
            serial.rx_backpressure,
            serial.tx_backpressure,
        ));
        self.emit_console_line(line.as_str());
    }

    fn emit_smp_activity_local_seat(&mut self) {
        let Some(runtime) = self.local_seat.as_ref() else {
            self.emit_console_line("[smp] activity local-seat attached=no hdmi=unavailable");
            return;
        };
        let status = runtime.status();
        let trace = runtime.keyboard_trace();
        let mirrored_drops = runtime.dropped_mirrored_lines();
        let backend_enabled = runtime.backend_keyboard_polling_enabled();
        let backend_attached = runtime.backend_attached();
        #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
        let linked_keyboard_ready = crate::local_seat::linked_local_seat_usb_keyboard_ready();
        #[cfg(not(all(feature = "usb", target_arch = "aarch64", target_os = "none")))]
        let linked_keyboard_ready = false;
        #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
        let linked_first_report = crate::local_seat::linked_local_seat_usb_first_report_ready();
        #[cfg(not(all(feature = "usb", target_arch = "aarch64", target_os = "none")))]
        let linked_first_report = false;
        #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
        let linked_first_byte = crate::local_seat::linked_local_seat_usb_first_byte_ready();
        #[cfg(not(all(feature = "usb", target_arch = "aarch64", target_os = "none")))]
        let linked_first_byte = false;
        let metrics = self.metrics;
        let display = runtime.display_trace();
        let line = format_message(format_args!(
            "[smp] activity local-seat runtime=present attached={} keyboard_device={} display={} backend_poll={} backend_polls={} backend_bytes={} keyboard_ready={} first_report={} first_byte={} queued={} accepted={} drained={} echoed={} drop={} no_reply={} cooldown={} cooldown_skips={} hdmi_drop={}",
            Self::yes_no(backend_attached),
            status.keyboard_device,
            status.display_device,
            Self::yes_no(backend_enabled),
            trace.backend_poll_calls,
            trace.backend_read_bytes,
            Self::yes_no(linked_keyboard_ready || backend_attached),
            Self::yes_no(linked_first_report),
            Self::yes_no(linked_first_byte),
            trace.queued_bytes,
            trace.accepted_bytes,
            trace.drained_bytes,
            trace.echoed_bytes,
            trace.dropped_bytes,
            trace.driver_task_no_replies,
            trace.poll_cooldown_turns,
            trace.poll_cooldown_skips,
            mirrored_drops,
        ));
        self.emit_console_line(line.as_str());
        let turns = format_message(format_args!(
            "[smp] activity local-seat-turns output_polls={} hdmi_pump={} net_mirror={} net_suppressed={} priority={} skipped={} serial_yield={} post_runtime={}",
            metrics.local_seat_output_keyboard_polls,
            metrics.local_seat_hdmi_pump_turns,
            metrics.local_seat_net_mirror_lines,
            metrics.local_seat_net_mirror_suppressed,
            metrics.local_seat_keyboard_priority_turns,
            metrics.local_seat_runtime_skipped_turns,
            metrics.local_seat_serial_dispatch_yielded_turns,
            metrics.local_seat_post_runtime_hits,
        ));
        self.emit_console_line(turns.as_str());
        let display_line = format_message(format_args!(
            "[smp] activity local-seat-display pending_bytes={} redraw_bytes={} pending_redraw={} scrollback={} open_line={} submitted={} deferred={} busy={} no_reply={} redraw_no_reply={} coalesced={} backpressure_bytes={} superseded_bytes={}",
            display.pending_bytes,
            display.redraw_bytes,
            Self::yes_no(display.pending_redraw),
            display.scrollback_offset,
            Self::yes_no(display.open_line),
            display.submitted_frames,
            display.deferred_frames,
            display.busy_frames,
            display.no_reply_frames,
            display.redraw_no_reply_streak,
            display.coalesced_redraws,
            display.backpressure_bytes,
            display.superseded_bytes,
        ));
        self.emit_console_line(display_line.as_str());
    }

    fn emit_smp_activity_rates(
        &mut self,
        previous: Option<SmpActivitySnapshot>,
        current: SmpActivitySnapshot,
    ) {
        let Some(previous) = previous else {
            self.emit_console_line(
                "[smp] activity rates sample=first run_again=yes cpu_pct=unavailable",
            );
            return;
        };
        let window_ms = current.now_ms.saturating_sub(previous.now_ms);
        if window_ms == 0 {
            self.emit_console_line(
                "[smp] activity rates window_ms=0 status=stale run_again=yes cpu_pct=unavailable",
            );
            return;
        }
        let line = format_message(format_args!(
            "[smp] activity rates window_ms={} cpu_pct=unavailable view=counter-delta task_allocation=multi",
            window_ms,
        ));
        self.emit_console_line(line.as_str());
        self.emit_smp_activity_core_rates(previous, current, window_ms);
    }

    #[cfg(feature = "kernel")]
    fn emit_smp_activity_core_rates(
        &mut self,
        previous: SmpActivitySnapshot,
        current: SmpActivitySnapshot,
        window_ms: u64,
    ) {
        let policy = affinity::policy();
        if !policy.enabled || policy.max_cores == 0 {
            self.emit_smp_activity_unassigned_rates(previous, current, window_ms, "affinity-off");
            return;
        }

        for core in 0..policy.max_cores {
            let tasks = Self::format_smp_activity_core_assignments(&policy, core);
            let authority = policy.authority_core == Some(core);
            let serial = policy.drivers.serial == Some(core);
            let local_seat = Self::smp_activity_driver_assigned(
                crate::hal::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
                policy.drivers.usb_local_seat,
                core,
            ) || Self::smp_activity_driver_assigned(
                crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
                policy.drivers.hdmi_text,
                core,
            );
            let net = Self::smp_activity_driver_assigned(
                crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
                policy.drivers.bcmgenet_v5,
                core,
            ) || Self::smp_activity_driver_assigned(
                crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
                policy.drivers.cyw43455,
                core,
            ) || Self::smp_activity_driver_assigned(
                crate::hal::driver_task::RTL8139_DRIVER_TASK_CONTRACT,
                policy.drivers.rtl8139,
                core,
            ) || Self::smp_activity_driver_assigned(
                crate::hal::driver_task::VIRTIO_NET_DRIVER_TASK_CONTRACT,
                policy.drivers.virtio_net,
                core,
            ) || Self::smp_activity_driver_assigned(
                crate::hal::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
                policy.drivers.sdio_host,
                core,
            ) || Self::smp_activity_driver_assigned(
                crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
                policy.drivers.pcie_root,
                core,
            );
            let rates = SmpActivityRates::from_snapshots(
                previous, current, window_ms, authority, serial, local_seat, net,
            );
            let line = format_message(format_args!(
                "[smp] activity core c={} tasks={} win={} cmd_s={} line_s={} tick_s={} serial_drop_s={} seat_drop_s={} seat_no_reply_s={} hdmi_drop_s={} net_drop_s={} seatPoll_s={} kbdB_s={} hdmiB_s={} netRx_s={} netTx_s={} tcpB_s={} drop_s={}",
                core,
                tasks.as_str(),
                window_ms,
                rates.command_per_s,
                rates.line_per_s,
                rates.tick_per_s,
                rates.serial_drop_per_s,
                rates.seat_drop_per_s,
                rates.seat_no_reply_per_s,
                rates.hdmi_drop_per_s,
                rates.net_drop_per_s,
                rates.seat_poll_per_s,
                rates.keyboard_bytes_per_s,
                rates.display_bytes_per_s,
                rates.net_rx_per_s,
                rates.net_tx_per_s,
                rates.tcp_bytes_per_s,
                rates.drop_per_s,
            ));
            self.emit_console_line(line.as_str());
        }
    }

    #[cfg(feature = "kernel")]
    fn format_smp_activity_core_assignments(
        policy: &crate::generated::AffinityPolicy,
        core: u8,
    ) -> HeaplessString<128> {
        let mut buf = HeaplessString::new();
        Self::push_smp_activity_assignment(
            &mut buf,
            policy.authority_core == Some(core),
            "authority",
        );
        Self::push_smp_activity_assignment(
            &mut buf,
            Self::core_slice_has(policy.ninedoor_cores, core),
            "ninedoor",
        );
        Self::push_smp_activity_assignment(
            &mut buf,
            Self::core_slice_has(policy.provider_cores, core),
            "provider",
        );
        Self::push_smp_activity_assignment(
            &mut buf,
            Self::core_slice_has(policy.worker_cores, core),
            "worker",
        );
        Self::push_smp_activity_driver_assignment(
            &mut buf,
            crate::hal::driver_task::SERIAL_DRIVER_TASK_CONTRACT,
            policy.drivers.serial,
            core,
            "serial",
        );
        Self::push_smp_activity_driver_assignment(
            &mut buf,
            crate::hal::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            policy.drivers.usb_local_seat,
            core,
            "usb",
        );
        Self::push_smp_activity_driver_assignment(
            &mut buf,
            crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
            policy.drivers.hdmi_text,
            core,
            "hdmi",
        );
        Self::push_smp_activity_driver_assignment(
            &mut buf,
            crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
            policy.drivers.bcmgenet_v5,
            core,
            "genet",
        );
        Self::push_smp_activity_driver_assignment(
            &mut buf,
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            policy.drivers.cyw43455,
            core,
            "cyw43",
        );
        Self::push_smp_activity_driver_assignment(
            &mut buf,
            crate::hal::driver_task::RTL8139_DRIVER_TASK_CONTRACT,
            policy.drivers.rtl8139,
            core,
            "rtl8139",
        );
        Self::push_smp_activity_driver_assignment(
            &mut buf,
            crate::hal::driver_task::VIRTIO_NET_DRIVER_TASK_CONTRACT,
            policy.drivers.virtio_net,
            core,
            "virtio",
        );
        Self::push_smp_activity_driver_assignment(
            &mut buf,
            crate::hal::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
            policy.drivers.sdio_host,
            core,
            "sdio",
        );
        Self::push_smp_activity_driver_assignment(
            &mut buf,
            crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
            policy.drivers.pcie_root,
            core,
            "pcie",
        );
        if buf.is_empty() {
            let _ = buf.push_str("none");
        }
        buf
    }

    #[cfg(feature = "kernel")]
    fn smp_activity_driver_assigned(
        contract: crate::hal::driver_task::DriverTaskContract,
        assigned: Option<u8>,
        core: u8,
    ) -> bool {
        assigned == Some(core)
            && crate::hal::driver_task::driver_task_contract_active_for_current_profile(contract)
    }

    #[cfg(feature = "kernel")]
    fn push_smp_activity_driver_assignment<const N: usize>(
        buf: &mut HeaplessString<N>,
        contract: crate::hal::driver_task::DriverTaskContract,
        assigned: Option<u8>,
        core: u8,
        label: &str,
    ) {
        Self::push_smp_activity_assignment(
            buf,
            Self::smp_activity_driver_assigned(contract, assigned, core),
            label,
        );
    }

    #[cfg(feature = "kernel")]
    fn push_smp_activity_assignment<const N: usize>(
        buf: &mut HeaplessString<N>,
        assigned: bool,
        label: &str,
    ) {
        if !assigned {
            return;
        }
        if !buf.is_empty() {
            let _ = buf.push_str(",");
        }
        let _ = buf.push_str(label);
    }

    #[cfg(feature = "kernel")]
    fn core_slice_has(cores: &[u8], core: u8) -> bool {
        cores.iter().copied().any(|candidate| candidate == core)
    }

    #[cfg(not(feature = "kernel"))]
    fn emit_smp_activity_core_rates(
        &mut self,
        previous: SmpActivitySnapshot,
        current: SmpActivitySnapshot,
        window_ms: u64,
    ) {
        self.emit_smp_activity_unassigned_rates(previous, current, window_ms, "host-test");
    }

    fn emit_smp_activity_unassigned_rates(
        &mut self,
        previous: SmpActivitySnapshot,
        current: SmpActivitySnapshot,
        window_ms: u64,
        tasks: &str,
    ) {
        let rates = SmpActivityRates::from_snapshots(
            previous,
            current,
            window_ms,
            true,
            true,
            current.local_seat.is_some(),
            true,
        );
        let line = format_message(format_args!(
            "[smp] activity core c=n/a tasks={} win={} cmd_s={} line_s={} tick_s={} serial_drop_s={} seat_drop_s={} seat_no_reply_s={} hdmi_drop_s={} net_drop_s={} seatPoll_s={} kbdB_s={} hdmiB_s={} netRx_s={} netTx_s={} tcpB_s={} drop_s={}",
            tasks,
            window_ms,
            rates.command_per_s,
            rates.line_per_s,
            rates.tick_per_s,
            rates.serial_drop_per_s,
            rates.seat_drop_per_s,
            rates.seat_no_reply_per_s,
            rates.hdmi_drop_per_s,
            rates.net_drop_per_s,
            rates.seat_poll_per_s,
            rates.keyboard_bytes_per_s,
            rates.display_bytes_per_s,
            rates.net_rx_per_s,
            rates.net_tx_per_s,
            rates.tcp_bytes_per_s,
            rates.drop_per_s,
        ));
        self.emit_console_line(line.as_str());
    }

    #[cfg(feature = "net-console")]
    fn emit_smp_activity_net(&mut self) {
        let Some(net) = self.net.as_ref() else {
            self.emit_console_line("[smp] activity net attached=no feature=net-console");
            return;
        };
        let telemetry = net.telemetry();
        let counters = net.stats();
        let status = net.status_report();
        let contract = net.driver_task_contract();
        let state = format_message(format_args!(
            "[smp] activity net attached=yes backend={} mode={} active={} standby={} src={} dhcp={} contract={}",
            status.backend,
            status.mode,
            status.active_interface,
            status.standby_interface,
            status.address_source,
            status.dhcp_phase,
            contract.name,
        ));
        self.emit_console_line(state.as_str());
        let link = format_message(format_args!(
            "[smp] activity net-link link={} last_poll_ms={} tx_drops={} ip={} gw={}",
            Self::yes_no(telemetry.link_up),
            telemetry.last_poll_ms,
            telemetry.tx_drops,
            status.ip.as_str(),
            status.gateway.as_str(),
        ));
        self.emit_console_line(link.as_str());
        let io = format_message(format_args!(
            "[smp] activity net-io rx={} tx={} rx_used={} tx_used={} smoltcp={} udp_rx={} udp_tx={} tx_free={} tx_inflight={}",
            counters.rx_packets,
            counters.tx_packets,
            counters.rx_used_advances,
            counters.tx_used_advances,
            counters.smoltcp_polls,
            counters.udp_rx,
            counters.udp_tx,
            counters.tx_free,
            counters.tx_in_flight,
        ));
        self.emit_console_line(io.as_str());
        let tcp = format_message(format_args!(
            "[smp] activity net-tcp accepts={} auth={} rx_bytes={} tx_bytes={} recv_ready={} recv_budget_hits={} flush_polls={} flush_exhaust={} smoke_ok={} smoke_fail={}",
            counters.tcp_accepts,
            counters.tcp_auth_sessions,
            counters.tcp_rx_bytes,
            counters.tcp_tx_bytes,
            counters.tcp_console_recv_ready,
            counters.tcp_console_recv_budget_hits,
            self.metrics.net_post_dispatch_flush_polls,
            self.metrics.net_post_dispatch_flush_exhaustions,
            counters.tcp_smoke_outbound,
            counters.tcp_smoke_outbound_failures,
        ));
        self.emit_console_line(tcp.as_str());
        if net_status_active_interface_is_wifi(&status) {
            let wifi = format_message(format_args!(
                "[smp] activity net-wifi assoc={} link={} eapol_rx={} eapol_start={} eapol_secure={}",
                counters.wifi_assoc,
                counters.wifi_link_up,
                counters.wifi_host_eapol_rx,
                counters.wifi_host_eapol_start,
                counters.wifi_host_eapol_secure,
            ));
            self.emit_console_line(wifi.as_str());
        }
    }

    #[cfg(not(feature = "net-console"))]
    fn emit_smp_activity_net(&mut self) {
        self.emit_console_line("[smp] activity net attached=no feature=disabled");
    }

    fn emit_smp_activity_driver_contracts(&mut self) {
        use crate::hal::driver_task::{
            active_builtin_isolation_summary, driver_task_runtime_proof,
            CYW43_WIFI_DRIVER_TASK_CONTRACT, GENET_DRIVER_TASK_CONTRACT,
            HDMI_TEXT_DRIVER_TASK_CONTRACT, PCIE_ROOT_DRIVER_TASK_CONTRACT,
            RTL8139_DRIVER_TASK_CONTRACT, SDIO_HOST_DRIVER_TASK_CONTRACT,
            SERIAL_DRIVER_TASK_CONTRACT, USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            VIRTIO_NET_DRIVER_TASK_CONTRACT,
        };

        let summary = active_builtin_isolation_summary();
        let proof = driver_task_runtime_proof();
        let line = format_message(format_args!(
            "[smp] activity driver-proof contracts={} requested_dedicated={} dedicated={} compat={} substrate={} configured={} live={} failed={} hot_mask=0x{:x} compat_mask=0x{:x}",
            summary.contracts,
            summary.requested_dedicated_sel4_tasks,
            summary.dedicated_sel4_tasks,
            summary.root_task_compatibility,
            Self::yes_no(proof.substrate_active),
            proof.configured_count,
            proof.live_tcb_count,
            proof.failed_count,
            proof.hot_path_role_mask,
            proof.compatibility_service_role_mask,
        ));
        self.emit_console_line(line.as_str());
        #[cfg(feature = "kernel")]
        {
            let selected = crate::hal::driver_task::pi4_pre_root_net_bootstrap_selection();
            let line = format_message(format_args!(
                "[smp] activity selected profile={} net={} active_contracts=selected-only",
                crate::hal::driver_task::CURRENT_DRIVER_TASK_RUNTIME_PROFILE.as_str(),
                selected.as_str(),
            ));
            self.emit_console_line(line.as_str());
        }
        self.emit_smp_activity_contract_group(&[
            ("serial", SERIAL_DRIVER_TASK_CONTRACT),
            ("usb", USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT),
            ("hdmi", HDMI_TEXT_DRIVER_TASK_CONTRACT),
        ]);
        self.emit_smp_activity_contract_group(&[
            ("genet", GENET_DRIVER_TASK_CONTRACT),
            ("cyw43", CYW43_WIFI_DRIVER_TASK_CONTRACT),
            ("virtio", VIRTIO_NET_DRIVER_TASK_CONTRACT),
        ]);
        self.emit_smp_activity_contract_group(&[
            ("rtl8139", RTL8139_DRIVER_TASK_CONTRACT),
            ("sdio", SDIO_HOST_DRIVER_TASK_CONTRACT),
            ("pcie", PCIE_ROOT_DRIVER_TASK_CONTRACT),
        ]);
    }

    fn emit_smp_activity_contract_group(
        &mut self,
        contracts: &[(&'static str, crate::hal::driver_task::DriverTaskContract)],
    ) {
        let mut line = HeaplessString::<256>::new();
        let _ = write!(line, "[smp] activity contracts");
        let mut emitted = false;
        for (label, contract) in contracts.iter().copied() {
            if !crate::hal::driver_task::driver_task_contract_active_for_current_profile(contract) {
                continue;
            }
            emitted = true;
            let _ = write!(
                line,
                " {}={}:{}:{}/{}/{}",
                label,
                contract.name,
                contract.class.as_str(),
                contract.budget.max_ops_per_turn,
                contract.budget.max_bytes_per_turn,
                contract.budget.max_frames_per_turn,
            );
        }
        if emitted {
            self.emit_console_line(line.as_str());
        }
    }

    #[cfg(feature = "kernel")]
    fn emit_smp_activity_affinity(&mut self) {
        let proof = crate::hal::driver_task::driver_task_runtime_proof();
        let policy = affinity::policy();
        let authority = Self::format_optional_core(policy.authority_core);
        let ninedoor = Self::format_core_slice(policy.ninedoor_cores);
        let provider = Self::format_core_slice(policy.provider_cores);
        let worker = Self::format_core_slice(policy.worker_cores);
        let line = format_message(format_args!(
            "[smp] activity affinity enabled={} max_cores={} authority={} ninedoor={} provider={} worker={} configured={} applied={} proof={}",
            Self::yes_no(policy.enabled),
            policy.max_cores,
            authority.as_str(),
            ninedoor.as_str(),
            provider.as_str(),
            worker.as_str(),
            proof.affinity_configured_count,
            proof.affinity_applied_count,
            Self::yes_no(proof.affinity_proof),
        ));
        self.emit_console_line(line.as_str());
        let mut drivers = HeaplessString::<256>::new();
        let _ = write!(drivers, "[smp] activity affinity-drivers policy=selected");
        Self::push_smp_activity_affinity_driver(
            &mut drivers,
            "serial",
            crate::hal::driver_task::SERIAL_DRIVER_TASK_CONTRACT,
            policy.drivers.serial,
        );
        Self::push_smp_activity_affinity_driver(
            &mut drivers,
            "usb",
            crate::hal::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            policy.drivers.usb_local_seat,
        );
        Self::push_smp_activity_affinity_driver(
            &mut drivers,
            "hdmi",
            crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
            policy.drivers.hdmi_text,
        );
        Self::push_smp_activity_affinity_driver(
            &mut drivers,
            "genet",
            crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
            policy.drivers.bcmgenet_v5,
        );
        Self::push_smp_activity_affinity_driver(
            &mut drivers,
            "cyw43",
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            policy.drivers.cyw43455,
        );
        Self::push_smp_activity_affinity_driver(
            &mut drivers,
            "rtl8139",
            crate::hal::driver_task::RTL8139_DRIVER_TASK_CONTRACT,
            policy.drivers.rtl8139,
        );
        Self::push_smp_activity_affinity_driver(
            &mut drivers,
            "virtio",
            crate::hal::driver_task::VIRTIO_NET_DRIVER_TASK_CONTRACT,
            policy.drivers.virtio_net,
        );
        Self::push_smp_activity_affinity_driver(
            &mut drivers,
            "sdio",
            crate::hal::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
            policy.drivers.sdio_host,
        );
        Self::push_smp_activity_affinity_driver(
            &mut drivers,
            "pcie",
            crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
            policy.drivers.pcie_root,
        );
        let _ = write!(
            drivers,
            " applied_proof={}",
            Self::yes_no(proof.affinity_proof)
        );
        self.emit_console_line(drivers.as_str());
    }

    #[cfg(feature = "kernel")]
    fn push_smp_activity_affinity_driver<const N: usize>(
        line: &mut HeaplessString<N>,
        label: &str,
        contract: crate::hal::driver_task::DriverTaskContract,
        core: Option<u8>,
    ) {
        if !crate::hal::driver_task::driver_task_contract_active_for_current_profile(contract) {
            return;
        }
        let _ = write!(
            line,
            " {}={}",
            label,
            Self::format_optional_core(core).as_str()
        );
    }

    #[cfg(not(feature = "kernel"))]
    fn emit_smp_activity_affinity(&mut self) {
        self.emit_console_line("[smp] activity affinity unavailable=host-test");
    }

    #[cfg(feature = "kernel")]
    fn emit_mem(&mut self) -> bool {
        let context = match self.console_context {
            Some(context) => context,
            None => return false,
        };
        let header = context.bootinfo.header();
        let count = (header.untyped.end - header.untyped.start) as usize;
        let mut ram_ut = 0usize;
        for desc in header.untypedList.iter().take(count) {
            if desc.isDevice == 0 {
                ram_ut += 1;
            }
        }
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let _ = write!(
            line,
            "[mem] untyped caps={} ram_ut={} device_ut={}",
            count,
            ram_ut,
            count.saturating_sub(ram_ut),
        );
        self.emit_console_line(line.as_str());
        true
    }

    #[cfg(not(feature = "kernel"))]
    fn emit_mem(&mut self) -> bool {
        let _ = self;
        false
    }

    #[cfg(all(feature = "kernel", target_os = "none"))]
    fn emit_cache_log(&mut self, count: usize) {
        struct CacheLineWriter<
            'a,
            'b,
            D,
            T,
            I,
            V,
            const RX: usize,
            const TX: usize,
            const LINE: usize,
        >
        where
            D: SerialDriver,
            T: TimerSource,
            I: IpcDispatcher,
            V: CapabilityValidator,
        {
            pump: &'a mut EventPump<'b, D, T, I, V, RX, TX, LINE>,
        }

        impl<'a, 'b, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize> fmt::Write
            for CacheLineWriter<'a, 'b, D, T, I, V, RX, TX, LINE>
        where
            D: SerialDriver,
            T: TimerSource,
            I: IpcDispatcher,
            V: CapabilityValidator,
        {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                for line in s.split('\n') {
                    if line.is_empty() {
                        continue;
                    }
                    self.pump.emit_console_line(line);
                }
                Ok(())
            }
        }

        let mut writer = CacheLineWriter { pump: self };
        crate::hal::cache::write_recent_ops(&mut writer, count);
    }

    fn emit_ack(&mut self, status: AckStatus, verb: &str, detail: Option<&str>) {
        let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
        let ack_line = AckLine {
            status,
            verb,
            detail,
        };
        match render_ack(&mut line, &ack_line) {
            Ok(()) => self.emit_console_line(line.as_str()),
            Err(LineFormatError::Truncated) => {
                self.audit.denied("console ack truncated");
                self.emit_console_line("ERR PARSE reason=policy detail=ack-truncated");
            }
        }
    }

    fn emit_ack_ok(&mut self, verb: &str, detail: Option<&str>) {
        self.emit_ack(AckStatus::Ok, verb, detail);
    }

    fn emit_ack_err(&mut self, verb: &str, detail: Option<&str>) {
        self.emit_ack(AckStatus::Err, verb, detail);
    }

    fn emit_refusal(&mut self, verb: &str, reason: RefusalReason, detail: Option<&str>) {
        let detail = match detail {
            Some(detail) => format_message(format_args!("reason={} {}", reason.as_str(), detail)),
            None => format_message(format_args!("reason={}", reason.as_str())),
        };
        self.emit_ack_err(verb, Some(detail.as_str()));
        crate::observe::record_pressure(reason.pressure_kind());
    }

    #[cfg(feature = "net-console")]
    fn net_disabled_refusal_detail(&self) -> HeaplessString<224> {
        let mut detail = HeaplessString::<224>::new();
        let _ = write!(detail, "detail=net-disabled");
        if let Some(cause) = self.net_unavailable_detail.as_ref() {
            let _ = write!(detail, " cause={cause}");
        }
        detail
    }

    #[cfg(feature = "kernel")]
    fn refusal_for_ninedoor_error(err: &NineDoorBridgeError) -> (RefusalReason, &'static str) {
        match err {
            NineDoorBridgeError::Unsupported(_) => (RefusalReason::Policy, "unsupported"),
            NineDoorBridgeError::AttachTimeout => (RefusalReason::Busy, "attach-timeout"),
            NineDoorBridgeError::InvalidPath => (RefusalReason::Policy, "invalid-path"),
            NineDoorBridgeError::Permission => (RefusalReason::Policy, "denied"),
            NineDoorBridgeError::BufferFull => (RefusalReason::Quota, "buffer-full"),
            NineDoorBridgeError::InvalidPayload => (RefusalReason::Policy, "invalid-payload"),
            NineDoorBridgeError::Busy => (RefusalReason::Busy, "busy"),
        }
    }

    #[cfg(feature = "kernel")]
    fn emit_ninedoor_refusal(&mut self, verb: &str, path: Option<&str>, err: &NineDoorBridgeError) {
        let (reason, detail_tag) = Self::refusal_for_ninedoor_error(err);
        let detail = match path {
            Some(path) => {
                format_message(format_args!("detail={detail_tag} path={path} error={err}"))
            }
            None => format_message(format_args!("detail={detail_tag} error={err}")),
        };
        self.emit_refusal(verb, reason, Some(detail.as_str()));
    }

    fn emit_auth_failure(&mut self, verb: &str) {
        self.metrics.ui_denies = self.metrics.ui_denies.saturating_add(1);
        self.emit_refusal(verb, RefusalReason::Policy, Some("detail=unauthenticated"));
    }

    #[cfg(feature = "kernel")]
    fn hardware_has_device_kind(kind: crate::generated::HardwareDeviceKind) -> bool {
        crate::generated::hardware_config()
            .devices
            .iter()
            .any(|device| device.kind == kind)
    }

    #[cfg(feature = "kernel")]
    fn usb_debug_commands_enabled(&self) -> bool {
        #[cfg(test)]
        if self.test_pi4_debug_commands {
            return true;
        }

        crate::generated::hardware_config().local_seat.enabled
    }

    #[cfg(feature = "kernel")]
    fn wifi_debug_commands_enabled(&self) -> bool {
        #[cfg(test)]
        if self.test_pi4_debug_commands {
            return true;
        }

        if !Self::hardware_has_device_kind(crate::generated::HardwareDeviceKind::Wifi) {
            return false;
        }

        #[cfg(feature = "net-console")]
        if let Some(net) = self.net.as_ref() {
            return net_status_active_interface_is_wifi(&net.status_report());
        }

        true
    }

    #[cfg(feature = "kernel")]
    fn maybe_handle_wifi_debug_line(&mut self, line: &str) -> bool {
        if self.last_input_source == ConsoleInputSource::Net {
            return false;
        }

        let mut parts = line.split_ascii_whitespace();
        let Some(head) = parts.next() else {
            return false;
        };
        if !head.eq_ignore_ascii_case("wifi") {
            return false;
        }
        if !self.wifi_debug_commands_enabled() {
            return false;
        }

        let command = match parts.next() {
            None => WifiDebugCommand::Help,
            Some(subcommand) if subcommand.eq_ignore_ascii_case("help") => WifiDebugCommand::Help,
            Some(subcommand) if subcommand.eq_ignore_ascii_case("dump-state") => {
                WifiDebugCommand::DumpState
            }
            Some(subcommand) if subcommand.eq_ignore_ascii_case("probe-ht") => {
                WifiDebugCommand::ProbeHt
            }
            Some(subcommand) if subcommand.eq_ignore_ascii_case("diag") => WifiDebugCommand::Diag,
            Some(subcommand) if subcommand.eq_ignore_ascii_case("load-fw") => {
                WifiDebugCommand::LoadFirmware
            }
            Some(subcommand) if subcommand.eq_ignore_ascii_case("retry") => WifiDebugCommand::Retry,
            Some(_) => {
                self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
                self.emit_refusal(
                    WIFI_DEBUG_ACK_LABEL,
                    RefusalReason::Policy,
                    Some("detail=unknown-subcommand"),
                );
                return true;
            }
        };

        if parts.next().is_some() {
            self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
            self.emit_refusal(
                WIFI_DEBUG_ACK_LABEL,
                RefusalReason::Policy,
                Some("detail=too-many-arguments"),
            );
            return true;
        }

        self.handle_wifi_debug_command(command);
        true
    }

    #[cfg(feature = "kernel")]
    fn maybe_handle_usb_debug_line(&mut self, line: &str) -> bool {
        if self.last_input_source == ConsoleInputSource::Net {
            return false;
        }

        let mut parts = line.split_ascii_whitespace();
        let Some(head) = parts.next() else {
            return false;
        };
        if !head.eq_ignore_ascii_case("usb") {
            return false;
        }
        if !self.usb_debug_commands_enabled() {
            return false;
        }

        let command = match parts.next() {
            None => UsbDebugCommand::Help,
            Some(subcommand) if subcommand.eq_ignore_ascii_case("help") => UsbDebugCommand::Help,
            Some(subcommand) if subcommand.eq_ignore_ascii_case("status") => {
                UsbDebugCommand::Status
            }
            Some(subcommand) if subcommand.eq_ignore_ascii_case("dump-state") => {
                UsbDebugCommand::DumpState
            }
            Some(subcommand) if subcommand.eq_ignore_ascii_case("diag") => UsbDebugCommand::Diag,
            Some(subcommand) if subcommand.eq_ignore_ascii_case("enable-kbd") => {
                UsbDebugCommand::EnableKeyboard
            }
            Some(subcommand) if subcommand.eq_ignore_ascii_case("probe-kbd") => {
                UsbDebugCommand::ProbeKeyboard
            }
            Some(_) => {
                self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
                self.emit_refusal(
                    USB_DEBUG_ACK_LABEL,
                    RefusalReason::Policy,
                    Some("detail=unknown-subcommand"),
                );
                return true;
            }
        };

        if parts.next().is_some() {
            self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
            self.emit_refusal(
                USB_DEBUG_ACK_LABEL,
                RefusalReason::Policy,
                Some("detail=too-many-arguments"),
            );
            return true;
        }

        self.handle_usb_debug_command(command);
        true
    }

    #[cfg(feature = "kernel")]
    fn handle_wifi_debug_command(&mut self, command: WifiDebugCommand) {
        let subcommand = match command {
            WifiDebugCommand::Help => "help",
            WifiDebugCommand::DumpState => "dump-state",
            WifiDebugCommand::ProbeHt => "probe-ht",
            WifiDebugCommand::Diag => "diag",
            WifiDebugCommand::LoadFirmware => "load-fw",
            WifiDebugCommand::Retry => "retry",
        };
        let profile = Self::wifi_debug_command_profile(command);

        if matches!(command, WifiDebugCommand::Help) {
            self.emit_console_line("WiFi debug commands:");
            self.emit_console_line("  wifi help       - Show WiFi debug command help");
            self.emit_console_line(
                "  wifi dump-state - Show cached SDIO, clock, and contract trace state",
            );
            self.emit_console_line(
                "  wifi probe-ht   - Run linked-runtime-backed HT diagnostics or report runtime-required",
            );
            self.emit_console_line(
                "  wifi diag       - Show compact linked-runtime gate state; passive unless HT diagnostics are safe",
            );
            self.emit_console_line(
                "  wifi load-fw    - Retry linked-runtime firmware load when the boundary supports it",
            );
            self.emit_console_line(
                "  wifi retry      - Run linked-runtime transport and firmware retry when supported",
            );
            self.metrics.accepted_commands = self.metrics.accepted_commands.saturating_add(1);
            self.emit_ack_ok(
                WIFI_DEBUG_ACK_LABEL,
                Some("detail=subcommand=help scope=serial-local"),
            );
            return;
        }

        let _wifi_breadcrumb_uart_guard = crate::hal::pi4_wifi::suppress_wifi_breadcrumb_uart();
        let _wifi_log_uart_guard = crate::bootstrap::log::suppress_uart_log_output();

        self.emit_wifi_debug_status(subcommand, "begin", profile, None);
        if self.wifi_debug.is_none()
            && self.emit_wifi_driver_task_runtime_snapshot_if_present(
                command,
                subcommand,
                profile,
                "debug-handle-unavailable",
            )
        {
            return;
        }
        let result = match command {
            WifiDebugCommand::Help => Ok(None),
            WifiDebugCommand::DumpState => match self.wifi_debug.as_mut() {
                Some(wifi_debug) => wifi_debug.dump_state("console-dump-state").map(Some),
                None => Err(crate::hal::HalError::Unsupported("wifi-debug-unavailable")),
            },
            WifiDebugCommand::ProbeHt => {
                let ready = match self.wifi_debug.as_mut() {
                    Some(wifi_debug) => wifi_debug.probe_ht_clock(),
                    None => Err(crate::hal::HalError::Unsupported("wifi-debug-unavailable")),
                };
                match ready {
                    Ok(ready) => {
                        let detail = format_message(format_args!(
                            "wifi ht: ready={}",
                            if ready { "yes" } else { "no" }
                        ));
                        self.emit_console_line(detail.as_str());
                        match self.wifi_debug.as_mut() {
                            Some(wifi_debug) => wifi_debug.dump_state("console-probe-ht").map(Some),
                            None => {
                                Err(crate::hal::HalError::Unsupported("wifi-debug-unavailable"))
                            }
                        }
                    }
                    Err(err) => Err(err),
                }
            }
            WifiDebugCommand::Diag => {
                (|| -> Result<Option<WifiDebugSnapshot>, crate::hal::HalError> {
                    let before = match self.wifi_debug.as_mut() {
                        Some(wifi_debug) => wifi_debug.dump_state("console-diag-before"),
                        None => Err(crate::hal::HalError::Unsupported("wifi-debug-unavailable")),
                    }?;
                    self.emit_console_line("wifi: diag stage=before-ht-probe");
                    self.emit_wifi_diag_summary(&before);

                    if let Some(reason) = Self::wifi_diag_ht_probe_skip_reason(&before) {
                        let detail = format_message(format_args!(
                            "wifi: diag ht_probe skipped reason={reason} exact_error={}",
                            before.control_plane_exact_error,
                        ));
                        self.emit_console_line(detail.as_str());
                        self.emit_console_line(
                            "wifi: diag stage=after-ht-probe skipped=yes snapshot=unchanged",
                        );
                        return Ok(None);
                    }

                    let ready = match self.wifi_debug.as_mut() {
                        Some(wifi_debug) => wifi_debug.probe_ht_clock(),
                        None => Err(crate::hal::HalError::Unsupported("wifi-debug-unavailable")),
                    }?;
                    let detail = format_message(format_args!(
                        "wifi: diag ht_probe ready={}",
                        if ready { "yes" } else { "no" }
                    ));
                    self.emit_console_line(detail.as_str());

                    let after = match self.wifi_debug.as_mut() {
                        Some(wifi_debug) => wifi_debug.dump_state("console-diag-after-ht"),
                        None => Err(crate::hal::HalError::Unsupported("wifi-debug-unavailable")),
                    }?;
                    self.emit_console_line("wifi: diag stage=after-ht-probe");
                    self.emit_wifi_diag_summary(&after);
                    Ok(None)
                })()
            }
            WifiDebugCommand::LoadFirmware => match self.wifi_debug.as_mut() {
                Some(wifi_debug) => wifi_debug.load_firmware().map(Some),
                None => Err(crate::hal::HalError::Unsupported("wifi-debug-unavailable")),
            },
            WifiDebugCommand::Retry => match self.wifi_debug.as_mut() {
                Some(wifi_debug) => wifi_debug.retry_transport_and_firmware().map(Some),
                None => Err(crate::hal::HalError::Unsupported("wifi-debug-unavailable")),
            },
        };

        match result {
            Ok(snapshot) => {
                if let Some(snapshot) = snapshot {
                    self.emit_wifi_snapshot_with_traces(&snapshot);
                }
                self.emit_wifi_debug_status(subcommand, "complete", profile, Some("result=ok"));
                self.metrics.accepted_commands = self.metrics.accepted_commands.saturating_add(1);
                let detail = format_message(format_args!(
                    "detail=subcommand={subcommand} scope=serial-local"
                ));
                self.emit_ack_ok(WIFI_DEBUG_ACK_LABEL, Some(detail.as_str()));
            }
            Err(err) => {
                if Self::wifi_error_is_driver_task_runtime_required(&err)
                    && self.emit_wifi_driver_task_runtime_snapshot_if_present(
                        command,
                        subcommand,
                        profile,
                        "hal-runtime-required",
                    )
                {
                    return;
                }
                let error_snapshot_stage = match command {
                    WifiDebugCommand::Help => None,
                    WifiDebugCommand::DumpState => None,
                    WifiDebugCommand::ProbeHt => Some("console-probe-ht-error"),
                    WifiDebugCommand::Diag => Some("console-diag-error"),
                    WifiDebugCommand::LoadFirmware => Some("console-load-fw-error"),
                    WifiDebugCommand::Retry => Some("console-retry-error"),
                };
                if let Some(stage) = error_snapshot_stage {
                    if let Some(wifi_debug) = self.wifi_debug.as_mut() {
                        if let Ok(snapshot) = wifi_debug.dump_state(stage) {
                            self.emit_wifi_snapshot(&snapshot);
                        }
                    }
                }
                let detail = format_message(format_args!("result=error error={err}"));
                self.emit_wifi_debug_status(subcommand, "complete", profile, Some(detail.as_str()));
                self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
                let detail =
                    format_message(format_args!("detail=subcommand={subcommand} error={err}"));
                self.emit_refusal(
                    WIFI_DEBUG_ACK_LABEL,
                    RefusalReason::Policy,
                    Some(detail.as_str()),
                );
            }
        }
    }

    #[cfg(feature = "kernel")]
    fn handle_usb_debug_command(&mut self, command: UsbDebugCommand) {
        let subcommand = match command {
            UsbDebugCommand::Help => "help",
            UsbDebugCommand::Status => "status",
            UsbDebugCommand::DumpState => "dump-state",
            UsbDebugCommand::Diag => "diag",
            UsbDebugCommand::EnableKeyboard => "enable-kbd",
            UsbDebugCommand::ProbeKeyboard => "probe-kbd",
        };

        if matches!(command, UsbDebugCommand::Help) {
            self.emit_console_line("USB local-seat debug commands:");
            self.emit_console_line("  usb help        - Show USB local-seat debug command help");
            self.emit_console_line(
                "  usb status      - Show local-seat runtime attach, polling, and contract trace",
            );
            self.emit_console_line("  usb dump-state  - Alias for usb status");
            self.emit_console_line(
                "  usb diag        - Show passive linked-runtime gates without live xHCI probing",
            );
            self.emit_console_line(
                "  usb enable-kbd  - Arm runtime USB keyboard probing after boot",
            );
            self.emit_console_line(
                "  usb probe-kbd   - Run one bounded keyboard probe slice with contract trace",
            );
            self.metrics.accepted_commands = self.metrics.accepted_commands.saturating_add(1);
            self.emit_ack_ok(
                USB_DEBUG_ACK_LABEL,
                Some("detail=subcommand=help scope=serial-local"),
            );
            return;
        }

        if self.local_seat.is_none() {
            self.emit_usb_debug_unavailable(subcommand, "local-seat-unavailable");
            return;
        }
        match command {
            UsbDebugCommand::Help => {}
            UsbDebugCommand::Status | UsbDebugCommand::DumpState => {
                self.with_local_seat_mirror_suppressed(|this| {
                    let (backend_attached, polling_enabled) = {
                        let local_seat = match this.local_seat.as_mut() {
                            Some(local_seat) => local_seat,
                            None => {
                                this.emit_usb_debug_unavailable(
                                    subcommand,
                                    "local-seat-unavailable",
                                );
                                return;
                            }
                        };
                        (
                            local_seat.backend_attached(),
                            local_seat.backend_keyboard_polling_enabled(),
                        )
                    };
                    this.emit_usb_status(backend_attached, polling_enabled, None);
                });
                self.mirror_usb_debug_hdmi_frontier(subcommand);
            }
            UsbDebugCommand::Diag => {
                self.with_local_seat_mirror_suppressed(|this| {
                    let (backend_attached, polling_enabled) = {
                        let local_seat = match this.local_seat.as_mut() {
                            Some(local_seat) => local_seat,
                            None => {
                                this.emit_usb_debug_unavailable(
                                    subcommand,
                                    "local-seat-unavailable",
                                );
                                return;
                            }
                        };
                        (
                            local_seat.backend_attached(),
                            local_seat.backend_keyboard_polling_enabled(),
                        )
                    };
                    this.emit_usb_status(
                        backend_attached,
                        polling_enabled,
                        Some("action=diag-passive"),
                    );
                    this.emit_console_line(
                        "usb: diag action=probe-skipped reason=linked-runtime-only use=usb-status",
                    );
                    this.emit_usb_startup_blackbox(backend_attached, polling_enabled);
                });
                self.mirror_usb_debug_hdmi_frontier("diag");
            }
            UsbDebugCommand::EnableKeyboard => {
                let (backend_attached, polling_enabled) = {
                    let local_seat = match self.local_seat.as_mut() {
                        Some(local_seat) => local_seat,
                        None => {
                            self.emit_usb_debug_unavailable(subcommand, "local-seat-unavailable");
                            return;
                        }
                    };
                    local_seat.enable_backend_keyboard_polling();
                    (
                        local_seat.backend_attached(),
                        local_seat.backend_keyboard_polling_enabled(),
                    )
                };
                self.emit_usb_status(
                    backend_attached,
                    polling_enabled,
                    Some("action=keyboard-poll-armed"),
                );
            }
            UsbDebugCommand::ProbeKeyboard => {
                self.emit_console_line("usb: probing local-seat keyboard now");
                self.preflush_serial_before_raw_uart();
                let (backend_attached, polling_enabled, probe_result) = {
                    let local_seat = match self.local_seat.as_mut() {
                        Some(local_seat) => local_seat,
                        None => {
                            self.emit_usb_debug_unavailable(subcommand, "local-seat-unavailable");
                            return;
                        }
                    };
                    let probe_result = local_seat.probe_backend_keyboard_once();
                    (
                        local_seat.backend_attached(),
                        local_seat.backend_keyboard_polling_enabled(),
                        probe_result,
                    )
                };
                let detail = format_message(format_args!(
                    "action=keyboard-probe-complete probe_result={}",
                    probe_result.as_str()
                ));
                self.emit_usb_status(backend_attached, polling_enabled, Some(detail.as_str()));
            }
        }

        self.metrics.accepted_commands = self.metrics.accepted_commands.saturating_add(1);
        let detail = format_message(format_args!(
            "detail=subcommand={subcommand} scope=serial-local"
        ));
        self.emit_ack_ok(USB_DEBUG_ACK_LABEL, Some(detail.as_str()));
    }

    #[cfg(feature = "kernel")]
    fn mirror_usb_debug_hdmi_frontier(&mut self, subcommand: &str) {
        let line = format_message(format_args!(
            "[drivers] USB {} complete: full diagnostics on serial; HDMI preserved",
            subcommand
        ));
        let Some(local_seat) = self.local_seat.as_mut() else {
            self.pending_usb_debug_hdmi_frontier = Some(line);
            return;
        };
        if !local_seat.mirror_high_impact_line(line.as_str()) {
            self.pending_usb_debug_hdmi_frontier = Some(line);
        }
    }

    fn retry_pending_usb_debug_hdmi_frontier(&mut self) {
        #[cfg(all(feature = "kernel", feature = "usb"))]
        {
            if self.pending_usb_debug_hdmi_frontier.is_none()
                || self.serial.tx_pending()
                || self.serial.interactive_input_active()
                || self.console_output_flush_active
                || !self.pending_console_output.is_empty()
                || self.physical_console_input_pending_for_output()
                || crate::hal::driver_task::driver_task_ring_command_active(
                    crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
                )
            {
                return;
            }
            let Some(line) = self.pending_usb_debug_hdmi_frontier.take() else {
                return;
            };
            let Some(local_seat) = self.local_seat.as_mut() else {
                self.pending_usb_debug_hdmi_frontier = Some(line);
                return;
            };
            if !local_seat.mirror_high_impact_line(line.as_str()) {
                self.pending_usb_debug_hdmi_frontier = Some(line);
            }
        }
    }

    #[cfg(feature = "kernel")]
    fn emit_usb_debug_unavailable(&mut self, subcommand: &str, error: &str) {
        self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
        let detail = format_message(format_args!("detail=subcommand={subcommand} error={error}"));
        self.emit_refusal(
            USB_DEBUG_ACK_LABEL,
            RefusalReason::Policy,
            Some(detail.as_str()),
        );
    }

    #[cfg(feature = "kernel")]
    fn emit_usb_runtime_recovery(&mut self, frame: crate::hal::driver_task::DriverFrameDescriptor) {
        let (
            diag_valid,
            stage,
            command_completion_blocked,
            recoveries,
            failures,
            queue_collapse,
            reason,
        ) = Self::usb_runtime_keyboard_recovery_diag_fields(frame);
        let line = format_message(format_args!(
            "usb: runtime_recovery diag_valid={} recoveries={} failures={} queue_collapse={} stage={} stage_code={} reason={} reason_code={} command_completion_blocked={}",
            Self::yes_no(diag_valid),
            recoveries,
            failures,
            queue_collapse,
            Self::usb_runtime_keyboard_recovery_stage_label(stage),
            stage,
            Self::usb_runtime_keyboard_recovery_reason_label(reason),
            reason,
            command_completion_blocked,
        ));
        self.emit_console_line(line.as_str());
    }

    #[cfg(feature = "kernel")]
    fn emit_usb_status(
        &mut self,
        backend_attached: bool,
        polling_enabled: bool,
        action_detail: Option<&str>,
    ) {
        #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
        {
            if polling_enabled {
                if let Some(local_seat) = self.local_seat.as_mut() {
                    local_seat.poll_backend_keyboard();
                }
            }
        }
        #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
        let linked_detail = crate::local_seat::linked_local_seat_usb_runtime_detail();
        #[cfg(not(all(feature = "usb", target_arch = "aarch64", target_os = "none")))]
        let linked_detail = 0u16;
        #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
        let linked_result = crate::local_seat::linked_local_seat_usb_runtime_result();
        #[cfg(not(all(feature = "usb", target_arch = "aarch64", target_os = "none")))]
        let linked_result = 0u32;
        #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
        let linked_frame = crate::local_seat::linked_local_seat_usb_runtime_frame();
        #[cfg(not(all(feature = "usb", target_arch = "aarch64", target_os = "none")))]
        let linked_frame = crate::hal::driver_task::DriverFrameDescriptor {
            offset: 0,
            len: 0,
            flags: 0,
        };
        let (queued_reports, doorbell_pending, preserved_events, transfer_events, report_status) =
            Self::usb_runtime_queue_fields(linked_result);
        let queue_valid = Self::usb_runtime_detail_has_queue_result(linked_detail);
        let mut line = format_message(format_args!(
            "usb: local-seat attached={} polling={}",
            if backend_attached { "yes" } else { "no" },
            if polling_enabled {
                "enabled"
            } else {
                "deferred"
            },
        ));
        #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
        {
            let _ = write!(
                line,
                " controller={} keyboard={}",
                if crate::local_seat::linked_local_seat_usb_controller_ready() {
                    "ready"
                } else {
                    "not-ready"
                },
                if crate::local_seat::linked_local_seat_usb_keyboard_ready() {
                    "yes"
                } else {
                    "no"
                }
            );
        }
        let queue_line = format_message(format_args!(
            "usb: runtime_queue queue_valid={} detail=0x{:04x} result=0x{:08x} queued_reports={} doorbell_pending={} preserved_events={} transfer_events={} report_status={}",
            Self::yes_no(queue_valid),
            linked_detail,
            linked_result,
            queued_reports,
            Self::yes_no(doorbell_pending),
            preserved_events,
            transfer_events,
            Self::usb_runtime_keyboard_report_status_label(report_status),
        ));
        self.emit_console_line(queue_line.as_str());
        self.emit_usb_runtime_recovery(linked_frame);
        if let Some(action_detail) = action_detail {
            let _ = write!(line, " {action_detail}");
        }
        self.emit_console_line(line.as_str());
        if let Some(local_seat) = self.local_seat.as_ref() {
            let keyboard_drop = local_seat.dropped_keyboard_bytes();
            let trace = local_seat.keyboard_trace();
            let drop_line = format_message(format_args!(
                "usb: local-seat drops keyboard_drop={} driver_task_budget_overruns={} driver_task_no_replies={} poll_cooldown={} cooldown_skips={}",
                keyboard_drop,
                trace.driver_task_budget_overruns,
                trace.driver_task_no_replies,
                trace.poll_cooldown_turns,
                trace.poll_cooldown_skips,
            ));
            self.emit_console_line(drop_line.as_str());
            let trace_line = format_message(format_args!(
                "usb: local-seat input queued={} backend_polls={} backend_bytes={} accepted={} drained={} echoed={} dropped={}",
                trace.queued_bytes,
                trace.backend_poll_calls,
                trace.backend_read_bytes,
                trace.accepted_bytes,
                trace.drained_bytes,
                trace.echoed_bytes,
                trace.dropped_bytes,
            ));
            self.emit_console_line(trace_line.as_str());
        }
        let local_trace = self
            .local_seat
            .as_ref()
            .map(|local_seat| local_seat.keyboard_trace())
            .unwrap_or_default();
        let display_trace = self
            .local_seat
            .as_ref()
            .map(|local_seat| local_seat.display_trace())
            .unwrap_or_default();
        let stall_line = format_message(format_args!(
            "usb: stall_telemetry queue_valid={} queued_reports={} doorbell={} preserved={} transfer_events={} report_status={} local_queued={} local_drop={} backend_polls={} backend_bytes={} no_reply={} cooldown={} cooldown_skips={} serial_tx_pending={} serial_interactive={}",
            Self::yes_no(queue_valid),
            queued_reports,
            Self::yes_no(doorbell_pending),
            preserved_events,
            transfer_events,
            Self::usb_runtime_keyboard_report_status_label(report_status),
            local_trace.queued_bytes,
            local_trace.dropped_bytes,
            local_trace.backend_poll_calls,
            local_trace.backend_read_bytes,
            local_trace.driver_task_no_replies,
            local_trace.poll_cooldown_turns,
            local_trace.poll_cooldown_skips,
            Self::yes_no(self.serial.tx_pending()),
            Self::yes_no(self.serial.interactive_input_active()),
        ));
        self.emit_console_line(stall_line.as_str());
        let sustained_blocker = Self::usb_runtime_sustained_input_blocker(
            queue_valid,
            queued_reports,
            transfer_events,
            report_status,
            local_trace.driver_task_no_replies,
            self.metrics.local_seat_runtime_skipped_turns,
        );
        let usb_burst = local_seat_usb_burst_proof(
            local_trace.accepted_bytes,
            local_trace.drained_bytes,
            local_trace.echoed_bytes,
            local_trace.dropped_bytes,
        );
        let sustained_line = format_message(format_args!(
            "usb: sustained_input queue_valid={} detail=0x{:04x} result=0x{:08x} queued_reports={} transfer_events={} report_status={} accepted={} drained={} echoed={} no_reply={} no_reply_streak={} recovery_aux_requests={} recovery_aux_pending={} runtime_skipped={} blocker={} usb_burst={} drops={}",
            Self::yes_no(queue_valid),
            linked_detail,
            linked_result,
            queued_reports,
            transfer_events,
            Self::usb_runtime_keyboard_report_status_label(report_status),
            local_trace.accepted_bytes,
            local_trace.drained_bytes,
            local_trace.echoed_bytes,
            local_trace.driver_task_no_replies,
            local_trace.driver_task_no_reply_streak,
            local_trace.recovery_aux_requests,
            Self::yes_no(local_trace.recovery_aux_pending),
            self.metrics.local_seat_runtime_skipped_turns,
            sustained_blocker,
            Self::yes_no(usb_burst),
            local_trace.dropped_bytes,
        ));
        self.emit_console_line(sustained_line.as_str());
        let output_pressure_line = format_message(format_args!(
            "usb: output_pressure serial_tx_pending={} serial_interactive={} deferred={} flushed={} backpressure={} hdmi_pending_bytes={} hdmi_redraw_bytes={} hdmi_pending_redraw={} hdmi_scrollback={} hdmi_open_line={} hdmi_submitted={} hdmi_deferred={} hdmi_busy={} hdmi_no_reply={} hdmi_redraw_no_reply={} hdmi_coalesced={} hdmi_backpressure_bytes={} hdmi_superseded_bytes={}",
            Self::yes_no(self.serial.tx_pending()),
            Self::yes_no(self.serial.interactive_input_active()),
            self.metrics.physical_console_output_deferred,
            self.metrics.physical_console_output_flushed,
            self.metrics.physical_console_output_backpressure,
            display_trace.pending_bytes,
            display_trace.redraw_bytes,
            Self::yes_no(display_trace.pending_redraw),
            display_trace.scrollback_offset,
            Self::yes_no(display_trace.open_line),
            display_trace.submitted_frames,
            display_trace.deferred_frames,
            display_trace.busy_frames,
            display_trace.no_reply_frames,
            display_trace.redraw_no_reply_streak,
            display_trace.coalesced_redraws,
            display_trace.backpressure_bytes,
            display_trace.superseded_bytes,
        ));
        self.emit_console_line(output_pressure_line.as_str());
        let pump_line = format_message(format_args!(
            "usb: event_loop keyboard_priority={} runtime_skipped={} serial_dispatch_yielded={} post_runtime_keyboard={} output_keyboard_polls={} hdmi_pump={}",
            self.metrics.local_seat_keyboard_priority_turns,
            self.metrics.local_seat_runtime_skipped_turns,
            self.metrics.local_seat_serial_dispatch_yielded_turns,
            self.metrics.local_seat_post_runtime_hits,
            self.metrics.local_seat_output_keyboard_polls,
            self.metrics.local_seat_hdmi_pump_turns,
        ));
        self.emit_console_line(pump_line.as_str());
        self.emit_usb_related_driver_counter(
            "usb-runtime",
            crate::hal::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
        );
        self.emit_usb_related_driver_counter(
            "hdmi-display",
            crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
        );
        let diag_exact_issue = None;
        let (verdict, focus) =
            Self::usb_capture_verdict(backend_attached, polling_enabled, diag_exact_issue);
        let verdict_line = format_message(format_args!("usb: verdict={verdict} focus={focus}"));
        self.emit_console_line(verdict_line.as_str());
        #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
        {
            let linked_controller_ready =
                crate::local_seat::linked_local_seat_usb_controller_ready();
            let linked_keyboard_ready = crate::local_seat::linked_local_seat_usb_keyboard_ready();
            let linked_first_report = crate::local_seat::linked_local_seat_usb_first_report_ready();
            let linked_first_byte = crate::local_seat::linked_local_seat_usb_first_byte_ready();
            let linked_progress = crate::hal::driver_task::latest_driver_task_ring_progress(
                crate::hal::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            );
            let linked_detail_gate = Self::usb_runtime_gate_for_linked_detail(linked_detail);
            let linked_progress_gate = linked_progress.map_or(0, |progress| {
                Self::usb_runtime_gate_for_progress_phase(progress.phase)
            });
            let progress_refines_linked_detail = Self::usb_runtime_progress_refines_linked_detail(
                linked_detail,
                linked_detail_gate,
                linked_progress_gate,
            );
            let linked_progress_proof_gate = if linked_detail == 0 && linked_progress_gate != 0 {
                linked_progress_gate.saturating_sub(1)
            } else {
                linked_progress_gate
            };
            let linked_gate = linked_detail_gate.max(linked_progress_proof_gate);
            let local_trace = self
                .local_seat
                .as_ref()
                .map(|local_seat| local_seat.keyboard_trace())
                .unwrap_or_default();
            let parser_ingress = local_trace.backend_read_bytes != 0
                || local_trace.accepted_bytes != 0
                || local_trace.echoed_bytes != 0;
            let (
                queued_reports,
                doorbell_pending,
                preserved_events,
                transfer_events,
                report_status,
            ) = Self::usb_runtime_queue_fields(linked_result);
            let queue_valid = Self::usb_runtime_detail_has_queue_result(linked_detail);
            let input_observation = Self::usb_runtime_keyboard_input_observation(
                linked_first_byte,
                parser_ingress,
                queue_valid,
                queued_reports,
                doorbell_pending,
                report_status,
            );
            let keyboard_ready = linked_keyboard_ready;
            let first_report = linked_first_report;
            let prompt_polling_enabled = self
                .local_seat
                .as_ref()
                .is_some_and(|local_seat| local_seat.backend_keyboard_polling_enabled());
            let first_byte = linked_first_byte;
            let first_byte_source = if first_byte {
                "linked-runtime-hid"
            } else if parser_ingress {
                "local-seat-queue-diagnostic"
            } else {
                "none"
            };
            let proof_gate = if first_byte {
                linked_gate.max(10)
            } else if first_report {
                linked_gate.max(9)
            } else if keyboard_ready {
                linked_gate.max(8)
            } else if linked_controller_ready {
                linked_gate.max(3)
            } else {
                linked_gate
            };
            let progress_next = linked_progress
                .map(|progress| Self::usb_runtime_next_action_for_progress_phase(progress.phase));
            let next_step = if first_byte {
                "keyboard-first-byte"
            } else if input_observation == "idle-report-no-key-byte" {
                "press-key-for-first-byte"
            } else if first_report {
                "keyboard-first-byte"
            } else if keyboard_ready {
                "keyboard-first-report"
            } else if progress_refines_linked_detail {
                progress_next.unwrap_or("inspect-linked-usb-runtime-progress")
            } else if linked_detail != 0 {
                Self::usb_runtime_next_for_linked_detail(linked_detail)
            } else if let Some(progress_next) = progress_next {
                progress_next
            } else {
                "linked-runtime-init"
            };
            let progress_blocker = linked_progress
                .map(|progress| Self::usb_runtime_blocker_for_progress_phase(progress.phase));
            let blocker = if first_byte {
                "none"
            } else if input_observation == "idle-report-no-key-byte" {
                "awaiting-physical-key"
            } else if first_report {
                "keyboard-first-byte"
            } else if keyboard_ready {
                "hid-first-report"
            } else if progress_refines_linked_detail {
                progress_blocker.unwrap_or("linked-runtime-progress")
            } else if linked_detail != 0 {
                Self::usb_runtime_blocker_for_linked_detail(linked_detail)
            } else if let Some(progress_blocker) = progress_blocker {
                progress_blocker
            } else {
                "linked-runtime-no-detail"
            };
            let mut runtime_line = HeaplessString::<384>::new();
            let _ = FmtWrite::write_fmt(
                &mut runtime_line,
                format_args!(
                    "usb: runtime_gate keyboard={} first_report={} first_byte={} first_byte_source={} proof_gate={} target_gate=10 next={} blocker={} detail=0x{:04x} result=0x{:08x} progress_gate={} progress_phase={} progress_phase_name={}",
                    Self::yes_no(keyboard_ready),
                    Self::yes_no(first_report),
                    Self::yes_no(first_byte),
                    first_byte_source,
                    proof_gate,
                    next_step,
                    blocker,
                    linked_detail,
                    linked_result,
                    linked_progress_gate,
                    linked_progress.map_or(0, |progress| progress.phase),
                    linked_progress.map_or("none", |progress| progress.phase_name),
                ),
            );
            self.emit_console_line(runtime_line.as_str());
            let acceptance_line = format_message(format_args!(
                "usb: acceptance xhci={} hid_keyboard={} first_report={} first_byte={} usable={} prompt_polling={} input_observation={} death_proof=no note=hid_keyboard_requires_first_byte_for_input",
                Self::yes_no(proof_gate >= 3),
                Self::yes_no(keyboard_ready),
                Self::yes_no(first_report),
                Self::yes_no(first_byte),
                Self::yes_no(first_byte),
                Self::yes_no(prompt_polling_enabled),
                input_observation,
            ));
            self.emit_console_line(acceptance_line.as_str());
            let runtime_contract = format_message(format_args!(
                "usb: runtime_contract current={} expected={} blocker={} proof_gate={} target_gate=10 detail=0x{:04x} result=0x{:08x}",
                Self::usb_runtime_step_label(proof_gate),
                next_step,
                blocker,
                proof_gate,
                linked_detail,
                linked_result,
            ));
            self.emit_console_line(runtime_contract.as_str());
            let mut linked_runtime_snapshot = HeaplessString::<384>::new();
            let _ = FmtWrite::write_fmt(
                &mut linked_runtime_snapshot,
                format_args!(
                    "usb: linked_runtime_snapshot detail=0x{:04x} result=0x{:08x} proof_gate={} source=linked-runtime-progress-cache recovery_policy={} progress_gate={} progress_phase={} progress_phase_name={}",
                    linked_detail,
                    linked_result,
                    linked_gate,
                    Self::usb_runtime_recovery_policy_for_linked_detail(linked_detail),
                    linked_progress_gate,
                    linked_progress.map_or(0, |progress| progress.phase),
                    linked_progress.map_or("none", |progress| progress.phase_name),
                ),
            );
            self.emit_console_line(linked_runtime_snapshot.as_str());
            if let Some(progress) = linked_progress {
                let raw_progress_gate = Self::usb_runtime_gate_for_progress_phase(progress.phase);
                let progress_superseded = Self::usb_runtime_progress_superseded_by_keyboard(
                    first_byte,
                    proof_gate,
                    raw_progress_gate,
                );
                let progress_line_gate = if progress_superseded {
                    proof_gate
                } else {
                    raw_progress_gate
                };
                let progress_line_blocker = if progress_superseded {
                    "none"
                } else {
                    Self::usb_runtime_blocker_for_progress_phase(progress.phase)
                };
                let progress_line_next = if progress_superseded {
                    "keyboard-first-byte"
                } else {
                    Self::usb_runtime_next_action_for_progress_phase(progress.phase)
                };
                let progress_line = format_message(format_args!(
                    "usb: linked_runtime_progress marker_valid={} sequence={} phase={} phase_name={} aux0=0x{:08x} gate={} blocker={} next_action={} superseded={}",
                    Self::yes_no(progress.marker_valid),
                    progress.sequence,
                    progress.phase,
                    progress.phase_name,
                    progress.aux0,
                    progress_line_gate,
                    progress_line_blocker,
                    progress_line_next,
                    Self::yes_no(progress_superseded),
                ));
                self.emit_console_line(progress_line.as_str());
            } else {
                self.emit_console_line(
                    "usb: linked_runtime_progress marker_valid=no sequence=0 phase=0 phase_name=none aux0=0x00000000 gate=0 blocker=none next_action=submit-linked-runtime-command superseded=no",
                );
            }
            if linked_detail
                == pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING
                || linked_detail
                    == pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY
            {
                let runtime_queue = format_message(format_args!(
                    "usb: runtime_queue queue_valid=yes detail=0x{:04x} result=0x{:08x} queued_reports={} doorbell_pending={} preserved_events={} transfer_events={} report_status={}",
                    linked_detail,
                    linked_result,
                    queued_reports,
                    Self::yes_no(doorbell_pending),
                    preserved_events,
                    transfer_events,
                    Self::usb_runtime_keyboard_report_status_label(report_status),
                ));
                self.emit_console_line(runtime_queue.as_str());
                self.emit_usb_runtime_recovery(linked_frame);
            } else {
                let runtime_progress = format_message(format_args!(
                    "usb: runtime_progress phase=enumeration detail=0x{:04x} result=0x{:08x} queue=not-applicable",
                    linked_detail, linked_result,
                ));
                self.emit_console_line(runtime_progress.as_str());
            }
            let runtime_next_action = format_message(format_args!(
                "usb: runtime_next_action action={} reason={} recovery_policy={} input_observation={} detail=0x{:04x}",
                next_step,
                blocker,
                Self::usb_runtime_recovery_policy_for_linked_detail(linked_detail),
                input_observation,
                linked_detail,
            ));
            self.emit_console_line(runtime_next_action.as_str());
            let keyboard_trace_source = if linked_keyboard_ready || linked_first_report {
                "linked-runtime"
            } else {
                "local-seat-queue-diagnostic"
            };
            let keyboard_line = format_message(format_args!(
                "usb: keyboard_trace source={} polls={} backend_bytes={} queued={} accepted={} drained={} echoed={} dropped={} overruns={} no_reply={} recovery_aux={} recovery_pending={} cooldown={} cooldown_skips={}",
                keyboard_trace_source,
                local_trace.backend_poll_calls,
                local_trace.backend_read_bytes,
                local_trace.queued_bytes,
                local_trace.accepted_bytes,
                local_trace.drained_bytes,
                local_trace.echoed_bytes,
                local_trace.dropped_bytes,
                local_trace.driver_task_budget_overruns,
                local_trace.driver_task_no_replies,
                local_trace.recovery_aux_requests,
                Self::yes_no(local_trace.recovery_aux_pending),
                local_trace.poll_cooldown_turns,
                local_trace.poll_cooldown_skips,
            ));
            self.emit_console_line(keyboard_line.as_str());
        }
    }

    #[cfg(feature = "kernel")]
    fn emit_usb_related_driver_counter(
        &mut self,
        domain: &'static str,
        contract: crate::hal::driver_task::DriverTaskContract,
    ) {
        let Some(counters) = crate::hal::driver_task::driver_task_counter_snapshot(contract) else {
            let line = format_message(format_args!(
                "usb: stall_counter domain={domain} active=no contract={}",
                contract.name,
            ));
            self.emit_console_line(line.as_str());
            return;
        };
        let line = format_message(format_args!(
            "usb: stall_counter domain={domain} contract={} submitted={} completed={} busy={} same={} timeouts={} keep_active={} aborts={} fault={} budget={} rx={}/{} tx={}/{}",
            contract.name,
            counters.submitted_turns,
            counters.completed_turns,
            counters.busy_conflicts,
            counters.same_request_resumes,
            counters.timeouts,
            counters.keep_active_timeouts,
            counters.aborts,
            counters.fault_turns,
            counters.budget_exhausted_turns,
            counters.rx_frames,
            counters.rx_bytes,
            counters.tx_frames,
            counters.tx_bytes,
        ));
        self.emit_console_line(line.as_str());
    }

    #[cfg(feature = "kernel")]
    fn usb_capture_verdict(
        backend_attached: bool,
        polling_enabled: bool,
        exact_issue: Option<&'static str>,
    ) -> (&'static str, &'static str) {
        match exact_issue {
            Some("live-usbsts-read-before-run")
            | Some("live-usbcmd-read-before-run")
            | Some("halt-revalidation-timeout") => ("pre-run-halt-revalidation", "halt-before-run"),
            Some(issue) if issue.starts_with("pre-run-") => {
                ("pre-run-ownership-edge", "publish-before-run")
            }
            Some(issue) if issue.starts_with("post-run-") => {
                ("post-run-ownership-edge", "publish-after-run")
            }
            Some("usbcmd-run-barrier-wedged") | Some("usbcmd-run-store-wedged") => {
                ("run-transition-edge", "usbcmd-run")
            }
            Some(_) => ("xhci-diagnostic-edge", "controller-transition"),
            None if !backend_attached => ("backend-not-attached", "probe-controller"),
            None if polling_enabled => ("probe-in-progress", "poll-keyboard"),
            None => ("no-controller-edge-yet", "probe-keyboard"),
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_step_label(proof_gate: u8) -> &'static str {
        match proof_gate {
            0..=2 => "ownership-reset",
            3..=4 => "command-event-ring",
            5..=7 => "enumeration",
            8 => "keyboard-ready",
            9 => "hid-first-report",
            _ => "keyboard-online",
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_gate_for_linked_detail(detail: u16) -> u8 {
        match detail {
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY => 3,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING => 4,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY => 4,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ROOT_PORT_CONNECTED => 5,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED => 5,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_ADDRESSED => 6,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED => 6,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR => 7,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED => 6,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_TOPOLOGY_SEEN
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED => 7,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ENDPOINT_SEEN
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED => 7,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY => 8,
            pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING => 8,
            pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY => 9,
            _ => 0,
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_linked_detail_refinable_by_progress(detail: u16) -> bool {
        matches!(
            detail,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_ADDRESSED
                | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED
                | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR
                | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED
                | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR
                | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED
                | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_TOPOLOGY_SEEN
                | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED
                | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED
                | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
                | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED
                | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ENDPOINT_SEEN
                | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED
        )
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_progress_refines_linked_detail(
        linked_detail: u16,
        linked_detail_gate: u8,
        linked_progress_gate: u8,
    ) -> bool {
        linked_progress_gate > 0
            && (linked_progress_gate > linked_detail_gate
                || (linked_progress_gate == linked_detail_gate
                    && Self::usb_runtime_linked_detail_refinable_by_progress(linked_detail)))
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_gate_for_progress_phase(phase: u32) -> u8 {
        match phase {
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_ENTRY_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_RECV_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_POLL_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_REPLY_PENDING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_POLL_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_RING_READ_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_COMMAND_OBSERVED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_COMMAND_VALIDATED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_READY => 1,
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DISPATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_ENTER
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_AUX_MATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_FRAME_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_MARK_ENTER
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_LOADED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_VALID
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_INVALID
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_TOTALS_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_FORBIDDEN_PRESENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_ROLE_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCES_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RUNTIME_ENTRY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_INIT_ENTRY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_STATE_ACCESS_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_STATE_RESET_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_STATE_RESET_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HW_ENTRY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DMA_RANGE_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CAPS_READ_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CAPS_INVALID => 2,
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CAPS_READ
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HALT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HALT_WAIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HALTED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_WAIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CNR_WAIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RUN_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RUN_WAIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RUN_REQUESTED => 3,
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DMA_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_LOW_WRITTEN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_HIGH_WRITTEN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_HIGH_FLUSHED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_LOW_WRITTEN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_HIGH_WRITTEN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_HIGH_FLUSHED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DNCTRL_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_WRITTEN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_FLUSHED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_IMAN_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_IMOD_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTSZ_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_LOW_WRITTEN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_HIGH_WRITTEN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_HIGH_FLUSHED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_SLOT0_WRITTEN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_SLOT0_CLEANED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_ARRAY_FILLED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_ARRAY_CLEANED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_SUBMIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_TRB_WRITTEN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_DOORBELL_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_PENDING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_DMA_LOAD_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_INVALIDATE_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_PEEK_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_PORT_STATUS
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_COMMAND
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_OTHER
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_ERDP_ACK_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_ERDP_ACK_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_RETURN_PENDING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_LOW_WRITTEN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_HIGH_WRITTEN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_HIGH_FLUSHED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RINGS_READY => 4,
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_POWER_WRITE_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_CONNECT_WAIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_CONNECT_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_PR_SET
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_POLL_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_PRC_SEEN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_ENABLE_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_RETRY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_ENABLE_SLOT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_ENABLE_SLOT_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_CONTEXTS_PUBLISHED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_COMMAND
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_OTHER
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_PEEK_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_PORT_STATUS
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_POLL_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_POLL_PENDING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_FAILED => 5,
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_ADDRESSED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_WAIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_DATA_EVENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_EVENT_IGNORED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT_IGNORED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_WAIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_DATA_EVENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_EVENT_IGNORED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT_IGNORED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_TIMEOUT => 6,
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_WAIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_DATA_EVENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_IGNORED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_IGNORED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_WAIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_DATA_EVENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_EVENT_IGNORED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT_IGNORED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_FOUND
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_NO_INTERFACE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_NO_INTERRUPT_IN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_MALFORMED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_SET_CONFIGURATION_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_SET_CONFIGURATION_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_SET_CONFIGURATION_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONTROL_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONTROL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONTROL_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SCAN_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_WAIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_IGNORED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_WAIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DATA_EVENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_IGNORED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_IGNORED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CONTEXT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CONTEXT_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_POWER_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_POWER_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_WAIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DATA_EVENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_ACK_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_PAYLOAD_READ
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DISCONNECTED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_RESET_ACTIVE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_ENABLE_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_CLEAR_CHANGES_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_CLEAR_CHANGES_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_CLEAR_CHANGES_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_EVENT_IGNORED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT_IGNORED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CHILD_PROBE_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CHILD_SPEED_FALLBACK_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SCAN_NO_KEYBOARD => 7,
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_FAILED => 8,
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_DONE => 4,
            _ => 0,
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_blocker_for_progress_phase(phase: u32) -> &'static str {
        match phase {
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_ENTRY_READY => {
                "linked-runtime-recv-not-ready"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_RECV_READY => {
                "linked-runtime-command-not-observed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_POLL_READY => {
                "linked-runtime-command-not-visible"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_REPLY_PENDING => {
                "linked-runtime-reply-cap-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_POLL_BEGIN => {
                "linked-runtime-endpoint-poll-blocked"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_RING_READ_BEGIN => {
                "linked-runtime-ring-read-blocked"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_BEGIN => {
                "usb-engine-init-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_MARK_ENTER => {
                "usb-engine-init-mark-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_READY => {
                "usb-engine-init-descriptor-ready-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_BEGIN => {
                "usb-engine-init-resource-check-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_FAILED => {
                "usb-engine-init-resource-check-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_INVALID => {
                "usb-resource-descriptor-invalid"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH => {
                "usb-resource-hot-path-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_MISSING => {
                "usb-resource-xhci-mmio-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_MISSING => {
                "usb-resource-dma-arena-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_MISSING => {
                "usb-resource-shared-pages-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_MISSING => {
                "usb-resource-pcie-bus-link-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_VALID
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_TOTALS_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_ROLE_READY => {
                "usb-engine-init-resource-subcheck-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCES_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_BEGIN => {
                "usb-engine-init-hardware-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RUNTIME_ENTRY => {
                "usb-engine-init-runtime-entry-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_INIT_ENTRY => {
                "usb-runtime-init-entry-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_STATE_ACCESS_BEGIN => {
                "usb-runtime-state-access-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_STATE_RESET_BEGIN => {
                "usb-engine-init-state-reset-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_STATE_RESET_DONE => {
                "usb-engine-init-hardware-entry-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HW_ENTRY => {
                "usb-xhci-mmio-entry-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DMA_RANGE_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CAPS_READ_BEGIN => {
                "usb-xhci-capability-read-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CAPS_INVALID => {
                "usb-xhci-capability-invalid"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_PCIE_FLUSH_BEGIN => {
                "usb-pcie-posted-write-flush-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_PCIE_FLUSH_FAILED => {
                "usb-pcie-posted-write-flush-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_PCIE_FLUSH_DONE => {
                "usb-pcie-posted-write-flush-next-edge-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CAPS_READ => {
                "usb-xhci-controller-halt-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HALT_BEGIN => {
                "usb-xhci-halt-write-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HALT_WAIT_BEGIN => {
                "usb-xhci-halt-wait-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HALTED => "usb-xhci-reset-no-reply",
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_BEGIN => {
                "usb-xhci-reset-write-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_WAIT_BEGIN => {
                "usb-xhci-reset-wait-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CNR_WAIT_BEGIN => {
                "usb-xhci-controller-not-ready-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_DONE => {
                "usb-xhci-dma-setup-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DMA_READY => {
                "usb-xhci-ring-setup-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_BEGIN => {
                "usb-xhci-dcbaap-programming-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_LOW_WRITTEN => {
                "usb-xhci-dcbaap-high-write-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_HIGH_WRITTEN => {
                "usb-xhci-dcbaap-high-flush-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_HIGH_FLUSHED => {
                "usb-xhci-crcr-programming-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_BEGIN => {
                "usb-xhci-crcr-programming-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_LOW_WRITTEN => {
                "usb-xhci-crcr-high-write-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_HIGH_WRITTEN => {
                "usb-xhci-crcr-high-flush-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_HIGH_FLUSHED => {
                "usb-xhci-dnctrl-programming-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DNCTRL_BEGIN => {
                "usb-xhci-dnctrl-programming-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_BEGIN => {
                "usb-xhci-config-write-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_WRITTEN => {
                "usb-xhci-config-flush-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_FLUSHED => {
                "usb-xhci-dcbaap-programming-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_IMAN_BEGIN => {
                "usb-xhci-iman-programming-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_IMOD_BEGIN => {
                "usb-xhci-imod-programming-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTSZ_BEGIN => {
                "usb-xhci-erstsz-programming-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_BEGIN => {
                "usb-xhci-erstba-programming-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_LOW_WRITTEN => {
                "usb-xhci-erstba-high-write-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_HIGH_WRITTEN => {
                "usb-xhci-erstba-high-flush-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_HIGH_FLUSHED => {
                "usb-xhci-scratchpad-programming-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_BEGIN => {
                "usb-xhci-scratchpad-publication-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_SLOT0_WRITTEN => {
                "usb-xhci-scratchpad-slot0-clean-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_SLOT0_CLEANED => {
                "usb-xhci-scratchpad-array-fill-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_ARRAY_FILLED => {
                "usb-xhci-scratchpad-array-clean-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_ARRAY_CLEANED => {
                "usb-xhci-dnctrl-programming-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_BEGIN => {
                "usb-xhci-erdp-programming-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_LOW_WRITTEN => {
                "usb-xhci-erdp-high-write-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_HIGH_WRITTEN => {
                "usb-xhci-erdp-high-flush-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_HIGH_FLUSHED => {
                "usb-xhci-rings-ready-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RINGS_READY => {
                "usb-xhci-run-transition-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RUN_BEGIN => {
                "usb-xhci-run-request-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RUN_WAIT_BEGIN => {
                "usb-xhci-run-transition-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RUN_REQUESTED => {
                "usb-xhci-command-ring-proof-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_SUBMIT_BEGIN => {
                "usb-xhci-command-proof-submit-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_TRB_WRITTEN => {
                "usb-xhci-command-proof-doorbell-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_DOORBELL_BEGIN => {
                "usb-xhci-command-doorbell-publish-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_DOORBELL_DONE => {
                "enable-slot-completion-pending"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_BEGIN => {
                "enable-slot-completion-poll-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_PENDING => {
                "enable-slot-completion-pending"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_READY => {
                "usb-xhci-command-ring-ready"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_FAILED => {
                "enable-slot-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_DMA_LOAD_DONE => {
                "enable-slot-event-dma-load-done-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_INVALIDATE_DONE => {
                "enable-slot-event-invalidate-done-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_PEEK_BEGIN => {
                "enable-slot-event-peek-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_BEGIN => {
                "enable-slot-event-read-begin-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_DONE => {
                "enable-slot-event-read-done-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_SLOT_EMPTY => {
                "enable-slot-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_CYCLE_MISMATCH => {
                "enable-slot-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_PORT_STATUS => {
                "enable-slot-poll-leading-port-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_COMMAND => {
                "enable-slot-command-event-seen"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_OTHER => {
                "enable-slot-poll-non-command-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_ERDP_ACK_BEGIN => {
                "enable-slot-event-ack-pending"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_ERDP_ACK_DONE => {
                "enable-slot-event-ack-complete"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_RETURN_PENDING => {
                "enable-slot-completion-pending"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_BEGIN => {
                "root-port-reset-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_DONE => {
                "address-enable-slot-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_POWER_WRITE_DONE => {
                "root-port-connect-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_CONNECT_WAIT_BEGIN => {
                "root-port-connect-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_CONNECT_TIMEOUT => {
                "root-port-connect-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_PR_SET => {
                "root-port-reset-completion-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_POLL_BEGIN => {
                "root-port-reset-completion-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_PRC_SEEN => {
                "root-port-enable-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_ENABLE_TIMEOUT => {
                "root-port-enable-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_TIMEOUT => {
                "root-port-reset-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_RETRY => {
                "root-port-reset-retry"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_FAILED => {
                "root-port-reset-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_BEGIN => {
                "root-port-stale-cleanup-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_DONE => {
                "address-enable-slot-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_FAILED => {
                "root-port-stale-cleanup-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_ENABLE_SLOT_BEGIN => {
                "address-enable-slot-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_ENABLE_SLOT_DONE => {
                "address-device-context-publish-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_CONTEXTS_PUBLISHED => {
                "address-device-command-submit-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_BEGIN => {
                "address-device-command-completion-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_COMMAND => {
                "address-device-command-event-completion-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_CYCLE_MISMATCH => {
                "address-device-command-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_OTHER => {
                "address-device-command-event-other"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_PEEK_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_POLL_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_POLL_PENDING => {
                "address-device-command-completion-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_PORT_STATUS => {
                "address-device-command-event-port-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_SLOT_EMPTY => {
                "address-device-command-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_DONE => {
                "address-device-publish-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_ADDRESSED => {
                "device-descriptor-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_BEGIN => {
                "device-descriptor-submit-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_WAIT_BEGIN => {
                "device-descriptor-transfer-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_DATA_EVENT => {
                "device-descriptor-status-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT => {
                "config-descriptor-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_FAILED => {
                "device-descriptor-transfer-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_TIMEOUT => {
                "device-descriptor-transfer-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_TIMEOUT => {
                "device-descriptor-status-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_EVENT_SLOT_EMPTY => {
                "device-descriptor-transfer-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_EVENT_CYCLE_MISMATCH => {
                "device-descriptor-transfer-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_EVENT_IGNORED => {
                "device-descriptor-transfer-event-ignored"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT_SLOT_EMPTY => {
                "device-descriptor-status-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT_CYCLE_MISMATCH => {
                "device-descriptor-status-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT_IGNORED => {
                "device-descriptor-status-event-ignored"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_BEGIN => {
                "device-descriptor-prime-submit-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_WAIT_BEGIN => {
                "device-descriptor-prime-transfer-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_DATA_EVENT => {
                "device-descriptor-prime-status-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT => {
                "device-descriptor-full-read-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_FAILED => {
                "device-descriptor-prime-transfer-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_TIMEOUT => {
                "device-descriptor-prime-transfer-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_TIMEOUT => {
                "device-descriptor-prime-status-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_EVENT_SLOT_EMPTY => {
                "device-descriptor-prime-transfer-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_EVENT_CYCLE_MISMATCH => {
                "device-descriptor-prime-transfer-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_EVENT_IGNORED => {
                "device-descriptor-prime-transfer-event-ignored"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT_SLOT_EMPTY => {
                "device-descriptor-prime-status-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT_CYCLE_MISMATCH => {
                "device-descriptor-prime-status-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT_IGNORED => {
                "device-descriptor-prime-status-event-ignored"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_BEGIN => {
                "config-descriptor-header-submit-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_WAIT_BEGIN => {
                "config-descriptor-header-transfer-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_DATA_EVENT => {
                "config-descriptor-header-status-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT => {
                "config-descriptor-full-read-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_FAILED => {
                "config-descriptor-header-transfer-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_TIMEOUT => {
                "config-descriptor-header-transfer-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_TIMEOUT => {
                "config-descriptor-header-status-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_SLOT_EMPTY => {
                "config-descriptor-header-transfer-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_CYCLE_MISMATCH => {
                "config-descriptor-header-transfer-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_IGNORED => {
                "config-descriptor-header-transfer-event-ignored"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_SLOT_EMPTY => {
                "config-descriptor-header-status-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_CYCLE_MISMATCH => {
                "config-descriptor-header-status-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_IGNORED => {
                "config-descriptor-header-status-event-ignored"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_BEGIN => {
                "config-descriptor-full-submit-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_WAIT_BEGIN => {
                "config-descriptor-full-transfer-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_DATA_EVENT => {
                "config-descriptor-full-status-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT => {
                "hid-endpoint-not-ready"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_FAILED => {
                "config-descriptor-full-transfer-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_TIMEOUT => {
                "config-descriptor-full-transfer-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_TIMEOUT => {
                "config-descriptor-full-status-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_EVENT_SLOT_EMPTY => {
                "config-descriptor-full-transfer-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_EVENT_CYCLE_MISMATCH => {
                "config-descriptor-full-transfer-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_EVENT_IGNORED => {
                "config-descriptor-full-transfer-event-ignored"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT_SLOT_EMPTY => {
                "config-descriptor-full-status-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT_CYCLE_MISMATCH => {
                "config-descriptor-full-status-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT_IGNORED => {
                "config-descriptor-full-status-event-ignored"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_BEGIN => {
                "hid-endpoint-parse-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_FOUND => {
                "hid-configure-endpoint-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_MISSING => {
                "hid-endpoint-not-found"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_NO_INTERFACE => {
                "hid-interface-not-found"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_NO_INTERRUPT_IN => {
                "hid-interrupt-in-not-found"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_MALFORMED => {
                "hid-config-descriptor-malformed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_BEGIN => {
                "hid-configure-endpoint-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_DONE => {
                "hid-set-configuration-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_FAILED => {
                "hid-configure-endpoint-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_SET_CONFIGURATION_BEGIN => {
                "hid-set-configuration-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_SET_CONFIGURATION_DONE => {
                "hid-control-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_SET_CONFIGURATION_FAILED => {
                "hid-set-configuration-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONTROL_BEGIN => {
                "hid-control-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONTROL_DONE => {
                "hid-interrupt-queue-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONTROL_FAILED => {
                "hid-control-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_BEGIN => {
                "hid-interrupt-queue-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_READY => {
                "first-hid-report"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_FAILED => {
                "hid-interrupt-queue-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SCAN_BEGIN => {
                "hub-child-scan-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_BEGIN => {
                "hub-set-configuration-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_WAIT_BEGIN => {
                "hub-set-configuration-status-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT => {
                "hub-set-configuration-complete-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_SLOT_EMPTY => {
                "hub-set-configuration-status-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_CYCLE_MISMATCH => {
                "hub-set-configuration-status-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_IGNORED => {
                "hub-set-configuration-status-event-ignored"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_TIMEOUT => {
                "hub-set-configuration-status-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_FAILED => {
                "hub-set-configuration-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_DONE => {
                "hub-set-configuration-settle-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_BEGIN => {
                "hub-descriptor-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_WAIT_BEGIN => {
                "hub-descriptor-transfer-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DATA_EVENT => {
                "hub-descriptor-status-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT => {
                "hub-context-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_FAILED => {
                "hub-descriptor-transfer-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_TIMEOUT => {
                "hub-descriptor-transfer-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_TIMEOUT => {
                "hub-descriptor-status-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_SLOT_EMPTY => {
                "hub-descriptor-transfer-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_CYCLE_MISMATCH => {
                "hub-descriptor-transfer-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_IGNORED => {
                "hub-descriptor-transfer-event-ignored"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_SLOT_EMPTY => {
                "hub-descriptor-status-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_CYCLE_MISMATCH => {
                "hub-descriptor-status-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_IGNORED => {
                "hub-descriptor-status-event-ignored"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DONE => {
                "hub-context-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CONTEXT_BEGIN => {
                "hub-context-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CONTEXT_DONE => {
                "hub-port-power-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_POWER_BEGIN => {
                "hub-port-power-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_POWER_DONE => {
                "hub-port-status-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_BEGIN => {
                "hub-port-status-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_WAIT_BEGIN => {
                "hub-port-status-transfer-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DATA_EVENT => {
                "hub-port-status-status-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT => {
                "hub-port-reset-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_ACK_DONE => {
                "hub-port-status-payload-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_PAYLOAD_READ => {
                "hub-port-reset-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DISCONNECTED => {
                "hub-port-disconnected"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_RESET_ACTIVE => {
                "hub-port-reset-still-active"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_ENABLE_MISSING => {
                "hub-port-enable-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_CLEAR_CHANGES_BEGIN => {
                "hub-port-clear-changes-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_CLEAR_CHANGES_DONE => {
                "hub-port-reset-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_CLEAR_CHANGES_FAILED => {
                "hub-port-clear-changes-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_TIMEOUT => {
                "hub-port-status-transfer-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_TIMEOUT => {
                "hub-port-status-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_EVENT_SLOT_EMPTY => {
                "hub-port-status-transfer-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_EVENT_CYCLE_MISMATCH => {
                "hub-port-status-transfer-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_EVENT_IGNORED => {
                "hub-port-status-transfer-event-ignored"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT_SLOT_EMPTY => {
                "hub-port-status-status-event-slot-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT_CYCLE_MISMATCH => {
                "hub-port-status-status-event-cycle-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT_IGNORED => {
                "hub-port-status-status-event-ignored"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DONE => {
                "hub-port-reset-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_FAILED => {
                "hub-port-status-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_BEGIN => {
                "hub-port-reset-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_BEGIN => {
                "hub-port-reset-set-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_DONE => {
                "hub-port-reset-completion-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_FAILED => {
                "hub-port-reset-set-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_READY => {
                "hub-child-probe-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CHILD_PROBE_BEGIN => {
                "hub-child-probe-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CHILD_SPEED_FALLBACK_BEGIN => {
                "hub-child-speed-fallback-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SCAN_NO_KEYBOARD => {
                "hub-topology-no-keyboard"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_FAILED => {
                "address-device-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_FAILED => {
                "usb-engine-init-failed"
            }
            _ => "usb-linked-runtime-progress-no-reply",
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_next_action_for_progress_phase(phase: u32) -> &'static str {
        match phase {
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_BEGIN => {
                "inspect-linked-usb-runtime-engine-init-dispatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_MARK_ENTER => {
                "inspect-linked-usb-runtime-descriptor-load"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_READY => {
                "inspect-linked-usb-runtime-resource-check"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_BEGIN => {
                "inspect-linked-usb-descriptor-resource-scan"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_FAILED => {
                "inspect-usb-runtime-init-descriptor-ranges-and-bus-link"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_INVALID => {
                "inspect-usb-runtime-init-descriptor-header"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH => {
                "inspect-usb-runtime-hot-path-and-role-bit"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_MISSING => {
                "inspect-usb-xhci-mmio-resource-range"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_MISSING => {
                "inspect-usb-dma-arena-resource-range"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_MISSING => {
                "inspect-usb-shared-page-resource-range"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_MISSING => {
                "inspect-usb-pcie-pointer-free-bus-link"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_VALID
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_TOTALS_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_ROLE_READY => {
                "inspect-next-usb-resource-subcheck"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCES_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_BEGIN => {
                "inspect-usb-runtime-init-entry"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RUNTIME_ENTRY => {
                "inspect-usb-engine-init-hot-path-dispatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_INIT_ENTRY => {
                "inspect-usb-runtime-state-storage"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_STATE_ACCESS_BEGIN => {
                "inspect-usb-runtime-state-borrow"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_STATE_RESET_BEGIN => {
                "inspect-usb-runtime-state-reset"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_STATE_RESET_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HW_ENTRY => {
                "inspect-usb-xhci-mmio-entry"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DMA_RANGE_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CAPS_READ_BEGIN => {
                "inspect-usb-xhci-capability-register-read"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CAPS_INVALID => {
                "inspect-usb-xhci-capability-snapshot"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CAPS_READ => {
                "inspect-xhci-halt-status-loop"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HALT_BEGIN => {
                "inspect-xhci-run-clear-same-runtime-readback-drain"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HALT_WAIT_BEGIN => {
                "inspect-xhci-halted-status-loop"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HALTED => {
                "inspect-xhci-reset-completion-loop"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_BEGIN => {
                "inspect-xhci-reset-same-runtime-readback-drain"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_WAIT_BEGIN => {
                "inspect-xhci-reset-clear-loop"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CNR_WAIT_BEGIN => {
                "inspect-xhci-controller-not-ready-loop"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RESET_DONE => {
                "inspect-xhci-dma-and-scratchpad-layout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_PCIE_FLUSH_BEGIN => {
                "inspect-historical-usb-pcie-posted-write-flush"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_PCIE_FLUSH_DONE => {
                "inspect-next-usb-register-programming-edge"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_PCIE_FLUSH_FAILED => {
                "inspect-historical-pcie-owner-posted-write-flush-fault"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DMA_READY => {
                "inspect-xhci-command-event-ring-programming"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_BEGIN => {
                "inspect-xhci-dcbaap-register-programming"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_LOW_WRITTEN => {
                "inspect-xhci-dcbaap-high-register-write"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_HIGH_WRITTEN => {
                "inspect-xhci-dcbaap-same-runtime-readback-drain"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DCBAAP_HIGH_FLUSHED => {
                "inspect-xhci-command-ring-control-programming"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_BEGIN => {
                "inspect-xhci-command-ring-control-programming"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_LOW_WRITTEN => {
                "inspect-xhci-crcr-high-register-write"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_HIGH_WRITTEN => {
                "inspect-xhci-crcr-same-runtime-readback-drain"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CRCR_HIGH_FLUSHED => {
                "inspect-xhci-device-notification-control-programming"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DNCTRL_BEGIN => {
                "inspect-xhci-device-notification-control-programming"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_BEGIN => {
                "inspect-xhci-enabled-slot-config-register-write"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_WRITTEN => {
                "inspect-xhci-enabled-slot-config-posted-write-drain"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_FLUSHED => {
                "inspect-xhci-dcbaap-register-programming"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_IMAN_BEGIN => {
                "inspect-xhci-interrupter-control-programming"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_IMOD_BEGIN => {
                "inspect-xhci-interrupter-moderation-programming"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTSZ_BEGIN => {
                "inspect-xhci-event-ring-segment-table-size"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_BEGIN => {
                "inspect-xhci-event-ring-segment-table-address"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_LOW_WRITTEN => {
                "inspect-xhci-erstba-high-register-write"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_HIGH_WRITTEN => {
                "inspect-xhci-erstba-same-runtime-readback-drain"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERSTBA_HIGH_FLUSHED => {
                "inspect-xhci-scratchpad-array-publication"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_BEGIN => {
                "inspect-xhci-scratchpad-array-publication"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_SLOT0_WRITTEN => {
                "inspect-xhci-scratchpad-dcbaa-slot0-clean"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_SLOT0_CLEANED => {
                "inspect-xhci-scratchpad-pointer-array-translation"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_ARRAY_FILLED => {
                "inspect-xhci-scratchpad-pointer-array-clean"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_SCRATCHPAD_ARRAY_CLEANED => {
                "inspect-xhci-device-notification-control-programming"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_SUBMIT_BEGIN => {
                "verify-enable-slot-trb-publish"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_TRB_WRITTEN => {
                "verify-command-doorbell-publish"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_DOORBELL_BEGIN => {
                "verify-command-doorbell-posted-write-barrier"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_DOORBELL_DONE => {
                "poll-enable-slot-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_BEGIN => {
                "inspect-event-ring-command-completion-poll"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_PENDING => {
                "poll-enable-slot-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_READY => {
                "continue-root-port-sampling"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_POLL_FAILED => {
                "inspect-enable-slot-command-completion-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_DMA_LOAD_DONE => {
                "inspect-event-ring-trb-memory-read"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_INVALIDATE_DONE => {
                "inspect-event-trb-word-read-after-invalidate"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_PEEK_BEGIN => {
                "inspect-event-ring-trb-read-or-cache-invalidate"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_BEGIN => {
                "inspect-event-ring-dma-load-or-trb-read"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_DONE => {
                "inspect-event-trb-classification-after-read"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_SLOT_EMPTY => {
                "inspect-event-ring-publication-or-controller-writeback"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_CYCLE_MISMATCH => {
                "inspect-event-ring-cycle-state"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_PORT_STATUS => {
                "ack-leading-port-status-and-continue-command-poll"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_COMMAND => {
                "decode-enable-slot-command-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_OTHER => {
                "skip-non-command-event-and-continue-command-poll"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_ERDP_ACK_BEGIN => {
                "complete-prompt-safe-erdp-ack"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_ERDP_ACK_DONE => {
                "continue-enable-slot-command-poll"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_RETURN_PENDING => {
                "poll-enable-slot-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_BEGIN => {
                "inspect-root-port-reset-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_DONE => {
                "submit-address-enable-slot-command"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_POWER_WRITE_DONE => {
                "wait-root-port-connect-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_CONNECT_WAIT_BEGIN => {
                "wait-root-port-connect-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_CONNECT_TIMEOUT => {
                "inspect-root-port-connect-and-power"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_PR_SET => {
                "poll-root-port-reset-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_POLL_BEGIN => {
                "poll-root-port-reset-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_PRC_SEEN => {
                "inspect-root-port-enable-bit"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_ENABLE_TIMEOUT => {
                "inspect-root-port-enable-after-reset"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_TIMEOUT => {
                "inspect-root-port-reset-change-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_RETRY => {
                "continue-root-port-reset-retry"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_FAILED => {
                "inspect-root-port-reset-retry-exhaustion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_BEGIN => {
                "complete-stale-uboot-root-port-cleanup"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_DONE => {
                "submit-address-enable-slot-command"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_FAILED => {
                "continue-with-first-root-port-reset-proof"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_ENABLE_SLOT_BEGIN => {
                "inspect-address-enable-slot-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_ENABLE_SLOT_DONE => {
                "publish-address-device-contexts-and-dcbaa"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_CONTEXTS_PUBLISHED => {
                "submit-address-device-command"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_BEGIN => {
                "poll-address-device-command-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_COMMAND => {
                "consume-address-device-command-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_CYCLE_MISMATCH => {
                "inspect-address-device-event-cycle-state"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_OTHER => {
                "inspect-address-device-unexpected-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_PEEK_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_POLL_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_POLL_PENDING => {
                "poll-address-device-command-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_PORT_STATUS => {
                "preserve-port-event-and-continue-address-command"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_SLOT_EMPTY => {
                "wait-for-address-device-command-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_DONE => {
                "publish-device-addressed-detail"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_ADDRESSED => {
                "read-device-descriptor"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_BEGIN => {
                "publish-ep0-device-descriptor-transfer"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_WAIT_BEGIN => {
                "poll-ep0-device-descriptor-transfer"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_DATA_EVENT => {
                "poll-ep0-device-descriptor-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT => {
                "read-config-descriptor"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_FAILED => {
                "inspect-ep0-device-descriptor-transfer-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_TIMEOUT => {
                "inspect-missing-ep0-device-descriptor-data-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_TIMEOUT => {
                "inspect-missing-ep0-device-descriptor-status-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_EVENT_SLOT_EMPTY => {
                "inspect-ep0-device-descriptor-event-ring-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_EVENT_CYCLE_MISMATCH => {
                "inspect-ep0-device-descriptor-event-cycle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_TRANSFER_EVENT_IGNORED => {
                "inspect-ep0-device-descriptor-ignored-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT_SLOT_EMPTY => {
                "inspect-ep0-device-descriptor-status-event-ring-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT_CYCLE_MISMATCH => {
                "inspect-ep0-device-descriptor-status-event-cycle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT_IGNORED => {
                "inspect-ep0-device-descriptor-status-ignored-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_BEGIN => {
                "publish-ep0-device-descriptor-prime"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_WAIT_BEGIN => {
                "poll-ep0-device-descriptor-prime"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_DATA_EVENT => {
                "poll-ep0-device-descriptor-prime-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT => {
                "read-full-device-descriptor"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_FAILED => {
                "inspect-ep0-device-descriptor-prime-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_TIMEOUT => {
                "inspect-missing-ep0-device-descriptor-prime-data-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_TIMEOUT => {
                "inspect-missing-ep0-device-descriptor-prime-status-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_EVENT_SLOT_EMPTY => {
                "inspect-ep0-device-descriptor-prime-event-ring-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_EVENT_CYCLE_MISMATCH => {
                "inspect-ep0-device-descriptor-prime-event-cycle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_TRANSFER_EVENT_IGNORED => {
                "inspect-ep0-device-descriptor-prime-ignored-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT_SLOT_EMPTY => {
                "inspect-ep0-device-descriptor-prime-status-event-ring-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT_CYCLE_MISMATCH => {
                "inspect-ep0-device-descriptor-prime-status-event-cycle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_STATUS_EVENT_IGNORED => {
                "inspect-ep0-device-descriptor-prime-status-ignored-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_BEGIN => {
                "publish-ep0-config-descriptor-header"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_WAIT_BEGIN => {
                "poll-ep0-config-descriptor-header"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_DATA_EVENT => {
                "poll-ep0-config-descriptor-header-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT => {
                "read-full-config-descriptor"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_FAILED => {
                "inspect-ep0-config-descriptor-header-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_TIMEOUT => {
                "inspect-missing-ep0-config-descriptor-header-data-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_TIMEOUT => {
                "inspect-missing-ep0-config-descriptor-header-status-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_SLOT_EMPTY => {
                "inspect-ep0-config-descriptor-header-event-ring-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_CYCLE_MISMATCH => {
                "inspect-ep0-config-descriptor-header-event-cycle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_IGNORED => {
                "inspect-ep0-config-descriptor-header-ignored-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_SLOT_EMPTY => {
                "inspect-ep0-config-descriptor-header-status-event-ring-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_CYCLE_MISMATCH => {
                "inspect-ep0-config-descriptor-header-status-event-cycle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_IGNORED => {
                "inspect-ep0-config-descriptor-header-status-ignored-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_BEGIN => {
                "publish-ep0-full-config-descriptor"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_WAIT_BEGIN => {
                "poll-ep0-full-config-descriptor"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_DATA_EVENT => {
                "poll-ep0-full-config-descriptor-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT => {
                "parse-hid-keyboard-endpoint"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_FAILED => {
                "inspect-ep0-full-config-descriptor-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_TIMEOUT => {
                "inspect-missing-ep0-full-config-descriptor-data-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_TIMEOUT => {
                "inspect-missing-ep0-full-config-descriptor-status-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_EVENT_SLOT_EMPTY => {
                "inspect-ep0-full-config-descriptor-event-ring-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_EVENT_CYCLE_MISMATCH => {
                "inspect-ep0-full-config-descriptor-event-cycle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_TRANSFER_EVENT_IGNORED => {
                "inspect-ep0-full-config-descriptor-ignored-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT_SLOT_EMPTY => {
                "inspect-ep0-full-config-descriptor-status-event-ring-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT_CYCLE_MISMATCH => {
                "inspect-ep0-full-config-descriptor-status-event-cycle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT_IGNORED => {
                "inspect-ep0-full-config-descriptor-status-ignored-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_BEGIN => {
                "parse-hid-keyboard-endpoint"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_FOUND => {
                "submit-hid-configure-endpoint"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_MISSING => {
                "inspect-config-descriptor-hid-interface"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_NO_INTERFACE => {
                "inspect-config-descriptor-interface-classes"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_NO_INTERRUPT_IN => {
                "inspect-config-descriptor-endpoint-shape"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_MALFORMED => {
                "inspect-config-descriptor-lengths"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_BEGIN => {
                "poll-hid-configure-endpoint"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_DONE => {
                "submit-hid-set-configuration"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_FAILED => {
                "inspect-hid-endpoint-context"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_SET_CONFIGURATION_BEGIN => {
                "poll-hid-set-configuration"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_SET_CONFIGURATION_DONE => {
                "submit-hid-control-setup"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_SET_CONFIGURATION_FAILED => {
                "inspect-hid-set-configuration-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONTROL_BEGIN => {
                "poll-hid-control-setup"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONTROL_DONE => {
                "arm-hid-interrupt-queue"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONTROL_FAILED => {
                "inspect-hid-class-control-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_BEGIN => {
                "poll-hid-interrupt-queue-arm"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_READY => {
                "wait-first-hid-report"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_FAILED => {
                "inspect-hid-interrupt-doorbell"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SCAN_BEGIN => {
                "probe-hub-child-ports"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_BEGIN => {
                "poll-hub-set-configuration-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_WAIT_BEGIN => {
                "poll-hub-set-configuration-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT => {
                "wait-hub-set-configuration-settle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_SLOT_EMPTY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_CYCLE_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_IGNORED => {
                "inspect-hub-set-configuration-status-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_TIMEOUT => {
                "inspect-missing-hub-set-configuration-status-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_FAILED => {
                "inspect-hub-set-configuration-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_DONE => {
                "wait-hub-set-configuration-settle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_BEGIN => {
                "read-hub-descriptor"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_WAIT_BEGIN => {
                "poll-ep0-hub-descriptor"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DATA_EVENT => {
                "poll-ep0-hub-descriptor-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT => {
                "evaluate-hub-context"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_FAILED => {
                "inspect-ep0-hub-descriptor-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_TIMEOUT => {
                "inspect-missing-ep0-hub-descriptor-data-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_TIMEOUT => {
                "inspect-missing-ep0-hub-descriptor-status-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_SLOT_EMPTY => {
                "inspect-ep0-hub-descriptor-event-ring-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_CYCLE_MISMATCH => {
                "inspect-ep0-hub-descriptor-event-cycle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_IGNORED => {
                "inspect-ep0-hub-descriptor-ignored-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_SLOT_EMPTY => {
                "inspect-ep0-hub-descriptor-status-event-ring-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_CYCLE_MISMATCH => {
                "inspect-ep0-hub-descriptor-status-event-cycle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_IGNORED => {
                "inspect-ep0-hub-descriptor-status-ignored-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DONE => {
                "evaluate-hub-context"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CONTEXT_BEGIN => {
                "poll-hub-context-evaluate"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CONTEXT_DONE => {
                "power-hub-child-port"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_POWER_BEGIN => {
                "wait-hub-port-power"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_POWER_DONE => {
                "read-hub-port-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_BEGIN => {
                "inspect-hub-port-status-control-transfer"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DOORBELL_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_WAIT_BEGIN => {
                "poll-ep0-hub-port-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DATA_EVENT => {
                "poll-ep0-hub-port-status-status"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT => {
                "clear-hub-port-changes-or-reset"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_ACK_DONE => {
                "read-hub-port-status-payload"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_PAYLOAD_READ => {
                "clear-hub-port-changes-or-reset"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DISCONNECTED => {
                "power-cycle-or-skip-hub-port"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_RESET_ACTIVE => {
                "wait-hub-port-reset-clear"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_ENABLE_MISSING => {
                "retry-hub-port-reset"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_CLEAR_CHANGES_BEGIN => {
                "wait-hub-port-clear-changes"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_CLEAR_CHANGES_DONE => {
                "retry-hub-port-reset-or-child-probe"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_CLEAR_CHANGES_FAILED => {
                "inspect-hub-port-clear-changes"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_TIMEOUT => {
                "inspect-missing-hub-port-status-data-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_TIMEOUT => {
                "inspect-missing-hub-port-status-status-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_EVENT_SLOT_EMPTY => {
                "inspect-hub-port-status-event-ring-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_EVENT_CYCLE_MISMATCH => {
                "inspect-hub-port-status-event-cycle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_EVENT_IGNORED => {
                "inspect-hub-port-status-ignored-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT_SLOT_EMPTY => {
                "inspect-hub-port-status-status-event-ring-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT_CYCLE_MISMATCH => {
                "inspect-hub-port-status-status-event-cycle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT_IGNORED => {
                "inspect-hub-port-status-status-ignored-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DONE => {
                "clear-hub-port-changes-or-reset"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_FAILED => {
                "inspect-hub-port-status-control-transfer"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_BEGIN => {
                "submit-hub-port-reset"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_BEGIN => {
                "inspect-hub-port-reset-control-transfer"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_DONE => {
                "poll-hub-port-reset-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_FAILED => {
                "inspect-hub-port-reset-control-transfer"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_READY => {
                "probe-hub-child-device"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CHILD_PROBE_BEGIN => {
                "inspect-hub-child-address-or-descriptor"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CHILD_SPEED_FALLBACK_BEGIN => {
                "probe-hub-child-fallback-speed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SCAN_NO_KEYBOARD => {
                "inspect-hub-child-config-descriptor"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_FAILED => {
                "continue-enumeration-same-controller"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_BEGIN => {
                "inspect-xhci-event-ring-dequeue-pointer"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_LOW_WRITTEN => {
                "inspect-xhci-erdp-high-register-write"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_HIGH_WRITTEN => {
                "inspect-xhci-erdp-same-runtime-readback-drain"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ERDP_HIGH_FLUSHED => {
                "inspect-xhci-run-transition-and-same-runtime-readback-drain"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RINGS_READY => {
                "inspect-xhci-run-transition-and-same-runtime-readback-drain"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RUN_BEGIN => {
                "inspect-xhci-run-command-same-runtime-readback-drain"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RUN_WAIT_BEGIN => {
                "inspect-xhci-run-state-transition"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_RUN_REQUESTED => {
                "poll-enable-slot-completion"
            }
            _ => "inspect-linked-usb-runtime-progress",
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_next_for_linked_detail(detail: u16) -> &'static str {
        match detail {
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY => "command-ring-ready",
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING => {
                "command-ring-ready"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY => {
                "root-port-connected"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ROOT_PORT_CONNECTED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED => {
                "device-addressed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_ADDRESSED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED => {
                "device-descriptor"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED => {
                "config-descriptor"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_TOPOLOGY_SEEN
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED => "hid-keyboard",
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ENDPOINT_SEEN
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED => "keyboard-ready",
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY => {
                "keyboard-first-report"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING => {
                "keyboard-first-report"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY => {
                "keyboard-first-byte"
            }
            _ => "keyboard-ready",
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_blocker_for_linked_detail(detail: u16) -> &'static str {
        match detail {
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY => {
                "command-event-ring-not-proven"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING => {
                "enable-slot-completion-pending"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY => {
                "command-ring-ready"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ROOT_PORT_CONNECTED => {
                "root-port-connected"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED => {
                "enable-slot-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_ADDRESSED => "device-addressed",
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED => {
                "address-device-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR => "device-descriptor",
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED => {
                "device-descriptor-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR => "config-descriptor",
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED => {
                "config-descriptor-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_TOPOLOGY_SEEN => {
                "hub-topology-no-keyboard"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED => "hub-attach-failed",
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED => {
                "hub-set-configuration-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED => {
                "hub-descriptor-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED => {
                "hub-context-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ENDPOINT_SEEN => {
                "hid-endpoint-not-ready"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED => "hid-attach-failed",
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY => "none",
            pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING => {
                "hid-first-report"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY => {
                "keyboard-first-byte"
            }
            _ => "keyboard-not-ready",
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_next_action_for_linked_detail(detail: u16) -> &'static str {
        match detail {
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY => {
                "submit-enable-slot-command"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING => {
                "poll-enable-slot-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED => {
                "cold-reinit-and-reenumerate"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED => {
                "continue-enumeration-same-controller"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY => "wait-first-report",
            pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING => {
                "poll-linked-interrupt-in-and-rering-endpoint-doorbell"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY => {
                "wait-first-keyboard-byte"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ROOT_PORT_CONNECTED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_ADDRESSED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_TOPOLOGY_SEEN
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ENDPOINT_SEEN => {
                "continue-enumeration"
            }
            _ => "wait-driver-task-replay",
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_recovery_policy_for_linked_detail(detail: u16) -> &'static str {
        match detail {
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY => {
                "same-controller-command-proof"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING => {
                "same-controller-command-proof"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY => {
                "same-controller-enumeration"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED => "cold-reinit",
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED => {
                "same-controller"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY
            | pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING
            | pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY => {
                "hid-report-path"
            }
            _ => "continue-or-wait",
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_queue_fields(result: u32) -> (u32, bool, u32, u32, u32) {
        (
            result & 0xff,
            ((result >> 8) & 0x1) != 0,
            (result >> 16) & 0xff,
            (result >> 24) & 0xff,
            (result >> pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT)
                & pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_MASK,
        )
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_keyboard_recovery_diag_fields(
        frame: crate::hal::driver_task::DriverFrameDescriptor,
    ) -> (bool, u32, u32, u32, u32, u32, u32) {
        let valid =
            ((frame.offset >> 16) as u16) == pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_DIAG_MAGIC;
        if !valid {
            return (false, 0, 0, 0, 0, 0, 0);
        }
        (
            true,
            frame.offset & 0xff,
            (frame.offset >> 8) & 0xff,
            (frame.len & 0xff) as u32,
            ((frame.len >> 8) & 0xff) as u32,
            (frame.flags & 0xff) as u32,
            ((frame.flags >> 8) & 0xff) as u32,
        )
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_keyboard_recovery_stage_label(stage: u32) -> &'static str {
        match stage as u8 {
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_NONE => "none",
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_BEGIN => "begin",
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_NOT_READY => "not-ready",
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_LIMIT => "limit",
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_STOP_ENDPOINT => {
                "stop-endpoint"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_RESET_ENDPOINT => {
                "reset-endpoint"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_RESET_RING => "reset-ring",
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_SET_DEQUEUE => "set-dequeue",
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_REARM => "rearm",
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_READY => "ready",
            _ => "unknown",
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_keyboard_recovery_reason_label(reason: u32) -> &'static str {
        match reason as u8 {
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_NONE => "none",
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_REENUMERATION_LIMIT => {
                "reenumeration-limit"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_QUEUE_COLLAPSE => {
                "queue-collapse"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_FULL_QUEUE_NO_EVENT => {
                "full-queue-no-event"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_STEADY_IDLE => {
                "steady-idle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_OVERQUEUE => "overqueue",
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_PRE_FIRST_UNDERFILLED => {
                "pre-first-underfilled"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_AUX_UNDERFILLED => {
                "aux-underfilled"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_STEADY_UNMATCHED => {
                "steady-unmatched"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_AUX_UNMATCHED => {
                "aux-unmatched"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_HARD_UNMATCHED => {
                "hard-unmatched"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_REARM_COLLAPSE => {
                "rearm-collapse"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_MATCHED_TRANSFER_FAULT => {
                "matched-transfer-fault"
            }
            _ => "unknown",
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_keyboard_report_status_label(status: u32) -> &'static str {
        match status as u8 {
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NONE => "none",
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_SHORT => "short-payload",
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE => "idle-report",
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_DECODED_EMPTY => {
                "decoded-empty"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_DECODE_FAILED => {
                "decode-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_FLEXIBLE_FALLBACK => {
                "flexible-fallback"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_PRODUCED_BYTE => {
                "produced-byte"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_FILTERED_KEY => {
                "filtered-key"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_UNMATCHED_TRANSFER => {
                "unmatched-transfer"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_QUEUE_COLLAPSE => {
                "queue-collapse"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_SUCCESS => {
                "recovery-success"
            }
            pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_FAILED => {
                "recovery-failed"
            }
            _ => "unknown",
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_keyboard_input_observation(
        first_byte: bool,
        parser_ingress: bool,
        queue_valid: bool,
        queued_reports: u32,
        doorbell_pending: bool,
        report_status: u32,
    ) -> &'static str {
        if first_byte {
            "byte-produced"
        } else if parser_ingress {
            "parser-ingress-without-linked-byte"
        } else if !queue_valid {
            "queue-telemetry-unavailable"
        } else if queued_reports == 0 {
            "interrupt-queue-empty"
        } else if doorbell_pending {
            "doorbell-pending"
        } else {
            match report_status as u8 {
                pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE => {
                    "idle-report-no-key-byte"
                }
                pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_SHORT => "short-payload",
                pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_DECODE_FAILED => {
                    "decode-failed"
                }
                pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_UNMATCHED_TRANSFER => {
                    "unmatched-transfer"
                }
                pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_QUEUE_COLLAPSE => {
                    "queue-collapse"
                }
                pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_SUCCESS => {
                    "recovery-success"
                }
                pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_FAILED => {
                    "recovery-failed"
                }
                pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NONE => {
                    "armed-awaiting-report"
                }
                _ => "awaiting-physical-key",
            }
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_sustained_input_blocker(
        queue_valid: bool,
        queued_reports: u32,
        transfer_events: u32,
        report_status: u32,
        no_replies: u64,
        runtime_skipped: u64,
    ) -> &'static str {
        if !queue_valid {
            "queue-telemetry-unavailable"
        } else if report_status
            == pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_FAILED as u32
        {
            "usb-post-first-byte-recovery-failed"
        } else if report_status
            == pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_QUEUE_COLLAPSE as u32
        {
            "usb-post-first-byte-queue-collapse"
        } else if report_status
            == pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_UNMATCHED_TRANSFER as u32
        {
            "usb-post-first-byte-unmatched-transfer"
        } else if queued_reports == 0 {
            "usb-post-first-byte-queue-empty"
        } else if queued_reports <= 8 && transfer_events >= 32 {
            "usb-post-first-byte-queue-collapse-risk"
        } else if no_replies != 0 {
            "usb-post-first-byte-no-reply"
        } else if runtime_skipped != 0 {
            "usb-post-first-byte-runtime-skipped"
        } else {
            "none"
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_runtime_detail_has_queue_result(detail: u16) -> bool {
        matches!(
            detail,
            pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING
                | pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY
        )
    }

    const fn usb_runtime_progress_superseded_by_keyboard(
        first_byte: bool,
        proof_gate: u8,
        progress_gate: u8,
    ) -> bool {
        first_byte && progress_gate < proof_gate
    }

    #[cfg(feature = "kernel")]
    fn usb_runtime_blocker_holds_current_gate(blocker: &str) -> bool {
        matches!(
            blocker,
            "usb-xhci-command-ring-proof-no-reply"
                | "usb-xhci-command-proof-submit-no-reply"
                | "usb-xhci-command-proof-doorbell-no-reply"
                | "usb-xhci-command-doorbell-publish-no-reply"
                | "enable-slot-completion-pending"
                | "enable-slot-completion-poll-no-reply"
                | "enable-slot-event-dma-load-done-no-reply"
                | "enable-slot-event-invalidate-done-no-reply"
                | "enable-slot-event-peek-no-reply"
                | "enable-slot-event-read-begin-no-reply"
                | "enable-slot-event-read-done-no-reply"
                | "enable-slot-event-slot-empty"
                | "enable-slot-event-cycle-mismatch"
                | "enable-slot-failed"
                | "enable-slot-poll-leading-port-status"
                | "enable-slot-command-event-seen"
                | "enable-slot-poll-non-command-event"
                | "enable-slot-event-ack-pending"
                | "enable-slot-event-ack-complete"
                | "root-port-reset-no-reply"
                | "root-port-connect-no-reply"
                | "root-port-connect-timeout"
                | "root-port-reset-completion-no-reply"
                | "root-port-enable-no-reply"
                | "root-port-enable-timeout"
                | "root-port-reset-timeout"
                | "root-port-reset-retry"
                | "root-port-reset-failed"
                | "root-port-stale-cleanup-no-reply"
                | "root-port-stale-cleanup-failed"
                | "address-enable-slot-no-reply"
                | "address-device-context-publish-no-reply"
                | "address-device-command-submit-no-reply"
                | "address-device-command-completion-no-reply"
                | "address-device-command-event-completion-no-reply"
                | "address-device-command-event-cycle-mismatch"
                | "address-device-command-event-other"
                | "address-device-command-event-port-status"
                | "address-device-command-event-slot-empty"
                | "address-device-publish-no-reply"
                | "device-descriptor-no-reply"
                | "device-descriptor-submit-no-reply"
                | "device-descriptor-transfer-no-reply"
                | "device-descriptor-status-no-reply"
                | "device-descriptor-transfer-failed"
                | "device-descriptor-transfer-timeout"
                | "device-descriptor-status-timeout"
                | "device-descriptor-prime-submit-no-reply"
                | "device-descriptor-prime-transfer-no-reply"
                | "device-descriptor-prime-status-no-reply"
                | "device-descriptor-full-read-no-reply"
                | "device-descriptor-prime-transfer-failed"
                | "device-descriptor-prime-transfer-timeout"
                | "device-descriptor-prime-status-timeout"
                | "config-descriptor-no-reply"
                | "config-descriptor-header-submit-no-reply"
                | "config-descriptor-header-transfer-no-reply"
                | "config-descriptor-header-status-no-reply"
                | "config-descriptor-header-transfer-failed"
                | "config-descriptor-header-transfer-timeout"
                | "config-descriptor-header-status-timeout"
                | "config-descriptor-header-transfer-event-slot-empty"
                | "config-descriptor-header-transfer-event-cycle-mismatch"
                | "config-descriptor-header-transfer-event-ignored"
                | "config-descriptor-header-status-event-slot-empty"
                | "config-descriptor-header-status-event-cycle-mismatch"
                | "config-descriptor-header-status-event-ignored"
                | "config-descriptor-full-submit-no-reply"
                | "config-descriptor-full-transfer-no-reply"
                | "config-descriptor-full-status-no-reply"
                | "config-descriptor-full-transfer-failed"
                | "config-descriptor-full-transfer-timeout"
                | "config-descriptor-full-status-timeout"
                | "config-descriptor-full-transfer-event-slot-empty"
                | "config-descriptor-full-transfer-event-cycle-mismatch"
                | "config-descriptor-full-transfer-event-ignored"
                | "config-descriptor-full-status-event-slot-empty"
                | "config-descriptor-full-status-event-cycle-mismatch"
                | "config-descriptor-full-status-event-ignored"
                | "hid-endpoint-not-ready"
                | "hid-endpoint-parse-no-reply"
                | "hid-endpoint-not-found"
                | "hid-interface-not-found"
                | "hid-interrupt-in-not-found"
                | "hid-config-descriptor-malformed"
                | "hid-configure-endpoint-no-reply"
                | "hid-configure-endpoint-failed"
                | "hid-set-configuration-no-reply"
                | "hid-set-configuration-failed"
                | "hid-control-no-reply"
                | "hid-control-failed"
                | "hid-interrupt-queue-no-reply"
                | "hid-interrupt-queue-failed"
                | "hub-child-scan-no-reply"
                | "hub-set-configuration-no-reply"
                | "hub-set-configuration-status-no-reply"
                | "hub-set-configuration-complete-no-reply"
                | "hub-set-configuration-status-event-slot-empty"
                | "hub-set-configuration-status-event-cycle-mismatch"
                | "hub-set-configuration-status-event-ignored"
                | "hub-set-configuration-status-timeout"
                | "hub-set-configuration-failed"
                | "hub-set-configuration-settle-no-reply"
                | "hub-descriptor-no-reply"
                | "hub-descriptor-transfer-no-reply"
                | "hub-descriptor-status-no-reply"
                | "hub-descriptor-transfer-failed"
                | "hub-descriptor-transfer-timeout"
                | "hub-descriptor-status-timeout"
                | "hub-descriptor-transfer-event-slot-empty"
                | "hub-descriptor-transfer-event-cycle-mismatch"
                | "hub-descriptor-transfer-event-ignored"
                | "hub-descriptor-status-event-slot-empty"
                | "hub-descriptor-status-event-cycle-mismatch"
                | "hub-descriptor-status-event-ignored"
                | "hub-context-no-reply"
                | "hub-port-power-no-reply"
                | "hub-port-status-no-reply"
                | "hub-port-status-transfer-no-reply"
                | "hub-port-status-status-no-reply"
                | "hub-port-status-transfer-timeout"
                | "hub-port-status-timeout"
                | "hub-port-status-transfer-event-slot-empty"
                | "hub-port-status-transfer-event-cycle-mismatch"
                | "hub-port-status-transfer-event-ignored"
                | "hub-port-status-status-event-slot-empty"
                | "hub-port-status-status-event-cycle-mismatch"
                | "hub-port-status-status-event-ignored"
                | "hub-port-status-failed"
                | "hub-port-reset-no-reply"
                | "hub-port-reset-set-no-reply"
                | "hub-port-reset-completion-no-reply"
                | "hub-port-reset-set-failed"
                | "hub-child-probe-no-reply"
                | "hub-child-speed-fallback-no-reply"
                | "hub-topology-no-keyboard"
        )
    }

    #[cfg(feature = "kernel")]
    fn emit_usb_startup_blackbox(&mut self, backend_attached: bool, polling_enabled: bool) {
        self.emit_console_line("usb: diag recorder=startup-blackbox mode=passive source=cached");
        #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
        {
            let linked_controller_ready =
                crate::local_seat::linked_local_seat_usb_controller_ready();
            let linked_detail = crate::local_seat::linked_local_seat_usb_runtime_detail();
            let linked_result = crate::local_seat::linked_local_seat_usb_runtime_result();
            let linked_progress = crate::hal::driver_task::latest_driver_task_ring_progress(
                crate::hal::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            );
            let linked_detail_gate = Self::usb_runtime_gate_for_linked_detail(linked_detail);
            let linked_progress_gate = linked_progress.map_or(0, |progress| {
                Self::usb_runtime_gate_for_progress_phase(progress.phase)
            });
            let linked_progress_blocker = linked_progress.map_or("none", |progress| {
                Self::usb_runtime_blocker_for_progress_phase(progress.phase)
            });
            let linked_progress_next_action = linked_progress
                .map_or("wait-linked-runtime-init", |progress| {
                    Self::usb_runtime_next_action_for_progress_phase(progress.phase)
                });
            let progress_refines_linked_detail = Self::usb_runtime_progress_refines_linked_detail(
                linked_detail,
                linked_detail_gate,
                linked_progress_gate,
            );
            let linked_gate = linked_detail_gate.max(linked_progress_gate);
            let linked_keyboard_ready = crate::local_seat::linked_local_seat_usb_keyboard_ready();
            let linked_first_report = crate::local_seat::linked_local_seat_usb_first_report_ready();
            let linked_first_byte = crate::local_seat::linked_local_seat_usb_first_byte_ready();
            let local_trace = self
                .local_seat
                .as_ref()
                .map(|local_seat| local_seat.keyboard_trace())
                .unwrap_or_default();
            let keyboard_ready = linked_keyboard_ready;
            let first_report = linked_first_report;
            let parser_ingress = local_trace.backend_read_bytes != 0
                || local_trace.accepted_bytes != 0
                || local_trace.echoed_bytes != 0;
            let first_byte = linked_first_byte;
            let (
                queued_reports,
                doorbell_pending,
                _preserved_events,
                _transfer_events,
                report_status,
            ) = Self::usb_runtime_queue_fields(linked_result);
            let queue_valid = Self::usb_runtime_detail_has_queue_result(linked_detail);
            let input_observation = Self::usb_runtime_keyboard_input_observation(
                linked_first_byte,
                parser_ingress,
                queue_valid,
                queued_reports,
                doorbell_pending,
                report_status,
            );
            let mut proof_gate = linked_gate;
            if linked_controller_ready {
                proof_gate = proof_gate.max(3);
            }
            if keyboard_ready {
                proof_gate = proof_gate.max(8);
            }
            if first_report {
                proof_gate = proof_gate.max(9);
            }
            if first_byte {
                proof_gate = proof_gate.max(10);
            }
            let active_blocker = if proof_gate >= 10 {
                "none"
            } else if keyboard_ready && !first_report {
                "hid-first-report"
            } else if first_report && !first_byte {
                "keyboard-first-byte"
            } else if progress_refines_linked_detail {
                linked_progress_blocker
            } else if linked_detail != 0 {
                Self::usb_runtime_blocker_for_linked_detail(linked_detail)
            } else if let Some(progress) = linked_progress {
                Self::usb_runtime_blocker_for_progress_phase(progress.phase)
            } else {
                "linked-runtime-no-detail"
            };
            let failing_gate = if proof_gate >= 10 {
                0
            } else if Self::usb_runtime_blocker_holds_current_gate(active_blocker) {
                proof_gate.max(1)
            } else {
                proof_gate.saturating_add(1).max(1)
            };
            let queue_result = Self::usb_runtime_detail_has_queue_result(linked_detail);
            let (
                queued_reports,
                doorbell_pending,
                preserved_events,
                transfer_events,
                report_status,
            ) = if queue_result {
                Self::usb_runtime_queue_fields(linked_result)
            } else {
                (0, false, 0, 0, 0)
            };
            let next_action = if proof_gate >= 10 {
                "acceptance-complete"
            } else if keyboard_ready && !first_report {
                "inspect-xhci-event-ring-interrupt-delivery"
            } else if first_report && !first_byte {
                "inspect-hid-report-to-console-byte-path"
            } else if progress_refines_linked_detail {
                linked_progress_next_action
            } else if linked_detail != 0 {
                Self::usb_runtime_next_action_for_linked_detail(linked_detail)
            } else if let Some(progress) = linked_progress {
                Self::usb_runtime_next_action_for_progress_phase(progress.phase)
            } else {
                "wait-linked-runtime-init"
            };
            if let Some(progress) = linked_progress {
                let progress_line = format_message(format_args!(
                    "usb: diag linked_runtime_progress marker_valid={} sequence={} phase={} phase_name={} aux0=0x{:08x} gate={} blocker={} next_action={}",
                    Self::yes_no(progress.marker_valid),
                    progress.sequence,
                    progress.phase,
                    progress.phase_name,
                    progress.aux0,
                    Self::usb_runtime_gate_for_progress_phase(progress.phase),
                    Self::usb_runtime_blocker_for_progress_phase(progress.phase),
                    Self::usb_runtime_next_action_for_progress_phase(progress.phase),
                ));
                self.emit_console_line(progress_line.as_str());
            }
            let gate1_evidence = if failing_gate == 1 && linked_detail == 0 {
                if let Some(progress) = linked_progress {
                    format_message(format_args!(
                        "linked_runtime_phase={} phase_name={} blocker={} linked_controller={} detail=0x{:04x}",
                        progress.phase,
                        progress.phase_name,
                        Self::usb_runtime_blocker_for_progress_phase(progress.phase),
                        Self::yes_no(linked_controller_ready),
                        linked_detail,
                    ))
                } else {
                    format_message(format_args!(
                        "hardware_owner=linked-runtime root_action=admission-descriptor-diagnostics linked_controller={} detail=0x{:04x}",
                        Self::yes_no(linked_controller_ready),
                        linked_detail,
                    ))
                }
            } else {
                format_message(format_args!(
                    "hardware_owner=linked-runtime root_action=admission-descriptor-diagnostics linked_controller={} detail=0x{:04x}",
                    Self::yes_no(linked_controller_ready),
                    linked_detail,
                ))
            };

            self.emit_usb_gate_line(
                1,
                "hal-resources",
                Self::usb_startup_gate_status(1, proof_gate, failing_gate),
                format_args!("{}", gate1_evidence.as_str()),
                "pcie-vl805",
            );
            self.emit_usb_gate_line(
                2,
                "pcie-vl805",
                Self::usb_startup_gate_status(2, proof_gate, failing_gate),
                format_args!(
                    "backend_attached={} linked_controller={} runtime_result=0x{:08x}",
                    Self::yes_no(backend_attached),
                    Self::yes_no(linked_controller_ready),
                    linked_result,
                ),
                "xhci-operational",
            );
            self.emit_usb_gate_line(
                3,
                "xhci-operational",
                Self::usb_startup_gate_status(3, proof_gate, failing_gate),
                format_args!(
                    "linked_detail=0x{:04x} linked_gate={}",
                    linked_detail, linked_gate,
                ),
                "command-event-rings",
            );
            self.emit_usb_gate_line(
                4,
                "command-event-rings",
                Self::usb_startup_gate_status(4, proof_gate, failing_gate),
                format_args!(
                    "blocker={} linked_detail=0x{:04x} queue_result={} queued_reports={} doorbell={} preserved_events={} transfer_events={}",
                    active_blocker,
                    linked_detail,
                    if queue_result { "yes" } else { "no" },
                    queued_reports,
                    Self::yes_no(doorbell_pending),
                    preserved_events,
                    transfer_events,
                ),
                "root-port-connected",
            );
            self.emit_usb_gate_line(
                5,
                "root-port-connected",
                Self::usb_startup_gate_status(5, proof_gate, failing_gate),
                format_args!(
                    "linked_detail=0x{:04x} result=0x{:08x}",
                    linked_detail, linked_result,
                ),
                "device-addressed",
            );
            self.emit_usb_gate_line(
                6,
                "device-addressed",
                Self::usb_startup_gate_status(6, proof_gate, failing_gate),
                format_args!(
                    "linked_detail=0x{:04x} progress_phase={} progress_phase_name={} progress_blocker={}",
                    linked_detail,
                    linked_progress.map_or(0, |progress| progress.phase),
                    linked_progress.map_or("none", |progress| progress.phase_name),
                    linked_progress.map_or("none", |progress| {
                        Self::usb_runtime_blocker_for_progress_phase(progress.phase)
                    }),
                ),
                "config-and-hid-descriptors",
            );
            self.emit_usb_gate_line(
                7,
                "config-and-hid-descriptors",
                Self::usb_startup_gate_status(7, proof_gate, failing_gate),
                format_args!(
                    "linked_detail=0x{:04x} progress_phase={} progress_phase_name={} progress_blocker={}",
                    linked_detail,
                    linked_progress.map_or(0, |progress| progress.phase),
                    linked_progress.map_or("none", |progress| progress.phase_name),
                    linked_progress.map_or("none", |progress| {
                        Self::usb_runtime_blocker_for_progress_phase(progress.phase)
                    }),
                ),
                "keyboard-ready",
            );
            self.emit_usb_gate_line(
                8,
                "keyboard-ready",
                Self::usb_startup_gate_status(8, proof_gate, failing_gate),
                format_args!(
                    "linked={} polling={}",
                    Self::yes_no(linked_keyboard_ready),
                    if polling_enabled {
                        "enabled"
                    } else {
                        "deferred"
                    },
                ),
                "first-hid-report",
            );
            self.emit_usb_gate_line(
                9,
                "first-hid-report",
                Self::usb_startup_gate_status(9, proof_gate, failing_gate),
                format_args!(
                    "first_report={} queued_reports={} doorbell={} transfer_events={} report_status={}",
                    Self::yes_no(first_report),
                    queued_reports,
                    Self::yes_no(doorbell_pending),
                    transfer_events,
                    Self::usb_runtime_keyboard_report_status_label(report_status),
                ),
                "first-console-byte",
            );
            self.emit_usb_gate_line(
                10,
                "first-console-byte",
                Self::usb_startup_gate_status(10, proof_gate, failing_gate),
                format_args!(
                    "first_byte={} first_byte_source={} parser_ingress={} backend_bytes={} accepted={} echoed={} input_observation={}",
                    Self::yes_no(first_byte),
                    if first_byte {
                        "linked-runtime-hid"
                    } else if parser_ingress {
                        "local-seat-queue-diagnostic"
                    } else {
                        "none"
                    },
                    Self::yes_no(parser_ingress),
                    local_trace.backend_read_bytes,
                    local_trace.accepted_bytes,
                    local_trace.echoed_bytes,
                    input_observation,
                ),
                "acceptance-complete",
            );
            let interrupt_in_proven = first_report
                || keyboard_ready
                || linked_progress.is_some_and(|progress| {
                    progress.phase
                        == pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_READY
                });
            let evidence = format_message(format_args!(
                "usb: evidence xhci queue_result={} transfer_ring_queued={} doorbell={} preserved_events={} transfer_events={} endpoint={} first_report_policy=deep-queue-rering-doorbell cerr=3 max_packet=runtime-private interval=runtime-private source=linked-runtime-result",
                if queue_result { "yes" } else { "no" },
                queued_reports,
                Self::yes_no(doorbell_pending),
                preserved_events,
                transfer_events,
                if interrupt_in_proven {
                    "interrupt-in"
                } else {
                    "unproven"
                },
            ));
            self.emit_console_line(evidence.as_str());
            let boundary = format_message(format_args!(
                "usb: evidence boundary console_client=event-pump hal=admission-descriptor-diagnostics-only linked_runtime_owner=usb-local-seat failure_domain={} proof_gate={} target_gate=10 proof_effect={}",
                active_blocker,
                proof_gate,
                if proof_gate >= 10 {
                    "acceptance-green"
                } else {
                    "acceptance-red"
                },
            ));
            self.emit_console_line(boundary.as_str());
            let next = format_message(format_args!(
                "usb: next_action={} blocker={} proof_gate={} target_gate=10 detail=0x{:04x} result=0x{:08x} source=linked-runtime",
                next_action, active_blocker, proof_gate, linked_detail, linked_result,
            ));
            self.emit_console_line(next.as_str());
        }
        #[cfg(not(all(feature = "usb", target_arch = "aarch64", target_os = "none")))]
        {
            let status = if backend_attached { "pass" } else { "unknown" };
            self.emit_usb_gate_line(
                1,
                "hal-resources",
                status,
                format_args!(
                    "backend_attached={} polling={} target=non-pi4-runtime",
                    Self::yes_no(backend_attached),
                    if polling_enabled {
                        "enabled"
                    } else {
                        "deferred"
                    },
                ),
                "boot-pi4-linked-runtime",
            );
            for gate in 2..=10 {
                self.emit_usb_gate_line(
                    gate,
                    Self::usb_startup_gate_name(gate),
                    "unknown",
                    format_args!("evidence=requires-aarch64-none-usb-runtime"),
                    "boot-pi4-linked-runtime",
                );
            }
            self.emit_console_line(
                "usb: evidence boundary console_client=event-pump hal=unavailable linked_runtime_owner=unavailable failure_domain=host-test-profile proof_gate=0 target_gate=10 proof_effect=host-profile-unproven",
            );
            self.emit_console_line(
                "usb: next_action=boot-pi4-linked-runtime blocker=host-test-profile proof_gate=0 target_gate=10 detail=0x0000 result=0x00000000 source=host-profile",
            );
        }
    }

    #[cfg(feature = "kernel")]
    fn emit_usb_gate_line(
        &mut self,
        gate: u8,
        name: &'static str,
        status: &'static str,
        evidence: fmt::Arguments<'_>,
        next: &'static str,
    ) {
        let mut line = format_message(format_args!(
            "usb: gate {} name={} status={} evidence=",
            gate, name, status
        ));
        if FmtWrite::write_fmt(&mut line, evidence).is_err() {
            let _ = write!(line, "truncated");
        }
        let _ = write!(line, " next={next}");
        self.emit_console_line(line.as_str());
    }

    #[cfg(feature = "kernel")]
    const fn usb_startup_gate_status(gate: u8, proof_gate: u8, failing_gate: u8) -> &'static str {
        if failing_gate != 0 {
            if gate < failing_gate {
                "pass"
            } else if gate == failing_gate {
                "fail"
            } else {
                "blocked"
            }
        } else if gate <= proof_gate {
            "pass"
        } else {
            "blocked"
        }
    }

    #[cfg(feature = "kernel")]
    const fn usb_startup_gate_name(gate: u8) -> &'static str {
        match gate {
            1 => "hal-resources",
            2 => "pcie-vl805",
            3 => "xhci-operational",
            4 => "command-event-rings",
            5 => "root-port-connected",
            6 => "device-addressed",
            7 => "config-and-hid-descriptors",
            8 => "keyboard-ready",
            9 => "first-hid-report",
            10 => "first-console-byte",
            _ => "unknown",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_debug_command_profile(command: WifiDebugCommand) -> &'static str {
        match command {
            WifiDebugCommand::Help => "help",
            WifiDebugCommand::DumpState | WifiDebugCommand::ProbeHt | WifiDebugCommand::Diag => {
                "bounded"
            }
            WifiDebugCommand::LoadFirmware | WifiDebugCommand::Retry => "stateful",
        }
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_debug_status(
        &mut self,
        subcommand: &str,
        action: &str,
        profile: &str,
        detail: Option<&str>,
    ) {
        let mut line = format_message(format_args!(
            "wifi: debug subcommand={subcommand} action={action} profile={profile} mode=one-shot"
        ));
        if let Some(detail) = detail {
            let _ = write!(line, " {detail}");
        }
        self.emit_console_line(line.as_str());
    }

    #[cfg(feature = "kernel")]
    fn wifi_error_is_driver_task_runtime_required(err: &crate::hal::HalError) -> bool {
        matches!(
            err,
            crate::hal::HalError::Unsupported("pi4-wifi-driver-task-runtime-required")
        )
    }

    #[cfg(feature = "kernel")]
    const fn wifi_command_supports_driver_task_snapshot(command: WifiDebugCommand) -> bool {
        matches!(
            command,
            WifiDebugCommand::DumpState
                | WifiDebugCommand::Diag
                | WifiDebugCommand::ProbeHt
                | WifiDebugCommand::LoadFirmware
                | WifiDebugCommand::Retry
        )
    }

    #[cfg(feature = "kernel")]
    const fn wifi_command_accepts_driver_task_snapshot_success(command: WifiDebugCommand) -> bool {
        matches!(
            command,
            WifiDebugCommand::DumpState | WifiDebugCommand::Diag
        )
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_driver_task_runtime_snapshot_if_present(
        &mut self,
        command: WifiDebugCommand,
        subcommand: &str,
        profile: &str,
        source: &str,
    ) -> bool {
        if !Self::wifi_command_supports_driver_task_snapshot(command) {
            return false;
        }
        let live_net_frontier = self.wifi_live_net_frontier();
        let live_net_supersedes_runtime = live_net_frontier.is_some();
        let host_eapol_exact = self
            .net_unavailable_detail
            .as_ref()
            .and_then(|cause| Self::wifi_host_eapol_exact_from_cause(cause.as_str()));
        let host_eapol_exact =
            host_eapol_exact.or_else(|| self.wifi_host_eapol_exact_from_current_net_status());
        let fault = if live_net_supersedes_runtime || host_eapol_exact.is_some() {
            None
        } else {
            crate::drivers::driver_task_net::latest_cyw43_runtime_command_fault_status()
        };
        let sdio_status = crate::drivers::driver_task_net::latest_sdio_runtime_replay_status();
        let progress_present = Self::wifi_driver_task_runtime_progress_present();
        if source == "debug-handle-unavailable"
            && self.net_unavailable_detail.is_none()
            && host_eapol_exact.is_none()
            && fault.is_none()
            && sdio_status.is_none()
            && !progress_present
            && !live_net_supersedes_runtime
        {
            return false;
        }
        if self.net_unavailable_detail.is_none()
            && host_eapol_exact.is_none()
            && fault.is_none()
            && sdio_status.is_none()
            && !progress_present
            && !live_net_supersedes_runtime
        {
            return false;
        }

        if let Some(frontier) = live_net_frontier.as_ref() {
            let detail = format_message(format_args!(
                "wifi: driver-task replay state detail=live-net-frontier source={source} active={} address_source={} dhcp_phase={} ip={} assoc={} link={} eapol_secure={}",
                frontier.active_interface,
                frontier.address_source,
                frontier.dhcp_phase,
                frontier.ip,
                frontier.wifi_assoc,
                frontier.wifi_link_up,
                frontier.wifi_host_eapol_secure,
            ));
            self.emit_console_line(detail.as_str());
        } else if let Some(cause) = self.net_unavailable_detail.as_ref() {
            let detail = format_message(format_args!(
                "wifi: driver-task replay failure detail=net-disabled cause={cause}"
            ));
            self.emit_console_line(detail.as_str());
        } else {
            let detail = format_message(format_args!(
                "wifi: driver-task replay failure detail=net-state-unavailable source={source}"
            ));
            self.emit_console_line(detail.as_str());
        }
        let evidence_source = if live_net_supersedes_runtime {
            "live-net-status"
        } else {
            source
        };
        self.emit_wifi_driver_task_startup_blackbox(fault, host_eapol_exact, evidence_source);
        if let Some(fault) = fault {
            let fault_line = format_message(format_args!(
                "wifi: cyw43 fault stage={} op={} target=0x{:08x} payload_off={} payload_len={} total_len={} control_cmd={} control_cmd_hex=0x{:08x} control_id={} control_header_mode={} control_response_len={} detail=0x{:04x} reason={} result=0x{:08x}",
                fault.stage,
                fault.op,
                fault.target_addr,
                fault.payload_offset,
                fault.payload_len,
                fault.total_len,
                fault.control_cmd,
                fault.control_cmd,
                fault.control_id,
                fault.control_header_mode,
                fault.control_response_len,
                fault.detail,
                fault.reason,
                fault.result,
            ));
            self.emit_console_line(fault_line.as_str());
            let next = Self::wifi_runtime_fault_next_action(fault);
            let next_line = format_message(format_args!(
                "wifi: linked_runtime next_action={} source={} recovery_contract=block-primary+progress-bounded-owner-replay",
                next, source
            ));
            self.emit_console_line(next_line.as_str());
        }
        if live_net_supersedes_runtime
            || Self::wifi_command_accepts_driver_task_snapshot_success(command)
        {
            let completion_source = if live_net_supersedes_runtime {
                "live-net-status"
            } else {
                "linked-runtime-replay-failure"
            };
            let status_detail =
                format_message(format_args!("result=ok source={completion_source}"));
            self.emit_wifi_debug_status(
                subcommand,
                "complete",
                profile,
                Some(status_detail.as_str()),
            );
            self.metrics.accepted_commands = self.metrics.accepted_commands.saturating_add(1);
            let detail = format_message(format_args!(
                "detail=subcommand={subcommand} scope=serial-local source={completion_source}"
            ));
            self.emit_ack_ok(WIFI_DEBUG_ACK_LABEL, Some(detail.as_str()));
        } else {
            self.emit_wifi_debug_status(
                subcommand,
                "complete",
                profile,
                Some(
                    "result=err source=linked-runtime-replay-failure error=pi4-wifi-driver-task-runtime-required",
                ),
            );
            self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
            let detail = format_message(format_args!(
                "detail=subcommand={subcommand} error=pi4-wifi-driver-task-runtime-required source=linked-runtime-replay-failure"
            ));
            self.emit_refusal(
                WIFI_DEBUG_ACK_LABEL,
                RefusalReason::Policy,
                Some(detail.as_str()),
            );
        }
        true
    }

    #[cfg(feature = "kernel")]
    fn wifi_host_eapol_exact_from_cause(cause: &str) -> Option<&'static str> {
        if cause.contains("wifi-host-eapol-pending") || cause.contains("host-eapol-pending") {
            Some("wifi-host-eapol-pending")
        } else if cause.contains("wifi-host-eapol-required")
            || cause.contains("host-eapol-required")
        {
            Some("host-eapol-required")
        } else {
            None
        }
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    fn wifi_host_eapol_exact_from_status(status: &NetStatusReport) -> Option<&'static str> {
        Self::wifi_host_eapol_exact_from_cause(status.address_source)
            .or_else(|| Self::wifi_host_eapol_exact_from_cause(status.dhcp_phase))
    }

    #[cfg(feature = "kernel")]
    fn wifi_host_eapol_exact_from_current_net_status(&self) -> Option<&'static str> {
        #[cfg(feature = "net-console")]
        {
            self.net.as_ref().and_then(|net| {
                let status = net.status_report();
                Self::wifi_host_eapol_exact_from_status(&status)
            })
        }
        #[cfg(not(feature = "net-console"))]
        {
            None
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_live_net_frontier(&self) -> Option<WifiLiveNetFrontier> {
        #[cfg(feature = "net-console")]
        {
            self.net.as_ref().and_then(|net| {
                let status = net.status_report();
                let counters = net.stats();
                let frontier = WifiLiveNetFrontier {
                    active_interface: status.active_interface,
                    address_source: status.address_source,
                    dhcp_phase: status.dhcp_phase,
                    ip: status.ip.clone(),
                    wifi_assoc: counters.wifi_assoc,
                    wifi_link_up: counters.wifi_link_up,
                    wifi_host_eapol_secure: counters.wifi_host_eapol_secure,
                };
                Self::wifi_live_net_status_supersedes_runtime(&frontier).then_some(frontier)
            })
        }
        #[cfg(not(feature = "net-console"))]
        {
            None
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_live_net_status_supersedes_runtime(frontier: &WifiLiveNetFrontier) -> bool {
        if frontier.active_interface != "wifi" {
            return false;
        }
        let dhcp_frontier = matches!(
            frontier.address_source,
            "dhcp-pending" | "dhcp-lease" | "dhcp-failed"
        ) || matches!(frontier.dhcp_phase, "selecting" | "bound" | "failed");
        let secure_counters = frontier.wifi_assoc != 0
            && frontier.wifi_link_up != 0
            && frontier.wifi_host_eapol_secure != 0;
        dhcp_frontier || secure_counters
    }

    #[cfg(feature = "kernel")]
    fn wifi_live_net_dhcp_bound(frontier: &WifiLiveNetFrontier) -> bool {
        frontier.active_interface == "wifi"
            && frontier.address_source == "dhcp-lease"
            && frontier.dhcp_phase == "bound"
    }

    #[cfg(feature = "kernel")]
    fn wifi_live_net_blocker(frontier: &WifiLiveNetFrontier) -> &'static str {
        match (frontier.address_source, frontier.dhcp_phase) {
            ("dhcp-lease", "bound") => "nettest-netstats-cohsh",
            ("dhcp-failed", _) | (_, "failed") => "dhcp-failed",
            ("dhcp-pending", _) | (_, "selecting") => "dhcp-pending",
            _ => "dhcp-bound",
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_driver_task_runtime_progress_present() -> bool {
        crate::hal::driver_task::latest_driver_task_ring_progress(
            crate::hal::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
        )
        .is_some()
            || crate::hal::driver_task::latest_driver_task_ring_progress(
                crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            )
            .is_some()
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_snapshot_with_traces(&mut self, snapshot: &WifiDebugSnapshot) {
        self.emit_wifi_snapshot(snapshot);
        let (firmware_trace, sdhci_trace, control_plane_trace) =
            if let Some(wifi_debug) = self.wifi_debug.as_mut() {
                (
                    wifi_debug.firmware_contract_trace(),
                    wifi_debug.sdhci_contract_trace(),
                    wifi_debug.control_plane_trace(),
                )
            } else {
                (None, None, None)
            };
        if let Some(trace) = firmware_trace {
            self.emit_wifi_firmware_contract(&trace);
        }
        if let Some(trace) = sdhci_trace {
            self.emit_wifi_sdhci_contract(&trace);
        }
        if let Some(trace) = control_plane_trace {
            self.emit_wifi_control_plane_trace(&trace);
        }
        self.emit_wifi_readiness_summary(snapshot, firmware_trace, control_plane_trace);
        #[cfg(feature = "net-console")]
        self.emit_wifi_network_status();
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_diag_summary(&mut self, snapshot: &WifiDebugSnapshot) {
        let (firmware_trace, control_plane_trace) =
            if let Some(wifi_debug) = self.wifi_debug.as_mut() {
                (
                    wifi_debug.firmware_contract_trace(),
                    wifi_debug.control_plane_trace(),
                )
            } else {
                (None, None)
            };
        self.emit_wifi_readiness_summary(snapshot, firmware_trace, control_plane_trace);
        self.emit_wifi_startup_blackbox(snapshot, firmware_trace, control_plane_trace);
        #[cfg(feature = "net-console")]
        self.emit_wifi_network_status();
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_snapshot(&mut self, snapshot: &WifiDebugSnapshot) {
        let headline = format_message(format_args!(
            "wifi: power={} reset={} card={} rca=0x{:04x} ocr=0x{:08x}",
            Self::wifi_power_label(snapshot.power_state),
            Self::wifi_reset_label(snapshot.reset_state),
            if snapshot.card_ready { "yes" } else { "no" },
            snapshot.card_rca,
            snapshot.card_ocr,
        ));
        self.emit_console_line(headline.as_str());

        let transport = format_message(format_args!(
            "wifi: clock={}Hz preferred={}Hz width={} ioex={} iordy={}",
            snapshot.current_clock_hz,
            snapshot.preferred_data_clock_hz,
            Self::wifi_bus_width_label(snapshot.bus_width),
            Self::format_optional_u8(snapshot.io_enable),
            Self::format_optional_u8(snapshot.io_ready),
        ));
        self.emit_console_line(transport.as_str());

        let shadow = format_message(format_args!(
            "wifi: chipclk={} wake={} sleep={} cardcap={} programmed={} shadow={} fn={}",
            Self::format_optional_u8(snapshot.chipclkcsr),
            Self::format_optional_u8(snapshot.wakeupctrl),
            Self::format_optional_u8(snapshot.sleepcsr),
            Self::format_optional_u8(snapshot.cardcap),
            Self::format_optional_u32(snapshot.programmed_backplane_window),
            Self::format_optional_u32(snapshot.shadow_backplane_window),
            Self::format_optional_fn_addr(snapshot.shadow_backplane_fn_addr),
        ));
        self.emit_console_line(shadow.as_str());

        let recovery = format_message(format_args!(
            "wifi: f2_recover stage={} policy={} op={} drained={} count={} nak_sent={} rearm_budget={} rearm_action={}",
            snapshot.control_plane_frame_recovery_stage.unwrap_or("n/a"),
            snapshot
                .control_plane_frame_recovery_policy
                .unwrap_or("n/a"),
            match snapshot.control_plane_frame_recovery_write {
                Some(true) => "write",
                Some(false) => "read",
                None => "n/a",
            },
            match snapshot.control_plane_frame_recovery_drained {
                Some(true) => "yes",
                Some(false) => "no",
                None => "n/a",
            },
            Self::format_optional_u16(snapshot.control_plane_frame_recovery_count),
            Self::wifi_reply_recovery_nak_sent(snapshot),
            Self::wifi_reply_rearm_budget(snapshot),
            Self::wifi_reply_rearm_action(snapshot),
        ));
        self.emit_console_line(recovery.as_str());

        let bootstrap = format_message(format_args!(
            "wifi: bootstrap={} no_ht={} probe_pending={} startup_link_stable={} safe_profile_locked={} safe_reason={} reply_mode={} reply_attempts={} empty_polls={} promoted_probe={}",
            snapshot.control_plane_bootstrap_phase,
            if snapshot.control_plane_no_ht_transport {
                "yes"
            } else {
                "no"
            },
            if snapshot.control_plane_probe_pending {
                "yes"
            } else {
                "no"
            },
            if snapshot.control_plane_startup_link_stable {
                "yes"
            } else {
                "no"
            },
            if snapshot.control_plane_startup_profile_locked {
                "yes"
            } else {
                "no"
            },
            snapshot.control_plane_startup_profile_reason,
            snapshot.control_plane_reply_mode,
            snapshot.control_plane_reply_attempts,
            snapshot.control_plane_reply_empty_polls,
            if snapshot.control_plane_promoted_probe_pending {
                "yes"
            } else {
                "no"
            },
        ));
        self.emit_console_line(bootstrap.as_str());
        let reply_probe = format_message(format_args!(
            "wifi: reply_probe lane={} effective_clock={}Hz",
            Self::wifi_reply_probe_lane(snapshot),
            Self::wifi_reply_probe_effective_clock_hz(snapshot),
        ));
        self.emit_console_line(reply_probe.as_str());
        let reply_terminal = format_message(format_args!(
            "wifi: reply_terminal action={} retry_clock={}Hz",
            Self::wifi_reply_terminal_action(snapshot),
            Self::wifi_reply_retry_clock_hz(snapshot),
        ));
        self.emit_console_line(reply_terminal.as_str());
        let replay_full_bootstrap = if snapshot.control_plane_exact_error.is_empty() {
            "n/a"
        } else if crate::net::cyw43_control_plane_bootstrap_replay_reason(
            snapshot.control_plane_exact_error,
        ) {
            "yes"
        } else {
            "no"
        };
        let snapshot_meta = format_message(format_args!(
            "wifi: snapshot source={} stage={} rescue={}/{} passive_limit={} replay_full_bootstrap={}",
            snapshot.debug_snapshot_source,
            snapshot.debug_snapshot_stage,
            snapshot.control_plane_startup_link_rescue_cycles,
            snapshot.control_plane_startup_link_rescue_limit,
            snapshot.control_plane_passive_startup_link_empty_poll_limit,
            replay_full_bootstrap,
        ));
        self.emit_console_line(snapshot_meta.as_str());

        let (verdict, focus) = Self::wifi_capture_verdict(snapshot);
        let golden_route = format_message(format_args!(
            "wifi: golden_path route={} state={} transport={} current={} next={} focus={} verdict={}",
            Self::wifi_golden_path_route(snapshot),
            Self::wifi_golden_path_state(snapshot),
            if snapshot.control_plane_no_ht_transport {
                "bounded-no-ht"
            } else {
                "strict"
            },
            Self::wifi_golden_path_current_step(snapshot),
            Self::wifi_golden_path_next_step(snapshot),
            focus,
            verdict,
        ));
        self.emit_console_line(golden_route.as_str());
        let reply_contract = format_message(format_args!(
            "wifi: reply_contract path={} strict_recovery_f2={} blocker_class={}",
            Self::wifi_reply_contract_path(snapshot),
            Self::wifi_reply_contract_strict_recovery_f2(snapshot),
            Self::wifi_reply_contract_blocker_class(snapshot),
        ));
        self.emit_console_line(reply_contract.as_str());
        let wifi_contract = format_message(format_args!(
            "wifi: contract current={} expected={} observed={} blocker={} path={}",
            Self::wifi_golden_path_current_step(snapshot),
            Self::wifi_contract_expected(snapshot),
            Self::wifi_contract_observed(snapshot),
            Self::wifi_reply_contract_blocker_class(snapshot),
            Self::wifi_reply_contract_path(snapshot),
        ));
        self.emit_console_line(wifi_contract.as_str());

        let control = format_message(format_args!(
            "wifi: f2_state={} exact_error={} sdhci_read_diag={}",
            snapshot.control_plane_f2_state,
            snapshot.control_plane_exact_error,
            snapshot.control_plane_sdhci_read_diag,
        ));
        self.emit_console_line(control.as_str());

        if Self::wifi_exact_error_is_join_security_blocker(snapshot.control_plane_exact_error) {
            let attribution = format_message(format_args!(
                "wifi: attribution={} transport={} sdhci={} f2={} failing_iovar={} status={}",
                Self::wifi_join_security_attribution(snapshot),
                Self::wifi_join_security_transport_label(snapshot),
                snapshot.control_plane_sdhci_read_diag,
                snapshot.control_plane_f2_state,
                Self::wifi_join_security_failing_iovar(snapshot),
                Self::wifi_join_security_status(snapshot),
            ));
            self.emit_console_line(attribution.as_str());
        }

        let verdict_line = format_message(format_args!(
            "wifi: verdict={verdict} focus={focus} bootstrap={}",
            snapshot.control_plane_bootstrap_phase,
        ));
        self.emit_console_line(verdict_line.as_str());
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_firmware_contract(&mut self, trace: &WifiFirmwareContractTrace) {
        let line = format_message(format_args!(
            "wifi: firmware_contract fw={} nvram={} clm={} board={} rstvec={} verified={} armcr4_release={} sr_kso={} current_clock={}Hz preferred={}Hz",
            trace.firmware_len,
            trace.nvram_len,
            Self::format_optional_usize(trace.clm_len),
            trace.board_type,
            Self::format_optional_u32(trace.reset_vector),
            if trace.firmware_download_verified {
                "yes"
            } else {
                "no"
            },
            trace.armcr4_release_attempts,
            if trace.sr_kso_clock_ready {
                "yes"
            } else {
                "no"
            },
            trace.current_clock_hz,
            trace.preferred_data_clock_hz,
        ));
        self.emit_console_line(line.as_str());

        let requests = format_message(format_args!(
            "wifi: firmware_ht_req alp=0x{:02x} ht=0x{:02x} ht_retry=0x{:02x} force_ht_after_proof={}",
            trace.alp_request,
            trace.ht_request,
            trace.ht_retry_request,
            Self::format_optional_u8(trace.force_ht_after_proof_request),
        ));
        self.emit_console_line(requests.as_str());

        let registers = format_message(format_args!(
            "wifi: firmware_ht_state chipclk={} wake={} sleep={} kso={} devon={} cardcap={} f1={} f2={} blocker={} next={}",
            Self::format_optional_u8(trace.chipclkcsr),
            Self::format_optional_u8(trace.wakeupctrl),
            Self::format_optional_u8(trace.sleepcsr),
            Self::wifi_sleep_bit_label(trace.sleepcsr, WIFI_SLEEPCSR_KSO),
            Self::wifi_sleep_bit_label(trace.sleepcsr, WIFI_SLEEPCSR_DEVON),
            Self::format_optional_u8(trace.cardcap),
            trace.f1_state,
            trace.f2_state,
            trace.blocker,
            trace.next_step,
        ));
        self.emit_console_line(registers.as_str());

        match trace.proof {
            Some(proof) => {
                let proof_line = format_message(format_args!(
                    "wifi: firmware_proof source={} upload={} nvram_tail={} rstvec={} cpuhalt={}",
                    proof.source,
                    proof.upload_state,
                    proof.nvram_tail_state,
                    proof.reset_vector_state,
                    proof.cpuhalt_state,
                ));
                self.emit_console_line(proof_line.as_str());
                let proof_state = format_message(format_args!(
                    "wifi: firmware_proof_state readback={} precondition={} verified={} attempts={} upload_clock={}Hz",
                    proof.readback_status,
                    proof.precondition_state,
                    if proof.verified { "yes" } else { "no" },
                    proof.armcr4_release_attempts,
                    proof.upload_clock_hz,
                ));
                self.emit_console_line(proof_state.as_str());
            }
            None => {
                self.emit_console_line(
                    "wifi: firmware_proof source=none upload=n/a nvram_tail=n/a rstvec=n/a cpuhalt=n/a",
                );
                self.emit_console_line(
                    "wifi: firmware_proof_state readback=n/a precondition=n/a verified=n/a attempts=0 upload_clock=0Hz",
                );
            }
        }

        let ht_summary = format_message(format_args!(
            "wifi: ht_summary state={} records={} f2_gate={}",
            trace.ht_summary, trace.ht_phase_count, trace.function2_gate,
        ));
        self.emit_console_line(ht_summary.as_str());
        for (index, record) in trace
            .ht_phase_records
            .iter()
            .take(usize::from(trace.ht_phase_count))
            .enumerate()
        {
            let line = format_message(format_args!(
                "wifi: ht_record[{index}] stage={} status={} chipclk={} wake={} sleep={} kso={} devon={} cardcap={}",
                record.stage,
                record.status,
                Self::format_optional_u8(record.chipclkcsr),
                Self::format_optional_u8(record.wakeupctrl),
                Self::format_optional_u8(record.sleepcsr),
                Self::wifi_sleep_bit_label(record.sleepcsr, WIFI_SLEEPCSR_KSO),
                Self::wifi_sleep_bit_label(record.sleepcsr, WIFI_SLEEPCSR_DEVON),
                Self::format_optional_u8(record.cardcap),
            ));
            self.emit_console_line(line.as_str());
        }
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_sdhci_contract(&mut self, trace: &WifiSdhciContractTrace) {
        let contract = format_message(format_args!(
            "wifi: sdhci_contract current={} preserved={} resolved={}",
            trace.current_diag, trace.preserved_diag, trace.resolved_diag,
        ));
        self.emit_console_line(contract.as_str());

        let live = format_message(format_args!(
            "wifi: sdhci_live cmd={} arg={} ps={} stat={}",
            Self::format_optional_u16(trace.current_cmd),
            Self::format_optional_u32(trace.current_arg),
            Self::format_optional_u32(trace.current_present),
            Self::format_optional_u32(trace.current_int_status),
        ));
        self.emit_console_line(live.as_str());

        let preserved = format_message(format_args!(
            "wifi: sdhci_preserved cmd={} arg={} ps={} stat={}",
            Self::format_optional_u16(trace.preserved_cmd),
            Self::format_optional_u32(trace.preserved_arg),
            Self::format_optional_u32(trace.preserved_present),
            Self::format_optional_u32(trace.preserved_int_status),
        ));
        self.emit_console_line(preserved.as_str());
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_control_plane_trace(&mut self, trace: &WifiControlPlaneTrace) {
        let cccr = format_message(format_args!(
            "wifi: cccr ioex={} iordy={} ien={} rframe_lo={} rframe_hi={} watermark={} devctl={} mesbusy={}",
            Self::format_optional_u8(trace.cccr_io_enable),
            Self::format_optional_u8(trace.cccr_io_ready),
            Self::format_optional_u8(trace.cccr_int_enable),
            Self::format_optional_u8(trace.f1_rframe_lo),
            Self::format_optional_u8(trace.f1_rframe_hi),
            Self::format_optional_u8(trace.f1_watermark),
            Self::format_optional_u8(trace.f1_device_ctl),
            Self::format_optional_u8(trace.f1_mesbusyctl),
        ));
        self.emit_console_line(cccr.as_str());

        let shadow = format_message(format_args!(
            "wifi: sdio_shadow block_size_count=0x{:08x} transfer_mode=0x{:08x} backplane_bytes={:02x}:{:02x}:{:02x}",
            trace.block_size_shadow,
            trace.transfer_mode_shadow,
            trace.backplane_window_low,
            trace.backplane_window_mid,
            trace.backplane_window_high,
        ));
        self.emit_console_line(shadow.as_str());

        let preserved = format_message(format_args!(
            "wifi: preserved_failure source={} stage={} exact={} sdhci={} f2_state={}",
            trace.cached_source,
            trace.cached_stage,
            trace.cached_exact_error,
            trace.cached_sdhci_read_diag,
            trace.cached_f2_state,
        ));
        self.emit_console_line(preserved.as_str());

        let boot_failure = format_message(format_args!(
            "wifi: boot_failure_snapshot source={} stage={} exact={} sdhci={} f2_state={}",
            trace.cached_source,
            trace.cached_stage,
            trace.cached_exact_error,
            trace.cached_sdhci_read_diag,
            trace.cached_f2_state,
        ));
        self.emit_console_line(boot_failure.as_str());

        let cached = format_message(format_args!(
            "wifi: cccr_cached ioex={} iordy={} ien={} if={} speed={} cardcap={} fbr1_blk={} fbr2_blk={}",
            Self::format_optional_u8(trace.cached_cccr_io_enable),
            Self::format_optional_u8(trace.cached_cccr_io_ready),
            Self::format_optional_u8(trace.cached_cccr_int_enable),
            Self::format_optional_u8(trace.cached_cccr_bus_interface),
            Self::format_optional_u8(trace.cached_cccr_speed),
            Self::format_optional_u8(trace.cached_cccr_cardcap),
            Self::format_optional_u16(trace.cached_fbr1_block_size),
            Self::format_optional_u16(trace.cached_fbr2_block_size),
        ));
        self.emit_console_line(cached.as_str());

        let bounded_summary = format_message(format_args!(
            "wifi: bounded_phase count={}",
            trace.bounded_phase_count,
        ));
        self.emit_console_line(bounded_summary.as_str());
        for (index, record) in trace
            .bounded_phase_records
            .iter()
            .take(usize::from(trace.bounded_phase_count))
            .enumerate()
        {
            let line = format_message(format_args!(
                "wifi: bounded_phase[{index}] stage={} action={} mode={} clock={}Hz width={} no_ht={}",
                record.stage,
                record.action,
                record.mode,
                record.current_clock_hz,
                record.bus_width,
                if record.no_ht_transport { "yes" } else { "no" },
            ));
            self.emit_console_line(line.as_str());
        }
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_readiness_summary(
        &mut self,
        snapshot: &WifiDebugSnapshot,
        firmware_trace: Option<WifiFirmwareContractTrace>,
        control_trace: Option<WifiControlPlaneTrace>,
    ) {
        let chipclk = firmware_trace
            .and_then(|trace| trace.chipclkcsr)
            .or(snapshot.chipclkcsr);
        let wake = firmware_trace
            .and_then(|trace| trace.wakeupctrl)
            .or(snapshot.wakeupctrl);
        let sleep = firmware_trace
            .and_then(|trace| trace.sleepcsr)
            .or(snapshot.sleepcsr);
        let cardcap = firmware_trace
            .and_then(|trace| trace.cardcap)
            .or(snapshot.cardcap);
        let ht_req = chipclk.is_some_and(|value| (value & WIFI_CHIPCLKCSR_HT_AVAIL_REQ) != 0);
        let ht_avail = chipclk.is_some_and(|value| (value & WIFI_CHIPCLKCSR_HT_AVAIL) != 0);
        let alp_req = chipclk.is_some_and(|value| (value & WIFI_CHIPCLKCSR_ALP_AVAIL_REQ) != 0);
        let alp_avail = chipclk.is_some_and(|value| (value & WIFI_CHIPCLKCSR_ALP_AVAIL) != 0);
        let force_ht = chipclk.is_some_and(|value| (value & WIFI_CHIPCLKCSR_FORCE_HT) != 0);
        let htwait = wake.is_some_and(|value| (value & WIFI_WAKE_TILL_HT_AVAIL) != 0);
        let f2_enabled = snapshot.io_enable.is_some_and(|value| (value & 0x04) != 0)
            || control_trace
                .and_then(|trace| trace.cccr_io_enable)
                .is_some_and(|value| (value & 0x04) != 0);
        let f2_ready = snapshot.io_ready.is_some_and(|value| (value & 0x04) != 0)
            || control_trace
                .and_then(|trace| trace.cccr_io_ready)
                .is_some_and(|value| (value & 0x04) != 0);
        let ht_line = format_message(format_args!(
            "wifi: ht_state chipclk={} ht_req={} ht_avail={} alp_req={} alp_avail={} force_ht={} wake_htwait={} sleep={} kso={} devon={} cardcap={} clock={}Hz width={}",
            Self::format_optional_u8(chipclk),
            Self::yes_no(ht_req),
            Self::yes_no(ht_avail),
            Self::yes_no(alp_req),
            Self::yes_no(alp_avail),
            Self::yes_no(force_ht),
            Self::yes_no(htwait),
            Self::format_optional_u8(sleep),
            Self::wifi_sleep_bit_label(sleep, WIFI_SLEEPCSR_KSO),
            Self::wifi_sleep_bit_label(sleep, WIFI_SLEEPCSR_DEVON),
            Self::format_optional_u8(cardcap),
            snapshot.current_clock_hz,
            Self::wifi_bus_width_label(snapshot.bus_width),
        ));
        self.emit_console_line(ht_line.as_str());

        let direct_core_control_blocker = Self::wifi_exact_error_is_direct_sdio_transport_blocker(
            snapshot.control_plane_exact_error,
        );
        let policy = if direct_core_control_blocker {
            "pre-f2-core-control"
        } else {
            "post-ht-proof"
        };
        let gate = if direct_core_control_blocker {
            "core-control-blocked-before-f2"
        } else if ht_avail {
            "allow-f2-after-ht"
        } else if f2_enabled && !f2_ready {
            "blocked-latched-not-ready"
        } else {
            "block-f2-until-ht"
        };
        let blocker_phase = if direct_core_control_blocker {
            "pre-f2-core-control"
        } else {
            "post-ht-proof"
        };
        let f2_line = format_message(format_args!(
            "wifi: f2_gate policy={} gate={} f2_enabled={} f2_ready={} ioex={} iordy={} blocker={} blocker_phase={}",
            policy,
            gate,
            Self::yes_no(f2_enabled),
            Self::yes_no(f2_ready),
            Self::format_optional_u8(snapshot.io_enable),
            Self::format_optional_u8(snapshot.io_ready),
            snapshot.control_plane_exact_error,
            blocker_phase,
        ));
        self.emit_console_line(f2_line.as_str());

        if let Some(trace) = firmware_trace {
            let release = format_message(format_args!(
                "wifi: firmware_release fw={} nvram={} clm={} rstvec={} verify={} armcr4_release={} sr_kso={} next={}",
                trace.firmware_len,
                trace.nvram_len,
                Self::format_optional_usize(trace.clm_len),
                Self::format_optional_u32(trace.reset_vector),
                Self::yes_no(trace.firmware_download_verified),
                trace.armcr4_release_attempts,
                Self::yes_no(trace.sr_kso_clock_ready),
                trace.next_step,
            ));
            self.emit_console_line(release.as_str());
        }

        if let Some(trace) = control_trace {
            let boot_failure = format_message(format_args!(
                "wifi: boot_failure source={} stage={} exact={} sdhci={} f2_state={}",
                trace.cached_source,
                trace.cached_stage,
                trace.cached_exact_error,
                trace.cached_sdhci_read_diag,
                trace.cached_f2_state,
            ));
            self.emit_console_line(boot_failure.as_str());
        } else {
            let boot_failure = format_message(format_args!(
                "wifi: boot_failure source={} stage={} exact={} sdhci={} f2_state={}",
                snapshot.debug_snapshot_source,
                snapshot.debug_snapshot_stage,
                snapshot.control_plane_exact_error,
                snapshot.control_plane_sdhci_read_diag,
                snapshot.control_plane_f2_state,
            ));
            self.emit_console_line(boot_failure.as_str());
        }
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_startup_blackbox(
        &mut self,
        snapshot: &WifiDebugSnapshot,
        firmware_trace: Option<WifiFirmwareContractTrace>,
        control_trace: Option<WifiControlPlaneTrace>,
    ) {
        let fault = crate::drivers::driver_task_net::latest_cyw43_runtime_command_fault_status();
        self.emit_console_line("wifi: diag recorder=startup-blackbox mode=passive source=cached");
        self.emit_wifi_startup_gates_from_evidence(
            Some(snapshot),
            firmware_trace,
            control_trace,
            fault,
            None,
            None,
            "snapshot",
        );
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_driver_task_startup_blackbox(
        &mut self,
        fault: Option<crate::drivers::driver_task_net::Cyw43RuntimeCommandFaultStatus>,
        explicit_exact_error: Option<&str>,
        source: &str,
    ) {
        let recorder = format_message(format_args!(
            "wifi: diag recorder=startup-blackbox mode=passive source={source}"
        ));
        self.emit_console_line(recorder.as_str());
        let sdio_runtime_status =
            crate::drivers::driver_task_net::latest_sdio_runtime_replay_status();
        self.emit_wifi_startup_gates_from_evidence(
            None,
            None,
            None,
            fault,
            sdio_runtime_status,
            explicit_exact_error,
            source,
        );
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_startup_gates_from_evidence(
        &mut self,
        snapshot: Option<&WifiDebugSnapshot>,
        firmware_trace: Option<WifiFirmwareContractTrace>,
        control_trace: Option<WifiControlPlaneTrace>,
        fault: Option<crate::drivers::driver_task_net::Cyw43RuntimeCommandFaultStatus>,
        sdio_runtime_status: Option<crate::drivers::driver_task_net::SdioRuntimeReplayStatus>,
        explicit_exact_error: Option<&str>,
        source: &str,
    ) {
        let live_net_frontier = self.wifi_live_net_frontier();
        let live_net_supersedes_runtime = live_net_frontier.is_some();
        let explicit_exact_error = if live_net_supersedes_runtime {
            ""
        } else {
            explicit_exact_error.unwrap_or("")
        };
        let explicit_join_security =
            Self::wifi_exact_error_is_join_security_blocker(explicit_exact_error);
        let fault = if live_net_supersedes_runtime {
            None
        } else {
            fault
        };
        let sdio_runtime_status = if live_net_supersedes_runtime {
            None
        } else {
            sdio_runtime_status
        };
        let cyw43_fault_gate: Option<u8> = if explicit_join_security {
            None
        } else {
            fault.map(Self::wifi_runtime_fault_gate)
        };
        let sdio_replay_gate: Option<u8> =
            sdio_runtime_status.and_then(Self::wifi_sdio_runtime_replay_gate);
        let sdio_runtime_progress = if live_net_supersedes_runtime {
            None
        } else {
            crate::hal::driver_task::latest_driver_task_ring_progress(
                crate::hal::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
            )
        };
        let cyw43_runtime_progress = if live_net_supersedes_runtime {
            None
        } else {
            crate::hal::driver_task::latest_driver_task_ring_progress(
                crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            )
        };
        let sdio_progress_gate = sdio_runtime_progress
            .and_then(|progress| Self::wifi_sdio_runtime_progress_gate(progress.phase));
        let cyw43_progress_gate = cyw43_runtime_progress
            .and_then(|progress| Self::wifi_cyw43_runtime_progress_gate(progress.phase));
        let driver_task_gate: Option<u8> = if explicit_join_security {
            Some(8)
        } else {
            cyw43_fault_gate
                .or(cyw43_progress_gate)
                .or(sdio_replay_gate)
                .or(sdio_progress_gate)
        };
        let live_net_channel_ready = live_net_supersedes_runtime;
        let power_ready = live_net_channel_ready
            || snapshot.is_some_and(|snapshot| {
                matches!(snapshot.power_state, WifiPowerState::On)
                    && matches!(snapshot.reset_state, WifiResetState::Deasserted)
            })
            || fault.is_some_and(Self::wifi_runtime_fault_implies_hal_power_ready);
        let card_selected = live_net_channel_ready
            || snapshot.is_some_and(|snapshot| snapshot.card_ready && snapshot.card_rca != 0);
        let f1_ready = live_net_channel_ready
            || snapshot.is_some_and(|snapshot| {
                snapshot.io_enable.is_some_and(|value| (value & 0x02) != 0)
                    && snapshot.io_ready.is_some_and(|value| (value & 0x02) != 0)
            })
            || control_trace.is_some_and(|trace| {
                trace
                    .cccr_io_enable
                    .is_some_and(|value| (value & 0x02) != 0)
                    && trace.cccr_io_ready.is_some_and(|value| (value & 0x02) != 0)
            });
        let ht_ready = live_net_channel_ready
            || snapshot.is_some_and(Self::wifi_snapshot_ht_avail)
            || firmware_trace.is_some_and(|trace| trace.sr_kso_clock_ready);
        let backplane_ready = live_net_channel_ready
            || snapshot.is_some_and(|snapshot| {
                snapshot.programmed_backplane_window.is_some()
                    || snapshot.shadow_backplane_window.is_some()
                    || snapshot.shadow_backplane_fn_addr.is_some()
            })
            || control_trace.is_some_and(|trace| {
                trace.backplane_window_low != 0
                    || trace.backplane_window_mid != 0
                    || trace.backplane_window_high != 0
            });
        let firmware_uploaded = live_net_channel_ready
            || firmware_trace
                .and_then(|trace| trace.proof)
                .is_some_and(|proof| proof.upload_state == "uploaded" || proof.verified);
        let firmware_verified = live_net_channel_ready
            || firmware_trace.is_some_and(|trace| trace.firmware_download_verified)
            || firmware_trace
                .and_then(|trace| trace.proof)
                .is_some_and(|proof| proof.verified);
        let f2_enabled = live_net_channel_ready
            || snapshot.is_some_and(|snapshot| {
                snapshot.io_enable.is_some_and(|value| (value & 0x04) != 0)
            })
            || control_trace.is_some_and(|trace| {
                trace
                    .cccr_io_enable
                    .is_some_and(|value| (value & 0x04) != 0)
            });
        let f2_ready = live_net_channel_ready
            || snapshot
                .is_some_and(|snapshot| snapshot.io_ready.is_some_and(|value| (value & 0x04) != 0))
            || control_trace
                .is_some_and(|trace| trace.cccr_io_ready.is_some_and(|value| (value & 0x04) != 0));
        let exact_error = snapshot.map_or(explicit_exact_error, |snapshot| {
            if live_net_supersedes_runtime {
                ""
            } else {
                snapshot.control_plane_exact_error
            }
        });
        let channel_ready = live_net_channel_ready
            || exact_error.is_empty()
            || Self::wifi_exact_error_is_join_security_blocker(exact_error);

        let dhcp_pass = live_net_frontier.as_ref().map_or_else(
            || self.wifi_diag_dhcp_bound(),
            Self::wifi_live_net_dhcp_bound,
        );
        let proof_gate: u8 = Self::wifi_startup_proof_gate(
            power_ready,
            card_selected,
            f1_ready,
            ht_ready,
            backplane_ready,
            firmware_uploaded && cyw43_fault_gate != Some(6),
            f2_enabled && f2_ready,
            channel_ready && cyw43_fault_gate.is_none(),
            dhcp_pass,
            false,
        );
        let failing_gate: u8 = if let Some(gate) = driver_task_gate {
            gate
        } else if proof_gate >= 10 {
            0
        } else {
            proof_gate.saturating_add(1).max(1)
        };
        let reported_proof_gate: u8 = if let Some(gate) = driver_task_gate {
            proof_gate.max(gate.saturating_sub(1))
        } else {
            proof_gate
        };
        let active_blocker = if let Some(frontier) = live_net_frontier.as_ref() {
            if Self::wifi_live_net_dhcp_bound(frontier) {
                Self::wifi_startup_blocker_for_gate(failing_gate, exact_error)
            } else {
                Self::wifi_live_net_blocker(frontier)
            }
        } else if explicit_join_security {
            exact_error
        } else if let Some(fault) = fault {
            fault.reason
        } else if let (Some(status), Some(_)) = (sdio_runtime_status, sdio_replay_gate) {
            Self::wifi_sdio_runtime_replay_blocker(status)
        } else if let Some(progress) = cyw43_runtime_progress {
            Self::wifi_cyw43_runtime_progress_blocker(progress.phase)
        } else if let Some(progress) = sdio_runtime_progress {
            Self::wifi_sdio_runtime_progress_blocker(progress.phase)
        } else {
            Self::wifi_startup_blocker_for_gate(failing_gate, exact_error)
        };
        let next_action = if let Some(frontier) = live_net_frontier.as_ref() {
            if Self::wifi_live_net_dhcp_bound(frontier) {
                Self::wifi_startup_next_action_for_gate(failing_gate, exact_error)
            } else {
                "run-dhcp-and-report-lease-state"
            }
        } else if explicit_join_security {
            "inspect-host-eapol-rx-path"
        } else if let Some(fault) = fault {
            Self::wifi_runtime_fault_next_action(fault)
        } else if let (Some(status), Some(_)) = (sdio_runtime_status, sdio_replay_gate) {
            Self::wifi_sdio_runtime_replay_next_action(status)
        } else if let Some(progress) = cyw43_runtime_progress {
            Self::wifi_cyw43_runtime_progress_next_action(progress.phase)
        } else if let Some(progress) = sdio_runtime_progress {
            Self::wifi_sdio_runtime_progress_next_action(progress.phase)
        } else {
            Self::wifi_startup_next_action_for_gate(failing_gate, exact_error)
        };
        let gate1_power = if let Some(snapshot) = snapshot {
            Self::wifi_power_label(snapshot.power_state)
        } else if fault.is_some_and(Self::wifi_runtime_fault_implies_hal_power_ready) {
            "on"
        } else {
            "unknown"
        };
        let gate1_reset = if let Some(snapshot) = snapshot {
            Self::wifi_reset_label(snapshot.reset_state)
        } else if fault.is_some_and(Self::wifi_runtime_fault_implies_hal_power_ready) {
            "deasserted"
        } else {
            "unknown"
        };
        let gate2_evidence = if let Some(snapshot) = snapshot {
            format_message(format_args!(
                "card={} rca=0x{:04x} ocr=0x{:08x}",
                if snapshot.card_ready { "yes" } else { "no" },
                snapshot.card_rca,
                snapshot.card_ocr,
            ))
        } else if let Some(fault) = fault {
            format_message(format_args!(
                "stage={} detail=0x{:04x} result=0x{:08x}",
                fault.stage, fault.detail, fault.result,
            ))
        } else if let Some(status) = sdio_runtime_status {
            format_message(format_args!(
                "stage={} status={} phase={} phase_name={} marker_valid={} source=linked-runtime",
                status.stage,
                status.status,
                sdio_runtime_progress.map_or(0, |progress| progress.phase),
                sdio_runtime_progress.map_or("none", |progress| progress.phase_name),
                sdio_runtime_progress.map_or("no", |progress| Self::yes_no(progress.marker_valid)),
            ))
        } else if let Some(progress) = sdio_runtime_progress {
            format_message(format_args!(
                "stage=engine-init status=progress-only phase={} phase_name={} marker_valid={} source=linked-runtime",
                progress.phase,
                progress.phase_name,
                Self::yes_no(progress.marker_valid),
            ))
        } else if let Some(progress) = cyw43_runtime_progress {
            format_message(format_args!(
                "stage=cyw43-transport status=progress-only phase={} phase_name={} marker_valid={} source=linked-runtime",
                progress.phase,
                progress.phase_name,
                Self::yes_no(progress.marker_valid),
            ))
        } else {
            format_message(format_args!("card=unknown rca=0x0000 ocr=0x00000000"))
        };
        if let Some(progress) = cyw43_runtime_progress {
            let progress_line = format_message(format_args!(
                "wifi: cyw43 linked_runtime_progress marker_valid={} sequence={} phase={} phase_name={} aux0=0x{:08x} gate={} blocker={} next_action={}",
                Self::yes_no(progress.marker_valid),
                progress.sequence,
                progress.phase,
                progress.phase_name,
                progress.aux0,
                Self::wifi_cyw43_runtime_progress_gate(progress.phase).unwrap_or(0),
                Self::wifi_cyw43_runtime_progress_blocker(progress.phase),
                Self::wifi_cyw43_runtime_progress_next_action(progress.phase),
            ));
            self.emit_console_line(progress_line.as_str());
        }
        if let Some(progress) = sdio_runtime_progress {
            let progress_line = format_message(format_args!(
                "wifi: sdio linked_runtime_progress marker_valid={} sequence={} phase={} phase_name={} aux0=0x{:08x} gate={} blocker={} next_action={}",
                Self::yes_no(progress.marker_valid),
                progress.sequence,
                progress.phase,
                progress.phase_name,
                progress.aux0,
                Self::wifi_sdio_runtime_progress_gate(progress.phase).unwrap_or(0),
                Self::wifi_sdio_runtime_progress_blocker(progress.phase),
                Self::wifi_sdio_runtime_progress_next_action(progress.phase),
            ));
            self.emit_console_line(progress_line.as_str());
        }

        self.emit_wifi_gate_line(
            1,
            "runtime-power-reset",
            Self::wifi_startup_gate_status(1, proof_gate, failing_gate),
            format_args!(
                "power={} reset={} source={}",
                gate1_power, gate1_reset, source,
            ),
            "sdio-card-select",
        );
        self.emit_wifi_gate_line(
            2,
            "sdio-card-select",
            Self::wifi_startup_gate_status(2, proof_gate, failing_gate),
            format_args!("{}", gate2_evidence.as_str()),
            "cccr-fbr-ready",
        );
        self.emit_wifi_gate_line(
            3,
            "cccr-fbr-ready",
            Self::wifi_startup_gate_status(3, proof_gate, failing_gate),
            format_args!(
                "ioex={} iordy={} fbr1_blk={} fbr2_blk={}",
                Self::format_optional_u8(snapshot.and_then(|snapshot| snapshot.io_enable)),
                Self::format_optional_u8(snapshot.and_then(|snapshot| snapshot.io_ready)),
                Self::format_optional_u16(
                    control_trace.and_then(|trace| trace.cached_fbr1_block_size)
                ),
                Self::format_optional_u16(
                    control_trace.and_then(|trace| trace.cached_fbr2_block_size)
                ),
            ),
            "ht-clock",
        );
        self.emit_wifi_gate_line(
            4,
            "ht-clock",
            Self::wifi_startup_gate_status(4, proof_gate, failing_gate),
            format_args!(
                "chipclk={} clock={}Hz width={}",
                Self::format_optional_u8(snapshot.and_then(|snapshot| snapshot.chipclkcsr)),
                snapshot.map_or(0, |snapshot| snapshot.current_clock_hz),
                snapshot.map_or("unknown", |snapshot| Self::wifi_bus_width_label(
                    snapshot.bus_width
                )),
            ),
            "backplane-window",
        );
        self.emit_wifi_gate_line(
            5,
            "backplane-window",
            Self::wifi_startup_gate_status(5, proof_gate, failing_gate),
            format_args!(
                "programmed={} shadow={} fn={}",
                Self::format_optional_u32(
                    snapshot.and_then(|snapshot| snapshot.programmed_backplane_window)
                ),
                Self::format_optional_u32(
                    snapshot.and_then(|snapshot| snapshot.shadow_backplane_window)
                ),
                Self::format_optional_fn_addr(
                    snapshot.and_then(|snapshot| snapshot.shadow_backplane_fn_addr)
                ),
            ),
            "firmware-upload",
        );
        self.emit_wifi_gate_line(
            6,
            "firmware-upload",
            Self::wifi_startup_gate_status(6, proof_gate, failing_gate),
            format_args!(
                "uploaded={} verified={} fault_detail=0x{:04x}",
                Self::yes_no(firmware_uploaded),
                Self::yes_no(firmware_verified),
                fault.map_or(0, |fault| fault.detail),
            ),
            "function2-ready",
        );
        self.emit_wifi_gate_line(
            7,
            "function2-ready",
            Self::wifi_startup_gate_status(7, proof_gate, failing_gate),
            format_args!(
                "f2_enabled={} f2_ready={} f2_state={} dependency={}",
                Self::yes_no(f2_enabled),
                Self::yes_no(f2_ready),
                snapshot.map_or("unknown", |snapshot| snapshot.control_plane_f2_state),
                Self::wifi_gate_dependency_label(7, failing_gate),
            ),
            Self::wifi_startup_gate_name_for_gate(8, exact_error),
        );
        self.emit_wifi_gate_line(
            8,
            Self::wifi_startup_gate_name_for_gate(8, exact_error),
            Self::wifi_startup_gate_status(8, proof_gate, failing_gate),
            format_args!(
                "exact={} control_stage={} sdhci={} reply_mode={} dependency={}",
                if exact_error.is_empty() {
                    "none"
                } else {
                    exact_error
                },
                Self::wifi_control_stage_for_exact_error(exact_error),
                snapshot.map_or("unknown", |snapshot| snapshot.control_plane_sdhci_read_diag),
                snapshot.map_or("unknown", |snapshot| snapshot.control_plane_reply_mode),
                Self::wifi_gate_dependency_label(8, failing_gate),
            ),
            "dhcp-bound",
        );
        self.emit_wifi_gate_line(
            9,
            "dhcp-bound",
            Self::wifi_startup_gate_status(9, proof_gate, failing_gate),
            format_args!(
                "{} dependency={}",
                self.wifi_diag_network_evidence(),
                Self::wifi_gate_dependency_label(9, failing_gate),
            ),
            "nettest-netstats-cohsh",
        );
        self.emit_wifi_gate_line(
            10,
            "nettest-netstats-cohsh",
            Self::wifi_startup_gate_status(10, proof_gate, failing_gate),
            format_args!(
                "{} dependency={}",
                self.wifi_diag_acceptance_evidence(),
                Self::wifi_gate_dependency_label(10, failing_gate),
            ),
            "acceptance-complete",
        );
        if let Some(fault) = fault {
            let owner_fault =
                crate::drivers::driver_task_net::latest_cyw43_sdio_owner_fault_status();
            let fault_line = format_message(format_args!(
                "wifi: evidence cyw43 stage={} op={} flags=0x{:04x} target=0x{:08x} payload_off={} payload_len={} total_len={} control_cmd={} control_cmd_hex=0x{:08x} control_id={} control_header_mode={} control_response_len={} detail=0x{:04x} reason={} result=0x{:08x}",
                fault.stage,
                fault.op,
                fault.flags,
                fault.target_addr,
                fault.payload_offset,
                fault.payload_len,
                fault.total_len,
                fault.control_cmd,
                fault.control_cmd,
                fault.control_id,
                fault.control_header_mode,
                fault.control_response_len,
                fault.detail,
                fault.reason,
                fault.result,
            ));
            self.emit_console_line(fault_line.as_str());
            if Self::wifi_runtime_fault_is_sdio_card_select(fault) {
                let sdio_command = format_message(format_args!(
                    "wifi: evidence sdio_command command={} attempt={} card_bits=0x{:04x} stage={} detail=0x{:04x} result=0x{:08x}",
                    Self::wifi_cyw43_card_select_command_label(fault.detail),
                    Self::wifi_cyw43_card_select_attempt(fault.result),
                    Self::wifi_cyw43_card_select_low_bits(fault.result),
                    fault.stage,
                    fault.detail,
                    fault.result,
                ));
                self.emit_console_line(sdio_command.as_str());
            }
            if let Some(owner_fault) = owner_fault {
                let cmd53 = format_message(format_args!(
                    "wifi: evidence sdio_cmd53 func={} addr=0x{:08x} target=0x{:08x} effective=0x{:08x} chunk_off={} payload_off={} len={} increment={} block_mode={} mode={} op={} source=owner-terminal",
                    owner_fault.function,
                    owner_fault.addr,
                    owner_fault.target_addr,
                    owner_fault.effective_target,
                    owner_fault.chunk_offset,
                    owner_fault.payload_offset,
                    owner_fault.len,
                    Self::yes_no(owner_fault.increment),
                    Self::yes_no(owner_fault.block_mode),
                    Self::wifi_sdio_owner_mode_label(owner_fault),
                    owner_fault.op,
                ));
                self.emit_console_line(cmd53.as_str());
                let status = format_message(format_args!(
                    "wifi: evidence sdio_status descriptor_status={} transfer_stage={} transfer_status=0x{:06x} transfer_reason={} r5=0x{:04x} retry={} host=0x{:02x} clock=0x{:04x}",
                    owner_fault.reason,
                    owner_fault.transfer_stage,
                    owner_fault.transfer_status,
                    owner_fault.transfer_reason,
                    owner_fault.r5,
                    owner_fault.retry,
                    owner_fault.host_control,
                    owner_fault.clock_control,
                ));
                self.emit_console_line(status.as_str());
                let payload = format_message(format_args!(
                    "wifi: evidence sdio_payload chunk_off={} payload_off={} first=0x{:02x} last=0x{:02x} xor=0x{:02x} sum=0x{:08x} owner_window={}",
                    owner_fault.chunk_offset,
                    owner_fault.payload_offset,
                    owner_fault.payload_first,
                    owner_fault.payload_last,
                    owner_fault.payload_xor,
                    owner_fault.payload_sum,
                    owner_fault.owner_window,
                ));
                self.emit_console_line(payload.as_str());
            } else if !Self::wifi_runtime_fault_is_sdio_card_select(fault) {
                let cmd53 = format_message(format_args!(
                    "wifi: evidence sdio_cmd53 func={} addr=0x{:08x} len={} increment={} block_mode={} op={} descriptor_status={} transfer_stage={} transfer_status=0x{:06x} r5=0x{:04x} source=cyw43-descriptor",
                    Self::wifi_cyw43_fault_cmd53_function(fault),
                    fault.target_addr,
                    fault.payload_len,
                    Self::wifi_cyw43_fault_cmd53_increment(fault),
                    Self::wifi_cyw43_fault_cmd53_mode(fault),
                    fault.op,
                    Self::wifi_cyw43_fault_descriptor_status(fault.detail),
                    Self::wifi_sdio_transfer_failure_stage(fault.result),
                    Self::wifi_sdio_transfer_failure_status(fault.result),
                    Self::wifi_sdio_transfer_failure_r5(fault.result),
                ));
                self.emit_console_line(cmd53.as_str());
            };
        }
        let direct_proof_gate = if explicit_join_security {
            7
        } else {
            proof_gate
        };
        let boundary = format_message(format_args!(
            "wifi: evidence boundary console_client=root-net-console hal=admission-descriptor-diagnostics-only linked_runtime_owner=cyw43+sdio failure_domain={} direct_proof_gate={} proof_gate={} frontier_gate={} failing_gate={} target_gate=10",
            active_blocker,
            direct_proof_gate,
            reported_proof_gate,
            reported_proof_gate,
            failing_gate,
        ));
        self.emit_console_line(boundary.as_str());
        let next = format_message(format_args!(
            "wifi: next_action={} blocker={} proof_gate={} target_gate=10 source={}",
            next_action, active_blocker, reported_proof_gate, source,
        ));
        self.emit_console_line(next.as_str());
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_gate_line(
        &mut self,
        gate: u8,
        name: &'static str,
        status: &'static str,
        evidence: fmt::Arguments<'_>,
        next: &'static str,
    ) {
        let mut line = format_message(format_args!(
            "wifi: gate {} name={} status={} evidence=",
            gate, name, status
        ));
        if FmtWrite::write_fmt(&mut line, evidence).is_err() {
            let _ = write!(line, "truncated");
        }
        let _ = write!(line, " next={next}");
        self.emit_console_line(line.as_str());
    }

    #[cfg(feature = "kernel")]
    const fn wifi_startup_gate_status(gate: u8, proof_gate: u8, failing_gate: u8) -> &'static str {
        if failing_gate != 0 {
            if gate <= proof_gate {
                "pass"
            } else if gate == failing_gate {
                "fail"
            } else if gate < failing_gate {
                "inferred"
            } else {
                "blocked"
            }
        } else if gate <= proof_gate {
            "pass"
        } else {
            "blocked"
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_gate_dependency_label(
        gate: u8,
        failing_gate: u8,
    ) -> HeaplessString<DEFAULT_LINE_CAPACITY> {
        if failing_gate != 0 && gate > failing_gate {
            format_message(format_args!("not-reached-due-to-gate-{failing_gate}"))
        } else {
            format_message(format_args!("ready-for-direct-evidence"))
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_runtime_fault_gate(
        fault: crate::drivers::driver_task_net::Cyw43RuntimeCommandFaultStatus,
    ) -> u8 {
        if Self::wifi_runtime_fault_is_sdio_card_select(fault) {
            return 2;
        }
        if Self::wifi_runtime_fault_is_transport_no_reply(fault) {
            return 3;
        }
        if Self::wifi_runtime_fault_is_firmware_stream(fault) {
            return 6;
        }
        Self::wifi_cyw43_fault_gate(fault.detail)
    }

    #[cfg(feature = "kernel")]
    fn wifi_sdio_runtime_replay_gate(
        status: crate::drivers::driver_task_net::SdioRuntimeReplayStatus,
    ) -> Option<u8> {
        if matches!(status.status, "ready" | "preserved-ready") {
            return None;
        }
        match status.stage {
            "descriptor-replay" => Some(1),
            "engine-init" | "owner-state" | "sdio-card-init-restart" => Some(2),
            "sdio-cmd0-go-idle"
            | "sdio-cmd5-ocr"
            | "sdio-cmd5-ready"
            | "sdio-cmd3-rca"
            | "sdio-cmd7-select"
            | "sdio-cmd7-select-pre-recover"
            | "sdio-cmd7-select-host-recover"
            | "sdio-cmd7-select-r1-fallback"
            | "sdio-cmd7-select-r1-host-recover"
            | "sdio-card-init-restart-host-recover" => Some(2),
            _ => None,
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_sdio_runtime_replay_blocker(
        status: crate::drivers::driver_task_net::SdioRuntimeReplayStatus,
    ) -> &'static str {
        if status.stage == "engine-init" && status.status == "no-reply" {
            return "sdio-engine-init-no-reply";
        }
        if status.stage == "descriptor-replay" && status.status == "pending" {
            return "sdio-descriptor-replay-pending";
        }
        if status.stage == "owner-state" && status.status == "descriptor-rejected" {
            return "sdio-owner-state-descriptor-rejected";
        }
        match status.stage {
            "descriptor-replay" => "sdio-descriptor-replay",
            "engine-init" => "sdio-engine-init",
            "owner-state" => "sdio-owner-state",
            "sdio-card-init-restart" => "sdio-card-init-restart",
            _ => "sdio-linked-runtime-replay",
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_sdio_runtime_replay_next_action(
        status: crate::drivers::driver_task_net::SdioRuntimeReplayStatus,
    ) -> &'static str {
        match status.stage {
            "descriptor-replay" => "verify-linked-sdio-runtime-descriptor-replay",
            "engine-init" => "inspect-linked-sdio-runtime-engine-init-dispatch",
            "owner-state" => "verify-linked-sdio-owner-state-descriptor",
            "sdio-card-init-restart" => "replay-linked-sdio-card-init-sequence",
            _ => "inspect-linked-sdio-runtime-replay-status",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_sdio_runtime_progress_gate(phase: u32) -> Option<u8> {
        match phase {
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_COMMAND_OBSERVED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_COMMAND_VALIDATED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DISPATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_ENTER
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_AUX_MATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_FRAME_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_MARK_ENTER
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_LOADED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_VALID
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_INVALID
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_TOTALS_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_FORBIDDEN_PRESENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_ROLE_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCES_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RUNTIME_ENTRY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ENGINE_INIT_BRANCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_SHADOW_RESET_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_SHADOW_RESET_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_INT_CLEAR_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_PRESENT_READ_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_POWER_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_CLOCK_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_INHIBIT_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_RESET_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_RESET_CLOCK_DISABLE_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_RESET_POWER_DISABLE_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_POWER_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_CLOCK_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_STATE_RESET_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_STATE_RESET_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_HW_ENTRY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_READY => Some(2),
            _ => None,
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_sdio_runtime_progress_blocker(phase: u32) -> &'static str {
        match phase {
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_BEGIN => {
                "sdio-engine-init-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_MARK_ENTER => {
                "sdio-engine-init-mark-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_READY => {
                "sdio-engine-init-descriptor-ready-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_BEGIN => {
                "sdio-engine-init-resource-check-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_FAILED => {
                "sdio-engine-init-resource-check-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_INVALID => {
                "sdio-resource-descriptor-invalid"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH => {
                "sdio-resource-hot-path-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_MISSING => {
                "sdio-resource-sdhci-mmio-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_MISSING => {
                "sdio-resource-dma-arena-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_MISSING => {
                "sdio-resource-shared-pages-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_MISSING => {
                "sdio-resource-bus-link-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_FORBIDDEN_PRESENT => {
                "sdio-resource-forbidden-window-present"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_VALID
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_TOTALS_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_ROLE_READY => {
                "sdio-engine-init-resource-subcheck-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCES_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_BEGIN => {
                "sdio-engine-init-hardware-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RUNTIME_ENTRY => {
                "sdio-engine-init-runtime-entry-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ENGINE_INIT_BRANCH => {
                "sdio-engine-init-branch-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_SHADOW_RESET_BEGIN => {
                "sdio-shadow-reset-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_SHADOW_RESET_DONE => {
                "sdio-state-reset-entry-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_STATE_RESET_BEGIN => {
                "sdio-state-reset-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_STATE_RESET_DONE => {
                "sdio-hardware-entry-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_HW_ENTRY => {
                "sdio-sdhci-mmio-entry-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_BEGIN => "sdio-adopt-no-reply",
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_INT_CLEAR_BEGIN => {
                "sdio-adopt-int-clear-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_PRESENT_READ_BEGIN => {
                "sdio-adopt-present-read-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_POWER_MISSING => {
                "sdio-adopt-power-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_CLOCK_FAILED => {
                "sdio-adopt-clock-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_INHIBIT_FAILED => {
                "sdio-adopt-inhibit-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_RESET_BEGIN => "sdio-reset-no-reply",
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_RESET_CLOCK_DISABLE_BEGIN => {
                "sdio-reset-clock-disable-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_RESET_POWER_DISABLE_BEGIN => {
                "sdio-reset-power-disable-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_POWER_READY => "sdio-power-no-reply",
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_CLOCK_READY => "sdio-clock-no-reply",
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_READY => "sdio-card-select-no-reply",
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_FAILED => {
                "sdio-engine-init-failed"
            }
            _ => "sdio-linked-runtime-progress-no-reply",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_sdio_runtime_progress_next_action(phase: u32) -> &'static str {
        match phase {
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_BEGIN => {
                "inspect-linked-sdio-runtime-engine-init-dispatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_MARK_ENTER => {
                "inspect-linked-sdio-runtime-descriptor-load"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_READY => {
                "inspect-linked-sdio-runtime-resource-check"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_BEGIN => {
                "inspect-linked-sdio-descriptor-resource-scan"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_FAILED => {
                "inspect-sdio-runtime-init-descriptor-mmio-range"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_INVALID => {
                "inspect-sdio-runtime-init-descriptor-header"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH => {
                "inspect-sdio-runtime-hot-path-and-role-bit"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_MISSING => {
                "inspect-sdio-sdhci-mmio-resource-range"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_MISSING => {
                "inspect-sdio-dma-resource-range"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_MISSING => {
                "inspect-sdio-shared-page-resource-range"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_MISSING => {
                "inspect-sdio-owner-bus-link"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_FORBIDDEN_PRESENT => {
                "inspect-sdio-resource-authority-window"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_VALID
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_TOTALS_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_ROLE_READY => {
                "inspect-next-sdio-resource-subcheck"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCES_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_HW_BEGIN => {
                "inspect-sdhci-reset-first-mmio-access"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RUNTIME_ENTRY => {
                "inspect-linked-sdio-engine-init-hot-path-dispatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ENGINE_INIT_BRANCH => {
                "inspect-sdio-shadow-reset-entry"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_SHADOW_RESET_BEGIN => {
                "inspect-sdio-register-shadow-reset"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_SHADOW_RESET_DONE => {
                "inspect-sdio-runtime-state-entry"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_STATE_RESET_BEGIN => {
                "inspect-sdio-runtime-state-reset"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_STATE_RESET_DONE => {
                "inspect-sdio-hardware-entry"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_HW_ENTRY => {
                "inspect-sdhci-first-mmio-access"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_BEGIN => {
                "inspect-sdhci-adopt-first-mmio-access"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_INT_CLEAR_BEGIN => {
                "inspect-sdhci-interrupt-status-clear"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_PRESENT_READ_BEGIN => {
                "inspect-sdhci-present-state-read"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_POWER_MISSING => {
                "inspect-sdhci-power-and-card-present-state"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_CLOCK_FAILED => {
                "inspect-sdhci-startup-clock-enable"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_INHIBIT_FAILED => {
                "inspect-sdhci-command-data-inhibit-clear"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_RESET_BEGIN => {
                "inspect-sdhci-reset-completion-loop"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_RESET_CLOCK_DISABLE_BEGIN => {
                "inspect-sdhci-clock-disable-write"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_RESET_POWER_DISABLE_BEGIN => {
                "inspect-sdhci-power-disable-write"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_POWER_READY => {
                "inspect-sdhci-clock-enable-loop"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_CLOCK_READY => {
                "inspect-sdhci-command-inhibit-and-card-detect"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_READY => {
                "replay-linked-sdio-card-init-sequence"
            }
            _ => "inspect-linked-sdio-runtime-progress",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_cyw43_runtime_progress_gate(phase: u32) -> Option<u8> {
        match phase {
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_COMMAND_OBSERVED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_COMMAND_VALIDATED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DISPATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_ENTER
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_AUX_MATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_FRAME_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_MARK_ENTER
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_LOADED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_VALID
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_INVALID
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_TOTALS_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_MMIO_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DMA_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_FORBIDDEN_PRESENT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_ROLE_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RUNTIME_ENTRY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_ENGINE_INIT_BRANCH
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_STATE_RESET_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_STATE_RESET_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_FORBIDDEN_SDIO_MMIO
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_BUS_LINK_CHECK_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SHARED_CONTROL_CHECK_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SHARED_CONTROL_MISSING
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SHARED_CONTROL_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_TRANSPORT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_BUS_LINK_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_ADOPT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_HOST_CONFIG_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD0_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD5_OCR_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD5_READY_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD3_RCA_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD7_SELECT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_SEND_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_SEND_DONE
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_WAIT_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_WAIT_TIMEOUT
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_REPLY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_READY => Some(2),
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_BLOCK_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_BLOCK_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F2_BLOCK_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F2_BLOCK_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_ENABLE_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_ENABLED => Some(3),
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_HOST_CONFIG_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_HOST_READY => Some(4),
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_BEGIN
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_TRANSPORT_READY => Some(5),
            _ => None,
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_cyw43_runtime_progress_blocker(phase: u32) -> &'static str {
        match phase {
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_BEGIN => {
                "cyw43-engine-init-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_MARK_ENTER => {
                "cyw43-engine-init-mark-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_READY => {
                "cyw43-engine-init-descriptor-ready-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_BEGIN => {
                "cyw43-engine-init-resource-check-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_FAILED => {
                "cyw43-engine-init-resource-check-failed"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_INVALID => {
                "cyw43-resource-descriptor-invalid"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH => {
                "cyw43-resource-hot-path-mismatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_MISSING => {
                "cyw43-resource-shared-pages-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_MISSING => {
                "cyw43-resource-sdio-owner-bus-link-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_FORBIDDEN_PRESENT => {
                "cyw43-resource-forbidden-window-present"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RUNTIME_ENTRY => {
                "cyw43-engine-init-runtime-entry-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_ENGINE_INIT_BRANCH => {
                "cyw43-engine-init-state-slot-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_STATE_RESET_BEGIN => {
                "cyw43-state-reset-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_STATE_RESET_DONE => {
                "cyw43-forbidden-sdio-mmio-check-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_FORBIDDEN_SDIO_MMIO => {
                "cyw43-resource-forbidden-sdio-mmio"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_BUS_LINK_CHECK_BEGIN => {
                "cyw43-resource-sdio-owner-bus-link-check-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SHARED_CONTROL_CHECK_BEGIN => {
                "cyw43-resource-shared-control-check-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SHARED_CONTROL_MISSING => {
                "cyw43-resource-shared-control-missing"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SHARED_CONTROL_READY => {
                "cyw43-engine-init-completion-publish-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_TRANSPORT_BEGIN => {
                "cyw43-transport-start-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_BUS_LINK_READY => {
                "cyw43-sdio-card-adoption-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_ADOPT_BEGIN => {
                "cyw43-sdio-card-adoption-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_HOST_CONFIG_BEGIN => {
                "cyw43-sdio-card-host-config-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD0_BEGIN => {
                "cyw43-sdio-card-cmd0-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD5_OCR_BEGIN => {
                "cyw43-sdio-card-cmd5-ocr-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD5_READY_BEGIN => {
                "cyw43-sdio-card-cmd5-ready-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD3_RCA_BEGIN => {
                "cyw43-sdio-card-cmd3-rca-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD7_SELECT_BEGIN => {
                "cyw43-sdio-card-cmd7-select-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_SEND_BEGIN => {
                "cyw43-sdio-owner-send-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_SEND_DONE => {
                "cyw43-sdio-owner-wait-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_WAIT_BEGIN => {
                "cyw43-sdio-owner-completion-pending"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_WAIT_TIMEOUT => {
                "cyw43-sdio-owner-completion-timeout"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_REPLY => {
                "cyw43-sdio-owner-replied"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_READY => {
                "cyw43-f1-block-size-start-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_BLOCK_BEGIN => {
                "cyw43-f1-block-size-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_BLOCK_READY => {
                "cyw43-f2-block-size-start-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F2_BLOCK_BEGIN => {
                "cyw43-f2-block-size-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F2_BLOCK_READY => {
                "cyw43-f1-enable-start-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_ENABLE_BEGIN => {
                "cyw43-f1-enable-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_ENABLED => {
                "cyw43-host-startup-clock-start-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_HOST_CONFIG_BEGIN => {
                "cyw43-host-startup-clock-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_HOST_READY => {
                "cyw43-backplane-alp-start-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_BEGIN => {
                "cyw43-backplane-alp-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_READY => {
                "cyw43-transport-ready-publish-no-reply"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_TRANSPORT_READY => {
                "cyw43-firmware-prep-no-reply"
            }
            _ => "cyw43-linked-runtime-progress-no-reply",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_cyw43_runtime_progress_next_action(phase: u32) -> &'static str {
        match phase {
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_BEGIN => {
                "inspect-linked-cyw43-runtime-engine-init-dispatch"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_MARK_ENTER => {
                "inspect-linked-cyw43-runtime-descriptor-load"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_DESCRIPTOR_READY => {
                "inspect-linked-cyw43-runtime-resource-check"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_BEGIN => {
                "inspect-linked-cyw43-descriptor-resource-scan"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_FAILED => {
                "inspect-cyw43-runtime-init-descriptor-ranges-and-bus-link"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_DESCRIPTOR_INVALID => {
                "inspect-cyw43-runtime-init-descriptor-header"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH => {
                "inspect-cyw43-runtime-hot-path-and-role-bit"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_SHARED_MISSING => {
                "inspect-cyw43-shared-page-resource-range"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_BUS_LINK_MISSING => {
                "inspect-cyw43-sdio-owner-bus-link-resource"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_FORBIDDEN_PRESENT => {
                "inspect-cyw43-resource-authority-window"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RUNTIME_ENTRY => {
                "inspect-linked-cyw43-engine-init-branch-entry"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_ENGINE_INIT_BRANCH => {
                "inspect-cyw43-runtime-state-slot-entry"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_STATE_RESET_BEGIN => {
                "inspect-cyw43-runtime-state-reset"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_STATE_RESET_DONE => {
                "inspect-cyw43-forbidden-sdio-mmio-resource-scan"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_FORBIDDEN_SDIO_MMIO => {
                "remove-cyw43-direct-sdio-mmio-resource"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_BUS_LINK_CHECK_BEGIN => {
                "inspect-cyw43-sdio-owner-bus-link-descriptor"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SHARED_CONTROL_CHECK_BEGIN => {
                "inspect-cyw43-shared-control-resource"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SHARED_CONTROL_MISSING => {
                "publish-cyw43-shared-control-resource"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SHARED_CONTROL_READY => {
                "inspect-cyw43-engine-init-completion-publication"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_TRANSPORT_BEGIN => {
                "verify-cyw43-sdio-owner-bus-link-ready"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_BUS_LINK_READY => {
                "adopt-linked-sdio-card-selected-state"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_ADOPT_BEGIN => {
                "adopt-linked-sdio-card-selected-state"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_HOST_CONFIG_BEGIN => {
                "verify-linked-sdio-startup-host-config-replay"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD0_BEGIN => {
                "verify-linked-sdio-cmd0-go-idle"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD5_OCR_BEGIN => {
                "verify-linked-sdio-cmd5-ocr-discovery"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD5_READY_BEGIN => {
                "verify-linked-sdio-cmd5-ready-ocr"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD3_RCA_BEGIN => {
                "verify-linked-sdio-cmd3-rca"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_CMD7_SELECT_BEGIN => {
                "verify-linked-sdio-cmd7-select"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_SEND_BEGIN => {
                "verify-linked-sdio-owner-endpoint-notification"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_SEND_DONE => {
                "wait-linked-sdio-owner-completion"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_WAIT_BEGIN => {
                "inspect-linked-sdio-owner-command-service"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_WAIT_TIMEOUT => {
                "inspect-sdio-owner-runtime-fault-or-scheduling"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_SDIO_OWNER_REPLY => {
                "continue-cyw43-card-adoption"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CARD_READY => {
                "start-cyw43-f1-block-size-write-and-readback"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_BLOCK_BEGIN => {
                "verify-cyw43-f1-block-size-write-and-readback"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_BLOCK_READY => {
                "start-cyw43-f2-block-size-write-and-readback"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F2_BLOCK_BEGIN => {
                "verify-cyw43-f2-block-size-write-and-readback"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F2_BLOCK_READY => {
                "start-cyw43-f1-ioex-and-iordy"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_ENABLE_BEGIN => {
                "verify-cyw43-f1-ioex-and-iordy"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_F1_ENABLED => {
                "start-sdio-host-startup-clock-replay"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_HOST_CONFIG_BEGIN => {
                "verify-sdio-host-startup-clock-replay"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_HOST_READY => {
                "start-cyw43-backplane-alp-and-window"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_BEGIN => {
                "inspect-cyw43-backplane-alp-and-window"
            }
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_BACKPLANE_READY
            | pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_TRANSPORT_READY => {
                "continue-cyw43-firmware-prep-and-upload"
            }
            _ => "inspect-linked-cyw43-runtime-progress",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_cyw43_fault_gate(detail: u16) -> u8 {
        match detail {
            0x5101 | 0x5102 | 0x5103 | 0x5104 => 6,
            0x5302 | 0x5303 | 0x5308 | 0x5309 | 0x530a => 6,
            0x5310 | 0x5311 | 0x5324..=0x5328 => 2,
            0x5313..=0x5315 => 3,
            0x5316 | 0x5317 | 0x5319 => 4,
            0x531a..=0x5323 => 5,
            0x5329 => 6,
            0x532a..=0x532e => 7,
            _ => 8,
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_runtime_fault_next_action(
        fault: crate::drivers::driver_task_net::Cyw43RuntimeCommandFaultStatus,
    ) -> &'static str {
        if Self::wifi_runtime_fault_is_sdio_card_select(fault) {
            return Self::wifi_cyw43_fault_next_action(fault.detail);
        }
        if Self::wifi_runtime_fault_is_transport_no_reply(fault) {
            return "slice-cyw43-transport-init-and-inspect-nested-sdio-owner";
        }
        if Self::wifi_runtime_fault_is_firmware_stream(fault) {
            return "inspect-cyw43-firmware-shared-payload-and-sdio-owner-transfer";
        }
        Self::wifi_cyw43_fault_next_action(fault.detail)
    }

    #[cfg(feature = "kernel")]
    const fn wifi_cyw43_fault_next_action(detail: u16) -> &'static str {
        match detail {
            0x5101 => "verify-sdio-owner-command-availability",
            0x5102 => "verify-cyw43-to-sdio-descriptor-window",
            0x5103 => "inspect-sdio-owner-cmd53-after-block-and-byte-retries",
            0x5104 => "verify-sdio-host-config-replay",
            0x5302 | 0x5303 | 0x5308 => "inspect-cyw43-firmware-upload",
            0x5309 | 0x530a => "verify-cyw43-command-descriptor-shared-payload-window",
            0x5310..=0x531f => "inspect-cyw43-backplane-window",
            0x5324 => "verify-linked-sdio-cmd0-go-idle",
            0x5325 => "verify-linked-sdio-cmd5-ocr-discovery",
            0x5326 => "verify-linked-sdio-cmd5-ready-ocr",
            0x5327 => "verify-linked-sdio-cmd3-rca",
            0x5328 => "verify-linked-sdio-cmd7-select",
            0x5320..=0x532f => "inspect-sdio-clock-and-card-state",
            _ => "inspect-cyw43-runtime-fault-stage",
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_runtime_fault_is_sdio_card_select(
        fault: crate::drivers::driver_task_net::Cyw43RuntimeCommandFaultStatus,
    ) -> bool {
        if matches!(fault.detail, 0x5324..=0x5328) {
            return true;
        }
        matches!(
            fault.stage,
            "sdio-cmd0-go-idle"
                | "sdio-cmd5-ocr"
                | "sdio-cmd5-ready"
                | "sdio-cmd3-rca"
                | "sdio-cmd7-select"
                | "sdio-cmd7-select-pre-recover"
                | "sdio-cmd7-select-host-recover"
                | "sdio-cmd7-select-r1-fallback"
                | "sdio-cmd7-select-r1-host-recover"
                | "sdio-card-init-restart-host-recover"
        )
    }

    #[cfg(feature = "kernel")]
    const fn wifi_cyw43_card_select_command_label(detail: u16) -> &'static str {
        match detail {
            0x5324 => "cmd0-go-idle",
            0x5325 => "cmd5-ocr",
            0x5326 => "cmd5-ready",
            0x5327 => "cmd3-rca",
            0x5328 => "cmd7-select",
            _ => "unknown",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_cyw43_card_select_attempt(result: u32) -> u8 {
        ((result >> 16) & 0xff) as u8
    }

    #[cfg(feature = "kernel")]
    const fn wifi_cyw43_card_select_low_bits(result: u32) -> u16 {
        (result & 0xffff) as u16
    }

    #[cfg(feature = "kernel")]
    fn wifi_runtime_fault_is_firmware_stream(
        fault: crate::drivers::driver_task_net::Cyw43RuntimeCommandFaultStatus,
    ) -> bool {
        matches!(
            fault.stage,
            "cyw43-firmware-prep" | "cyw43-firmware-chunk" | "cyw43-nvram-chunk"
        ) || matches!(fault.op, 2 | 3)
    }

    #[cfg(feature = "kernel")]
    fn wifi_runtime_fault_is_transport_no_reply(
        fault: crate::drivers::driver_task_net::Cyw43RuntimeCommandFaultStatus,
    ) -> bool {
        fault.op == pi4_driver_abi::DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT
            && fault.detail == 0
            && fault.reason == "cyw43-runtime-command-no-reply"
    }

    #[cfg(feature = "kernel")]
    fn wifi_runtime_fault_implies_hal_power_ready(
        fault: crate::drivers::driver_task_net::Cyw43RuntimeCommandFaultStatus,
    ) -> bool {
        Self::wifi_runtime_fault_is_sdio_card_select(fault)
            || Self::wifi_runtime_fault_is_transport_no_reply(fault)
            || Self::wifi_runtime_fault_is_firmware_stream(fault)
    }

    #[cfg(feature = "kernel")]
    const fn wifi_cyw43_fault_cmd53_function(
        fault: crate::drivers::driver_task_net::Cyw43RuntimeCommandFaultStatus,
    ) -> &'static str {
        match fault.op {
            2 | 3 => "1",
            7 | 8 | 9 | 10 => "2",
            _ => "unknown",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_cyw43_fault_cmd53_increment(
        fault: crate::drivers::driver_task_net::Cyw43RuntimeCommandFaultStatus,
    ) -> &'static str {
        match fault.op {
            2 | 3 => "yes",
            7 | 8 | 9 | 10 => "no",
            _ => "unknown",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_cyw43_fault_cmd53_mode(
        fault: crate::drivers::driver_task_net::Cyw43RuntimeCommandFaultStatus,
    ) -> &'static str {
        match fault.op {
            2 if fault.flags & 1 != 0 => "byte-retry",
            2 => "block-first",
            3 => "byte",
            7 | 8 | 9 | 10 => "fifo-fixed",
            _ => "unknown",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_sdio_owner_mode_label(
        fault: crate::drivers::driver_task_net::Cyw43SdioOwnerFaultStatus,
    ) -> &'static str {
        if fault.cmd != 53 {
            "non-cmd53"
        } else if fault.block_mode {
            "block"
        } else if fault.len == 512 {
            "byte512"
        } else {
            "byte-narrow"
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_cyw43_fault_descriptor_status(detail: u16) -> &'static str {
        match detail {
            0x5103 => "descriptor-transfer-failed",
            0x5102 => "descriptor-unavailable",
            0x5104 => "host-config-failed",
            0x5101 => "runtime-command-unavailable",
            0x5329 => "firmware-retry-exhausted",
            _ => "not-classified",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_sdio_transfer_failure_stage(result: u32) -> &'static str {
        match (result >> 24) & 0xff {
            1 => "inhibit",
            2 => "command",
            3 => "data-wait",
            4 => "data-end",
            5 => "response",
            _ => "unknown",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_sdio_transfer_failure_status(result: u32) -> u32 {
        result & 0x00ff_ffff
    }

    #[cfg(feature = "kernel")]
    const fn wifi_sdio_transfer_failure_r5(result: u32) -> u32 {
        if ((result >> 24) & 0xff) == 5 {
            Self::wifi_sdio_transfer_failure_status(result) & 0xcb00
        } else {
            0
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_startup_blocker_for_gate(gate: u8, exact_error: &str) -> &'static str {
        match gate {
            0 => "none",
            1 => "wifi-power-reset",
            2 => "sdio-card-select",
            3 => "cccr-fbr-ready",
            4 => "ht-clock",
            5 => "backplane-window",
            6 => "firmware-upload",
            7 => "function2-ready",
            8 => {
                if exact_error.is_empty() {
                    "firmware-channel"
                } else if Self::wifi_exact_error_is_join_security_blocker(exact_error) {
                    "host-eapol"
                } else if exact_error.starts_with("cyw43-control-") {
                    "control-exchange"
                } else {
                    "control-plane-exact-error"
                }
            }
            9 => "dhcp-bound",
            10 => "nettest-netstats-cohsh",
            _ => "unknown",
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_startup_gate_name_for_gate(gate: u8, exact_error: &str) -> &'static str {
        match gate {
            1 => "runtime-power-reset",
            2 => "sdio-card-select",
            3 => "cccr-fbr-ready",
            4 => "ht-clock",
            5 => "backplane-window",
            6 => "firmware-upload",
            7 => "function2-ready",
            8 => {
                if exact_error.starts_with("cyw43-control-") {
                    "control-exchange"
                } else if Self::wifi_exact_error_is_join_security_blocker(exact_error) {
                    "host-eapol"
                } else if exact_error.is_empty() {
                    "firmware-channel"
                } else {
                    "control-plane-exact-error"
                }
            }
            9 => "dhcp-bound",
            10 => "nettest-netstats-cohsh",
            _ => "unknown",
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_control_stage_for_exact_error(exact_error: &str) -> &str {
        if exact_error.is_empty() {
            "none"
        } else if Self::wifi_exact_error_is_join_security_blocker(exact_error) {
            "host-eapol"
        } else if exact_error.starts_with("cyw43-control-") {
            exact_error
        } else {
            "firmware-channel"
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_startup_next_action_for_gate(gate: u8, exact_error: &str) -> &'static str {
        match gate {
            0 => "acceptance-complete",
            1 => "verify-linked-runtime-power-reset-resources",
            2 => "verify-sdio-cmd0-cmd5-cmd3-cmd7",
            3 => "verify-cccr-fbr-and-block-size",
            4 => "verify-chipclkcsr-ht-avail",
            5 => "verify-backplane-window-programming",
            6 => "inspect-cyw43-firmware-upload",
            7 => "verify-function2-enable-ready",
            8 => {
                if exact_error.is_empty() {
                    "verify-firmware-channel-first-reply"
                } else if Self::wifi_exact_error_is_join_security_blocker(exact_error) {
                    "complete-host-eapol-handshake"
                } else if exact_error.starts_with("cyw43-control-") {
                    "inspect-cyw43-control-exchange"
                } else {
                    "inspect-control-plane-exact-error"
                }
            }
            9 => "run-dhcp-and-report-lease-state",
            10 => "run-nettest-netstats-and-cohsh",
            _ => "inspect-wifi-startup-gates",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_startup_proof_gate(
        power_ready: bool,
        card_selected: bool,
        f1_ready: bool,
        ht_ready: bool,
        backplane_ready: bool,
        firmware_uploaded: bool,
        f2_ready: bool,
        channel_ready: bool,
        dhcp_pass: bool,
        acceptance_pass: bool,
    ) -> u8 {
        if !power_ready {
            0
        } else if !card_selected {
            1
        } else if !f1_ready {
            2
        } else if !ht_ready {
            3
        } else if !backplane_ready {
            4
        } else if !firmware_uploaded {
            5
        } else if !f2_ready {
            6
        } else if !channel_ready {
            7
        } else if !dhcp_pass {
            8
        } else if !acceptance_pass {
            9
        } else {
            10
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_diag_dhcp_bound(&self) -> bool {
        #[cfg(feature = "net-console")]
        {
            self.net.as_ref().is_some_and(|net| {
                let status = net.status_report();
                status.active_interface == "wifi"
                    && status.address_source == "dhcp-lease"
                    && status.dhcp_phase == "bound"
            })
        }
        #[cfg(not(feature = "net-console"))]
        {
            false
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_diag_network_evidence(&self) -> HeaplessString<DEFAULT_LINE_CAPACITY> {
        #[cfg(feature = "net-console")]
        {
            if let Some(net) = self.net.as_ref() {
                let status = net.status_report();
                return format_message(format_args!(
                    "active={} address_source={} dhcp_phase={} ip={}",
                    status.active_interface, status.address_source, status.dhcp_phase, status.ip,
                ));
            }
        }
        format_message(format_args!(
            "active=none address_source=unavailable dhcp_phase=unavailable ip=unavailable"
        ))
    }

    #[cfg(feature = "kernel")]
    fn wifi_diag_acceptance_evidence(&self) -> HeaplessString<DEFAULT_LINE_CAPACITY> {
        #[cfg(feature = "net-console")]
        {
            if let Some(net) = self.net.as_ref() {
                let status = net.status_report();
                return format_message(format_args!(
                    "nettest=requires-command netstats=requires-command cohsh=requires-nettest backend={}",
                    status.backend,
                ));
            }
        }
        format_message(format_args!(
            "nettest=unavailable netstats=unavailable cohsh=unavailable backend=none"
        ))
    }

    #[cfg(feature = "net-console")]
    fn wifi_credential_warning_line(
        warning: WifiCredentialWarning,
    ) -> HeaplessString<DEFAULT_LINE_CAPACITY> {
        format_message(format_args!(
            "wifi warning: code={} detail={} action={}",
            warning.code, warning.detail, warning.action
        ))
    }

    #[cfg(feature = "net-console")]
    fn emit_wifi_credential_warning_for_status(
        &mut self,
        status: &NetStatusReport,
        stats: &NetCounters,
        force: bool,
    ) {
        if self.wifi_credential_warning_emitted && !force {
            return;
        }
        let Some(warning) = wifi_credential_warning_for_status(
            status,
            stats,
            wifi_runtime_credential_warning_reason(),
        ) else {
            return;
        };
        let line = Self::wifi_credential_warning_line(warning);
        self.emit_console_line(line.as_str());
        self.wifi_credential_warning_emitted = true;
    }

    #[cfg(feature = "net-console")]
    fn emit_wifi_credential_warning_current_before_prompt(&mut self) {
        if self.wifi_credential_warning_emitted {
            return;
        }
        let Some(net) = self.net.as_ref() else {
            return;
        };
        let status = net.status_report();
        let stats = net.stats();
        let Some(warning) = wifi_credential_warning_for_status(
            &status,
            &stats,
            wifi_runtime_credential_warning_reason(),
        ) else {
            return;
        };
        let line = Self::wifi_credential_warning_line(warning);
        self.emit_serial_line(line.as_str());
        if let Some(runtime) = self.local_seat.as_mut() {
            if !runtime.root_console_ready() {
                let _ = runtime.mirror_high_impact_line(line.as_str());
            }
        }
        self.wifi_credential_warning_emitted = true;
    }

    #[cfg(feature = "net-console")]
    fn emit_wifi_network_status(&mut self) {
        let Some(net) = self.net.as_ref() else {
            return;
        };
        let status = net.status_report();
        let stats = net.stats();
        if !net_status_wifi_relevant(&status) {
            return;
        }
        let line = format_message(format_args!(
            "wifi: net backend={} mode={} policy={} active={} standby={} address_source={} dhcp_phase={}",
            status.backend,
            status.mode,
            status.interface_policy,
            status.active_interface,
            status.standby_interface,
            status.address_source,
            status.dhcp_phase,
        ));
        self.emit_console_line(line.as_str());
        self.emit_wifi_credential_warning_for_status(&status, &stats, true);
    }

    #[cfg(feature = "kernel")]
    const fn wifi_snapshot_ht_avail(snapshot: &WifiDebugSnapshot) -> bool {
        match snapshot.chipclkcsr {
            Some(value) => (value & 0x80) != 0,
            None => false,
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_exact_error_is_direct_sdio_transport_blocker(exact_error: &str) -> bool {
        matches!(
            exact_error,
            "sdio-cmd52-write" | "sdio-cmd52-read" | "sdio-cmd53-r5-error"
        ) || exact_error.contains("cmd53-r5")
    }

    #[cfg(feature = "kernel")]
    fn wifi_exact_error_is_join_security_blocker(exact_error: &str) -> bool {
        exact_error.starts_with("cyw43-join-security-")
            || matches!(
                exact_error,
                "firmware-supplicant-unsupported"
                    | "host-eapol-required"
                    | "host-eapol-pending"
                    | "wifi-host-eapol-pending"
                    | "wsec-pmk-bad-argument"
            )
    }

    #[cfg(feature = "kernel")]
    fn wifi_exact_error_is_terminal_diag_blocker(exact_error: &str) -> bool {
        !exact_error.is_empty()
            && (matches!(
                exact_error,
                "cyw43-ht-clock-timeout"
                    | "cyw43-ht-clock-timeout-before-function2"
                    | "cyw43-device-on-timeout-before-ht"
                    | "cyw43-device-on-timeout-before-function2"
            ) || exact_error.starts_with("cyw43-control-plane-")
                || Self::wifi_exact_error_is_join_security_blocker(exact_error)
                || exact_error.starts_with("cyw43-function2-")
                || Self::wifi_exact_error_is_direct_sdio_transport_blocker(exact_error))
    }

    #[cfg(feature = "kernel")]
    fn wifi_join_security_attribution(snapshot: &WifiDebugSnapshot) -> &'static str {
        if snapshot.control_plane_exact_error == "firmware-supplicant-unsupported"
            && Self::wifi_sdhci_read_diag_is_clear(snapshot.control_plane_sdhci_read_diag)
            && snapshot.control_plane_f2_state == "linux-configured"
        {
            "firmware-feature-boundary"
        } else if matches!(
            snapshot.control_plane_exact_error,
            "host-eapol-required" | "host-eapol-pending" | "wifi-host-eapol-pending"
        ) && Self::wifi_sdhci_read_diag_is_clear(snapshot.control_plane_sdhci_read_diag)
            && snapshot.control_plane_f2_state == "linux-configured"
        {
            "host-supplicant-boundary"
        } else if snapshot.control_plane_exact_error == "wsec-pmk-bad-argument"
            && Self::wifi_sdhci_read_diag_is_clear(snapshot.control_plane_sdhci_read_diag)
            && snapshot.control_plane_f2_state == "linux-configured"
        {
            "join-credential-boundary"
        } else {
            "join-security-command-boundary"
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_join_security_transport_label(snapshot: &WifiDebugSnapshot) -> &'static str {
        if Self::wifi_sdhci_read_diag_is_clear(snapshot.control_plane_sdhci_read_diag)
            && snapshot.control_plane_f2_state == "linux-configured"
        {
            "healthy"
        } else {
            "needs-inspection"
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_sdhci_read_diag_is_clear(diag: &str) -> bool {
        diag.is_empty() || matches!(diag.as_bytes(), b"none")
    }

    #[cfg(feature = "kernel")]
    fn wifi_join_security_failing_iovar(snapshot: &WifiDebugSnapshot) -> &'static str {
        match snapshot.control_plane_exact_error {
            "firmware-supplicant-unsupported" => "sup_wpa,bsscfg:sup_wpa",
            "host-eapol-required" | "host-eapol-pending" | "wifi-host-eapol-pending" => "eapol",
            "wsec-pmk-bad-argument" => "WLC_SET_WSEC_PMK",
            "cyw43-join-security-sup-wpa-loop" => "sup_wpa",
            "cyw43-join-security-bsscfg-sup-wpa-loop" => "bsscfg:sup_wpa",
            "cyw43-join-security-wpaie-loop" => "wpaie",
            "cyw43-join-security-wpa-auth-initial-loop"
            | "cyw43-join-security-wpa-auth-final-loop" => "wpa_auth",
            "cyw43-join-security-auth-loop" => "auth",
            "cyw43-join-security-wsec-first-loop" => "wsec",
            _ => "unknown",
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_join_security_status(snapshot: &WifiDebugSnapshot) -> &'static str {
        if snapshot.control_plane_exact_error == "firmware-supplicant-unsupported" {
            "0xffffffe9"
        } else if snapshot.control_plane_exact_error == "host-eapol-required" {
            "host-required"
        } else if matches!(
            snapshot.control_plane_exact_error,
            "host-eapol-pending" | "wifi-host-eapol-pending"
        ) {
            "host-pending"
        } else if snapshot.control_plane_exact_error == "wsec-pmk-bad-argument" {
            "0xfffffffe"
        } else {
            "n/a"
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_diag_should_skip_ht_probe(snapshot: &WifiDebugSnapshot) -> bool {
        Self::wifi_diag_ht_probe_skip_reason(snapshot).is_some()
    }

    #[cfg(feature = "kernel")]
    fn wifi_diag_ht_probe_skip_reason(snapshot: &WifiDebugSnapshot) -> Option<&'static str> {
        if matches!(snapshot.power_state, WifiPowerState::Off)
            || matches!(snapshot.reset_state, WifiResetState::Asserted)
            || !snapshot.card_ready
            || snapshot.current_clock_hz == 0
        {
            return Some("transport-not-initialized");
        }
        if snapshot.debug_snapshot_stage == "cyw43-load-firmware-fail"
            && Self::wifi_exact_error_is_direct_sdio_transport_blocker(
                snapshot.control_plane_exact_error,
            )
        {
            return Some("pre-firmware-core-control-failure");
        }
        (snapshot.debug_snapshot_stage == "cyw43-init-control-plane-fail"
            || Self::wifi_exact_error_is_terminal_diag_blocker(snapshot.control_plane_exact_error))
        .then_some("preserved-control-plane-failure")
    }

    #[cfg(feature = "kernel")]
    fn wifi_capture_verdict(snapshot: &WifiDebugSnapshot) -> (&'static str, &'static str) {
        let exact_error = snapshot.control_plane_exact_error;
        if Self::wifi_exact_error_is_direct_sdio_transport_blocker(exact_error) {
            return ("firmware-core-control-edge", "firmware-core-control");
        }
        if Self::wifi_exact_error_is_join_security_blocker(exact_error) {
            return ("join-security-edge", "join-security");
        }
        if exact_error.is_empty()
            && snapshot.control_plane_no_ht_transport
            && snapshot.control_plane_bootstrap_phase == "startup-link-recovery"
            && snapshot.control_plane_f2_state.starts_with("latched-")
        {
            return ("function2-reply-edge", "first-function2-reply");
        }
        if exact_error
            == "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready"
            || exact_error == "cyw43-control-plane-sideband-read-stall-no-buffer-ready"
        {
            return ("function2-reply-edge", "first-function2-reply");
        }
        if exact_error.starts_with("cyw43-function2-disabled")
            && !Self::wifi_snapshot_ht_avail(snapshot)
        {
            return ("transport-edge", "wait-ht-clock");
        }
        if exact_error.starts_with("cyw43-function2-disabled")
            || exact_error.starts_with("cyw43-function2-enable-latched-not-ready")
            || exact_error.starts_with("cyw43-function2-ready-hidden-from-cccr")
            || exact_error.starts_with("cyw43-function2-ready-unreadable")
        {
            return ("function2-ready-edge", "function2-ready");
        }
        if exact_error.starts_with("cyw43-function2-reply-") {
            return ("function2-reply-edge", "first-function2-reply");
        }
        if exact_error.starts_with("cyw43-function2-interrupt-gated")
            || exact_error.starts_with("cyw43-function2-interrupt-unreadable")
            || exact_error == "cyw43-control-plane-linux-interrupts-deferred"
        {
            return ("interrupt-programming-edge", "function2-interrupts");
        }
        if exact_error.starts_with("cyw43-control-plane-sideband-")
            || exact_error == "cyw43-control-plane-sideband-unreadable"
        {
            return ("function1-sideband-edge", "function1-sideband");
        }
        if matches!(
            exact_error,
            "cyw43-control-plane-passive-startup-link-timeout"
                | "cyw43-control-plane-startup-link-rescue-budget-exhausted"
                | "cyw43-control-plane-pure-f2-startup-link-no-reply"
                | "cyw43-control-plane-state-visible-no-reply"
        ) {
            return ("function2-reply-edge", "first-function2-reply");
        }
        if !snapshot.card_ready {
            return ("sdio-card-edge", "card-select");
        }
        ("transport-edge", "control-plane-bootstrap")
    }

    #[cfg(feature = "kernel")]
    const fn wifi_golden_path_route(snapshot: &WifiDebugSnapshot) -> &'static str {
        if snapshot.control_plane_no_ht_transport {
            "strict-then-bounded-no-ht"
        } else {
            "strict-startup-link"
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_golden_path_state(snapshot: &WifiDebugSnapshot) -> &'static str {
        if snapshot.control_plane_no_ht_transport {
            "fallback-no-ht"
        } else if snapshot.control_plane_startup_link_stable {
            "startup-link-stable"
        } else {
            "primary"
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_golden_path_current_step(snapshot: &WifiDebugSnapshot) -> &'static str {
        let exact_error = snapshot.control_plane_exact_error;
        if Self::wifi_exact_error_is_direct_sdio_transport_blocker(exact_error) {
            return "firmware-core-control";
        }
        if Self::wifi_exact_error_is_join_security_blocker(exact_error) {
            return "join-security";
        }
        if exact_error.is_empty()
            && snapshot.control_plane_no_ht_transport
            && snapshot.control_plane_bootstrap_phase == "startup-link-recovery"
            && snapshot.control_plane_f2_state.starts_with("latched-")
        {
            return "first-function2-reply";
        }
        if exact_error
            == "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready"
            || exact_error == "cyw43-control-plane-sideband-read-stall-no-buffer-ready"
        {
            return "first-function2-reply";
        }
        if exact_error.starts_with("cyw43-function2-disabled")
            && !Self::wifi_snapshot_ht_avail(snapshot)
        {
            return "wait-ht-clock";
        }
        if exact_error.starts_with("cyw43-function2-disabled")
            || exact_error.starts_with("cyw43-function2-enable-latched-not-ready")
            || exact_error.starts_with("cyw43-function2-ready-hidden-from-cccr")
            || exact_error.starts_with("cyw43-function2-ready-unreadable")
        {
            return "function2-ready";
        }
        if exact_error.starts_with("cyw43-function2-reply-") {
            return "first-function2-reply";
        }
        if exact_error.starts_with("cyw43-function2-interrupt-gated")
            || exact_error.starts_with("cyw43-function2-interrupt-unreadable")
            || exact_error == "cyw43-control-plane-linux-interrupts-deferred"
        {
            return "function2-interrupts";
        }
        if exact_error.starts_with("cyw43-control-plane-sideband-")
            || exact_error == "cyw43-control-plane-sideband-unreadable"
        {
            return "function1-sideband";
        }
        if matches!(
            exact_error,
            "cyw43-control-plane-passive-startup-link-timeout"
                | "cyw43-control-plane-startup-link-rescue-budget-exhausted"
                | "cyw43-control-plane-pure-f2-startup-link-no-reply"
                | "cyw43-control-plane-state-visible-no-reply"
        ) {
            return "first-function2-reply";
        }
        if !snapshot.card_ready {
            return "sdio-card-select";
        }
        match snapshot.control_plane_bootstrap_phase {
            "first-write-startup-link" => "setup-firmware-channel",
            "startup-link-recovery" => "startup-link-recovery",
            "steady-state" if snapshot.control_plane_probe_pending => "setup-firmware-channel",
            "steady-state" => "wait-ht-clock",
            _ if snapshot.control_plane_startup_link_stable => "first-function2-reply",
            _ => "wait-ht-clock",
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_golden_path_next_step(snapshot: &WifiDebugSnapshot) -> &'static str {
        match Self::wifi_golden_path_current_step(snapshot) {
            "sdio-card-select" => "enable-function1",
            "wait-ht-clock" => "setup-firmware-channel",
            "firmware-core-control" => "firmware-upload",
            "function2-ready" => "setup-firmware-channel",
            "function2-interrupts" => "mailbox-ready",
            "join-security" => "join-submit",
            "setup-firmware-channel" => "wait-firmware-ready",
            "startup-link-recovery" => "first-function2-reply",
            "function1-sideband" => "first-function2-reply",
            "first-function2-reply" => "promote-link",
            _ => "wait-firmware-ready",
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_reply_contract_path(snapshot: &WifiDebugSnapshot) -> &'static str {
        if snapshot.control_plane_no_ht_transport
            && (snapshot.control_plane_startup_link_stable
                || matches!(
                    snapshot.control_plane_reply_mode,
                    "startup-link" | "startup-link-resume"
                ))
        {
            "startup-link-f2"
        } else if Self::wifi_exact_error_is_join_security_blocker(
            snapshot.control_plane_exact_error,
        ) {
            "join-security"
        } else if snapshot
            .control_plane_exact_error
            .starts_with("cyw43-control-plane-sideband-")
            || snapshot.control_plane_exact_error == "cyw43-control-plane-sideband-unreadable"
        {
            "function1-sideband"
        } else if snapshot
            .control_plane_exact_error
            .starts_with("cyw43-function2-enable-latched-not-ready")
            || snapshot
                .control_plane_exact_error
                .starts_with("cyw43-function2-ready-")
        {
            "function2-ready"
        } else {
            "strict-control-plane"
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_reply_contract_strict_recovery_f2(snapshot: &WifiDebugSnapshot) -> &'static str {
        if snapshot.control_plane_no_ht_transport
            && snapshot.control_plane_f2_state == "latched-linux-configured-no-iorx"
            && (matches!(
                snapshot.control_plane_reply_mode,
                "startup-link" | "startup-link-resume"
            ) || snapshot.control_plane_startup_link_stable)
        {
            "preserve-latch"
        } else {
            "repoll"
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_reply_contract_blocker_class(snapshot: &WifiDebugSnapshot) -> &'static str {
        let exact_error = snapshot.control_plane_exact_error;
        if Self::wifi_exact_error_is_direct_sdio_transport_blocker(exact_error) {
            "firmware-core-control"
        } else if Self::wifi_exact_error_is_join_security_blocker(exact_error) {
            "join-security"
        } else if exact_error.starts_with("cyw43-function2-reply-") {
            "direct-f2-reply"
        } else if exact_error
            == "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready"
            && snapshot.control_plane_f2_state == "latched-linux-configured-no-iorx"
        {
            "f1-sideband"
        } else if exact_error.starts_with("cyw43-control-plane-sideband-")
            || exact_error == "cyw43-control-plane-sideband-unreadable"
        {
            "f1-sideband"
        } else if exact_error.starts_with("cyw43-function2-enable-latched-not-ready")
            || exact_error.starts_with("cyw43-function2-ready-")
        {
            "f2-ready"
        } else if exact_error.starts_with("cyw43-function2-interrupt")
            || exact_error == "cyw43-control-plane-linux-interrupts-deferred"
        {
            "f2-interrupts"
        } else if exact_error.is_empty() {
            "none"
        } else {
            "transport"
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_contract_expected(snapshot: &WifiDebugSnapshot) -> &'static str {
        match Self::wifi_golden_path_current_step(snapshot) {
            "sdio-card-select" => "card-selected-rca",
            "wait-ht-clock" => "chipclkcsr-ht-avail",
            "firmware-core-control" => "f1-backplane-core-control",
            "function2-ready" => "ioex=0x06+iordy=0x06",
            "function2-interrupts" => "linux-f2-interrupts-armed",
            "join-security" => "linux-wpa2-security-order",
            "setup-firmware-channel" => "mailbox-version-readable",
            "startup-link-recovery" => "reply-rearm-complete",
            "function1-sideband" => "f1-sideband-readable",
            "first-function2-reply" => "sdpcm-reply-prefix",
            _ => "firmware-ready",
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_contract_observed(
        snapshot: &WifiDebugSnapshot,
    ) -> HeaplessString<DEFAULT_LINE_CAPACITY> {
        match Self::wifi_golden_path_current_step(snapshot) {
            "sdio-card-select" => format_message(format_args!(
                "card={}+rca=0x{:04x}",
                if snapshot.card_ready { "yes" } else { "no" },
                snapshot.card_rca,
            )),
            "wait-ht-clock" => format_message(format_args!(
                "chipclk={}+clock={}Hz",
                Self::format_optional_u8(snapshot.chipclkcsr),
                snapshot.current_clock_hz,
            )),
            "firmware-core-control" => format_message(format_args!(
                "exact={}+clock={}Hz",
                snapshot.control_plane_exact_error, snapshot.current_clock_hz,
            )),
            "function2-ready" => format_message(format_args!(
                "ioex={}+iordy={}",
                Self::format_optional_u8(snapshot.io_enable),
                Self::format_optional_u8(snapshot.io_ready),
            )),
            "function2-interrupts" => format_message(format_args!(
                "f2_state={}+reply_mode={}",
                snapshot.control_plane_f2_state, snapshot.control_plane_reply_mode,
            )),
            "join-security" => format_message(format_args!(
                "exact={}+sdhci={}",
                snapshot.control_plane_exact_error, snapshot.control_plane_sdhci_read_diag,
            )),
            "setup-firmware-channel" => format_message(format_args!(
                "clock={}Hz+safe_reason={}",
                snapshot.current_clock_hz, snapshot.control_plane_startup_profile_reason,
            )),
            "startup-link-recovery" => format_message(format_args!(
                "reply_mode={}+attempts={}",
                snapshot.control_plane_reply_mode, snapshot.control_plane_reply_attempts,
            )),
            "function1-sideband" => format_message(format_args!(
                "sdhci={}+f2={}",
                snapshot.control_plane_sdhci_read_diag, snapshot.control_plane_f2_state,
            )),
            "first-function2-reply" => format_message(format_args!(
                "reply_mode={}+empty_polls={}",
                snapshot.control_plane_reply_mode, snapshot.control_plane_reply_empty_polls,
            )),
            _ => format_message(format_args!(
                "exact={}+sdhci={}",
                snapshot.control_plane_exact_error, snapshot.control_plane_sdhci_read_diag,
            )),
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_reply_probe_lane(snapshot: &WifiDebugSnapshot) -> &'static str {
        if snapshot.control_plane_no_ht_transport
            && matches!(
                snapshot.control_plane_reply_mode,
                "startup-link" | "startup-link-resume"
            )
        {
            "startup-link"
        } else if snapshot.control_plane_no_ht_transport
            && snapshot.control_plane_startup_link_stable
        {
            "passive-startup-link"
        } else if snapshot.control_plane_no_ht_transport {
            "bounded-no-ht"
        } else {
            "strict"
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_reply_probe_effective_clock_hz(snapshot: &WifiDebugSnapshot) -> u32 {
        if snapshot.control_plane_no_ht_transport
            && matches!(
                snapshot.control_plane_reply_mode,
                "startup-link" | "startup-link-resume"
            )
        {
            snapshot.preferred_data_clock_hz
        } else if snapshot.control_plane_no_ht_transport
            && snapshot.control_plane_startup_link_stable
        {
            snapshot.preferred_data_clock_hz
        } else {
            snapshot.current_clock_hz
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_reply_terminal_action(snapshot: &WifiDebugSnapshot) -> &'static str {
        if matches!(
            Self::wifi_reply_probe_lane(snapshot),
            "startup-link" | "passive-startup-link"
        ) && Self::wifi_reply_contract_strict_recovery_f2(snapshot) == "preserve-latch"
            && matches!(
                Self::wifi_reply_contract_blocker_class(snapshot),
                "direct-f2-reply" | "f1-sideband"
            )
        {
            "fail-fast"
        } else if snapshot.control_plane_startup_link_stable {
            "passive-wait"
        } else if snapshot.control_plane_probe_pending
            || snapshot.control_plane_promoted_probe_pending
        {
            "resume-bounded-probe"
        } else if snapshot.control_plane_reply_attempts != 0 {
            "rearm"
        } else {
            "none"
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_reply_retry_clock_hz(snapshot: &WifiDebugSnapshot) -> u32 {
        if Self::wifi_reply_terminal_action(snapshot) == "fail-fast" {
            snapshot.preferred_data_clock_hz
        } else {
            Self::wifi_reply_probe_effective_clock_hz(snapshot)
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_reply_recovery_nak_sent(snapshot: &WifiDebugSnapshot) -> &'static str {
        if snapshot.control_plane_frame_recovery_policy == Some("linux-rxfail")
            && snapshot.control_plane_frame_recovery_write == Some(false)
        {
            "yes"
        } else {
            "no"
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_reply_rearm_budget(snapshot: &WifiDebugSnapshot) -> &'static str {
        if snapshot.control_plane_no_ht_transport
            && matches!(
                snapshot.control_plane_reply_mode,
                "startup-link" | "startup-link-resume"
            )
        {
            "reply-strict-recovery"
        } else if snapshot.control_plane_no_ht_transport {
            "reply-probe-bypass"
        } else {
            "strict"
        }
    }

    #[cfg(feature = "kernel")]
    fn wifi_reply_rearm_action(snapshot: &WifiDebugSnapshot) -> &'static str {
        if Self::wifi_reply_contract_strict_recovery_f2(snapshot) == "preserve-latch" {
            "skip-function2-ready-repoll"
        } else {
            "force-function2-ready-repoll"
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_power_label(state: WifiPowerState) -> &'static str {
        match state {
            WifiPowerState::Off => "off",
            WifiPowerState::On => "on",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_reset_label(state: WifiResetState) -> &'static str {
        match state {
            WifiResetState::Asserted => "asserted",
            WifiResetState::Deasserted => "deasserted",
        }
    }

    #[cfg(feature = "kernel")]
    const fn wifi_bus_width_label(width: SdioBusWidth) -> &'static str {
        match width {
            SdioBusWidth::OneBit => "1bit",
            SdioBusWidth::FourBit => "4bit",
        }
    }

    const fn yes_no(value: bool) -> &'static str {
        if value {
            "yes"
        } else {
            "no"
        }
    }

    #[cfg(feature = "kernel")]
    fn format_optional_core(value: Option<u8>) -> HeaplessString<16> {
        let mut buf = HeaplessString::new();
        match value {
            Some(value) => {
                let _ = write!(buf, "{value}");
            }
            None => {
                let _ = buf.push_str("n/a");
            }
        }
        buf
    }

    #[cfg(feature = "kernel")]
    fn format_core_slice(values: &[u8]) -> HeaplessString<32> {
        let mut buf = HeaplessString::new();
        if values.is_empty() {
            let _ = buf.push_str("n/a");
            return buf;
        }
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                let _ = buf.push(',');
            }
            let _ = write!(buf, "{value}");
        }
        buf
    }

    #[cfg(feature = "kernel")]
    fn wifi_sleep_bit_label(value: Option<u8>, mask: u8) -> &'static str {
        match value {
            Some(value) if (value & mask) != 0 => "yes",
            Some(_) => "no",
            None => "n/a",
        }
    }

    #[cfg(feature = "kernel")]
    fn format_optional_u8(value: Option<u8>) -> HeaplessString<16> {
        let mut buf = HeaplessString::new();
        match value {
            Some(value) => {
                let _ = write!(buf, "0x{value:02x}");
            }
            None => {
                let _ = buf.push_str("n/a");
            }
        }
        buf
    }

    #[cfg(feature = "kernel")]
    fn format_optional_u32(value: Option<u32>) -> HeaplessString<16> {
        let mut buf = HeaplessString::new();
        match value {
            Some(value) => {
                let _ = write!(buf, "0x{value:08x}");
            }
            None => {
                let _ = buf.push_str("n/a");
            }
        }
        buf
    }

    #[cfg(feature = "kernel")]
    fn format_optional_usize(value: Option<usize>) -> HeaplessString<24> {
        let mut buf = HeaplessString::new();
        match value {
            Some(value) => {
                let _ = write!(buf, "{value}");
            }
            None => {
                let _ = buf.push_str("n/a");
            }
        }
        buf
    }

    #[cfg(feature = "kernel")]
    fn format_optional_usize_hex(value: Option<usize>) -> HeaplessString<32> {
        let mut buf = HeaplessString::new();
        match value {
            Some(value) => {
                let _ = write!(buf, "0x{value:016x}");
            }
            None => {
                let _ = buf.push_str("n/a");
            }
        }
        buf
    }

    #[cfg(feature = "kernel")]
    fn format_optional_u16(value: Option<u16>) -> HeaplessString<16> {
        let mut buf = HeaplessString::new();
        match value {
            Some(value) => {
                let _ = write!(buf, "0x{value:04x}");
            }
            None => {
                let _ = buf.push_str("n/a");
            }
        }
        buf
    }

    #[cfg(feature = "kernel")]
    fn format_optional_fn_addr(value: Option<u32>) -> HeaplessString<16> {
        let mut buf = HeaplessString::new();
        match value {
            Some(value) => {
                let _ = write!(buf, "0x{value:05x}");
            }
            None => {
                let _ = buf.push_str("n/a");
            }
        }
        buf
    }

    fn handle_console_error(&mut self, err: ConsoleError) {
        let message = format_message(format_args!("console error: {}", err));
        self.audit.info(message.as_str());
        match err {
            ConsoleError::RateLimited(delay) => {
                let detail = format_message(format_args!("detail=rate-limited delay_ms={delay}"));
                self.emit_refusal("PARSE", RefusalReason::Quota, Some(detail.as_str()));
            }
            other => {
                let detail = format_message(format_args!("detail={}", other));
                self.emit_refusal("PARSE", RefusalReason::Policy, Some(detail.as_str()));
            }
        }
        if self.parser.clear_buffer() {
            self.audit
                .info("console: cleared partial input after parse error");
        }
        self.local_line.clear();
    }

    fn consume_serial(&mut self) -> bool {
        let mut consumed = false;
        let line_budget = if self.local_seat.is_some() {
            LOCAL_SEAT_SERIAL_LINES_PER_TURN
        } else {
            usize::MAX
        };
        let mut lines = 0usize;
        while lines < line_budget {
            let Some(line) = self.serial.next_line() else {
                break;
            };
            consumed = true;
            lines = lines.saturating_add(1);
            self.last_input_source = ConsoleInputSource::Serial;
            #[cfg(feature = "kernel")]
            crate::serial::emit_serial_input_consume_trace(line.len());
            if !self.local_line.is_empty() {
                self.local_line.clear();
                self.audit
                    .info("console: cleared local-seat input before serial");
            }
            self.process_console_line(&line);
        }
        consumed
    }

    fn record_local_seat_input_for_phase(
        &mut self,
        phase: LocalSeatConsumePhase,
        skip_runtime: bool,
    ) {
        match phase {
            LocalSeatConsumePhase::PreRuntime => {
                self.metrics.local_seat_keyboard_priority_turns = self
                    .metrics
                    .local_seat_keyboard_priority_turns
                    .saturating_add(1);
                if skip_runtime {
                    self.metrics.local_seat_runtime_skipped_turns = self
                        .metrics
                        .local_seat_runtime_skipped_turns
                        .saturating_add(1);
                }
                self.metrics.local_seat_serial_dispatch_yielded_turns = self
                    .metrics
                    .local_seat_serial_dispatch_yielded_turns
                    .saturating_add(1);
            }
            LocalSeatConsumePhase::PriorityFollowup => {}
            LocalSeatConsumePhase::PostRuntime => {
                self.metrics.local_seat_post_runtime_hits =
                    self.metrics.local_seat_post_runtime_hits.saturating_add(1);
                self.metrics.local_seat_serial_dispatch_yielded_turns = self
                    .metrics
                    .local_seat_serial_dispatch_yielded_turns
                    .saturating_add(1);
            }
        }
    }

    fn poll_local_seat_backend_for_ingress(&mut self) {
        if let Some(runtime) = self.local_seat.as_mut() {
            runtime.poll_backend_keyboard();
        }
    }

    fn consume_local_seat(&mut self, phase: LocalSeatConsumePhase, skip_runtime: bool) -> bool {
        let mut chunk = [0u8; KEYBOARD_POLL_CHUNK_BYTES];
        let mut empty_polls = 0usize;
        let mut consumed = false;
        let mut passes = 0usize;
        let mut burst_allowed = self.local_seat.as_ref().is_some_and(|runtime| {
            runtime.keyboard_trace().queued_bytes >= KEYBOARD_POLL_CHUNK_BYTES
        });
        while passes < LOCAL_SEAT_BACKEND_POLL_PASSES_PER_TURN
            || (burst_allowed && passes < LOCAL_SEAT_BURST_DRAIN_PASSES_PER_TURN)
        {
            passes = passes.saturating_add(1);
            self.poll_local_seat_backend_for_ingress();
            let read = match self.local_seat.as_mut() {
                Some(runtime) => runtime.drain_keyboard_bytes(&mut chunk),
                None => return consumed,
            };
            if read == 0 {
                empty_polls = empty_polls.saturating_add(1);
                if empty_polls >= LOCAL_SEAT_EMPTY_POLLS_BEFORE_YIELD {
                    break;
                }
                continue;
            }
            if !consumed {
                self.record_local_seat_input_for_phase(phase, skip_runtime);
            }
            empty_polls = 0;
            consumed = true;
            self.last_input_source = ConsoleInputSource::LocalSeat;
            if let Some(runtime) = self.local_seat.as_mut() {
                runtime.echo_input_bytes(&chunk[..read]);
            }
            if read == KEYBOARD_POLL_CHUNK_BYTES
                || self
                    .local_seat
                    .as_ref()
                    .is_some_and(|runtime| runtime.keyboard_trace().queued_bytes != 0)
            {
                burst_allowed = true;
            }
            if self.serial.clear_partial_line() {
                self.audit
                    .info("console: cleared serial input before local-seat");
            }
            for (index, &byte) in chunk[..read].iter().enumerate() {
                if self.consume_local_seat_escape_byte(byte) {
                    continue;
                }
                match byte {
                    b'\r' => {}
                    b'\n' => {
                        if self.local_line.is_empty() {
                            self.handle_console_error(ConsoleError::EmptyLine);
                        } else {
                            let mut line = HeaplessString::new();
                            core::mem::swap(&mut line, &mut self.local_line);
                            let prior_remainder = self.local_seat_chunk_input_pending;
                            self.local_seat_chunk_input_pending = index + 1 < read;
                            self.process_console_line(&line);
                            self.local_seat_chunk_input_pending = prior_remainder;
                        }
                    }
                    0x08 | 0x7f => {
                        self.local_line.pop();
                    }
                    _ => {
                        let ch = byte as char;
                        if ch.is_control() {
                            continue;
                        }
                        if self.local_line.push(ch).is_err() {
                            self.local_line.clear();
                            self.handle_console_error(ConsoleError::LineTooLong);
                        }
                    }
                }
            }
        }
        consumed
    }

    fn consume_local_seat_escape_byte(&mut self, byte: u8) -> bool {
        match self.local_seat_escape_state {
            LocalSeatEscapeState::Idle if byte == 0x1b => {
                self.local_seat_escape_state = LocalSeatEscapeState::Esc;
                true
            }
            LocalSeatEscapeState::Idle => false,
            LocalSeatEscapeState::Esc if byte == b'[' => {
                self.local_seat_escape_state = LocalSeatEscapeState::Csi;
                true
            }
            LocalSeatEscapeState::Esc => {
                self.local_seat_escape_state = LocalSeatEscapeState::Idle;
                true
            }
            LocalSeatEscapeState::Csi if byte.is_ascii_digit() || byte == b';' => true,
            LocalSeatEscapeState::Csi => {
                self.local_seat_escape_state = LocalSeatEscapeState::Idle;
                true
            }
        }
    }

    fn process_console_line(&mut self, line: &HeaplessString<LINE>) {
        self.metrics.console_lines = self.metrics.console_lines.saturating_add(1);
        let prior_input_turn_active = self.console_input_turn_active;
        let prior_output_budget = self.console_input_turn_output_budget;
        self.console_input_turn_active = self.last_input_source.is_physical_console();
        self.console_input_turn_output_budget = CONSOLE_INPUT_TURN_IMMEDIATE_OUTPUT_LINES;
        #[cfg(feature = "kernel")]
        if self.maybe_handle_usb_debug_line(line.as_str()) {
            if self.last_input_source.is_physical_console() {
                self.emit_prompt();
            }
            self.console_input_turn_active = prior_input_turn_active;
            self.console_input_turn_output_budget = prior_output_budget;
            return;
        }
        #[cfg(feature = "kernel")]
        if self.maybe_handle_wifi_debug_line(line.as_str()) {
            if self.last_input_source.is_physical_console() {
                self.emit_prompt();
            }
            self.console_input_turn_active = prior_input_turn_active;
            self.console_input_turn_output_budget = prior_output_budget;
            return;
        }
        if let Err(err) = self.feed_parser(line) {
            self.handle_console_error(err);
        }
        if self.last_input_source.is_physical_console() && !self.reboot_pending {
            self.emit_prompt();
        }
        self.console_input_turn_active = prior_input_turn_active;
        self.console_input_turn_output_budget = prior_output_budget;
    }

    fn feed_parser(&mut self, line: &HeaplessString<LINE>) -> Result<(), ConsoleError> {
        for byte in line.as_bytes() {
            self.parser.push_byte(*byte)?;
        }
        if let Some(command) = self.parser.push_byte(b'\n')? {
            match self.handle_command(command) {
                Ok(()) => {}
                Err(err) => {
                    #[cfg(feature = "kernel")]
                    self.handle_dispatch_error(err);
                    #[cfg(not(feature = "kernel"))]
                    match err {}
                }
            }
        }
        Ok(())
    }

    #[cfg(feature = "net-console")]
    fn handle_network_line(&mut self, line: HeaplessString<DEFAULT_LINE_CAPACITY>) {
        let mut converted: HeaplessString<LINE> = HeaplessString::new();
        if converted.push_str(line.as_str()).is_err() {
            self.audit
                .denied("net console line exceeded maximum length");
            return;
        }
        self.last_input_source = ConsoleInputSource::Net;
        self.process_console_line(&converted);
    }

    #[cfg(feature = "net-console")]
    fn drain_net_console_events(&mut self) {
        let session_is_net = matches!(self.session_origin, Some(ConsoleInputSource::Net));
        let mut end_reason: Option<NetConsoleDisconnectReason> = None;
        if let Some(net) = self.net.as_mut() {
            net.drain_console_events(&mut |event| match event {
                NetConsoleEvent::Connected { conn_id, peer } => match peer {
                    Some(remote) => {
                        log::info!(
                            target: "net-console",
                            "[net-console] conn {}: tcp-established auth=pending from {}",
                            conn_id,
                            remote
                        );
                    }
                    None => {
                        log::info!(
                            target: "net-console",
                            "[net-console] conn {}: tcp-established auth=pending",
                            conn_id
                        );
                    }
                },
                NetConsoleEvent::Authenticated { conn_id } => {
                    log::info!(
                        target: "net-console",
                        "[net-console] conn {}: authenticated",
                        conn_id
                    );
                }
                NetConsoleEvent::Disconnected {
                    conn_id,
                    reason,
                    bytes_read,
                    bytes_written,
                } => {
                    log::info!(
                        target: "net-console",
                        "[net-console] conn {}: closed reason={} (bytes_read={}, bytes_written={})",
                        conn_id,
                        reason.as_str(),
                        bytes_read,
                        bytes_written,
                    );
                    if session_is_net && end_reason.is_none() {
                        end_reason = Some(reason);
                    }
                }
            });
        }
        if session_is_net {
            if let Some(reason) = end_reason {
                let reason_label = Self::disconnect_reason_label(reason);
                self.end_session(reason_label);
            }
        }
    }

    #[inline(never)]
    pub(crate) fn handle_command(&mut self, command: Command) -> Result<(), CommandDispatchError> {
        #[cfg(feature = "kernel")]
        let command_clone = command.clone();
        #[cfg(feature = "kernel")]
        let mut forwarded = false;
        let verb_label = command.verb().ack_label();
        if matches!(self.session, Some(SessionRole::Queen)) {
            lifecycle::root_record_activity(self.now_ms);
        }
        let audit_net = matches!(self.last_input_source, ConsoleInputSource::Net);
        let conn_id = if audit_net {
            self.active_tcp_conn_id()
        } else {
            0
        };
        let start_sid = self.session_id.unwrap_or(0);
        let mut cmd_status = "ok";
        let term = if matches!(command, Command::Quit) {
            "EOF"
        } else {
            "END"
        };
        if audit_net {
            self.audit_tcp_cmd_begin(conn_id, start_sid, verb_label);
        }
        #[cfg(feature = "kernel")]
        let mut result: Result<(), CommandDispatchError> = Ok(());
        #[cfg(not(feature = "kernel"))]
        let result: Result<(), CommandDispatchError> = Ok(());
        match command {
            Command::Help => {
                self.audit.info("console: help");
                self.metrics.accepted_commands += 1;
                self.emit_help();
                self.emit_ack_ok(verb_label, None);
            }
            Command::BootInfo => {
                if self.emit_bootinfo() {
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok(verb_label, None);
                } else {
                    self.metrics.denied_commands += 1;
                    cmd_status = "err";
                    self.emit_refusal(
                        verb_label,
                        RefusalReason::Policy,
                        Some("detail=unavailable"),
                    );
                }
            }
            Command::Caps => {
                if self.emit_caps() {
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok(verb_label, None);
                } else {
                    self.metrics.denied_commands += 1;
                    cmd_status = "err";
                    self.emit_refusal(
                        verb_label,
                        RefusalReason::Policy,
                        Some("detail=unavailable"),
                    );
                }
            }
            Command::Smp { mode } => {
                if let Some(detail) = self.emit_smp(mode) {
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok(verb_label, Some(detail));
                } else {
                    self.metrics.denied_commands += 1;
                    cmd_status = "err";
                    self.emit_refusal(
                        verb_label,
                        RefusalReason::Policy,
                        Some("detail=unsupported"),
                    );
                }
            }
            Command::Mem => {
                if self.emit_mem() {
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok(verb_label, None);
                } else {
                    self.metrics.denied_commands += 1;
                    cmd_status = "err";
                    self.emit_refusal(
                        verb_label,
                        RefusalReason::Policy,
                        Some("detail=unavailable"),
                    );
                }
            }
            Command::CacheLog { count } => {
                let count = usize::from(count.unwrap_or(64));
                #[cfg(all(feature = "kernel", target_os = "none"))]
                {
                    self.emit_cache_log(count);
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok(verb_label, None);
                }
                #[cfg(not(all(feature = "kernel", target_os = "none")))]
                {
                    let _ = count;
                    self.metrics.denied_commands += 1;
                    cmd_status = "err";
                    self.emit_refusal(
                        verb_label,
                        RefusalReason::Policy,
                        Some("detail=unsupported"),
                    );
                }
            }
            Command::Ping => {
                self.audit.info("console: ping");
                self.metrics.accepted_commands += 1;
                self.emit_console_line("PONG");
                self.emit_ack_ok(verb_label, Some("reply=pong"));
            }
            Command::Test => {
                self.audit.info("console: test rejected (host-only)");
                self.metrics.denied_commands += 1;
                cmd_status = "err";
                self.emit_refusal(verb_label, RefusalReason::Policy, Some("detail=host-only"));
            }
            Command::NetTest => {
                #[cfg(feature = "net-console")]
                {
                    if let Some(net) = self.net.as_mut() {
                        match net.start_self_test(self.now_ms) {
                            NetSelfTestStartResult::Started => {
                                self.metrics.accepted_commands += 1;
                                self.emit_console_line("[net-selftest] triggered");
                                self.emit_ack_ok(verb_label, Some("detail=started"));
                            }
                            result => {
                                self.metrics.denied_commands += 1;
                                cmd_status = "err";
                                self.emit_refusal(
                                    verb_label,
                                    RefusalReason::Policy,
                                    result.refusal_detail(),
                                );
                            }
                        }
                    } else {
                        self.metrics.denied_commands += 1;
                        cmd_status = "err";
                        let detail = self.net_disabled_refusal_detail();
                        self.emit_refusal(verb_label, RefusalReason::Policy, Some(detail.as_str()));
                    }
                }
                #[cfg(not(feature = "net-console"))]
                {
                    self.metrics.denied_commands += 1;
                    cmd_status = "err";
                    self.emit_refusal(
                        verb_label,
                        RefusalReason::Policy,
                        Some("detail=net-disabled"),
                    );
                }
            }
            Command::NetStats => {
                #[cfg(feature = "net-console")]
                {
                    if let Some(net) = self.net.as_mut() {
                        let stats = net.stats();
                        let report = net.self_test_report();
                        let status = net.status_report();
                        let line_one = format_message(format_args!(
                            "netstats: rx_pkts={} tx_pkts={} rx_used={} tx_used={} polls={}",
                            stats.rx_packets,
                            stats.tx_packets,
                            stats.rx_used_advances,
                            stats.tx_used_advances,
                            stats.smoltcp_polls
                        ));
                        let line_two = format_message(format_args!(
                            "netstats: udp_rx={} udp_tx={} tcp_accepts={} tcp_auth={} tcp_rx_bytes={} tcp_recv_ready={} tcp_recv_budget_hits={} tcp_tx_bytes={}",
                            stats.udp_rx,
                            stats.udp_tx,
                            stats.tcp_accepts,
                            stats.tcp_auth_sessions,
                            stats.tcp_rx_bytes,
                            stats.tcp_console_recv_ready,
                            stats.tcp_console_recv_budget_hits,
                            stats.tcp_tx_bytes
                        ));
                        let line_three = format_message(format_args!(
                            "netstats: tcp_smoke_out={} tcp_smoke_out_failures={}",
                            stats.tcp_smoke_outbound, stats.tcp_smoke_outbound_failures
                        ));
                        let line_flush = format_message(format_args!(
                            "netstats: tcp_post_flush_polls={} tcp_post_flush_exhaustions={}",
                            self.metrics.net_post_dispatch_flush_polls,
                            self.metrics.net_post_dispatch_flush_exhaustions,
                        ));
                        let line_local_seat = if let Some(runtime) = self.local_seat.as_ref() {
                            let display = runtime.display_trace();
                            format_message(format_args!(
                                "netstats: local_seat_net_mirror={} local_seat_net_mirror_suppressed={} hdmi_pending_bytes={} hdmi_pending_redraw={} hdmi_no_reply={} hdmi_deferred={} hdmi_submitted={}",
                                self.metrics.local_seat_net_mirror_lines,
                                self.metrics.local_seat_net_mirror_suppressed,
                                display.pending_bytes,
                                Self::yes_no(display.pending_redraw),
                                display.no_reply_frames,
                                display.deferred_frames,
                                display.submitted_frames,
                            ))
                        } else {
                            format_message(format_args!(
                                "netstats: local_seat_net_mirror={} local_seat_net_mirror_suppressed={} hdmi=unavailable",
                                self.metrics.local_seat_net_mirror_lines,
                                self.metrics.local_seat_net_mirror_suppressed,
                            ))
                        };
                        let line_four = format_message(format_args!(
                            "netstats: tx_submit={} tx_complete={} tx_free={} tx_in_flight={} tx_double_submit={} tx_zero_len_attempt={} arp_rx={} arp_tx={}",
                            stats.tx_submit,
                            stats.tx_complete,
                            stats.tx_free,
                            stats.tx_in_flight,
                            stats.tx_double_submit,
                            stats.tx_zero_len_attempt,
                            stats.arp_rx,
                            stats.arp_tx,
                        ));
                        let line_five = format_message(format_args!(
                            "netstats: mode={} policy={} active={} standby={} addr_src={} ip={} gateway={} dhcp={}",
                            status.mode,
                            status.interface_policy,
                            status.active_interface,
                            status.standby_interface,
                            status.address_source,
                            status.ip,
                            status.gateway,
                            status.dhcp_phase
                        ));
                        let line_wifi = format_message(format_args!(
                            "netstats: wifi_assoc={} wifi_link={} eapol_rx={} eapol_start={} eapol_secure={} wifi_rxq_cur={} wifi_rxq_hwm={} wifi_rxq_drops={} wifi_runtime_rxq_cur={} wifi_runtime_rxq_hwm={} wifi_runtime_rxq_ovf={} wifi_runtime_rxq_max_drain={} wifi_runtime_rxq_drain_hit={}",
                            stats.wifi_assoc,
                            stats.wifi_link_up,
                            stats.wifi_host_eapol_rx,
                            stats.wifi_host_eapol_start,
                            stats.wifi_host_eapol_secure,
                            stats.wifi_rx_pending_queue_count,
                            stats.wifi_rx_pending_queue_high_water,
                            stats.wifi_rx_pending_drops,
                            stats.wifi_rx_runtime_queue_count,
                            stats.wifi_rx_runtime_queue_high_water,
                            stats.wifi_rx_runtime_queue_overflow_seen,
                            stats.wifi_rx_runtime_max_drained_per_turn,
                            stats.wifi_rx_runtime_drain_budget_hit,
                        ));
                        let line_wired = format_message(format_args!(
                            "netstats: genet_rx_hw={} genet_rx_last_len={} genet_rx_last_ethertype=0x{:04x}",
                            stats.rx_used_advances,
                            stats.driver_rx_last_len,
                            stats.driver_rx_last_ethertype,
                        ));
                        let line_wired_rxq = format_message(format_args!(
                            "netstats: genet_rxq runtime_cur={} runtime_hwm={} runtime_ovf={} runtime_max_drain={} runtime_drain_hit={} runtime_byte_hit={} root_cur={} root_hwm={} root_drops={}",
                            stats.genet_rx_runtime_queue_count,
                            stats.genet_rx_runtime_queue_high_water,
                            stats.genet_rx_runtime_queue_overflow_seen,
                            stats.genet_rx_runtime_max_drained_per_turn,
                            stats.genet_rx_runtime_drain_budget_hit,
                            stats.genet_rx_runtime_byte_budget_hit,
                            stats.genet_rx_pending_queue_count,
                            stats.genet_rx_pending_queue_high_water,
                            stats.genet_rx_pending_drops,
                        ));
                        let line_six = format_message(format_args!(
                            "netstatus: ip={} gateway={} src={} dhcp={}",
                            status.ip, status.gateway, status.address_source, status.dhcp_phase
                        ));
                        let status_line = format_message(format_args!(
                            "nettest: backend={} enabled={} running={} udp={} tcp={} last={:?}",
                            report.backend,
                            report.enabled,
                            report.running,
                            report.udp_target,
                            report.tcp_target,
                            report.last_result
                        ));
                        self.emit_console_line(line_one.as_str());
                        self.emit_console_line(line_two.as_str());
                        self.emit_console_line(line_three.as_str());
                        self.emit_console_line(line_flush.as_str());
                        self.emit_console_line(line_local_seat.as_str());
                        self.emit_console_line(line_four.as_str());
                        self.emit_console_line(line_five.as_str());
                        if net_status_active_interface_is_wifi(&status) {
                            self.emit_console_line(line_wifi.as_str());
                            self.emit_wifi_credential_warning_for_status(&status, &stats, true);
                        }
                        if net_status_active_interface_is_wired(&status) {
                            self.emit_console_line(line_wired.as_str());
                            self.emit_console_line(line_wired_rxq.as_str());
                        }
                        self.emit_console_line(line_six.as_str());
                        self.emit_console_line(status_line.as_str());
                        self.metrics.accepted_commands += 1;
                        self.emit_ack_ok(verb_label, None);
                    } else {
                        self.metrics.denied_commands += 1;
                        cmd_status = "err";
                        let detail = self.net_disabled_refusal_detail();
                        self.emit_refusal(verb_label, RefusalReason::Policy, Some(detail.as_str()));
                    }
                }
                #[cfg(not(feature = "net-console"))]
                {
                    self.metrics.denied_commands += 1;
                    cmd_status = "err";
                    self.emit_refusal(
                        verb_label,
                        RefusalReason::Policy,
                        Some("detail=net-disabled"),
                    );
                }
            }
            Command::Reboot => {
                if self.ensure_reboot_authorized(verb_label) {
                    if crate::reboot::backend_available() {
                        self.audit.info("console: reboot scheduled");
                        self.metrics.accepted_commands += 1;
                        self.reboot_pending = true;
                        self.reboot_flush_turns = REBOOT_ACK_FLUSH_TURNS;
                        self.emit_ack_ok(verb_label, Some("detail=scheduled"));
                        #[cfg(feature = "net-console")]
                        if self.last_input_source == ConsoleInputSource::Net {
                            if let Some(net) = self.net.as_mut() {
                                net.request_disconnect();
                            }
                        }
                    } else {
                        self.metrics.denied_commands += 1;
                        self.audit.denied("reboot denied: backend unavailable");
                        cmd_status = "err";
                        self.emit_refusal(
                            verb_label,
                            RefusalReason::Policy,
                            Some("detail=reboot-backend-unavailable"),
                        );
                    }
                } else {
                    cmd_status = "err";
                }
            }
            Command::Quit => {
                self.audit.info("console: quit");
                self.metrics.accepted_commands += 1;
                self.emit_ack_ok(verb_label, None);
                #[cfg(feature = "net-console")]
                if self.last_input_source == ConsoleInputSource::Net {
                    if let Some(net) = self.net.as_mut() {
                        net.request_disconnect();
                    }
                }
                self.end_session("quit");
            }
            Command::Attach { role, ticket } => {
                let attached = self.handle_attach(role, ticket);
                if !attached {
                    cmd_status = "err";
                }
                #[cfg(feature = "kernel")]
                {
                    forwarded = matches!(self.session, Some(_));
                }
            }
            Command::Tail { path, lines } => {
                if self.ensure_worker_session(verb_label) {
                    let path_str = path.as_str();
                    if let Err(denial) = self.check_ticket_scope(path_str, TicketVerb::Read) {
                        self.record_ticket_denial(path_str, TicketVerb::Read, denial);
                        self.emit_ticket_denied(verb_label, Some(path_str), denial);
                        cmd_status = "err";
                    } else {
                        #[cfg(feature = "kernel")]
                        let mut stream_bytes = 0u64;
                        #[cfg(not(feature = "kernel"))]
                        let stream_bytes = 0u64;
                        #[cfg(feature = "kernel")]
                        let mut path_supported = false;
                        #[cfg(feature = "kernel")]
                        let mut tail_error: Option<NineDoorBridgeError> = None;
                        #[cfg(feature = "kernel")]
                        let mut cursor_meta: Option<TelemetryTailMeta> = None;
                        #[cfg(feature = "kernel")]
                        {
                            let cursor_offset = self.ticket_cursor_offset(path_str).unwrap_or(0);
                            if path_str == "/log/queen.log" {
                                stream_bytes = self.prepare_log_tail_pending_stream(lines);
                                path_supported = true;
                            } else if path_str == "/proc/ingest/watch" {
                                if let Some(bridge) = self.ninedoor.as_mut() {
                                    let bytes = {
                                        let pending = self
                                            .pending_stream
                                            .get_or_insert_with(PendingStream::new);
                                        pending.reset();
                                        if bridge
                                            .ingest_watch_lines_into(
                                                self.now_ms,
                                                &mut *self.audit,
                                                &mut pending.lines,
                                            )
                                            .is_err()
                                        {
                                            pending.lines.clear();
                                        }
                                        pending.lines.iter().map(|line| line.len() as u64).sum()
                                    };
                                    stream_bytes = bytes;
                                    path_supported = true;
                                }
                            } else if let Some(bridge) = self.ninedoor.as_mut() {
                                let (bytes, meta, err) = {
                                    let pending =
                                        self.pending_stream.get_or_insert_with(PendingStream::new);
                                    pending.reset();
                                    match bridge.telemetry_tail_into(
                                        path_str,
                                        cursor_offset,
                                        &mut pending.lines,
                                    ) {
                                        Ok(Some(meta)) => {
                                            let bytes = pending
                                                .lines
                                                .iter()
                                                .map(|line| line.len() as u64)
                                                .sum();
                                            (bytes, Some(meta), None)
                                        }
                                        Ok(None) => (0, None, None),
                                        Err(err) => (0, None, Some(err)),
                                    }
                                };
                                if let Some(err) = err {
                                    tail_error = Some(err);
                                } else if let Some(meta) = meta {
                                    stream_bytes = bytes;
                                    cursor_meta = Some(meta);
                                    path_supported = true;
                                }
                            }
                        }
                        #[cfg(feature = "kernel")]
                        if let Some(err) = tail_error {
                            cmd_status = "err";
                            let sid = self.session_id.unwrap_or(0);
                            let err_msg = format_message(format_args!("{err}"));
                            self.audit_ninedoor_err(sid, "TAIL", path_str, err_msg.as_str());
                            self.emit_ninedoor_refusal(verb_label, Some(path_str), &err);
                        }
                        #[cfg(feature = "kernel")]
                        if cmd_status != "err" && !path_supported {
                            cmd_status = "err";
                            let detail = format_message(format_args!(
                                "detail=invalid-path path={}",
                                path_str
                            ));
                            self.emit_refusal(
                                verb_label,
                                RefusalReason::Policy,
                                Some(detail.as_str()),
                            );
                        }
                        if cmd_status != "err" {
                            if let Err(denial) = self.check_ticket_bandwidth(stream_bytes) {
                                self.record_ticket_denial(path_str, TicketVerb::Read, denial);
                                self.emit_ticket_denied(verb_label, Some(path_str), denial);
                                cmd_status = "err";
                            } else {
                                #[cfg(feature = "kernel")]
                                {
                                    let mut cursor_check: Option<CursorCheck> = None;
                                    if let Some(meta) = cursor_meta {
                                        match self.check_ticket_cursor(path_str, meta.start_offset)
                                        {
                                            Ok(check) => {
                                                cursor_check = check;
                                            }
                                            Err(denial) => {
                                                self.record_ticket_denial(
                                                    path_str,
                                                    TicketVerb::Read,
                                                    denial,
                                                );
                                                self.emit_ticket_denied(
                                                    verb_label,
                                                    Some(path_str),
                                                    denial,
                                                );
                                                cmd_status = "err";
                                            }
                                        }
                                    }
                                    if cmd_status != "err" {
                                        if let Some(pending) = self.pending_stream.as_mut() {
                                            pending.next_line = 0;
                                            pending.bandwidth_bytes = stream_bytes;
                                            pending.cursor = match (cursor_meta, cursor_check) {
                                                (Some(meta), Some(check)) => Some(PendingCursor {
                                                    path_key: path_str.to_owned(),
                                                    offset: meta.start_offset,
                                                    len: meta.consumed_bytes,
                                                    check,
                                                }),
                                                _ => None,
                                            };
                                        }
                                    }
                                }
                                if cmd_status != "err" {
                                    let message =
                                        format_message(format_args!("console: tail {}", path_str));
                                    self.audit.info(message.as_str());
                                    self.metrics.accepted_commands += 1;
                                    self.metrics.ui_reads = self.metrics.ui_reads.saturating_add(1);
                                    let detail = format_message(format_args!("path={}", path_str));
                                    self.emit_ack_ok(verb_label, Some(detail.as_str()));
                                    self.stream_end_pending = true;
                                    self.tail_active = true;
                                    let sid = self.session_id.unwrap_or(0);
                                    self.audit_tail_start(sid, path_str);
                                    #[cfg(feature = "kernel")]
                                    {
                                        forwarded = true;
                                    }
                                }
                            }
                        }
                    }
                } else {
                    cmd_status = "err";
                }
            }
            Command::Cat { path } => {
                if self.ensure_worker_session(verb_label) {
                    let path_str = path.as_str();
                    if let Err(denial) = self.check_ticket_scope(path_str, TicketVerb::Read) {
                        self.record_ticket_denial(path_str, TicketVerb::Read, denial);
                        self.emit_ticket_denied(verb_label, Some(path_str), denial);
                        cmd_status = "err";
                    } else {
                        let message = format_message(format_args!("console: cat {}", path_str));
                        self.audit.info(message.as_str());
                        self.metrics.accepted_commands += 1;
                        #[cfg(feature = "kernel")]
                        {
                            if let Some(bridge_ref) = self.ninedoor.as_mut() {
                                let log_path = path_str == "/log/queen.log";
                                let (data_bytes, cat_err) = {
                                    let pending =
                                        self.pending_stream.get_or_insert_with(PendingStream::new);
                                    pending.reset();
                                    if log_path {
                                        let mut cursor = log_buffer::export_cursor();
                                        let data_bytes = cursor.bytes();
                                        let exhausted = log_buffer::read_cursor_lines_into(
                                            &mut cursor,
                                            &mut pending.lines,
                                        );
                                        pending.log_cursor =
                                            if exhausted { None } else { Some(cursor) };
                                        (data_bytes, None)
                                    } else {
                                        match bridge_ref.cat_into(path_str, &mut pending.lines) {
                                            Ok(()) => {
                                                let data_bytes = pending
                                                    .lines
                                                    .iter()
                                                    .map(|line| line.len() as u64)
                                                    .sum();
                                                (data_bytes, None)
                                            }
                                            Err(err) => (0, Some(err)),
                                        }
                                    }
                                };
                                if let Some(err) = cat_err {
                                    cmd_status = "err";
                                    let sid = self.session_id.unwrap_or(0);
                                    let err_msg = format_message(format_args!("{err}"));
                                    self.audit_ninedoor_err(sid, "CAT", path_str, err_msg.as_str());
                                    self.emit_ninedoor_refusal(verb_label, Some(path_str), &err);
                                } else {
                                    let stream_bytes = if log_path { 0 } else { data_bytes };
                                    if let Err(denial) = self.check_ticket_bandwidth(stream_bytes) {
                                        self.record_ticket_denial(
                                            path_str,
                                            TicketVerb::Read,
                                            denial,
                                        );
                                        self.emit_ticket_denied(verb_label, Some(path_str), denial);
                                        if let Some(pending) = self.pending_stream.as_mut() {
                                            pending.lines.clear();
                                        }
                                        cmd_status = "err";
                                    } else {
                                        let cursor_check = match self
                                            .check_ticket_cursor(path_str, 0)
                                        {
                                            Ok(check) => check,
                                            Err(denial) => {
                                                self.record_ticket_denial(
                                                    path_str,
                                                    TicketVerb::Read,
                                                    denial,
                                                );
                                                self.emit_ticket_denied(
                                                    verb_label,
                                                    Some(path_str),
                                                    denial,
                                                );
                                                if let Some(pending) = self.pending_stream.as_mut()
                                                {
                                                    pending.lines.clear();
                                                }
                                                cmd_status = "err";
                                                None
                                            }
                                        };
                                        if cmd_status != "err" {
                                            let summary = {
                                                // Prefer user echo lines while also surfacing newer audit entries.
                                                // Keep references to avoid a large stack copy for /log/queen.log.
                                                let pending_lines = self
                                                    .pending_stream
                                                    .as_ref()
                                                    .map(|pending| pending.lines.as_slice())
                                                    .unwrap_or(&[]);
                                                let user_lines: HeaplessVec<
                                                    HeaplessString<DEFAULT_LINE_CAPACITY>,
                                                    { log_buffer::LOG_USER_SNAPSHOT_LINES },
                                                > = if log_path {
                                                    log_buffer::snapshot_user_lines::<
                                                        DEFAULT_LINE_CAPACITY,
                                                        { log_buffer::LOG_USER_SNAPSHOT_LINES },
                                                    >(
                                                    )
                                                } else {
                                                    HeaplessVec::new()
                                                };
                                                let mut summary_refs: HeaplessVec<
                                                    &str,
                                                    {
                                                        log_buffer::LOG_SNAPSHOT_LINES
                                                            + log_buffer::LOG_USER_SNAPSHOT_LINES
                                                    },
                                                > = HeaplessVec::new();
                                                if log_path {
                                                    if user_lines.is_empty() {
                                                        for line in pending_lines.iter() {
                                                            if summary_refs
                                                                .push(line.as_str())
                                                                .is_err()
                                                            {
                                                                break;
                                                            }
                                                        }
                                                    } else {
                                                        for user_line in user_lines.iter() {
                                                            if summary_refs
                                                                .push(user_line.as_str())
                                                                .is_err()
                                                            {
                                                                break;
                                                            }
                                                        }
                                                        let mut last_user_idx: Option<usize> = None;
                                                        for (idx, line) in
                                                            pending_lines.iter().enumerate()
                                                        {
                                                            if user_lines.iter().any(|user_line| {
                                                                user_line.as_str() == line.as_str()
                                                            }) {
                                                                last_user_idx = Some(idx);
                                                            }
                                                        }
                                                        let start = last_user_idx
                                                            .map(|idx| idx + 1)
                                                            .unwrap_or(0);
                                                        for line in pending_lines.iter().skip(start)
                                                        {
                                                            if line.as_str().starts_with('[') {
                                                                continue;
                                                            }
                                                            if user_lines.iter().any(|user_line| {
                                                                user_line.as_str() == line.as_str()
                                                            }) {
                                                                continue;
                                                            }
                                                            if summary_refs
                                                                .push(line.as_str())
                                                                .is_err()
                                                            {
                                                                break;
                                                            }
                                                        }
                                                    }
                                                } else {
                                                    for line in pending_lines.iter() {
                                                        if summary_refs.push(line.as_str()).is_err()
                                                        {
                                                            break;
                                                        }
                                                    }
                                                }
                                                let summary_lines: &[&str] =
                                                    summary_refs.as_slice();
                                                let mut summary: HeaplessString<128> =
                                                    HeaplessString::new();
                                                let mut selected: HeaplessVec<
                                                    usize,
                                                    { log_buffer::LOG_SNAPSHOT_LINES },
                                                > = HeaplessVec::new();
                                                let mut total_len = 0usize;
                                                let max_line_len = summary.capacity() / 2;
                                                let mut prefer_user_lines = true;
                                                for _pass in 0..2 {
                                                    for (idx, line) in
                                                        summary_lines.iter().enumerate().rev()
                                                    {
                                                        if prefer_user_lines
                                                            && line.starts_with('[')
                                                        {
                                                            continue;
                                                        }
                                                        let line_len = line.len();
                                                        if line_len > max_line_len {
                                                            continue;
                                                        }
                                                        let sep =
                                                            if total_len == 0 { 0 } else { 1 };
                                                        if line_len
                                                            .saturating_add(sep)
                                                            .saturating_add(total_len)
                                                            > summary.capacity()
                                                        {
                                                            continue;
                                                        }
                                                        total_len = total_len
                                                            .saturating_add(line_len)
                                                            .saturating_add(sep);
                                                        if selected.push(idx).is_err() {
                                                            break;
                                                        }
                                                    }
                                                    if !selected.is_empty() || !prefer_user_lines {
                                                        break;
                                                    }
                                                    selected.clear();
                                                    total_len = 0;
                                                    prefer_user_lines = false;
                                                }
                                                if selected.is_empty() && !summary_lines.is_empty()
                                                {
                                                    if let Some(line) = summary_lines.last() {
                                                        for ch in line.chars() {
                                                            if summary.push(ch).is_err() {
                                                                break;
                                                            }
                                                        }
                                                    }
                                                } else {
                                                    for (pos, idx) in
                                                        selected.iter().rev().enumerate()
                                                    {
                                                        if pos > 0 {
                                                            if summary.push('|').is_err() {
                                                                break;
                                                            }
                                                        }
                                                        if let Some(line) = summary_lines.get(*idx)
                                                        {
                                                            if summary.push_str(line).is_err() {
                                                                break;
                                                            }
                                                        }
                                                    }
                                                }
                                                if path_str.starts_with("/updates/")
                                                    || path_str.starts_with("/models/")
                                                {
                                                    if let Some(line) = summary_lines.first() {
                                                        if line.starts_with("b64:") {
                                                            summary.clear();
                                                            for ch in line.chars() {
                                                                if summary.push(ch).is_err() {
                                                                    break;
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                let max_summary_len = 128usize.saturating_sub(
                                                    "path=".len() + path_str.len() + " data=".len(),
                                                );
                                                if summary.len() > max_summary_len {
                                                    let mut trimmed: HeaplessString<128> =
                                                        HeaplessString::new();
                                                    for ch in summary.as_str().chars() {
                                                        if trimmed.len() >= max_summary_len {
                                                            break;
                                                        }
                                                        if trimmed.push(ch).is_err() {
                                                            break;
                                                        }
                                                    }
                                                    summary = trimmed;
                                                }
                                                summary
                                            };
                                            let detail = format_message(format_args!(
                                                "path={} data={}",
                                                path_str,
                                                summary.as_str()
                                            ));
                                            #[cfg(feature = "cohesix-dev")]
                                            {
                                                let message = format_message(format_args!(
                                                    "audit cat.ack path={}",
                                                    path_str
                                                ));
                                                crate::debug_uart::debug_uart_line(
                                                    message.as_str(),
                                                );
                                            }
                                            self.emit_ack_ok(verb_label, Some(detail.as_str()));
                                            self.metrics.ui_reads =
                                                self.metrics.ui_reads.saturating_add(1);
                                            self.stream_end_pending = true;
                                            if let Some(pending) = self.pending_stream.as_mut() {
                                                pending.next_line = 0;
                                                pending.bandwidth_bytes = stream_bytes;
                                                pending.cursor = if log_path {
                                                    None
                                                } else {
                                                    cursor_check.map(|check| PendingCursor {
                                                        path_key: path_str.to_owned(),
                                                        offset: 0,
                                                        len: data_bytes as usize,
                                                        check,
                                                    })
                                                };
                                            }
                                        }
                                    }
                                }
                            } else {
                                cmd_status = "err";
                                self.emit_refusal(
                                    verb_label,
                                    RefusalReason::Policy,
                                    Some("detail=ninedoor-unavailable"),
                                );
                            }
                        }
                        #[cfg(not(feature = "kernel"))]
                        {
                            cmd_status = "err";
                            self.emit_refusal(
                                verb_label,
                                RefusalReason::Policy,
                                Some("detail=ninedoor-unavailable"),
                            );
                        }
                    }
                } else {
                    cmd_status = "err";
                }
            }
            Command::Ls { path } => {
                if self.ensure_worker_session(verb_label) {
                    let path_str = path.as_str();
                    if let Err(denial) = self.check_ticket_scope(path_str, TicketVerb::Read) {
                        self.record_ticket_denial(path_str, TicketVerb::Read, denial);
                        self.emit_ticket_denied(verb_label, Some(path_str), denial);
                        cmd_status = "err";
                    } else {
                        let message = format_message(format_args!("console: ls {}", path_str));
                        self.audit.info(message.as_str());
                        self.metrics.accepted_commands += 1;
                        #[cfg(feature = "kernel")]
                        {
                            if let Some(bridge_ref) = self.ninedoor.as_mut() {
                                let (data_bytes, entries_len, list_err) = {
                                    let pending =
                                        self.pending_stream.get_or_insert_with(PendingStream::new);
                                    pending.reset();
                                    match bridge_ref.list_into(path_str, &mut pending.lines) {
                                        Ok(()) => {
                                            let data_bytes = pending
                                                .lines
                                                .iter()
                                                .map(|entry| entry.len() as u64)
                                                .sum();
                                            let entries_len = pending.lines.len();
                                            (data_bytes, entries_len, None)
                                        }
                                        Err(err) => (0, 0, Some(err)),
                                    }
                                };
                                if let Some(err) = list_err {
                                    cmd_status = "err";
                                    let sid = self.session_id.unwrap_or(0);
                                    let err_msg = format_message(format_args!("{err}"));
                                    self.audit_ninedoor_err(sid, "LS", path_str, err_msg.as_str());
                                    self.emit_ninedoor_refusal(verb_label, Some(path_str), &err);
                                } else if let Err(denial) = self.check_ticket_bandwidth(data_bytes)
                                {
                                    self.record_ticket_denial(path_str, TicketVerb::Read, denial);
                                    self.emit_ticket_denied(verb_label, Some(path_str), denial);
                                    if let Some(pending) = self.pending_stream.as_mut() {
                                        pending.lines.clear();
                                    }
                                    cmd_status = "err";
                                } else {
                                    let detail = format_message(format_args!(
                                        "path={} entries={}",
                                        path_str, entries_len
                                    ));
                                    self.emit_ack_ok(verb_label, Some(detail.as_str()));
                                    if let Some(pending) = self.pending_stream.as_mut() {
                                        pending.next_line = 0;
                                        pending.bandwidth_bytes = data_bytes;
                                        pending.cursor = None;
                                    }
                                    self.stream_end_pending = true;
                                }
                            } else {
                                cmd_status = "err";
                                self.emit_refusal(
                                    verb_label,
                                    RefusalReason::Policy,
                                    Some("detail=ninedoor-unavailable"),
                                );
                            }
                        }
                        #[cfg(not(feature = "kernel"))]
                        {
                            cmd_status = "err";
                            self.emit_refusal(
                                verb_label,
                                RefusalReason::Policy,
                                Some("detail=ninedoor-unavailable"),
                            );
                        }
                    }
                } else {
                    cmd_status = "err";
                }
            }
            Command::Log => {
                if self.ensure_authenticated(SessionRole::Queen) {
                    let path_str = "/log/queen.log";
                    if let Err(denial) = self.check_ticket_scope(path_str, TicketVerb::Read) {
                        self.record_ticket_denial(path_str, TicketVerb::Read, denial);
                        self.emit_ticket_denied(verb_label, Some(path_str), denial);
                        cmd_status = "err";
                    } else {
                        #[cfg(feature = "kernel")]
                        let stream_bytes = self.prepare_log_tail_pending_stream(None);
                        #[cfg(not(feature = "kernel"))]
                        let stream_bytes = 0u64;
                        if let Err(denial) = self.check_ticket_bandwidth(stream_bytes) {
                            self.record_ticket_denial(path_str, TicketVerb::Read, denial);
                            self.emit_ticket_denied(verb_label, Some(path_str), denial);
                            cmd_status = "err";
                        } else {
                            self.audit.info("console: log stream start");
                            self.metrics.accepted_commands += 1;
                            self.metrics.ui_reads = self.metrics.ui_reads.saturating_add(1);
                            self.emit_ack_ok(verb_label, None);
                            self.stream_end_pending = true;
                            self.tail_active = true;
                            let sid = self.session_id.unwrap_or(0);
                            self.audit_tail_start(sid, path_str);
                            #[cfg(feature = "kernel")]
                            {
                                forwarded = true;
                            }
                        }
                    }
                } else {
                    cmd_status = "err";
                    self.emit_auth_failure(verb_label);
                }
            }
            Command::Echo { path, payload } => {
                if self.ensure_worker_session(verb_label) {
                    let path_str = path.as_str();
                    let worker_restricted = matches!(self.session, Some(SessionRole::Worker))
                        && !(path_str.starts_with("/bus/") || path_str.starts_with("/lora/"));
                    if worker_restricted {
                        self.metrics.denied_commands += 1;
                        self.audit.denied("echo denied");
                        cmd_status = "err";
                        let detail = format_message(format_args!(
                            "detail=denied path={path_str} error=EPERM"
                        ));
                        self.emit_refusal(verb_label, RefusalReason::Policy, Some(detail.as_str()));
                    } else if let Err(denial) = self.check_ticket_scope(path_str, TicketVerb::Write)
                    {
                        self.record_ticket_denial(path_str, TicketVerb::Write, denial);
                        self.emit_ticket_denied(verb_label, Some(path_str), denial);
                        cmd_status = "err";
                    } else if let Err(denial) = self.check_ticket_bandwidth(payload.len() as u64) {
                        self.record_ticket_denial(path_str, TicketVerb::Write, denial);
                        self.emit_ticket_denied(verb_label, Some(path_str), denial);
                        cmd_status = "err";
                    } else {
                        let message = format_message(format_args!(
                            "console: echo {} bytes={}",
                            path_str,
                            payload.len()
                        ));
                        self.audit.info(message.as_str());
                        self.metrics.accepted_commands += 1;
                        #[cfg(feature = "kernel")]
                        {
                            if let Some(bridge_ref) = self.ninedoor.as_mut() {
                                match bridge_ref.echo(path_str, payload.as_str()) {
                                    Ok(()) => {
                                        let detail = format_message(format_args!(
                                            "path={} bytes={}",
                                            path_str,
                                            payload.len()
                                        ));
                                        self.emit_ack_ok(verb_label, Some(detail.as_str()));
                                        self.consume_ticket_bandwidth(payload.len() as u64);
                                    }
                                    Err(err) => {
                                        cmd_status = "err";
                                        let sid = self.session_id.unwrap_or(0);
                                        let err_msg = format_message(format_args!("{err}"));
                                        self.audit_ninedoor_err(
                                            sid,
                                            "ECHO",
                                            path.as_str(),
                                            err_msg.as_str(),
                                        );
                                        self.emit_ninedoor_refusal(
                                            verb_label,
                                            Some(path_str),
                                            &err,
                                        );
                                    }
                                }
                            } else {
                                cmd_status = "err";
                                self.emit_refusal(
                                    verb_label,
                                    RefusalReason::Policy,
                                    Some("detail=ninedoor-unavailable"),
                                );
                            }
                        }
                        #[cfg(not(feature = "kernel"))]
                        {
                            cmd_status = "err";
                            self.emit_refusal(
                                verb_label,
                                RefusalReason::Policy,
                                Some("detail=ninedoor-unavailable"),
                            );
                        }
                    }
                } else {
                    cmd_status = "err";
                }
            }
            Command::Spawn(payload) => {
                if self.ensure_authenticated(SessionRole::Queen) {
                    if let Err(denial) = self.check_ticket_scope(QUEEN_CTL_PATH, TicketVerb::Write)
                    {
                        self.record_ticket_denial(QUEEN_CTL_PATH, TicketVerb::Write, denial);
                        self.emit_ticket_denied(verb_label, Some(QUEEN_CTL_PATH), denial);
                        cmd_status = "err";
                    } else if let Err(denial) = self.check_ticket_bandwidth(payload.len() as u64) {
                        self.record_ticket_denial(QUEEN_CTL_PATH, TicketVerb::Write, denial);
                        self.emit_ticket_denied(verb_label, Some(QUEEN_CTL_PATH), denial);
                        cmd_status = "err";
                    } else {
                        let message =
                            format_message(format_args!("console: spawn {}", payload.as_str()));
                        self.audit.info(message.as_str());
                        self.metrics.accepted_commands += 1;
                        let detail = format_message(format_args!("payload={}", payload.as_str()));
                        self.emit_ack_ok(verb_label, Some(detail.as_str()));
                        self.consume_ticket_bandwidth(payload.len() as u64);
                        #[cfg(feature = "kernel")]
                        {
                            forwarded = true;
                        }
                    }
                } else {
                    cmd_status = "err";
                    self.emit_auth_failure(verb_label);
                }
            }
            Command::Kill(ident) => {
                if self.ensure_authenticated(SessionRole::Queen) {
                    let payload_len = format!("{{\"kill\":\"{}\"}}", ident.as_str()).len() as u64;
                    if let Err(denial) = self.check_ticket_scope(QUEEN_CTL_PATH, TicketVerb::Write)
                    {
                        self.record_ticket_denial(QUEEN_CTL_PATH, TicketVerb::Write, denial);
                        self.emit_ticket_denied(verb_label, Some(QUEEN_CTL_PATH), denial);
                        cmd_status = "err";
                    } else if let Err(denial) = self.check_ticket_bandwidth(payload_len) {
                        self.record_ticket_denial(QUEEN_CTL_PATH, TicketVerb::Write, denial);
                        self.emit_ticket_denied(verb_label, Some(QUEEN_CTL_PATH), denial);
                        cmd_status = "err";
                    } else {
                        let message =
                            format_message(format_args!("console: kill {}", ident.as_str()));
                        self.audit.info(message.as_str());
                        self.metrics.accepted_commands += 1;
                        let detail = format_message(format_args!("id={}", ident.as_str()));
                        self.emit_ack_ok(verb_label, Some(detail.as_str()));
                        self.consume_ticket_bandwidth(payload_len);
                        #[cfg(feature = "kernel")]
                        {
                            forwarded = true;
                        }
                    }
                } else {
                    cmd_status = "err";
                    self.emit_auth_failure(verb_label);
                }
            }
        }

        #[cfg(feature = "kernel")]
        if forwarded {
            if let Err(err) = self.forward_to_ninedoor(&command_clone) {
                self.stream_end_pending = false;
                self.pending_stream = None;
                cmd_status = "err";
                #[cfg(feature = "cohesix-dev")]
                if let CommandDispatchError::Bridge { source, .. } = &err {
                    let sid = self.session_id.unwrap_or(0);
                    let err_msg = format_message(format_args!("{source}"));
                    match &command_clone {
                        Command::Tail { path, .. } => {
                            self.audit_ninedoor_err(sid, "TAIL", path.as_str(), err_msg.as_str());
                        }
                        Command::Log => {
                            self.audit_ninedoor_err(sid, "LOG", "/log/queen.log", err_msg.as_str());
                        }
                        Command::Attach { .. } => {
                            self.audit_ninedoor_err(sid, "ATTACH", "-", err_msg.as_str());
                        }
                        Command::Spawn(_) => {
                            self.audit_ninedoor_err(sid, "SPAWN", "-", err_msg.as_str());
                        }
                        Command::Kill(_) => {
                            self.audit_ninedoor_err(sid, "KILL", "-", err_msg.as_str());
                        }
                        _ => {}
                    }
                }
                if self.tail_active {
                    let sid = self.session_id.unwrap_or(0);
                    self.audit_tail_stop(sid, "error");
                    self.tail_active = false;
                }
                result = Err(err);
            }
        }

        #[cfg(feature = "kernel")]
        if result.is_ok() && self.stream_end_pending {
            if self.pending_stream.is_some() {
                self.flush_pending_stream();
            } else {
                match &command_clone {
                    Command::Log => self.emit_log_snapshot(),
                    Command::Tail { path, .. } if path.as_str() == "/log/queen.log" => {
                        self.emit_log_snapshot();
                    }
                    Command::Tail { path, .. } if path.as_str() == "/proc/ingest/watch" => {
                        if let Some(bridge) = self.ninedoor.as_mut() {
                            if let Ok(lines) =
                                bridge.ingest_watch_lines(self.now_ms, &mut *self.audit)
                            {
                                for line in lines {
                                    self.emit_console_line(line.as_str());
                                }
                            }
                        }
                    }
                    _ => {}
                }
                self.emit_stream_end_if_pending();
            }
        }

        if result.is_ok() {
            self.emit_stream_end_if_pending();
        } else if self.tail_active {
            let sid = self.session_id.unwrap_or(0);
            self.audit_tail_stop(sid, "error");
            self.tail_active = false;
        }

        if audit_net {
            let end_sid = if term == "EOF" {
                start_sid
            } else {
                self.session_id.unwrap_or(start_sid)
            };
            self.audit_tcp_cmd_end(conn_id, end_sid, verb_label, cmd_status, term);
        }

        result
    }

    #[cfg(feature = "kernel")]
    #[inline(never)]
    fn forward_to_ninedoor(&mut self, command: &Command) -> Result<(), CommandDispatchError> {
        #[cfg(debug_assertions)]
        {
            vtable_sentinel();
        }

        let verb = command.verb();

        let Some(bridge_ref) = self.ninedoor.as_mut() else {
            #[cfg(debug_assertions)]
            {
                log::warn!("attempted to forward {verb:?} without an attached NineDoor bridge");
            }
            return Err(CommandDispatchError::NineDoorUnavailable { verb });
        };

        let bridge = &mut **bridge_ref;

        match command {
            Command::Attach { role, ticket } => {
                let ticket_str = ticket.as_ref().map(|value| value.as_str());
                let audit = &mut *self.audit;
                bridge
                    .attach(role.as_str(), ticket_str, audit)
                    .map_err(|source| CommandDispatchError::Bridge { verb, source })?;
            }
            Command::Tail { path, .. } => {
                let audit = &mut *self.audit;
                bridge
                    .tail(path.as_str(), audit)
                    .map_err(|source| CommandDispatchError::Bridge { verb, source })?;
            }
            Command::Log => {
                let audit = &mut *self.audit;
                bridge
                    .log_stream(audit)
                    .map_err(|source| CommandDispatchError::Bridge { verb, source })?;
            }
            Command::Spawn(payload) => {
                let audit = &mut *self.audit;
                bridge
                    .spawn(payload.as_str(), audit)
                    .map_err(|source| CommandDispatchError::Bridge { verb, source })?;
            }
            Command::Kill(identifier) => {
                let audit = &mut *self.audit;
                bridge
                    .kill(identifier.as_str(), audit)
                    .map_err(|source| CommandDispatchError::Bridge { verb, source })?;
            }
            Command::Help
            | Command::Quit
            | Command::BootInfo
            | Command::Caps
            | Command::Smp { .. }
            | Command::Mem
            | Command::CacheLog { .. }
            | Command::Ping
            | Command::Test
            | Command::NetTest
            | Command::NetStats
            | Command::Reboot
            | Command::Cat { .. }
            | Command::Echo { .. }
            | Command::Ls { .. } => {
                return Err(CommandDispatchError::UnsupportedForNineDoor { verb });
            }
        }

        Ok(())
    }

    #[cfg(feature = "kernel")]
    fn handle_dispatch_error(&mut self, err: CommandDispatchError) {
        match err {
            CommandDispatchError::NineDoorUnavailable { verb } => {
                self.audit.denied("ninedoor unavailable");
                self.emit_console_line("ERR: NineDoor unavailable");
                self.emit_refusal(
                    verb.ack_label(),
                    RefusalReason::Policy,
                    Some("detail=ninedoor-unavailable"),
                );
            }
            CommandDispatchError::UnsupportedForNineDoor { verb } => {
                self.audit.denied("ninedoor unsupported command");
                self.emit_console_line("ERR unsupported for NineDoor");
                self.emit_refusal(
                    verb.ack_label(),
                    RefusalReason::Policy,
                    Some("detail=unsupported"),
                );
            }
            CommandDispatchError::Bridge { verb, source } => {
                let audit_line = format_message(format_args!("ninedoor bridge error: {source}"));
                self.audit.denied(audit_line.as_str());
                self.emit_ninedoor_refusal(verb.ack_label(), None, &source);
            }
        }
    }

    fn emit_stream_end_if_pending(&mut self) {
        if self.stream_end_pending {
            #[cfg(feature = "kernel")]
            if let Some(pending) = self.pending_stream.as_ref() {
                if pending.next_line < pending.lines.len() || pending.log_cursor.is_some() {
                    return;
                }
            }
            if !self.try_emit_console_line("END") {
                return;
            }
            self.stream_end_pending = false;
            if self.tail_active {
                let sid = self.session_id.unwrap_or(0);
                self.audit_tail_stop(sid, "eof");
                self.tail_active = false;
            }
        }
    }

    #[cfg(feature = "kernel")]
    fn flush_pending_stream(&mut self) {
        if !self.stream_end_pending {
            return;
        }
        let network_origin = matches!(self.session_origin, Some(ConsoleInputSource::Net));
        let line_limit = if network_origin {
            NET_PENDING_STREAM_FLUSH_LINES_PER_TURN
        } else {
            LOCAL_PENDING_STREAM_FLUSH_LINES_PER_TURN
        };
        let byte_limit = if network_origin {
            NET_PENDING_STREAM_FLUSH_BYTES_PER_TURN
        } else {
            LOCAL_PENDING_STREAM_FLUSH_BYTES_PER_TURN
        };
        let Some(mut pending) = self.pending_stream.take() else {
            self.emit_stream_end_if_pending();
            return;
        };
        let mut emitted_lines = 0usize;
        let mut emitted_bytes = 0usize;
        loop {
            while pending.next_line < pending.lines.len() {
                let line = pending.lines[pending.next_line].as_str();
                if emitted_lines >= line_limit
                    || (emitted_lines > 0 && emitted_bytes.saturating_add(line.len()) > byte_limit)
                {
                    self.pending_stream = Some(pending);
                    return;
                }
                if !self.try_emit_console_line(line) {
                    self.pending_stream = Some(pending);
                    return;
                }
                pending.next_line = pending.next_line.saturating_add(1);
                emitted_lines = emitted_lines.saturating_add(1);
                emitted_bytes = emitted_bytes.saturating_add(line.len());
            }
            if !Self::refill_log_pending_stream(&mut pending) {
                break;
            }
        }
        self.consume_ticket_bandwidth(pending.bandwidth_bytes);
        if let Some(cursor) = pending.cursor {
            self.record_ticket_cursor(cursor.path_key, cursor.offset, cursor.len, cursor.check);
        }
        self.emit_stream_end_if_pending();
    }

    fn active_tcp_conn_id(&self) -> u64 {
        #[cfg(feature = "net-console")]
        {
            self.net_conn_id.unwrap_or(0)
        }
        #[cfg(not(feature = "net-console"))]
        {
            0
        }
    }

    fn end_session(&mut self, reason: &'static str) {
        if self.parser.clear_buffer() {
            let message = format_message(format_args!(
                "console: cleared partial input on session end reason={reason}"
            ));
            self.audit.info(message.as_str());
        }
        self.local_line.clear();
        if self.session.is_none() && !self.tail_active {
            return;
        }
        let was_queen = matches!(self.session, Some(SessionRole::Queen));
        let sid = self.session_id.unwrap_or(0);
        if self.tail_active {
            self.audit_tail_stop(sid, reason);
            self.tail_active = false;
        }
        if matches!(self.session_origin, Some(ConsoleInputSource::Net)) && self.session_id.is_some()
        {
            let conn_id = self.active_tcp_conn_id();
            self.audit_tcp_session_detach(conn_id, sid, reason);
        }
        self.session = None;
        self.session_role = None;
        self.session_ticket = None;
        self.ticket_usage = None;
        self.session_id = None;
        self.session_origin = None;
        self.stream_end_pending = false;
        #[cfg(feature = "kernel")]
        {
            self.pending_stream = None;
        }
        #[cfg(feature = "kernel")]
        if let Some(bridge) = self.ninedoor.as_mut() {
            bridge.reset_session();
        }
        if was_queen {
            let cut_reason = match reason {
                "eof" | "error" => Some(lifecycle::RootCutReason::NetworkUnreachable),
                _ => None,
            };
            if let Some(cut_reason) = cut_reason {
                lifecycle::root_mark_cut(cut_reason);
            }
        }
    }

    #[cfg(feature = "net-console")]
    fn disconnect_reason_label(reason: NetConsoleDisconnectReason) -> &'static str {
        match reason {
            NetConsoleDisconnectReason::Quit => "quit",
            NetConsoleDisconnectReason::Eof => "eof",
            NetConsoleDisconnectReason::Reset => "error",
            NetConsoleDisconnectReason::Error => "error",
        }
    }

    fn audit_tcp_cmd_begin(&mut self, conn_id: u64, sid: u64, verb: &str) {
        #[cfg(feature = "cohesix-dev")]
        {
            let message = format_message(format_args!(
                "audit tcp.cmd.begin conn_id={} sid={} verb={}",
                conn_id, sid, verb
            ));
            crate::debug_uart::debug_uart_line(message.as_str());
        }
        #[cfg(not(feature = "cohesix-dev"))]
        {
            let _ = conn_id;
            let _ = sid;
            let _ = verb;
        }
    }

    fn audit_tcp_cmd_end(&mut self, conn_id: u64, sid: u64, verb: &str, status: &str, term: &str) {
        #[cfg(feature = "cohesix-dev")]
        {
            let message = format_message(format_args!(
                "audit tcp.cmd.end conn_id={} sid={} verb={} status={} term={}",
                conn_id, sid, verb, status, term
            ));
            crate::debug_uart::debug_uart_line(message.as_str());
        }
        #[cfg(not(feature = "cohesix-dev"))]
        {
            let _ = conn_id;
            let _ = sid;
            let _ = verb;
            let _ = status;
            let _ = term;
        }
    }

    fn audit_tcp_session_attach(&mut self, conn_id: u64, sid: u64, role: &str) {
        #[cfg(feature = "cohesix-dev")]
        {
            let message = format_message(format_args!(
                "audit tcp.session.attach conn_id={} sid={} role={}",
                conn_id, sid, role
            ));
            crate::debug_uart::debug_uart_line(message.as_str());
        }
        #[cfg(not(feature = "cohesix-dev"))]
        {
            let _ = conn_id;
            let _ = sid;
            let _ = role;
        }
    }

    fn audit_tcp_session_detach(&mut self, conn_id: u64, sid: u64, reason: &str) {
        #[cfg(feature = "cohesix-dev")]
        {
            let message = format_message(format_args!(
                "audit tcp.session.detach conn_id={} sid={} reason={}",
                conn_id, sid, reason
            ));
            crate::debug_uart::debug_uart_line(message.as_str());
        }
        #[cfg(not(feature = "cohesix-dev"))]
        {
            let _ = conn_id;
            let _ = sid;
            let _ = reason;
        }
    }

    fn audit_tail_start(&mut self, sid: u64, path: &str) {
        #[cfg(feature = "cohesix-dev")]
        {
            let message =
                format_message(format_args!("audit tail.start sid={} path={}", sid, path));
            crate::debug_uart::debug_uart_line(message.as_str());
        }
        #[cfg(not(feature = "cohesix-dev"))]
        {
            let _ = sid;
            let _ = path;
        }
    }

    fn audit_tail_stop(&mut self, sid: u64, reason: &str) {
        #[cfg(feature = "cohesix-dev")]
        {
            let message = format_message(format_args!(
                "audit tail.stop sid={} reason={}",
                sid, reason
            ));
            crate::debug_uart::debug_uart_line(message.as_str());
        }
        #[cfg(not(feature = "cohesix-dev"))]
        {
            let _ = sid;
            let _ = reason;
        }
    }

    fn audit_ninedoor_err(&mut self, sid: u64, op: &str, path: &str, err: &str) {
        #[cfg(feature = "cohesix-dev")]
        {
            let message = format_message(format_args!(
                "audit ninedoor.err sid={} op={} path={} err={}",
                sid, op, path, err
            ));
            crate::debug_uart::debug_uart_line(message.as_str());
        }
        #[cfg(not(feature = "cohesix-dev"))]
        {
            let _ = sid;
            let _ = op;
            let _ = path;
            let _ = err;
        }
    }

    fn session_role_label(&self) -> &'static str {
        self.session_role
            .map(cohsh_core::role_label)
            .unwrap_or("unauthenticated")
    }

    fn session_ticket_label(&self) -> &str {
        self.session_ticket.as_deref().unwrap_or("none")
    }

    fn record_ticket_claim_denial(&mut self, role: Role, ticket: &str, err: &dyn fmt::Display) {
        let role_label = cohsh_core::role_label(role);
        let message = format!(
            "ui-ticket outcome=deny reason=invalid-claims role={} ticket={} detail={err}",
            role_label, ticket
        );
        self.audit.denied(message.as_str());
    }

    fn record_ticket_expired(&mut self, role: Role, ticket: &str, claims: &TicketClaims) {
        let role_label = cohsh_core::role_label(role);
        let ttl_s = claims.budget.ttl_s().unwrap_or(0);
        let message = format!(
            "ui-ticket outcome=deny reason=expired role={} ticket={} issued_at_ms={} ttl_s={} now_ms={}",
            role_label, ticket, claims.issued_at_ms, ttl_s, self.now_ms
        );
        self.audit.denied(message.as_str());
    }

    fn record_ticket_denial(&mut self, path: &str, verb: TicketVerb, denial: TicketDeny) {
        let path_label = if path.is_empty() { "/" } else { path };
        let verb_label = ticket_verb_label(verb);
        let mut message = format!(
            "ui-ticket outcome=deny reason={} role={} ticket={} path={} verb={}",
            ticket_deny_reason(denial),
            self.session_role_label(),
            self.session_ticket_label(),
            path_label,
            verb_label
        );
        match denial {
            TicketDeny::Scope => {}
            TicketDeny::Rate { limit_per_s } => {
                message.push_str(&format!(
                    " limit_per_s={limit_per_s} window_ms={TICKET_RATE_WINDOW_MS}"
                ));
            }
            TicketDeny::Bandwidth {
                limit_bytes,
                remaining_bytes,
                requested_bytes,
            } => {
                message.push_str(&format!(
                    " limit_bytes={limit_bytes} remaining_bytes={remaining_bytes} requested_bytes={requested_bytes}"
                ));
            }
            TicketDeny::CursorResume { limit } => {
                message.push_str(&format!(" limit={limit}"));
            }
            TicketDeny::CursorAdvance { limit } => {
                message.push_str(&format!(" limit={limit}"));
            }
        }
        self.audit.denied(message.as_str());
    }

    fn emit_ticket_denied(&mut self, verb: &str, path: Option<&str>, denial: TicketDeny) {
        self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
        self.metrics.ui_denies = self.metrics.ui_denies.saturating_add(1);
        let (reason, error) = match denial {
            TicketDeny::Scope => (RefusalReason::Policy, "EPERM"),
            TicketDeny::Rate { .. }
            | TicketDeny::Bandwidth { .. }
            | TicketDeny::CursorResume { .. }
            | TicketDeny::CursorAdvance { .. } => (RefusalReason::Quota, "ELIMIT"),
        };
        let detail = match path {
            Some(path) => format_message(format_args!("path={path} error={error}")),
            None => format_message(format_args!("error={error}")),
        };
        self.emit_refusal(verb, reason, Some(detail.as_str()));
    }

    fn check_ticket_scope(&mut self, path: &str, verb: TicketVerb) -> Result<(), TicketDeny> {
        let Some(usage) = self.ticket_usage.as_mut() else {
            return Ok(());
        };
        if usage.scopes.is_empty() {
            return Ok(());
        }
        let Some(components) = split_request_path(path) else {
            return Ok(());
        };
        usage.check_scope(&components, verb, false, self.now_ms)
    }

    fn check_ticket_bandwidth(&self, requested: u64) -> Result<(), TicketDeny> {
        let Some(usage) = self.ticket_usage.as_ref() else {
            return Ok(());
        };
        usage.check_bandwidth(requested)
    }

    fn consume_ticket_bandwidth(&mut self, consumed: u64) {
        if let Some(usage) = self.ticket_usage.as_mut() {
            usage.consume_bandwidth(consumed);
        }
    }

    fn check_ticket_cursor(
        &self,
        path: &str,
        offset: u64,
    ) -> Result<Option<CursorCheck>, TicketDeny> {
        if !is_telemetry_path(path) {
            return Ok(None);
        }
        let Some(usage) = self.ticket_usage.as_ref() else {
            return Ok(None);
        };
        usage.check_cursor(path, offset).map(Some)
    }

    fn ticket_cursor_offset(&self, path: &str) -> Option<u64> {
        if !is_telemetry_path(path) {
            return None;
        }
        self.ticket_usage
            .as_ref()
            .and_then(|usage| usage.cursor_offset(path))
    }

    fn record_ticket_cursor(&mut self, path: String, offset: u64, len: usize, check: CursorCheck) {
        if let Some(usage) = self.ticket_usage.as_mut() {
            usage.record_cursor(path, offset, len, check);
        }
    }

    fn ensure_authenticated(&mut self, minimum: SessionRole) -> bool {
        match (self.session, minimum) {
            (Some(SessionRole::Queen), _) => true,
            (Some(SessionRole::Worker), SessionRole::Worker) => true,
            _ => {
                self.metrics.denied_commands += 1;
                self.audit.denied("unauthenticated command");
                false
            }
        }
    }

    fn ensure_reboot_authorized(&mut self, verb: &str) -> bool {
        if !self.ensure_authenticated(SessionRole::Queen) {
            self.emit_auth_failure(verb);
            return false;
        }
        let authorized_by_tcp_secret = matches!(self.session_origin, Some(ConsoleInputSource::Net));
        let authorized_by_ticket = self
            .session_ticket
            .as_deref()
            .is_some_and(|ticket| !ticket.trim().is_empty());
        if authorized_by_tcp_secret || authorized_by_ticket {
            return true;
        }
        self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
        self.audit
            .denied("reboot denied: secret-backed session required");
        self.emit_refusal(verb, RefusalReason::Policy, Some("detail=secret-required"));
        false
    }

    fn ensure_worker_session(&mut self, verb: &str) -> bool {
        if !self.ensure_authenticated(SessionRole::Worker) {
            self.emit_auth_failure(verb);
            return false;
        }
        #[cfg(feature = "kernel")]
        {
            if !lifecycle::root_snapshot().reachable {
                self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
                self.audit.denied("worker command denied: root unreachable");
                self.emit_refusal(verb, RefusalReason::Cut, Some("detail=root-unreachable"));
                return false;
            }
        }
        true
    }

    #[inline(never)]
    fn handle_attach(
        &mut self,
        role: HeaplessString<{ MAX_ROLE_LEN }>,
        ticket: Option<HeaplessString<{ MAX_TICKET_LEN }>>,
    ) -> bool {
        if let Err(delay) = self.throttle.check(self.now_ms) {
            let message = format_message(format_args!("attach throttled ({} ms)", delay));
            self.audit.denied(message.as_str());
            self.metrics.denied_commands += 1;
            let detail = format_message(format_args!("detail=throttled delay_ms={delay}"));
            self.emit_refusal(
                ConsoleVerb::Attach.ack_label(),
                RefusalReason::Quota,
                Some(detail.as_str()),
            );
            return false;
        }

        let Some(requested_role) =
            cohsh_core::parse_role(role.as_str(), RoleParseMode::AllowWorkerAlias)
        else {
            self.audit.denied("attach: invalid role");
            self.metrics.denied_commands += 1;
            self.emit_refusal(
                ConsoleVerb::Attach.ack_label(),
                RefusalReason::Policy,
                Some("detail=invalid-role"),
            );
            return false;
        };

        if !matches!(requested_role, Role::Queen) {
            #[cfg(feature = "kernel")]
            {
                if !lifecycle::gate_allows(lifecycle::GATE_WORKER_ATTACH) {
                    let state = lifecycle::state();
                    let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
                    let _ = FmtWrite::write_fmt(
                        &mut line,
                        format_args!(
                            "lifecycle denied action={} state={} reason=gate-denied",
                            lifecycle::GATE_WORKER_ATTACH.name,
                            lifecycle::state_label(state)
                        ),
                    );
                    log_buffer::append_log_line(line.as_str());
                    self.audit.denied("attach denied by lifecycle");
                    self.metrics.denied_commands += 1;
                    let detail = format_message(format_args!(
                        "detail=lifecycle-denied state={}",
                        lifecycle::state_label(state)
                    ));
                    self.emit_refusal(
                        ConsoleVerb::Attach.ack_label(),
                        RefusalReason::Policy,
                        Some(detail.as_str()),
                    );
                    return false;
                }
            }
        }

        let ticket_str = ticket.as_ref().map(|t| t.as_str());
        log::info!(
            target: "net-console",
            "[net-console] auth: parsed role={:?} ticket_present={}",
            requested_role,
            ticket_str.is_some()
        );
        let validated = self.validator.validate(requested_role, ticket_str);
        if let Err(err) = self.parser.record_login_attempt(validated, self.now_ms) {
            let message = format_message(format_args!("attach rate limited: {}", err));
            self.audit.denied(message.as_str());
            self.metrics.denied_commands += 1;
            match err {
                ConsoleError::RateLimited(delay) => {
                    let detail =
                        format_message(format_args!("detail=rate-limited delay_ms={delay}"));
                    self.emit_refusal(
                        ConsoleVerb::Attach.ack_label(),
                        RefusalReason::Quota,
                        Some(detail.as_str()),
                    );
                }
                other => {
                    let detail = format_message(format_args!("detail={}", other));
                    self.emit_refusal(
                        ConsoleVerb::Attach.ack_label(),
                        RefusalReason::Policy,
                        Some(detail.as_str()),
                    );
                }
            }
            if matches!(requested_role, Role::Queen) {
                lifecycle::root_mark_policy_denied();
            }
            return false;
        }

        if validated {
            let mut ticket_usage = None;
            if let Some(ticket) = ticket_str {
                let claims = match TicketToken::decode_unverified(ticket) {
                    Ok(claims) => claims,
                    Err(err) => {
                        self.metrics.denied_commands =
                            self.metrics.denied_commands.saturating_add(1);
                        self.metrics.ui_denies = self.metrics.ui_denies.saturating_add(1);
                        self.record_ticket_claim_denial(requested_role, ticket, &err);
                        self.emit_refusal(
                            ConsoleVerb::Attach.ack_label(),
                            RefusalReason::Policy,
                            Some("detail=invalid-claims"),
                        );
                        if matches!(requested_role, Role::Queen) {
                            lifecycle::root_mark_policy_denied();
                        }
                        return false;
                    }
                };
                if let Some(ttl_s) = claims.budget.ttl_s() {
                    let ttl_ms = ttl_s.saturating_mul(1_000);
                    let expires_at_ms = claims.issued_at_ms.saturating_add(ttl_ms);
                    if self.now_ms >= expires_at_ms {
                        self.metrics.denied_commands =
                            self.metrics.denied_commands.saturating_add(1);
                        self.metrics.ui_denies = self.metrics.ui_denies.saturating_add(1);
                        self.record_ticket_expired(requested_role, ticket, &claims);
                        self.emit_refusal(
                            ConsoleVerb::Attach.ack_label(),
                            RefusalReason::Quota,
                            Some("detail=expired"),
                        );
                        if matches!(requested_role, Role::Queen) {
                            lifecycle::root_mark_policy_denied();
                        }
                        return false;
                    }
                }
                match TicketUsage::from_claims(
                    &claims,
                    crate::generated::ticket_limits(),
                    self.now_ms,
                ) {
                    Ok(usage) => {
                        if usage.has_enforcement() {
                            ticket_usage = Some(usage);
                        }
                    }
                    Err(err) => {
                        self.metrics.denied_commands =
                            self.metrics.denied_commands.saturating_add(1);
                        self.metrics.ui_denies = self.metrics.ui_denies.saturating_add(1);
                        self.record_ticket_claim_denial(requested_role, ticket, &err);
                        self.emit_refusal(
                            ConsoleVerb::Attach.ack_label(),
                            RefusalReason::Quota,
                            Some("detail=invalid-claims"),
                        );
                        if matches!(requested_role, Role::Queen) {
                            lifecycle::root_mark_policy_denied();
                        }
                        return false;
                    }
                }
            }

            self.session = SessionRole::from_role(requested_role);
            self.session_role = Some(requested_role);
            self.session_ticket = ticket_str.map(|value| value.to_owned());
            self.ticket_usage = ticket_usage;
            self.session_origin = Some(self.last_input_source);
            let sid = self.next_session_id;
            self.next_session_id = self.next_session_id.wrapping_add(1);
            self.session_id = Some(sid);
            self.metrics.accepted_commands += 1;
            self.throttle.register_success();
            let message = format_message(format_args!("attach accepted role={:?}", requested_role));
            self.audit.info(message.as_str());
            let role_label = cohsh_core::role_label(requested_role);
            let detail = format_message(format_args!("role={role_label}"));
            self.emit_ack_ok(ConsoleVerb::Attach.ack_label(), Some(detail.as_str()));
            if matches!(requested_role, Role::Queen) {
                lifecycle::root_mark_session_active(self.now_ms);
            }
            log::info!(
                target: "net-console",
                "[net-console] auth: success; attaching session role={role_label}"
            );
            if matches!(self.session_origin, Some(ConsoleInputSource::Net)) {
                let conn_id = self.active_tcp_conn_id();
                self.audit_tcp_session_attach(conn_id, sid, role_label);
            }
            return true;
        } else {
            self.throttle.register_failure(self.now_ms);
            self.metrics.denied_commands += 1;
            self.audit.denied("attach denied");
            log::warn!(
                target: "net-console",
                "[net-console] auth: failed validation for role={:?} ticket_present={}",
                requested_role,
                ticket_str.is_some()
            );
            self.emit_refusal(
                ConsoleVerb::Attach.ack_label(),
                RefusalReason::Policy,
                Some("detail=denied"),
            );
            if matches!(requested_role, Role::Queen) {
                lifecycle::root_mark_policy_denied();
            }
        }
        false
    }
}

#[cfg(test)]
impl<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>
    EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    pub(crate) fn serial_mut(&mut self) -> &mut SerialPort<D, RX, TX, LINE> {
        &mut self.serial
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    pub(crate) fn post_prompt_local_seat_attach_pending_for_test(&self) -> bool {
        self.post_prompt_local_seat_attach_pending
    }
}

#[cfg(feature = "net-console")]
impl<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>
    EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    /// Access the attached networking poller (test support only).
    pub fn network_mut(&mut self) -> Option<&mut (dyn NetPoller + 'a)> {
        self.net.as_deref_mut()
    }
}

#[cfg(feature = "kernel")]
#[derive(Debug)]
pub(crate) enum CommandDispatchError {
    NineDoorUnavailable {
        verb: ConsoleVerb,
    },
    UnsupportedForNineDoor {
        verb: ConsoleVerb,
    },
    Bridge {
        verb: ConsoleVerb,
        source: NineDoorBridgeError,
    },
}

#[cfg(not(feature = "kernel"))]
pub(crate) type CommandDispatchError = core::convert::Infallible;

#[cfg(feature = "kernel")]
#[cfg_attr(not(debug_assertions), allow(dead_code))]
#[inline(never)]
extern "C" fn vtable_sentinel() {}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "kernel")]
    use crate::hal::{
        HalError, WifiBoundedPhaseRecord, WifiFirmwareProofTrace, WifiHtPhaseRecord,
        WIFI_BOUNDED_PHASE_RECORD_CAPACITY, WIFI_HT_PHASE_RECORD_CAPACITY,
    };
    #[cfg(feature = "net-console")]
    use crate::net::{NetCounters, NetSelfTestStartResult, NetStatusReport, NetTelemetry};
    #[cfg(feature = "kernel")]
    use crate::ninedoor::NineDoorBridge;
    use crate::serial::test_support::LoopbackSerial;
    use crate::serial::SerialPort;
    use cohesix_ticket::{BudgetSpec, MountSpec, TicketClaims, TicketIssuer, TicketKey};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    static WIFI_DRIVER_TASK_PROGRESS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    fn wifi_driver_task_progress_test_guard() -> std::sync::MutexGuard<'static, ()> {
        WIFI_DRIVER_TASK_PROGRESS_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct TestTimer {
        ticks: HeaplessVec<TickEvent, 8>,
        index: usize,
    }

    impl TestTimer {
        fn single(tick: TickEvent) -> Self {
            let mut ticks = HeaplessVec::new();
            let _ = ticks.push(tick);
            Self { ticks, index: 0 }
        }

        fn repeated(count: usize, spacing_ms: u64) -> Self {
            let mut ticks = HeaplessVec::new();
            for i in 0..count {
                let _ = ticks.push(TickEvent {
                    tick: (i + 1) as u64,
                    now_ms: (i as u64 + 1) * spacing_ms,
                });
            }
            Self { ticks, index: 0 }
        }
    }

    impl TimerSource for TestTimer {
        fn poll(&mut self, _now_ms: u64) -> Option<TickEvent> {
            if self.index >= self.ticks.len() {
                return None;
            }
            let tick = self.ticks[self.index];
            self.index += 1;
            Some(tick)
        }
    }

    #[test]
    fn bootstrap_backoff_triggers_once_limit_reached() {
        let mut backoff = BootstrapBackoff::new(3);
        assert_eq!(backoff.observe(false), None);
        assert_eq!(backoff.observe(false), None);
        assert_eq!(backoff.observe(false), Some(3));
    }

    #[test]
    fn bootstrap_backoff_resets_when_message_staged() {
        let mut backoff = BootstrapBackoff::new(2);
        assert_eq!(backoff.observe(false), None);
        assert_eq!(backoff.observe(true), None);
        assert_eq!(backoff.observe(false), None);
        assert_eq!(backoff.observe(false), Some(2));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_diag_decodes_sdio_transfer_failure_result() {
        type TestPump<'a> = EventPump<
            'a,
            LoopbackSerial<16>,
            TestTimer,
            NullIpc,
            TicketTable<4>,
            4,
            4,
            DEFAULT_LINE_CAPACITY,
        >;

        let response_r5 = (5 << 24) | 0x0100;
        assert_eq!(
            TestPump::wifi_sdio_transfer_failure_stage(response_r5),
            "response"
        );
        assert_eq!(
            TestPump::wifi_sdio_transfer_failure_status(response_r5),
            0x0100
        );
        assert_eq!(TestPump::wifi_sdio_transfer_failure_r5(response_r5), 0x0100);

        let data_wait = (3 << 24) | 0x0000_8000;
        assert_eq!(
            TestPump::wifi_sdio_transfer_failure_stage(data_wait),
            "data-wait"
        );
        assert_eq!(TestPump::wifi_sdio_transfer_failure_r5(data_wait), 0);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn usb_runtime_entry_progress_reports_gate_one_transport_blockers() {
        type TestPump<'a> = EventPump<
            'a,
            LoopbackSerial<16>,
            TestTimer,
            NullIpc,
            TicketTable<4>,
            4,
            4,
            DEFAULT_LINE_CAPACITY,
        >;

        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_ENTRY_READY,
            ),
            1
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_ENTRY_READY,
            ),
            "linked-runtime-recv-not-ready"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_RECV_READY,
            ),
            1
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_RECV_READY,
            ),
            "linked-runtime-command-not-observed"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_POLL_READY,
            ),
            1
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_POLL_READY,
            ),
            "linked-runtime-command-not-visible"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_REPLY_PENDING,
            ),
            1
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_REPLY_PENDING,
            ),
            "linked-runtime-reply-cap-missing"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_POLL_BEGIN,
            ),
            1
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_POLL_BEGIN,
            ),
            "linked-runtime-endpoint-poll-blocked"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_RING_READ_BEGIN,
            ),
            1
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_RING_READ_BEGIN,
            ),
            "linked-runtime-ring-read-blocked"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn usb_runtime_entry_progress_reports_command_event_peek_blocker() {
        type TestPump<'a> = EventPump<
            'a,
            LoopbackSerial<16>,
            TestTimer,
            NullIpc,
            TicketTable<4>,
            4,
            4,
            DEFAULT_LINE_CAPACITY,
        >;

        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_PEEK_BEGIN,
            ),
            4
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_PEEK_BEGIN,
            ),
            "enable-slot-event-peek-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_PEEK_BEGIN,
            ),
            "inspect-event-ring-trb-read-or-cache-invalidate"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_DMA_LOAD_DONE,
            ),
            4
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_DMA_LOAD_DONE,
            ),
            "enable-slot-event-dma-load-done-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_DMA_LOAD_DONE,
            ),
            "inspect-event-ring-trb-memory-read"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_INVALIDATE_DONE,
            ),
            4
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_INVALIDATE_DONE,
            ),
            "enable-slot-event-invalidate-done-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_INVALIDATE_DONE,
            ),
            "inspect-event-trb-word-read-after-invalidate"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_BEGIN,
            ),
            4
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_BEGIN,
            ),
            "enable-slot-event-read-begin-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_BEGIN,
            ),
            "inspect-event-ring-dma-load-or-trb-read"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_DONE,
            ),
            4
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_DONE,
            ),
            "enable-slot-event-read-done-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_COMMAND_PROOF_EVENT_READ_DONE,
            ),
            "inspect-event-trb-classification-after-read"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn usb_runtime_progress_reports_address_device_blockers() {
        type TestPump<'a> = EventPump<
            'a,
            LoopbackSerial<16>,
            TestTimer,
            NullIpc,
            TicketTable<4>,
            4,
            4,
            DEFAULT_LINE_CAPACITY,
        >;

        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_BEGIN,
            ),
            5
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_BEGIN,
            ),
            "root-port-reset-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_BEGIN,
            ),
            "inspect-root-port-reset-completion"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_POLL_BEGIN,
            ),
            5
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_POLL_BEGIN,
            ),
            "root-port-reset-completion-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_RESET_POLL_BEGIN,
            ),
            "poll-root-port-reset-completion"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_CONNECT_TIMEOUT,
            ),
            "root-port-connect-timeout"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_CONNECT_TIMEOUT,
            ),
            "inspect-root-port-connect-and-power"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_DONE,
            ),
            "address-enable-slot-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ROOT_PORT_STALE_CLEANUP_DONE,
            ),
            "submit-address-enable-slot-command"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_BEGIN,
            ),
            5
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_BEGIN,
            ),
            "address-device-command-completion-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_BEGIN,
            ),
            "poll-address-device-command-completion"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_SLOT_EMPTY,
            ),
            5
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_SLOT_EMPTY,
            ),
            "address-device-command-event-slot-empty"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_EVENT_SLOT_EMPTY,
            ),
            "wait-for-address-device-command-event"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_FAILED,
            ),
            5
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_FAILED,
            ),
            "address-device-failed"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_FAILED,
            ),
            "continue-enumeration-same-controller"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_DONE,
            ),
            6
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_DONE,
            ),
            "address-device-publish-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_ADDRESS_COMMAND_DONE,
            ),
            "publish-device-addressed-detail"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_ADDRESSED,
            ),
            6
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_ADDRESSED,
            ),
            "device-descriptor-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_ADDRESSED,
            ),
            "read-device-descriptor"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_WAIT_BEGIN,
            ),
            6
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_WAIT_BEGIN,
            ),
            "device-descriptor-transfer-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_WAIT_BEGIN,
            ),
            "poll-ep0-device-descriptor-transfer"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_DATA_EVENT,
            ),
            6
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_DATA_EVENT,
            ),
            "device-descriptor-status-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_DATA_EVENT,
            ),
            "poll-ep0-device-descriptor-status"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT,
            ),
            7
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT,
            ),
            "config-descriptor-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_STATUS_EVENT,
            ),
            "read-config-descriptor"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_WAIT_BEGIN,
            ),
            6
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_WAIT_BEGIN,
            ),
            "device-descriptor-prime-transfer-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_DEVICE_DESCRIPTOR_PRIME_WAIT_BEGIN,
            ),
            "poll-ep0-device-descriptor-prime"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_WAIT_BEGIN,
            ),
            7
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_WAIT_BEGIN,
            ),
            "config-descriptor-header-transfer-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_WAIT_BEGIN,
            ),
            "poll-ep0-config-descriptor-header"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_SLOT_EMPTY,
            ),
            7
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_SLOT_EMPTY,
            ),
            "config-descriptor-header-transfer-event-slot-empty"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_TRANSFER_EVENT_SLOT_EMPTY,
            ),
            "inspect-ep0-config-descriptor-header-event-ring-empty"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_CYCLE_MISMATCH,
            ),
            "config-descriptor-header-status-event-cycle-mismatch"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT_CYCLE_MISMATCH,
            ),
            "inspect-ep0-config-descriptor-header-status-event-cycle"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT,
            ),
            "config-descriptor-full-read-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_HEADER_STATUS_EVENT,
            ),
            "read-full-config-descriptor"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_WAIT_BEGIN,
            ),
            "config-descriptor-full-transfer-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_WAIT_BEGIN,
            ),
            "poll-ep0-full-config-descriptor"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT,
            ),
            "hid-endpoint-not-ready"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_CONFIG_DESCRIPTOR_FULL_STATUS_EVENT,
            ),
            "parse-hid-keyboard-endpoint"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_IGNORED,
            ),
            7
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_IGNORED,
            ),
            "hub-set-configuration-status-event-ignored"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SET_CONFIGURATION_STATUS_EVENT_IGNORED,
            ),
            "inspect-hub-set-configuration-status-event"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_WAIT_BEGIN,
            ),
            7
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_WAIT_BEGIN,
            ),
            "hub-descriptor-transfer-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_WAIT_BEGIN,
            ),
            "poll-ep0-hub-descriptor"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DATA_EVENT,
            ),
            "hub-descriptor-status-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_DATA_EVENT,
            ),
            "poll-ep0-hub-descriptor-status"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_SLOT_EMPTY,
            ),
            "hub-descriptor-transfer-event-slot-empty"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_TRANSFER_EVENT_SLOT_EMPTY,
            ),
            "inspect-ep0-hub-descriptor-event-ring-empty"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_CYCLE_MISMATCH,
            ),
            "hub-descriptor-status-event-cycle-mismatch"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT_CYCLE_MISMATCH,
            ),
            "inspect-ep0-hub-descriptor-status-event-cycle"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_DESCRIPTOR_STATUS_EVENT,
            ),
            "evaluate-hub-context"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_BEGIN,
            ),
            7
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_BEGIN,
            ),
            "hid-endpoint-parse-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_BEGIN,
            ),
            "parse-hid-keyboard-endpoint"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_NO_INTERFACE,
            ),
            "hid-interface-not-found"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_NO_INTERFACE,
            ),
            "inspect-config-descriptor-interface-classes"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_NO_INTERRUPT_IN,
            ),
            "hid-interrupt-in-not-found"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_ENDPOINT_PARSE_NO_INTERRUPT_IN,
            ),
            "inspect-config-descriptor-endpoint-shape"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_SCAN_BEGIN,
            ),
            "hub-child-scan-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_POWER_BEGIN,
            ),
            "hub-port-power-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_POWER_BEGIN,
            ),
            "wait-hub-port-power"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_BEGIN,
            ),
            7
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_POWER_DONE,
            ),
            "hub-port-status-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_BEGIN,
            ),
            "inspect-hub-port-status-control-transfer"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_WAIT_BEGIN,
            ),
            "hub-port-status-transfer-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DATA_EVENT,
            ),
            "poll-ep0-hub-port-status-status"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_EVENT,
            ),
            "hub-port-reset-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_TRANSFER_EVENT_IGNORED,
            ),
            "hub-port-status-transfer-event-ignored"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_STATUS_TIMEOUT,
            ),
            "inspect-missing-hub-port-status-status-event"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_DONE,
            ),
            "hub-port-reset-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_STATUS_FAILED,
            ),
            "hub-port-status-failed"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_BEGIN,
            ),
            "hub-port-reset-set-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_DONE,
            ),
            "poll-hub-port-reset-completion"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_PORT_RESET_SET_FAILED,
            ),
            "hub-port-reset-set-failed"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CHILD_PROBE_BEGIN,
            ),
            "inspect-hub-child-address-or-descriptor"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CHILD_SPEED_FALLBACK_BEGIN,
            ),
            "hub-child-speed-fallback-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HUB_CHILD_SPEED_FALLBACK_BEGIN,
            ),
            "probe-hub-child-fallback-speed"
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_BEGIN,
            ),
            "hid-configure-endpoint-no-reply"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_CONFIGURE_ENDPOINT_BEGIN,
            ),
            "poll-hid-configure-endpoint"
        );
        assert_eq!(
            TestPump::usb_runtime_gate_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_READY,
            ),
            8
        );
        assert_eq!(
            TestPump::usb_runtime_blocker_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_READY,
            ),
            "first-hid-report"
        );
        assert_eq!(
            TestPump::usb_runtime_next_action_for_progress_phase(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_USB_HID_INTERRUPT_QUEUE_READY,
            ),
            "wait-first-hid-report"
        );
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn net_diag_idle_requires_zero_tx_drops() {
        let snapshot = NetDiagSnapshot::default();
        assert!(EventPump::<
            LoopbackSerial<16>,
            TestTimer,
            NullIpc,
            TicketTable<4>,
            4,
            4,
            DEFAULT_LINE_CAPACITY,
        >::net_diag_idle(
            snapshot,
            NetTelemetry {
                link_up: true,
                tx_drops: 0,
                last_poll_ms: 0,
            }
        ));
        assert!(!EventPump::<
            LoopbackSerial<16>,
            TestTimer,
            NullIpc,
            TicketTable<4>,
            4,
            4,
            DEFAULT_LINE_CAPACITY,
        >::net_diag_idle(
            snapshot,
            NetTelemetry {
                link_up: true,
                tx_drops: 1,
                last_poll_ms: 0,
            }
        ));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn net_diag_changed_ignores_tx_churn_counters() {
        let prev = NetDiagSnapshot::default();
        let mut curr = prev;
        curr.rx_irq_count = 8;
        curr.rx_kicks = 8;
        curr.rx_desc_posted = 8;
        curr.rx_used_seen = 8;
        curr.rx_frames_to_stack = 8;
        curr.poll_calls = 8;
        curr.rx_frames_into_smoltcp = 8;
        curr.accept_attempts = 8;
        curr.bytes_read = 4_096;
        curr.bytes_written = 8_192;
        curr.rx_cache_clean = 8;
        curr.rx_cache_invalidate = 8;
        curr.tx_submits = 10;
        curr.tx_kicks = 10;
        curr.tx_used_seen = 7;
        curr.tx_completions = 3;
        curr.tx_frames_from_smoltcp = 20;
        curr.outbound_frames = 20;
        curr.outbound_bytes = 1_500;
        assert!(!EventPump::<
            LoopbackSerial<16>,
            TestTimer,
            NullIpc,
            TicketTable<4>,
            4,
            4,
            DEFAULT_LINE_CAPACITY,
        >::net_diag_changed(prev, curr));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn net_diag_changed_detects_backpressure_progress() {
        let prev = NetDiagSnapshot::default();
        let mut curr = prev;
        curr.outbound_would_block = 1;
        assert!(EventPump::<
            LoopbackSerial<16>,
            TestTimer,
            NullIpc,
            TicketTable<4>,
            4,
            4,
            DEFAULT_LINE_CAPACITY,
        >::net_diag_changed(prev, curr));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn net_disabled_refusal_detail_defaults_to_generic_reason() {
        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let pump = EventPump::new(serial, timer, ipc, store, &mut audit);

        assert_eq!(
            pump.net_disabled_refusal_detail().as_str(),
            "detail=net-disabled"
        );
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn net_disabled_refusal_detail_preserves_init_cause() {
        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut cause = HeaplessString::<192>::new();
        let _ = cause.push_str("cyw43-control-plane-pure-f2-startup-link-no-reply");
        let pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network_unavailable_detail(Some(cause));

        assert_eq!(
            pump.net_disabled_refusal_detail().as_str(),
            "detail=net-disabled cause=cyw43-control-plane-pure-f2-startup-link-no-reply"
        );
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn root_console_waits_for_reachable_net_console_status() {
        let mut status = NetStatusReport {
            backend: "bcmgenet-v5",
            mode: "dhcp",
            interface_policy: "wifi",
            active_interface: "wifi",
            standby_interface: "wired",
            address_source: "wifi-associating",
            ip: HeaplessString::new(),
            gateway: HeaplessString::new(),
            dhcp_phase: "associating",
        };

        assert!(!super::net_status_allows_root_console(&status));
        status.address_source = "dhcp-pending";
        status.dhcp_phase = "selecting";
        assert!(!super::net_status_allows_root_console(&status));
        assert_eq!(
            super::net_status_pre_root_serial_release_reason(&status),
            None
        );
        status.address_source = "wifi-host-eapol-pending";
        status.dhcp_phase = "host-eapol-pending";
        assert!(!super::net_status_allows_root_console(&status));
        assert_eq!(super::net_status_terminal_failure_reason(&status), None);
        assert_eq!(
            super::net_status_pre_root_serial_release_reason(&status),
            None
        );
        status.address_source = "wifi-host-eapol-required";
        status.dhcp_phase = "host-eapol-required";
        assert!(!super::net_status_allows_root_console(&status));
        assert_eq!(
            super::net_status_terminal_failure_reason(&status),
            Some("wifi-host-eapol-required")
        );
        assert_eq!(
            super::net_status_pre_root_serial_release_reason(&status),
            Some("wifi-host-eapol-required")
        );
        status.address_source = "wifi-association-failed";
        status.dhcp_phase = "failed";
        assert!(!super::net_status_allows_root_console(&status));
        assert_eq!(
            super::net_status_terminal_failure_reason(&status),
            Some("wifi-association-failed")
        );
        status.address_source = "dhcp-failed";
        status.dhcp_phase = "failed";
        assert!(!super::net_status_allows_root_console(&status));
        assert_eq!(super::net_status_terminal_failure_reason(&status), None);
        assert_eq!(
            super::net_status_pre_root_serial_release_reason(&status),
            None
        );
        status.address_source = "future-intermediate-state";
        status.dhcp_phase = "probing";
        assert!(!super::net_status_allows_root_console(&status));
        assert_eq!(super::net_status_terminal_failure_reason(&status), None);
        assert_eq!(
            super::net_status_pre_root_serial_release_reason(&status),
            None
        );
        status.address_source = "dhcp-lease";
        status.dhcp_phase = "bound";
        assert!(super::net_status_allows_root_console(&status));
        assert_eq!(super::net_status_terminal_failure_reason(&status), None);
        status.address_source = "dev-virt";
        status.dhcp_phase = "disabled";
        assert!(super::net_status_allows_root_console(&status));
        status.address_source = "manifest-static";
        assert!(super::net_status_allows_root_console(&status));
    }

    #[test]
    fn start_cli_emits_serial_banner_and_prompt_without_network() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);

        pump.start_cli();
        let emitted = pump.serial_mut().driver_mut().drain_tx();
        let transcript = core::str::from_utf8(emitted.as_slice()).unwrap();

        assert!(transcript.contains(CONSOLE_BANNER));
        assert!(transcript.contains("Cohesix console starting"));
        assert!(!transcript.contains("Cohesix console ready"));
        assert!(!transcript.contains(CONSOLE_PROMPT));
    }

    #[test]
    fn console_ready_announcement_enables_local_seat_attach() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.start_cli();
        let emitted = pump.serial_mut().driver_mut().drain_tx();
        let transcript = core::str::from_utf8(emitted.as_slice()).unwrap();

        assert!(transcript.contains(CONSOLE_BANNER));
        assert!(!transcript.contains(CONSOLE_PROMPT));
        assert_eq!(pump.metrics().local_seat_output_keyboard_polls, 0);
        #[cfg(all(feature = "kernel", feature = "usb"))]
        assert!(!pump.post_prompt_local_seat_attach_pending_for_test());
        assert!(!pump.local_seat.as_ref().unwrap().root_console_ready());

        pump.announce_console_ready();
        let ready_emitted = pump.serial_mut().driver_mut().drain_tx();
        let ready_transcript = core::str::from_utf8(ready_emitted.as_slice()).unwrap();
        assert!(ready_transcript.contains("Cohesix console ready"));
        assert!(ready_transcript.contains("Commands:"));
        assert!(ready_transcript.ends_with(CONSOLE_PROMPT));
        #[cfg(all(feature = "kernel", feature = "usb"))]
        assert!(pump.post_prompt_local_seat_attach_pending_for_test());
        drop(pump);
        assert!(local_seat.root_console_ready());
        assert!(local_seat
            .mirrored_lines_snapshot()
            .iter()
            .any(|line| line.as_str() == CONSOLE_PROMPT));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn console_ready_wifi_warning_reaches_serial_and_hdmi_before_prompt() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status = NetStatusReport {
            backend: "cyw43",
            mode: "dhcp",
            interface_policy: "wifi",
            active_interface: "wifi",
            standby_interface: "wired",
            address_source: "wifi-psk-too-short",
            ip: HeaplessString::new(),
            gateway: HeaplessString::new(),
            dhcp_phase: "disabled",
        };
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 160,
            buffer_lines: 8,
        });
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network(&mut net)
            .with_local_seat(&mut local_seat);

        pump.announce_console_ready();
        let ready_emitted = pump.serial_mut().driver_mut().drain_tx();
        let ready_transcript = core::str::from_utf8(ready_emitted.as_slice()).unwrap();

        assert!(
            ready_transcript.contains(
                "wifi warning: code=invalid-config detail=psk-too-short action=check-wifi-password-length"
            ),
            "{ready_transcript}"
        );
        assert!(ready_transcript.ends_with(CONSOLE_PROMPT));
        drop(pump);
        assert!(local_seat
            .mirrored_lines_snapshot()
            .iter()
            .any(|line| line.as_str().contains("wifi warning: code=invalid-config")));
    }

    #[test]
    fn pre_root_boot_progress_mirrors_to_local_seat_before_prompt() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 4,
        });
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.publish_pre_root_boot_progress("[boot] waiting for Wi-Fi");
        assert!(!pump.local_seat.as_ref().unwrap().root_console_ready());
        drop(pump);

        assert!(local_seat
            .mirrored_lines_snapshot()
            .iter()
            .any(|line| line.as_str() == "[boot] waiting for Wi-Fi"));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn post_prompt_local_seat_retry_policy_uses_quiet_window_for_active_usb() {
        assert_eq!(
            post_prompt_local_seat_attach_retry_policy(false, false),
            Some((
                POST_PROMPT_LOCAL_SEAT_ATTACH_RETRY_MS,
                POST_PROMPT_LOCAL_SEAT_ATTACH_RETRY_IDLE_TURNS,
                "serial-safe-usb-progress"
            ))
        );
        assert_eq!(
            post_prompt_local_seat_attach_retry_policy(true, false),
            None
        );
        assert_eq!(
            post_prompt_local_seat_attach_retry_policy(true, true),
            Some((
                POST_PROMPT_LOCAL_SEAT_ATTACH_ACTIVE_USB_RETRY_MS,
                POST_PROMPT_LOCAL_SEAT_ATTACH_ACTIVE_USB_RETRY_IDLE_TURNS,
                "serial-safe-active-usb-progress"
            ))
        );
        assert_eq!(
            post_prompt_local_seat_attach_retry_policy(false, true),
            Some((
                POST_PROMPT_LOCAL_SEAT_ATTACH_ACTIVE_USB_RETRY_MS,
                POST_PROMPT_LOCAL_SEAT_ATTACH_ACTIVE_USB_RETRY_IDLE_TURNS,
                "serial-safe-active-usb-progress"
            ))
        );
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn post_prompt_local_seat_attach_arms_while_usb_runtime_is_active() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(4, 1_000);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);
        pump.post_prompt_local_seat_attach_pending = true;
        pump.post_prompt_local_seat_attach_not_before_ms = 0;
        pump.post_prompt_local_seat_attach_usb_active_override = Some(true);

        pump.maybe_run_post_prompt_local_seat_attach(true);
        assert!(pump.post_prompt_local_seat_attach_pending_for_test());

        pump.maybe_run_post_prompt_local_seat_attach(true);
        assert!(!pump.post_prompt_local_seat_attach_pending_for_test());
        assert_eq!(pump.post_prompt_local_seat_attach_idle_turns, 0);
        assert_eq!(pump.post_prompt_local_seat_attach_retry_turns, 0);
        assert_eq!(pump.post_prompt_local_seat_attach_not_before_ms, 0);
        assert!(pump.post_prompt_local_seat_attach_active_usb_traced);
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn post_prompt_local_seat_attach_does_not_wait_for_serial_output_drain() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(4, 1_000);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);
        pump.post_prompt_local_seat_attach_pending = true;
        pump.post_prompt_local_seat_attach_not_before_ms = 0;
        pump.serial_mut()
            .enqueue_tx_best_effort(b"pending serial output that cannot fully drain");

        pump.maybe_run_post_prompt_local_seat_attach(false);
        assert!(pump.post_prompt_local_seat_attach_pending_for_test());
        assert!(pump.serial.tx_pending());

        pump.maybe_run_post_prompt_local_seat_attach(false);
        assert!(!pump.post_prompt_local_seat_attach_pending_for_test());
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn poll_runs_post_prompt_local_seat_attach_with_serial_output_pending() {
        let driver = LoopbackSerial::<4>::new();
        let serial = SerialPort::<_, 32, 256, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(4, 1_000);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);
        pump.post_prompt_local_seat_attach_pending = true;
        pump.post_prompt_local_seat_attach_not_before_ms = 0;
        assert!(
            pump.serial_mut()
                .enqueue_tx_best_effort(&[b'x'; DEFAULT_LINE_CAPACITY])
                > 0
        );

        pump.poll();
        assert!(pump.post_prompt_local_seat_attach_pending_for_test());
        assert!(pump.serial.tx_pending());

        pump.poll();
        assert!(!pump.post_prompt_local_seat_attach_pending_for_test());
        assert!(pump.serial.tx_pending());
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn post_prompt_local_seat_retry_turn_cooldown_blocks_immediate_reentry() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(4, 1_000);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);
        pump.post_prompt_local_seat_attach_pending = true;
        pump.post_prompt_local_seat_attach_not_before_ms = 0;
        pump.post_prompt_local_seat_attach_retry_turns = 2;

        pump.maybe_run_post_prompt_local_seat_attach(false);
        assert!(pump.post_prompt_local_seat_attach_pending_for_test());
        assert_eq!(pump.post_prompt_local_seat_attach_retry_turns, 1);

        pump.maybe_run_post_prompt_local_seat_attach(false);
        assert!(pump.post_prompt_local_seat_attach_pending_for_test());
        assert_eq!(pump.post_prompt_local_seat_attach_retry_turns, 0);

        pump.maybe_run_post_prompt_local_seat_attach(false);
        assert!(pump.post_prompt_local_seat_attach_pending_for_test());
        pump.maybe_run_post_prompt_local_seat_attach(false);
        assert!(!pump.post_prompt_local_seat_attach_pending_for_test());
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn start_cli_defers_local_seat_attach_until_serial_idle_grace() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(3, 1_000);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.start_cli();
        pump.announce_console_ready();
        assert!(pump.post_prompt_local_seat_attach_pending_for_test());
        pump.serial_mut().driver_mut().push_rx(b"help\n");

        pump.poll();
        assert!(pump.post_prompt_local_seat_attach_pending_for_test());
        let rendered = String::from_utf8(
            pump.serial_mut()
                .driver_mut()
                .drain_tx()
                .into_iter()
                .collect(),
        )
        .expect("serial output must be utf8");
        assert!(rendered.contains("Commands:"), "{rendered}");

        pump.poll();
        assert!(pump.post_prompt_local_seat_attach_pending_for_test());
        pump.poll();
        assert!(!pump.post_prompt_local_seat_attach_pending_for_test());
    }

    #[test]
    fn serial_partial_input_suppresses_output_side_keyboard_polling() {
        let driver = LoopbackSerial::<8192>::new();
        driver.push_rx(b"w");
        let mut serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        assert!(serial.poll_io());
        assert!(serial.interactive_input_active());
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        local_seat.mark_root_console_ready();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.emit_serial_line("serial output");

        assert_eq!(pump.metrics().local_seat_output_keyboard_polls, 0);
        drop(pump);
        assert!(local_seat.mirrored_lines_snapshot().is_empty());
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn net_origin_output_mirrors_to_local_seat_without_keyboard_poll() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        local_seat.mark_root_console_ready();
        local_seat.enable_backend_keyboard_polling();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network(&mut net)
            .with_local_seat(&mut local_seat);
        pump.last_input_source = ConsoleInputSource::Net;

        pump.emit_console_line("REST response line");

        assert_eq!(pump.metrics().local_seat_output_keyboard_polls, 0);
        drop(pump);
        assert_eq!(net.sent.len(), 1);
        assert_eq!(net.sent[0].as_str(), "REST response line");
        assert!(local_seat
            .mirrored_lines_snapshot()
            .iter()
            .any(|line| line.as_str() == "REST response line"));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn net_origin_output_samples_bursts_after_initial_window() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 64,
        });
        local_seat.mark_root_console_ready();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network(&mut net)
            .with_local_seat(&mut local_seat);
        pump.last_input_source = ConsoleInputSource::Net;

        for idx in 0..(LOCAL_SEAT_NET_MIRROR_INITIAL_LINES + 18) {
            let line = format_message(format_args!("REST response line {}", idx));
            pump.emit_console_line(line.as_str());
        }

        assert_eq!(pump.metrics().local_seat_net_mirror_lines, 16);
        assert_eq!(pump.metrics().local_seat_net_mirror_suppressed, 18);
        assert_eq!(pump.local_seat_hdmi_pump_pass_limit(), 1);
        drop(pump);
        assert_eq!(net.sent.len(), 34);
        let mirrored = local_seat.mirrored_lines_snapshot();
        assert!(mirrored
            .iter()
            .any(|line| line.as_str() == "REST response line 15"));
        assert!(!mirrored
            .iter()
            .any(|line| line.as_str() == "REST response line 16"));
        assert!(!mirrored
            .iter()
            .any(|line| line.as_str() == "REST response line 17"));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn net_origin_mirror_sampling_keeps_stride_samples() {
        assert!(local_seat_network_mirror_sample_allowed(0));
        assert!(local_seat_network_mirror_sample_allowed(15));
        assert!(!local_seat_network_mirror_sample_allowed(16));
        assert!(!local_seat_network_mirror_sample_allowed(254));
        assert!(local_seat_network_mirror_sample_allowed(255));
        assert!(!local_seat_network_mirror_sample_allowed(256));
        assert!(local_seat_network_mirror_sample_allowed(511));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn pressured_hdmi_suppresses_net_origin_mirror_after_tcp_send() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        local_seat.mark_root_console_ready();
        local_seat.inject_linked_hdmi_pending_bytes_for_test(2_048);
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network(&mut net)
            .with_local_seat(&mut local_seat);
        pump.last_input_source = ConsoleInputSource::Net;

        pump.emit_console_line("REST response line");

        assert_eq!(pump.metrics().local_seat_net_mirror_lines, 0);
        assert_eq!(pump.metrics().local_seat_net_mirror_suppressed, 1);
        drop(pump);
        assert_eq!(net.sent.len(), 1);
        assert_eq!(net.sent[0].as_str(), "REST response line");
        assert!(!local_seat
            .mirrored_lines_snapshot()
            .iter()
            .any(|line| line.as_str() == "REST response line"));
    }

    #[test]
    fn serial_input_skips_pre_runtime_local_seat_polling() {
        let driver = LoopbackSerial::<8192>::new();
        driver.push_rx(b"help\n");
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        local_seat.mark_root_console_ready();
        local_seat.enable_backend_keyboard_polling();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.poll();

        assert_eq!(pump.last_input_source, ConsoleInputSource::Serial);
        let rendered = String::from_utf8(
            pump.serial_mut()
                .driver_mut()
                .drain_tx()
                .into_iter()
                .collect(),
        )
        .expect("serial output must be utf8");
        assert!(rendered.contains("Commands:"), "{rendered}");
        drop(pump);
        assert_eq!(local_seat.keyboard_trace().backend_poll_calls, 0);
    }

    #[test]
    fn serial_tx_pending_still_services_local_seat_backend() {
        let driver = LoopbackSerial::<128>::new();
        let serial = SerialPort::<_, 128, 128, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        local_seat.mark_root_console_ready();
        local_seat.enable_backend_keyboard_polling();
        local_seat.enqueue_keyboard_bytes(b"x");
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        assert_eq!(
            pump.serial_mut()
                .enqueue_tx_best_effort(b"pending serial output"),
            21
        );
        assert!(pump.serial_mut().tx_pending());
        assert!(pump.consume_local_seat(LocalSeatConsumePhase::PreRuntime, true));
        assert!(pump.serial_mut().tx_pending());
        drop(pump);

        let trace = local_seat.keyboard_trace();
        assert_eq!(trace.backend_poll_calls, 1);
        assert_eq!(trace.drained_bytes, 1);
        assert_eq!(trace.echoed_bytes, 1);
        assert_eq!(trace.queued_bytes, 0);
    }

    #[test]
    fn local_seat_output_defers_behind_serial_tx_pressure() {
        let driver = LoopbackSerial::<128>::new();
        let serial = SerialPort::<_, 128, 128, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        local_seat.mark_root_console_ready();
        local_seat.enable_backend_keyboard_polling();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        assert_eq!(
            pump.serial_mut()
                .enqueue_tx_best_effort(b"pending serial output"),
            21
        );
        pump.last_input_source = ConsoleInputSource::LocalSeat;
        pump.console_input_turn_active = true;
        pump.console_input_turn_output_budget = CONSOLE_INPUT_TURN_IMMEDIATE_OUTPUT_LINES;
        assert!(pump.serial_mut().tx_pending());
        assert!(pump.should_defer_physical_console_output());

        pump.emit_serial_line("PONG");
        pump.emit_prompt();

        assert_eq!(pump.pending_console_output.len(), 2);
        assert!(pump.pending_console_output.iter().any(|output| {
            output.kind == PendingConsoleOutputKind::Line && output.text.as_str() == "PONG"
        }));
        assert!(pump.pending_console_output.iter().any(|output| {
            output.kind == PendingConsoleOutputKind::Prompt
                && output.text.as_str() == CONSOLE_PROMPT
        }));
        assert_eq!(pump.metrics().physical_console_output_deferred, 2);
        assert!(pump.serial_mut().tx_pending());
    }

    #[test]
    fn output_service_drains_display_controls_without_command_text() {
        let driver = LoopbackSerial::<128>::new();
        let serial = SerialPort::<_, 128, 128, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        local_seat.mark_root_console_ready();
        local_seat.enable_backend_keyboard_polling();
        local_seat.enqueue_keyboard_bytes(b"\x1b[Bx");
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.service_local_seat_keyboard_during_output();
        drop(pump);

        let trace = local_seat.keyboard_trace();
        assert_eq!(trace.backend_poll_calls, 1);
        assert_eq!(trace.drained_bytes, 3);
        assert_eq!(trace.echoed_bytes, 3);
        assert_eq!(trace.queued_bytes, 1);

        let mut remaining = [0u8; 4];
        assert_eq!(local_seat.drain_keyboard_bytes(&mut remaining), 1);
        assert_eq!(remaining[0], b'x');
    }

    #[test]
    fn local_line_does_not_block_hdmi_echo_pump_gate() {
        let driver = LoopbackSerial::<128>::new();
        let serial = SerialPort::<_, 128, 128, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        local_seat.mark_root_console_ready();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.local_line.push('x').unwrap();

        assert!(pump.physical_console_input_pending_for_output());
        assert!(!pump.physical_console_input_pending_for_display_pump());

        let accepted = pump
            .local_seat
            .as_mut()
            .expect("local seat attached")
            .enqueue_keyboard_bytes(b"y");
        assert_eq!(accepted, 1);
        assert!(pump.physical_console_input_pending_for_display_pump());

        let mut drained = [0u8; 1];
        assert_eq!(
            pump.local_seat
                .as_mut()
                .expect("local seat attached")
                .drain_keyboard_bytes(&mut drained),
            1
        );
        assert!(!pump.physical_console_input_pending_for_display_pump());

        pump.local_seat_chunk_input_pending = true;
        assert!(pump.physical_console_input_pending_for_display_pump());
    }

    #[test]
    fn local_seat_burst_drains_before_hdmi_echo_pump_is_allowed() {
        let driver = LoopbackSerial::<512>::new();
        let serial = SerialPort::<_, 512, 512, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 192,
            buffer_lines: 4,
        });
        local_seat.mark_root_console_ready();
        let burst_bytes = LOCAL_SEAT_BURST_DRAIN_PASSES_PER_TURN * KEYBOARD_POLL_CHUNK_BYTES;
        let mut payload = Vec::new();
        payload.resize(burst_bytes + 64, b'a');
        assert_eq!(local_seat.enqueue_keyboard_bytes(&payload), payload.len());
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        assert!(pump.consume_local_seat(LocalSeatConsumePhase::PreRuntime, true));
        let trace = pump
            .local_seat
            .as_ref()
            .expect("local seat attached")
            .keyboard_trace();
        assert_eq!(trace.queued_bytes, 64);
        assert_eq!(trace.drained_bytes, burst_bytes as u64);
        assert_eq!(trace.echoed_bytes, burst_bytes as u64);
        assert!(pump.physical_console_input_pending_for_display_pump());

        assert!(pump.consume_local_seat(LocalSeatConsumePhase::PriorityFollowup, false));
        let trace = pump
            .local_seat
            .as_ref()
            .expect("local seat attached")
            .keyboard_trace();
        assert_eq!(trace.queued_bytes, 0);
        assert_eq!(trace.accepted_bytes, payload.len() as u64);
        assert_eq!(trace.drained_bytes, payload.len() as u64);
        assert_eq!(trace.echoed_bytes, payload.len() as u64);
        assert_eq!(trace.dropped_bytes, 0);
        assert!(!pump.physical_console_input_pending_for_display_pump());
        assert!(pump.physical_console_input_pending_for_output());
    }

    #[test]
    fn local_seat_small_input_keeps_single_pass_drain_contract() {
        let driver = LoopbackSerial::<512>::new();
        let serial = SerialPort::<_, 512, 512, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 192,
            buffer_lines: 4,
        });
        local_seat.mark_root_console_ready();
        local_seat.enable_backend_keyboard_polling();
        assert_eq!(local_seat.enqueue_keyboard_bytes(b"abc"), 3);
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        assert!(pump.consume_local_seat(LocalSeatConsumePhase::PreRuntime, true));
        let trace = pump
            .local_seat
            .as_ref()
            .expect("local seat attached")
            .keyboard_trace();

        assert_eq!(
            trace.backend_poll_calls,
            LOCAL_SEAT_BACKEND_POLL_PASSES_PER_TURN as u64
        );
        assert_eq!(trace.drained_bytes, 3);
        assert_eq!(trace.echoed_bytes, 3);
        assert_eq!(trace.queued_bytes, 0);
    }

    #[test]
    fn local_seat_usb_burst_proof_requires_caught_up_zero_drop_echo() {
        let burst_bytes =
            (LOCAL_SEAT_BURST_DRAIN_PASSES_PER_TURN * KEYBOARD_POLL_CHUNK_BYTES) as u64;

        assert!(local_seat_usb_burst_proof(
            burst_bytes,
            burst_bytes,
            burst_bytes,
            0
        ));
        assert!(!local_seat_usb_burst_proof(
            burst_bytes - 1,
            burst_bytes - 1,
            burst_bytes - 1,
            0
        ));
        assert!(!local_seat_usb_burst_proof(
            burst_bytes,
            burst_bytes - 1,
            burst_bytes,
            0
        ));
        assert!(!local_seat_usb_burst_proof(
            burst_bytes,
            burst_bytes,
            burst_bytes - 1,
            0
        ));
        assert!(!local_seat_usb_burst_proof(
            burst_bytes,
            burst_bytes,
            burst_bytes,
            1
        ));
    }

    struct NullIpc;

    impl IpcDispatcher for NullIpc {
        fn dispatch(&mut self, _now_ms: u64) {}
    }

    #[test]
    fn usb_runtime_progress_supersession_requires_keyboard_byte() {
        type ConsoleTestPump =
            EventPump<'static, LoopbackSerial<32>, TestTimer, NullIpc, TicketTable<4>, 32, 32, 32>;

        assert!(ConsoleTestPump::usb_runtime_progress_superseded_by_keyboard(true, 10, 7));
        assert!(!ConsoleTestPump::usb_runtime_progress_superseded_by_keyboard(false, 10, 7));
        assert!(!ConsoleTestPump::usb_runtime_progress_superseded_by_keyboard(true, 10, 10));
    }

    #[cfg(feature = "kernel")]
    type KernelConsoleTestPump =
        EventPump<'static, LoopbackSerial<32>, TestTimer, NullIpc, TicketTable<4>, 32, 32, 32>;

    #[cfg(feature = "kernel")]
    struct StubIpc {
        dispatched: bool,
        message: Option<BootstrapMessage>,
    }

    #[cfg(feature = "kernel")]
    impl StubIpc {
        fn new(message: BootstrapMessage) -> Self {
            Self {
                dispatched: false,
                message: Some(message),
            }
        }
    }

    #[cfg(feature = "kernel")]
    impl IpcDispatcher for StubIpc {
        fn dispatch(&mut self, _now_ms: u64) {
            self.dispatched = true;
        }

        fn take_bootstrap_message(&mut self) -> Option<BootstrapMessage> {
            if self.dispatched {
                self.message.take()
            } else {
                None
            }
        }
    }

    #[cfg(feature = "kernel")]
    struct ProbeIpc {
        staged: Option<BootstrapMessage>,
        pending: Option<BootstrapMessage>,
        polls: u32,
    }

    #[cfg(feature = "kernel")]
    impl ProbeIpc {
        fn new(message: BootstrapMessage) -> Self {
            Self {
                staged: None,
                pending: Some(message),
                polls: 0,
            }
        }
    }

    #[cfg(feature = "kernel")]
    impl IpcDispatcher for ProbeIpc {
        fn dispatch(&mut self, _now_ms: u64) {
            if self.staged.is_none() {
                self.staged = self.pending.take();
            }
        }

        fn take_bootstrap_message(&mut self) -> Option<BootstrapMessage> {
            self.staged.take()
        }

        fn bootstrap_poll(&mut self, _now_ms: u64) -> bool {
            self.polls = self.polls.saturating_add(1);
            if self.polls > 1 {
                panic!("bootstrap probe failed to observe drained message");
            }
            false
        }
    }

    struct AuditLog {
        entries: heapless::Vec<HeaplessString<64>, 32>,
        denials: heapless::Vec<HeaplessString<64>, 32>,
    }

    impl AuditLog {
        fn new() -> Self {
            Self {
                entries: heapless::Vec::new(),
                denials: heapless::Vec::new(),
            }
        }
    }

    fn issue_token(secret: &str, role: Role) -> String {
        let budget = match role {
            Role::Queen => BudgetSpec::unbounded(),
            Role::WorkerHeartbeat => BudgetSpec::default_heartbeat(),
            Role::WorkerGpu => BudgetSpec::default_gpu(),
            Role::WorkerBus => BudgetSpec::default_heartbeat(),
            Role::WorkerLora => BudgetSpec::default_heartbeat(),
        };
        let issuer = TicketIssuer::new(secret);
        let claims = TicketClaims::new(role, budget, None, MountSpec::empty(), unix_time_ms());
        issuer.issue(claims).unwrap().encode().unwrap()
    }

    fn unix_time_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    impl AuditSink for AuditLog {
        fn info(&mut self, message: &str) {
            let mut buf = HeaplessString::new();
            let _ = buf.push_str(message);
            let _ = self.entries.push(buf);
        }

        fn denied(&mut self, message: &str) {
            let mut buf = HeaplessString::new();
            let _ = buf.push_str(message);
            let _ = self.denials.push(buf);
        }
    }

    #[cfg(feature = "kernel")]
    struct ReachableRootGuard;

    #[cfg(feature = "kernel")]
    impl ReachableRootGuard {
        fn new(now_ms: u64) -> Self {
            crate::lifecycle::root_mark_session_active(now_ms);
            Self
        }
    }

    #[cfg(feature = "kernel")]
    impl Drop for ReachableRootGuard {
        fn drop(&mut self) {
            crate::lifecycle::root_mark_cut(crate::lifecycle::RootCutReason::NetworkUnreachable);
        }
    }

    #[cfg(feature = "net-console")]
    struct FakeNet {
        lines: heapless::Vec<ConsoleLine, 64>,
        sent: heapless::Vec<HeaplessString<DEFAULT_LINE_CAPACITY>, 64>,
        start_result: NetSelfTestStartResult,
        status: NetStatusReport,
        counters: NetCounters,
        polls: usize,
        tcp_flushes: usize,
        tcp_flush_send_counts: heapless::Vec<usize, 32>,
        tcp_flush_activity_remaining: usize,
        exhaust_poll_budget: bool,
        exhaust_flush_budget: bool,
        disconnect_requests: usize,
    }

    #[cfg(feature = "net-console")]
    impl FakeNet {
        fn new() -> Self {
            Self {
                lines: heapless::Vec::new(),
                sent: heapless::Vec::new(),
                start_result: NetSelfTestStartResult::Unsupported,
                status: NetStatusReport::default(),
                counters: NetCounters::default(),
                polls: 0,
                tcp_flushes: 0,
                tcp_flush_send_counts: heapless::Vec::new(),
                tcp_flush_activity_remaining: 0,
                exhaust_poll_budget: false,
                exhaust_flush_budget: false,
                disconnect_requests: 0,
            }
        }
    }

    #[cfg(feature = "net-console")]
    impl NetPoller for FakeNet {
        fn poll(&mut self, _now_ms: u64) -> bool {
            self.polls = self.polls.saturating_add(1);
            true
        }

        fn poll_with_budget(
            &mut self,
            _now_ms: u64,
            budget: &mut DriverServiceBudget,
        ) -> Result<bool, crate::hal::driver_task::DriverServiceBudgetError> {
            if self.exhaust_poll_budget {
                budget.charge_bytes(self.driver_task_contract().budget.max_bytes_per_turn)?;
            } else {
                budget.charge_ops(1)?;
                budget.charge_frames(1)?;
            }
            Ok(self.poll(_now_ms))
        }

        fn flush_tcp_with_budget(
            &mut self,
            _now_ms: u64,
            budget: &mut DriverServiceBudget,
        ) -> Result<bool, crate::hal::driver_task::DriverServiceBudgetError> {
            if self.exhaust_flush_budget {
                budget.charge_bytes(self.driver_task_contract().budget.max_bytes_per_turn)?;
            }
            self.tcp_flushes = self.tcp_flushes.saturating_add(1);
            let _ = self.tcp_flush_send_counts.push(self.sent.len());
            let activity = self.tcp_flush_activity_remaining != 0;
            self.tcp_flush_activity_remaining = self.tcp_flush_activity_remaining.saturating_sub(1);
            Ok(activity)
        }

        fn driver_task_contract(&self) -> crate::hal::driver_task::DriverTaskContract {
            crate::hal::driver_task::VIRTIO_NET_DRIVER_TASK_CONTRACT
        }

        fn telemetry(&self) -> NetTelemetry {
            NetTelemetry {
                link_up: true,
                tx_drops: 0,
                last_poll_ms: 0,
            }
        }

        fn stats(&self) -> NetCounters {
            self.counters
        }

        fn drain_console_lines(&mut self, _now_ms: u64, visitor: &mut dyn FnMut(ConsoleLine)) {
            let _ = self.drain_console_lines_bounded(_now_ms, usize::MAX, visitor);
        }

        fn drain_console_lines_bounded(
            &mut self,
            _now_ms: u64,
            max_lines: usize,
            visitor: &mut dyn FnMut(ConsoleLine),
        ) -> usize {
            let mut drained = 0usize;
            while !self.lines.is_empty() {
                if drained >= max_lines {
                    break;
                }
                let line = self.lines.remove(0);
                visitor(line);
                drained = drained.saturating_add(1);
            }
            drained
        }

        fn ingest_snapshot(&self) -> IngestSnapshot {
            IngestSnapshot::default()
        }

        fn send_console_line(&mut self, line: &str) -> bool {
            let mut buf = HeaplessString::new();
            if buf.push_str(line).is_err() {
                return false;
            }
            let _ = self.sent.push(buf);
            true
        }

        fn request_disconnect(&mut self) {
            self.disconnect_requests = self.disconnect_requests.saturating_add(1);
        }

        fn start_self_test(&mut self, _now_ms: u64) -> NetSelfTestStartResult {
            self.start_result
        }

        fn status_report(&self) -> NetStatusReport {
            self.status.clone()
        }
    }

    #[cfg(feature = "kernel")]
    struct FakeWifiDebug {
        snapshot: WifiDebugSnapshot,
        control_trace: WifiControlPlaneTrace,
        ht_ready: bool,
        calls: heapless::Vec<&'static str, 8>,
        expect_breadcrumb_suppression: bool,
        breadcrumb_suppression_observed: bool,
        runtime_required: bool,
    }

    #[cfg(feature = "kernel")]
    impl FakeWifiDebug {
        fn new() -> Self {
            Self {
                snapshot: WifiDebugSnapshot {
                    power_state: WifiPowerState::On,
                    reset_state: WifiResetState::Deasserted,
                    current_clock_hz: 400_000,
                    preferred_data_clock_hz: 3_125_000,
                    bus_width: SdioBusWidth::FourBit,
                    card_ready: true,
                    card_rca: 1,
                    card_ocr: 0xb0ff_ff00,
                    io_enable: Some(0x02),
                    io_ready: Some(0x02),
                    chipclkcsr: Some(0x50),
                    wakeupctrl: Some(0x02),
                    sleepcsr: Some(0x01),
                    cardcap: Some(0x08),
                    programmed_backplane_window: Some(0x0019_8000),
                    shadow_backplane_window: Some(0x0019_8000),
                    shadow_backplane_fn_addr: Some(0x08000),
                    control_plane_frame_recovery_stage: Some("control-plane-reply-full-block-read"),
                    control_plane_frame_recovery_policy: Some("linux-rxfail"),
                    control_plane_frame_recovery_write: Some(false),
                    control_plane_frame_recovery_drained: Some(false),
                    control_plane_frame_recovery_count: Some(0x0040),
                    control_plane_bootstrap_phase: "first-write-startup-link",
                    control_plane_reply_mode: "startup-link",
                    control_plane_reply_attempts: 1,
                    control_plane_reply_empty_polls: 0,
                    control_plane_no_ht_transport: true,
                    control_plane_probe_pending: true,
                    control_plane_startup_link_stable: false,
                    control_plane_startup_profile_locked: true,
                    control_plane_startup_profile_reason: "promoted-io-unstable",
                    control_plane_promoted_probe_pending: false,
                    debug_snapshot_source: "cached",
                    debug_snapshot_stage: "control-plane-startup-link-rearm-stalled",
                    control_plane_startup_link_rescue_cycles: 1,
                    control_plane_startup_link_rescue_limit: 2,
                    control_plane_passive_startup_link_empty_poll_limit: 8,
                    control_plane_f2_state: "latched-linux-configured-no-iorx",
                    control_plane_sdhci_read_diag: "f1-reply-read-command-phase-no-data-active",
                    control_plane_exact_error: "cyw43-control-plane-sideband-command-stall",
                },
                control_trace: WifiControlPlaneTrace {
                    cccr_io_enable: Some(0x06),
                    cccr_io_ready: Some(0x02),
                    cccr_int_enable: Some(0x07),
                    f1_rframe_lo: Some(0x40),
                    f1_rframe_hi: Some(0x00),
                    f1_watermark: Some(0x08),
                    f1_device_ctl: Some(0x02),
                    f1_mesbusyctl: Some(0x01),
                    block_size_shadow: 0x0000_0200,
                    transfer_mode_shadow: 0x0000_0033,
                    backplane_window_low: 0x00,
                    backplane_window_mid: 0x80,
                    backplane_window_high: 0x19,
                    cached_source: "cached",
                    cached_stage: "control-plane-startup-link-rearm-stalled",
                    cached_exact_error: "cyw43-control-plane-sideband-command-stall",
                    cached_sdhci_read_diag: "f1-reply-read-command-phase-no-data-active",
                    cached_f2_state: "latched-linux-configured-no-iorx",
                    cached_cccr_io_enable: Some(0x06),
                    cached_cccr_io_ready: Some(0x02),
                    cached_cccr_int_enable: Some(0x07),
                    cached_cccr_bus_interface: Some(0x80),
                    cached_cccr_speed: Some(0x02),
                    cached_cccr_cardcap: Some(0x08),
                    cached_fbr1_block_size: Some(64),
                    cached_fbr2_block_size: Some(512),
                    bounded_phase_count: 1,
                    bounded_phase_records: {
                        let mut records =
                            [WifiBoundedPhaseRecord::EMPTY; WIFI_BOUNDED_PHASE_RECORD_CAPACITY];
                        records[0] = WifiBoundedPhaseRecord {
                            stage: "control-plane-startup-link-rearm-stalled",
                            action: "cached-failure",
                            mode: "startup-link",
                            current_clock_hz: 400_000,
                            bus_width: "4bit",
                            no_ht_transport: true,
                        };
                        records
                    },
                },
                ht_ready: true,
                calls: heapless::Vec::new(),
                expect_breadcrumb_suppression: false,
                breadcrumb_suppression_observed: false,
                runtime_required: false,
            }
        }

        fn require_breadcrumb_suppression(&mut self) {
            if self.expect_breadcrumb_suppression {
                assert!(
                    crate::hal::pi4_wifi::wifi_breadcrumb_uart_suppression_depth_for_test() != 0,
                    "wifi debug HAL callback ran without suppressing raw pi4-wifi UART breadcrumbs"
                );
                self.breadcrumb_suppression_observed = true;
            }
        }

        fn push_call(&mut self, call: &'static str) {
            self.require_breadcrumb_suppression();
            let _ = self.calls.push(call);
        }
    }

    #[cfg(feature = "kernel")]
    impl WifiDebugOps for FakeWifiDebug {
        fn dump_state(&mut self, _stage: &'static str) -> Result<WifiDebugSnapshot, HalError> {
            self.push_call("dump-state");
            if self.runtime_required {
                return Err(HalError::Unsupported(
                    "pi4-wifi-driver-task-runtime-required",
                ));
            }
            Ok(self.snapshot)
        }

        fn firmware_contract_trace(&mut self) -> Option<WifiFirmwareContractTrace> {
            self.require_breadcrumb_suppression();
            Some(WifiFirmwareContractTrace {
                firmware_len: 0x14000,
                nvram_len: 0x0180,
                clm_len: Some(0x4000),
                board_type: "brcm,bcm43455-fmac",
                reset_vector: Some(0x0019_8000),
                firmware_download_verified: false,
                armcr4_release_attempts: 1,
                sr_kso_clock_ready: false,
                alp_request: 0x08,
                ht_request: 0x10,
                ht_retry_request: 0x10,
                force_ht_after_proof_request: None,
                chipclkcsr: Some(0x50),
                wakeupctrl: Some(0x02),
                sleepcsr: Some(0x01),
                cardcap: Some(0x08),
                f1_state: "enabled-ready",
                f2_state: "disabled-not-ready",
                current_clock_hz: 400_000,
                preferred_data_clock_hz: 3_125_000,
                blocker: "chipclkcsr-ht-avail-missing",
                next_step: "linux-capture-post-release-chipclkcsr",
                proof: Some(WifiFirmwareProofTrace {
                    source: "cached",
                    upload_state: "uploaded",
                    nvram_tail_state: "written",
                    reset_vector_state: "written",
                    cpuhalt_state: "released",
                    precondition_state: "alp-only",
                    readback_status: "skipped",
                    verified: false,
                    armcr4_release_attempts: 1,
                    upload_clock_hz: 400_000,
                }),
                ht_summary: "ht-timeout-cached",
                function2_gate: "function2-disabled-until-ht-proof",
                ht_phase_count: 1,
                ht_phase_records: {
                    let mut records = [WifiHtPhaseRecord::EMPTY; WIFI_HT_PHASE_RECORD_CAPACITY];
                    records[0] = WifiHtPhaseRecord {
                        stage: "debug-probe-ht",
                        status: "active-ht-timeout",
                        chipclkcsr: Some(0x50),
                        wakeupctrl: Some(0x02),
                        sleepcsr: Some(0x01),
                        cardcap: Some(0x08),
                    };
                    records
                },
            })
        }

        fn sdhci_contract_trace(&mut self) -> Option<WifiSdhciContractTrace> {
            self.require_breadcrumb_suppression();
            Some(WifiSdhciContractTrace {
                current_diag: "f1-reply-read-command-phase-no-data-active",
                preserved_diag: "f2-reply-read-command-phase-no-data-active",
                resolved_diag: "f1-reply-read-command-phase-no-data-active",
                current_cmd: Some(53),
                current_arg: Some(0x1400_8000),
                current_present: Some(0x0001_0002),
                current_int_status: Some(0x0000_0001),
                preserved_cmd: Some(53),
                preserved_arg: Some(0x2400_8000),
                preserved_present: Some(0x0001_0002),
                preserved_int_status: Some(0x0000_0002),
            })
        }

        fn control_plane_trace(&mut self) -> Option<WifiControlPlaneTrace> {
            self.require_breadcrumb_suppression();
            Some(self.control_trace)
        }

        fn probe_ht_clock(&mut self) -> Result<bool, HalError> {
            self.push_call("probe-ht");
            if self.runtime_required {
                return Err(HalError::Unsupported(
                    "pi4-wifi-driver-task-runtime-required",
                ));
            }
            Ok(self.ht_ready)
        }

        fn load_firmware(&mut self) -> Result<WifiDebugSnapshot, HalError> {
            self.push_call("load-fw");
            if self.runtime_required {
                return Err(HalError::Unsupported(
                    "pi4-wifi-driver-task-runtime-required",
                ));
            }
            Ok(self.snapshot)
        }

        fn retry_transport_and_firmware(&mut self) -> Result<WifiDebugSnapshot, HalError> {
            self.push_call("retry");
            if self.runtime_required {
                return Err(HalError::Unsupported(
                    "pi4-wifi-driver-task-runtime-required",
                ));
            }
            Ok(self.snapshot)
        }
    }

    #[test]
    fn pump_bootstrap_logs_subsystems() {
        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        pump.poll();
        let metrics = pump.metrics();
        drop(pump);
        assert!(audit.entries.iter().any(|e| e.contains("event-pump")));
        assert_eq!(metrics.timer_ticks, 1);
    }

    #[test]
    fn timer_tick_publishes_hal_timebase() {
        let set_count = crate::hal::timebase_set_count();

        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 5 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);

        pump.poll();

        assert_eq!(pump.now_ms, 5);
        assert!(crate::hal::timebase_set_count() > set_count);
    }

    #[test]
    fn authentication_throttles_failures() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::repeated(3, 5);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "secret").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        let driver = pump.serial_mut().driver_mut();
        driver.push_rx(b"attach queen wrong\nattach queen wrong\n");
        pump.poll();
        drop(pump);
        assert!(audit.denials.iter().any(|line| line.contains("attach")));
        assert!(!audit.denials.is_empty());
    }

    #[test]
    fn queen_attach_without_ticket_is_permitted() {
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "bootstrap").unwrap();
        assert!(store.validate(Role::Queen, None));
        assert!(store.validate(Role::Queen, Some("   ")));
    }

    #[test]
    fn worker_roles_still_require_tickets() {
        let mut store: TicketTable<4> = TicketTable::new();
        store
            .register(Role::WorkerHeartbeat, "worker-ticket")
            .unwrap();
        assert!(!store.validate(Role::WorkerHeartbeat, None));
        assert!(!store.validate(Role::WorkerHeartbeat, Some("  ")));
        let token = issue_token("worker-ticket", Role::WorkerHeartbeat);
        assert!(store.validate(Role::WorkerHeartbeat, Some(token.as_str())));
    }

    #[test]
    fn precomputed_ticket_key_validates_like_secret_registration() {
        let mut store: TicketTable<4> = TicketTable::new();
        store
            .register_key(
                Role::WorkerHeartbeat,
                TicketKey::from_secret("worker-ticket"),
            )
            .unwrap();

        let token = issue_token("worker-ticket", Role::WorkerHeartbeat);
        assert!(store.validate(Role::WorkerHeartbeat, Some(token.as_str())));
    }

    #[cfg(feature = "kernel")]
    struct CaptureBootstrap {
        messages: heapless::Vec<BootstrapMessage, 4>,
    }

    #[cfg(feature = "kernel")]
    impl CaptureBootstrap {
        fn new() -> Self {
            Self {
                messages: heapless::Vec::new(),
            }
        }
    }

    #[cfg(feature = "kernel")]
    impl BootstrapMessageHandler for CaptureBootstrap {
        fn handle(&mut self, message: &BootstrapMessage, audit: &mut dyn AuditSink) {
            let mut line = HeaplessString::<96>::new();
            let _ = line.push_str("handler bootstrap badge=");
            let _ = write!(line, "0x{:016x}", message.badge);
            audit.info(line.as_str());
            let _ = self.messages.push(message.clone());
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn bootstrap_handler_receives_staged_message() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 5 });

        let mut payload: HeaplessVec<sel4_sys::seL4_Word, { MAX_BOOTSTRAP_WORDS }> =
            HeaplessVec::new();
        let _ = payload.push(0x1234);
        let message = BootstrapMessage {
            badge: 0xDEAD,
            info: sel4_sys::seL4_MessageInfo::new(0xCA, 0, 0, 1),
            payload,
        };
        let ipc = StubIpc::new(message.clone());
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let handler = &mut CaptureBootstrap::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_bootstrap_handler(handler);

        pump.poll();

        assert_eq!(handler.messages.len(), 1);
        assert_eq!(handler.messages[0].badge, 0xDEAD);
        assert_eq!(handler.messages[0].payload.as_slice(), &[0x1234]);
        assert!(audit
            .entries
            .iter()
            .any(|entry| entry.contains("handler bootstrap")));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn bootstrap_probe_exits_after_poll_consumes_message() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 5 });

        let mut payload: HeaplessVec<sel4_sys::seL4_Word, { MAX_BOOTSTRAP_WORDS }> =
            HeaplessVec::new();
        let _ = payload.push(0xC0DE);
        let message = BootstrapMessage {
            badge: 0xBEEF,
            info: sel4_sys::seL4_MessageInfo::new(0xAA, 0, 0, 1),
            payload,
        };

        let ipc = ProbeIpc::new(message.clone());
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let handler = &mut CaptureBootstrap::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_bootstrap_handler(handler);

        pump.bootstrap_probe();

        let metrics = pump.metrics();
        drop(pump);

        assert_eq!(handler.messages.len(), 1);
        assert_eq!(handler.messages[0], message);
        assert_eq!(metrics.bootstrap_messages, 1);
    }

    #[test]
    fn smp_activity_emits_userspace_diagnostics_and_hdmi_mirror() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 512, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 42,
        });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 256,
            buffer_lines: 32,
        });
        local_seat.mark_root_console_ready();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);
        pump.serial_mut().driver_mut().push_rx(b"smp activity\n");

        pump.poll();

        let transcript = pump.serial_mut().driver_mut().drain_tx();
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "[smp] activity begin source=userspace benchmark=off hdmi=high-impact-only"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("[smp] activity pump now_ms="),
            "{rendered}"
        );
        assert!(
            rendered.contains("serial_rx_backpressure=")
                && rendered.contains("serial_tx_backpressure="),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "[smp] activity local-seat runtime=present attached=no keyboard_device=usb-kbd0 display=hdmi0"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("[smp] activity driver-proof contracts="),
            "{rendered}"
        );
        assert!(
            rendered.contains("[smp] activity rates sample=first run_again=yes"),
            "{rendered}"
        );
        let affinity_line = if cfg!(feature = "kernel") {
            "[smp] activity affinity enabled="
        } else {
            "[smp] activity affinity unavailable=host-test"
        };
        assert!(rendered.contains(affinity_line), "{rendered}");
        assert!(rendered.contains("OK SMP mode=activity"), "{rendered}");
        assert!(
            !rendered.contains("debug scheduler dump begin"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("DRIVER_TASK_DEFAULT requested="),
            "{rendered}"
        );
        drop(pump);

        let mirrored = local_seat.mirrored_lines_snapshot();
        assert!(mirrored.iter().any(|line| line.contains(
            "[smp] activity begin source=userspace benchmark=off hdmi=high-impact-only"
        )));
        assert!(mirrored
            .iter()
            .any(|line| line.contains("[smp] activity end")));
    }

    #[cfg(not(all(feature = "kernel", sel4_config_debug_build)))]
    #[test]
    fn smp_snapshot_stays_distinct_from_activity_on_non_debug_builds() {
        let driver = LoopbackSerial::<2048>::new();
        let serial = SerialPort::<_, 512, 2048, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 42,
        });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        pump.serial_mut().driver_mut().push_rx(b"smp\n");

        pump.poll();

        let transcript = pump.serial_mut().driver_mut().drain_tx();
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(rendered.contains("ERR reason=unsupported"), "{rendered}");
        assert!(
            rendered.contains("ERR SMP reason=policy detail=unsupported"),
            "{rendered}"
        );
        assert!(!rendered.contains("[smp] activity begin"), "{rendered}");
        assert!(!rendered.contains("OK SMP mode=activity"), "{rendered}");
    }

    #[cfg(all(feature = "kernel", sel4_config_debug_build))]
    #[test]
    fn smp_snapshot_preflushes_pending_serial_and_dumps() {
        let driver = LoopbackSerial::<4>::new();
        let serial = SerialPort::<_, 512, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 42,
        });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        assert!(
            pump.serial_mut()
                .enqueue_tx_best_effort(b"pending serial output")
                > 0
        );
        assert!(pump.serial_mut().tx_pending());

        let detail = pump.emit_smp(SmpMode::Snapshot);

        assert_eq!(detail, Some("mode=snapshot"));
        assert!(pump.last_smp_activity_snapshot.is_none());
        let mut transcript = Vec::new();
        for _ in 0..64 {
            pump.serial_mut().flush_tx();
            transcript.extend(pump.serial_mut().driver_mut().drain_tx());
            if !pump.serial_mut().tx_pending() {
                break;
            }
        }
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("[smp] debug scheduler dump begin"),
            "{rendered}"
        );
        assert!(
            rendered.contains("[smp] debug scheduler dump end"),
            "{rendered}"
        );
        assert!(!rendered.contains("[smp] activity begin"), "{rendered}");
        assert!(!rendered.contains("raw=pressure-gated"), "{rendered}");
    }

    #[cfg(all(feature = "kernel", sel4_config_debug_build))]
    #[test]
    fn smp_snapshot_emits_debug_dump_without_activity_telemetry() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 512, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 42,
        });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        pump.serial_mut().driver_mut().push_rx(b"smp\n");

        pump.poll();

        let transcript = pump.serial_mut().driver_mut().drain_tx();
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains("[smp] debug scheduler dump begin"),
            "{rendered}"
        );
        assert!(
            rendered.contains("[smp] debug scheduler dump end"),
            "{rendered}"
        );
        assert!(!rendered.contains("[smp] activity begin"), "{rendered}");
        assert!(rendered.contains("OK SMP mode=snapshot"), "{rendered}");
    }

    #[test]
    fn smp_activity_second_sample_reports_userspace_rates() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 512, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);

        pump.now_ms = 1_000;
        pump.metrics.console_lines = 1;
        pump.metrics.accepted_commands = 1;
        pump.metrics.timer_ticks = 1;
        pump.handle_command(Command::Smp {
            mode: SmpMode::Activity,
        })
        .unwrap();
        let _ = pump.serial_mut().driver_mut().drain_tx();

        pump.now_ms = 2_000;
        pump.metrics.console_lines = 5;
        pump.metrics.accepted_commands = 4;
        pump.metrics.timer_ticks = 3;
        pump.handle_command(Command::Smp {
            mode: SmpMode::Activity,
        })
        .unwrap();

        let transcript = pump.serial_mut().driver_mut().drain_tx();
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        let core_rate_line = if cfg!(feature = "kernel") {
            "[smp] activity core c=0 tasks=authority win=1000"
        } else {
            "[smp] activity core c=n/a tasks=host-test win=1000"
        };
        assert!(rendered.contains(core_rate_line), "{rendered}");
        assert!(rendered.contains("cmd_s=3"), "{rendered}");
        assert!(rendered.contains("line_s=4"), "{rendered}");
        assert!(rendered.contains("tick_s=2"), "{rendered}");
        assert!(rendered.contains("seat_drop_s="), "{rendered}");
        assert!(rendered.contains("seat_no_reply_s="), "{rendered}");
        assert!(rendered.contains("hdmi_drop_s="), "{rendered}");
        assert!(rendered.contains("net_drop_s="), "{rendered}");
        assert!(rendered.contains("cpu_pct=unavailable"), "{rendered}");
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn smp_activity_includes_net_telemetry_when_attached() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 512, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 7 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.backend = "virtio-net";
        net.status.mode = "dhcp";
        net.status.active_interface = "wired";
        net.status.standby_interface = "none";
        net.status.address_source = "dhcp-lease";
        net.status.dhcp_phase = "bound";
        net.status.ip.push_str("192.168.10.50").unwrap();
        net.status.gateway.push_str("192.168.10.1").unwrap();
        net.counters.rx_packets = 3;
        net.counters.tx_packets = 5;
        net.counters.tcp_accepts = 2;
        net.counters.tcp_auth_sessions = 1;
        net.counters.wifi_host_eapol_start = 1;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.serial_mut().driver_mut().push_rx(b"smp activity\n");

        pump.poll();

        let transcript = pump.serial_mut().driver_mut().drain_tx();
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains("[smp] activity net attached=yes backend=virtio-net"),
            "{rendered}"
        );
        assert!(rendered.contains("ip=192.168.10.50"), "{rendered}");
        assert!(
            rendered.contains("[smp] activity net-io rx=3 tx=5"),
            "{rendered}"
        );
        assert!(
            rendered.contains("[smp] activity net-tcp accepts=2 auth=1"),
            "{rendered}"
        );
        assert!(!rendered.contains("[smp] activity net-wifi"), "{rendered}");
        assert!(rendered.contains("OK SMP mode=activity"), "{rendered}");
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn smp_activity_includes_wifi_telemetry_when_wifi_active() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 512, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 7 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.backend = "cyw43";
        net.status.mode = "dhcp";
        net.status.active_interface = "wifi";
        net.status.standby_interface = "wired";
        net.status.address_source = "dhcp-lease";
        net.status.dhcp_phase = "bound";
        net.counters.wifi_assoc = 1;
        net.counters.wifi_link_up = 1;
        net.counters.wifi_host_eapol_rx = 2;
        net.counters.wifi_host_eapol_start = 1;
        net.counters.wifi_host_eapol_secure = 1;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.serial_mut().driver_mut().push_rx(b"smp activity\n");

        pump.poll();

        let transcript = pump.serial_mut().driver_mut().drain_tx();
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "[smp] activity net-wifi assoc=1 link=1 eapol_rx=2 eapol_start=1 eapol_secure=1"
            ),
            "{rendered}"
        );
    }

    #[test]
    fn successful_attach_allows_privileged_commands() {
        let driver = LoopbackSerial::<256>::new();
        let serial = SerialPort::<_, 256, 256, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ok").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        let driver = pump.serial_mut().driver_mut();
        let token = issue_token("ok", Role::Queen);
        let line = format!("attach queen {token}\nlog\n");
        driver.push_rx(line.as_bytes());
        pump.poll();
        drop(pump);
        assert!(audit
            .entries
            .iter()
            .any(|entry| entry.contains("log stream")));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn network_lines_feed_parser() {
        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, 32>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        let mut line = HeaplessString::new();
        assert!(line.push_str("ping").is_ok());
        assert!(net.lines.push(ConsoleLine::new(line, 1)).is_ok());
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.poll();
        drop(pump);
        assert!(net
            .sent
            .iter()
            .any(|line| line.as_str().starts_with("OK PING")));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn network_dispatch_post_response_flushes_get_separate_service_turns() {
        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, 32>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.exhaust_flush_budget = true;
        net.tcp_flush_activity_remaining = NET_POST_DISPATCH_FLUSH_POLLS + 4;
        let mut line = HeaplessString::new();
        assert!(line.push_str("ping").is_ok());
        assert!(net.lines.push(ConsoleLine::new(line, 1)).is_ok());
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);

        pump.poll();
        let metrics = pump.metrics;
        drop(pump);

        assert_eq!(net.tcp_flushes, NET_POST_DISPATCH_FLUSH_POLLS);
        assert_eq!(
            metrics.net_post_dispatch_flush_polls,
            NET_POST_DISPATCH_FLUSH_POLLS as u64
        );
        assert_eq!(metrics.net_post_dispatch_flush_exhaustions, 1);
        assert!(net
            .sent
            .iter()
            .any(|line| line.as_str().starts_with("OK PING")));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn network_dispatch_gets_bounded_post_response_flush() {
        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, 32>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.tcp_flush_activity_remaining = NET_POST_DISPATCH_FLUSH_POLLS + 4;
        let mut line = HeaplessString::new();
        assert!(line.push_str("ping").is_ok());
        assert!(net.lines.push(ConsoleLine::new(line, 1)).is_ok());
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);

        pump.poll();
        let metrics = pump.metrics;
        drop(pump);

        assert_eq!(net.polls, 1);
        assert_eq!(net.tcp_flushes, NET_POST_DISPATCH_FLUSH_POLLS);
        assert_eq!(
            metrics.net_post_dispatch_flush_polls,
            NET_POST_DISPATCH_FLUSH_POLLS as u64
        );
        assert_eq!(metrics.net_post_dispatch_flush_exhaustions, 1);
        assert!(net
            .sent
            .iter()
            .any(|line| line.as_str().starts_with("OK PING")));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn network_dispatch_flushes_between_batched_commands() {
        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, 32>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.tcp_flush_activity_remaining = NET_POST_DISPATCH_FLUSH_POLLS * 2 + 4;
        for conn_id in 1..=2 {
            let mut line = HeaplessString::new();
            assert!(line.push_str("ping").is_ok());
            assert!(net.lines.push(ConsoleLine::new(line, conn_id)).is_ok());
        }
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);

        pump.poll();
        let metrics = pump.metrics;
        drop(pump);

        assert_eq!(net.polls, 1);
        assert_eq!(net.tcp_flushes, NET_POST_DISPATCH_FLUSH_POLLS * 2);
        assert_eq!(
            metrics.net_post_dispatch_flush_polls,
            (NET_POST_DISPATCH_FLUSH_POLLS * 2) as u64
        );
        assert_eq!(metrics.net_post_dispatch_flush_exhaustions, 2);
        assert!(net
            .tcp_flush_send_counts
            .iter()
            .any(|count| *count > 0 && *count < net.sent.len()));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn linked_runtime_data_ready_drains_extra_console_dispatch_rounds() {
        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, 32>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.backend = "bcmgenet-v5";
        net.status.active_interface = "wired";
        net.status.address_source = "dhcp-lease";
        net.status.dhcp_phase = "bound";
        let line_count = CONSOLE_DISPATCH_BURST + 2;
        for conn_id in 1..=line_count {
            let mut line = HeaplessString::new();
            assert!(line.push_str("ping").is_ok());
            assert!(net
                .lines
                .push(ConsoleLine::new(line, conn_id as u64))
                .is_ok());
        }
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);

        pump.poll();
        let metrics = pump.metrics;
        drop(pump);

        assert_eq!(metrics.accepted_commands, line_count as u64);
        assert_eq!(net.lines.len(), 0);
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn cyw43_data_ready_uses_wifi_only_deeper_dispatch_rounds() {
        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, 32>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.backend = "cyw43";
        net.status.active_interface = "wifi";
        net.status.address_source = "dhcp-lease";
        net.status.dhcp_phase = "bound";
        let line_count = CONSOLE_DISPATCH_BURST * NET_CYW43_HOT_DISPATCH_ROUNDS + 2;
        for conn_id in 1..=line_count {
            let mut line = HeaplessString::new();
            assert!(line.push_str("ping").is_ok());
            assert!(net
                .lines
                .push(ConsoleLine::new(line, conn_id as u64))
                .is_ok());
        }
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);

        pump.poll();
        let metrics = pump.metrics;
        drop(pump);

        assert_eq!(
            metrics.accepted_commands,
            (CONSOLE_DISPATCH_BURST * NET_CYW43_HOT_DISPATCH_ROUNDS) as u64
        );
        assert_eq!(net.lines.len(), 2);
        assert_eq!(NET_LINKED_RUNTIME_HOT_DISPATCH_ROUNDS, 3);
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn non_data_ready_net_dispatch_stays_single_burst() {
        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, 32>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.backend = "bcmgenet-v5";
        net.status.active_interface = "wired";
        net.status.address_source = "dhcp-pending";
        net.status.dhcp_phase = "requesting";
        let line_count = CONSOLE_DISPATCH_BURST + 2;
        for conn_id in 1..=line_count {
            let mut line = HeaplessString::new();
            assert!(line.push_str("ping").is_ok());
            assert!(net
                .lines
                .push(ConsoleLine::new(line, conn_id as u64))
                .is_ok());
        }
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);

        pump.poll();
        let metrics = pump.metrics;
        drop(pump);

        assert_eq!(metrics.accepted_commands, CONSOLE_DISPATCH_BURST as u64);
        assert_eq!(net.lines.len(), line_count - CONSOLE_DISPATCH_BURST);
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn pre_root_network_poll_does_not_accept_console_input() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        let mut line = HeaplessString::new();
        assert!(line.push_str("ping").is_ok());
        assert!(net.lines.push(ConsoleLine::new(line, 1)).is_ok());
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.serial_mut().driver_mut().push_rx(b"ping\n");

        pump.poll_pre_root_network();

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        assert!(transcript.is_empty());
        assert!(net.sent.is_empty());
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn host_eapol_pending_uses_small_runtime_burst_without_input_pressure() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.address_source = "wifi-host-eapol-pending";
        net.status.dhcp_phase = "host-eapol-pending";

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.poll();
        drop(pump);

        assert_eq!(WIFI_HOST_EAPOL_RUNTIME_BURST_POLLS, 8);
        assert_eq!(net.polls, WIFI_HOST_EAPOL_RUNTIME_BURST_POLLS + 1);
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn host_eapol_burst_polls_get_separate_service_turns() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.address_source = "wifi-host-eapol-pending";
        net.status.dhcp_phase = "host-eapol-pending";
        net.exhaust_poll_budget = true;

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.poll();
        drop(pump);

        assert_eq!(net.polls, WIFI_HOST_EAPOL_RUNTIME_BURST_POLLS + 1);
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn wired_dhcp_lease_needs_physical_pressure_service() {
        let mut wired = NetStatusReport::default();
        wired.active_interface = "wired";
        wired.address_source = "dhcp-lease";
        wired.dhcp_phase = "bound";
        assert!(net_status_needs_physical_pressure_service(&wired));

        let mut wifi = wired.clone();
        wifi.active_interface = "wifi";
        assert!(!net_status_needs_physical_pressure_service(&wifi));

        let mut pre_lease = wired.clone();
        pre_lease.address_source = "dhcp-pending";
        pre_lease.dhcp_phase = "requesting";
        assert!(!net_status_needs_physical_pressure_service(&pre_lease));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn linked_runtime_data_ready_enables_hot_dispatch_rounds() {
        let mut genet = NetStatusReport::default();
        genet.backend = "bcmgenet-v5";
        genet.active_interface = "wired";
        genet.address_source = "dhcp-lease";
        genet.dhcp_phase = "bound";
        assert_eq!(
            net_hot_dispatch_rounds_for_status(&genet),
            NET_LINKED_RUNTIME_HOT_DISPATCH_ROUNDS
        );

        let mut wifi = genet.clone();
        wifi.backend = "cyw43";
        wifi.active_interface = "wifi";
        assert_eq!(
            net_hot_dispatch_rounds_for_status(&wifi),
            NET_CYW43_HOT_DISPATCH_ROUNDS
        );

        let mut pre_dhcp = genet.clone();
        pre_dhcp.address_source = "dhcp-pending";
        pre_dhcp.dhcp_phase = "requesting";
        assert_eq!(net_hot_dispatch_rounds_for_status(&pre_dhcp), 1);

        let mut virtio = genet.clone();
        virtio.backend = "virtio-net";
        assert_eq!(net_hot_dispatch_rounds_for_status(&virtio), 1);
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn post_dispatch_flush_limit_extends_under_display_pressure() {
        let mut genet = NetStatusReport::default();
        genet.backend = "bcmgenet-v5";
        genet.active_interface = "wired";
        genet.address_source = "dhcp-lease";
        genet.dhcp_phase = "bound";
        let mut wifi = genet.clone();
        wifi.backend = "cyw43";
        wifi.active_interface = "wifi";

        assert_eq!(
            net_post_dispatch_flush_limit_for_display(None),
            NET_POST_DISPATCH_FLUSH_POLLS
        );
        assert_eq!(
            net_post_dispatch_flush_limit_for_display(Some(LocalSeatDisplayTrace::default())),
            NET_POST_DISPATCH_FLUSH_POLLS
        );
        assert_eq!(
            net_post_dispatch_flush_limit_for_status(&genet, None),
            NET_POST_DISPATCH_FLUSH_POLLS
        );
        assert_eq!(
            net_post_dispatch_flush_limit_for_status(&wifi, None),
            NET_CYW43_POST_DISPATCH_FLUSH_POLLS
        );

        let mut pending = LocalSeatDisplayTrace {
            pending_bytes: 1,
            ..LocalSeatDisplayTrace::default()
        };
        assert_eq!(
            net_post_dispatch_flush_limit_for_display(Some(pending)),
            NET_POST_DISPATCH_BACKLOG_FLUSH_POLLS
        );
        assert_eq!(
            net_post_dispatch_flush_limit_for_status(&genet, Some(pending)),
            NET_POST_DISPATCH_BACKLOG_FLUSH_POLLS
        );
        assert_eq!(
            net_post_dispatch_flush_limit_for_status(&wifi, Some(pending)),
            NET_CYW43_POST_DISPATCH_BACKLOG_FLUSH_POLLS
        );

        pending = LocalSeatDisplayTrace {
            no_reply_frames: 1,
            deferred_frames: 2,
            submitted_frames: 1,
            ..LocalSeatDisplayTrace::default()
        };
        assert_eq!(
            net_post_dispatch_flush_limit_for_display(Some(pending)),
            NET_POST_DISPATCH_BACKLOG_FLUSH_POLLS
        );
        assert_eq!(
            net_post_dispatch_flush_limit_for_status(&genet, Some(pending)),
            NET_POST_DISPATCH_BACKLOG_FLUSH_POLLS
        );
        assert_eq!(
            net_post_dispatch_flush_limit_for_status(&wifi, Some(pending)),
            NET_CYW43_POST_DISPATCH_BACKLOG_FLUSH_POLLS
        );

        pending = LocalSeatDisplayTrace {
            no_reply_frames: 1,
            deferred_frames: 1,
            submitted_frames: 2,
            ..LocalSeatDisplayTrace::default()
        };
        assert_eq!(
            net_post_dispatch_flush_limit_for_display(Some(pending)),
            NET_POST_DISPATCH_FLUSH_POLLS
        );
        assert_eq!(
            net_post_dispatch_flush_limit_for_status(&genet, Some(pending)),
            NET_POST_DISPATCH_FLUSH_POLLS
        );
        assert_eq!(
            net_post_dispatch_flush_limit_for_status(&wifi, Some(pending)),
            NET_CYW43_POST_DISPATCH_FLUSH_POLLS
        );
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn host_eapol_pending_does_not_delay_serial_echo() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.address_source = "wifi-host-eapol-pending";
        net.status.dhcp_phase = "host-eapol-pending";

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.serial_mut().driver_mut().push_rx(b"n");
        pump.poll();
        let echoed = pump.serial_mut().driver_mut().drain_tx();
        drop(pump);

        assert_eq!(net.polls, 0);
        assert_eq!(echoed.as_slice(), b"n");
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn host_eapol_required_does_not_delay_serial_echo() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.address_source = "wifi-host-eapol-required";
        net.status.dhcp_phase = "host-eapol-required";

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.serial_mut().driver_mut().push_rx(b"n");
        pump.poll();
        let echoed = pump.serial_mut().driver_mut().drain_tx();
        drop(pump);

        assert_eq!(net.polls, 0);
        assert_eq!(echoed.as_slice(), b"n");
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn host_eapol_pending_does_not_delay_local_seat_echo() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.address_source = "wifi-host-eapol-pending";
        net.status.dhcp_phase = "host-eapol-pending";
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        local_seat.enqueue_keyboard_bytes(b"hel");

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network(&mut net)
            .with_local_seat(&mut local_seat);
        pump.poll();

        assert_eq!(
            pump.local_seat
                .as_ref()
                .expect("local-seat should be attached")
                .input_echo_preview(),
            "hel"
        );
        drop(pump);
        assert_eq!(net.polls, 0);
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn local_seat_input_skips_ready_network_poll_for_keyboard_turn() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.address_source = "dhcp-lease";
        net.status.dhcp_phase = "bound";
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        local_seat.enqueue_keyboard_bytes(b"x");

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network(&mut net)
            .with_local_seat(&mut local_seat);
        pump.poll();
        let metrics = pump.metrics();
        drop(pump);

        assert_eq!(net.polls, 0);
        assert_eq!(metrics.local_seat_keyboard_priority_turns, 1);
        assert_eq!(metrics.local_seat_runtime_skipped_turns, 1);
        assert_eq!(metrics.local_seat_serial_dispatch_yielded_turns, 1);
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn serial_input_skips_ready_network_data_poll_for_driver_task_turn() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.address_source = "dhcp-lease";
        net.status.dhcp_phase = "bound";

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.serial_mut().driver_mut().push_rx(b"n");
        pump.poll();
        let echoed = pump.serial_mut().driver_mut().drain_tx();
        drop(pump);

        assert_eq!(net.polls, 0);
        assert_eq!(echoed.as_slice(), b"n");
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn serial_input_defers_buffered_network_console_lines_for_driver_task_turn() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.address_source = "dhcp-lease";
        net.status.dhcp_phase = "bound";
        let mut line = HeaplessString::new();
        assert!(line.push_str("ping").is_ok());
        assert!(net.lines.push(ConsoleLine::new(line, 1)).is_ok());

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.serial_mut().driver_mut().push_rx(b"n");
        pump.poll();
        let echoed = pump.serial_mut().driver_mut().drain_tx();
        drop(pump);

        assert_eq!(net.polls, 0);
        assert_eq!(net.lines.len(), 1);
        assert!(net.sent.is_empty());
        assert_eq!(echoed.as_slice(), b"n");
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn serial_tx_backlog_skips_ready_network_data_poll_for_driver_task_turn() {
        let driver = LoopbackSerial::<8>::new();
        let serial = SerialPort::<_, 32, 128, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.address_source = "dhcp-lease";
        net.status.dhcp_phase = "bound";

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.serial_mut().enqueue_tx(
            b"serial backlog must drain before ready network data consumes the pump turn",
        );
        pump.poll();
        let emitted = pump.serial_mut().driver_mut().drain_tx();
        drop(pump);

        assert_eq!(net.polls, 0);
        assert!(!emitted.is_empty());
        assert!(emitted.len() < 64);
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn wired_dhcp_lease_services_network_under_serial_backlog() {
        let driver = LoopbackSerial::<8>::new();
        let serial = SerialPort::<_, 32, 128, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.active_interface = "wired";
        net.status.address_source = "dhcp-lease";
        net.status.dhcp_phase = "bound";

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.serial_mut().enqueue_tx(
            b"serial backlog must coexist with Genet ARP and TCP setup after DHCP binds",
        );
        pump.poll();
        let emitted = pump.serial_mut().driver_mut().drain_tx();
        drop(pump);

        assert_eq!(net.polls, 1);
        assert!(!emitted.is_empty());
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn host_eapol_required_does_not_delay_local_seat_echo() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.address_source = "wifi-host-eapol-required";
        net.status.dhcp_phase = "host-eapol-required";
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        local_seat.enqueue_keyboard_bytes(b"hel");

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network(&mut net)
            .with_local_seat(&mut local_seat);
        pump.poll();

        assert_eq!(
            pump.local_seat
                .as_ref()
                .expect("local-seat should be attached")
                .input_echo_preview(),
            "hel"
        );
        drop(pump);
        assert_eq!(net.polls, 0);
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn keyboard_input_pauses_ready_network_poll_for_one_turn() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        local_seat.enqueue_keyboard_bytes(b"hel");

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network(&mut net)
            .with_local_seat(&mut local_seat);
        pump.poll();
        drop(pump);

        assert_eq!(net.polls, 0);
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn pre_root_host_eapol_pending_uses_larger_burst() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.address_source = "wifi-host-eapol-pending";
        net.status.dhcp_phase = "host-eapol-pending";

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.poll_pre_root_network();
        drop(pump);

        assert_eq!(net.polls, WIFI_HOST_EAPOL_PRE_ROOT_BURST_POLLS + 1);
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn host_eapol_pending_ignores_idle_usb_first_report_wait() {
        assert!(!net_physical_input_pressure_for_status(false, true, true));
        assert!(net_physical_input_pressure_for_status(true, true, true));
        assert!(net_physical_input_pressure_for_status(false, true, false));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn wifi_warning_names_password_mic_failure() {
        let mut status = NetStatusReport {
            backend: "cyw43",
            mode: "dhcp",
            interface_policy: "wifi",
            active_interface: "wifi",
            standby_interface: "wired",
            address_source: "wifi-host-eapol-required",
            ip: HeaplessString::new(),
            gateway: HeaplessString::new(),
            dhcp_phase: "host-eapol-required",
        };
        let stats = NetCounters::default();

        assert_eq!(
            wifi_credential_warning_for_status(
                &status,
                &stats,
                Some("host-eapol-post-secure-ptk-install")
            ),
            None
        );

        let warning =
            wifi_credential_warning_for_status(&status, &stats, Some("host-eapol-m3-mic"))
                .expect("M3 MIC failure should warn");
        assert_eq!(warning.code, "password-or-security-mismatch");
        assert_eq!(warning.detail, "wpa2-key-mic-failed");

        status.address_source = "dhcp-lease";
        status.dhcp_phase = "bound";
        assert_eq!(
            wifi_credential_warning_for_status(&status, &stats, Some("host-eapol-m3-mic")),
            None
        );
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn wifi_warning_names_invalid_config_and_association_failure() {
        let mut status = NetStatusReport {
            backend: "cyw43",
            mode: "dhcp",
            interface_policy: "wifi",
            active_interface: "wifi",
            standby_interface: "wired",
            address_source: "wifi-psk-too-short",
            ip: HeaplessString::new(),
            gateway: HeaplessString::new(),
            dhcp_phase: "disabled",
        };
        let stats = NetCounters::default();

        let invalid = wifi_credential_warning_for_status(&status, &stats, None)
            .expect("invalid PSK should warn");
        assert_eq!(invalid.code, "invalid-config");
        assert_eq!(invalid.detail, "psk-too-short");

        status.address_source = "wifi-association-failed";
        status.dhcp_phase = "wifi-association-failed";
        let assoc = wifi_credential_warning_for_status(&status, &stats, None)
            .expect("association failure should warn");
        assert_eq!(assoc.code, "ssid-or-ap-unavailable");
        assert_eq!(assoc.detail, "association-not-complete");
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn nettest_reports_dhcp_pending_detail() {
        let driver = LoopbackSerial::<128>::new();
        let serial = SerialPort::<_, 128, 128, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.start_result = NetSelfTestStartResult::DhcpPending;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.session = Some(SessionRole::Queen);
        pump.serial_mut().driver_mut().push_rx(b"nettest\n");

        pump.poll();
        pump.poll();

        let transcript = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains("ERR NETTEST reason=policy detail=dhcp-pending"),
            "{rendered}"
        );
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn nettest_reports_wifi_host_eapol_required_detail() {
        let driver = LoopbackSerial::<128>::new();
        let serial = SerialPort::<_, 128, 128, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.start_result = NetSelfTestStartResult::WifiHostEapolRequired;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.session = Some(SessionRole::Queen);
        pump.serial_mut().driver_mut().push_rx(b"nettest\n");

        pump.poll();
        pump.poll();

        let transcript = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains("ERR NETTEST reason=policy detail=wifi-host-eapol-required"),
            "{rendered}"
        );
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn nettest_reports_wifi_host_eapol_pending_detail() {
        let driver = LoopbackSerial::<128>::new();
        let serial = SerialPort::<_, 128, 128, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.start_result = NetSelfTestStartResult::WifiHostEapolPending;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.session = Some(SessionRole::Queen);
        pump.serial_mut().driver_mut().push_rx(b"nettest\n");

        pump.poll();
        pump.poll();

        let transcript = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains("ERR NETTEST reason=policy detail=wifi-host-eapol-pending"),
            "{rendered}"
        );
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn nettest_reports_not_ready_reason() {
        let driver = LoopbackSerial::<128>::new();
        let serial = SerialPort::<_, 128, 128, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.start_result = NetSelfTestStartResult::NotReadyIpcBuffer;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.session = Some(SessionRole::Queen);
        pump.serial_mut().driver_mut().push_rx(b"nettest\n");

        pump.poll();
        pump.poll();

        let transcript = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains("ERR NETTEST reason=policy detail=not-ready:ipc-buffer"),
            "{rendered}"
        );
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn netstats_emits_compact_status_line() {
        let driver = LoopbackSerial::<1024>::new();
        let serial = SerialPort::<_, 512, 512, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.mode = "dhcp";
        net.status.interface_policy = "wired";
        net.status.active_interface = "wired";
        net.status.standby_interface = "none";
        net.status.address_source = "dhcp-lease";
        net.status.dhcp_phase = "bound";
        net.status.ip.push_str("192.168.10.50").unwrap();
        net.status.gateway.push_str("192.168.10.1").unwrap();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.session = Some(SessionRole::Queen);
        pump.serial_mut().driver_mut().push_rx(b"netstats\n");

        pump.poll();

        let transcript = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "netstatus: ip=192.168.10.50 gateway=192.168.10.1 src=dhcp-lease dhcp=bound"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "netstats: mode=dhcp policy=wired active=wired standby=none addr_src=dhcp-lease ip=192.168.10.50 gateway=192.168.10.1 dhcp=bound"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "netstats: tx_submit=0 tx_complete=0 tx_free=0 tx_in_flight=0 tx_double_submit=0 tx_zero_len_attempt=0 arp_rx=0 arp_tx=0"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("netstats: tcp_post_flush_polls=0 tcp_post_flush_exhaustions=0"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "netstats: genet_rx_hw=0 genet_rx_last_len=0 genet_rx_last_ethertype=0x0000"
            ),
            "{rendered}"
        );
        assert!(!rendered.contains("netstats: wifi_assoc="), "{rendered}");
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn netstats_emits_wifi_dhcp_bound_secure_counters() {
        let driver = LoopbackSerial::<1024>::new();
        let serial = SerialPort::<_, 512, 512, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.mode = "dhcp";
        net.status.interface_policy = "wifi";
        net.status.active_interface = "wifi";
        net.status.standby_interface = "wired";
        net.status.address_source = "dhcp-lease";
        net.status.dhcp_phase = "bound";
        net.status.ip.push_str("192.168.50.23").unwrap();
        net.status.gateway.push_str("192.168.50.1").unwrap();
        net.counters.rx_packets = 4;
        net.counters.tx_packets = 5;
        net.counters.udp_rx = 1;
        net.counters.udp_tx = 2;
        net.counters.tcp_accepts = 1;
        net.counters.tcp_auth_sessions = 1;
        net.counters.tcp_rx_bytes = 128;
        net.counters.tcp_console_recv_ready = 7;
        net.counters.tcp_console_recv_budget_hits = 2;
        net.counters.wifi_assoc = 1;
        net.counters.wifi_link_up = 1;
        net.counters.wifi_host_eapol_rx = 2;
        net.counters.wifi_host_eapol_start = 1;
        net.counters.wifi_host_eapol_secure = 1;
        net.counters.wifi_rx_runtime_queue_count = 3;
        net.counters.wifi_rx_runtime_queue_high_water = 5;
        net.counters.wifi_rx_runtime_queue_overflow_seen = 0;
        net.counters.wifi_rx_runtime_max_drained_per_turn = 4;
        net.counters.wifi_rx_runtime_drain_budget_hit = 1;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.session = Some(SessionRole::Queen);
        pump.serial_mut().driver_mut().push_rx(b"netstats\n");

        pump.poll();

        let transcript = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "netstatus: ip=192.168.50.23 gateway=192.168.50.1 src=dhcp-lease dhcp=bound"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "netstats: mode=dhcp policy=wifi active=wifi standby=wired addr_src=dhcp-lease ip=192.168.50.23 gateway=192.168.50.1 dhcp=bound"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "netstats: wifi_assoc=1 wifi_link=1 eapol_rx=2 eapol_start=1 eapol_secure=1 wifi_rxq_cur=0 wifi_rxq_hwm=0 wifi_rxq_drops=0 wifi_runtime_rxq_cur=3 wifi_runtime_rxq_hwm=5 wifi_runtime_rxq_ovf=0 wifi_runtime_rxq_max_drain=4 wifi_runtime_rxq_drain_hit=1"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "netstats: udp_rx=1 udp_tx=2 tcp_accepts=1 tcp_auth=1 tcp_rx_bytes=128 tcp_recv_ready=7 tcp_recv_budget_hits=2"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("netstats: tcp_post_flush_polls=0 tcp_post_flush_exhaustions=0"),
            "{rendered}"
        );
        assert!(!rendered.contains("netstats: genet_rx_hw="), "{rendered}");
        assert!(!rendered.contains("wifi warning:"), "{rendered}");
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn netstats_emits_wifi_warning_for_association_failure() {
        let driver = LoopbackSerial::<2048>::new();
        let serial = SerialPort::<_, 1024, 1024, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.mode = "dhcp";
        net.status.interface_policy = "wifi";
        net.status.active_interface = "wifi";
        net.status.standby_interface = "wired";
        net.status.address_source = "wifi-association-failed";
        net.status.dhcp_phase = "wifi-association-failed";
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.session = Some(SessionRole::Queen);
        pump.serial_mut().driver_mut().push_rx(b"netstats\n");

        pump.poll();

        let transcript = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "wifi warning: code=ssid-or-ap-unavailable detail=association-not-complete action=check-ssid-ap-range-and-security"
            ),
            "{rendered}"
        );
    }

    #[test]
    fn console_acknowledgements_emit_expected_lines() {
        let driver = LoopbackSerial::<512>::new();
        let serial = SerialPort::<_, 256, 256, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        {
            let driver = pump.serial_mut().driver_mut();
            let token = issue_token("ticket", Role::Queen);
            let line = format!("log\nattach queen {token}\nlog\n");
            driver.push_rx(line.as_bytes());
        }
        pump.poll();
        pump.poll();
        pump.poll();
        let tx = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let transcript: Vec<u8> = tx.into_iter().collect();
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("ERR LOG reason=policy detail=unauthenticated"),
            "{rendered}"
        );
        assert!(rendered.contains("OK ATTACH role=queen"), "{rendered}");
        assert!(rendered.contains("OK LOG"), "{rendered}");
    }

    #[test]
    fn reboot_requires_authenticated_secret_backed_session() {
        let _guard = crate::reboot::test_lock();
        crate::reboot::reset_test_backend();
        crate::reboot::set_test_backend_available(true);
        let driver = LoopbackSerial::<2048>::new();
        let serial = SerialPort::<_, 2048, 2048, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        pump.serial_mut()
            .driver_mut()
            .push_rx(b"reboot\nattach queen\nreboot\n");

        pump.poll();

        let tx = pump.serial_mut().driver_mut().drain_tx();
        let transcript: Vec<u8> = tx.into_iter().collect();
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("ERR REBOOT reason=policy detail=unauthenticated"),
            "{rendered}"
        );
        assert!(rendered.contains("OK ATTACH role=queen"), "{rendered}");
        assert!(
            rendered.contains("ERR REBOOT reason=policy detail=secret-required"),
            "{rendered}"
        );
        assert_eq!(crate::reboot::test_reboot_requests(), 0);
        crate::reboot::reset_test_backend();
    }

    #[test]
    fn local_seat_ticket_backed_reboot_schedules_backend_request() {
        let _guard = crate::reboot::test_lock();
        crate::reboot::reset_test_backend();
        crate::reboot::set_test_backend_available(true);
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(2, 1);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 192,
            buffer_lines: 4,
        });
        local_seat.mark_root_console_ready();
        local_seat.enable_backend_keyboard_polling();
        let token = issue_token("ticket", Role::Queen);
        let line = format!("attach queen {token}\nreboot\n");
        assert_eq!(
            local_seat.enqueue_keyboard_bytes(line.as_bytes()),
            line.len()
        );
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.poll();
        assert_eq!(crate::reboot::test_reboot_requests(), 0);
        pump.poll();
        assert_eq!(crate::reboot::test_reboot_requests(), 0);
        pump.poll();

        let tx = pump.serial_mut().driver_mut().drain_tx();
        let transcript: Vec<u8> = tx.into_iter().collect();
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(rendered.contains("OK ATTACH role=queen"), "{rendered}");
        assert!(
            rendered.contains("OK REBOOT detail=scheduled"),
            "{rendered}"
        );
        assert_eq!(crate::reboot::test_reboot_requests(), 1);
        crate::reboot::reset_test_backend();
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn tcp_secret_backed_reboot_flushes_then_requests_backend() {
        let _guard = crate::reboot::test_lock();
        crate::reboot::reset_test_backend();
        crate::reboot::set_test_backend_available(true);
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(4, 1);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        let mut attach = HeaplessString::new();
        attach.push_str("attach queen").unwrap();
        let mut reboot = HeaplessString::new();
        reboot.push_str("reboot").unwrap();
        net.lines.push(ConsoleLine::new(attach, 1)).unwrap();
        net.lines.push(ConsoleLine::new(reboot, 2)).unwrap();

        {
            let mut pump =
                EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);

            pump.poll();
            assert_eq!(crate::reboot::test_reboot_requests(), 0);
            pump.poll();
            assert_eq!(crate::reboot::test_reboot_requests(), 1);
        }

        let sent = net
            .sent
            .iter()
            .map(|line| line.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(sent.contains("OK ATTACH role=queen"), "{sent}");
        assert!(sent.contains("OK REBOOT detail=scheduled"), "{sent}");
        assert_eq!(net.disconnect_requests, 1);
        assert!(audit
            .entries
            .iter()
            .any(|entry| entry.as_str() == "console: reboot firing source=net"));
        crate::reboot::reset_test_backend();
    }

    #[test]
    fn parser_recovers_after_invalid_command() {
        let driver = LoopbackSerial::<256>::new();
        let serial = SerialPort::<_, 256, 256, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        {
            let driver = pump.serial_mut().driver_mut();
            driver.push_rx(b"bogus\nhelp\n");
        }
        pump.poll();
        pump.poll();
        let tx = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let transcript: Vec<u8> = tx.into_iter().collect();
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(rendered.contains("ERR PARSE"), "{rendered}");
        assert!(rendered.contains("Commands:"), "{rendered}");
    }

    #[test]
    fn session_end_clears_partial_input() {
        let driver = LoopbackSerial::<256>::new();
        let serial = SerialPort::<_, 256, 256, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        pump.parser
            .push_byte(b'x')
            .expect("partial byte should be accepted");
        pump.end_session("test");
        {
            let driver = pump.serial_mut().driver_mut();
            driver.push_rx(b"help\n");
        }
        pump.poll();
        let tx = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let transcript: Vec<u8> = tx.into_iter().collect();
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(rendered.contains("Commands:"), "{rendered}");
    }

    #[test]
    fn local_seat_keyboard_input_uses_distinct_physical_source() {
        let driver = LoopbackSerial::<256>::new();
        let serial = SerialPort::<_, 256, 256, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat =
            crate::local_seat::LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
                keyboard_device: "usb-kbd0",
                display_device: "hdmi0",
                line_bytes: 64,
                buffer_lines: 8,
            });
        local_seat.enqueue_keyboard_bytes(b"ping\n");
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.poll();
        assert_eq!(pump.last_input_source, ConsoleInputSource::LocalSeat);

        let tx = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let rendered =
            String::from_utf8(tx.into_iter().collect()).expect("serial output must be utf8");
        assert!(rendered.contains("PONG"), "{rendered}");
        assert!(rendered.contains("OK PING reply=pong"), "{rendered}");
        assert!(rendered.contains("cohesix> "), "{rendered}");
    }

    #[test]
    fn local_seat_ansi_arrow_sequence_does_not_enter_parser() {
        let driver = LoopbackSerial::<1024>::new();
        let serial = SerialPort::<_, 1024, 1024, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(2, 1);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat =
            crate::local_seat::LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
                keyboard_device: "usb-kbd0",
                display_device: "hdmi0",
                line_bytes: 64,
                buffer_lines: 8,
            });
        local_seat.enqueue_keyboard_bytes(b"\x1b");
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.poll();
        pump.local_seat
            .as_mut()
            .expect("local seat remains attached")
            .enqueue_keyboard_bytes(b"[A");
        pump.poll();

        assert!(pump.local_line.is_empty());
        let tx = pump.serial_mut().driver_mut().drain_tx();
        let rendered =
            String::from_utf8(tx.into_iter().collect()).expect("serial output must be utf8");
        assert!(!rendered.contains("ERR PARSE"), "{rendered}");
    }

    #[test]
    fn serial_input_preempts_concurrent_local_seat_line() {
        let driver = LoopbackSerial::<1024>::new();
        let serial = SerialPort::<_, 1024, 1024, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(2, 1);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat =
            crate::local_seat::LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
                keyboard_device: "usb-kbd0",
                display_device: "hdmi0",
                line_bytes: 64,
                buffer_lines: 8,
            });
        local_seat.enqueue_keyboard_bytes(b"pi");
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);
        pump.serial_mut().driver_mut().push_rx(b"help\n");

        pump.poll();

        assert_eq!(pump.last_input_source, ConsoleInputSource::Serial);
        assert!(pump.local_line.is_empty());
        let metrics = pump.metrics();
        assert_eq!(metrics.local_seat_keyboard_priority_turns, 0);
        assert_eq!(metrics.local_seat_runtime_skipped_turns, 0);
        assert_eq!(metrics.local_seat_serial_dispatch_yielded_turns, 0);
        let first_turn_tx = pump.serial_mut().driver_mut().drain_tx();
        let first_turn_rendered = String::from_utf8(first_turn_tx.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            first_turn_rendered.contains("Commands:"),
            "{first_turn_rendered}"
        );

        pump.poll();

        assert_eq!(pump.last_input_source, ConsoleInputSource::LocalSeat);
        assert_eq!(pump.local_line.as_str(), "pi");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_and_usb_keyboards_share_parser_after_usb_polling_enabled() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(2, 1);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat =
            crate::local_seat::LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
                keyboard_device: "usb-kbd0",
                display_device: "hdmi0",
                line_bytes: 64,
                buffer_lines: 8,
            });
        local_seat.mark_root_console_ready();
        local_seat.enable_backend_keyboard_polling();
        local_seat.enqueue_keyboard_bytes(b"ping\n");
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);
        pump.serial_mut().driver_mut().push_rx(b"help\n");

        pump.poll();
        assert_eq!(pump.last_input_source, ConsoleInputSource::Serial);
        let serial_turn = String::from_utf8(
            pump.serial_mut()
                .driver_mut()
                .drain_tx()
                .into_iter()
                .collect(),
        )
        .expect("serial output must be utf8");
        assert!(serial_turn.contains("Commands:"), "{serial_turn}");

        pump.poll();
        assert_eq!(pump.last_input_source, ConsoleInputSource::LocalSeat);
        let usb_turn = String::from_utf8(
            pump.serial_mut()
                .driver_mut()
                .drain_tx()
                .into_iter()
                .collect(),
        )
        .expect("serial output must be utf8");
        assert!(usb_turn.contains("PONG"), "{usb_turn}");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn local_seat_usb_status_reports_current_priority_turn() {
        let driver = LoopbackSerial::<32768>::new();
        let serial = SerialPort::<_, 32768, 32768, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(2, 1);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat =
            crate::local_seat::LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
                keyboard_device: "usb-kbd0",
                display_device: "hdmi0",
                line_bytes: 64,
                buffer_lines: 8,
            });
        local_seat.enqueue_keyboard_bytes(b"usb status\n");
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_local_seat(&mut local_seat)
            .with_test_pi4_debug_commands();

        pump.poll();

        let tx = pump.serial_mut().driver_mut().drain_tx();
        let rendered =
            String::from_utf8(tx.into_iter().collect()).expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "usb: event_loop keyboard_priority=1 runtime_skipped=1 serial_dispatch_yielded=1"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK USB detail=subcommand=status"),
            "{rendered}"
        );
    }

    #[test]
    fn serial_console_flood_yields_between_lines_when_local_seat_is_active() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(4, 1);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat =
            crate::local_seat::LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
                keyboard_device: "usb-kbd0",
                display_device: "hdmi0",
                line_bytes: 64,
                buffer_lines: 8,
            });
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);
        pump.serial_mut().driver_mut().push_rx(b"ping\nhelp\n");

        pump.poll();
        let first_turn = String::from_utf8(
            pump.serial_mut()
                .driver_mut()
                .drain_tx()
                .into_iter()
                .collect(),
        )
        .expect("serial output must be utf8");
        assert!(first_turn.contains("PONG"), "{first_turn}");
        assert!(!first_turn.contains("Commands:"), "{first_turn}");

        pump.local_seat
            .as_mut()
            .expect("local-seat should be attached")
            .enqueue_keyboard_bytes(b"x");
        pump.poll();
        assert_eq!(pump.last_input_source, ConsoleInputSource::LocalSeat);
        assert_eq!(pump.local_line.as_str(), "x");
        let second_turn = String::from_utf8(
            pump.serial_mut()
                .driver_mut()
                .drain_tx()
                .into_iter()
                .collect(),
        )
        .expect("serial output must be utf8");
        assert!(!second_turn.contains("Commands:"), "{second_turn}");

        pump.poll();
        assert_eq!(pump.last_input_source, ConsoleInputSource::Serial);
        assert!(pump.local_line.is_empty());
        let third_turn = String::from_utf8(
            pump.serial_mut()
                .driver_mut()
                .drain_tx()
                .into_iter()
                .collect(),
        )
        .expect("serial output must be utf8");
        assert!(third_turn.contains("Commands:"), "{third_turn}");
    }

    #[test]
    fn serial_partial_input_defers_background_console_output_until_idle() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(4, 1);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);

        pump.serial_mut().driver_mut().push_rx(b"h");
        pump.poll();
        let echoed = pump.serial_mut().driver_mut().drain_tx();
        assert_eq!(echoed.as_slice(), b"h");

        pump.emit_console_line("[log] background burst");
        assert_eq!(pump.pending_console_output.len(), 1);
        assert_eq!(pump.metrics().physical_console_output_deferred, 1);
        assert!(pump.serial_mut().driver_mut().drain_tx().is_empty());

        pump.serial_mut().driver_mut().push_rx(b"elp\n");
        pump.poll();
        let command_turn = String::from_utf8(
            pump.serial_mut()
                .driver_mut()
                .drain_tx()
                .into_iter()
                .collect(),
        )
        .expect("serial output must be utf8");
        assert!(command_turn.contains("Commands:"), "{command_turn}");
        assert!(!command_turn.contains("[log] background burst"));

        pump.poll();
        let idle_turn = String::from_utf8(
            pump.serial_mut()
                .driver_mut()
                .drain_tx()
                .into_iter()
                .collect(),
        )
        .expect("serial output must be utf8");
        assert!(idle_turn.contains("[log] background burst"), "{idle_turn}");
        assert_eq!(pump.metrics().physical_console_output_flushed, 1);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_input_idle_trace_waits_for_quiet_console_output() {
        let driver = LoopbackSerial::<128>::new();
        let serial = SerialPort::<_, 128, 128, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: SERIAL_INPUT_IDLE_TRACE_INTERVAL_MS,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        pump.banner_emitted = true;
        pump.now_ms = SERIAL_INPUT_IDLE_TRACE_INTERVAL_MS;
        pump.serial.enqueue_tx(b"busy");

        pump.maybe_emit_serial_input_idle_trace(false);

        assert_eq!(pump.serial_input_idle_trace_count, 0);
        assert_eq!(
            pump.serial_input_idle_trace_next_ms,
            SERIAL_INPUT_IDLE_TRACE_INTERVAL_MS * 2
        );

        pump.serial.flush_tx();
        pump.serial.driver_mut().drain_tx();
        pump.now_ms = pump.serial_input_idle_trace_next_ms;
        pump.maybe_emit_serial_input_idle_trace(false);

        assert_eq!(pump.serial_input_idle_trace_count, 0);
    }

    #[test]
    fn local_seat_remainder_defers_console_output_until_keyboard_idle() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::repeated(4, 1);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat =
            crate::local_seat::LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
                keyboard_device: "usb-kbd0",
                display_device: "hdmi0",
                line_bytes: 64,
                buffer_lines: 8,
            });
        local_seat.mark_root_console_ready();
        local_seat.enqueue_keyboard_bytes(b"help\nx");
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.poll();
        assert_eq!(pump.last_input_source, ConsoleInputSource::LocalSeat);
        assert_eq!(pump.local_line.as_str(), "x");
        assert!(pump.pending_console_output.len() >= 2);
        let first_turn = String::from_utf8(
            pump.serial_mut()
                .driver_mut()
                .drain_tx()
                .into_iter()
                .collect(),
        )
        .expect("serial output must be utf8");
        assert!(first_turn.contains("Commands:"), "{first_turn}");
        assert!(!first_turn.contains("Show this help"), "{first_turn}");

        pump.local_seat
            .as_mut()
            .expect("local-seat should be attached")
            .enqueue_keyboard_bytes(b"\x08");
        pump.poll();
        assert!(pump.local_line.is_empty());
        let _ = pump.serial_mut().driver_mut().drain_tx();

        pump.poll();
        let idle_turn = String::from_utf8(
            pump.serial_mut()
                .driver_mut()
                .drain_tx()
                .into_iter()
                .collect(),
        )
        .expect("serial output must be utf8");
        assert!(idle_turn.contains("Show this help"), "{idle_turn}");
        assert!(pump.metrics().physical_console_output_flushed > 0);
    }

    #[test]
    fn local_seat_output_does_not_block_follow_on_keyboard_input_on_slow_serial() {
        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 32, 16, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat =
            crate::local_seat::LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
                keyboard_device: "usb-kbd0",
                display_device: "hdmi0",
                line_bytes: 64,
                buffer_lines: 8,
            });
        local_seat.enqueue_keyboard_bytes(b"help\nx");
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.poll();

        assert_eq!(pump.last_input_source, ConsoleInputSource::LocalSeat);
        assert_eq!(pump.local_line.as_str(), "x");
        assert_eq!(
            pump.local_seat
                .as_ref()
                .expect("local-seat should be attached")
                .input_echo_preview(),
            "x"
        );
        assert!(pump.metrics().physical_console_output_deferred > 0);
        assert!(!pump.pending_console_output.is_empty());
    }

    #[test]
    fn physical_console_source_switches_clear_partial_input() {
        let driver = LoopbackSerial::<512>::new();
        let serial = SerialPort::<_, 512, 512, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat =
            crate::local_seat::LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
                keyboard_device: "usb-kbd0",
                display_device: "hdmi0",
                line_bytes: 64,
                buffer_lines: 8,
            });
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.local_seat
            .as_mut()
            .expect("local-seat should be attached")
            .enqueue_keyboard_bytes(b"pi");
        pump.poll();
        assert_eq!(pump.local_line.as_str(), "pi");

        pump.serial_mut().driver_mut().push_rx(b"help\n");
        pump.poll();
        assert!(pump.local_line.is_empty());
        assert_eq!(pump.last_input_source, ConsoleInputSource::Serial);

        pump.serial_mut().driver_mut().push_rx(b"he");
        pump.poll();
        pump.local_seat
            .as_mut()
            .expect("local-seat should be attached")
            .enqueue_keyboard_bytes(b"ping\n");
        pump.poll();
        assert!(!pump.serial.clear_partial_line());
        assert_eq!(pump.last_input_source, ConsoleInputSource::LocalSeat);

        drop(pump);
        assert!(audit
            .entries
            .iter()
            .any(|entry| entry.as_str() == "console: cleared local-seat input before serial"));
        assert!(audit
            .entries
            .iter()
            .any(|entry| entry.as_str() == "console: cleared serial input before local-seat"));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn tail_command_emits_end_sentinel() {
        let _root_guard = ReachableRootGuard::new(1);
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut bridge = NineDoorBridge::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_ninedoor(&mut bridge);
        pump.session = Some(SessionRole::Worker);
        {
            let driver = pump.serial_mut().driver_mut();
            driver.push_rx(b"tail /log/queen.log\n");
        }
        pump.poll();
        let transcript = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains("OK TAIL path=/log/queen.log"),
            "{rendered}"
        );
        assert!(rendered.contains("END\r\n"), "{rendered}");
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn net_origin_pending_stream_chunks_without_truncating() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        let total_lines = NET_PENDING_STREAM_FLUSH_LINES_PER_TURN + 3;
        {
            let mut pump =
                EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
            pump.session = Some(SessionRole::Worker);
            pump.session_origin = Some(ConsoleInputSource::Net);
            pump.last_input_source = ConsoleInputSource::Net;
            pump.stream_end_pending = true;
            pump.tail_active = true;

            let mut pending = PendingStream::new();
            for index in 0..total_lines {
                let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
                let _ = write!(line, "line-{index:02}");
                let _ = pending.lines.push(line);
            }
            pending.bandwidth_bytes = pending.lines.iter().map(|line| line.len() as u64).sum();
            pump.pending_stream = Some(pending);

            pump.flush_pending_stream();
            assert!(pump.stream_end_pending);
            assert!(pump.pending_stream.is_some());
            pump.flush_pending_stream();
            assert!(!pump.stream_end_pending);
            assert!(pump.pending_stream.is_none());
        }

        assert_eq!(net.sent.len(), total_lines + 1);
        for index in 0..total_lines {
            assert_eq!(net.sent[index].as_str(), format!("line-{index:02}"));
        }
        assert_eq!(net.sent[total_lines].as_str(), "END");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cat_queen_log_streams_full_payload_after_ack() {
        let _root_guard = ReachableRootGuard::new(1);
        let first_marker = "[test] cat-queen-log-streams-full-payload marker=batch-first index=00";
        let last_marker = "[test] cat-queen-log-streams-full-payload marker=batch-last index=69";
        for index in 0..70 {
            let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
            let label = if index == 69 {
                "batch-last"
            } else {
                "batch-first"
            };
            let _ = write!(
                line,
                "[test] cat-queen-log-streams-full-payload marker={} index={index:02}",
                label
            );
            log_buffer::append_log_line(line.as_str());
        }
        let driver = LoopbackSerial::<16384>::new();
        let serial = SerialPort::<_, 16384, 16384, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut bridge = NineDoorBridge::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_ninedoor(&mut bridge);
        pump.session = Some(SessionRole::Worker);
        {
            let driver = pump.serial_mut().driver_mut();
            driver.push_rx(b"cat /log/queen.log\n");
        }
        for _ in 0..4 {
            pump.poll();
        }
        let transcript = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains("OK CAT path=/log/queen.log"),
            "{rendered}"
        );
        assert!(
            rendered
                .lines()
                .any(|line| line.trim_end_matches('\r') == first_marker),
            "{rendered}"
        );
        assert!(
            rendered
                .lines()
                .any(|line| line.trim_end_matches('\r') == last_marker),
            "{rendered}"
        );
        let end = rendered.rfind("END\r\n").expect("END must be emitted");
        let last = rendered
            .rfind(last_marker)
            .expect("last marker must be emitted");
        assert!(last < end, "{rendered}");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn tail_queen_log_honors_default_and_requested_line_counts() {
        let _root_guard = ReachableRootGuard::new(1);
        fn append_tail_markers(prefix: &str) -> (String, String) {
            let first_marker = format!("[test] {prefix} marker=batch-first index=00");
            let last_marker = format!("[test] {prefix} marker=batch-last index=69");
            for index in 0..70 {
                let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
                let label = if index == 69 {
                    "batch-last"
                } else {
                    "batch-first"
                };
                let _ = write!(line, "[test] {prefix} marker={} index={index:02}", label);
                log_buffer::append_log_line(line.as_str());
            }
            (first_marker, last_marker)
        }

        fn run_tail(command: &[u8]) -> String {
            let driver = LoopbackSerial::<16384>::new();
            let serial = SerialPort::<_, 16384, 16384, DEFAULT_LINE_CAPACITY>::new(driver);
            let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
            let ipc = NullIpc;
            let store: TicketTable<4> = TicketTable::new();
            let mut audit = AuditLog::new();
            let mut bridge = NineDoorBridge::new();
            let mut pump =
                EventPump::new(serial, timer, ipc, store, &mut audit).with_ninedoor(&mut bridge);
            pump.session = Some(SessionRole::Worker);
            {
                let driver = pump.serial_mut().driver_mut();
                driver.push_rx(command);
            }
            for _ in 0..8 {
                pump.poll();
            }
            let transcript = {
                let driver = pump.serial_mut().driver_mut();
                driver.drain_tx()
            };
            String::from_utf8(transcript.into_iter().collect()).expect("serial output must be utf8")
        }

        let (first_marker, last_marker) = append_tail_markers("tail-default-lines");
        let rendered = run_tail(b"tail /log/queen.log\n");
        assert!(
            rendered.contains("OK TAIL path=/log/queen.log"),
            "{rendered}"
        );
        assert!(
            !rendered
                .lines()
                .any(|line| line.trim_end_matches('\r') == first_marker),
            "{rendered}"
        );
        assert!(
            rendered
                .lines()
                .any(|line| line.trim_end_matches('\r') == last_marker),
            "{rendered}"
        );
        assert!(rendered.contains("END\r\n"), "{rendered}");

        let (first_marker, last_marker) = append_tail_markers("tail-requested-lines");
        let rendered = run_tail(b"tail /log/queen.log 70\n");
        assert!(
            rendered.contains("OK TAIL path=/log/queen.log"),
            "{rendered}"
        );
        assert!(
            rendered
                .lines()
                .any(|line| line.trim_end_matches('\r') == first_marker),
            "{rendered}"
        );
        assert!(
            rendered
                .lines()
                .any(|line| line.trim_end_matches('\r') == last_marker),
            "{rendered}"
        );
        assert!(rendered.contains("END\r\n"), "{rendered}");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn log_command_emits_end_sentinel_and_quit_clears_session() {
        let driver = LoopbackSerial::<2048>::new();
        let serial = SerialPort::<_, 2048, 2048, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut bridge = NineDoorBridge::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_ninedoor(&mut bridge);
        {
            let driver = pump.serial_mut().driver_mut();
            let token = issue_token("ticket", Role::Queen);
            let line = format!("attach queen {token}\n");
            driver.push_rx(line.as_bytes());
            driver.push_rx(b"log\n");
            driver.push_rx(b"quit\n");
            driver.push_rx(b"log\n");
        }
        pump.poll();
        pump.poll();
        pump.poll();
        pump.poll();
        let mut rendered = String::new();
        loop {
            pump.serial_mut().poll_io();
            let transcript = {
                let driver = pump.serial_mut().driver_mut();
                driver.drain_tx()
            };
            if transcript.is_empty() {
                break;
            }
            rendered.push_str(
                String::from_utf8(transcript.into_iter().collect())
                    .expect("serial output must be utf8")
                    .as_str(),
            );
        }
        assert!(rendered.contains("OK ATTACH role=queen"), "{rendered}");
        assert!(rendered.contains("OK LOG"), "{rendered}");
        assert!(rendered.contains("END\r\n"), "{rendered}");
        assert!(rendered.contains("OK QUIT"), "{rendered}");
        assert!(
            rendered.contains("ERR LOG reason=policy detail=unauthenticated"),
            "{rendered}"
        );
    }

    #[test]
    fn ping_generates_pong_ack() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 32>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pong").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        pump.session = Some(SessionRole::Queen);
        {
            let driver = pump.serial_mut().driver_mut();
            driver.push_rx(b"PING\n");
        }
        pump.poll();
        pump.poll();
        let tx = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let transcript: Vec<u8> = tx.into_iter().collect();
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(rendered.contains("PONG"), "{rendered}");
    }

    #[test]
    fn local_seat_keyboard_ingress_uses_shared_parser_and_mirror() {
        let driver = LoopbackSerial::<64>::new();
        let serial = SerialPort::<_, 64, 64, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        local_seat.mark_root_console_ready();
        local_seat.enqueue_keyboard_bytes(b"ping\n");

        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);
        pump.session = Some(SessionRole::Queen);
        pump.poll();
        drop(pump);

        let mirrored = local_seat.mirrored_lines_snapshot();
        assert!(mirrored.iter().any(|line| line.contains("PONG")));
        assert!(mirrored.iter().any(|line| line.contains("cohesix>")));
    }

    #[test]
    fn serial_burst_drain_with_local_seat_dispatches_one_line_per_turn() {
        let driver = LoopbackSerial::<256>::new();
        let serial = SerialPort::<_, 256, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        local_seat.mark_root_console_ready();

        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);
        pump.session = Some(SessionRole::Queen);
        pump.serial_mut().driver_mut().push_rx(b"help\nhelp\n");

        pump.poll();

        assert_eq!(pump.metrics.accepted_commands, 1);
        pump.poll();
        assert_eq!(pump.metrics.accepted_commands, 2);
        assert!(pump.local_line.is_empty());
        assert!(!pump.parser.clear_buffer());
    }

    #[test]
    fn local_seat_keyboard_ingress_echoes_typed_bytes_before_completion() {
        let driver = LoopbackSerial::<64>::new();
        let serial = SerialPort::<_, 64, 64, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        local_seat.mark_root_console_ready();
        local_seat.enqueue_keyboard_bytes(b"hel");

        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);
        pump.poll();

        assert_eq!(
            pump.local_seat
                .as_ref()
                .expect("local-seat should be attached")
                .input_echo_preview(),
            "hel"
        );

        pump.local_seat
            .as_mut()
            .expect("local-seat should be attached")
            .enqueue_keyboard_bytes(b"\x08lp\n");
        pump.session = Some(SessionRole::Queen);
        pump.poll();

        assert!(pump
            .local_seat
            .as_ref()
            .expect("local-seat should be attached")
            .input_echo_preview()
            .is_empty());
        let mirrored = pump
            .local_seat
            .as_ref()
            .expect("local-seat should be attached")
            .mirrored_lines_snapshot();
        assert!(
            mirrored.iter().any(|line| line.contains("HELP")),
            "{mirrored:?}"
        );
    }

    #[test]
    fn local_seat_keyboard_ingress_backspace_edits_before_enter() {
        let driver = LoopbackSerial::<64>::new();
        let serial = SerialPort::<_, 64, 64, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        local_seat.mark_root_console_ready();
        local_seat.enqueue_keyboard_bytes(b"helx\x08p\n");

        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);
        pump.session = Some(SessionRole::Queen);
        pump.poll();

        assert!(pump.local_line.is_empty());
        let mirrored = pump
            .local_seat
            .as_ref()
            .expect("local-seat should be attached")
            .mirrored_lines_snapshot();
        assert!(
            mirrored.iter().any(|line| line.contains("HELP")),
            "{mirrored:?}"
        );
    }

    #[test]
    fn local_seat_input_drain_contract_keeps_serial_latency_bounded() {
        let (
            poll_passes,
            burst_passes,
            empty_polls,
            output_polls,
            hdmi_pump_passes,
            serial_lines,
            serial_chunk_bytes,
        ) = local_seat_input_drain_contract_for_test();
        assert_eq!(poll_passes, 1);
        assert_eq!(burst_passes, 4);
        assert_eq!(empty_polls, 1);
        assert!(output_polls >= 1);
        assert!(output_polls <= empty_polls);
        assert!(hdmi_pump_passes >= output_polls);
        assert!(hdmi_pump_passes <= burst_passes);
        assert_eq!(serial_lines, 1);
        assert_eq!(serial_chunk_bytes, 32);
        assert!(burst_passes * KEYBOARD_POLL_CHUNK_BYTES <= 512);
        assert!(serial_chunk_bytes <= crate::serial::DEFAULT_TX_CAPACITY / 2);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn usb_runtime_step_labels_keep_gate9_distinct_from_online() {
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_step_label(8),
            "keyboard-ready"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_step_label(9),
            "hid-first-report"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_step_label(10),
            "keyboard-online"
        );
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_runtime_detail_gates_match_ten_gate_ladder() {
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_gate_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY
            ),
            3
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_gate_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY
            ),
            4
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_gate_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING
            ),
            4
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_gate_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ROOT_PORT_CONNECTED
            ),
            5
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_gate_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_ADDRESSED
            ),
            6
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_gate_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED
            ),
            6
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_gate_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR
            ),
            7
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_gate_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ENDPOINT_SEEN
            ),
            7
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_gate_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
            ),
            7
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_gate_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY
            ),
            8
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_gate_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING
            ),
            8
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_gate_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY
            ),
            9
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_usb_runtime_detail_next_actions_are_actionable() {
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_next_action_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY
            ),
            "submit-enable-slot-command"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_blocker_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY
            ),
            "command-event-ring-not-proven"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_next_action_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING
            ),
            "poll-enable-slot-completion"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_recovery_policy_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY
            ),
            "same-controller-command-proof"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_recovery_policy_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING
            ),
            "same-controller-command-proof"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_recovery_policy_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY
            ),
            "same-controller-enumeration"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_next_action_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED
            ),
            "continue-enumeration-same-controller"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_blocker_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
            ),
            "hub-descriptor-failed"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_next_action_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
            ),
            "continue-enumeration-same-controller"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_recovery_policy_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
            ),
            "same-controller"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_next_action_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED
            ),
            "continue-enumeration-same-controller"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_next_action_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED
            ),
            "cold-reinit-and-reenumerate"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_recovery_policy_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED
            ),
            "cold-reinit"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_recovery_policy_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED
            ),
            "same-controller"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_recovery_policy_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING
            ),
            "hid-report-path"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_next_action_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY
            ),
            "wait-first-report"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_next_action_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING
            ),
            "poll-linked-interrupt-in-and-rering-endpoint-doorbell"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_next_action_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY
            ),
            "wait-first-keyboard-byte"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_blocker_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING
            ),
            "hid-first-report"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_blocker_for_linked_detail(
                pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY
            ),
            "keyboard-first-byte"
        );
        assert!(
            KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate(
                "enable-slot-completion-pending"
            )
        );
        assert!(
            KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate(
                "enable-slot-completion-poll-no-reply"
            )
        );
        assert!(
            KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate(
                "enable-slot-event-peek-no-reply"
            )
        );
        assert!(
            KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate(
                "enable-slot-event-dma-load-done-no-reply"
            )
        );
        assert!(
            KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate(
                "enable-slot-event-read-begin-no-reply"
            )
        );
        assert!(
            KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate(
                "enable-slot-event-read-done-no-reply"
            )
        );
        assert!(
            KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate(
                "enable-slot-event-slot-empty"
            )
        );
        assert!(
            KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate(
                "enable-slot-event-cycle-mismatch"
            )
        );
        assert!(
            KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate(
                "root-port-reset-no-reply"
            )
        );
        assert!(
            KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate(
                "root-port-reset-completion-no-reply"
            )
        );
        assert!(
            KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate(
                "root-port-stale-cleanup-failed"
            )
        );
        assert!(
            KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate(
                "address-device-command-completion-no-reply"
            )
        );
        for blocker in [
            "config-descriptor-header-status-event-ignored",
            "config-descriptor-full-transfer-event-slot-empty",
            "hid-endpoint-not-ready",
            "hid-interface-not-found",
            "hub-set-configuration-status-event-ignored",
            "hub-descriptor-transfer-no-reply",
            "hub-descriptor-status-event-cycle-mismatch",
            "hub-port-status-no-reply",
            "hub-port-status-transfer-event-ignored",
            "hub-port-reset-no-reply",
            "hub-port-reset-set-no-reply",
            "hub-port-reset-completion-no-reply",
            "hub-child-speed-fallback-no-reply",
            "hub-topology-no-keyboard",
        ] {
            assert!(
                KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate(blocker),
                "{blocker}"
            );
        }
        assert!(
            !KernelConsoleTestPump::usb_runtime_blocker_holds_current_gate("command-ring-ready")
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_startup_gate_status_marks_unproven_prefault_gates_as_inferred() {
        assert_eq!(
            KernelConsoleTestPump::wifi_startup_gate_status(1, 1, 6),
            "pass"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_startup_gate_status(5, 1, 6),
            "inferred"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_startup_gate_status(6, 1, 6),
            "fail"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_startup_gate_status(7, 1, 6),
            "blocked"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_post_release_mailbox_faults_stay_at_gate_seven() {
        let mailbox_fault = crate::drivers::driver_task_net::Cyw43RuntimeCommandFaultStatus {
            stage: "cyw43-firmware-release",
            op: 5,
            flags: 0,
            target_addr: 0,
            payload_offset: 0,
            payload_len: 0,
            total_len: 0,
            control_cmd: 0,
            control_id: 0,
            control_header_mode: "not-control",
            control_response_len: 0,
            detail: 0x532d,
            reason: "cyw43-post-release-mailbox-ready",
            result: 0,
        };
        let protocol_fault = crate::drivers::driver_task_net::Cyw43RuntimeCommandFaultStatus {
            detail: 0x532e,
            reason: "cyw43-post-release-protocol-version",
            ..mailbox_fault
        };

        assert_eq!(
            KernelConsoleTestPump::wifi_runtime_fault_gate(mailbox_fault),
            7
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_runtime_fault_gate(protocol_fault),
            7
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_sdio_replay_no_reply_reports_linked_runtime_blocker() {
        let status = crate::drivers::driver_task_net::SdioRuntimeReplayStatus {
            stage: "engine-init",
            status: "no-reply",
        };

        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_replay_gate(status),
            Some(2)
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_replay_blocker(status),
            "sdio-engine-init-no-reply"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_replay_next_action(status),
            "inspect-linked-sdio-runtime-engine-init-dispatch"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_sdio_adopt_progress_reports_gate_two_blockers() {
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_gate(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_BEGIN,
            ),
            Some(2)
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_blocker(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_PRESENT_READ_BEGIN,
            ),
            "sdio-adopt-present-read-no-reply"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_next_action(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_PRESENT_READ_BEGIN,
            ),
            "inspect-sdhci-present-state-read"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_gate(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RUNTIME_ENTRY,
            ),
            Some(2)
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_blocker(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RUNTIME_ENTRY,
            ),
            "sdio-engine-init-runtime-entry-no-reply"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_next_action(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RUNTIME_ENTRY,
            ),
            "inspect-linked-sdio-engine-init-hot-path-dispatch"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_blocker(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ENGINE_INIT_BRANCH,
            ),
            "sdio-engine-init-branch-no-reply"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_next_action(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ENGINE_INIT_BRANCH,
            ),
            "inspect-sdio-shadow-reset-entry"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_blocker(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_SHADOW_RESET_BEGIN,
            ),
            "sdio-shadow-reset-no-reply"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_next_action(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_SHADOW_RESET_DONE,
            ),
            "inspect-sdio-runtime-state-entry"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_blocker(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_HW_ENTRY,
            ),
            "sdio-sdhci-mmio-entry-no-reply"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_next_action(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_HW_ENTRY,
            ),
            "inspect-sdhci-first-mmio-access"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_blocker(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_ADOPT_CLOCK_FAILED,
            ),
            "sdio-adopt-clock-failed"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_sdio_runtime_progress_next_action(
                pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_SDIO_RESET_CLOCK_DISABLE_BEGIN,
            ),
            "inspect-sdhci-clock-disable-write"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_usb_runtime_queue_fields_decode_first_report_telemetry() {
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_queue_fields(0x0302_0104),
            (4, true, 2, 3, 0)
        );
        let filtered_result = 0x0302_0104
            | (u32::from(pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_FILTERED_KEY)
                << pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT);
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_queue_fields(filtered_result),
            (4, true, 2, 3, 7)
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_queue_fields(0x0000_0020),
            (32, false, 0, 0, 0)
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_report_status_label(7),
            "filtered-key"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_report_status_label(8),
            "unmatched-transfer"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_report_status_label(9),
            "queue-collapse"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_report_status_label(10),
            "recovery-success"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_report_status_label(11),
            "recovery-failed"
        );
        let diag_frame = crate::hal::driver_task::DriverFrameDescriptor {
            offset: (u32::from(pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_DIAG_MAGIC) << 16)
                | u32::from(pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_STAGE_SET_DEQUEUE)
                | (3 << 8),
            len: 1 | (2 << 8),
            flags: 4
                | (u16::from(
                    pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_REASON_FULL_QUEUE_NO_EVENT,
                ) << 8),
        };
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_recovery_diag_fields(diag_frame),
            (true, 7, 3, 1, 2, 4, 3)
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_recovery_stage_label(7),
            "set-dequeue"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_recovery_reason_label(3),
            "full-queue-no-event"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_input_observation(
                false, false, true, 128, false, 2
            ),
            "idle-report-no-key-byte"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_input_observation(
                true, false, true, 127, false, 6
            ),
            "byte-produced"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_input_observation(
                false, false, true, 0, false, 0
            ),
            "interrupt-queue-empty"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_input_observation(
                false, false, true, 128, false, 8
            ),
            "unmatched-transfer"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_input_observation(
                false, false, true, 4, false, 9
            ),
            "queue-collapse"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_keyboard_input_observation(
                false, false, true, 4, false, 11
            ),
            "recovery-failed"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_sustained_input_blocker(true, 4, 255, 6, 6, 97),
            "usb-post-first-byte-queue-collapse-risk"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_sustained_input_blocker(true, 8, 255, 6, 0, 0),
            "usb-post-first-byte-queue-collapse-risk"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_sustained_input_blocker(true, 9, 255, 6, 0, 0),
            "none"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_sustained_input_blocker(true, 4, 255, 9, 0, 0),
            "usb-post-first-byte-queue-collapse"
        );
        assert_eq!(
            KernelConsoleTestPump::usb_runtime_sustained_input_blocker(true, 4, 255, 11, 0, 0),
            "usb-post-first-byte-recovery-failed"
        );
        assert!(!KernelConsoleTestPump::usb_runtime_detail_has_queue_result(
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY
        ));
        assert!(KernelConsoleTestPump::usb_runtime_detail_has_queue_result(
            pi4_driver_abi::DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING
        ));
        assert!(KernelConsoleTestPump::usb_runtime_progress_superseded_by_keyboard(true, 10, 7));
        assert!(!KernelConsoleTestPump::usb_runtime_progress_superseded_by_keyboard(false, 10, 7));
        assert!(!KernelConsoleTestPump::usb_runtime_progress_superseded_by_keyboard(true, 10, 10));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_wifi_debug_command_uses_attached_debug_ops() {
        let driver = LoopbackSerial::<32768>::new();
        let serial = SerialPort::<_, 32768, 32768, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut wifi = FakeWifiDebug::new();
        wifi.expect_breadcrumb_suppression = true;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_wifi_debug(&mut wifi)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi dump-state\n");
        for _ in 0..40 {
            pump.poll();
        }

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "wifi: debug subcommand=dump-state action=begin profile=bounded mode=one-shot"
            ),
            "{rendered}"
        );
        assert!(rendered.contains("wifi: power=on"), "{rendered}");
        assert!(
            rendered.contains("wifi: bootstrap=first-write-startup-link"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: snapshot source=cached stage=control-plane-startup-link-rearm-stalled rescue=1/2 passive_limit=8 replay_full_bootstrap=no"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: golden_path route=strict-then-bounded-no-ht state=fallback-no-ht transport=bounded-no-ht current=function1-sideband next=first-function2-reply focus=function1-sideband verdict=function1-sideband-edge"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: reply_contract path=startup-link-f2 strict_recovery_f2=preserve-latch blocker_class=f1-sideband"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: contract current=function1-sideband expected=f1-sideband-readable observed=sdhci=f1-reply-read-command-phase-no-data-active+f2=latched-linux-configured-no-iorx blocker=f1-sideband path=startup-link-f2"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: firmware_ht_req alp=0x08 ht=0x10 ht_retry=0x10 force_ht_after_proof=n/a"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: firmware_ht_state chipclk=0x50 wake=0x02 sleep=0x01 kso=yes devon=no cardcap=0x08 f1=enabled-ready f2=disabled-not-ready blocker=chipclkcsr-ht-avail-missing next=linux-capture-post-release-chipclkcsr"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: firmware_proof source=cached upload=uploaded nvram_tail=written rstvec=written cpuhalt=released"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: ht_summary state=ht-timeout-cached records=1 f2_gate=function2-disabled-until-ht-proof"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: ht_record[0] stage=debug-probe-ht status=active-ht-timeout chipclk=0x50 wake=0x02 sleep=0x01 kso=yes devon=no cardcap=0x08"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: sdhci_contract current=f1-reply-read-command-phase-no-data-active preserved=f2-reply-read-command-phase-no-data-active resolved=f1-reply-read-command-phase-no-data-active"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: sdhci_live cmd=0x0035 arg=0x14008000 ps=0x00010002 stat=0x00000001"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: sdhci_preserved cmd=0x0035 arg=0x24008000 ps=0x00010002 stat=0x00000002"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: cccr ioex=0x06 iordy=0x02 ien=0x07 rframe_lo=0x40 rframe_hi=0x00 watermark=0x08 devctl=0x02 mesbusy=0x01"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: sdio_shadow block_size_count=0x00000200 transfer_mode=0x00000033 backplane_bytes=00:80:19"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: preserved_failure source=cached stage=control-plane-startup-link-rearm-stalled exact=cyw43-control-plane-sideband-command-stall sdhci=f1-reply-read-command-phase-no-data-active f2_state=latched-linux-configured-no-iorx"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: boot_failure_snapshot source=cached stage=control-plane-startup-link-rearm-stalled exact=cyw43-control-plane-sideband-command-stall sdhci=f1-reply-read-command-phase-no-data-active f2_state=latched-linux-configured-no-iorx"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: cccr_cached ioex=0x06 iordy=0x02 ien=0x07 if=0x80 speed=0x02 cardcap=0x08 fbr1_blk=0x0040 fbr2_blk=0x0200"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: bounded_phase[0] stage=control-plane-startup-link-rearm-stalled action=cached-failure mode=startup-link clock=400000Hz width=4bit no_ht=yes"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("safe_profile_locked=yes safe_reason=promoted-io-unstable"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: f2_recover stage=control-plane-reply-full-block-read policy=linux-rxfail op=read drained=no count=0x0040 nak_sent=yes rearm_budget=reply-strict-recovery rearm_action=skip-function2-ready-repoll"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: reply_probe lane=startup-link effective_clock=3125000Hz"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: f2_state=latched-linux-configured-no-iorx exact_error=cyw43-control-plane-sideband-command-stall sdhci_read_diag=f1-reply-read-command-phase-no-data-active"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: verdict=function1-sideband-edge focus=function1-sideband bootstrap=first-write-startup-link"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: debug subcommand=dump-state action=complete profile=bounded mode=one-shot result=ok"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK WIFI detail=subcommand=dump-state"),
            "{rendered}"
        );
        assert_eq!(wifi.calls.as_slice(), &["dump-state"]);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn qemu_manifest_hides_usb_wifi_debug_surface() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut wifi = FakeWifiDebug::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_wifi_debug(&mut wifi)
            .with_local_seat(&mut local_seat);

        pump.serial_mut().driver_mut().push_rx(b"help\n");
        for _ in 0..4 {
            pump.poll();
        }
        let help_transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        let help_rendered = String::from_utf8(help_transcript).expect("serial output must be utf8");
        assert!(!help_rendered.contains("usb <"), "{help_rendered}");
        assert!(!help_rendered.contains("wifi <"), "{help_rendered}");

        pump.serial_mut()
            .driver_mut()
            .push_rx(b"wifi dump-state\nusb status\n");
        for _ in 0..512 {
            pump.poll();
        }
        let command_transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let command_rendered =
            String::from_utf8(command_transcript).expect("serial output must be utf8");
        assert!(!command_rendered.contains("OK WIFI"), "{command_rendered}");
        assert!(!command_rendered.contains("ERR WIFI"), "{command_rendered}");
        assert!(!command_rendered.contains("OK USB"), "{command_rendered}");
        assert!(!command_rendered.contains("ERR USB"), "{command_rendered}");
        assert!(wifi.calls.is_empty());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_usb_wifi_debug_reject_unadvertised_triage_aliases() {
        let driver = LoopbackSerial::<2048>::new();
        let serial = SerialPort::<_, 2048, 2048, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut wifi = FakeWifiDebug::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_wifi_debug(&mut wifi)
            .with_local_seat(&mut local_seat)
            .with_test_pi4_debug_commands();

        pump.serial_mut()
            .driver_mut()
            .push_rx(b"wifi triage\nusb triage\n");
        for _ in 0..16 {
            pump.poll();
        }

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("ERR WIFI reason=policy detail=unknown-subcommand"),
            "{rendered}"
        );
        assert!(
            rendered.contains("ERR USB reason=policy detail=unknown-subcommand"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("OK WIFI detail=subcommand=diag"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("OK USB detail=subcommand=diag"),
            "{rendered}"
        );
        assert!(wifi.calls.is_empty());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_usb_wifi_help_lists_help_subcommands() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut wifi = FakeWifiDebug::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_wifi_debug(&mut wifi)
            .with_local_seat(&mut local_seat)
            .with_test_pi4_debug_commands();

        pump.serial_mut()
            .driver_mut()
            .push_rx(b"usb help\nwifi help\n");
        for _ in 0..16 {
            pump.poll();
        }

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("usb help        - Show USB local-seat debug command help"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi help       - Show WiFi debug command help"),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK USB detail=subcommand=help scope=serial-local"),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK WIFI detail=subcommand=help scope=serial-local"),
            "{rendered}"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_wifi_diag_command_runs_dump_probe_dump() {
        let driver = LoopbackSerial::<32768>::new();
        let serial = SerialPort::<_, 32768, 32768, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut wifi = FakeWifiDebug::new();
        wifi.snapshot.debug_snapshot_stage = "console-diag-before";
        wifi.snapshot.control_plane_exact_error = "";
        wifi.expect_breadcrumb_suppression = true;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_wifi_debug(&mut wifi)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi diag\n");
        let mut transcript = Vec::new();
        for _ in 0..40 {
            pump.poll();
            transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        }

        transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(wifi.breadcrumb_suppression_observed);
        assert!(
            rendered
                .contains("wifi: debug subcommand=diag action=begin profile=bounded mode=one-shot"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: diag stage=before-ht-probe"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: diag ht_probe ready=yes"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: diag recorder=startup-blackbox mode=passive source=cached"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: gate 1 name=runtime-power-reset status=pass"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: gate 6 name=firmware-upload"),
            "{rendered}"
        );
        assert!(rendered.contains("wifi: next_action="), "{rendered}");
        assert!(
            rendered.contains("wifi: diag stage=after-ht-probe"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: ht_state chipclk=0x50"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: f2_gate policy=post-ht-proof gate=blocked-latched-not-ready"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("wifi: firmware_proof source=cached upload=uploaded"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("wifi: cccr_cached ioex=0x06 iordy=0x02"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: debug subcommand=diag action=complete profile=bounded mode=one-shot result=ok"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK WIFI detail=subcommand=diag"),
            "{rendered}"
        );
        assert!(rendered.contains("cohesix> "), "{rendered}");
        assert_eq!(
            wifi.calls.as_slice(),
            &["dump-state", "probe-ht", "dump-state"]
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_wifi_diag_skips_probe_after_control_plane_failure() {
        let driver = LoopbackSerial::<32768>::new();
        let serial = SerialPort::<_, 32768, 32768, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut wifi = FakeWifiDebug::new();
        wifi.snapshot.current_clock_hz = 41_666_666;
        wifi.snapshot.card_ready = true;
        wifi.snapshot.debug_snapshot_stage = "console-diag-before";
        wifi.snapshot.control_plane_exact_error = "cyw43-control-plane-partial-hint-visibility";
        wifi.expect_breadcrumb_suppression = true;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_wifi_debug(&mut wifi)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi diag\n");
        let mut transcript = Vec::new();
        for _ in 0..40 {
            pump.poll();
            transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        }

        transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(wifi.breadcrumb_suppression_observed);
        assert!(
            rendered.contains(
                "wifi: diag ht_probe skipped reason=preserved-control-plane-failure exact_error=cyw43-control-plane-partial-hint-visibility"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: diag stage=after-ht-probe skipped=yes snapshot=unchanged"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: debug subcommand=diag action=complete profile=bounded mode=one-shot result=ok"),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK WIFI detail=subcommand=diag"),
            "{rendered}"
        );
        assert_eq!(wifi.calls.as_slice(), &["dump-state"]);
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn serial_wifi_diag_reports_cached_driver_task_failure_without_hal_debug_handle() {
        let driver = LoopbackSerial::<2048>::new();
        let serial = SerialPort::<_, 2048, 2048, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut cause = HeaplessString::<192>::new();
        let _ = cause.push_str("cyw43-command driver-task runtime init failed");
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network_unavailable_detail(Some(cause))
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi diag\n");
        let mut transcript = Vec::new();
        for _ in 0..128 {
            pump.poll();
            transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        }

        transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("wifi: driver-task replay failure detail=net-disabled cause=cyw43-command driver-task runtime init failed"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: diag recorder=startup-blackbox mode=passive source=debug-handle-unavailable"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: gate 1 name=runtime-power-reset status=fail"),
            "{rendered}"
        );
        assert!(rendered.contains("wifi: next_action="), "{rendered}");
        assert!(
            rendered.contains("wifi: debug subcommand=diag action=complete profile=bounded mode=one-shot result=ok source=linked-runtime-replay-failure"),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK WIFI detail=subcommand=diag scope=serial-local source=linked-runtime-replay-failure"),
            "{rendered}"
        );
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn serial_wifi_diag_reports_progress_only_snapshot_without_hal_debug_handle() {
        let _progress_guard = wifi_driver_task_progress_test_guard();
        let cyw43 = crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT;
        let sdio = crate::hal::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT;
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(cyw43);
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(sdio);
        crate::hal::driver_task::test_record_driver_task_ring_progress_snapshot(
            cyw43,
            77,
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CONTROL_RX_POLL_BEGIN,
            0x4359_5734,
        );

        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi diag\n");
        let mut transcript = Vec::new();
        for _ in 0..128 {
            pump.poll();
            transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        }

        transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        drop(pump);
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(cyw43);
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(sdio);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "wifi: driver-task replay failure detail=net-state-unavailable source=debug-handle-unavailable"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: cyw43 linked_runtime_progress marker_valid=yes sequence=77"),
            "{rendered}"
        );
        assert!(
            rendered.contains("phase_name=cyw43-control-rx-poll-begin"),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK WIFI detail=subcommand=diag scope=serial-local source=linked-runtime-replay-failure"),
            "{rendered}"
        );
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn serial_wifi_diag_reports_host_eapol_required_as_live_frontier() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut cause = HeaplessString::<192>::new();
        let _ = cause.push_str("host-eapol-required driver-task runtime init failed");
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network_unavailable_detail(Some(cause))
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi diag\n");
        let mut transcript = Vec::new();
        for _ in 0..128 {
            pump.poll();
            transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        }

        transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("cause=host-eapol-required driver-task runtime init failed"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: gate 8 name=host-eapol status=fail evidence=exact=host-eapol-required"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: evidence boundary console_client=root-net-console hal=admission-descriptor-diagnostics-only linked_runtime_owner=cyw43+sdio failure_domain=host-eapol-required direct_proof_gate=7"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: next_action=inspect-host-eapol-rx-path blocker=host-eapol-required proof_gate=7"
            ),
            "{rendered}"
        );
        assert!(
            !rendered.contains("wifi: cyw43 fault stage=cyw43-control-exchange"),
            "{rendered}"
        );
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn serial_wifi_diag_reports_host_eapol_pending_as_live_frontier() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut cause = HeaplessString::<192>::new();
        let _ = cause.push_str("wifi-host-eapol-pending driver-task runtime init failed");
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network_unavailable_detail(Some(cause))
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi diag\n");
        let mut transcript = Vec::new();
        for _ in 0..128 {
            pump.poll();
            transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        }

        transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("cause=wifi-host-eapol-pending driver-task runtime init failed"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: gate 8 name=host-eapol status=fail evidence=exact=wifi-host-eapol-pending"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: evidence boundary console_client=root-net-console hal=admission-descriptor-diagnostics-only linked_runtime_owner=cyw43+sdio failure_domain=wifi-host-eapol-pending direct_proof_gate=7"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: next_action=inspect-host-eapol-rx-path blocker=wifi-host-eapol-pending proof_gate=7"
            ),
            "{rendered}"
        );
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn serial_wifi_diag_prefers_live_net_host_eapol_pending_frontier() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.active_interface = "wifi";
        net.status.address_source = "wifi-host-eapol-pending";
        net.status.dhcp_phase = "host-eapol-pending";
        let mut wifi = FakeWifiDebug::new();
        wifi.runtime_required = true;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network(&mut net)
            .with_wifi_debug(&mut wifi)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi diag\n");
        let mut transcript = Vec::new();
        for _ in 0..128 {
            pump.poll();
            transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        }

        transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "wifi: gate 8 name=host-eapol status=fail evidence=exact=wifi-host-eapol-pending"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: evidence boundary console_client=root-net-console hal=admission-descriptor-diagnostics-only linked_runtime_owner=cyw43+sdio failure_domain=wifi-host-eapol-pending direct_proof_gate=7"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: next_action=inspect-host-eapol-rx-path blocker=wifi-host-eapol-pending proof_gate=7"
            ),
            "{rendered}"
        );
        assert!(!rendered.contains("wifi: cyw43 fault stage="), "{rendered}");
        assert_eq!(wifi.calls.as_slice(), &["dump-state"]);
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn serial_wifi_diag_prefers_live_dhcp_frontier_after_secure_release() {
        let _progress_guard = wifi_driver_task_progress_test_guard();
        let cyw43 = crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT;
        let sdio = crate::hal::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT;
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(cyw43);
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(sdio);
        crate::hal::driver_task::test_record_driver_task_ring_progress_snapshot(
            cyw43,
            77,
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_CYW43_CONTROL_RX_POLL_BEGIN,
            0x4359_5734,
        );

        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        net.status.active_interface = "wifi";
        net.status.interface_policy = "wifi";
        net.status.address_source = "dhcp-pending";
        net.status.dhcp_phase = "selecting";
        let _ = net.status.ip.push_str("0.0.0.0");
        net.counters.wifi_assoc = 1;
        net.counters.wifi_link_up = 1;
        net.counters.wifi_host_eapol_rx = 4;
        net.counters.wifi_host_eapol_secure = 1;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network(&mut net)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi diag\n");
        let mut transcript = Vec::new();
        for _ in 0..128 {
            pump.poll();
            transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        }

        transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        drop(pump);
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(cyw43);
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(sdio);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "wifi: driver-task replay state detail=live-net-frontier source=debug-handle-unavailable active=wifi address_source=dhcp-pending dhcp_phase=selecting ip=0.0.0.0 assoc=1 link=1 eapol_secure=1"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: diag recorder=startup-blackbox mode=passive source=live-net-status"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: gate 8 name=firmware-channel status=pass"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: gate 9 name=dhcp-bound status=fail evidence=active=wifi address_source=dhcp-pending dhcp_phase=selecting ip=0.0.0.0"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "wifi: next_action=run-dhcp-and-report-lease-state blocker=dhcp-pending proof_gate=8 target_gate=10 source=live-net-status"
            ),
            "{rendered}"
        );
        assert!(
            !rendered.contains("wifi: driver-task replay failure detail=net-state-unavailable"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("wifi: cyw43 linked_runtime_progress"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("wifi: gate 8 name=control-exchange status=fail"),
            "{rendered}"
        );
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn serial_wifi_probe_ht_prefers_live_dhcp_frontier_after_secure_release() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut wifi = FakeWifiDebug::new();
        wifi.runtime_required = true;
        let mut net = FakeNet::new();
        net.status.active_interface = "wifi";
        net.status.interface_policy = "wifi";
        net.status.address_source = "dhcp-pending";
        net.status.dhcp_phase = "selecting";
        let _ = net.status.ip.push_str("0.0.0.0");
        net.counters.wifi_assoc = 1;
        net.counters.wifi_link_up = 1;
        net.counters.wifi_host_eapol_rx = 4;
        net.counters.wifi_host_eapol_secure = 1;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network(&mut net)
            .with_wifi_debug(&mut wifi)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi probe-ht\n");
        let mut transcript = Vec::new();
        for _ in 0..64 {
            pump.poll();
            transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        }

        transcript.extend(pump.serial_mut().driver_mut().drain_tx());
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "wifi: driver-task replay state detail=live-net-frontier source=hal-runtime-required active=wifi address_source=dhcp-pending dhcp_phase=selecting ip=0.0.0.0 assoc=1 link=1 eapol_secure=1"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: debug subcommand=probe-ht action=complete profile=bounded mode=one-shot result=ok source=live-net-status"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "OK WIFI detail=subcommand=probe-ht scope=serial-local source=live-net-status"
            ),
            "{rendered}"
        );
        assert!(
            !rendered.contains("ERR WIFI reason=policy detail=subcommand=probe-ht"),
            "{rendered}"
        );
        assert_eq!(wifi.calls.as_slice(), &["probe-ht"]);
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn serial_wifi_diag_reports_runtime_required_driver_task_snapshot() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut cause = HeaplessString::<192>::new();
        let _ = cause.push_str("cyw43-command driver-task runtime init failed");
        let mut wifi = FakeWifiDebug::new();
        wifi.runtime_required = true;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network_unavailable_detail(Some(cause))
            .with_wifi_debug(&mut wifi)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi diag\n");
        for _ in 0..8 {
            pump.poll();
        }

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("wifi: driver-task replay failure detail=net-disabled cause=cyw43-command driver-task runtime init failed"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: debug subcommand=diag action=complete profile=bounded mode=one-shot result=ok source=linked-runtime-replay-failure"),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK WIFI detail=subcommand=diag scope=serial-local source=linked-runtime-replay-failure"),
            "{rendered}"
        );
        assert_eq!(wifi.calls.as_slice(), &["dump-state"]);
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn serial_wifi_probe_ht_reports_runtime_required_driver_task_snapshot_error() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut cause = HeaplessString::<192>::new();
        let _ = cause.push_str("cyw43-command driver-task runtime init failed");
        let mut wifi = FakeWifiDebug::new();
        wifi.runtime_required = true;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network_unavailable_detail(Some(cause))
            .with_wifi_debug(&mut wifi)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi probe-ht\n");
        for _ in 0..8 {
            pump.poll();
        }

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("wifi: driver-task replay failure detail=net-disabled cause=cyw43-command driver-task runtime init failed"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: debug subcommand=probe-ht action=complete profile=bounded mode=one-shot result=err source=linked-runtime-replay-failure error=pi4-wifi-driver-task-runtime-required"),
            "{rendered}"
        );
        assert!(
            rendered.contains("ERR WIFI reason=policy detail=subcommand=probe-ht error=pi4-wifi-driver-task-runtime-required source=linked-runtime-replay-failure"),
            "{rendered}"
        );
        assert_eq!(wifi.calls.as_slice(), &["probe-ht"]);
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn serial_wifi_load_fw_reports_runtime_required_driver_task_snapshot_error() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut cause = HeaplessString::<192>::new();
        let _ = cause.push_str("sdio-host driver-task runtime init failed");
        let mut wifi = FakeWifiDebug::new();
        wifi.runtime_required = true;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network_unavailable_detail(Some(cause))
            .with_wifi_debug(&mut wifi)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi load-fw\n");
        for _ in 0..8 {
            pump.poll();
        }

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("wifi: driver-task replay failure detail=net-disabled cause=sdio-host driver-task runtime init failed"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: debug subcommand=load-fw action=complete profile=stateful mode=one-shot result=err source=linked-runtime-replay-failure error=pi4-wifi-driver-task-runtime-required"),
            "{rendered}"
        );
        assert!(
            rendered.contains("ERR WIFI reason=policy detail=subcommand=load-fw error=pi4-wifi-driver-task-runtime-required source=linked-runtime-replay-failure"),
            "{rendered}"
        );
        assert_eq!(wifi.calls.as_slice(), &["load-fw"]);
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn serial_wifi_retry_reports_runtime_required_driver_task_snapshot_error() {
        let driver = LoopbackSerial::<4096>::new();
        let serial = SerialPort::<_, 4096, 4096, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut cause = HeaplessString::<192>::new();
        let _ = cause.push_str("sdio-host driver-task runtime init failed");
        let mut wifi = FakeWifiDebug::new();
        wifi.runtime_required = true;
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_network_unavailable_detail(Some(cause))
            .with_wifi_debug(&mut wifi)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi retry\n");
        for _ in 0..8 {
            pump.poll();
        }

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("wifi: driver-task replay failure detail=net-disabled cause=sdio-host driver-task runtime init failed"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: debug subcommand=retry action=complete profile=stateful mode=one-shot result=err source=linked-runtime-replay-failure error=pi4-wifi-driver-task-runtime-required"),
            "{rendered}"
        );
        assert!(
            rendered.contains("ERR WIFI reason=policy detail=subcommand=retry error=pi4-wifi-driver-task-runtime-required source=linked-runtime-replay-failure"),
            "{rendered}"
        );
        assert_eq!(wifi.calls.as_slice(), &["retry"]);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_wifi_diag_unavailable_returns_error_and_prompt() {
        #[cfg(feature = "net-console")]
        let _progress_guard = wifi_driver_task_progress_test_guard();
        #[cfg(feature = "net-console")]
        {
            crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(
                crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            );
            crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(
                crate::hal::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
            );
        }
        let driver = LoopbackSerial::<1024>::new();
        let serial = SerialPort::<_, 1024, 1024, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi diag\n");
        for _ in 0..4 {
            pump.poll();
        }

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered
                .contains("wifi: debug subcommand=diag action=begin profile=bounded mode=one-shot"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: debug subcommand=diag action=complete profile=bounded mode=one-shot result=error error=unsupported operation: wifi-debug-unavailable"),
            "{rendered}"
        );
        assert!(
            rendered.contains("ERR WIFI reason=policy detail=subcommand=diag error=unsupported operation: wifi-debug-unavailable"),
            "{rendered}"
        );
        assert!(rendered.contains("cohesix> "), "{rendered}");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_wifi_debug_command_preserves_long_exact_error_lines() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut wifi = FakeWifiDebug::new();
        wifi.snapshot.control_plane_exact_error =
            "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready";
        wifi.snapshot.control_plane_sdhci_read_diag = "f2-reply-read-stalled-no-buffer-ready";
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_wifi_debug(&mut wifi)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi dump-state\n");
        pump.poll();
        pump.poll();

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "wifi: f2_state=latched-linux-configured-no-iorx exact_error=cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready sdhci_read_diag=f2-reply-read-stalled-no-buffer-ready"
            ),
            "{rendered}"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_usb_debug_command_enables_local_seat_polling() {
        let driver = LoopbackSerial::<2048>::new();
        let serial = SerialPort::<_, 512, 2048, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_local_seat(&mut local_seat)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"usb enable-kbd\n");
        for _ in 0..8 {
            pump.poll();
        }

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("usb: local-seat attached=no polling=enabled"),
            "{rendered}"
        );
        assert!(
            rendered.contains("usb: runtime_queue queue_valid=no detail=0x0000 result=0x00000000"),
            "{rendered}"
        );
        assert!(
            rendered.contains("usb: stall_telemetry queue_valid=no"),
            "{rendered}"
        );
        assert!(
            rendered.contains("usb: output_pressure serial_tx_pending="),
            "{rendered}"
        );
        assert!(
            rendered.contains("usb: stall_counter domain=usb-runtime"),
            "{rendered}"
        );
        assert!(
            rendered.contains("usb: stall_counter domain=hdmi-display"),
            "{rendered}"
        );
        assert!(
            rendered.contains("usb: verdict=backend-not-attached focus=probe-controller"),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK USB detail=subcommand=enable-kbd"),
            "{rendered}"
        );
        assert!(local_seat.backend_keyboard_polling_enabled());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_usb_debug_probe_command_returns_without_arming_background_polling() {
        let driver = LoopbackSerial::<2048>::new();
        let serial = SerialPort::<_, 512, 2048, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_local_seat(&mut local_seat)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"usb probe-kbd\n");
        pump.poll();
        pump.poll();

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("usb: probing local-seat keyboard now"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "usb: local-seat attached=no polling=deferred action=keyboard-probe-complete probe_result=backend-unavailable"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("usb: verdict=backend-not-attached focus=probe-controller"),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK USB detail=subcommand=probe-kbd"),
            "{rendered}"
        );
        assert!(!local_seat.backend_keyboard_polling_enabled());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_usb_dump_state_reports_lexical_subcommand() {
        let driver = LoopbackSerial::<2048>::new();
        let serial = SerialPort::<_, 512, 2048, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_local_seat(&mut local_seat)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"usb dump-state\n");
        for _ in 0..4 {
            pump.poll();
        }

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("usb: local-seat attached=no polling=deferred"),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK USB detail=subcommand=dump-state scope=serial-local"),
            "{rendered}"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_usb_diag_command_skips_live_probe_without_arming_background_polling() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_local_seat(&mut local_seat)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"usb diag\n");
        for _ in 0..4 {
            pump.poll();
        }

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("usb: local-seat attached=no polling=deferred action=diag-passive"),
            "{rendered}"
        );
        assert!(
            rendered.contains(
                "usb: diag action=probe-skipped reason=linked-runtime-only use=usb-status"
            ),
            "{rendered}"
        );
        assert!(!rendered.contains("action=diag-before-probe"), "{rendered}");
        assert!(!rendered.contains("action=diag-after-probe"), "{rendered}");
        assert!(
            rendered.contains("usb: diag recorder=startup-blackbox mode=passive source=cached"),
            "{rendered}"
        );
        assert!(
            rendered.contains("usb: gate 1 name=hal-resources"),
            "{rendered}"
        );
        assert!(
            rendered.contains("usb: gate 10 name=first-console-byte"),
            "{rendered}"
        );
        assert!(
            rendered.contains("usb: next_action=boot-pi4-linked-runtime"),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK USB detail=subcommand=diag"),
            "{rendered}"
        );
        assert!(!local_seat.backend_keyboard_polling_enabled());
        let mirrored = local_seat.mirrored_lines_snapshot();
        assert_eq!(mirrored.len(), 1, "{mirrored:?}");
        assert!(mirrored[0].contains("USB diag complete"), "{mirrored:?}");
        assert_eq!(local_seat.dropped_mirrored_lines(), 0);
        assert!(rendered.contains("cohesix> "), "{rendered}");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_usb_diag_unavailable_returns_error_and_prompt() {
        let driver = LoopbackSerial::<512>::new();
        let serial = SerialPort::<_, 512, 512, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"usb diag\n");
        for _ in 0..4 {
            pump.poll();
        }

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "ERR USB reason=policy detail=subcommand=diag error=local-seat-unavailable"
            ),
            "{rendered}"
        );
        assert!(rendered.contains("cohesix> "), "{rendered}");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_capture_verdict_classifies_first_reply_edge() {
        let snapshot = WifiDebugSnapshot {
            power_state: WifiPowerState::On,
            reset_state: WifiResetState::Deasserted,
            current_clock_hz: 400_000,
            preferred_data_clock_hz: 12_500_000,
            bus_width: SdioBusWidth::FourBit,
            card_ready: true,
            card_rca: 1,
            card_ocr: 0xb0ff_ff00,
            io_enable: Some(0x06),
            io_ready: Some(0x02),
            chipclkcsr: Some(0x3a),
            wakeupctrl: Some(0x02),
            sleepcsr: Some(0x01),
            cardcap: Some(0x08),
            programmed_backplane_window: Some(0x1800_0000),
            shadow_backplane_window: Some(0x1800_0000),
            shadow_backplane_fn_addr: Some(0x08000),
            control_plane_frame_recovery_stage: Some("control-plane-reply-prefix-read"),
            control_plane_frame_recovery_policy: Some("linux-rxfail"),
            control_plane_frame_recovery_write: Some(false),
            control_plane_frame_recovery_drained: Some(false),
            control_plane_frame_recovery_count: Some(0x40),
            control_plane_bootstrap_phase: "first-write-startup-link",
            control_plane_reply_mode: "startup-link",
            control_plane_reply_attempts: 1,
            control_plane_reply_empty_polls: 0,
            control_plane_no_ht_transport: true,
            control_plane_probe_pending: false,
            control_plane_startup_link_stable: false,
            control_plane_startup_profile_locked: true,
            control_plane_startup_profile_reason: "promoted-io-unstable",
            control_plane_promoted_probe_pending: false,
            debug_snapshot_source: "cached",
            debug_snapshot_stage: "control-plane-passive-startup-link-timeout",
            control_plane_startup_link_rescue_cycles: 0,
            control_plane_startup_link_rescue_limit: 2,
            control_plane_passive_startup_link_empty_poll_limit: 16,
            control_plane_f2_state: "latched-linux-configured-no-iorx",
            control_plane_sdhci_read_diag: "f2-reply-read-data-wait",
            control_plane_exact_error: "cyw43-control-plane-pure-f2-startup-link-no-reply",
        };
        assert_eq!(
            KernelConsoleTestPump::wifi_capture_verdict(&snapshot),
            ("function2-reply-edge", "first-function2-reply")
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_capture_verdict_classifies_function2_reply_read_errors_as_reply_edge() {
        let snapshot = WifiDebugSnapshot {
            power_state: WifiPowerState::On,
            reset_state: WifiResetState::Deasserted,
            current_clock_hz: 400_000,
            preferred_data_clock_hz: 12_500_000,
            bus_width: SdioBusWidth::FourBit,
            card_ready: true,
            card_rca: 1,
            card_ocr: 0xb0ff_ff00,
            io_enable: Some(0x06),
            io_ready: Some(0x02),
            chipclkcsr: Some(0x3a),
            wakeupctrl: Some(0x02),
            sleepcsr: Some(0x01),
            cardcap: Some(0x08),
            programmed_backplane_window: Some(0x1800_0000),
            shadow_backplane_window: Some(0x1800_0000),
            shadow_backplane_fn_addr: Some(0x08000),
            control_plane_frame_recovery_stage: Some("control-plane-reply-speculative-read"),
            control_plane_frame_recovery_policy: Some("linux-rxfail"),
            control_plane_frame_recovery_write: Some(false),
            control_plane_frame_recovery_drained: Some(true),
            control_plane_frame_recovery_count: Some(0),
            control_plane_bootstrap_phase: "startup-link-recovery",
            control_plane_reply_mode: "startup-link-resume",
            control_plane_reply_attempts: 1,
            control_plane_reply_empty_polls: 0,
            control_plane_no_ht_transport: true,
            control_plane_probe_pending: false,
            control_plane_startup_link_stable: false,
            control_plane_startup_profile_locked: true,
            control_plane_startup_profile_reason: "",
            control_plane_promoted_probe_pending: false,
            debug_snapshot_source: "cached",
            debug_snapshot_stage: "control-plane-startup-link-rearm-stalled",
            control_plane_startup_link_rescue_cycles: 1,
            control_plane_startup_link_rescue_limit: 2,
            control_plane_passive_startup_link_empty_poll_limit: 8,
            control_plane_f2_state: "latched-no-iorx",
            control_plane_sdhci_read_diag: "f2-reply-read-stalled-no-buffer-ready",
            control_plane_exact_error: "cyw43-function2-reply-read-stall-no-buffer-ready",
        };
        assert_eq!(
            KernelConsoleTestPump::wifi_capture_verdict(&snapshot),
            ("function2-reply-edge", "first-function2-reply")
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_golden_path_current_step(&snapshot),
            "first-function2-reply"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_reply_contract_prefers_preserve_latch_on_startup_link() {
        let snapshot = WifiDebugSnapshot {
            power_state: WifiPowerState::On,
            reset_state: WifiResetState::Deasserted,
            current_clock_hz: 400_000,
            preferred_data_clock_hz: 12_500_000,
            bus_width: SdioBusWidth::FourBit,
            card_ready: true,
            card_rca: 1,
            card_ocr: 0xb0ff_ff00,
            io_enable: Some(0x06),
            io_ready: Some(0x02),
            chipclkcsr: Some(0x3a),
            wakeupctrl: Some(0x02),
            sleepcsr: Some(0x01),
            cardcap: Some(0x08),
            programmed_backplane_window: None,
            shadow_backplane_window: None,
            shadow_backplane_fn_addr: None,
            control_plane_frame_recovery_stage: Some("control-plane-reply-speculative-read"),
            control_plane_frame_recovery_policy: Some("linux-rxfail"),
            control_plane_frame_recovery_write: Some(false),
            control_plane_frame_recovery_drained: Some(true),
            control_plane_frame_recovery_count: Some(0),
            control_plane_bootstrap_phase: "startup-link-passive-wait",
            control_plane_reply_mode: "startup-link",
            control_plane_reply_attempts: 0,
            control_plane_reply_empty_polls: 0,
            control_plane_no_ht_transport: true,
            control_plane_probe_pending: false,
            control_plane_startup_link_stable: true,
            control_plane_startup_profile_locked: true,
            control_plane_startup_profile_reason: "promoted-io-unstable",
            control_plane_promoted_probe_pending: false,
            debug_snapshot_source: "cached",
            debug_snapshot_stage: "cyw43-init-control-plane-fail",
            control_plane_startup_link_rescue_cycles: 1,
            control_plane_startup_link_rescue_limit: 2,
            control_plane_passive_startup_link_empty_poll_limit: 2,
            control_plane_f2_state: "latched-linux-configured-no-iorx",
            control_plane_sdhci_read_diag: "f2-reply-read-stalled-no-buffer-ready",
            control_plane_exact_error: "cyw43-function2-reply-read-stall-no-buffer-ready",
        };
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_contract_path(&snapshot),
            "startup-link-f2"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_contract_strict_recovery_f2(&snapshot),
            "preserve-latch"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_contract_blocker_class(&snapshot),
            "direct-f2-reply"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_terminal_action(&snapshot),
            "fail-fast"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_retry_clock_hz(&snapshot),
            12_500_000
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_passive_startup_link_reply_probe_reports_bulk_clock_and_preserve_latch() {
        let snapshot = WifiDebugSnapshot {
            power_state: WifiPowerState::On,
            reset_state: WifiResetState::Deasserted,
            current_clock_hz: 400_000,
            preferred_data_clock_hz: 12_500_000,
            bus_width: SdioBusWidth::FourBit,
            card_ready: true,
            card_rca: 1,
            card_ocr: 0xb0ff_ff00,
            io_enable: Some(0x06),
            io_ready: Some(0x02),
            chipclkcsr: Some(0x3a),
            wakeupctrl: Some(0x02),
            sleepcsr: Some(0x01),
            cardcap: Some(0x08),
            programmed_backplane_window: None,
            shadow_backplane_window: None,
            shadow_backplane_fn_addr: None,
            control_plane_frame_recovery_stage: Some("control-plane-reply-speculative-read"),
            control_plane_frame_recovery_policy: Some("linux-rxfail"),
            control_plane_frame_recovery_write: Some(false),
            control_plane_frame_recovery_drained: Some(true),
            control_plane_frame_recovery_count: Some(0),
            control_plane_bootstrap_phase: "startup-link-passive-wait",
            control_plane_reply_mode: "none",
            control_plane_reply_attempts: 0,
            control_plane_reply_empty_polls: 0,
            control_plane_no_ht_transport: true,
            control_plane_probe_pending: false,
            control_plane_startup_link_stable: true,
            control_plane_startup_profile_locked: true,
            control_plane_startup_profile_reason: "promoted-io-unstable",
            control_plane_promoted_probe_pending: false,
            debug_snapshot_source: "cached",
            debug_snapshot_stage: "cyw43-init-control-plane-fail",
            control_plane_startup_link_rescue_cycles: 1,
            control_plane_startup_link_rescue_limit: 2,
            control_plane_passive_startup_link_empty_poll_limit: 2,
            control_plane_f2_state: "latched-linux-configured-no-iorx",
            control_plane_sdhci_read_diag: "f2-reply-read-stalled-no-buffer-ready",
            control_plane_exact_error: "cyw43-function2-reply-read-stall-no-buffer-ready",
        };

        assert_eq!(
            KernelConsoleTestPump::wifi_reply_probe_lane(&snapshot),
            "passive-startup-link"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_probe_effective_clock_hz(&snapshot),
            12_500_000
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_contract_strict_recovery_f2(&snapshot),
            "preserve-latch"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_terminal_action(&snapshot),
            "fail-fast"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_passive_startup_link_sideband_blocker_reports_fail_fast() {
        let snapshot = WifiDebugSnapshot {
            power_state: WifiPowerState::On,
            reset_state: WifiResetState::Deasserted,
            current_clock_hz: 400_000,
            preferred_data_clock_hz: 12_500_000,
            bus_width: SdioBusWidth::FourBit,
            card_ready: true,
            card_rca: 1,
            card_ocr: 0xb0ff_ff00,
            io_enable: Some(0x06),
            io_ready: Some(0x02),
            chipclkcsr: Some(0x3a),
            wakeupctrl: Some(0x02),
            sleepcsr: Some(0x01),
            cardcap: Some(0x08),
            programmed_backplane_window: None,
            shadow_backplane_window: None,
            shadow_backplane_fn_addr: None,
            control_plane_frame_recovery_stage: Some("control-plane-reply-speculative-read"),
            control_plane_frame_recovery_policy: Some("linux-rxfail"),
            control_plane_frame_recovery_write: Some(false),
            control_plane_frame_recovery_drained: Some(true),
            control_plane_frame_recovery_count: Some(0),
            control_plane_bootstrap_phase: "startup-link-passive-wait",
            control_plane_reply_mode: "none",
            control_plane_reply_attempts: 0,
            control_plane_reply_empty_polls: 0,
            control_plane_no_ht_transport: true,
            control_plane_probe_pending: false,
            control_plane_startup_link_stable: true,
            control_plane_startup_profile_locked: true,
            control_plane_startup_profile_reason: "post-firmware-ready-function2-sideband",
            control_plane_promoted_probe_pending: false,
            debug_snapshot_source: "cached",
            debug_snapshot_stage: "cyw43-init-control-plane-fail",
            control_plane_startup_link_rescue_cycles: 1,
            control_plane_startup_link_rescue_limit: 2,
            control_plane_passive_startup_link_empty_poll_limit: 2,
            control_plane_f2_state: "latched-linux-configured-no-iorx",
            control_plane_sdhci_read_diag: "f1-reply-read-stalled-no-buffer-ready",
            control_plane_exact_error: "cyw43-control-plane-sideband-read-stall-no-buffer-ready",
        };

        assert_eq!(
            KernelConsoleTestPump::wifi_reply_contract_blocker_class(&snapshot),
            "f1-sideband"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_terminal_action(&snapshot),
            "fail-fast"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_startup_link_enable_latched_sideband_blocker_reports_fail_fast() {
        let snapshot = WifiDebugSnapshot {
            power_state: WifiPowerState::On,
            reset_state: WifiResetState::Deasserted,
            current_clock_hz: 400_000,
            preferred_data_clock_hz: 12_500_000,
            bus_width: SdioBusWidth::FourBit,
            card_ready: true,
            card_rca: 1,
            card_ocr: 0xb0ff_ff00,
            io_enable: Some(0x06),
            io_ready: Some(0x02),
            chipclkcsr: Some(0x3a),
            wakeupctrl: Some(0x02),
            sleepcsr: Some(0x01),
            cardcap: Some(0x08),
            programmed_backplane_window: None,
            shadow_backplane_window: None,
            shadow_backplane_fn_addr: None,
            control_plane_frame_recovery_stage: Some("control-plane-reply-speculative-read"),
            control_plane_frame_recovery_policy: Some("linux-rxfail"),
            control_plane_frame_recovery_write: Some(false),
            control_plane_frame_recovery_drained: Some(true),
            control_plane_frame_recovery_count: Some(0),
            control_plane_bootstrap_phase: "startup-link-recovery",
            control_plane_reply_mode: "startup-link-resume",
            control_plane_reply_attempts: 0,
            control_plane_reply_empty_polls: 0,
            control_plane_no_ht_transport: true,
            control_plane_probe_pending: false,
            control_plane_startup_link_stable: false,
            control_plane_startup_profile_locked: true,
            control_plane_startup_profile_reason: "promoted-io-unstable",
            control_plane_promoted_probe_pending: false,
            debug_snapshot_source: "cached",
            debug_snapshot_stage: "cyw43-init-control-plane-fail",
            control_plane_startup_link_rescue_cycles: 1,
            control_plane_startup_link_rescue_limit: 2,
            control_plane_passive_startup_link_empty_poll_limit: 2,
            control_plane_f2_state: "latched-linux-configured-no-iorx",
            control_plane_sdhci_read_diag: "f1-reply-read-stalled-no-buffer-ready",
            control_plane_exact_error:
                "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",
        };

        assert_eq!(
            KernelConsoleTestPump::wifi_reply_contract_strict_recovery_f2(&snapshot),
            "preserve-latch"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_contract_blocker_class(&snapshot),
            "f1-sideband"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_terminal_action(&snapshot),
            "fail-fast"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_capture_verdict_prefers_exact_error_over_card_not_ready() {
        let snapshot = WifiDebugSnapshot {
            power_state: WifiPowerState::Off,
            reset_state: WifiResetState::Asserted,
            current_clock_hz: 0,
            preferred_data_clock_hz: 12_500_000,
            bus_width: SdioBusWidth::OneBit,
            card_ready: false,
            card_rca: 0,
            card_ocr: 0,
            io_enable: None,
            io_ready: None,
            chipclkcsr: None,
            wakeupctrl: None,
            sleepcsr: None,
            cardcap: None,
            programmed_backplane_window: None,
            shadow_backplane_window: None,
            shadow_backplane_fn_addr: None,
            control_plane_frame_recovery_stage: None,
            control_plane_frame_recovery_policy: None,
            control_plane_frame_recovery_write: None,
            control_plane_frame_recovery_drained: None,
            control_plane_frame_recovery_count: None,
            control_plane_bootstrap_phase: "steady-state",
            control_plane_reply_mode: "none",
            control_plane_reply_attempts: 0,
            control_plane_reply_empty_polls: 0,
            control_plane_no_ht_transport: false,
            control_plane_probe_pending: false,
            control_plane_startup_link_stable: false,
            control_plane_startup_profile_locked: false,
            control_plane_startup_profile_reason: "none",
            control_plane_promoted_probe_pending: false,
            debug_snapshot_source: "live",
            debug_snapshot_stage: "console-dump-state",
            control_plane_startup_link_rescue_cycles: 0,
            control_plane_startup_link_rescue_limit: 2,
            control_plane_passive_startup_link_empty_poll_limit: 16,
            control_plane_f2_state: "unproven",
            control_plane_sdhci_read_diag: "f1-reply-read-command-timeout",
            control_plane_exact_error: "cyw43-control-plane-sideband-command-timeout",
        };
        assert_eq!(
            KernelConsoleTestPump::wifi_capture_verdict(&snapshot),
            ("function1-sideband-edge", "function1-sideband")
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_capture_verdict_keeps_sideband_read_stall_specific() {
        let snapshot = WifiDebugSnapshot {
            power_state: WifiPowerState::Off,
            reset_state: WifiResetState::Asserted,
            current_clock_hz: 0,
            preferred_data_clock_hz: 12_500_000,
            bus_width: SdioBusWidth::OneBit,
            card_ready: false,
            card_rca: 0,
            card_ocr: 0,
            io_enable: None,
            io_ready: None,
            chipclkcsr: None,
            wakeupctrl: None,
            sleepcsr: None,
            cardcap: None,
            programmed_backplane_window: None,
            shadow_backplane_window: None,
            shadow_backplane_fn_addr: None,
            control_plane_frame_recovery_stage: None,
            control_plane_frame_recovery_policy: None,
            control_plane_frame_recovery_write: None,
            control_plane_frame_recovery_drained: None,
            control_plane_frame_recovery_count: None,
            control_plane_bootstrap_phase: "startup-link-recovery",
            control_plane_reply_mode: "none",
            control_plane_reply_attempts: 0,
            control_plane_reply_empty_polls: 0,
            control_plane_no_ht_transport: true,
            control_plane_probe_pending: true,
            control_plane_startup_link_stable: false,
            control_plane_startup_profile_locked: true,
            control_plane_startup_profile_reason: "ht-not-ready",
            control_plane_promoted_probe_pending: false,
            debug_snapshot_source: "cached",
            debug_snapshot_stage: "control-plane-startup-link-rearm-stalled",
            control_plane_startup_link_rescue_cycles: 1,
            control_plane_startup_link_rescue_limit: 2,
            control_plane_passive_startup_link_empty_poll_limit: 8,
            control_plane_f2_state: "latched-linux-configured-no-iorx",
            control_plane_sdhci_read_diag: "f1-reply-read-stalled-no-buffer-ready",
            control_plane_exact_error:
                "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",
        };
        assert_eq!(
            KernelConsoleTestPump::wifi_capture_verdict(&snapshot),
            ("function2-reply-edge", "first-function2-reply")
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_golden_path_current_step(&snapshot),
            "first-function2-reply"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_function2_disabled_without_ht_reports_wait_ht_clock() {
        let mut fake = FakeWifiDebug::new();
        fake.snapshot.control_plane_exact_error = "cyw43-function2-disabled";
        fake.snapshot.control_plane_f2_state = "unproven";
        fake.snapshot.control_plane_bootstrap_phase = "steady-state";
        fake.snapshot.control_plane_no_ht_transport = false;
        fake.snapshot.control_plane_probe_pending = false;
        fake.snapshot.chipclkcsr = Some(0x50);
        fake.snapshot.io_enable = Some(0x02);
        fake.snapshot.io_ready = Some(0x02);

        assert_eq!(
            KernelConsoleTestPump::wifi_capture_verdict(&fake.snapshot),
            ("transport-edge", "wait-ht-clock")
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_golden_path_current_step(&fake.snapshot),
            "wait-ht-clock"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_contract_expected(&fake.snapshot),
            "chipclkcsr-ht-avail"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_join_security_exact_error_reports_join_gate() {
        let mut fake = FakeWifiDebug::new();
        fake.snapshot.debug_snapshot_stage = "cyw43-init-control-plane-fail";
        fake.snapshot.control_plane_exact_error = "cyw43-join-security-wpa-auth-initial-loop";
        fake.snapshot.control_plane_sdhci_read_diag = "f1-reply-read-command-error";
        fake.snapshot.control_plane_f2_state = "linux-configured";
        fake.snapshot.control_plane_bootstrap_phase = "steady-state";
        fake.snapshot.control_plane_no_ht_transport = false;
        fake.snapshot.control_plane_probe_pending = false;
        fake.snapshot.io_enable = Some(0x06);
        fake.snapshot.io_ready = Some(0x06);

        assert_eq!(
            KernelConsoleTestPump::wifi_capture_verdict(&fake.snapshot),
            ("join-security-edge", "join-security")
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_golden_path_current_step(&fake.snapshot),
            "join-security"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_golden_path_next_step(&fake.snapshot),
            "join-submit"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_contract_path(&fake.snapshot),
            "join-security"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_contract_blocker_class(&fake.snapshot),
            "join-security"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_contract_expected(&fake.snapshot),
            "linux-wpa2-security-order"
        );
        assert!(KernelConsoleTestPump::wifi_diag_should_skip_ht_probe(
            &fake.snapshot
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_firmware_supplicant_unsupported_reports_join_gate() {
        let mut fake = FakeWifiDebug::new();
        fake.snapshot.debug_snapshot_stage = "cyw43-init-control-plane-fail";
        fake.snapshot.control_plane_exact_error = "firmware-supplicant-unsupported";
        fake.snapshot.control_plane_sdhci_read_diag = "none";
        fake.snapshot.control_plane_f2_state = "linux-configured";
        fake.snapshot.control_plane_bootstrap_phase = "steady-state";
        fake.snapshot.control_plane_no_ht_transport = false;
        fake.snapshot.control_plane_probe_pending = false;
        fake.snapshot.io_enable = Some(0x06);
        fake.snapshot.io_ready = Some(0x06);

        assert_eq!(
            KernelConsoleTestPump::wifi_capture_verdict(&fake.snapshot),
            ("join-security-edge", "join-security")
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_golden_path_current_step(&fake.snapshot),
            "join-security"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_contract_path(&fake.snapshot),
            "join-security"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_contract_blocker_class(&fake.snapshot),
            "join-security"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_attribution(&fake.snapshot),
            "firmware-feature-boundary"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_transport_label(&fake.snapshot),
            "healthy"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_failing_iovar(&fake.snapshot),
            "sup_wpa,bsscfg:sup_wpa"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_status(&fake.snapshot),
            "0xffffffe9"
        );
        assert!(KernelConsoleTestPump::wifi_diag_should_skip_ht_probe(
            &fake.snapshot
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_host_eapol_required_reports_join_gate() {
        let mut fake = FakeWifiDebug::new();
        fake.snapshot.debug_snapshot_stage = "cyw43-init-control-plane-fail";
        fake.snapshot.control_plane_exact_error = "host-eapol-required";
        fake.snapshot.control_plane_sdhci_read_diag = "none";
        fake.snapshot.control_plane_f2_state = "linux-configured";
        fake.snapshot.control_plane_bootstrap_phase = "steady-state";
        fake.snapshot.io_enable = Some(0x06);
        fake.snapshot.io_ready = Some(0x06);

        assert_eq!(
            KernelConsoleTestPump::wifi_capture_verdict(&fake.snapshot),
            ("join-security-edge", "join-security")
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_attribution(&fake.snapshot),
            "host-supplicant-boundary"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_failing_iovar(&fake.snapshot),
            "eapol"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_status(&fake.snapshot),
            "host-required"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_host_eapol_pending_reports_join_gate() {
        let mut fake = FakeWifiDebug::new();
        fake.snapshot.debug_snapshot_stage = "cyw43-init-control-plane-fail";
        fake.snapshot.control_plane_exact_error = "wifi-host-eapol-pending";
        fake.snapshot.control_plane_sdhci_read_diag = "none";
        fake.snapshot.control_plane_f2_state = "linux-configured";
        fake.snapshot.control_plane_bootstrap_phase = "steady-state";
        fake.snapshot.io_enable = Some(0x06);
        fake.snapshot.io_ready = Some(0x06);

        assert_eq!(
            KernelConsoleTestPump::wifi_capture_verdict(&fake.snapshot),
            ("join-security-edge", "join-security")
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_attribution(&fake.snapshot),
            "host-supplicant-boundary"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_failing_iovar(&fake.snapshot),
            "eapol"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_status(&fake.snapshot),
            "host-pending"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_startup_gate_requires_explicit_acceptance_after_dhcp() {
        assert_eq!(
            KernelConsoleTestPump::wifi_startup_proof_gate(
                true, true, true, true, true, true, true, true, true, false,
            ),
            9
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_startup_proof_gate(
                true, true, true, true, true, true, true, true, true, true,
            ),
            10
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_wsec_pmk_bad_argument_reports_join_gate() {
        let mut fake = FakeWifiDebug::new();
        fake.snapshot.debug_snapshot_stage = "cyw43-init-control-plane-fail";
        fake.snapshot.control_plane_exact_error = "wsec-pmk-bad-argument";
        fake.snapshot.control_plane_sdhci_read_diag = "none";
        fake.snapshot.control_plane_f2_state = "linux-configured";
        fake.snapshot.control_plane_bootstrap_phase = "steady-state";
        fake.snapshot.io_enable = Some(0x06);
        fake.snapshot.io_ready = Some(0x06);

        assert_eq!(
            KernelConsoleTestPump::wifi_capture_verdict(&fake.snapshot),
            ("join-security-edge", "join-security")
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_golden_path_current_step(&fake.snapshot),
            "join-security"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_attribution(&fake.snapshot),
            "join-credential-boundary"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_transport_label(&fake.snapshot),
            "healthy"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_failing_iovar(&fake.snapshot),
            "WLC_SET_WSEC_PMK"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_join_security_status(&fake.snapshot),
            "0xfffffffe"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_direct_sdio_cmd53_reports_firmware_core_control() {
        let mut fake = FakeWifiDebug::new();
        fake.snapshot.debug_snapshot_stage = "cyw43-load-firmware-fail";
        fake.snapshot.control_plane_exact_error = "sdio-cmd53-r5-error";
        fake.snapshot.control_plane_f2_state = "unproven";
        fake.snapshot.control_plane_sdhci_read_diag = "sdio-cmd53-r5-error";

        assert_eq!(
            KernelConsoleTestPump::wifi_capture_verdict(&fake.snapshot),
            ("firmware-core-control-edge", "firmware-core-control")
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_golden_path_current_step(&fake.snapshot),
            "firmware-core-control"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_reply_contract_blocker_class(&fake.snapshot),
            "firmware-core-control"
        );
        assert!(KernelConsoleTestPump::wifi_diag_should_skip_ht_probe(
            &fake.snapshot
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_diag_skips_ht_probe_when_transport_is_not_initialized() {
        let mut fake = FakeWifiDebug::new();
        fake.snapshot.power_state = WifiPowerState::Off;
        fake.snapshot.reset_state = WifiResetState::Asserted;
        fake.snapshot.current_clock_hz = 0;
        fake.snapshot.card_ready = false;

        assert_eq!(
            KernelConsoleTestPump::wifi_diag_ht_probe_skip_reason(&fake.snapshot),
            Some("transport-not-initialized")
        );
        assert!(KernelConsoleTestPump::wifi_diag_should_skip_ht_probe(
            &fake.snapshot
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_diag_skips_ht_probe_for_preserved_control_plane_failure() {
        let mut fake = FakeWifiDebug::new();
        fake.snapshot.current_clock_hz = 41_666_666;
        fake.snapshot.debug_snapshot_stage = "console-diag-before";
        fake.snapshot.control_plane_exact_error = "cyw43-control-plane-partial-hint-visibility";

        assert_eq!(
            KernelConsoleTestPump::wifi_diag_ht_probe_skip_reason(&fake.snapshot),
            Some("preserved-control-plane-failure")
        );
        assert!(KernelConsoleTestPump::wifi_diag_should_skip_ht_probe(
            &fake.snapshot
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_capture_verdict_falls_back_to_first_reply_when_exact_error_is_blank() {
        let snapshot = WifiDebugSnapshot {
            power_state: WifiPowerState::On,
            reset_state: WifiResetState::Deasserted,
            current_clock_hz: 400_000,
            preferred_data_clock_hz: 12_500_000,
            bus_width: SdioBusWidth::FourBit,
            card_ready: true,
            card_rca: 1,
            card_ocr: 0xb0ff_ff00,
            io_enable: Some(0x06),
            io_ready: Some(0x02),
            chipclkcsr: Some(0x3a),
            wakeupctrl: Some(0x02),
            sleepcsr: Some(0x01),
            cardcap: Some(0x08),
            programmed_backplane_window: Some(0x1800_0000),
            shadow_backplane_window: Some(0x1800_0000),
            shadow_backplane_fn_addr: Some(0x08000),
            control_plane_frame_recovery_stage: None,
            control_plane_frame_recovery_policy: None,
            control_plane_frame_recovery_write: None,
            control_plane_frame_recovery_drained: None,
            control_plane_frame_recovery_count: None,
            control_plane_bootstrap_phase: "startup-link-recovery",
            control_plane_reply_mode: "startup-link-resume",
            control_plane_reply_attempts: 2,
            control_plane_reply_empty_polls: 0,
            control_plane_no_ht_transport: true,
            control_plane_probe_pending: false,
            control_plane_startup_link_stable: false,
            control_plane_startup_profile_locked: true,
            control_plane_startup_profile_reason: "",
            control_plane_promoted_probe_pending: false,
            debug_snapshot_source: "cached",
            debug_snapshot_stage: "control-plane-startup-link-rearm-stalled",
            control_plane_startup_link_rescue_cycles: 2,
            control_plane_startup_link_rescue_limit: 2,
            control_plane_passive_startup_link_empty_poll_limit: 4,
            control_plane_f2_state: "latched-linux-configured-no-iorx",
            control_plane_sdhci_read_diag: "f1-reply-read-stalled-no-buffer-ready",
            control_plane_exact_error: "",
        };
        assert_eq!(
            KernelConsoleTestPump::wifi_capture_verdict(&snapshot),
            ("function2-reply-edge", "first-function2-reply")
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_golden_path_step_prefers_function1_sideband_edge() {
        let snapshot = WifiDebugSnapshot {
            power_state: WifiPowerState::Off,
            reset_state: WifiResetState::Asserted,
            current_clock_hz: 0,
            preferred_data_clock_hz: 12_500_000,
            bus_width: SdioBusWidth::OneBit,
            card_ready: false,
            card_rca: 0,
            card_ocr: 0,
            io_enable: None,
            io_ready: None,
            chipclkcsr: None,
            wakeupctrl: None,
            sleepcsr: None,
            cardcap: None,
            programmed_backplane_window: None,
            shadow_backplane_window: None,
            shadow_backplane_fn_addr: None,
            control_plane_frame_recovery_stage: None,
            control_plane_frame_recovery_policy: None,
            control_plane_frame_recovery_write: None,
            control_plane_frame_recovery_drained: None,
            control_plane_frame_recovery_count: None,
            control_plane_bootstrap_phase: "steady-state",
            control_plane_reply_mode: "none",
            control_plane_reply_attempts: 0,
            control_plane_reply_empty_polls: 0,
            control_plane_no_ht_transport: false,
            control_plane_probe_pending: false,
            control_plane_startup_link_stable: false,
            control_plane_startup_profile_locked: false,
            control_plane_startup_profile_reason: "none",
            control_plane_promoted_probe_pending: false,
            debug_snapshot_source: "live",
            debug_snapshot_stage: "console-dump-state",
            control_plane_startup_link_rescue_cycles: 0,
            control_plane_startup_link_rescue_limit: 2,
            control_plane_passive_startup_link_empty_poll_limit: 16,
            control_plane_f2_state: "unproven",
            control_plane_sdhci_read_diag: "f1-reply-read-command-timeout",
            control_plane_exact_error: "cyw43-control-plane-sideband-command-timeout",
        };
        assert_eq!(
            KernelConsoleTestPump::wifi_golden_path_current_step(&snapshot),
            "function1-sideband"
        );
        assert_eq!(
            KernelConsoleTestPump::wifi_golden_path_next_step(&snapshot),
            "first-function2-reply"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn usb_capture_verdict_classifies_run_transition_edge() {
        assert_eq!(
            KernelConsoleTestPump::usb_capture_verdict(true, true, Some("usbcmd-run-store-wedged"),),
            ("run-transition-edge", "usbcmd-run")
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn local_seat_wifi_debug_command_mirrors_output() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut local_seat = LocalSeatRuntime::new(crate::local_seat::LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 128,
            buffer_lines: 128,
        });
        local_seat.mark_root_console_ready();
        let mut wifi = FakeWifiDebug::new();
        local_seat.enqueue_keyboard_bytes(b"wifi probe-ht\n");

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_wifi_debug(&mut wifi)
            .with_local_seat(&mut local_seat)
            .with_test_pi4_debug_commands();
        pump.session = Some(SessionRole::Queen);
        pump.poll();
        drop(pump);

        let mirrored = local_seat.mirrored_lines_snapshot();
        assert!(
            mirrored.iter().any(|line| line.contains(
                "wifi: debug subcommand=probe-ht action=begin profile=bounded mode=one-shot"
            )),
            "{mirrored:?}"
        );
        assert!(
            mirrored
                .iter()
                .any(|line| line.contains("wifi ht: ready=yes")),
            "{mirrored:?}"
        );
        assert!(mirrored.iter().any(|line| line.contains(
            "wifi: debug subcommand=probe-ht action=complete profile=bounded mode=one-shot result=ok"
        )), "{mirrored:?}");
        assert!(
            mirrored
                .iter()
                .any(|line| line.contains("OK WIFI detail=subcommand=probe-ht")),
            "{mirrored:?}"
        );
        assert_eq!(wifi.calls.as_slice(), &["probe-ht", "dump-state"]);
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn serial_wifi_debug_command_reports_wifi_network_phase() {
        let driver = LoopbackSerial::<8192>::new();
        let serial = SerialPort::<_, 8192, 8192, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut wifi = FakeWifiDebug::new();
        let mut net = FakeNet::new();
        net.status = NetStatusReport {
            backend: "bcmgenet-v5",
            mode: "dhcp",
            interface_policy: "wifi",
            active_interface: "wifi",
            standby_interface: "wired",
            address_source: "wifi-associating",
            ip: HeaplessString::new(),
            gateway: HeaplessString::new(),
            dhcp_phase: "associating",
        };
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_wifi_debug(&mut wifi)
            .with_network(&mut net)
            .with_test_pi4_debug_commands();

        pump.serial_mut().driver_mut().push_rx(b"wifi dump-state\n");
        pump.poll();
        pump.poll();

        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        drop(pump);
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains(
                "wifi: net backend=bcmgenet-v5 mode=dhcp policy=wifi active=wifi standby=wired address_source=wifi-associating dhcp_phase=associating"
            ),
            "{rendered}"
        );
    }

    #[cfg(all(feature = "kernel", feature = "net-console"))]
    #[test]
    fn network_wifi_debug_command_stays_outside_shared_console_grammar() {
        let driver = LoopbackSerial::<256>::new();
        let serial = SerialPort::<_, 256, 256, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let store: TicketTable<4> = TicketTable::new();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        let mut line = HeaplessString::new();
        line.push_str("wifi dump-state").unwrap();
        net.lines.push(ConsoleLine::new(line, 1)).unwrap();
        let mut wifi = FakeWifiDebug::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_wifi_debug(&mut wifi)
            .with_network(&mut net);

        pump.poll();
        drop(pump);

        let rendered = net
            .sent
            .iter()
            .map(|line| line.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("ERR PARSE"), "{rendered}");
        assert!(wifi.calls.is_empty());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn forwards_commands_to_ninedoor_bridge() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 5 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "secret").unwrap();
        let mut audit = AuditLog::new();
        let mut bridge = NineDoorBridge::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_ninedoor(&mut bridge);

        pump.session = Some(SessionRole::Queen);
        pump.handle_command(Command::Log)
            .expect("forward log to NineDoor");

        assert!(audit
            .entries
            .iter()
            .any(|entry| entry.contains("nine-door: log stream requested")));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn ls_command_emits_directory_entries() {
        let _root_guard = ReachableRootGuard::new(5);
        let driver = LoopbackSerial::<512>::new();
        let serial = SerialPort::<_, 512, 512, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 5 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "secret").unwrap();
        let mut audit = AuditLog::new();
        let mut bridge = NineDoorBridge::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_ninedoor(&mut bridge);

        pump.session = Some(SessionRole::Queen);
        let mut path = HeaplessString::new();
        path.push_str("/log").unwrap();
        pump.handle_command(Command::Ls { path })
            .expect("ls command should succeed");

        pump.serial_mut().poll_io();
        let transcript: Vec<u8> = pump
            .serial_mut()
            .driver_mut()
            .drain_tx()
            .into_iter()
            .collect();
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(rendered.contains("OK LS"), "{rendered}");
        assert!(rendered.contains("queen.log"), "{rendered}");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn error_when_forwarding_without_ninedoor() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 5 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "secret").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);

        pump.session = Some(SessionRole::Queen);
        let result = pump.handle_command(Command::Log);

        let err = result.expect_err("missing NineDoor bridge should fail forwarding");
        match &err {
            CommandDispatchError::NineDoorUnavailable { verb } => {
                assert_eq!(verb.ack_label(), "LOG");
            }
            other => panic!("unexpected result: {other:?}"),
        }
        pump.handle_dispatch_error(err);

        assert!(audit
            .denials
            .iter()
            .any(|entry| entry.contains("ninedoor unavailable")));
    }
}
