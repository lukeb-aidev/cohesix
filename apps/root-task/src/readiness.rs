// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the readiness module for root-task.
// Author: Lukas Bower
//! Runtime readiness gates for networking and self-test components.

use core::sync::atomic::{AtomicBool, Ordering};

use heapless::String as HeaplessString;

#[derive(Clone, Copy, Debug, Default)]
pub struct ReadinessSnapshot {
    pub root_ep_ready: bool,
    pub ipc_buffer_installed: bool,
    pub cspace_window_ready: bool,
    pub bootstrap_committed: bool,
}

impl ReadinessSnapshot {
    #[must_use]
    pub const fn ready(self) -> bool {
        self.root_ep_ready
            && self.ipc_buffer_installed
            && self.cspace_window_ready
            && self.bootstrap_committed
    }

    #[must_use]
    pub fn missing_reason(self) -> Option<&'static str> {
        if !self.root_ep_ready {
            return Some("root-ep");
        }
        if !self.ipc_buffer_installed {
            return Some("ipc-buffer");
        }
        if !self.cspace_window_ready {
            return Some("cspace-window");
        }
        if !self.bootstrap_committed {
            return Some("bootstrap-commit");
        }
        None
    }

    #[must_use]
    pub fn render_flags(self) -> HeaplessString<96> {
        let mut line = HeaplessString::<96>::new();
        let _ = core::fmt::write(
            &mut line,
            format_args!(
                "root_ep={} ipc_buf={} cspace={} committed={}",
                self.root_ep_ready as u8,
                self.ipc_buffer_installed as u8,
                self.cspace_window_ready as u8,
                self.bootstrap_committed as u8
            ),
        );
        line
    }
}

static ROOT_EP_READY: AtomicBool = AtomicBool::new(false);
static IPC_BUFFER_INSTALLED: AtomicBool = AtomicBool::new(false);
static CSPACE_WINDOW_READY: AtomicBool = AtomicBool::new(false);
static BOOTSTRAP_COMMITTED: AtomicBool = AtomicBool::new(false);

fn snapshot() -> ReadinessSnapshot {
    ReadinessSnapshot {
        root_ep_ready: ROOT_EP_READY.load(Ordering::Acquire),
        ipc_buffer_installed: IPC_BUFFER_INSTALLED.load(Ordering::Acquire),
        cspace_window_ready: CSPACE_WINDOW_READY.load(Ordering::Acquire),
        bootstrap_committed: BOOTSTRAP_COMMITTED.load(Ordering::Acquire),
    }
}

/// Capture the current readiness snapshot.
#[must_use]
pub fn current() -> ReadinessSnapshot {
    snapshot()
}

/// Return a not-ready reason (if any) along with the current snapshot.
pub fn gate() -> Option<(ReadinessSnapshot, &'static str)> {
    let snap = snapshot();
    snap.missing_reason().map(|reason| (snap, reason))
}

/// Mark the root endpoint as ready.
pub fn mark_root_ep_ready() {
    ROOT_EP_READY.store(true, Ordering::Release);
}

/// Mark the IPC buffer as installed.
pub fn mark_ipc_buffer_installed() {
    IPC_BUFFER_INSTALLED.store(true, Ordering::Release);
}

/// Mark the init CSpace window as validated.
pub fn mark_cspace_window_ready() {
    CSPACE_WINDOW_READY.store(true, Ordering::Release);
}

/// Mark bootstrap as fully committed.
pub fn mark_bootstrap_committed() {
    BOOTSTRAP_COMMITTED.store(true, Ordering::Release);
}

#[cfg(test)]
pub fn reset_for_tests() {
    ROOT_EP_READY.store(false, Ordering::Release);
    IPC_BUFFER_INSTALLED.store(false, Ordering::Release);
    CSPACE_WINDOW_READY.store(false, Ordering::Release);
    BOOTSTRAP_COMMITTED.store(false, Ordering::Release);
}
