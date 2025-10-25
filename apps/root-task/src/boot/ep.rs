// Author: Lukas Bower
#![allow(dead_code)]

use core::convert::TryFrom;

use sel4_sys::{
    seL4_BootInfo, seL4_CPtr, seL4_CapInitThreadCNode, seL4_Error, seL4_IllegalOperation,
    seL4_ObjectType,
};

use crate::boot::bi_extra::first_regular_untyped_from_extra;
use crate::caps::traced_retype_into_slot;
use crate::cspace::CSpace;
use crate::sel4;
use crate::serial;

/// One-shot endpoint bootstrap: pick a regular untyped, retype, publish, and trace.
pub fn bootstrap_ep(bi: &seL4_BootInfo, cs: &mut CSpace) -> Result<seL4_CPtr, seL4_Error> {
    if sel4::ep_ready() {
        return Ok(sel4::root_endpoint());
    }

    let (ut, desc) = first_regular_untyped_from_extra(bi).ok_or(seL4_IllegalOperation)?;

    crate::trace::println!(
        "[untyped: cap=0x{cap:x} size_bits={size_bits} is_device={is_device} paddr=0x{paddr:x}]",
        cap = ut,
        size_bits = desc.size_bits,
        is_device = desc.is_device,
        paddr = desc.paddr,
    );

    let ep_slot = cs.alloc_slot()?;
    debug_assert_ne!(
        ep_slot,
        sel4::seL4_CapNull,
        "allocated endpoint slot must not be null"
    );

    let root = seL4_CapInitThreadCNode as seL4_CPtr;
    let node_index = ep_slot;
    let guard_depth_word = sel4::word_bits();
    let node_depth = u8::try_from(guard_depth_word)
        .expect("seL4_WordBits must fit within u8 for guard-depth addressing");
    let node_offset = 0;

    crate::trace::println!(
        "[cs: root=0x{root:x} bits={bits} first_free=0x{slot:x}]",
        root = root,
        bits = bi.initThreadCNodeSizeBits,
        slot = ep_slot,
    );

    if let Some(capacity) = 1usize.checked_shl(bi.initThreadCNodeSizeBits as u32) {
        debug_assert!(
            (ep_slot as usize) < capacity,
            "endpoint slot 0x{:x} exceeds init CNode capacity 0x{:x}",
            ep_slot,
            capacity,
        );
    }
    debug_assert!(
        ep_slot >= bi.empty.start,
        "endpoint slot 0x{:x} precedes first free slot 0x{:x}",
        ep_slot,
        bi.empty.start,
    );

    log::info!(
        "[boot] endpoint retype dest root=initCNode index=0x{index:04x} depth={depth}(WB) offset=0",
        index = node_index,
        depth = guard_depth_word,
    );

    match traced_retype_into_slot(
        ut,
        seL4_ObjectType::seL4_EndpointObject,
        0,
        root,
        node_index,
        node_depth,
        node_offset,
    ) {
        Ok(()) => {
            log::info!(
                "[boot] endpoint retype ok slot=0x{slot:04x}",
                slot = ep_slot
            );
        }
        Err(err) => {
            log::error!(
                "[boot] endpoint retype failed slot=0x{slot:04x} err={err} ({name})",
                slot = ep_slot,
                err = err,
                name = sel4::error_name(err),
            );
            return Err(err);
        }
    }

    let slot_ident = sel4::debug_cap_identify(ep_slot);
    log::info!(
        "[boot] endpoint slot=0x{slot:04x} identify=0x{ident:08x}",
        slot = ep_slot,
        ident = slot_ident,
    );

    sel4::set_ep(ep_slot);
    serial::puts("[boot] EP ready\n");

    Ok(ep_slot)
}
