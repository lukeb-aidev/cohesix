// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the bootstrap/state module for root-task.
// Author: Lukas Bower
//! Bootstrap run-state guard rails to enforce single-shot execution and phase ordering.

#![allow(dead_code)]

use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use heapless::String as HeaplessString;

use crate::bootstrap::log as boot_log;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum BootstrapRunState {
    Cold = 0,
    Running = 1,
    Committed = 2,
    Aborted = 3,
}

impl BootstrapRunState {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Running => "running",
            Self::Committed => "committed",
            Self::Aborted => "aborted",
        }
    }
}

static BOOTSTRAP_ATTEMPTED: AtomicBool = AtomicBool::new(false);
static BOOTSTRAP_STATE: AtomicU32 = AtomicU32::new(BootstrapRunState::Cold as u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapReentry {
    AlreadyAttempted(BootstrapRunState),
}

fn state_from_raw(raw: u32) -> BootstrapRunState {
    match raw {
        1 => BootstrapRunState::Running,
        2 => BootstrapRunState::Committed,
        3 => BootstrapRunState::Aborted,
        _ => BootstrapRunState::Cold,
    }
}

fn log_blocked_state(caller: &str, state: BootstrapRunState) {
    let mut line = HeaplessString::<96>::new();
    let _ = write!(
        line,
        "[boot] phase advance blocked: state={} caller={caller}",
        state.label()
    );
    boot_log::force_uart_line(line.as_str());
}

/// Attempt to transition from `Cold` to `Running`, guarding against re-entry.
pub fn enter_once(caller: &'static str) -> Result<(), BootstrapReentry> {
    let already_attempted = BOOTSTRAP_ATTEMPTED.swap(true, Ordering::AcqRel);
    let state_now = state();
    if already_attempted || state_now != BootstrapRunState::Cold {
        log_blocked_state(caller, state_now);
        return Err(BootstrapReentry::AlreadyAttempted(state_now));
    }

    BOOTSTRAP_STATE.store(BootstrapRunState::Running as u32, Ordering::Release);
    Ok(())
}

/// Return the current bootstrap run-state.
#[must_use]
pub fn state() -> BootstrapRunState {
    state_from_raw(BOOTSTRAP_STATE.load(Ordering::Acquire))
}

/// Mark the bootstrap as irrecoverably aborted.
pub fn mark_aborted() {
    BOOTSTRAP_STATE.store(BootstrapRunState::Aborted as u32, Ordering::Release);
}

/// Mark the bootstrap as successfully committed.
pub fn mark_committed() {
    BOOTSTRAP_STATE.store(BootstrapRunState::Committed as u32, Ordering::Release);
}

/// Return `true` when phase transitions remain permitted.
pub fn phase_mutable(caller: &'static str) -> bool {
    let state_now = state();
    if matches!(
        state_now,
        BootstrapRunState::Committed | BootstrapRunState::Aborted
    ) {
        log_blocked_state(caller, state_now);
        return false;
    }
    true
}

/// Reset bootstrap run-state (testing hooks only).
#[cfg(test)]
pub fn reset_for_tests() {
    BOOTSTRAP_ATTEMPTED.store(false, Ordering::Release);
    BOOTSTRAP_STATE.store(BootstrapRunState::Cold as u32, Ordering::Release);
}
