// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Deterministic node lifecycle state machine for host NineDoor.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::time::Instant;

/// Lifecycle states exposed via /proc/lifecycle/state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    /// Root-task started, manifest loaded, identity pending.
    Booting,
    /// Dependencies missing; control-plane degraded.
    Degraded,
    /// Full control-plane available.
    Online,
    /// No new work accepted; telemetry ingest continues.
    Draining,
    /// Work drained; safe to reboot or power off.
    Quiesced,
    /// Explicitly disabled or unrecoverable failure.
    Offline,
}

impl LifecycleState {
    /// Render the canonical state label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Booting => "BOOTING",
            Self::Degraded => "DEGRADED",
            Self::Online => "ONLINE",
            Self::Draining => "DRAINING",
            Self::Quiesced => "QUIESCED",
            Self::Offline => "OFFLINE",
        }
    }
}

/// Lifecycle control commands accepted via /queen/lifecycle/ctl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleCommand {
    /// Move from ONLINE/DEGRADED to DRAINING.
    Cordon,
    /// Move from DRAINING to QUIESCED.
    Drain,
    /// Move to ONLINE from any non-ONLINE state.
    Resume,
    /// Move to QUIESCED from ONLINE/DEGRADED/DRAINING.
    Quiesce,
    /// Move to BOOTING from any non-BOOTING state.
    Reset,
}

/// Lifecycle transition errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleError {
    /// Command was not recognised.
    InvalidCommand,
    /// Command is not valid for the current state.
    InvalidTransition,
    /// Auto transition is not configured.
    AutoTransitionDenied,
    /// Transition requires zero outstanding leases.
    OutstandingLeases { leases: usize },
}

/// Transition record for logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleTransition {
    /// Prior state.
    pub from: LifecycleState,
    /// New state.
    pub to: LifecycleState,
    /// Reason string for audit.
    pub reason: &'static str,
}

/// Snapshot of lifecycle state for /proc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleSnapshot {
    /// Current state.
    pub state: LifecycleState,
    /// Current reason.
    pub reason: String,
    /// Milliseconds since the current state began.
    pub since_ms: u64,
}

/// Declarative gate for lifecycle-driven access control.
#[derive(Debug, Clone, Copy)]
pub struct LifecycleGate {
    /// Gate label.
    pub name: &'static str,
    /// Allow in ONLINE.
    pub allow_online: bool,
    /// Allow in DEGRADED.
    pub allow_degraded: bool,
    /// Allow in DRAINING.
    pub allow_draining: bool,
}

impl LifecycleGate {
    /// Return whether the gate allows the supplied state.
    pub fn allows(self, state: LifecycleState) -> bool {
        match state {
            LifecycleState::Online => self.allow_online,
            LifecycleState::Degraded => self.allow_degraded,
            LifecycleState::Draining => self.allow_draining,
            LifecycleState::Booting | LifecycleState::Quiesced | LifecycleState::Offline => false,
        }
    }
}

/// Gate for new work admission (spawn/bind/mount).
pub const GATE_NEW_WORK: LifecycleGate = LifecycleGate {
    name: "new-work",
    allow_online: true,
    allow_degraded: true,
    allow_draining: false,
};

/// Gate for telemetry ingest creation and append.
pub const GATE_TELEMETRY_INGEST: LifecycleGate = LifecycleGate {
    name: "telemetry-ingest",
    allow_online: true,
    allow_degraded: true,
    allow_draining: true,
};

/// Gate for worker attach.
pub const GATE_WORKER_ATTACH: LifecycleGate = LifecycleGate {
    name: "worker-attach",
    allow_online: true,
    allow_degraded: true,
    allow_draining: true,
};

/// Gate for worker telemetry writes.
pub const GATE_WORKER_TELEMETRY: LifecycleGate = LifecycleGate {
    name: "worker-telemetry",
    allow_online: true,
    allow_degraded: true,
    allow_draining: true,
};

/// Gate for worker job submission.
pub const GATE_WORKER_JOB: LifecycleGate = LifecycleGate {
    name: "worker-job",
    allow_online: true,
    allow_degraded: true,
    allow_draining: false,
};

/// Gate for host-side publishes.
pub const GATE_HOST_PUBLISH: LifecycleGate = LifecycleGate {
    name: "host-publish",
    allow_online: true,
    allow_degraded: true,
    allow_draining: false,
};

/// Auto-transition rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleAutoTransition {
    /// Current state required for the transition.
    pub from: LifecycleState,
    /// Target state for the transition.
    pub to: LifecycleState,
}

/// Lifecycle configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleConfig {
    /// Initial lifecycle state.
    pub initial_state: LifecycleState,
    /// Allowed automatic transitions.
    pub auto_transitions: Vec<LifecycleAutoTransition>,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            initial_state: LifecycleState::Booting,
            auto_transitions: vec![LifecycleAutoTransition {
                from: LifecycleState::Booting,
                to: LifecycleState::Online,
            }],
        }
    }
}

