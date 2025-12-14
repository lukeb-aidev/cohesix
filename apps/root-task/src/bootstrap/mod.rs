// Author: Lukas Bower
#![allow(dead_code)]

use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};

use heapless::String;
use spin::Mutex;

use crate::sel4::BootInfo;
#[cfg(target_os = "none")]
use crate::sel4::BootInfoView;

/// BootInfo snapshotting and canary validation helpers.
pub mod bootinfo_snapshot;
/// Capability-space helpers extracted from the seL4 boot info structure.
pub mod cspace;
/// Slot encoding helpers for bootstrap-only capability syscalls.
pub mod cspace_encode;
/// Syscall wrappers for capability operations using invocation addressing only.
pub mod cspace_sys;
/// ABI guard helpers ensuring the seL4 FFI signatures remain pinned.
pub mod ffi;
/// Fail-fast invariants for the bootstrap guard rails.
pub mod hard_guard;
/// IPC buffer bring-up helpers with deterministic logging waypoints.
pub mod ipcbuf;
/// Single-page view wrapper for the init thread IPC buffer.
pub mod ipcbuf_view;
/// Linker-derived layout diagnostics for early boot.
pub mod layout;
/// Early boot logging backends.
pub mod log;
/// Strict phase tracking and invariant enforcement for bootstrap.
pub mod phases;
/// Thin wrapper around `seL4_Untyped_Retype` tailored for the init CSpace policy.
pub mod retype;
/// Helpers for selecting RAM-backed untyped capabilities during bootstrap.
pub mod untyped_pick;

pub use untyped_pick::{
    device_pt_pool, ensure_device_pt_pool, pick_untyped, DevicePtPoolConfig, RetypePlan,
    UntypedSelection,
};

#[cfg(feature = "untyped-debug")]
pub mod untyped;
#[cfg(not(feature = "untyped-debug"))]
mod untyped_stub {}

/// Enumerates the significant waypoints encountered during bootstrap. Each
/// transition is surfaced directly via the UART, independent of the endpoint
/// log transport state.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BootPhase {
    Begin,
    CSpaceInit,
    UntypedEnumerate,
    RetypeBegin,
    RetypeProgress { done: u32, total: u32 },
    RetypeDone,
    DTBParseDeferred,
    DTBParseDone,
    EPAttachWait,
    EPAttachOk,
    HandOff,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct BootTracerState {
    phase: BootPhase,
    last_slot: Option<u32>,
    progress_done: u32,
    progress_total: u32,
}

impl BootTracerState {
    const fn new() -> Self {
        Self {
            phase: BootPhase::Begin,
            last_slot: None,
            progress_done: 0,
            progress_total: 0,
        }
    }
}

impl Default for BootTracerState {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of the boot tracer state consumed by watchdog logic.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BootSnapshot {
    pub phase: BootPhase,
    pub sequence: u64,
    pub last_slot: Option<u32>,
    pub progress_done: u32,
    pub progress_total: u32,
}

impl BootSnapshot {
    #[must_use]
    pub const fn new(
        phase: BootPhase,
        sequence: u64,
        last_slot: Option<u32>,
        progress_done: u32,
        progress_total: u32,
    ) -> Self {
        Self {
            phase,
            sequence,
            last_slot,
            progress_done,
            progress_total,
        }
    }
}

/// UART-backed tracer emitting monotonic beacons during bootstrap.
pub struct BootTracer {
    state: Mutex<BootTracerState>,
    sequence: AtomicU64,
}

impl BootTracer {
    const MAX_LINE: usize = 96;

    const fn new() -> Self {
        Self {
            state: Mutex::new(BootTracerState::new()),
            sequence: AtomicU64::new(0),
        }
    }

    fn render_phase(phase: BootPhase, done: u32, total: u32) -> String<{ Self::MAX_LINE }> {
        let mut line = String::<{ Self::MAX_LINE }>::new();
        match phase {
            BootPhase::RetypeProgress { .. } => {
                let _ = fmt::write(
                    &mut line,
                    format_args!("[boot:phase] RetypeProgress {done}/{total}"),
                );
            }
            _ => {
                let name = match phase {
                    BootPhase::Begin => "Begin",
                    BootPhase::CSpaceInit => "CSpaceInit",
                    BootPhase::UntypedEnumerate => "UntypedEnumerate",
                    BootPhase::RetypeBegin => "RetypeBegin",
                    BootPhase::RetypeProgress { .. } => unreachable!(),
                    BootPhase::RetypeDone => "RetypeDone",
                    BootPhase::DTBParseDeferred => "DTBParseDeferred",
                    BootPhase::DTBParseDone => "DTBParseDone",
                    BootPhase::EPAttachWait => "EPAttachWait",
                    BootPhase::EPAttachOk => "EPAttachOk",
                    BootPhase::HandOff => "HandOff",
                };
                let _ = fmt::write(&mut line, format_args!("[boot:phase] {name}"));
            }
        }
        line
    }

