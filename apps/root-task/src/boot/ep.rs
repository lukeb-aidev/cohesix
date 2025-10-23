// Author: Lukas Bower
#![allow(dead_code)]
use core::sync::atomic::{AtomicUsize, Ordering};

use sel4_sys::{
    seL4_BootInfo, seL4_CPtr, seL4_CapInitThreadCNode, seL4_CapNull, seL4_Error,
    seL4_IllegalOperation, seL4_ObjectType,
};

use crate::boot::bi_extra::first_regular_untyped_from_extra;
use crate::caps::traced_retype_into_slot;
use crate::cspace::CSpace;

static EP_SLOT: AtomicUsize = AtomicUsize::new(0);

/// Records the live endpoint capability slot for later guarded IPC.
#[inline]
pub fn set_ep(ep: seL4_CPtr) {
    debug_assert!(ep != seL4_CapNull, "endpoint slot must be non-null");
    EP_SLOT.store(ep as usize, Ordering::Release);
}

/// Returns the published endpoint capability slot if initialised, otherwise `seL4_CapNull`.
#[inline]
#[must_use]
pub fn get_ep() -> seL4_CPtr {
    EP_SLOT.load(Ordering::Acquire) as seL4_CPtr
}

/// One-shot endpoint bootstrap: pick a regular untyped, retype, publish, and trace.
pub fn bootstrap_ep(bi: &seL4_BootInfo, cs: &mut CSpace) -> Result<seL4_CPtr, seL4_Error> {
    if get_ep() != seL4_CapNull {
        return Ok(get_ep());
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

    let root = seL4_CapInitThreadCNode as seL4_CPtr;
    let node_index = bi.initThreadCNode;
    let node_depth = bi.initThreadCNodeSizeBits as u8;

    crate::trace::println!(
        "[cs: root=0x{root:x} bits={bits} first_free=0x{slot:x}]",
        root = node_index,
        bits = node_depth,
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

    traced_retype_into_slot(
        ut,
        seL4_ObjectType::seL4_EndpointObject,
        0,
        root,
        node_index,
        node_depth,
        ep_slot,
    )?;

    Ok(ep_slot)
}
