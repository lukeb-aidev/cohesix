// Author: Lukas Bower
#![allow(dead_code)]
#![allow(unsafe_code)]

use core::fmt::Write;
use heapless::String;
use sel4_sys::{seL4_CPtr, seL4_CapNull, seL4_Error, seL4_IllegalOperation};

use crate::boot::bi_extra::UntypedDesc;
use crate::bootstrap::cspace::CSpaceWindow;
use crate::bootstrap::cspace_sys::{retype_endpoint_auto, verify_root_cnode_slot};
use crate::bootstrap::log::force_uart_line;
use crate::bootstrap::untyped_pick::device_pt_pool;
use crate::cspace::CSpace;
use crate::kernel::BootError;
use crate::sel4::{self, BootInfoExt, BootInfoView};
use crate::serial;

pub static mut ROOT_EP: seL4_CPtr = seL4_CapNull;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointInitError {
    pub root: seL4_CPtr,
    pub root_bits: u8,
    pub first_free: seL4_CPtr,
    pub dest_slot: seL4_CPtr,
    pub ut_cap: seL4_CPtr,
    pub syscall: &'static str,
    pub code: Option<seL4_Error>,
}

impl core::fmt::Display for EndpointInitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "root=0x{root:04x} bits={bits} first_free=0x{first_free:04x} dest=0x{dest:04x} ut=0x{ut:03x} syscall={syscall} code={code}",
            root = self.root,
            bits = self.root_bits,
            first_free = self.first_free,
            dest = self.dest_slot,
            ut = self.ut_cap,
            syscall = self.syscall,
            code = self.code.map(|c| c as i32).unwrap_or(-1),
        )
    }
}

fn select_endpoint_untyped(view: &BootInfoView) -> Result<(seL4_CPtr, UntypedDesc), seL4_Error> {
    let bi = view.header();
    const MIN_ENDPOINT_BITS: u8 = 12;
    let count = (bi.untyped.end - bi.untyped.start) as usize;
    let descriptors = &bi.untypedList[..count];
    let reserved_device_pt_ut = device_pt_pool().map(|pool| pool.ut_slot);
    descriptors
        .iter()
        .enumerate()
        .find_map(|(index, desc)| {
            if desc.isDevice != 0 || desc.sizeBits < MIN_ENDPOINT_BITS {
                return None;
            }
            let cap = bi.untyped.start + index as seL4_CPtr;
            if Some(cap) == reserved_device_pt_ut {
                let mut line = String::<144>::new();
                let _ = write!(
                    line,
                    "[ep:init] skip reserved device-pt ut=0x{cap:03x} bits={bits} index={index}",
                    cap = cap,
                    bits = desc.sizeBits,
                    index = index,
                );
                force_uart_line(line.as_str());
                return None;
            }
            Some((cap, (*desc).into()))
        })
        .ok_or(seL4_IllegalOperation)
}

