// Author: Lukas Bower
#![allow(dead_code)]
#![allow(unsafe_code)]

use sel4_sys::{seL4_CPtr, seL4_CapNull, seL4_Error, seL4_IllegalOperation};

use crate::boot::bi_extra::UntypedDesc;
use crate::bootstrap::cspace::CSpaceWindow;
use crate::bootstrap::cspace_sys::{retype_endpoint_auto, verify_root_cnode_slot};
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
    log::info!("[boot] root endpoint published ep=0x{ep:04x}", ep = ep);
    crate::sel4::set_ep(ep);
}

/// One-shot endpoint bootstrap: pick a regular untyped, retype, publish, and trace.
/// Assumes the init CNode root (slot 0x0002) has `initBits = 13` and that
/// `first_free` is set within the kernel-advertised empty window
/// `[empty_start..empty_end)`. This function consumes exactly one slot from that
/// window and leaves ordering of earlier boot phases unchanged.
pub fn bootstrap_ep(view: &BootInfoView, cs: &mut CSpace) -> Result<seL4_CPtr, seL4_Error> {
    if sel4::ep_ready() {
        return Ok(sel4::root_endpoint());
    }

    let bi = view.header();
    let (ut, desc) = select_endpoint_untyped(view)?;

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
    cs.reserve_slot(ep_slot);
    log_window_state("alloc", cs.root(), cs.depth(), ep_slot, None);
    debug_assert_ne!(
        ep_slot,
        sel4::seL4_CapNull,
        "allocated endpoint slot must not be null",
    );

    let mut window = CSpaceWindow::from_bootinfo(view);
    window.first_free = ep_slot;
    window.assert_contains(ep_slot);
    log_window_state(
        "bootinfo",
        window.root,
        window.bits,
        window.first_free,
        Some((window.empty_start, window.empty_end)),
    );
    debug_assert_eq!(
        view.init_cnode_bits(),
        window.bits,
        "canonical init CNode bits mismatch"
    );

    crate::trace::println!(
        "[cs: root=0x{root:x} bits={bits} first_free=0x{slot:x}]",
        root = window.root,
        bits = window.bits,
        slot = window.first_free,
    );

    if crate::boot::flags::trace_dest() {
        log::info!(
            "[boot] endpoint retype dest root=0x{root:04x} depth={depth} slot=0x{slot:04x}",
            root = window.root,
            depth = window.bits,
            slot = window.first_free,
        );
    }

    log::trace!(
        "B1: about to retype endpoint ut=0x{ut:04x} slot=0x{slot:04x}",
        ut = ut,
        slot = ep_slot,
    );

    if let Err(err) = verify_root_cnode_slot(bi, ep_slot as sel4_sys::seL4_Word) {
        log::error!(
            "[boot] init CNode path probe failed slot=0x{slot:04x} err={err} ({name})",
            slot = ep_slot,
            err = err,
            name = sel4::error_name(err),
        );
        return Err(err);
    }

    let err = retype_endpoint_auto(
        bi,
        ut as sel4_sys::seL4_Word,
        ep_slot as sel4_sys::seL4_Word,
    );
    log::info!(
        "[ep] retype root=0x{root:04x} depth={depth} dst=0x{slot:04x} err={err}",
        root = window.root,
        depth = window.bits,
        slot = ep_slot,
        err = err,
    );
    if err != sel4_sys::seL4_NoError {
        log::trace!("B1.ret = Err({code})", code = sel4::error_name(err));
        log::error!(
            "[boot] endpoint retype failed slot=0x{slot:04x} err={err:?} ({name})",
            slot = ep_slot,
            err = err,
            name = sel4::error_name(err),
        );
        return Err(err);
    }
    log::trace!("B1.ret = Ok");
    window.bump();
    log::info!("[cs] first_free=0x{slot:04x}", slot = cs.next_free_slot());

    let slot_ident = sel4::debug_cap_identify(ep_slot);
    log::info!(
        "[boot] endpoint slot=0x{slot:04x} identify=0x{ident:08x}",
        slot = ep_slot,
        ident = slot_ident,
    );

    publish_root_ep(ep_slot);
    serial::puts(
        "[boot] EP ready
",
    );

    Ok(ep_slot)
}

/// Retype an additional endpoint for dedicated fault handling without updating the
/// published root endpoint.
pub fn bootstrap_fault_ep(view: &BootInfoView, cs: &mut CSpace) -> Result<seL4_CPtr, seL4_Error> {
    let bi = view.header();
    let (ut, desc) = select_endpoint_untyped(view)?;

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
    cs.reserve_slot(ep_slot);
    log_window_state("alloc", cs.root(), cs.depth(), ep_slot, None);
    debug_assert_ne!(
        ep_slot,
        sel4::seL4_CapNull,
        "allocated endpoint slot must not be null",
    );

    let mut window = CSpaceWindow::from_bootinfo(view);
    window.first_free = ep_slot;
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
