// Author: Lukas Bower
//! Bootstrap helpers for mapping the PL011 UART console.
#![allow(unsafe_code)]

use crate::bootstrap::cspace::CSpace;
use crate::cspace::tuples::RetypeTuple;
use crate::uart::pl011;
use log::warn;
use sel4_sys::{self, seL4_CPtr, seL4_CapInitThreadVSpace, seL4_Error, seL4_NoError};

const PL011_PADDR: u64 = 0x0900_0000;

/// Locate the device untyped that backs the PL011 UART MMIO page.
#[must_use]
pub fn find_pl011_device_ut(bi: &sel4_sys::seL4_BootInfo) -> Option<seL4_CPtr> {
    let ut_start = bi.untyped.start;
    let ut_end = bi.untyped.end;
    let total = ut_end.saturating_sub(ut_start) as usize;
    for (index, desc) in bi.untypedList.iter().take(total).enumerate() {
        if desc.isDevice == 0 {
            continue;
        }
        let base = desc.paddr as u64;
        let span = 1u64 << desc.sizeBits;
        let limit = base.saturating_add(span);
        if base <= PL011_PADDR && PL011_PADDR + 0x1000 <= limit {
            return Some(ut_start + index as seL4_CPtr);
        }
    }
    None
}

/// Best-effort mapping for the PL011 UART into the init VSpace.
pub fn bootstrap_map_pl011(
    bi: &sel4_sys::seL4_BootInfo,
    cs: &mut CSpace,
    tuple: &RetypeTuple,
) -> Result<(), seL4_Error> {
    let Some(device_ut) = find_pl011_device_ut(bi) else {
        warn!("[pl011] device untyped not found; continuing without MMIO console");
        return Ok(());
    };

    let page_slot = cs.alloc_slot()?;
    log::info!(
        "[cs] win root=0x{root:04x} bits={bits} first_free=0x{slot:04x}",
        root = cs.root(),
        bits = cs.depth(),
        slot = page_slot,
    );

    let map_err = pl011::map_pl011_smallpage(
        device_ut,
        page_slot as sel4_sys::seL4_Word,
        tuple,
        seL4_CapInitThreadVSpace,
    );
    if map_err == seL4_NoError {
        log::info!("[cs] first_free=0x{slot:04x}", slot = cs.next_free_slot());
        Ok(())
    } else {
        warn!(
            "[pl011] map failed slot=0x{slot:04x} err={err}",
            slot = page_slot,
            err = map_err
        );
        Err(map_err)
    }
}
