// Author: Lukas Bower
//! Linear bootstrap phase tracking and invariant enforcement.
#![allow(dead_code)]

use core::fmt::Write;

use heapless::String;

use crate::bootstrap::log as boot_log;
use crate::sel4::{BootInfo, BootInfoView};

/// Fatal bootstrap error surfaced when invariants are violated.
#[derive(Debug, Clone)]
pub struct FatalBootstrapError {
    message: String<160>,
}

impl FatalBootstrapError {
    fn new(message: String<160>) -> Self {
        Self { message }
    }

    /// Returns the captured error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl core::fmt::Display for FatalBootstrapError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.message.as_str())
    }
}

/// Bootstrap phases executed exactly once in order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapPhase {
    CSpaceCanonicalise,
    BootInfoValidate,
    MemoryLayoutBuild,
    UntypedPlan,
    RetypeCommit,
    IPCInstall,
    UserlandHandoff,
}

impl BootstrapPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CSpaceCanonicalise => "CSpaceCanonicalise",
            Self::BootInfoValidate => "BootInfoValidate",
            Self::MemoryLayoutBuild => "MemoryLayoutBuild",
            Self::UntypedPlan => "UntypedPlan",
            Self::RetypeCommit => "RetypeCommit",
            Self::IPCInstall => "IPCInstall",
            Self::UserlandHandoff => "UserlandHandoff",
        }
    }
}

const ORDERING: &[BootstrapPhase] = &[
    BootstrapPhase::CSpaceCanonicalise,
    BootstrapPhase::BootInfoValidate,
    BootstrapPhase::MemoryLayoutBuild,
    BootstrapPhase::UntypedPlan,
    BootstrapPhase::RetypeCommit,
    BootstrapPhase::IPCInstall,
    BootstrapPhase::UserlandHandoff,
];

/// Tracks bootstrap progress and rejects re-entry or phase reordering.
pub struct BootstrapSequencer {
    next: usize,
}

impl BootstrapSequencer {
    /// Constructs a new sequencer positioned before the first phase.
    #[must_use]
    pub const fn new() -> Self {
        Self { next: 0 }
    }

    fn expect_next(&self, phase: BootstrapPhase) -> Result<(), FatalBootstrapError> {
        if self.next >= ORDERING.len() {
            return Err(FatalBootstrapError::new(String::from(
                "bootstrap phase advance attempted after completion",
            )));
        }

        if ORDERING[self.next] != phase {
            let mut msg = String::<160>::from("bootstrap phase order violation: expected ");
            let _ = write!(&mut msg, "{}", ORDERING[self.next].as_str());
            let _ = write!(&mut msg, ", saw {}", phase.as_str());
            return Err(FatalBootstrapError::new(msg));
        }

        Ok(())
    }

    /// Marks the supplied phase as executed, emitting a UART beacon.
    pub fn advance(&mut self, phase: BootstrapPhase) -> Result<(), FatalBootstrapError> {
        self.expect_next(phase)?;
        crate::bootstrap::log::force_uart_line(phase.as_str());
        self.next += 1;
        Ok(())
    }

    /// Validates invariants that must hold for the init CSpace window.
    pub fn validate_bootinfo(&mut self, view: &BootInfoView) -> Result<(), FatalBootstrapError> {
        boot_log::force_uart_line("[mark] bootinfo.validate.begin");
        self.advance(BootstrapPhase::BootInfoValidate)?;

        let init_bits = view.init_cnode_bits() as usize;
        let guard_bits: usize = 0;
        if init_bits > sel4_sys::seL4_WordBits as usize - guard_bits {
            let mut msg = String::<160>::new();
            let _ = write!(
                msg,
                "initThreadCNodeBits={} exceeds word width minus guard bits",
                init_bits
            );
            return Err(FatalBootstrapError::new(msg));
        }

        if view.root_cnode_cap() != sel4_sys::seL4_CapInitThreadCNode {
            return Err(FatalBootstrapError::new(String::from(
                "canonical root CNode mismatch: expected seL4_CapInitThreadCNode",
            )));
        }

        let (empty_start, empty_end) = view.init_cnode_empty_range();
        if empty_end <= empty_start {
            return Err(FatalBootstrapError::new(String::from(
                "bootinfo empty CSpace window is empty",
            )));
        }

        if empty_start < sel4_sys::seL4_NumInitialCaps as sel4_sys::seL4_CPtr {
            return Err(FatalBootstrapError::new(String::from(
                "first_free slot overlaps kernel-reserved capability range",
            )));
        }

        let capacity = 1usize << init_bits;
        if empty_end as usize > capacity {
            return Err(FatalBootstrapError::new(String::from(
                "bootinfo empty window exceeds init CNode capacity",
            )));
        }

        boot_log::force_uart_line("[mark] bootinfo.validate.ok");

        Ok(())
    }
}

/// Canonicalises the bootinfo pointer into a view and validates invariants.
pub fn canonical_bootinfo_view(
    sequencer: &mut BootstrapSequencer,
    bootinfo: &'static BootInfo,
) -> Result<BootInfoView, FatalBootstrapError> {
    boot_log::force_uart_line("[mark] bootinfo.view.begin");
    sequencer.advance(BootstrapPhase::CSpaceCanonicalise)?;
    match BootInfoView::new(bootinfo) {
        Ok(view) => {
            sequencer.validate_bootinfo(&view)?;
            boot_log::force_uart_line("[mark] bootinfo.view.ok");
            Ok(view)
        }
        Err(err) => {
            let mut msg = String::<160>::new();
            let _ = write!(msg, "bootinfo view construction failed: {err:?}");
            Err(FatalBootstrapError::new(msg))
        }
    }
}

/// Ensures the bootinfo pointer can be snapshotted after validation.
pub fn snapshot_bootinfo(
    bootinfo: &'static BootInfo,
    view: &BootInfoView,
) -> Result<&'static crate::bootstrap::bootinfo_snapshot::BootInfoState, FatalBootstrapError> {
    crate::bootstrap::bootinfo_snapshot::BootInfoState::init(view.header()).map_err(|err| {
        let mut msg = String::<160>::new();
        let _ = write!(msg, "bootinfo snapshot failed: {err:?}");
        FatalBootstrapError::new(msg)
    })
}