    /// Advances the tracer to the supplied phase, updating diagnostics and
    /// emitting a UART beacon describing the transition.
    pub fn advance(&self, phase: BootPhase) {
        let mut progress_done = 0u32;
        let mut progress_total = 0u32;
        if let Some(mut guard) = self.state.try_lock() {
            guard.phase = phase;
            if let BootPhase::RetypeProgress { done, total } = phase {
                guard.progress_done = done;
                guard.progress_total = total;
            }
            progress_done = guard.progress_done;
            progress_total = guard.progress_total;
        }
        let _ = self.sequence.fetch_add(1, Ordering::AcqRel) + 1;
        let line = Self::render_phase(phase, progress_done, progress_total);
        crate::bootstrap::log::force_uart_line(line.as_str());
    }

    /// Records the most recent CSpace slot touched while retyping.
    pub fn record_slot(&self, slot: u32) {
        if let Some(mut guard) = self.state.try_lock() {
            guard.last_slot = Some(slot);
        }
        let _ = self.sequence.fetch_add(1, Ordering::AcqRel) + 1;
    }

    /// Returns a snapshot of the current boot progress state for watchdogs.
    #[must_use]
    pub fn snapshot(&self) -> BootSnapshot {
        if let Some(guard) = self.state.try_lock() {
            BootSnapshot::new(
                guard.phase,
                self.sequence.load(Ordering::Acquire),
                guard.last_slot,
                guard.progress_done,
                guard.progress_total,
            )
        } else {
            BootSnapshot::new(
                BootPhase::Begin,
                self.sequence.load(Ordering::Acquire),
                None,
                0,
                0,
            )
        }
    }
}

static BOOT_TRACER: BootTracer = BootTracer::new();

/// Returns the singleton boot tracer.
#[must_use]
pub fn boot_tracer() -> &'static BootTracer {
    &BOOT_TRACER
}

#[macro_export]
/// Emits a bootstrapping progress marker prefixed with `[boot]`.
macro_rules! bp {
    ($name:expr) => {
        ::log::info!(concat!("[boot] ", $name));
    };
}

#[inline(always)]
/// Helper returning an error when the supplied seL4 return code represents failure.
pub fn ktry(step: &str, rc: i32) -> Result<(), i32> {
    if rc != sel4_sys::seL4_NoError as i32 {
        ::log::error!("[boot] {step}: seL4 err={rc}");
        return Err(rc);
    }
    Ok(())
}

#[cfg(test)]
pub mod tests {
    pub mod cspace_math;
    pub mod retype_args;
}

/// Emit the final bootstrap beacons and return immediately to unblock the console handoff.
pub fn run_minimal(bootinfo: &'static BootInfo) {
    // *** MINIMAL BYPASS FOR DIAG ***
    crate::bootstrap::log::force_uart_line("[BOOT] run.min.start");
    ::log::info!("[boot] run.minimal.begin");

    #[cfg(target_os = "none")]
    {
        match BootInfoView::new(bootinfo) {
            Ok(view) => {
                let (empty_start, empty_end) = view.init_cnode_empty_range();
                let window = crate::bootstrap::cspace::CSpaceWindow::new(
                    view.root_cnode_cap(),
                    view.canonical_root_cap(),
                    crate::bootstrap::cspace_sys::bits_as_u8(usize::from(view.init_cnode_bits())),
                    empty_start,
                    empty_end,
                    empty_start,
                );
                ::log::info!(
                    "[cs: win] root=0x{root:04x} bits={bits} first_free=0x{slot:04x}",
                    root = window.root,
                    bits = window.bits,
                    slot = window.first_free,
                );
            }
            Err(err) => {
                ::log::error!("[boot] bootinfo view error: {err}");
            }
        }
    }

    #[cfg(not(target_os = "none"))]
    let _ = bootinfo;

    ::log::info!("[boot] run.minimal.end");
    crate::bootstrap::log::force_uart_line("[BOOT] run.min.end");
}
