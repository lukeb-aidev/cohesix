// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Track per-session lifecycle state for Secure9P observability.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::time::Instant;

use super::lifecycle::LifecycleState;

/// Explicit session lifecycle states for `/proc/9p/session/*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    /// Session handshake in progress (pre-attach).
    Setup,
    /// Session attached and active.
    Active,
    /// Session attached but draining control actions.
    Draining,
    /// Session closed or revoked.
    Closed,
}

impl SessionPhase {
    /// Render the canonical state label.
    pub fn as_str(self) -> &'static str {
        match self {
            SessionPhase::Setup => "SETUP",
            SessionPhase::Active => "ACTIVE",
            SessionPhase::Draining => "DRAINING",
            SessionPhase::Closed => "CLOSED",
        }
    }
}

/// Session lifecycle tracker for observability.
#[derive(Debug, Clone)]
pub struct SessionLifecycle {
    phase: SessionPhase,
    since: Instant,
    owner: Option<String>,
}

impl SessionLifecycle {
    /// Create a new lifecycle tracker in SETUP state.
    pub fn new(now: Instant) -> Self {
        Self {
            phase: SessionPhase::Setup,
            since: now,
            owner: None,
        }
    }

    /// Return the current phase.
    pub fn phase(&self) -> SessionPhase {
        self.phase
    }

    /// Return the current owner label, if attached.
    pub fn owner(&self) -> Option<&str> {
        self.owner.as_deref()
    }

    /// Return elapsed milliseconds since the current phase began.
    pub fn since_ms(&self, now: Instant) -> u64 {
        now.duration_since(self.since).as_millis() as u64
    }

    /// Mark the session active with an optional owner label.
    pub fn mark_active(&mut self, now: Instant, owner: Option<String>) {
        if self.phase == SessionPhase::Closed {
            return;
        }
        self.phase = SessionPhase::Active;
        self.since = now;
        if owner.is_some() {
            self.owner = owner;
        }
    }

    /// Mark the session closed.
    pub fn mark_closed(&mut self, now: Instant) {
        if self.phase == SessionPhase::Closed {
            return;
        }
        self.phase = SessionPhase::Closed;
        self.since = now;
    }

    /// Update the phase for lifecycle-driven draining semantics.
    pub fn refresh_for_lifecycle(&mut self, state: LifecycleState, now: Instant) {
        if matches!(self.phase, SessionPhase::Closed | SessionPhase::Setup) {
            return;
        }
        let draining = matches!(state, LifecycleState::Draining);
        match (self.phase, draining) {
            (SessionPhase::Active, true) => {
                self.phase = SessionPhase::Draining;
                self.since = now;
            }
            (SessionPhase::Draining, false) => {
                self.phase = SessionPhase::Active;
                self.since = now;
            }
            _ => {}
        }
    }
}
