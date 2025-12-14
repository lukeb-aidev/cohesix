// Author: Lukas Bower
#![allow(dead_code)]
#![allow(unsafe_code)]

use sel4_sys::{seL4_CPtr, seL4_CapNull, seL4_Error, seL4_IllegalOperation};

use crate::boot::bi_extra::UntypedDesc;
use crate::bootstrap::bootinfo_snapshot::BootInfoSnapshot;
use crate::bootstrap::cspace::CSpaceWindow;
use crate::bootstrap::cspace_sys::{bits_as_u8, retype_endpoint_auto, verify_root_cnode_slot};
use crate::cspace::CSpace;
use crate::sel4::{self, BootInfoView};
use crate::serial;

pub static mut ROOT_EP: seL4_CPtr = seL4_CapNull;

fn select_endpoint_untyped(view: &BootInfoView) -> Result<(seL4_CPtr, UntypedDesc), seL4_Error> {
    let bi = view.header();
    const MIN_ENDPOINT_BITS: u8 = 12;
    let count = (bi.untyped.end - bi.untyped.start) as usize;
    let descriptors = &bi.untypedList[..count];
    descriptors
        .iter()
        .enumerate()
        .find_map(|(index, desc)| {
            if desc.isDevice != 0 || desc.sizeBits < MIN_ENDPOINT_BITS {
                return None;
            }
            let cap = bi.untyped.start + index as seL4_CPtr;
            Some((cap, (*desc).into()))
        })
        .ok_or(seL4_IllegalOperation)
}

fn log_window_state(
    tag: &str,
    root: seL4_CPtr,
    bits: u8,
    first_free: seL4_CPtr,
    empty: Option<(seL4_CPtr, seL4_CPtr)>,
) {
    let (start, end) = empty.unwrap_or((first_free, first_free));
    log::info!(
        "[cs] {tag} root=0x{root:04x} bits={bits} first_free=0x{slot:04x} empty=[0x{start:04x}..0x{end:04x})",
        root = root,
        bits = bits,
        slot = first_free,
        start = start,
        end = end,
    );
}

pub fn publish_root_ep(ep: seL4_CPtr) {
    unsafe {
        ROOT_EP = ep;
    }
    serial::puts("[boot] root endpoint published\n");
    crate::sel4::set_ep(ep);
}

