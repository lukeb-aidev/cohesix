// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Deterministic node lifecycle state machine for root-task.
// Author: Lukas Bower
#![cfg(feature = "kernel")]

use core::fmt::Write;
use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};

use heapless::String as HeaplessString;

use crate::generated::{self, LifecycleState};

const REASON_CAP: usize = 96;
const LOG_LINE_CAP: usize = 160;
const ROOT_FLAG_NETWORK: u8 = 1;
const ROOT_FLAG_POLICY: u8 = 1 << 1;
const ROOT_FLAG_REVOKED: u8 = 1 << 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleCommand {
    Cordon,
    Drain,
    Resume,
    Quiesce,
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleError {
    InvalidCommand,
    InvalidTransition,
    AutoTransitionDenied,
    OutstandingLeases { leases: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LifecycleTransition {
    pub from: LifecycleState,
    pub to: LifecycleState,
    pub reason: &'static str,
}

#[derive(Clone, Debug)]
pub struct LifecycleSnapshot {
    pub state: LifecycleState,
    pub reason: HeaplessString<REASON_CAP>,
    pub since_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootCutReason {
    None,
    NetworkUnreachable,
    SessionRevoked,
    PolicyDenied,
    LifecycleOffline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RootSnapshot {
    pub reachable: bool,
    pub last_seen_ms: u64,
    pub cut_reason: RootCutReason,
}

#[derive(Clone, Copy, Debug)]
pub struct LifecycleGate {
    pub name: &'static str,
    pub allow_online: bool,
    pub allow_degraded: bool,
    pub allow_draining: bool,
}

impl LifecycleGate {
    pub const fn allows(self, state: LifecycleState) -> bool {
        match state {
            LifecycleState::Online => self.allow_online,
            LifecycleState::Degraded => self.allow_degraded,
            LifecycleState::Draining => self.allow_draining,
            LifecycleState::Booting | LifecycleState::Quiesced | LifecycleState::Offline => false,
        }
    }
}

pub const GATE_NEW_WORK: LifecycleGate = LifecycleGate {
    name: "new-work",
    allow_online: true,
    allow_degraded: true,
    allow_draining: false,
};

pub const GATE_TELEMETRY_INGEST: LifecycleGate = LifecycleGate {
    name: "telemetry-ingest",
    allow_online: true,
    allow_degraded: true,
    allow_draining: true,
};

pub const GATE_WORKER_ATTACH: LifecycleGate = LifecycleGate {
    name: "worker-attach",
    allow_online: true,
    allow_degraded: true,
    allow_draining: true,
};

pub const GATE_WORKER_TELEMETRY: LifecycleGate = LifecycleGate {
    name: "worker-telemetry",
    allow_online: true,
    allow_degraded: true,
    allow_draining: true,
};

pub const GATE_WORKER_JOB: LifecycleGate = LifecycleGate {
    name: "worker-job",
    allow_online: true,
    allow_degraded: true,
    allow_draining: false,
};

pub const GATE_HOST_PUBLISH: LifecycleGate = LifecycleGate {
    name: "host-publish",
    allow_online: true,
    allow_degraded: true,
    allow_draining: false,
};

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LifecycleReason {
    Boot = 0,
    Manifest = 1,
    BootComplete = 2,
    Cordon = 3,
    Drain = 4,
    Resume = 5,
    Quiesce = 6,
    Reset = 7,
    Unknown = 255,
}

static LIFECYCLE_STATE: AtomicU8 = AtomicU8::new(LifecycleState::Booting as u8);
static LIFECYCLE_REASON: AtomicU8 = AtomicU8::new(LifecycleReason::Boot as u8);
static LIFECYCLE_SINCE_MS: AtomicU64 = AtomicU64::new(0);
static ROOT_SESSION_ACTIVE: AtomicU8 = AtomicU8::new(0);
static ROOT_LAST_SEEN_MS: AtomicU64 = AtomicU64::new(0);
static ROOT_CUT_FLAGS: AtomicU8 = AtomicU8::new(0);

pub fn init(now_ms: u64) -> LifecycleSnapshot {
    let config = generated::lifecycle_config();
    let state = config.initial_state;
    let reason = if state == LifecycleState::Booting {
        LifecycleReason::Boot
    } else {
        LifecycleReason::Manifest
    };
    store_state(state, now_ms, reason);
    snapshot()
}

pub fn snapshot() -> LifecycleSnapshot {
    let state = state_from_u8(LIFECYCLE_STATE.load(Ordering::SeqCst));
    let reason = reason_from_u8(LIFECYCLE_REASON.load(Ordering::SeqCst));
    let since_ms = LIFECYCLE_SINCE_MS.load(Ordering::SeqCst);
    let mut reason_buf = HeaplessString::new();
    let _ = reason_buf.push_str(reason_label(reason));
    LifecycleSnapshot {
        state,
        reason: reason_buf,
        since_ms,
    }
}

pub fn root_snapshot() -> RootSnapshot {
    let lifecycle_state = state();
    let session_active = ROOT_SESSION_ACTIVE.load(Ordering::SeqCst) != 0;
    let last_seen_ms = ROOT_LAST_SEEN_MS.load(Ordering::SeqCst);
    let flags = ROOT_CUT_FLAGS.load(Ordering::SeqCst);
    let offline = matches!(lifecycle_state, LifecycleState::Offline);
    let reachable = session_active && !offline;
    let cut_reason = if reachable {
        RootCutReason::None
    } else if offline {
        RootCutReason::LifecycleOffline
    } else if flags & ROOT_FLAG_REVOKED != 0 {
        RootCutReason::SessionRevoked
    } else if flags & ROOT_FLAG_POLICY != 0 {
        RootCutReason::PolicyDenied
    } else if flags & ROOT_FLAG_NETWORK != 0 {
        RootCutReason::NetworkUnreachable
    } else {
        RootCutReason::NetworkUnreachable
    };
    RootSnapshot {
        reachable,
        last_seen_ms,
        cut_reason,
    }
}

pub fn root_mark_session_active(now_ms: u64) {
    ROOT_SESSION_ACTIVE.store(1, Ordering::SeqCst);
    ROOT_LAST_SEEN_MS.store(now_ms, Ordering::SeqCst);
    ROOT_CUT_FLAGS.store(0, Ordering::SeqCst);
}

pub fn root_record_activity(now_ms: u64) {
    if ROOT_SESSION_ACTIVE.load(Ordering::SeqCst) != 0 {
        ROOT_LAST_SEEN_MS.store(now_ms, Ordering::SeqCst);
    }
}

pub fn root_mark_cut(reason: RootCutReason) {
    ROOT_SESSION_ACTIVE.store(0, Ordering::SeqCst);
    match reason {
        RootCutReason::NetworkUnreachable => {
            ROOT_CUT_FLAGS.fetch_or(ROOT_FLAG_NETWORK, Ordering::SeqCst);
        }
        RootCutReason::SessionRevoked => {
            ROOT_CUT_FLAGS.fetch_or(ROOT_FLAG_REVOKED, Ordering::SeqCst);
        }
        RootCutReason::PolicyDenied => {
            ROOT_CUT_FLAGS.fetch_or(ROOT_FLAG_POLICY, Ordering::SeqCst);
        }
        RootCutReason::LifecycleOffline | RootCutReason::None => {}
    }
}

pub fn root_mark_policy_denied() {
    if ROOT_SESSION_ACTIVE.load(Ordering::SeqCst) == 0 {
        ROOT_CUT_FLAGS.fetch_or(ROOT_FLAG_POLICY, Ordering::SeqCst);
    }
}

pub fn state() -> LifecycleState {
    state_from_u8(LIFECYCLE_STATE.load(Ordering::SeqCst))
}

pub fn parse_command(line: &str) -> Result<LifecycleCommand, LifecycleError> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err(LifecycleError::InvalidCommand);
    }
    if trimmed.eq_ignore_ascii_case("cordon") {
        Ok(LifecycleCommand::Cordon)
    } else if trimmed.eq_ignore_ascii_case("drain") {
        Ok(LifecycleCommand::Drain)
    } else if trimmed.eq_ignore_ascii_case("resume") {
        Ok(LifecycleCommand::Resume)
    } else if trimmed.eq_ignore_ascii_case("quiesce") {
        Ok(LifecycleCommand::Quiesce)
    } else if trimmed.eq_ignore_ascii_case("reset") {
        Ok(LifecycleCommand::Reset)
    } else {
        Err(LifecycleError::InvalidCommand)
    }
}

pub fn apply_command(
    command: LifecycleCommand,
    now_ms: u64,
    outstanding_leases: usize,
) -> Result<LifecycleTransition, LifecycleError> {
    let from = state();
    let (to, reason) = command_target(command);
    if from == to {
        return Err(LifecycleError::InvalidTransition);
    }
    if !command_allowed(from, command) {
        return Err(LifecycleError::InvalidTransition);
    }
    if matches!(command, LifecycleCommand::Drain | LifecycleCommand::Quiesce | LifecycleCommand::Reset)
        && outstanding_leases > 0
    {
        return Err(LifecycleError::OutstandingLeases {
            leases: outstanding_leases,
        });
    }
    store_state(to, now_ms, reason_from_str(reason));
    Ok(LifecycleTransition { from, to, reason })
}

pub fn auto_boot_complete(now_ms: u64) -> Result<LifecycleTransition, LifecycleError> {
    apply_auto_transition(LifecycleState::Online, "boot-complete", now_ms)
}

pub fn apply_auto_transition(
    target: LifecycleState,
    reason: &'static str,
    now_ms: u64,
) -> Result<LifecycleTransition, LifecycleError> {
    let from = state();
    if from == target {
        return Err(LifecycleError::InvalidTransition);
    }
    if !auto_transition_allowed(generated::lifecycle_config(), from, target) {
        return Err(LifecycleError::AutoTransitionDenied);
    }
    store_state(target, now_ms, reason_from_str(reason));
    Ok(LifecycleTransition { from, to: target, reason })
}

pub fn gate_allows(gate: LifecycleGate) -> bool {
    gate.allows(state())
}

pub fn format_transition_log(transition: &LifecycleTransition) -> HeaplessString<LOG_LINE_CAP> {
    let mut line = HeaplessString::new();
    let _ = write!(
        line,
        "lifecycle transition old={} new={} reason={}",
        state_label(transition.from),
        state_label(transition.to),
        transition.reason
    );
    line
}

pub fn format_denied_log(
    state: LifecycleState,
    action: &str,
    error: LifecycleError,
) -> HeaplessString<LOG_LINE_CAP> {
    let mut line = HeaplessString::new();
    let _ = match error {
        LifecycleError::OutstandingLeases { leases } => write!(
            line,
            "lifecycle denied action={} state={} reason=outstanding-leases leases={}",
            action,
            state_label(state),
            leases
        ),
        LifecycleError::InvalidCommand => write!(
            line,
            "lifecycle denied action={} state={} reason=invalid-command",
            action,
            state_label(state)
        ),
        LifecycleError::AutoTransitionDenied => write!(
            line,
            "lifecycle denied action={} state={} reason=auto-transition-denied",
            action,
            state_label(state)
        ),
        LifecycleError::InvalidTransition => write!(
            line,
            "lifecycle denied action={} state={} reason=invalid-transition",
            action,
            state_label(state)
        ),
    };
    line
}

fn store_state(state: LifecycleState, since_ms: u64, reason: LifecycleReason) {
    LIFECYCLE_STATE.store(state as u8, Ordering::SeqCst);
    LIFECYCLE_REASON.store(reason as u8, Ordering::SeqCst);
    LIFECYCLE_SINCE_MS.store(since_ms, Ordering::SeqCst);
}

fn state_from_u8(value: u8) -> LifecycleState {
    match value {
        x if x == LifecycleState::Booting as u8 => LifecycleState::Booting,
        x if x == LifecycleState::Degraded as u8 => LifecycleState::Degraded,
        x if x == LifecycleState::Online as u8 => LifecycleState::Online,
        x if x == LifecycleState::Draining as u8 => LifecycleState::Draining,
        x if x == LifecycleState::Quiesced as u8 => LifecycleState::Quiesced,
        x if x == LifecycleState::Offline as u8 => LifecycleState::Offline,
        _ => LifecycleState::Booting,
    }
}

fn reason_from_u8(value: u8) -> LifecycleReason {
    match value {
        x if x == LifecycleReason::Boot as u8 => LifecycleReason::Boot,
        x if x == LifecycleReason::Manifest as u8 => LifecycleReason::Manifest,
        x if x == LifecycleReason::BootComplete as u8 => LifecycleReason::BootComplete,
        x if x == LifecycleReason::Cordon as u8 => LifecycleReason::Cordon,
        x if x == LifecycleReason::Drain as u8 => LifecycleReason::Drain,
        x if x == LifecycleReason::Resume as u8 => LifecycleReason::Resume,
        x if x == LifecycleReason::Quiesce as u8 => LifecycleReason::Quiesce,
        x if x == LifecycleReason::Reset as u8 => LifecycleReason::Reset,
        _ => LifecycleReason::Unknown,
    }
}

fn reason_from_str(value: &str) -> LifecycleReason {
    match value {
        "boot" => LifecycleReason::Boot,
        "manifest" => LifecycleReason::Manifest,
        "boot-complete" => LifecycleReason::BootComplete,
        "cordon" => LifecycleReason::Cordon,
        "drain" => LifecycleReason::Drain,
        "resume" => LifecycleReason::Resume,
        "quiesce" => LifecycleReason::Quiesce,
        "reset" => LifecycleReason::Reset,
        _ => LifecycleReason::Unknown,
    }
}

fn reason_label(reason: LifecycleReason) -> &'static str {
    match reason {
        LifecycleReason::Boot => "boot",
        LifecycleReason::Manifest => "manifest",
        LifecycleReason::BootComplete => "boot-complete",
        LifecycleReason::Cordon => "cordon",
        LifecycleReason::Drain => "drain",
        LifecycleReason::Resume => "resume",
        LifecycleReason::Quiesce => "quiesce",
        LifecycleReason::Reset => "reset",
        LifecycleReason::Unknown => "unknown",
    }
}

fn command_target(command: LifecycleCommand) -> (LifecycleState, &'static str) {
    match command {
        LifecycleCommand::Cordon => (LifecycleState::Draining, "cordon"),
        LifecycleCommand::Drain => (LifecycleState::Quiesced, "drain"),
        LifecycleCommand::Resume => (LifecycleState::Online, "resume"),
        LifecycleCommand::Quiesce => (LifecycleState::Quiesced, "quiesce"),
        LifecycleCommand::Reset => (LifecycleState::Booting, "reset"),
    }
}

fn command_allowed(state: LifecycleState, command: LifecycleCommand) -> bool {
    match command {
        LifecycleCommand::Cordon => matches!(state, LifecycleState::Online | LifecycleState::Degraded),
        LifecycleCommand::Drain => matches!(state, LifecycleState::Draining),
        LifecycleCommand::Resume => !matches!(state, LifecycleState::Online),
        LifecycleCommand::Quiesce => {
            matches!(state, LifecycleState::Online | LifecycleState::Degraded | LifecycleState::Draining)
        }
        LifecycleCommand::Reset => !matches!(state, LifecycleState::Booting),
    }
}

fn auto_transition_allowed(
    config: generated::LifecycleConfig,
    from: LifecycleState,
    to: LifecycleState,
) -> bool {
    config
        .auto_transitions
        .iter()
        .any(|transition| transition.from == from && transition.to == to)
}

pub fn state_label(state: LifecycleState) -> &'static str {
    match state {
        LifecycleState::Booting => "BOOTING",
        LifecycleState::Degraded => "DEGRADED",
        LifecycleState::Online => "ONLINE",
        LifecycleState::Draining => "DRAINING",
        LifecycleState::Quiesced => "QUIESCED",
        LifecycleState::Offline => "OFFLINE",
    }
}

pub fn root_cut_reason_label(reason: RootCutReason) -> &'static str {
    match reason {
        RootCutReason::None => "none",
        RootCutReason::NetworkUnreachable => "network_unreachable",
        RootCutReason::SessionRevoked => "session_revoked",
        RootCutReason::PolicyDenied => "policy_denied",
        RootCutReason::LifecycleOffline => "lifecycle_offline",
    }
}