/// Lifecycle state machine for host NineDoor.
#[derive(Debug, Clone)]
pub struct LifecycleStateMachine {
    config: LifecycleConfig,
    state: LifecycleState,
    reason: String,
    since: Instant,
}

impl LifecycleStateMachine {
    /// Create a new state machine seeded with default config.
    pub fn new(now: Instant) -> Self {
        let config = LifecycleConfig::default();
        let state = config.initial_state;
        let reason = if state == LifecycleState::Booting {
            "boot"
        } else {
            "manifest"
        };
        Self {
            config,
            state,
            reason: reason.to_owned(),
            since: now,
        }
    }

    /// Return the current state.
    pub fn state(&self) -> LifecycleState {
        self.state
    }

    /// Return a snapshot for /proc.
    pub fn snapshot(&self, now: Instant) -> LifecycleSnapshot {
        let since_ms = now.duration_since(self.since).as_millis() as u64;
        LifecycleSnapshot {
            state: self.state,
            reason: self.reason.clone(),
            since_ms,
        }
    }

    /// Apply a manual control command.
    pub fn apply_command(
        &mut self,
        command: LifecycleCommand,
        now: Instant,
        outstanding_leases: usize,
    ) -> Result<LifecycleTransition, LifecycleError> {
        let from = self.state;
        let (to, reason) = command_target(command);
        if from == to {
            return Err(LifecycleError::InvalidTransition);
        }
        if !command_allowed(from, command) {
            return Err(LifecycleError::InvalidTransition);
        }
        if matches!(
            command,
            LifecycleCommand::Drain | LifecycleCommand::Quiesce | LifecycleCommand::Reset
        ) && outstanding_leases > 0
        {
            return Err(LifecycleError::OutstandingLeases {
                leases: outstanding_leases,
            });
        }
        self.state = to;
        self.reason.clear();
        self.reason.push_str(reason);
        self.since = now;
        Ok(LifecycleTransition { from, to, reason })
    }

    /// Apply the default boot-complete auto transition.
    pub fn auto_boot_complete(
        &mut self,
        now: Instant,
    ) -> Result<LifecycleTransition, LifecycleError> {
        self.apply_auto_transition(LifecycleState::Online, "boot-complete", now)
    }

    /// Apply an auto transition when configured.
    pub fn apply_auto_transition(
        &mut self,
        target: LifecycleState,
        reason: &'static str,
        now: Instant,
    ) -> Result<LifecycleTransition, LifecycleError> {
        let from = self.state;
        if from == target {
            return Err(LifecycleError::InvalidTransition);
        }
        if !auto_transition_allowed(&self.config, from, target) {
            return Err(LifecycleError::AutoTransitionDenied);
        }
        self.state = target;
        self.reason.clear();
        self.reason.push_str(reason);
        self.since = now;
        Ok(LifecycleTransition {
            from,
            to: target,
            reason,
        })
    }

    /// Return whether a gate permits the current state.
    pub fn gate_allows(&self, gate: LifecycleGate) -> bool {
        gate.allows(self.state)
    }
}

/// Parse a lifecycle control command.
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

/// Format a transition log line.
pub fn format_transition_log(transition: &LifecycleTransition) -> String {
    format!(
        "lifecycle transition old={} new={} reason={}",
        transition.from.as_str(),
        transition.to.as_str(),
        transition.reason
    )
}

/// Format a denied transition or command log line.
pub fn format_denied_log(state: LifecycleState, action: &str, error: LifecycleError) -> String {
    match error {
        LifecycleError::OutstandingLeases { leases } => format!(
            "lifecycle denied action={} state={} reason=outstanding-leases leases={}",
            action,
            state.as_str(),
            leases
        ),
        LifecycleError::InvalidCommand => format!(
            "lifecycle denied action={} state={} reason=invalid-command",
            action,
            state.as_str()
        ),
        LifecycleError::AutoTransitionDenied => format!(
            "lifecycle denied action={} state={} reason=auto-transition-denied",
            action,
            state.as_str()
        ),
        LifecycleError::InvalidTransition => format!(
            "lifecycle denied action={} state={} reason=invalid-transition",
            action,
            state.as_str()
        ),
    }
}

/// Format a gate denial log line.
pub fn format_gate_denied_log(state: LifecycleState, action: &str) -> String {
    format!(
        "lifecycle denied action={} state={} reason=gate-denied",
        action,
        state.as_str()
    )
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
        LifecycleCommand::Cordon => {
            matches!(state, LifecycleState::Online | LifecycleState::Degraded)
        }
        LifecycleCommand::Drain => matches!(state, LifecycleState::Draining),
        LifecycleCommand::Resume => !matches!(state, LifecycleState::Online),
        LifecycleCommand::Quiesce => matches!(
            state,
            LifecycleState::Online | LifecycleState::Degraded | LifecycleState::Draining
        ),
        LifecycleCommand::Reset => !matches!(state, LifecycleState::Booting),
    }
}

fn auto_transition_allowed(
    config: &LifecycleConfig,
    from: LifecycleState,
    to: LifecycleState,
) -> bool {
    config
        .auto_transitions
        .iter()
        .any(|transition| transition.from == from && transition.to == to)
}
