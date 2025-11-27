// Author: Lukas Bower
//! Bootstrap helpers for copying the init TCB capability.
#![allow(unsafe_code)]

use core::sync::atomic::{AtomicBool, Ordering};

use crate::cspace::CSpace;
use crate::sel4::BootInfoExt;
use sel4_sys::{self, seL4_CPtr, seL4_Error};

static INIT_TCB_REUSE_ANNOUNCED: AtomicBool = AtomicBool::new(false);

/// Publishes the canonical TCB for the root task.
///
/// The init thread TCB cap provided by the kernel is already suitable for the
/// root task, so we avoid speculative `CNode_Copy` syscalls during bootstrap.
/// This keeps the boot log clean (no "Target slot invalid" decode warnings)
/// while preserving the existing behaviour of running on the init TCB.
pub fn bootstrap_copy_init_tcb(
    bootinfo: &sel4_sys::seL4_BootInfo,
    cs: &mut CSpace,
) -> Result<seL4_CPtr, seL4_Error> {
    let _ = cs;
    let init_tcb = bootinfo.init_tcb_cap();
    if !INIT_TCB_REUSE_ANNOUNCED.swap(true, Ordering::Relaxed) {
        log::info!(
            "[tcb] canonical root uses init TCB cap=0x{cap:04x}; copy suppressed",
            cap = init_tcb,
        );
    }

    Ok(init_tcb)
}
