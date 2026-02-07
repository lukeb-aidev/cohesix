// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Authority queue for serializing control-plane mutations.
// Author: Lukas Bower
#![cfg(feature = "kernel")]

extern crate alloc;

use alloc::collections::VecDeque;

/// Authority operations that must remain serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityOp {
    /// `/queen/ctl` mutations (spawn/kill/budget).
    QueenCtl,
    /// `/queen/schedule/ctl` mutations.
    ScheduleCtl,
    /// `/queen/lease/ctl` mutations.
    LeaseCtl,
    /// `/queen/export/ctl` mutations.
    ExportCtl,
    /// `/queen/lifecycle/ctl` mutations.
    LifecycleCtl,
    /// `/policy/ctl` mutations.
    PolicyCtl,
    /// `/actions/queue` mutations.
    ActionsQueue,
}

impl AuthorityOp {
    /// Short label for audit logs.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::QueenCtl => "queen-ctl",
            Self::ScheduleCtl => "schedule-ctl",
            Self::LeaseCtl => "lease-ctl",
            Self::ExportCtl => "export-ctl",
            Self::LifecycleCtl => "lifecycle-ctl",
            Self::PolicyCtl => "policy-ctl",
            Self::ActionsQueue => "actions-queue",
        }
    }
}

/// Errors raised by the authority queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityError {
    /// The queue is saturated or already busy.
    Busy,
}

/// Token returned when entering the authority queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorityToken(());

/// Queue that serializes authority mutations.
#[derive(Debug)]
pub struct AuthorityQueue {
    pending: VecDeque<AuthorityOp>,
    max_pending: usize,
    busy: bool,
}

impl AuthorityQueue {
    /// Create a new authority queue.
    #[must_use]
    pub fn new(max_pending: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            max_pending: max_pending.max(1),
            busy: false,
        }
    }

    /// Enter the authority queue, returning a token used to exit.
    pub fn enter(&mut self, op: AuthorityOp) -> Result<AuthorityToken, AuthorityError> {
        if self.busy || self.pending.len() >= self.max_pending {
            return Err(AuthorityError::Busy);
        }
        self.busy = true;
        self.pending.push_back(op);
        Ok(AuthorityToken(()))
    }

    /// Exit the authority queue and clear the active entry.
    pub fn exit(&mut self, _token: AuthorityToken) {
        let _ = self.pending.pop_front();
        self.busy = false;
    }

    /// Inspect the currently active operation, if any.
    #[must_use]
    pub fn active(&self) -> Option<AuthorityOp> {
        self.pending.front().copied()
    }
}
