// Copyright © 2025 Lukas Bower
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
use cohesix_ticket::{Role, TicketClaims, TicketQuotas, TicketToken, TicketVerb};
use heapless::{String as HeaplessString, Vec as HeaplessVec};
use cohsh_core::{ConsoleVerb, RoleParseMode};

#[cfg(feature = "kernel")]
use crate::bootstrap::log as boot_log;
use crate::console::proto::{render_ack, AckLine, AckStatus, LineFormatError};
use crate::console::{Command, CommandParser, ConsoleError, MAX_ROLE_LEN, MAX_TICKET_LEN};
#[cfg(feature = "kernel")]
use crate::debug_uart::debug_uart_str;
#[cfg(feature = "kernel")]
use crate::log_buffer;
#[cfg(feature = "net-console")]
use crate::net::{
    ConsoleLine, NetConsoleDisconnectReason, NetConsoleEvent, NetDiagSnapshot, NetPoller,
    NetTelemetry, CONSOLE_QUEUE_DEPTH, NET_DIAG, NET_DIAG_FEATURED,
};
#[cfg(feature = "net-console")]
use crate::observe::IngestSnapshot;
#[cfg(feature = "kernel")]
use crate::ninedoor::{NineDoorBridge, NineDoorBridgeError};
#[cfg(feature = "kernel")]
use crate::ninedoor::TelemetryTail;
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
#[cfg(feature = "net-console")]
const NET_DIAG_HEARTBEAT_MS: u64 = 5_000;
#[cfg(feature = "net-console")]
const NET_DIAG_RATE_LIMIT_MS: u64 = 1_000;
#[cfg(feature = "net-console")]
const NET_DIAG_RATE_KINDS: usize = 1;
#[cfg(feature = "net-console")]
const NET_DIAG_HEARTBEAT_POLLS: u64 = 1_024;
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
            TicketClaimError::ScopeRate { rate, max } => write!(f, "scope rate {rate} exceeds {max}"),
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
    Rate { limit_per_s: u32 },
    Bandwidth {
        limit_bytes: u64,
        remaining_bytes: u64,
        requested_bytes: u64,
    },
    CursorResume { limit: u32 },
    CursorAdvance { limit: u32 },
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
        let is_resume = last.map_or(false, |last| offset < last);
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
            if best.map_or(true, |(_, best_len)| match_len > best_len) {
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
            Role::WorkerHeartbeat
            | Role::WorkerGpu
            | Role::WorkerBus
            | Role::WorkerLora => Some(Self::Worker),
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
    lines: HeaplessVec<
        HeaplessString<DEFAULT_LINE_CAPACITY>,
        { log_buffer::LOG_SNAPSHOT_LINES },
    >,
    next_line: usize,
    bandwidth_bytes: u64,
    cursor: Option<PendingCursor>,
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

    /// Attach a NineDoor handler to the event pump.
    #[cfg(feature = "kernel")]
    pub fn with_ninedoor(mut self, bridge: &'a mut NineDoorBridge) -> Self {
        self.ninedoor = Some(bridge);
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

    /// Execute a single cooperative polling cycle.
    pub fn poll(&mut self) {
        self.serial.poll_io();
        self.consume_serial();

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
            let mut buffered: HeaplessVec<
                ConsoleLine,
                { CONSOLE_QUEUE_DEPTH },
            > = HeaplessVec::new();
            net.drain_console_lines(self.now_ms, &mut |line| {
                let _ = buffered.push(line);
            });
            let ingest_snapshot: IngestSnapshot = net.ingest_snapshot();
            Some((activity, telemetry, buffered, conn_id, ingest_snapshot))
        } else {
            None
        };

        #[cfg(feature = "net-console")]
        if let Some((activity, telemetry, buffered, conn_id, ingest_snapshot)) = net_poll {
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
                bridge.update_ingest_snapshot(ingest_snapshot);
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
        let activity = self.last_net_diag_emitted.map_or(true, |prev| {
            Self::net_diag_changed(prev.snapshot, snapshot)
                || prev.link_up != telemetry.link_up
                || prev.tx_drops != telemetry.tx_drops
        });
        let heartbeat_poll = self.last_net_diag_emitted.map_or(false, |prev| {
            snapshot.poll_calls.saturating_sub(prev.snapshot.poll_calls) >= NET_DIAG_HEARTBEAT_POLLS
        });
        let heartbeat_time = self.last_net_diag_log_ms.map_or(false, |last| {
            self.now_ms.saturating_sub(last) >= NET_DIAG_HEARTBEAT_MS
        });

        activity || heartbeat_poll || heartbeat_time
    }

    #[cfg(feature = "net-console")]
    fn net_diag_changed(prev: NetDiagSnapshot, curr: NetDiagSnapshot) -> bool {
        let mut prev = prev;
        let mut curr = curr;
        prev.poll_calls = 0;
        curr.poll_calls = 0;
        prev != curr
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
            boot_log::switch_logger_to_log_buffer();
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
        #[cfg(feature = "net-console")]
        if self.last_input_source == ConsoleInputSource::Net {
            if let Some(net) = self.net.as_mut() {
                return net.send_console_line(line);
            }
        }
        false
    }

    fn emit_serial_line(&mut self, line: &str) {
        self.serial.enqueue_tx(line.as_bytes());
        self.serial.enqueue_tx(b"\r\n");
    }

    fn emit_prompt(&mut self) {
        self.serial.enqueue_tx(CONSOLE_PROMPT.as_bytes());
    }

    fn emit_help(&mut self) {
        self.emit_console_line("Commands:");
        self.emit_console_line("  help  - Show this help");
        self.emit_console_line("  bi    - Show bootinfo summary");
        self.emit_console_line("  caps  - Show capability slots");
        self.emit_console_line("  mem   - Show untyped summary");
        self.emit_console_line("  ping  - Respond with pong");
        self.emit_console_line("  test  - Self-test (host-only; use cohsh)");
        self.emit_console_line("  nettest  - Run network self-test (dev-virt)");
        self.emit_console_line("  netstats - Show network counters");
        self.emit_console_line("  quit  - Exit the console session");
    }

    fn emit_help_serial_only(&mut self) {
        self.emit_serial_line("Commands:");
        self.emit_serial_line("  help  - Show this help");
        self.emit_serial_line("  bi    - Show bootinfo summary");
        self.emit_serial_line("  caps  - Show capability slots");
        self.emit_serial_line("  mem   - Show untyped summary");
        self.emit_serial_line("  ping  - Respond with pong");
        self.emit_serial_line("  test  - Self-test (host-only; use cohsh)");
        self.emit_serial_line("  nettest  - Run network self-test (dev-virt)");
        self.emit_serial_line("  netstats - Show network counters");
        self.emit_serial_line("  quit  - Exit the console session");
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
                self.emit_console_line("ERR PARSE reason=ack-truncated");
            }
        }
    }

    fn emit_ack_ok(&mut self, verb: &str, detail: Option<&str>) {
        self.emit_ack(AckStatus::Ok, verb, detail);
    }

    fn emit_ack_err(&mut self, verb: &str, detail: Option<&str>) {
        self.emit_ack(AckStatus::Err, verb, detail);
    }

    fn emit_auth_failure(&mut self, verb: &str) {
        self.metrics.ui_denies = self.metrics.ui_denies.saturating_add(1);
        self.emit_ack_err(verb, Some("reason=unauthenticated"));
    }

    fn handle_console_error(&mut self, err: ConsoleError) {
        let message = format_message(format_args!("console error: {}", err));
        self.audit.info(message.as_str());
        let detail = match err {
            ConsoleError::RateLimited(delay) => {
                format_message(format_args!("reason=rate-limited delay_ms={delay}"))
            }
            other => format_message(format_args!("reason={}", other)),
        };
        self.emit_ack_err("PARSE", Some(detail.as_str()));
        if self.parser.clear_buffer() {
            self.audit
                .info("console: cleared partial input after parse error");
        }
    }

    fn consume_serial(&mut self) {
        while let Some(line) = self.serial.next_line() {
            self.last_input_source = ConsoleInputSource::Serial;
            self.process_console_line(&line);
        }
    }

    fn process_console_line(&mut self, line: &HeaplessString<LINE>) {
        self.metrics.console_lines = self.metrics.console_lines.saturating_add(1);
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
                    self.emit_ack_err(verb_label, Some("reason=unavailable"));
                }
            }
            Command::Caps => {
                if self.emit_caps() {
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok(verb_label, None);
                } else {
                    self.metrics.denied_commands += 1;
                    cmd_status = "err";
                    self.emit_ack_err(verb_label, Some("reason=unavailable"));
                }
            }
            Command::Mem => {
                if self.emit_mem() {
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok(verb_label, None);
                } else {
                    self.metrics.denied_commands += 1;
                    cmd_status = "err";
                    self.emit_ack_err(verb_label, Some("reason=unavailable"));
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
                    self.emit_ack_err(verb_label, Some("reason=unsupported"));
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
                self.emit_ack_err(verb_label, Some("reason=host-only"));
            }
            Command::NetTest => {
                #[cfg(feature = "net-console")]
                {
                    if let Some(net) = self.net.as_mut() {
                        if net.start_self_test(self.now_ms) {
                            self.metrics.accepted_commands += 1;
                            self.emit_console_line("[net-selftest] triggered");
                            self.emit_ack_ok(verb_label, None);
                        } else {
                            self.metrics.denied_commands += 1;
                            cmd_status = "err";
                            self.emit_ack_err(verb_label, Some("reason=unsupported"));
                        }
                    } else {
                        self.metrics.denied_commands += 1;
                        cmd_status = "err";
                        self.emit_ack_err(verb_label, Some("reason=net-disabled"));
                    }
                }
                #[cfg(not(feature = "net-console"))]
                {
                    self.metrics.denied_commands += 1;
                    cmd_status = "err";
                    self.emit_ack_err(verb_label, Some("reason=net-disabled"));
                }
            }
            Command::NetStats => {
                #[cfg(feature = "net-console")]
                {
                    if let Some(net) = self.net.as_mut() {
                        let stats = net.stats();
                        let report = net.self_test_report();
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
                        let status_line = format_message(format_args!(
                            "nettest: enabled={} running={} last={:?}",
                            report.enabled, report.running, report.last_result
                        ));
                        self.emit_console_line(line_one.as_str());
                        self.emit_console_line(line_two.as_str());
                        self.emit_console_line(line_three.as_str());
                        self.emit_console_line(line_four.as_str());
                        self.emit_console_line(status_line.as_str());
                        self.metrics.accepted_commands += 1;
                        self.emit_ack_ok(verb_label, None);
                    } else {
                        self.metrics.denied_commands += 1;
                        cmd_status = "err";
                        self.emit_ack_err(verb_label, Some("reason=net-disabled"));
                    }
                }
                #[cfg(not(feature = "net-console"))]
                {
                    self.metrics.denied_commands += 1;
                    cmd_status = "err";
                    self.emit_ack_err(verb_label, Some("reason=net-disabled"));
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
                if self.ensure_authenticated(SessionRole::Worker) {
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
                        let mut pending_stream: Option<PendingStream> = None;
                        #[cfg(feature = "kernel")]
                        let mut telemetry_stream: Option<TelemetryTail> = None;
                        #[cfg(feature = "kernel")]
                        let mut path_supported = false;
                        #[cfg(feature = "kernel")]
                        {
                            let cursor_offset = self.ticket_cursor_offset(path_str).unwrap_or(0);
                            if path_str == "/log/queen.log" {
                                let lines = log_buffer::snapshot_lines::<
                                    DEFAULT_LINE_CAPACITY,
                                    { log_buffer::LOG_SNAPSHOT_LINES },
                                >();
                                stream_bytes = lines.iter().map(|line| line.len() as u64).sum();
                                pending_stream = Some(PendingStream {
                                    lines,
                                    next_line: 0,
                                    bandwidth_bytes: stream_bytes,
                                    cursor: None,
                                });
                                path_supported = true;
                            } else if path_str == "/proc/ingest/watch" {
                                if let Some(bridge) = self.ninedoor.as_mut() {
                                    let lines = bridge
                                        .ingest_watch_lines(self.now_ms, &mut *self.audit)
                                        .unwrap_or_else(|_| HeaplessVec::new());
                                    stream_bytes = lines.iter().map(|line| line.len() as u64).sum();
                                    pending_stream = Some(PendingStream {
                                        lines,
                                        next_line: 0,
                                        bandwidth_bytes: stream_bytes,
                                        cursor: None,
                                    });
                                    path_supported = true;
                                }
                            } else if let Some(bridge) = self.ninedoor.as_mut() {
                                match bridge.telemetry_tail(path_str, cursor_offset) {
                                    Ok(stream) => {
                                        telemetry_stream = stream;
                                        if let Some(stream) = telemetry_stream.as_ref() {
                                            stream_bytes = stream
                                                .lines
                                                .iter()
                                                .map(|line| line.len() as u64)
                                                .sum();
                                            path_supported = true;
                                        }
                                    }
                                    Err(err) => {
                                        let detail = format_message(format_args!(
                                            "reason=ninedoor-error error={err}"
                                        ));
                                        cmd_status = "err";
                                        let sid = self.session_id.unwrap_or(0);
                                        let err_msg = format_message(format_args!("{err}"));
                                        self.audit_ninedoor_err(
                                            sid,
                                            "TAIL",
                                            path_str,
                                            err_msg.as_str(),
                                        );
                                        self.emit_ack_err(verb_label, Some(detail.as_str()));
                                    }
                                }
                            }
                        }
                        #[cfg(feature = "kernel")]
                        if cmd_status != "err" && !path_supported {
                            cmd_status = "err";
                            let detail = format_message(format_args!(
                                "reason=invalid-path path={}",
                                path_str
                            ));
                            self.emit_ack_err(verb_label, Some(detail.as_str()));
                        }
                        if cmd_status != "err" {
                            if let Err(denial) = self.check_ticket_bandwidth(stream_bytes) {
                                self.record_ticket_denial(path_str, TicketVerb::Read, denial);
                                self.emit_ticket_denied(verb_label, Some(path_str), denial);
                                cmd_status = "err";
                            } else {
                                #[cfg(feature = "kernel")]
                                if let Some(stream) = telemetry_stream {
                                    let cursor_check =
                                        match self.check_ticket_cursor(path_str, stream.start_offset) {
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
                                                cmd_status = "err";
                                                None
                                            }
                                        };
                                    if cmd_status != "err" {
                                        pending_stream = Some(PendingStream {
                                            lines: stream.lines,
                                            next_line: 0,
                                            bandwidth_bytes: stream_bytes,
                                            cursor: cursor_check.map(|check| PendingCursor {
                                                path_key: path_str.to_owned(),
                                                offset: stream.start_offset,
                                                len: stream.consumed_bytes,
                                                check,
                                            }),
                                        });
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
                                        self.pending_stream = pending_stream;
                                        forwarded = true;
                                    }
                                }
                            }
                        }
                    }
                } else {
                    cmd_status = "err";
                    self.emit_auth_failure(verb_label);
                }
            }
            Command::Cat { path } => {
                if self.ensure_authenticated(SessionRole::Worker) {
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
                                match bridge_ref.cat(path_str) {
                                    Ok(lines) => {
                                        let data_bytes =
                                            lines.iter().map(|line| line.len() as u64).sum();
                                        let log_path = path_str == "/log/queen.log";
                                        let stream_bytes = if log_path { 0 } else { data_bytes };
                                        if let Err(denial) = self.check_ticket_bandwidth(stream_bytes) {
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
                                        } else {
                                            let cursor_check =
                                                match self.check_ticket_cursor(path_str, 0) {
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
                                                        cmd_status = "err";
                                                        None
                                                    }
                                                };
                                            if cmd_status != "err" {
                                                let summary = {
                                                    // Prefer user echo lines while also surfacing newer audit entries.
                                                    // Keep references to avoid a large stack copy for /log/queen.log.
                                                    let user_lines: HeaplessVec<
                                                        HeaplessString<DEFAULT_LINE_CAPACITY>,
                                                        { log_buffer::LOG_USER_SNAPSHOT_LINES },
                                                    > = if log_path {
                                                        log_buffer::snapshot_user_lines::<
                                                            DEFAULT_LINE_CAPACITY,
                                                            { log_buffer::LOG_USER_SNAPSHOT_LINES },
                                                        >()
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
                                                            for line in lines.iter() {
                                                                if summary_refs.push(line.as_str()).is_err() {
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
                                                                lines.iter().enumerate()
                                                            {
                                                                if user_lines
                                                                    .iter()
                                                                    .any(|user_line| {
                                                                        user_line.as_str()
                                                                            == line.as_str()
                                                                    })
                                                                {
                                                                    last_user_idx = Some(idx);
                                                                }
                                                            }
                                                            let start = last_user_idx
                                                                .map(|idx| idx + 1)
                                                                .unwrap_or(0);
                                                            for line in lines.iter().skip(start) {
                                                                if line.as_str().starts_with('[') {
                                                                    continue;
                                                                }
                                                                if user_lines
                                                                    .iter()
                                                                    .any(|user_line| {
                                                                        user_line.as_str()
                                                                            == line.as_str()
                                                                    })
                                                                {
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
                                                        for line in lines.iter() {
                                                            if summary_refs.push(line.as_str()).is_err() {
                                                                break;
                                                            }
                                                        }
                                                    }
                                                    let summary_lines: &[&str] = summary_refs.as_slice();
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
                                                            let sep = if total_len == 0 { 0 } else { 1 };
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
                                                    if selected.is_empty()
                                                        && !summary_lines.is_empty()
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
                                                            if let Some(line) =
                                                                summary_lines.get(*idx)
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
                                                        "path=".len()
                                                            + path_str.len()
                                                            + " data=".len(),
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
                                                } else {
                                                    self.pending_stream = Some(PendingStream {
                                                        lines,
                                                        next_line: 0,
                                                        bandwidth_bytes: stream_bytes,
                                                        cursor: cursor_check.map(|check| PendingCursor {
                                                            path_key: path_str.to_owned(),
                                                            offset: 0,
                                                            len: data_bytes as usize,
                                                            check,
                                                        }),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                    Err(err) => {
                                        let detail = format_message(format_args!(
                                            "reason=ninedoor-error error={err}"
                                        ));
                                        cmd_status = "err";
                                        let sid = self.session_id.unwrap_or(0);
                                        let err_msg = format_message(format_args!("{err}"));
                                        self.audit_ninedoor_err(
                                            sid,
                                            "CAT",
                                            path_str,
                                            err_msg.as_str(),
                                        );
                                        self.emit_ack_err(verb_label, Some(detail.as_str()));
                                    }
                                }
                            } else {
                                cmd_status = "err";
                                self.emit_ack_err(verb_label, Some("reason=ninedoor-unavailable"));
                            }
                        }
                        #[cfg(not(feature = "kernel"))]
                        {
                            cmd_status = "err";
                            self.emit_ack_err(verb_label, Some("reason=ninedoor-unavailable"));
                        }
                    }
                } else {
                    cmd_status = "err";
                    self.emit_auth_failure(verb_label);
                }
            }
            Command::Ls { path } => {
                if self.ensure_authenticated(SessionRole::Worker) {
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
                                match bridge_ref.list(path_str) {
                                    Ok(entries) => {
                                        let data_bytes =
                                            entries.iter().map(|entry| entry.len() as u64).sum();
                                        if let Err(denial) = self.check_ticket_bandwidth(data_bytes) {
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
                                        } else {
                                            let detail = format_message(format_args!(
                                                "path={} entries={}",
                                                path_str,
                                                entries.len()
                                            ));
                                            self.emit_ack_ok(verb_label, Some(detail.as_str()));
                                            for entry in entries {
                                                self.emit_console_line(entry.as_str());
                                            }
                                            self.consume_ticket_bandwidth(data_bytes);
                                            self.stream_end_pending = true;
                                        }
                                    }
                                    Err(err) => {
                                        let detail = format_message(format_args!(
                                            "reason=ninedoor-error error={err}"
                                        ));
                                        cmd_status = "err";
                                        let sid = self.session_id.unwrap_or(0);
                                        let err_msg = format_message(format_args!("{err}"));
                                        self.audit_ninedoor_err(
                                            sid,
                                            "LS",
                                            path_str,
                                            err_msg.as_str(),
                                        );
                                        self.emit_ack_err(verb_label, Some(detail.as_str()));
                                    }
                                }
                            } else {
                                cmd_status = "err";
                                self.emit_ack_err(verb_label, Some("reason=ninedoor-unavailable"));
                            }
                        }
                        #[cfg(not(feature = "kernel"))]
                        {
                            cmd_status = "err";
                            self.emit_ack_err(verb_label, Some("reason=ninedoor-unavailable"));
                        }
                    }
                } else {
                    cmd_status = "err";
                    self.emit_auth_failure(verb_label);
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
                if self.ensure_authenticated(SessionRole::Worker) {
                    let path_str = path.as_str();
                    let worker_restricted = matches!(self.session, Some(SessionRole::Worker))
                        && !(path_str.starts_with("/bus/") || path_str.starts_with("/lora/"));
                    if worker_restricted {
                        self.metrics.denied_commands += 1;
                        self.audit.denied("echo denied");
                        cmd_status = "err";
                        self.emit_ack_err(verb_label, Some("reason=denied"));
                    } else {
                        if let Err(denial) = self.check_ticket_scope(path_str, TicketVerb::Write)
                        {
                            self.record_ticket_denial(path_str, TicketVerb::Write, denial);
                            self.emit_ticket_denied(verb_label, Some(path_str), denial);
                            cmd_status = "err";
                        } else if let Err(denial) =
                            self.check_ticket_bandwidth(payload.len() as u64)
                        {
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
                                            let detail = format_message(format_args!(
                                                "reason=ninedoor-error error={err}"
                                            ));
                                            cmd_status = "err";
                                            let sid = self.session_id.unwrap_or(0);
                                            let err_msg = format_message(format_args!("{err}"));
                                            self.audit_ninedoor_err(
                                                sid,
                                                "ECHO",
                                                path.as_str(),
                                                err_msg.as_str(),
                                            );
                                            self.emit_ack_err(verb_label, Some(detail.as_str()));
                                        }
                                    }
                                } else {
                                    cmd_status = "err";
                                    self.emit_ack_err(
                                        verb_label,
                                        Some("reason=ninedoor-unavailable"),
                                    );
                                }
                            }
                            #[cfg(not(feature = "kernel"))]
                            {
                                cmd_status = "err";
                                self.emit_ack_err(
                                    verb_label,
                                    Some("reason=ninedoor-unavailable"),
                                );
                            }
                        }
                    }
                } else {
                    cmd_status = "err";
                    self.emit_auth_failure(verb_label);
                }
            }
            Command::Spawn(payload) => {
                if self.ensure_authenticated(SessionRole::Queen) {
                    if let Err(denial) =
                        self.check_ticket_scope(QUEEN_CTL_PATH, TicketVerb::Write)
                    {
                        self.record_ticket_denial(QUEEN_CTL_PATH, TicketVerb::Write, denial);
                        self.emit_ticket_denied(verb_label, Some(QUEEN_CTL_PATH), denial);
                        cmd_status = "err";
                    } else if let Err(denial) =
                        self.check_ticket_bandwidth(payload.len() as u64)
                    {
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
                    let payload_len =
                        format!("{{\"kill\":\"{}\"}}", ident.as_str()).len() as u64;
                    if let Err(denial) =
                        self.check_ticket_scope(QUEEN_CTL_PATH, TicketVerb::Write)
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
                self.emit_ack_err(verb.ack_label(), Some("reason=ninedoor-unavailable"));
            }
            CommandDispatchError::UnsupportedForNineDoor { verb } => {
                self.audit.denied("ninedoor unsupported command");
                self.emit_console_line("ERR unsupported for NineDoor");
                self.emit_ack_err(verb.ack_label(), Some("reason=unsupported"));
            }
            CommandDispatchError::Bridge { verb, source } => {
                let detail = format_message(format_args!("reason=ninedoor-error error={source}"));
                let audit_line = format_message(format_args!("ninedoor bridge error: {source}"));
                self.audit.denied(audit_line.as_str());
                self.emit_ack_err(verb.ack_label(), Some(detail.as_str()));
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
        if self.session.is_none() && !self.tail_active {
            return;
        }
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
        let reason = match denial {
            TicketDeny::Scope => "EPERM",
            TicketDeny::Rate { .. }
            | TicketDeny::Bandwidth { .. }
            | TicketDeny::CursorResume { .. }
            | TicketDeny::CursorAdvance { .. } => "ELIMIT",
        };
        let detail = match path {
            Some(path) => format_message(format_args!("path={path} reason={reason}")),
            None => format_message(format_args!("reason={reason}")),
        };
        self.emit_ack_err(verb, Some(detail.as_str()));
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

    fn check_ticket_cursor(&self, path: &str, offset: u64) -> Result<Option<CursorCheck>, TicketDeny> {
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
            let detail = format_message(format_args!("reason=throttled delay_ms={delay}"));
            self.emit_ack_err(ConsoleVerb::Attach.ack_label(), Some(detail.as_str()));
            return false;
        }

        let Some(requested_role) =
            cohsh_core::parse_role(role.as_str(), RoleParseMode::AllowWorkerAlias)
        else {
            self.audit.denied("attach: invalid role");
            self.metrics.denied_commands += 1;
            self.emit_ack_err(
                ConsoleVerb::Attach.ack_label(),
                Some("reason=invalid-role"),
            );
            return false;
        };

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
            let detail = match err {
                ConsoleError::RateLimited(delay) => {
                    format_message(format_args!("reason=rate-limited delay_ms={delay}"))
                }
                other => format_message(format_args!("reason={}", other)),
            };
            self.emit_ack_err(ConsoleVerb::Attach.ack_label(), Some(detail.as_str()));
            return false;
        }

        if validated {
            let mut ticket_usage = None;
            if let Some(ticket) = ticket_str {
                let claims = match TicketToken::decode_unverified(ticket) {
                    Ok(claims) => claims,
                    Err(err) => {
                        self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
                        self.metrics.ui_denies = self.metrics.ui_denies.saturating_add(1);
                        self.record_ticket_claim_denial(requested_role, ticket, &err);
                        self.emit_ack_err(ConsoleVerb::Attach.ack_label(), Some("reason=invalid-claims"));
                        return false;
                    }
                };
                if let Some(ttl_s) = claims.budget.ttl_s() {
                    let ttl_ms = ttl_s.saturating_mul(1_000);
                    let expires_at_ms = claims.issued_at_ms.saturating_add(ttl_ms);
                    if self.now_ms >= expires_at_ms {
                        self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
                        self.metrics.ui_denies = self.metrics.ui_denies.saturating_add(1);
                        self.record_ticket_expired(requested_role, ticket, &claims);
                        self.emit_ack_err(ConsoleVerb::Attach.ack_label(), Some("reason=expired"));
                        return false;
                    }
                }
                match TicketUsage::from_claims(&claims, crate::generated::ticket_limits(), self.now_ms)
                {
                    Ok(usage) => {
                        if usage.has_enforcement() {
                            ticket_usage = Some(usage);
                        }
                    }
                    Err(err) => {
                        self.metrics.denied_commands = self.metrics.denied_commands.saturating_add(1);
                        self.metrics.ui_denies = self.metrics.ui_denies.saturating_add(1);
                        self.record_ticket_claim_denial(requested_role, ticket, &err);
                        self.emit_ack_err(ConsoleVerb::Attach.ack_label(), Some("reason=invalid-claims"));
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
            self.emit_ack_err(ConsoleVerb::Attach.ack_label(), Some("reason=denied"));
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
    NineDoorUnavailable { verb: ConsoleVerb },
    UnsupportedForNineDoor { verb: ConsoleVerb },
    Bridge { verb: ConsoleVerb, source: NineDoorBridgeError },
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
    #[cfg(feature = "net-console")]
    use crate::net::NetTelemetry;
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

    #[test]
    fn pump_bootstrap_logs_subsystems() {
        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, 32>::new(driver);
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
        struct FakeNet {
            lines: heapless::Vec<ConsoleLine, 4>,
            sent: heapless::Vec<HeaplessString<DEFAULT_LINE_CAPACITY>, 4>,
        }

        impl FakeNet {
            fn new() -> Self {
                Self {
                    lines: heapless::Vec::new(),
                    sent: heapless::Vec::new(),
                }
            }
        }

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

            fn drain_console_lines(
                &mut self,
                _now_ms: u64,
                visitor: &mut dyn FnMut(ConsoleLine),
            ) {
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
        }

        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, 32>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "net").unwrap();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        let mut line = HeaplessString::new();
        let token = issue_token("net", Role::Queen);
        line.push_str(format!("attach queen {token}").as_str())
            .unwrap();
        net.lines.push(ConsoleLine::new(line, 1)).unwrap();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.poll();
        drop(pump);
        assert!(audit
            .entries
            .iter()
            .any(|entry| entry.contains("attach accepted")));
        assert!(net
            .sent
            .iter()
            .any(|line| line.as_str().starts_with("OK ATTACH")));
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
            rendered.contains("ERR LOG reason=unauthenticated"),
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
            rendered.contains("ERR LOG reason=unauthenticated"),
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
