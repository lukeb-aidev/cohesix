// Author: Lukas Bower
//! Bootstrap helpers for copying the init TCB capability.
#![allow(unsafe_code)]

use crate::bootstrap::cspace_sys::cnode_copy_raw;
use crate::cspace::CSpace;
use crate::sel4::BootInfoExt;
use sel4_sys::{self, seL4_CPtr, seL4_CapRights_ReadWrite, seL4_Error, seL4_NoError, seL4_Word};

/// Copy the init thread TCB capability into the next free slot of the init CSpace.
pub fn bootstrap_copy_init_tcb(
    bootinfo: &sel4_sys::seL4_BootInfo,
    cs: &mut CSpace,
    init_bits: u8,
) -> Result<seL4_CPtr, seL4_Error> {
    let dst_slot = cs.alloc_slot()?;
    log::info!(
        "[cs] win root=0x{root:04x} bits={bits} first_free=0x{slot:04x}",
        root = cs.root(),
        bits = cs.depth(),
        slot = dst_slot,
    );

    let init_root = cs.root();
    let src_slot = bootinfo.init_tcb_cap();
    let err = cnode_copy_raw(
        bootinfo,
        init_root,
        dst_slot as seL4_Word,
        init_root,
        src_slot as seL4_Word,
        seL4_CapRights_ReadWrite,
    );
    log::info!(
        "[tcb] copy   -> dst=0x{slot:04x} err={err}",
        slot = dst_slot,
        err = err
    );

    if err == seL4_NoError {
        log::info!("[cs] first_free=0x{slot:04x}", slot = cs.next_free_slot());
        Ok(dst_slot)
    } else {
        Err(err)
    }
}