fn trace_ep_retype(
    ut: seL4_CPtr,
    desc: &UntypedDesc,
    dest_slot: seL4_CPtr,
    depth: u8,
    first_free: seL4_CPtr,
) {
    let mut line = String::<192>::new();
    let _ = write!(
        line,
        "[ep:init] ut=0x{ut:03x} bits={bits} paddr=0x{paddr:08x} -> dest=0x{dest:04x} depth={depth} first_free=0x{first_free:04x}",
        ut = ut,
        bits = desc.size_bits,
        paddr = desc.paddr,
        dest = dest_slot,
        depth = depth,
        first_free = first_free,
    );
    force_uart_line(line.as_str());
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
pub fn bootstrap_ep(view: &BootInfoView, cs: &mut CSpace) -> Result<seL4_CPtr, BootError> {
    if sel4::ep_ready() {
        return Ok(sel4::root_endpoint());
    }

    let bi = view.header();
    let (empty_start, _empty_end) = view.init_cnode_empty_range();
    if bi.init_cnode_cap() == seL4_CapNull {
        let err = EndpointInitError {
            root: bi.init_cnode_cap(),
            root_bits: view.init_cnode_bits(),
            first_free: empty_start,
            dest_slot: empty_start,
            ut_cap: sel4_sys::seL4_CapNull,
            syscall: "validate_root",
            code: None,
        };
        log::error!("[ep:init] invalid root cnode: {err}");
        return Err(BootError::EndpointInit(err));
    }

    let (ut, desc) = select_endpoint_untyped(view).map_err(|code| {
        BootError::EndpointInit(EndpointInitError {
            root: bi.init_cnode_cap(),
            root_bits: view.init_cnode_bits(),
            first_free: empty_start,
            dest_slot: empty_start,
            ut_cap: bi.untyped.start,
            syscall: "select_untyped",
            code: Some(code),
        })
    })?;

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

    let mut window = CSpaceWindow::from_bootinfo(view);

    let ep_slot = cs.alloc_slot().map_err(|_| {
        BootError::EndpointInit(EndpointInitError {
            root: window.root,
            root_bits: window.bits,
            first_free: window.first_free,
            dest_slot: window.first_free,
            ut_cap: ut,
            syscall: "alloc_slot",
            code: Some(sel4_sys::seL4_IllegalOperation),
        })
    })?;
    cs.reserve_slot(ep_slot);
    log_window_state("alloc", cs.root(), cs.depth(), ep_slot, None);
    debug_assert_ne!(
        ep_slot,
        sel4::seL4_CapNull,
        "allocated endpoint slot must not be null",
    );

    let mut window = CSpaceWindow::from_bootinfo(view);
    window.first_free = ep_slot;
    if ep_slot < window.empty_start || ep_slot >= window.empty_end {
        let err = EndpointInitError {
            root: window.root,
            root_bits: window.bits,
            first_free: window.first_free,
            dest_slot: ep_slot,
            ut_cap: ut,
            syscall: "slot_validate",
            code: Some(sel4_sys::seL4_RangeError),
        };
        log::error!("[ep:init] destination slot outside empty window: {err}");
        return Err(BootError::EndpointInit(err));
    }
    window.assert_contains(ep_slot);
    log_window_state(
        "bootinfo",
        window.root,
        window.bits,
        window.first_free,
        Some((window.empty_start, window.empty_end)),
    );
    if view.init_cnode_bits() != window.bits {
        let err = EndpointInitError {
            root: window.root,
            root_bits: window.bits,
            first_free: window.first_free,
            dest_slot: ep_slot,
            ut_cap: ut,
            syscall: "depth_validate",
            code: Some(sel4_sys::seL4_IllegalOperation),
        };
        log::error!("[ep:init] init cnode bits mismatch: {err}");
        return Err(BootError::EndpointInit(err));
    }

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
    trace_ep_retype(ut, &desc, ep_slot, window.bits, window.first_free);

    if let Err(err) = verify_root_cnode_slot(bi, ep_slot as sel4_sys::seL4_Word) {
        let init_err = EndpointInitError {
            root: window.root,
            root_bits: window.bits,
            first_free: window.first_free,
            dest_slot: ep_slot,
            ut_cap: ut,
            syscall: "verify_root_cnode_slot",
            code: Some(err),
        };
        log::error!("[boot] init CNode path probe failed: {init_err}");
        return Err(BootError::EndpointInit(init_err));
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
        let init_err = EndpointInitError {
            root: window.root,
            root_bits: window.bits,
            first_free: window.first_free,
            dest_slot: ep_slot,
            ut_cap: ut,
            syscall: "retype_endpoint_auto",
            code: Some(err),
        };
        log::trace!("B1.ret = Err({code})", code = sel4::error_name(err));
        log::error!("[boot] endpoint retype failed: {init_err}");
        return Err(BootError::EndpointInit(init_err));
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