/// One-shot endpoint bootstrap: pick a regular untyped, retype, publish, and trace.
/// Assumes the init CNode root (slot 0x0002) has `initBits = 13` and that
/// `first_free` is set within the kernel-advertised empty window
/// `[empty_start..empty_end)`. This function consumes exactly one slot from that
/// window and leaves ordering of earlier boot phases unchanged.
pub fn bootstrap_ep(snapshot: &BootInfoSnapshot, cs: &mut CSpace) -> Result<seL4_CPtr, seL4_Error> {
    let view = snapshot.view();
    if sel4::ep_ready() {
        return Ok(sel4::root_endpoint());
    }

    serial::puts("[boot] bootstrap_ep: entry\n");

    let bi = view.header();
    let (ut, desc) = select_endpoint_untyped(&view)?;

    #[cfg(feature = "untyped-debug")]
    {
        crate::trace::println!(
            "[ram-ut: cap=0x{cap:x} size_bits={size_bits} is_device={is_device} paddr=0x{paddr:x}]",
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
    serial::puts("[boot] bootstrap_ep: after alloc_slot\n");
    debug_assert_ne!(
        ep_slot,
        sel4::seL4_CapNull,
        "allocated endpoint slot must not be null",
    );

    let (empty_start, empty_end) = view.init_cnode_empty_range();
    assert!(
        empty_start < empty_end,
        "bootinfo empty window must have positive width",
    );
    let init_bits = view.init_cnode_bits();
    assert_eq!(init_bits, 13, "unexpected init CNode bits");
    let init_root = view.root_cnode_cap();
    assert_eq!(
        init_root,
        sel4_sys::seL4_CapInitThreadCNode,
        "unexpected init thread CNode capability",
    );

    let mut window = CSpaceWindow::new(
        init_root,
        view.canonical_root_cap(),
        bits_as_u8(usize::from(init_bits)),
        empty_start,
        empty_end,
        ep_slot,
    );
    window.assert_contains(ep_slot);
    serial::puts("[boot] bootstrap_ep: window ready\n");
    debug_assert_eq!(
        view.init_cnode_bits(),
        window.bits,
        "canonical init CNode bits mismatch",
    );

    crate::trace::println!(
        "[cs: root=0x{root:x} bits={bits} first_free=0x{slot:x}]",
        root = window.root,
        bits = window.bits,
        slot = window.first_free,
    );

    serial::puts("[boot] bootstrap_ep: before verify\n");
    if let Err(err) = verify_root_cnode_slot(bi, ep_slot as sel4_sys::seL4_Word) {
        serial::puts("[boot] bootstrap_ep: verify_root_cnode_slot failed\n");
        return Err(err);
    }

    serial::puts("[boot] bootstrap_ep: before retype\n");
    let err = retype_endpoint_auto(
        bi,
        ut as sel4_sys::seL4_Word,
        ep_slot as sel4_sys::seL4_Word,
    );
    if err != sel4_sys::seL4_NoError {
        serial::puts("[boot] bootstrap_ep: retype_endpoint_auto failed\n");
        return Err(err);
    }
    serial::puts("[boot] bootstrap_ep: after retype\n");
    window.bump();

    let slot_ident = sel4::debug_cap_identify(ep_slot);
    let _ = slot_ident;

    publish_root_ep(ep_slot);
    serial::puts("[boot] bootstrap_ep: after publish\n");

    Ok(ep_slot)
}

/// Retype an additional endpoint for dedicated fault handling without updating the
/// published root endpoint.
pub fn bootstrap_fault_ep(
    snapshot: &BootInfoSnapshot,
    cs: &mut CSpace,
) -> Result<seL4_CPtr, seL4_Error> {
    let view = snapshot.view();
    let bi = view.header();
    let (ut, desc) = select_endpoint_untyped(&view)?;

    #[cfg(feature = "untyped-debug")]
    {
        crate::trace::println!(
            "[ram-ut: cap=0x{cap:x} size_bits={size_bits} is_device={is_device} paddr=0x{paddr:x}]",
            cap = ut,
            size_bits = desc.size_bits,
            is_device = desc.is_device,
            paddr = desc.paddr,
        );
    }

    #[cfg(not(feature = "untyped-debug"))]
    let _ = desc;

    let ep_slot = cs.alloc_slot()?;
    log_window_state("alloc", cs.root(), cs.depth(), ep_slot, None);
    debug_assert_ne!(
        ep_slot,
        sel4::seL4_CapNull,
        "allocated endpoint slot must not be null",
    );

    let (empty_start, empty_end) = view.init_cnode_empty_range();
    let mut window = CSpaceWindow::new(
        view.root_cnode_cap(),
        view.canonical_root_cap(),
        bits_as_u8(usize::from(view.init_cnode_bits())),
        empty_start,
        empty_end,
        ep_slot,
    );
    window.assert_contains(ep_slot);
    log_window_state(
        "bootinfo",
        window.root,
        window.bits,
        window.first_free,
        Some((window.empty_start, window.empty_end)),
    );

    let err = retype_endpoint_auto(
        bi,
        ut as sel4_sys::seL4_Word,
        ep_slot as sel4_sys::seL4_Word,
    );
    log::info!(
        "[ep] fault ep retype root=0x{root:04x} depth={depth} dst=0x{slot:04x} err={err}",
        root = window.root,
        depth = window.bits,
        slot = ep_slot,
        err = err,
    );
    if err != sel4_sys::seL4_NoError {
        log::error!(
            "[boot] fault endpoint retype failed slot=0x{slot:04x} err={err:?} ({name})",
            slot = ep_slot,
            err = err,
            name = sel4::error_name(err),
        );
        return Err(err);
    }

    window.bump();
    log::info!("[cs] first_free=0x{slot:04x}", slot = cs.next_free_slot());
    let slot_ident = sel4::debug_cap_identify(ep_slot);
    log::info!(
        "[boot] fault endpoint slot=0x{slot:04x} identify=0x{ident:08x}",
        slot = ep_slot,
        ident = slot_ident,
    );

    Ok(ep_slot)
}
