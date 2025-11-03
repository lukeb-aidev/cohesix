// Author: Lukas Bower
#![allow(dead_code)]
#![allow(unsafe_code)]

use sel4_sys::{seL4_CPtr, seL4_CapNull, seL4_Error, seL4_IllegalOperation};

use crate::boot::bi_extra::first_regular_untyped_from_extra;
use crate::bootstrap::cspace::CSpaceWindow;
use crate::bootstrap::cspace_sys::retype_endpoint_once;
use crate::cspace::CSpace;
use crate::sel4::{self, BootInfoView};
use crate::serial;

pub static mut ROOT_EP: seL4_CPtr = seL4_CapNull;

pub fn publish_root_ep(ep: seL4_CPtr) {
    unsafe {
        ROOT_EP = ep;
    }
    log::info!("[boot] root endpoint published ep=0x{:x}", ep as usize);
    crate::sel4::set_ep(ep);
}

/// One-shot endpoint bootstrap: pick a regular untyped, retype, publish, and trace.
pub fn bootstrap_ep(view: &BootInfoView, cs: &mut CSpace) -> Result<seL4_CPtr, seL4_Error> {
    if sel4::ep_ready() {
        return Ok(sel4::root_endpoint());
    }

    let bi = view.header();
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
        "allocated endpoint slot must not be null",
    );

    let mut window = CSpaceWindow::from_bootinfo(view);
    window.first_free = ep_slot;
    log::info!(
        "[boot:ep] win root=0x{root:x} bits={bits} first_free=0x{slot:x}",
        root = window.root,
        bits = window.bits,
        slot = window.first_free,
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
            depth = sel4_sys::seL4_WordBits,
            slot = window.first_free,
        );
    }

    log::trace!(
        "B1: about to retype endpoint ut=0x{ut:04x} slot=0x{slot:04x}",
        ut = ut,
        slot = ep_slot,
    );

    match retype_endpoint_once(ut, &mut window) {
        Ok(slot) => {
            debug_assert_eq!(slot, ep_slot);
            log::trace!("B1.ret = Ok");
            log::info!(
                "[rt-fix] retype:endpoint OK slot=0x{slot:04x}",
                slot = ep_slot,
            );
        }
        Err(err) => {
            log::trace!("B1.ret = Err({code})", code = sel4::error_name(err),);
            log::error!(
                "[boot] endpoint retype failed slot=0x{slot:04x} err={err:?} ({name})",
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

    publish_root_ep(ep_slot);
    serial::puts(
        "[boot] EP ready
",
    );

    Ok(ep_slot)
}
