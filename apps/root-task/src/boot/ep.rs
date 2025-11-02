// Author: Lukas Bower
#![allow(dead_code)]

use core::convert::TryFrom;

use sel4_sys::{seL4_BootInfo, seL4_CPtr, seL4_Error, seL4_IllegalOperation};

use crate::boot::bi_extra::first_regular_untyped_from_extra;
use crate::bootstrap::cspace_sys;
use crate::cspace::tuples::retype_endpoint_into_slot;
use crate::cspace::CSpace;
use crate::sel4::{self, seL4_Word};
use crate::serial;

/// One-shot endpoint bootstrap: pick a regular untyped, retype, publish, and trace.
pub fn bootstrap_ep(
    bi: &seL4_BootInfo,
    cs: &mut CSpace,
    tuple: &crate::cspace::tuples::RetypeTuple,
) -> Result<seL4_CPtr, seL4_Error> {
    if sel4::ep_ready() {
        return Ok(sel4::root_endpoint());
    }

    let (ut, desc) = first_regular_untyped_from_extra(bi).ok_or(seL4_IllegalOperation)?;

    #[cfg(feature = "untyped-debug")]
    {
        crate::trace::println!(
            "[untyped: cap=0x{cap:x} size_bits={size_bits} is_device={is_device} paddr=0x{paddr:x}]",
            cap = ut,
            size_bits = desc.size_bits,
            is_device = desc.is_device,
            paddr = desc.paddr,
        );
    }

    #[cfg(not(feature = "untyped-debug"))]
    {
        let _ = desc;
    }

    let ep_slot = cs.alloc_slot()?;
    debug_assert_ne!(
        ep_slot,
        sel4::seL4_CapNull,
        "allocated endpoint slot must not be null"
    );

    let (root, node_index_word, node_depth_word, node_offset_word) =
        cspace_sys::init_cnode_retype_dest(ep_slot);
    let node_index = seL4_CPtr::try_from(node_index_word)
        .expect("init CNode destination index must fit in seL4_CPtr");
    let node_depth =
        u8::try_from(node_depth_word).expect("initThreadCNodeSizeBits must fit within u8");
    let _node_offset = seL4_CPtr::try_from(node_offset_word)
        .expect("init CNode destination offset must fit in seL4_CPtr");
    debug_assert_eq!(node_index, 0);
    debug_assert_eq!(node_depth, bi.initThreadCNodeSizeBits as u8);

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

    if crate::boot::flags::trace_dest() {
        log::info!(
            "[boot] endpoint retype dest root=initCNode index=0x{index:04x} depth={depth} offset=0x{offset:04x}",
            index = node_index,
            depth = node_depth_word,
            offset = node_offset_word,
        );
    }

    log::trace!(
        "B1: about to retype endpoint ut=0x{ut:04x} slot=0x{slot:04x}",
        ut = ut,
        slot = ep_slot,
    );
    let retype_err = retype_endpoint_into_slot(ut, ep_slot as seL4_Word, tuple);
    if retype_err == sel4_sys::seL4_NoError {
        log::trace!("B1.ret = Ok");
        log::info!(
            "[rt-fix] retype:endpoint OK slot=0x{slot:04x}",
            slot = ep_slot
        );
    } else {
        log::trace!(
            "B1.ret = Err({code})",
            code = sel4::error_name(retype_err as seL4_Error)
        );
        log::error!(
            "[boot] endpoint retype failed slot=0x{slot:04x} err={err} ({name})",
            slot = ep_slot,
            err = retype_err,
            name = sel4::error_name(retype_err),
        );
        return Err(retype_err);
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
