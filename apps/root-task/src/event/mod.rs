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
use crate::console::{Command, CommandParser, ConsoleError, MAX_ROLE_LEN, MAX_TICKET_LEN};
#[cfg(feature = "kernel")]
use crate::debug_uart::debug_uart_str;
#[cfg(feature = "kernel")]
use crate::hal::{SdioBusWidth, WifiDebugOps, WifiDebugSnapshot, WifiPowerState, WifiResetState};
use crate::local_seat::{LocalSeatRuntime, KEYBOARD_POLL_CHUNK_BYTES};
#[cfg(feature = "kernel")]
use crate::log_buffer;
#[cfg(feature = "net-console")]
use crate::net::NetSelfTestStartResult;
#[cfg(feature = "net-console")]
use crate::net::{
    ConsoleLine, NetConsoleDisconnectReason, NetConsoleEvent, NetDiagSnapshot, NetPoller,
    NetTelemetry, CONSOLE_QUEUE_DEPTH, NET_DIAG, NET_DIAG_FEATURED,
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

fn format_message(args: fmt::Arguments<'_>) -> HeaplessString<128> {
    let mut buf = HeaplessString::new();
    if FmtWrite::write_fmt(&mut buf, args).is_err() {
        // Truncated diagnostic; best-effort only.
    }
    buf
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

const CONSOLE_BANNER: &str = "[Cohesix] Root console ready (type 'help' for commands)";
const CONSOLE_PROMPT: &str = "cohesix> ";
const QUEEN_CTL_PATH: &str = "/queen/ctl";
#[cfg(feature = "kernel")]
const WIFI_DEBUG_ACK_LABEL: &str = "WIFI";
#[cfg(feature = "kernel")]
const USB_DEBUG_ACK_LABEL: &str = "USB";
#[cfg(feature = "net-console")]
const NET_DIAG_RATE_LIMIT_MS: u64 = 15_000;
#[cfg(feature = "net-console")]
const NET_DIAG_RATE_KINDS: usize = 1;
#[cfg(feature = "net-console")]
const NET_DIAG_STUCK_MS: u64 = 3_000;

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
        if self.entries.is_full() {
            return Err(TicketRegistryError::Capacity);
        }
        self.entries
            .push(TicketRecord {
                role,
                key: cohesix_ticket::TicketKey::from_secret(secret),
            })
            .map_err(|_| TicketRegistryError::Capacity)
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
    #[cfg(feature = "kernel")]
    /// Bootstrap IPC messages processed.
    pub bootstrap_messages: u64,
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
    Net,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WifiDebugCommand {
    Help,
    DumpState,
    ProbeHt,
    LoadFirmware,
    Retry,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UsbDebugCommand {
    Help,
    Status,
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
    lines: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, { log_buffer::LOG_SNAPSHOT_LINES }>,
    next_line: usize,
    bandwidth_bytes: u64,
    cursor: Option<PendingCursor>,
}

#[cfg(feature = "kernel")]
impl PendingStream {
    fn new() -> Self {
        Self {
            lines: HeaplessVec::new(),
            next_line: 0,
            bandwidth_bytes: 0,
            cursor: None,
        }
    }

    fn reset(&mut self) {
        self.lines.clear();
        self.next_line = 0;
        self.bandwidth_bytes = 0;
        self.cursor = None;
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
    #[cfg(feature = "kernel")]
    ninedoor: Option<&'a mut NineDoorBridge>,
    #[cfg(feature = "kernel")]
    bootstrap_handler: Option<&'a mut dyn BootstrapMessageHandler>,
    #[cfg(feature = "kernel")]
    console_context: Option<ConsoleContext>,
    #[cfg(feature = "kernel")]
    wifi_debug: Option<&'a mut dyn WifiDebugOps>,
    local_seat: Option<&'a mut LocalSeatRuntime>,
    banner_emitted: bool,
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
            #[cfg(feature = "kernel")]
            ninedoor: None,
            #[cfg(feature = "kernel")]
            bootstrap_handler: None,
            #[cfg(feature = "kernel")]
            console_context: None,
            #[cfg(feature = "kernel")]
            wifi_debug: None,
            local_seat: None,
            banner_emitted: false,
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
        self.local_seat = Some(runtime);
        self
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

    /// Execute a single cooperative polling cycle.
    pub fn poll(&mut self) {
        self.serial.poll_io();
        self.consume_serial();
        self.consume_local_seat();

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
        let net_poll = if let Some(net) = self.net.as_mut() {
            let activity = net.poll(self.now_ms);
            let telemetry = net.telemetry();
            let conn_id = net.active_console_conn_id();
            let mut buffered: HeaplessVec<ConsoleLine, { CONSOLE_QUEUE_DEPTH }> =
                HeaplessVec::new();
            net.drain_console_lines(self.now_ms, &mut |line| {
                let _ = buffered.push(line);
            });
            let ingest_snapshot: IngestSnapshot = net.ingest_snapshot();
            Some((activity, telemetry, buffered, conn_id, ingest_snapshot))
        } else {
            None
        };

        #[cfg(feature = "net-console")]
        if let Some((activity, telemetry, buffered, conn_id, _ingest_snapshot)) = net_poll {
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
            for line in buffered {
                self.handle_network_line(line.text);
            }
            #[cfg(feature = "kernel")]
            if let Some(bridge) = self.ninedoor.as_mut() {
                bridge.update_ingest_snapshot(_ingest_snapshot);
            }
            self.drain_net_console_events();
        }

        self.ipc.dispatch(self.now_ms);
        #[cfg(feature = "kernel")]
        self.drain_bootstrap_ipc();
        #[cfg(feature = "kernel")]
        self.flush_pending_stream();
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
        // Ignore rapidly changing TX bookkeeping during sustained link failure;
        // these counters can churn continuously without any meaningful traffic.
        prev.poll_calls = 0;
        curr.poll_calls = 0;
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
                    "NETDIAG in_bytes={} out_bytes={} tx_drops={} link={} q_lines={} q_bytes={} q_drops={} q_wblk={} suppressed={}",
                    snapshot.bytes_read,
                    snapshot.bytes_written,
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

    #[cfg(feature = "kernel")]
    /// Emit console audit messages once the UART bridge is connected.
    pub fn announce_console_ready(&mut self) {
        if self.ninedoor.is_some() {
            if crate::generated::hardware_config().local_seat.enabled {
                boot_log::force_uart_line(
                    "[trace] log channel remains UART during local-seat bring-up",
                );
            } else {
                boot_log::switch_logger_to_log_buffer();
            }
        }
        self.audit.info("console: attach uart");
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

    /// Emit the interactive banner and initial prompt over the serial console.
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
        self.emit_serial_line("Cohesix console ready");
        self.emit_help_serial_only();
        #[cfg(feature = "net-console")]
        if let Some(net) = self.net.as_mut() {
            let _ = net.send_console_line(
                "[net-console] authenticate using AUTH <role> <token> to receive console output",
            );
        }
        debug_uart_str("[dbg] console: writing 'cohesix>' prompt\n");
        self.emit_prompt();
        self.serial.poll_io();
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
                let source = match self.last_input_source {
                    ConsoleInputSource::Serial => "serial",
                    ConsoleInputSource::Net => "net",
                };
                let message = format_message(format_args!(
                    "audit console.emit.failed source={} line={}",
                    source, line
                ));
                crate::debug_uart::debug_uart_line(message.as_str());
            }
        }
    }

    fn try_emit_console_line(&mut self, line: &str) -> bool {
        if self.last_input_source == ConsoleInputSource::Serial {
            self.emit_serial_line(line);
            return true;
        }
        if let Some(runtime) = self.local_seat.as_mut() {
            runtime.mirror_line(line);
        }
        #[cfg(feature = "net-console")]
        if self.last_input_source == ConsoleInputSource::Net {
            if let Some(net) = self.net.as_mut() {
                return net.send_console_line(line);
            }
        }
        false
    }

    fn emit_serial_line(&mut self, line: &str) {
        if let Some(runtime) = self.local_seat.as_mut() {
            runtime.mirror_line(line);
        }
        self.serial.enqueue_tx(line.as_bytes());
        self.serial.enqueue_tx(b"\r\n");
    }

    fn emit_prompt(&mut self) {
        if let Some(runtime) = self.local_seat.as_mut() {
            runtime.mirror_line(CONSOLE_PROMPT);
        }
        self.serial.enqueue_tx(CONSOLE_PROMPT.as_bytes());
    }

    fn emit_help(&mut self) {
        self.emit_console_line("Commands:");
        self.emit_console_line("  help  - Show this help");
        self.emit_console_line("  bi    - Show bootinfo summary");
        self.emit_console_line("  caps  - Show capability slots");
        self.emit_console_line("  smp   - Show SMP scheduler/CPU info (debug builds only)");
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
        self.emit_serial_line("  smp   - Show SMP scheduler/CPU info (debug builds only)");
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
        if self.local_seat.is_none() || self.last_input_source == ConsoleInputSource::Net {
            return;
        }
        let line = "  usb <help|status|enable-kbd|probe-kbd> - USB local-seat diagnostics (serial/local only)";
        if serial_only {
            self.emit_serial_line(line);
        } else {
            self.emit_console_line(line);
        }
    }

    #[cfg(feature = "kernel")]
    fn emit_wifi_debug_help(&mut self, serial_only: bool) {
        if self.wifi_debug.is_none() || self.last_input_source == ConsoleInputSource::Net {
            return;
        }
        let line = "  wifi <help|dump-state|probe-ht|load-fw|retry> - WiFi bring-up diagnostics (serial/local only)";
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

    #[allow(unsafe_code)]
    #[cfg(all(feature = "kernel", sel4_config_debug_build))]
    fn emit_smp(&mut self) -> bool {
        self.emit_console_line("[smp] debug scheduler dump begin");
        let policy = affinity::policy();
        affinity::debug_dump_per_core(&policy, |line| self.emit_console_line(line));
        self.emit_console_line("[smp] debug scheduler dump end");
        true
    }

    #[cfg(not(all(feature = "kernel", sel4_config_debug_build)))]
    fn emit_smp(&mut self) -> bool {
        self.emit_console_line("ERR reason=unsupported");
        false
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

        let command = match parts.next() {
            None => WifiDebugCommand::Help,
            Some(subcommand) if subcommand.eq_ignore_ascii_case("help") => WifiDebugCommand::Help,
            Some(subcommand) if subcommand.eq_ignore_ascii_case("dump-state") => {
                WifiDebugCommand::DumpState
            }
            Some(subcommand) if subcommand.eq_ignore_ascii_case("probe-ht") => {
                WifiDebugCommand::ProbeHt
            }
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

        let command = match parts.next() {
            None => UsbDebugCommand::Help,
            Some(subcommand) if subcommand.eq_ignore_ascii_case("help") => UsbDebugCommand::Help,
            Some(subcommand) if subcommand.eq_ignore_ascii_case("status") => {
                UsbDebugCommand::Status
            }
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
            WifiDebugCommand::LoadFirmware => "load-fw",
            WifiDebugCommand::Retry => "retry",
        };
        let profile = Self::wifi_debug_command_profile(command);

        if matches!(command, WifiDebugCommand::Help) {
            self.emit_console_line("WiFi debug commands:");
            self.emit_console_line("  wifi dump-state - Show cached SDIO and clock state");
            self.emit_console_line("  wifi probe-ht   - Probe HT clock readiness without reboot");
            self.emit_console_line(
                "  wifi load-fw    - Retry firmware load from current transport",
            );
            self.emit_console_line("  wifi retry      - Rebuild transport, then reload firmware");
            self.metrics.accepted_commands = self.metrics.accepted_commands.saturating_add(1);
            self.emit_ack_ok(
                WIFI_DEBUG_ACK_LABEL,
                Some("detail=subcommand=help scope=serial-local"),
            );
            return;
        }

        self.emit_wifi_debug_status(subcommand, "begin", profile, None);
        let result = match command {
            WifiDebugCommand::Help => unreachable!(),
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
                    self.emit_wifi_snapshot(&snapshot);
                }
                self.emit_wifi_debug_status(subcommand, "complete", profile, Some("result=ok"));
                self.metrics.accepted_commands = self.metrics.accepted_commands.saturating_add(1);
                let detail = format_message(format_args!(
                    "detail=subcommand={subcommand} scope=serial-local"
                ));
                self.emit_ack_ok(WIFI_DEBUG_ACK_LABEL, Some(detail.as_str()));
            }
            Err(err) => {
                let error_snapshot_stage = match command {
                    WifiDebugCommand::Help => None,
                    WifiDebugCommand::DumpState => None,
                    WifiDebugCommand::ProbeHt => Some("console-probe-ht-error"),
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
            UsbDebugCommand::EnableKeyboard => "enable-kbd",
            UsbDebugCommand::ProbeKeyboard => "probe-kbd",
        };

        if matches!(command, UsbDebugCommand::Help) {
            self.emit_console_line("USB local-seat debug commands:");
            self.emit_console_line(
                "  usb status      - Show local-seat runtime attach and polling state",
            );
            self.emit_console_line(
                "  usb enable-kbd  - Arm runtime USB keyboard probing after boot",
            );
            self.emit_console_line(
                "  usb probe-kbd   - Arm and immediately run one keyboard probe pass",
            );
            self.metrics.accepted_commands = self.metrics.accepted_commands.saturating_add(1);
            self.emit_ack_ok(
                USB_DEBUG_ACK_LABEL,
                Some("detail=subcommand=help scope=serial-local"),
            );
            return;
        }

        if self.local_seat.is_none() {
            self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
            self.emit_refusal(
                USB_DEBUG_ACK_LABEL,
                RefusalReason::Policy,
                Some("detail=local-seat-unavailable"),
            );
            return;
        }
        match command {
            UsbDebugCommand::Help => unreachable!(),
            UsbDebugCommand::Status => {
                let (backend_attached, polling_enabled) = {
                    let local_seat = match self.local_seat.as_mut() {
                        Some(local_seat) => local_seat,
                        None => unreachable!("local-seat checked present before USB debug status"),
                    };
                    (
                        local_seat.backend_attached(),
                        local_seat.backend_keyboard_polling_enabled(),
                    )
                };
                self.emit_usb_status(backend_attached, polling_enabled, None);
            }
            UsbDebugCommand::EnableKeyboard => {
                let (backend_attached, polling_enabled) = {
                    let local_seat = match self.local_seat.as_mut() {
                        Some(local_seat) => local_seat,
                        None => unreachable!(
                            "local-seat checked present before USB keyboard poll enable"
                        ),
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
                if let Some(preflight) = self
                    .local_seat
                    .as_ref()
                    .and_then(|local_seat| local_seat.backend_keyboard_probe_preflight_status())
                {
                    self.emit_usb_probe_preflight(preflight);
                }
                let (backend_attached, polling_enabled) = {
                    let local_seat = match self.local_seat.as_mut() {
                        Some(local_seat) => local_seat,
                        None => {
                            unreachable!("local-seat checked present before USB keyboard probe")
                        }
                    };
                    local_seat.probe_backend_keyboard_once();
                    (
                        local_seat.backend_attached(),
                        local_seat.backend_keyboard_polling_enabled(),
                    )
                };
                self.emit_usb_status(
                    backend_attached,
                    polling_enabled,
                    Some("action=keyboard-probe-complete"),
                );
            }
        }

        self.metrics.accepted_commands = self.metrics.accepted_commands.saturating_add(1);
        let detail = format_message(format_args!(
            "detail=subcommand={subcommand} scope=serial-local"
        ));
        self.emit_ack_ok(USB_DEBUG_ACK_LABEL, Some(detail.as_str()));
    }

    #[cfg(feature = "kernel")]
    fn emit_usb_probe_preflight(&mut self, status: crate::local_seat_pi4::UsbProbePreflightStatus) {
        let route_line = format_message(format_args!(
            "usb: golden_path preflight route={} attempt={}/{} current={} next={} origin={} handoff={} seed={} halt_guard={}",
            status.route,
            status.strategy_idx,
            status.strategy_count,
            status.current_step,
            status.next_step,
            status.origin,
            status.handoff,
            status.seed,
            status.halt_guard,
        ));
        self.emit_console_line(route_line.as_str());
        let mut edge_line = format_message(format_args!(
            "usb: golden_path preflight policy={} dma={} bus={} poll_only={} followup={} expected_diag=0x{:04x}",
            status.policy,
            if status.prefer_high { "high" } else { "low" },
            if status.pcie_dma_window {
                "pcie-window"
            } else {
                "phys"
            },
            if status.poll_only { "yes" } else { "no" },
            status.followup_step,
            status.expected_diag_stage,
        ));
        if let Some(tag) = status.expected_diag_tag {
            let _ = write!(edge_line, " expected_tag={tag}");
        }
        if let Some(exact) = status.expected_diag_exact {
            let _ = write!(edge_line, " expected_exact={exact}");
        }
        self.emit_console_line(edge_line.as_str());
        let plan_line = format_message(format_args!(
            "usb: golden_path preflight ctor={} pre={} legacy={} run={} publish={} post_ready={}",
            status.constructor,
            status.pre_reset,
            status.legacy,
            status.run,
            status.publish,
            status.post_ready_irq,
        ));
        self.emit_console_line(plan_line.as_str());
    }

    #[cfg(feature = "kernel")]
    fn emit_usb_status(
        &mut self,
        backend_attached: bool,
        polling_enabled: bool,
        action_detail: Option<&str>,
    ) {
        let mut line = format_message(format_args!(
            "usb: local-seat attached={} polling={}",
            if backend_attached { "yes" } else { "no" },
            if polling_enabled {
                "enabled"
            } else {
                "deferred"
            },
        ));
        if let Some(action_detail) = action_detail {
            let _ = write!(line, " {action_detail}");
        }
        self.emit_console_line(line.as_str());
        let diag = crate::local_seat_pi4::latest_xhci_diag_status();
        let (verdict, focus) = Self::usb_capture_verdict(
            backend_attached,
            polling_enabled,
            diag.as_ref().and_then(|status| status.exact_issue),
        );
        if let Some(route) = crate::local_seat_pi4::latest_usb_probe_route_status() {
            let route_line = format_message(format_args!(
                "usb: golden_path route={} attempt={}/{} current={} next={} origin={} handoff={} seed={} halt_guard={}",
                route.route,
                route.strategy_idx + 1,
                route.strategy_count,
                route.current_step,
                route.next_step,
                route.origin,
                route.handoff,
                route.seed,
                route.halt_guard,
            ));
            self.emit_console_line(route_line.as_str());
            let mut progress_line = format_message(format_args!(
                "usb: golden_path outcome={} progress={} policy={} dma={} bus={} poll_only={} connected_mask=0x{:04x} detect_passes={}",
                route.outcome,
                route.progress,
                route.policy,
                if route.prefer_high { "high" } else { "low" },
                if route.pcie_dma_window {
                    "pcie-window"
                } else {
                    "phys"
                },
                if route.poll_only { "yes" } else { "no" },
                route.connected_mask,
                route.detect_passes,
            ));
            if let Some(port) = route.port {
                let _ = write!(progress_line, " port={port}");
            }
            if route.slow_recheck {
                let _ = write!(progress_line, " slow_recheck=yes");
            }
            if let Some(stage) = route.diag_stage {
                let _ = write!(progress_line, " diag_stage=0x{stage:04x}");
                if let Some(tag) = route.diag_tag {
                    let _ = write!(progress_line, " diag_tag={tag}");
                }
                if let Some(exact) = route.diag_exact {
                    let _ = write!(progress_line, " diag_exact={exact}");
                }
            }
            self.emit_console_line(progress_line.as_str());
        }
        let verdict_line = format_message(format_args!("usb: verdict={verdict} focus={focus}"));
        self.emit_console_line(verdict_line.as_str());
        if let Some(diag) = diag {
            let mut diag_line =
                format_message(format_args!("usb: xhci stage=0x{:04x}", diag.stage));
            if let Some(tag) = diag.tag {
                let _ = write!(diag_line, " tag={tag}");
            }
            if let Some(exact_issue) = diag.exact_issue {
                let _ = write!(diag_line, " exact={exact_issue}");
            }
            self.emit_console_line(diag_line.as_str());
            let mut values_line = format_message(format_args!("usb: xhci values"));
            if let Some((a_label, b_label, c_label)) = diag.value_labels {
                let _ = write!(
                    values_line,
                    " {a_label}=0x{:016x} {b_label}=0x{:016x} {c_label}=0x{:016x}",
                    diag.a, diag.b, diag.c
                );
            } else {
                let _ = write!(
                    values_line,
                    " a=0x{:016x} b=0x{:016x} c=0x{:016x}",
                    diag.a, diag.b, diag.c
                );
            }
            self.emit_console_line(values_line.as_str());
        }
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
    const fn wifi_debug_command_profile(command: WifiDebugCommand) -> &'static str {
        match command {
            WifiDebugCommand::Help => "help",
            WifiDebugCommand::DumpState | WifiDebugCommand::ProbeHt => "bounded",
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
            "wifi: f2_recover stage={} policy={} op={} drained={} count={}",
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

        let control = format_message(format_args!(
            "wifi: f2_state={} exact_error={} sdhci_read_diag={}",
            snapshot.control_plane_f2_state,
            snapshot.control_plane_exact_error,
            snapshot.control_plane_sdhci_read_diag,
        ));
        self.emit_console_line(control.as_str());

        let verdict_line = format_message(format_args!(
            "wifi: verdict={verdict} focus={focus} bootstrap={}",
            snapshot.control_plane_bootstrap_phase,
        ));
        self.emit_console_line(verdict_line.as_str());
    }

    #[cfg(feature = "kernel")]
    fn wifi_capture_verdict(snapshot: &WifiDebugSnapshot) -> (&'static str, &'static str) {
        let exact_error = snapshot.control_plane_exact_error;
        if exact_error
            == "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready"
        {
            return ("function1-sideband-edge", "function1-sideband");
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
            "function2-ready" => "setup-firmware-channel",
            "function2-interrupts" => "mailbox-ready",
            "setup-firmware-channel" => "wait-firmware-ready",
            "startup-link-recovery" => "first-function2-reply",
            "function1-sideband" => "first-function2-reply",
            "first-function2-reply" => "promote-link",
            _ => "wait-firmware-ready",
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

    fn consume_serial(&mut self) {
        while let Some(line) = self.serial.next_line() {
            self.last_input_source = ConsoleInputSource::Serial;
            self.process_console_line(&line);
        }
    }

    fn consume_local_seat(&mut self) {
        let mut chunk = [0u8; KEYBOARD_POLL_CHUNK_BYTES];
        loop {
            if let Some(runtime) = self.local_seat.as_mut() {
                runtime.poll_backend_keyboard();
            }
            let read = match self.local_seat.as_mut() {
                Some(runtime) => runtime.drain_keyboard_bytes(&mut chunk),
                None => return,
            };
            if read == 0 {
                break;
            }
            self.last_input_source = ConsoleInputSource::Serial;
            for &byte in &chunk[..read] {
                match byte {
                    b'\r' => {}
                    b'\n' => {
                        if self.local_line.is_empty() {
                            self.handle_console_error(ConsoleError::EmptyLine);
                        } else {
                            let mut line = HeaplessString::new();
                            core::mem::swap(&mut line, &mut self.local_line);
                            self.process_console_line(&line);
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
    }

    fn process_console_line(&mut self, line: &HeaplessString<LINE>) {
        self.metrics.console_lines = self.metrics.console_lines.saturating_add(1);
        #[cfg(feature = "kernel")]
        if self.maybe_handle_usb_debug_line(line.as_str()) {
            if self.last_input_source == ConsoleInputSource::Serial {
                self.emit_prompt();
            }
            return;
        }
        #[cfg(feature = "kernel")]
        if self.maybe_handle_wifi_debug_line(line.as_str()) {
            if self.last_input_source == ConsoleInputSource::Serial {
                self.emit_prompt();
            }
            return;
        }
        if let Err(err) = self.feed_parser(line) {
            self.handle_console_error(err);
        }
        if self.last_input_source == ConsoleInputSource::Serial {
            self.emit_prompt();
        }
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
                            "[net-console] conn {}: established from {}",
                            conn_id,
                            remote
                        );
                    }
                    None => {
                        log::info!(
                            target: "net-console",
                            "[net-console] conn {}: established",
                            conn_id
                        );
                    }
                },
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
            Command::Smp => {
                if self.emit_smp() {
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok(verb_label, None);
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
                                self.emit_ack_ok(verb_label, None);
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
                            "netstats: udp_rx={} udp_tx={} tcp_accepts={} tcp_rx_bytes={} tcp_tx_bytes={}",
                            stats.udp_rx,
                            stats.udp_tx,
                            stats.tcp_accepts,
                            stats.tcp_rx_bytes,
                            stats.tcp_tx_bytes
                        ));
                        let line_three = format_message(format_args!(
                            "netstats: tcp_smoke_out={} tcp_smoke_out_failures={}",
                            stats.tcp_smoke_outbound, stats.tcp_smoke_outbound_failures
                        ));
                        let line_four = format_message(format_args!(
                            "netstats: tx_submit={} tx_complete={} tx_free={} tx_in_flight={} tx_double_submit={} tx_zero_len_attempt={}",
                            stats.tx_submit,
                            stats.tx_complete,
                            stats.tx_free,
                            stats.tx_in_flight,
                            stats.tx_double_submit,
                            stats.tx_zero_len_attempt
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
                        self.emit_console_line(line_four.as_str());
                        self.emit_console_line(line_five.as_str());
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
            Command::Tail { path } => {
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
                                let bytes = {
                                    let pending =
                                        self.pending_stream.get_or_insert_with(PendingStream::new);
                                    pending.reset();
                                    log_buffer::snapshot_lines_into(&mut pending.lines);
                                    pending.lines.iter().map(|line| line.len() as u64).sum()
                                };
                                stream_bytes = bytes;
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
                                let (data_bytes, cat_err) = {
                                    let pending =
                                        self.pending_stream.get_or_insert_with(PendingStream::new);
                                    pending.reset();
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
                                };
                                if let Some(err) = cat_err {
                                    cmd_status = "err";
                                    let sid = self.session_id.unwrap_or(0);
                                    let err_msg = format_message(format_args!("{err}"));
                                    self.audit_ninedoor_err(sid, "CAT", path_str, err_msg.as_str());
                                    self.emit_ninedoor_refusal(verb_label, Some(path_str), &err);
                                } else {
                                    let log_path = path_str == "/log/queen.log";
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
                                            if log_path {
                                                self.pending_stream = None;
                                            } else if let Some(pending) =
                                                self.pending_stream.as_mut()
                                            {
                                                pending.next_line = 0;
                                                pending.bandwidth_bytes = stream_bytes;
                                                pending.cursor =
                                                    cursor_check.map(|check| PendingCursor {
                                                        path_key: path_str.to_owned(),
                                                        offset: 0,
                                                        len: data_bytes as usize,
                                                        check,
                                                    });
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
                        let (stream_bytes, pending_stream) = {
                            let lines = log_buffer::snapshot_lines::<
                                DEFAULT_LINE_CAPACITY,
                                { log_buffer::LOG_SNAPSHOT_LINES },
                            >();
                            let stream_bytes = lines.iter().map(|line| line.len() as u64).sum();
                            let pending_stream = Some(PendingStream {
                                lines,
                                next_line: 0,
                                bandwidth_bytes: stream_bytes,
                                cursor: None,
                            });
                            (stream_bytes, pending_stream)
                        };
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
                                self.pending_stream = pending_stream;
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
                        Command::Tail { path } => {
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
                    Command::Tail { path } if path.as_str() == "/log/queen.log" => {
                        self.emit_log_snapshot();
                    }
                    Command::Tail { path } if path.as_str() == "/proc/ingest/watch" => {
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
            Command::Tail { path } => {
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
            | Command::Smp
            | Command::Mem
            | Command::CacheLog { .. }
            | Command::Ping
            | Command::Test
            | Command::NetTest
            | Command::NetStats
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
                if pending.next_line < pending.lines.len() {
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
        let Some(mut pending) = self.pending_stream.take() else {
            self.emit_stream_end_if_pending();
            return;
        };
        while pending.next_line < pending.lines.len() {
            let line = pending.lines[pending.next_line].as_str();
            if !self.try_emit_console_line(line) {
                self.pending_stream = Some(pending);
                return;
            }
            pending.next_line = pending.next_line.saturating_add(1);
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
            role_label,
            ticket,
            claims.issued_at_ms,
            ttl_s,
            self.now_ms
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
    use crate::hal::HalError;
    #[cfg(feature = "net-console")]
    use crate::net::{NetSelfTestStartResult, NetStatusReport, NetTelemetry};
    #[cfg(feature = "kernel")]
    use crate::ninedoor::NineDoorBridge;
    use crate::serial::test_support::LoopbackSerial;
    use crate::serial::SerialPort;
    use cohesix_ticket::{BudgetSpec, MountSpec, TicketClaims, TicketIssuer};
    use std::time::{SystemTime, UNIX_EPOCH};

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
    fn net_diag_changed_detects_real_io_progress() {
        let prev = NetDiagSnapshot::default();
        let mut curr = prev;
        curr.bytes_written = 1;
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

    struct NullIpc;

    impl IpcDispatcher for NullIpc {
        fn dispatch(&mut self, _now_ms: u64) {}
    }

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

    #[cfg(feature = "net-console")]
    struct FakeNet {
        lines: heapless::Vec<ConsoleLine, 4>,
        sent: heapless::Vec<HeaplessString<DEFAULT_LINE_CAPACITY>, 8>,
        start_result: NetSelfTestStartResult,
        status: NetStatusReport,
    }

    #[cfg(feature = "net-console")]
    impl FakeNet {
        fn new() -> Self {
            Self {
                lines: heapless::Vec::new(),
                sent: heapless::Vec::new(),
                start_result: NetSelfTestStartResult::Unsupported,
                status: NetStatusReport::default(),
            }
        }
    }

    #[cfg(feature = "net-console")]
    impl NetPoller for FakeNet {
        fn poll(&mut self, _now_ms: u64) -> bool {
            true
        }

        fn telemetry(&self) -> NetTelemetry {
            NetTelemetry {
                link_up: true,
                tx_drops: 0,
                last_poll_ms: 0,
            }
        }

        fn drain_console_lines(&mut self, _now_ms: u64, visitor: &mut dyn FnMut(ConsoleLine)) {
            while !self.lines.is_empty() {
                let line = self.lines.remove(0);
                visitor(line);
            }
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
        ht_ready: bool,
        calls: heapless::Vec<&'static str, 8>,
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
                    control_plane_f2_state: "latched-linux-configured-no-iorx",
                    control_plane_sdhci_read_diag: "f1-reply-read-command-phase-no-data-active",
                    control_plane_exact_error: "cyw43-control-plane-sideband-command-stall",
                },
                ht_ready: true,
                calls: heapless::Vec::new(),
            }
        }
    }

    #[cfg(feature = "kernel")]
    impl WifiDebugOps for FakeWifiDebug {
        fn dump_state(&mut self, _stage: &'static str) -> Result<WifiDebugSnapshot, HalError> {
            let _ = self.calls.push("dump-state");
            Ok(self.snapshot)
        }

        fn probe_ht_clock(&mut self) -> Result<bool, HalError> {
            let _ = self.calls.push("probe-ht");
            Ok(self.ht_ready)
        }

        fn load_firmware(&mut self) -> Result<WifiDebugSnapshot, HalError> {
            let _ = self.calls.push("load-fw");
            Ok(self.snapshot)
        }

        fn retry_transport_and_firmware(&mut self) -> Result<WifiDebugSnapshot, HalError> {
            let _ = self.calls.push("retry");
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
        crate::hal::set_timebase_now_ms(0);

        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 5 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);

        pump.poll();

        assert_eq!(crate::hal::timebase().now_ms(), 5);

        crate::hal::set_timebase_now_ms(0);
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
        line.push_str("ping").unwrap();
        net.lines.push(ConsoleLine::new(line, 1)).unwrap();
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
        let driver = LoopbackSerial::<512>::new();
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
    }

    #[test]
    fn console_acknowledgements_emit_expected_lines() {
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
    fn tail_command_emits_end_sentinel() {
        let driver = LoopbackSerial::<512>::new();
        let serial = SerialPort::<_, 512, 512, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "queen-ticket").unwrap();
        store
            .register(Role::WorkerHeartbeat, "worker-ticket")
            .unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        {
            let driver = pump.serial_mut().driver_mut();
            let worker_token = issue_token("worker-ticket", Role::WorkerHeartbeat);
            let line = format!("attach worker {worker_token}\n");
            driver.push_rx(line.as_bytes());
            driver.push_rx(b"tail /log/queen.log\n");
        }
        pump.poll();
        pump.poll();
        let transcript = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains("OK ATTACH role=worker-heartbeat"),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK TAIL path=/log/queen.log"),
            "{rendered}"
        );
        assert!(rendered.contains("END\r\n"), "{rendered}");
    }

    #[test]
    fn log_command_emits_end_sentinel_and_quit_clears_session() {
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

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_wifi_debug_command_uses_attached_debug_ops() {
        let driver = LoopbackSerial::<256>::new();
        let serial = SerialPort::<_, 256, 256, DEFAULT_LINE_CAPACITY>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut wifi = FakeWifiDebug::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_wifi_debug(&mut wifi);

        pump.serial_mut().driver_mut().push_rx(b"wifi dump-state\n");
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
                "wifi: golden_path route=strict-then-bounded-no-ht state=fallback-no-ht transport=bounded-no-ht current=function1-sideband next=first-function2-reply focus=function1-sideband verdict=function1-sideband-edge"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("safe_profile_locked=yes safe_reason=promoted-io-unstable"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: f2_recover stage=control-plane-reply-full-block-read policy=linux-rxfail op=read drained=no count=0x0040"),
            "{rendered}"
        );
        assert!(
            rendered.contains("wifi: f2_state=latched-linux-configured-no-iorx exact_error=cyw43-control-plane-sideband-command-stall"),
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
    fn serial_usb_debug_command_enables_local_seat_polling() {
        let driver = LoopbackSerial::<256>::new();
        let serial = SerialPort::<_, 256, 256, DEFAULT_LINE_CAPACITY>::new(driver);
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
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.serial_mut().driver_mut().push_rx(b"usb enable-kbd\n");
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
            rendered.contains("usb: local-seat attached=no polling=enabled"),
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
        let driver = LoopbackSerial::<256>::new();
        let serial = SerialPort::<_, 256, 256, DEFAULT_LINE_CAPACITY>::new(driver);
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
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_local_seat(&mut local_seat);

        pump.serial_mut().driver_mut().push_rx(b"usb probe-kbd\n");
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
                "usb: local-seat attached=no polling=deferred action=keyboard-probe-complete"
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
            control_plane_f2_state: "latched-linux-configured-no-iorx",
            control_plane_sdhci_read_diag: "f2-reply-read-data-wait",
            control_plane_exact_error: "cyw43-control-plane-pure-f2-startup-link-no-reply",
        };
        assert_eq!(
            EventPump::<
                SerialPort<LoopbackSerial<32>, 32, 32, 32>,
                TestTimer,
                NullIpc,
            >::wifi_capture_verdict(&snapshot),
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
            control_plane_f2_state: "latched-no-iorx",
            control_plane_sdhci_read_diag: "f2-reply-read-stalled-no-buffer-ready",
            control_plane_exact_error: "cyw43-function2-reply-read-stall-no-buffer-ready",
        };
        assert_eq!(
            EventPump::<
                SerialPort<LoopbackSerial<32>, 32, 32, 32>,
                TestTimer,
                NullIpc,
            >::wifi_capture_verdict(&snapshot),
            ("function2-reply-edge", "first-function2-reply")
        );
        assert_eq!(
            EventPump::<
                SerialPort<LoopbackSerial<32>, 32, 32, 32>,
                TestTimer,
                NullIpc,
            >::wifi_golden_path_current_step(&snapshot),
            "first-function2-reply"
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
            control_plane_f2_state: "unproven",
            control_plane_sdhci_read_diag: "f1-reply-read-command-timeout",
            control_plane_exact_error: "cyw43-control-plane-sideband-command-timeout",
        };
        assert_eq!(
            EventPump::<
                SerialPort<LoopbackSerial<32>, 32, 32, 32>,
                TestTimer,
                NullIpc,
            >::wifi_capture_verdict(&snapshot),
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
            control_plane_f2_state: "latched-linux-configured-no-iorx",
            control_plane_sdhci_read_diag: "f1-reply-read-stalled-no-buffer-ready",
            control_plane_exact_error:
                "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",
        };
        assert_eq!(
            EventPump::<
                SerialPort<LoopbackSerial<32>, 32, 32, 32>,
                TestTimer,
                NullIpc,
            >::wifi_capture_verdict(&snapshot),
            ("function1-sideband-edge", "function1-sideband")
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
            control_plane_f2_state: "unproven",
            control_plane_sdhci_read_diag: "f1-reply-read-command-timeout",
            control_plane_exact_error: "cyw43-control-plane-sideband-command-timeout",
        };
        assert_eq!(
            EventPump::<
                SerialPort<LoopbackSerial<32>, 32, 32, 32>,
                TestTimer,
                NullIpc,
            >::wifi_golden_path_current_step(&snapshot),
            "function1-sideband"
        );
        assert_eq!(
            EventPump::<
                SerialPort<LoopbackSerial<32>, 32, 32, 32>,
                TestTimer,
                NullIpc,
            >::wifi_golden_path_next_step(&snapshot),
            "first-function2-reply"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn usb_capture_verdict_classifies_run_transition_edge() {
        assert_eq!(
            EventPump::<
                SerialPort<LoopbackSerial<32>, 32, 32, 32>,
                TestTimer,
                NullIpc,
            >::usb_capture_verdict(true, true, Some("usbcmd-run-store-wedged")),
            ("run-transition-edge", "usbcmd-run")
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn local_seat_wifi_debug_command_mirrors_output() {
        let driver = LoopbackSerial::<256>::new();
        let serial = SerialPort::<_, 256, 256, DEFAULT_LINE_CAPACITY>::new(driver);
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
        let mut wifi = FakeWifiDebug::new();
        local_seat.enqueue_keyboard_bytes(b"wifi probe-ht\n");

        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit)
            .with_wifi_debug(&mut wifi)
            .with_local_seat(&mut local_seat);
        pump.session = Some(SessionRole::Queen);
        pump.poll();
        drop(pump);

        let mirrored = local_seat.mirrored_lines_snapshot();
        assert!(mirrored.iter().any(|line| line.contains(
            "wifi: debug subcommand=probe-ht action=begin profile=bounded mode=one-shot"
        )));
        assert!(mirrored
            .iter()
            .any(|line| line.contains("wifi ht: ready=yes")));
        assert!(mirrored.iter().any(|line| line.contains(
            "wifi: debug subcommand=probe-ht action=complete profile=bounded mode=one-shot result=ok"
        )));
        assert!(mirrored
            .iter()
            .any(|line| line.contains("OK WIFI detail=subcommand=probe-ht")));
        assert_eq!(wifi.calls.as_slice(), &["probe-ht", "dump-state"]);
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

        match result {
            Err(CommandDispatchError::NineDoorUnavailable { verb }) => {
                assert_eq!(verb.ack_label(), "LOG");
            }
            other => panic!("unexpected result: {other:?}"),
        }

        assert!(audit
            .denials
            .iter()
            .any(|entry| entry.contains("ninedoor unavailable")));
    }
}
