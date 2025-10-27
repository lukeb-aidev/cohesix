// Author: Lukas Bower
#![allow(dead_code)]
#![allow(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use core::cmp;
use core::fmt::{self, Write};
use core::panic::PanicInfo;
#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use cohesix_ticket::Role;

use crate::boot::{bi_extra, ep};
use crate::bootstrap::{
    cspace::CSpaceCtx, ipcbuf, log as boot_log, pick_untyped, retype::retype_one,
};
use crate::console::Console;
use crate::cspace::{cap_rights_read_write_grant, CSpace};
use crate::event::{
    AuditSink, BootstrapMessage, BootstrapMessageHandler, EventPump, IpcDispatcher, TickEvent,
    TicketTable, TimerSource,
};
use crate::hal::{HalError, Hardware, KernelHal};
#[cfg(feature = "net-console")]
use crate::net::{NetStack, CONSOLE_TCP_PORT};
#[cfg(feature = "kernel")]
use crate::ninedoor::NineDoorBridge;
use crate::platform::{Platform, SeL4Platform};
use crate::sel4::{
    bootinfo_debug_dump, error_name, root_endpoint, BootInfo, BootInfoExt, BootInfoView, KernelEnv,
    RetypeKind, RetypeStatus, IPC_PAGE_BYTES, MSG_MAX_WORDS,
};
use crate::serial::{
    pl011::Pl011, SerialPort, DEFAULT_LINE_CAPACITY, DEFAULT_RX_CAPACITY, DEFAULT_TX_CAPACITY,
};
use heapless::{String as HeaplessString, Vec as HeaplessVec};
#[cfg(feature = "net-console")]
use smoltcp::wire::Ipv4Address;

#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
use sel4_panicking::{self, DebugSink};

/// seL4 console writer backed by the kernel's `DebugPutChar` system call.
struct DebugConsole<'a, P: Platform> {
    platform: &'a P,
}

impl<'a, P: Platform> DebugConsole<'a, P> {
    const PREFIX: &'static str = "[cohesix:root-task] ";

    #[inline(always)]
    fn new(platform: &'a P) -> Self {
        Self { platform }
    }

    #[inline(always)]
    fn write_raw(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.platform.putc(byte);
        }
    }

    fn writeln_prefixed(&mut self, body: &str) {
        let _ = self.write_str(Self::PREFIX);
        let _ = self.write_str(body);
        self.newline();
    }

    fn newline(&mut self) {
        // seL4's serial console expects CRLF for neat output in QEMU.
        self.write_raw(b"\r\n");
    }

    fn report_bootinfo(&mut self, view: &BootInfoView) {
        let header = view.header();
        let header_bytes = view.header_bytes();
        let header_addr = header as *const _ as usize;
        let header_len = header_bytes.len();
        let header_range = header_bytes.as_ptr_range();
        let header_end = header_range.end as usize;

        let extra_words = view.extra_words();
        let extra_slice = view.extra();
        let extra_len = extra_slice.len();
        let (extra_start, extra_end) = if extra_len == 0 {
            (header_end, header_end)
        } else {
            let range = extra_slice.as_ptr_range();
            (range.start as usize, range.end as usize)
        };

        let _ = write!(
            self,
            "{prefix}bootinfo @ 0x{addr:016x} (header {header_len} bytes)\r\n",
            prefix = Self::PREFIX,
            addr = header_addr,
            header_len = header_len,
        );
        let _ = write!(
            self,
            "{prefix}bootinfo.extraLen = {extra_words} words ({extra_len} bytes)\r\n",
            prefix = Self::PREFIX,
            extra_words = extra_words,
            extra_len = extra_len,
        );
        let _ = write!(
            self,
            "{prefix}bootinfo.extra region [0x{start:016x}..0x{end:016x})\r\n",
            prefix = Self::PREFIX,
            start = extra_start,
            end = extra_end,
        );
        let _ = write!(
            self,
            "{prefix}node_id={node_id} nodes={nodes} ipc_buffer=0x{ipc:016x}\r\n",
            prefix = Self::PREFIX,
            node_id = header.nodeId,
            nodes = header.numNodes,
            ipc = header.ipcBuffer,
        );

        let bits = header.init_cnode_bits();
        let capacity = 1usize.checked_shl(bits as u32).unwrap_or(usize::MAX);
        let empty_start = header.empty_first_slot();
        let empty_end = header.empty_last_slot_excl();
        let empty_span = empty_end.saturating_sub(empty_start);

        let _ = write!(
            self,
            "{prefix}initThreadCNode=0x{cnode:04x} bits={bits} capacity={capacity}\r\n",
            prefix = Self::PREFIX,
            cnode = view.root_cnode_cap(),
            bits = bits,
            capacity = capacity,
        );
        let _ = write!(
            self,
            "{prefix}empty slots [0x{start:04x}..0x{end:04x}) span={span}\r\n",
            prefix = Self::PREFIX,
            start = empty_start,
            end = empty_end,
            span = empty_span,
        );
    }
}

impl<'a, P: Platform> Write for DebugConsole<'a, P> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_raw(s.as_bytes());
        Ok(())
    }
}

#[cfg(debug_assertions)]
fn log_text_span() {
    extern "C" {
        static __text_start: u8;
        static __text_end: u8;
    }

    let lo = core::ptr::addr_of!(__text_start) as usize;
    let hi = core::ptr::addr_of!(__text_end) as usize;
    log::info!("[dbg] .text [{:#x}..{:#x})", lo, hi);
    let handle_ptr = EventPump::<
        Pl011,
        KernelTimer,
        KernelIpc,
        TicketTable<4>,
        { DEFAULT_RX_CAPACITY },
        { DEFAULT_TX_CAPACITY },
        { DEFAULT_LINE_CAPACITY },
    >::handle_command as usize;
    let retype_ptr = crate::bootstrap::cspace_sys::untyped_retype_into_init_root as usize;
    log::info!(
        "[dbg] anchors: handle_cmd={:#x} retype_call={:#x}",
        handle_ptr,
        retype_ptr
    );
}

#[cfg(not(target_arch = "aarch64"))]
compile_error!("root-task kernel build currently supports only aarch64 targets");

const PL011_PADDR: usize = 0x0900_0000;
const DEVICE_FRAME_BITS: usize = 12;
const EARLY_DUMP_LIMIT: usize = 512;

#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
static mut EARLY_UART_SINK: DebugSink = DebugSink {
    context: core::ptr::null_mut(),
    emit: pl011_debug_emit,
};

#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
const PL011_DR_OFFSET: usize = 0x00;
#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
const PL011_FR_OFFSET: usize = 0x18;
#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
const PL011_FR_TXFF: u32 = 1 << 5;

#[cfg(target_arch = "aarch64")]
static mut TLS_IMAGE: sel4_sys::TlsImage = sel4_sys::TlsImage::new();

#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
unsafe extern "C" fn pl011_debug_emit(context: *mut (), byte: u8) {
    debug_assert!(!context.is_null(), "PL011 sink context must be valid");
    debug_assert!(
        context as usize & (core::mem::align_of::<u32>() - 1) == 0,
        "PL011 sink context must be 4-byte aligned",
    );
    let base = context.cast::<u8>();
    let dr = unsafe { base.add(PL011_DR_OFFSET).cast::<u32>() };
    let fr = unsafe { base.add(PL011_FR_OFFSET).cast::<u32>() };

    unsafe {
        while ptr::read_volatile(fr) & PL011_FR_TXFF != 0 {
            core::hint::spin_loop();
        }

        ptr::write_volatile(dr, u32::from(byte));
    }
}

/// Minimal blocking console loop used during early bring-up.
pub fn start_console(uart: Pl011) -> ! {
    let mut console = Console::new(uart);
    let _ = writeln!(console, "[cohesix] console ready");
    let mut buffer = [0u8; 256];

    loop {
        let _ = write!(console, "cohsh> ");
        let count = console.read_line(&mut buffer);
        let line = match core::str::from_utf8(&buffer[..count]) {
            Ok(text) => text.trim(),
            Err(_) => {
                let _ = writeln!(console, "invalid utf-8 input");
                continue;
            }
        };

        if line.is_empty() {
            continue;
        }

        if line.eq_ignore_ascii_case("help") {
            let _ = writeln!(console, "commands: help, echo <txt>, reboot (stub)");
            continue;
        }

        if let Some(rest) = line.strip_prefix("echo ") {
            let _ = writeln!(console, "{}", rest);
            continue;
        }

        if line.eq_ignore_ascii_case("reboot") {
            let _ = writeln!(console, "reboot not implemented");
            continue;
        }

        let _ = writeln!(console, "unknown: {}", line);
    }
}

static BOOTSTRAP_ONCE: AtomicBool = AtomicBool::new(false);

/// Root task entry point invoked by seL4 after kernel initialisation.
pub fn start<P: Platform>(bootinfo: &'static BootInfo, platform: &P) -> ! {
    bootstrap(platform, bootinfo)
}

fn bootstrap<P: Platform>(platform: &P, bootinfo: &'static BootInfo) -> ! {
    #[cfg(all(feature = "kernel", not(sel4_config_printing)))]
    crate::sel4::install_debug_sink();

    crate::alloc::init_heap();

    boot_log::init_logger_bootstrap_only();
    if BOOTSTRAP_ONCE.swap(true, Ordering::SeqCst) {
        log::error!("[boot] bootstrap called twice; refusing re-entry");
        #[cfg(feature = "kernel")]
        crate::sel4::debug_halt();
        loop {
            core::hint::spin_loop();
        }
    }
    crate::bp!("bootstrap.begin");

    let bootinfo_view = match BootInfoView::new(bootinfo) {
        Ok(view) => view,
        Err(err) => {
            log::error!("[boot] invalid bootinfo: {err}");
            #[cfg(feature = "kernel")]
            crate::sel4::debug_halt();
            loop {
                core::hint::spin_loop();
            }
        }
    };
    let bootinfo_ref: &'static sel4_sys::seL4_BootInfo = bootinfo_view.header();
    let mut console = DebugConsole::new(platform);

    let mut boot_cspace = CSpace::from_bootinfo(bootinfo_ref);
    let boot_first_free = boot_cspace.next_free_slot();

    console.writeln_prefixed("entered from seL4 (stage0)");
    console.writeln_prefixed("Cohesix boot: root-task online");

    #[cfg(debug_assertions)]
    log_text_span();

    console.report_bootinfo(&bootinfo_view);

    let mut cs_line = heapless::String::<96>::new();
    let _ = write!(
        cs_line,
        "cs: root=0x{root:04x} bits={bits} first_free=0x{first_free:04x}",
        root = bootinfo_ref.init_cnode_cap(),
        bits = bootinfo_ref.initThreadCNodeSizeBits,
        first_free = boot_first_free,
    );
    console.writeln_prefixed(cs_line.as_str());

    console.writeln_prefixed("Cohesix v0 (AArch64/virt)");

    bootinfo_debug_dump(&bootinfo_view);
    let mut kernel_env = KernelEnv::new(bootinfo_ref);
    let extra_bytes = bootinfo_view.extra();
    if !extra_bytes.is_empty() {
        console.writeln_prefixed("[boot] deferring DTB parse");
    }

    let ipc_buffer_ptr = bootinfo_ref.ipc_buffer_ptr();
    let ipc_vaddr = ipc_buffer_ptr.map(|ptr| ptr.as_ptr() as usize);

    if let Some(ptr) = ipc_buffer_ptr {
        let addr = ptr.as_ptr() as usize;
        assert_eq!(
            addr & (IPC_PAGE_BYTES - 1),
            0,
            "IPC buffer must be page-aligned",
        );
    }

    let ep_slot = match ep::bootstrap_ep(bootinfo_ref, &mut boot_cspace) {
        Ok(ep_slot) => ep_slot,
        Err(err) => {
            crate::trace::trace_fail(b"bootstrap_ep", err);
            let mut line = heapless::String::<160>::new();
            let _ = write!(
                line,
                "bootstrap_ep failed: {} ({})",
                err as i32,
                error_name(err)
            );
            console.writeln_prefixed(line.as_str());
            panic!("bootstrap_ep failed: {}", error_name(err));
        }
    };

    crate::trace::trace_ep(ep_slot);

    console.writeln_prefixed("[boot] root endpoint published");

    unsafe {
        #[cfg(all(feature = "kernel", target_arch = "aarch64"))]
        {
            sel4_sys::tls_set_base(core::ptr::addr_of_mut!(TLS_IMAGE));
            debug_assert!(
                sel4_sys::tls_image_mut().is_some(),
                "TLS base must resolve to an image after installation",
            );
        }

        if let Some(ipc_ptr) = ipc_buffer_ptr {
            sel4_sys::seL4_SetIPCBuffer(ipc_ptr.as_ptr());
            let mut msg = heapless::String::<64>::new();
            let _ = write!(msg, "ipc buffer ptr=0x{:016x}", ipc_ptr.as_ptr() as usize);
            console.writeln_prefixed(msg.as_str());
        } else {
            console.writeln_prefixed("bootinfo.ipcBuffer missing");
        }
    }

    debug_assert_eq!(ep_slot, root_endpoint());
    let rights = cap_rights_read_write_grant();

    crate::bp!("tcb.copy.begin");
    let tcb_copy_slot = match boot_cspace.alloc_slot() {
        Ok(slot) => slot,
        Err(err) => {
            panic!(
                "failed to allocate init CSpace slot for TCB copy: {} ({})",
                err,
                error_name(err)
            );
        }
    };
    let tcb_src_slot = bootinfo_ref.init_tcb_cap();
    let copy_err = boot_cspace.copy_here(tcb_copy_slot, tcb_src_slot, rights);
    if let Err(code) = crate::bootstrap::ktry("tcb.copy", copy_err as i32) {
        panic!(
            "copying init TCB capability failed: {} ({})",
            code,
            error_name(copy_err)
        );
    } else {
        log::info!(
            "[cnode] copy root=0x{root:04x} dst=0x{dst:04x} src=0x{src:04x} depth={depth}",
            root = boot_cspace.root(),
            dst = tcb_copy_slot,
            src = tcb_src_slot,
            depth = boot_cspace.depth()
        );
    }
    crate::bp!("tcb.copy.end");

    if let Some(ipc_vaddr) = ipc_vaddr {
        match ipcbuf::install_ipc_buffer(&mut kernel_env, tcb_copy_slot, ipc_vaddr) {
            Ok(_) => {
                let mut msg = heapless::String::<112>::new();
                let _ = write!(
                    msg,
                    "[boot] ipc buffer mapped @ 0x{ipc_vaddr:08x}",
                    ipc_vaddr = ipc_vaddr,
                );
                console.writeln_prefixed(msg.as_str());
            }
            Err(code) => {
                panic!(
                    "ipc buffer install failed: {} ({})",
                    code,
                    error_name(code as sel4_sys::seL4_Error)
                );
            }
        }
    } else {
        console.writeln_prefixed("bootinfo.ipcBuffer missing");
    }

    if let Some(ipc_ptr) = ipc_buffer_ptr {
        unsafe {
            sel4_sys::seL4_SetIPCBuffer(ipc_ptr.as_ptr());
        }
        let mut msg = heapless::String::<64>::new();
        let _ = write!(msg, "ipc buffer ptr=0x{:016x}", ipc_ptr.as_ptr() as usize);
        console.writeln_prefixed(msg.as_str());
    }

    let fault_handler_err = unsafe {
        sel4_sys::seL4_TCB_SetFaultHandler(
            tcb_copy_slot,
            ep_slot,
            sel4_sys::seL4_CapInitThreadCNode,
            sel4_sys::seL4_CapInitThreadVSpace,
        )
    };
    if let Err(code) = crate::bootstrap::ktry("tcb.fault_handler", fault_handler_err as i32) {
        let mut line = heapless::String::<160>::new();
        let _ = write!(
            line,
            "failed to install fault handler: {} ({})",
            code,
            error_name(fault_handler_err)
        );
        console.writeln_prefixed(line.as_str());
        panic!(
            "seL4_TCB_SetFaultHandler failed: {} ({})",
            code,
            error_name(fault_handler_err)
        );
    } else {
        log::info!(
            "[tcb] fault handler installed tcb_slot=0x{slot:04x} ep=0x{ep:04x}",
            slot = tcb_copy_slot,
            ep = ep_slot
        );
    }

    let mut cs = CSpaceCtx::new(bootinfo_view, boot_cspace);
    cs.tcb_copy_slot = tcb_copy_slot;
    // Track bootstrap slot usage: the TCB copy plus the earlier endpoint bootstrap
    // consume two slots before we begin retyping.
    let mut consumed_slots: usize = 2;

    let notification_untyped = pick_untyped(bootinfo_ref, sel4_sys::seL4_NotificationBits as u8);

    let notification_slot = retype_one(
        &mut cs,
        notification_untyped,
        sel4_sys::seL4_ObjectType::seL4_NotificationObject,
        0,
    )
    .expect("failed to retype notification into init CSpace");
    consumed_slots += 1;
    let _ = notification_slot;

    let mint_result = cs.mint_root_cnode_copy();
    match mint_result {
        Ok(()) => {
            consumed_slots += 1;
            debug_assert_ne!(
                cs.root_cnode_copy_slot,
                sel4_sys::seL4_CapNull,
                "mint_root_cnode_copy must populate root_cnode_copy_slot"
            );
        }
        Err(err) => {
            panic!(
                "failed to mint writable init CNode capability: {} ({})",
                err,
                error_name(err)
            );
        }
    }

    let empty_start = bootinfo_ref.empty_first_slot();
    let empty_end = bootinfo_ref.empty_last_slot_excl();
    let mut cnode_line = heapless::String::<160>::new();
    let empty_span = empty_end.saturating_sub(empty_start);
    let _ = write!(
        cnode_line,
        "bootinfo.empty slots [0x{start:04x}..0x{end:04x}) span={span} root_cnode_bits={bits}",
        start = empty_start,
        end = empty_end,
        span = empty_span,
        bits = bootinfo_ref.init_cnode_bits(),
    );
    console.writeln_prefixed(cnode_line.as_str());
    let mut hal = KernelHal::new(kernel_env);
    if consumed_slots > 0 {
        hal.consume_bootstrap_slots(consumed_slots);
    }

    #[cfg(feature = "kernel")]
    let ninedoor: &'static mut NineDoorBridge = {
        let bridge = Box::new(NineDoorBridge::new());
        Box::leak(bridge)
    };

    let uart_region = match hal.map_device(PL011_PADDR) {
        Ok(region) => region,
        Err(HalError::Sel4(err)) => {
            let error_code = err as i32;
            let error_label = error_name(err);
            let mut line = heapless::String::<128>::new();
            let _ = write!(
                line,
                "map_device(0x{addr:08x}) failed with {label} ({code})",
                addr = PL011_PADDR,
                label = error_label,
                code = error_code,
            );
            console.writeln_prefixed(line.as_str());

            let snapshot = hal.snapshot();
            let mut window = heapless::String::<160>::new();
            let _ = write!(
                window,
                "device_window base=0x{dbase:08x} cursor=0x{dcursor:08x}; dma_window base=0x{dmabase:08x} cursor=0x{dmacursor:08x}",
                dbase = snapshot.device_base,
                dcursor = snapshot.device_cursor,
                dmabase = snapshot.dma_base,
                dmacursor = snapshot.dma_cursor,
            );
            console.writeln_prefixed(window.as_str());

            let mut cspace = heapless::String::<160>::new();
            let _ = write!(
                cspace,
                "cspace used={used} remaining={remaining} capacity={capacity}",
                used = snapshot.cspace_used,
                remaining = snapshot.cspace_remaining,
                capacity = snapshot.cspace_capacity,
            );
            console.writeln_prefixed(cspace.as_str());

            let mut vspace = heapless::String::<192>::new();
            let _ = write!(
                vspace,
                "translation_state tables={tables} directories={directories} upper_directories={upper}",
                tables = snapshot.page_tables_mapped,
                directories = snapshot.page_directories_mapped,
                upper = snapshot.page_upper_directories_mapped,
            );
            console.writeln_prefixed(vspace.as_str());

            let mut root_info = heapless::String::<160>::new();
            let _ = write!(
                root_info,
                "cspace.root=0x{root:04x} depth={depth}",
                root = snapshot.cspace_root,
                depth = snapshot.cspace_root_depth,
            );
            console.writeln_prefixed(root_info.as_str());

            let stats = snapshot.untyped;
            let mut untyped = heapless::String::<192>::new();
            let _ = write!(
                untyped,
                "untyped total={total} used={used}; device total={dev_total} used={dev_used}",
                total = stats.total,
                used = stats.used,
                dev_total = stats.device_total,
                dev_used = stats.device_used,
            );
            console.writeln_prefixed(untyped.as_str());

            if let Some(last) = snapshot.last_retype {
                let mut detail = heapless::String::<256>::new();
                match last.status {
                    RetypeStatus::Pending => {
                        let _ = write!(
                            detail,
                            "retype status=pending raw.untyped=0x{ucap:08x} raw.paddr=0x{paddr:08x} raw.size_bits={usize_bits} raw.slot=0x{slot:04x} raw.offset={offset} raw.depth={depth} raw.root=0x{root:04x} raw.node_index=0x{node_index:04x} obj_type={otype} obj_size_bits={obj_bits}",
                            ucap = last.trace.untyped_cap,
                            paddr = last.trace.untyped_paddr,
                            usize_bits = last.trace.untyped_size_bits,
                            slot = last.trace.dest_slot,
                            offset = last.trace.dest_offset,
                            depth = last.trace.cnode_depth,
                            root = last.trace.cnode_root,
                            node_index = last.trace.node_index,
                            otype = last.trace.object_type,
                            obj_bits = last.trace.object_size_bits,
                        );
                    }
                    RetypeStatus::Ok => {
                        let _ = write!(
                            detail,
                            "retype status=ok raw.untyped=0x{ucap:08x} raw.paddr=0x{paddr:08x} raw.size_bits={usize_bits} raw.slot=0x{slot:04x} raw.offset={offset} raw.depth={depth} raw.root=0x{root:04x} raw.node_index=0x{node_index:04x} obj_type={otype} obj_size_bits={obj_bits}",
                            ucap = last.trace.untyped_cap,
                            paddr = last.trace.untyped_paddr,
                            usize_bits = last.trace.untyped_size_bits,
                            slot = last.trace.dest_slot,
                            offset = last.trace.dest_offset,
                            depth = last.trace.cnode_depth,
                            root = last.trace.cnode_root,
                            node_index = last.trace.node_index,
                            otype = last.trace.object_type,
                            obj_bits = last.trace.object_size_bits,
                        );
                    }
                    RetypeStatus::Err(code) => {
                        let _ = write!(
                            detail,
                            "retype status=err({code}) raw.untyped=0x{ucap:08x} raw.paddr=0x{paddr:08x} raw.size_bits={usize_bits} raw.slot=0x{slot:04x} raw.offset={offset} raw.depth={depth} raw.root=0x{root:04x} raw.node_index=0x{node_index:04x} obj_type={otype} obj_size_bits={obj_bits}",
                            code = code as i32,
                            ucap = last.trace.untyped_cap,
                            paddr = last.trace.untyped_paddr,
                            usize_bits = last.trace.untyped_size_bits,
                            slot = last.trace.dest_slot,
                            offset = last.trace.dest_offset,
                            depth = last.trace.cnode_depth,
                            root = last.trace.cnode_root,
                            node_index = last.trace.node_index,
                            otype = last.trace.object_type,
                            obj_bits = last.trace.object_size_bits,
                        );
                    }
                }
                console.writeln_prefixed(detail.as_str());

                let mut kind = heapless::String::<192>::new();
                match last.trace.kind {
                    RetypeKind::DevicePage { paddr } => {
                        let _ = write!(
                            kind,
                            "retype.kind=device_page target_paddr=0x{paddr:08x}",
                            paddr = paddr,
                        );
                    }
                    RetypeKind::DmaPage { paddr } => {
                        let _ = write!(
                            kind,
                            "retype.kind=dma_page target_paddr=0x{paddr:08x}",
                            paddr = paddr,
                        );
                    }
                    RetypeKind::PageTable { vaddr } => {
                        let _ = write!(
                            kind,
                            "retype.kind=page_table base_vaddr=0x{vaddr:08x}",
                            vaddr = vaddr,
                        );
                    }
                    RetypeKind::PageDirectory { vaddr } => {
                        let _ = write!(
                            kind,
                            "retype.kind=page_directory base_vaddr=0x{vaddr:08x}",
                            vaddr = vaddr,
                        );
                    }
                    RetypeKind::PageUpperDirectory { vaddr } => {
                        let _ = write!(
                            kind,
                            "retype.kind=page_upper_directory base_vaddr=0x{vaddr:08x}",
                            vaddr = vaddr,
                        );
                    }
                }
                console.writeln_prefixed(kind.as_str());

                let mut init = heapless::String::<192>::new();
                let _ = write!(
                    init,
                    "retype.init_cnode cap=0x{cap:04x} slot=0x{slot:04x} bits={bits} max_slots={max}",
                    cap = last.init_cnode_cap,
                    slot = last.init_cnode_slot,
                    bits = last.init_cnode_bits,
                    max = last.init_cnode_capacity,
                );
                console.writeln_prefixed(init.as_str());

                if let Some(sanitised) = last.sanitised {
                    let mut sanitised_line = heapless::String::<224>::new();
                    let _ = write!(
                        sanitised_line,
                        "retype.sanitised root=0x{root:04x} index=0x{index:04x} depth={depth} offset=0x{offset:04x}",
                        root = sanitised.cnode_root,
                        index = sanitised.node_index,
                        depth = sanitised.cnode_depth,
                        offset = sanitised.dest_offset,
                    );
                    console.writeln_prefixed(sanitised_line.as_str());
                } else if let Some(error) = last.sanitise_error {
                    let mut error_line = heapless::String::<224>::new();
                    let _ = write!(error_line, "retype.sanitise_error={error}");
                    console.writeln_prefixed(error_line.as_str());
                }

                let expected_depth = last.canonical_cnode_depth as usize;
                let actual_depth = last.trace.cnode_depth as usize;
                if actual_depth != expected_depth {
                    let mut depth = heapless::String::<192>::new();
                    let _ = write!(
                        depth,
                        "retype.cnode_depth mismatch: expected={expected} (canonical root depth) actual={actual}",
                        expected = expected_depth,
                        actual = actual_depth,
                    );
                    console.writeln_prefixed(depth.as_str());
                }

                let dest = last.trace.dest_offset as usize;
                if dest >= last.init_cnode_capacity {
                    let mut offset = heapless::String::<192>::new();
                    let _ = write!(
                        offset,
                        "retype.dest_offset out of range: offset=0x{dest:04x} limit=0x{limit:04x}",
                        dest = dest,
                        limit = last.init_cnode_capacity,
                    );
                    console.writeln_prefixed(offset.as_str());
                }
            } else {
                console.writeln_prefixed("no retype trace captured");
            }

            match hal.device_coverage(PL011_PADDR, DEVICE_FRAME_BITS) {
                Some(region) => {
                    let mut coverage = heapless::String::<192>::new();
                    let region_state = if region.used { "reserved" } else { "free" };
                    let _ = write!(
                        coverage,
                        "device coverage idx={index} [{base:#010x}..{limit:#010x}) size_bits={size} state={state}",
                        index = region.index,
                        base = region.base,
                        limit = region.limit,
                        size = region.size_bits,
                        state = region_state,
                    );
                    console.writeln_prefixed(coverage.as_str());
                }
                None => {
                    console.writeln_prefixed("no device untyped covers requested PL011 range");
                }
            }

            panic!("PL011 UART mapping failed: {}", err);
        }
    };

    let mapped_vaddr = uart_region.ptr().as_ptr() as usize;
    let mut map_line = heapless::String::<128>::new();
    let _ = write!(
        map_line,
        "PL011 mapped @ 0x{vaddr:016x} (paddr=0x{paddr:08x})",
        vaddr = mapped_vaddr,
        paddr = PL011_PADDR,
    );
    console.writeln_prefixed(map_line.as_str());

    let mut driver = Pl011::new(uart_region.ptr());
    driver.init();
    #[cfg(all(feature = "kernel", not(sel4_config_printing)))]
    {
        unsafe {
            EARLY_UART_SINK = DebugSink {
                context: uart_region.ptr().as_ptr().cast::<()>(),
                emit: pl011_debug_emit,
            };
        }

        let sink = unsafe { EARLY_UART_SINK };
        let emit_addr = sink.emit as usize;
        let ctx_addr = sink.context as usize;
        let mut sink_line = heapless::String::<128>::new();
        let _ = write!(
            sink_line,
            "[debug-sink] emit=0x{emit:016x} ctx=0x{ctx:016x}",
            emit = emit_addr,
            ctx = ctx_addr,
        );
        console.writeln_prefixed(sink_line.as_str());
        if emit_addr & 0b11 != 0 {
            panic!(
                "debug sink emit pointer not 4-byte aligned: 0x{emit:016x}",
                emit = emit_addr,
            );
        }
        if emit_addr <= 0x1000 {
            panic!(
                "debug sink emit pointer unexpectedly low: 0x{emit:016x}",
                emit = emit_addr,
            );
        }
        if ctx_addr <= 0x1000 {
            panic!(
                "debug sink context pointer unexpectedly low: 0x{ctx:016x}",
                ctx = ctx_addr,
            );
        }
        sel4_panicking::install_debug_sink(sink);
    }
    driver.write_str("[cohesix:root-task] uart logger online\n");

    #[cfg(feature = "debug-input")]
    {
        start_console(driver);
    }

    #[cfg(not(feature = "debug-input"))]
    {
        let serial =
            SerialPort::<_, DEFAULT_RX_CAPACITY, DEFAULT_TX_CAPACITY, DEFAULT_LINE_CAPACITY>::new(
                driver,
            );

        #[cfg(all(feature = "net-console", feature = "kernel"))]
        let mut net_stack = NetStack::new(&mut hal).expect("virtio-net device not found");
        #[cfg(all(feature = "net-console", not(feature = "kernel")))]
        let (mut net_stack, _) = NetStack::new(Ipv4Address::new(10, 0, 0, 2));
        let timer = KernelTimer::new(5);
        crate::bp!("ipc.poll.begin");
        let mut ipc = KernelIpc::new(ep_slot);
        ipc.bootstrap_probe();
        crate::bp!("ipc.poll.end");

        let mut tickets: TicketTable<4> = TicketTable::new();
        let _ = tickets.register(Role::Queen, "bootstrap");
        let _ = tickets.register(Role::WorkerHeartbeat, "worker");
        let _ = tickets.register(Role::WorkerGpu, "worker-gpu");

        crate::bp!("spawn.worker.begin");
        crate::bp!("spawn.worker.end");

        crate::bp!("dtb.parse.begin");
        if !extra_bytes.is_empty() {
            match bi_extra::locate_dtb(extra_bytes) {
                Ok(dtb_blob) => match bi_extra::parse_dtb(dtb_blob) {
                    Ok(dtb) => {
                        let header = dtb.header();
                        let mut msg = heapless::String::<96>::new();
                        let _ = write!(
                            msg,
                            "[boot] dtb totalsize={} struct_off={} strings_off={}",
                            header.totalsize(),
                            header.structure_offset(),
                            header.strings_offset(),
                        );
                        console.writeln_prefixed(msg.as_str());
                        let _ = bi_extra::dump_bootinfo(&bootinfo_view, EARLY_DUMP_LIMIT);
                    }
                    Err(err) => {
                        let mut msg = heapless::String::<96>::new();
                        let _ = write!(msg, "[boot] dtb parse failed: {err}");
                        console.writeln_prefixed(msg.as_str());
                    }
                },
                Err(err) => {
                    let mut msg = heapless::String::<112>::new();
                    let _ = write!(msg, "[boot] dtb locate failed: {err}");
                    console.writeln_prefixed(msg.as_str());
                }
            }
        } else {
            console.writeln_prefixed("[boot] no dtb payload present");
        }
        crate::bp!("dtb.parse.end");

        crate::bp!("logger.switch.begin");
        if let Err(err) = boot_log::switch_logger_to_userland() {
            log::error!("[boot] logger switch failed: {:?}", err);
            panic!("logger switch failed: {err:?}");
        }
        crate::bp!("logger.switch.end");
        #[cfg(all(feature = "net-console", feature = "kernel"))]
        {
            let mac = net_stack.hardware_address();
            let ip = net_stack.ipv4_address();
            let prefix = net_stack.prefix_len();
            let mut banner = heapless::String::<128>::new();
            if let Some(gw) = net_stack.gateway() {
                let _ = write!(banner, "[net] virtio up mac={mac} ip={ip}/{prefix} gw={gw}");
            } else {
                let _ = write!(banner, "[net] virtio up mac={mac} ip={ip}/{prefix}");
            }
            console.writeln_prefixed(banner.as_str());
            let mut listen = heapless::String::<64>::new();
            let _ = write!(listen, "[console] tcp listen :{CONSOLE_TCP_PORT}");
            console.writeln_prefixed(listen.as_str());
        }
        console.writeln_prefixed("initialising event pump");
        let mut audit = ConsoleAudit::new(&mut console);
        #[cfg(feature = "kernel")]
        let mut bootstrap_ipc = BootstrapIpcAudit::new();
        let mut pump = EventPump::new(serial, timer, ipc, tickets, &mut audit);

        #[cfg(feature = "kernel")]
        {
            pump = pump.with_bootstrap_handler(&mut bootstrap_ipc);
            pump = pump.with_ninedoor(ninedoor);
        }

        #[cfg(feature = "net-console")]
        {
            pump = pump.with_network(&mut net_stack);
        }

        crate::bp!("bootstrap.done");
        loop {
            pump.poll();
        }
    }
}

/// Panic handler implementation that emits diagnostics before halting.
pub fn panic_handler(info: &PanicInfo) -> ! {
    let platform = SeL4Platform::new(core::ptr::null());
    let mut console = DebugConsole::new(&platform);
    let _ = write!(
        console,
        "{prefix}panic: {info}\r\n",
        prefix = DebugConsole::<SeL4Platform>::PREFIX,
        info = info
    );
    loop {
        core::hint::spin_loop();
    }
}

struct KernelTimer {
    tick: u64,
    period_ms: u64,
}

impl KernelTimer {
    fn new(period_ms: u64) -> Self {
        Self { tick: 0, period_ms }
    }
}

impl TimerSource for KernelTimer {
    fn poll(&mut self, now_ms: u64) -> Option<TickEvent> {
        self.tick = self.tick.saturating_add(1);
        Some(TickEvent {
            tick: self.tick,
            now_ms: now_ms.saturating_add(self.period_ms),
        })
    }
}

const MAX_MESSAGE_WORDS: usize = MSG_MAX_WORDS;
const MAX_PAYLOAD_LOG_BYTES: usize = 512;
const HEX_CHUNK_BYTES: usize = 16;
const MAX_HEX_LINES: usize = (MAX_PAYLOAD_LOG_BYTES + HEX_CHUNK_BYTES - 1) / HEX_CHUNK_BYTES;

#[inline]
fn bounded_message_words(info: sel4_sys::seL4_MessageInfo) -> usize {
    cmp::min(info.length() as usize, MAX_MESSAGE_WORDS)
}

fn copy_message_words<F>(
    info: sel4_sys::seL4_MessageInfo,
    mut read_word: F,
) -> HeaplessVec<sel4_sys::seL4_Word, { MAX_MESSAGE_WORDS }>
where
    F: FnMut(usize) -> sel4_sys::seL4_Word,
{
    let mut payload = HeaplessVec::new();
    let word_count = bounded_message_words(info);
    for index in 0..word_count {
        let word = read_word(index);
        payload
            .push(word)
            .expect("payload length bounded by MAX_MESSAGE_WORDS");
    }
    payload
}

#[derive(Debug, PartialEq, Eq)]
enum PayloadPreview {
    Empty,
    Utf8(HeaplessString<{ MAX_PAYLOAD_LOG_BYTES }>),
    Hex(HeaplessVec<HeaplessString<96>, { MAX_HEX_LINES }>),
}

fn preview_payload(words: &[sel4_sys::seL4_Word]) -> PayloadPreview {
    if words.is_empty() {
        return PayloadPreview::Empty;
    }

    let mut bytes: heapless::Vec<u8, { MAX_PAYLOAD_LOG_BYTES }> = heapless::Vec::new();
    'outer: for &word in words {
        for byte in word.to_le_bytes() {
            if bytes.len() == MAX_PAYLOAD_LOG_BYTES {
                break 'outer;
            }
            bytes
                .push(byte)
                .expect("bytes length bounded by MAX_PAYLOAD_LOG_BYTES");
        }
    }

    if bytes.is_empty() {
        return PayloadPreview::Empty;
    }

    match core::str::from_utf8(bytes.as_slice()) {
        Ok(text) => {
            let mut owned = HeaplessString::<{ MAX_PAYLOAD_LOG_BYTES }>::new();
            let _ = owned.push_str(text);
            PayloadPreview::Utf8(owned)
        }
        Err(_) => {
            let mut lines: HeaplessVec<HeaplessString<96>, { MAX_HEX_LINES }> = HeaplessVec::new();
            let mut offset = 0usize;
            for chunk in bytes.as_slice().chunks(HEX_CHUNK_BYTES) {
                let mut line = HeaplessString::<96>::new();
                let _ = write!(line, "[staged hex] {:04x}:", offset);
                for byte in chunk {
                    let _ = write!(line, " {:02x}", byte);
                }
                lines
                    .push(line)
                    .expect("hex preview must not exceed MAX_HEX_LINES");
                offset += chunk.len();
            }
            PayloadPreview::Hex(lines)
        }
    }
}

fn log_bootstrap_payload(words: &[sel4_sys::seL4_Word]) {
    match preview_payload(words) {
        PayloadPreview::Empty => {}
        PayloadPreview::Utf8(text) => {
            log::info!("[staged utf8] {}", text.as_str());
        }
        PayloadPreview::Hex(lines) => {
            for line in lines {
                log::info!("{}", line.as_str());
            }
        }
    }
}

struct StagedMessage {
    badge: sel4_sys::seL4_Word,
    info: sel4_sys::seL4_MessageInfo,
    payload: HeaplessVec<sel4_sys::seL4_Word, { MAX_MESSAGE_WORDS }>,
}

impl StagedMessage {
    fn new(info: sel4_sys::seL4_MessageInfo, badge: sel4_sys::seL4_Word) -> Self {
        let payload = copy_message_words(info, |index| unsafe { sel4_sys::seL4_GetMR(index) });
        Self {
            badge,
            info,
            payload,
        }
    }

    fn is_empty(&self) -> bool {
        self.badge == 0 && self.info.length() == 0 && self.payload.is_empty()
    }

    #[cfg(test)]
    fn from_parts(
        info: sel4_sys::seL4_MessageInfo,
        badge: sel4_sys::seL4_Word,
        payload: &[sel4_sys::seL4_Word],
    ) -> Self {
        let mut buffer = HeaplessVec::new();
        for &word in payload.iter().take(MAX_MESSAGE_WORDS) {
            buffer
                .push(word)
                .expect("test payload respects MAX_MESSAGE_WORDS");
        }
        Self {
            badge,
            info,
            payload: buffer,
        }
    }
}

impl From<StagedMessage> for BootstrapMessage {
    fn from(message: StagedMessage) -> Self {
        Self {
            badge: message.badge,
            info: message.info,
            payload: message.payload,
        }
    }
}

struct KernelIpc {
    endpoint: sel4_sys::seL4_CPtr,
    staged_bootstrap: Option<StagedMessage>,
    staged_forwarded: bool,
    handlers_ready: bool,
}

impl KernelIpc {
    fn new(endpoint: sel4_sys::seL4_CPtr) -> Self {
        Self {
            endpoint,
            staged_bootstrap: None,
            staged_forwarded: false,
            handlers_ready: false,
        }
    }

    fn bootstrap_probe(&mut self) {
        if self.staged_bootstrap.is_some() {
            return;
        }

        let mut badge: sel4_sys::seL4_Word = 0;
        let info = unsafe { sel4_sys::seL4_Poll(self.endpoint, &mut badge) };
        log::info!(
            "[boot] poll ep=0x{ep:04x} badge=0x{badge:016x} info=0x{info:08x}",
            ep = self.endpoint,
            badge = badge,
            info = info.words[0],
        );

        self.staged_bootstrap = Some(StagedMessage::new(info, badge));
        self.staged_forwarded = false;
    }

    fn forward_staged(&mut self, now_ms: u64) {
        let Some(message) = self.staged_bootstrap.as_ref() else {
            return;
        };
        if self.staged_forwarded {
            return;
        }

        if message.is_empty() {
            log::trace!(
                "[ipc] bootstrap poll observed empty queue at {now_ms}ms",
                now_ms = now_ms
            );
        } else {
            log::info!(
                "[ipc] bootstrap staged ep=0x{ep:04x} badge=0x{badge:016x} info=0x{info:08x} words={words}",
                ep = self.endpoint,
                badge = message.badge,
                info = message.info.words[0],
                words = message.payload.len(),
            );
            log_bootstrap_payload(message.payload.as_slice());
        }
        self.staged_forwarded = true;
    }
}

impl IpcDispatcher for KernelIpc {
    fn dispatch(&mut self, now_ms: u64) {
        if self.handlers_ready {
            self.forward_staged(now_ms);
        }
    }

    fn handlers_ready(&mut self) {
        self.handlers_ready = true;
    }

    fn take_bootstrap_message(&mut self) -> Option<BootstrapMessage> {
        let staged = self.staged_bootstrap.take()?;
        self.staged_forwarded = false;
        Some(staged.into())
    }
}

struct BootstrapIpcAudit;

impl BootstrapIpcAudit {
    fn new() -> Self {
        Self
    }
}

impl BootstrapMessageHandler for BootstrapIpcAudit {
    fn handle(&mut self, message: &BootstrapMessage, audit: &mut dyn AuditSink) {
        let mut summary = heapless::String::<128>::new();
        let _ = write!(
            summary,
            "[ipc] bootstrap dispatch badge=0x{badge:016x} label=0x{label:08x} words={words}",
            badge = message.badge,
            label = message.info.words[0],
            words = message.payload.len(),
        );
        audit.info(summary.as_str());

        if !message.payload.is_empty() {
            let mut payload_line = heapless::String::<192>::new();
            let _ = payload_line.push_str("[ipc] bootstrap payload");
            for (index, word) in message.payload.iter().enumerate() {
                let _ = write!(payload_line, " w{index}=0x{word:016x}");
            }
            audit.info(payload_line.as_str());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_message_words, copy_message_words, preview_payload, KernelIpc, PayloadPreview,
        StagedMessage, HEX_CHUNK_BYTES, MAX_HEX_LINES, MAX_MESSAGE_WORDS, MAX_PAYLOAD_LOG_BYTES,
    };
    use core::fmt::Write as _;
    use heapless::{String as HeaplessString, Vec as HeaplessVec};

    #[test]
    fn staged_message_reports_empty() {
        let info = sel4_sys::seL4_MessageInfo::new(0, 0, 0, 0);
        let staged = StagedMessage::from_parts(info, 0, &[]);
        assert!(staged.is_empty());
    }

    #[test]
    fn staged_message_detects_payload() {
        let info = sel4_sys::seL4_MessageInfo::new(0x42, 0, 0, 2);
        let staged = StagedMessage::from_parts(info, 0x99, &[1, 2]);
        assert!(!staged.is_empty());
    }

    #[test]
    fn kernel_ipc_drains_staged_message() {
        let info = sel4_sys::seL4_MessageInfo::new(0x11, 0, 0, 2);
        let staged = StagedMessage::from_parts(info, 0xAA, &[0xFE, 0xED]);

        let mut ipc = KernelIpc::new(0x200);
        ipc.staged_bootstrap = Some(staged);

        ipc.dispatch(0);
        let message = ipc
            .take_bootstrap_message()
            .expect("staged message should be drained");
        assert_eq!(message.badge, 0xAA);
        assert_eq!(message.info.words[0], info.words[0]);
        assert_eq!(message.payload.as_slice(), &[0xFE, 0xED]);
        assert!(ipc.take_bootstrap_message().is_none());
    }

    #[test]
    fn copy_message_words_clamps_to_kernel_limit() {
        let info = sel4_sys::seL4_MessageInfo::new(0x11, 0, 0, 127);
        let mut source = [0usize; MAX_MESSAGE_WORDS + 16];
        for (index, word) in source.iter_mut().enumerate() {
            *word = index as usize;
        }
        let copied = copy_message_words(info, |index| source[index]);
        assert_eq!(copied.len(), MAX_MESSAGE_WORDS);
        assert_eq!(bounded_message_words(info), MAX_MESSAGE_WORDS);
        assert_eq!(copied[0], 0);
        assert_eq!(copied[MAX_MESSAGE_WORDS - 1], MAX_MESSAGE_WORDS - 1);
    }

    #[test]
    fn preview_payload_emits_utf8_when_valid() {
        let text = b"hello world!";
        let word_bytes = core::mem::size_of::<usize>();
        let mut chunk = [0u8; core::mem::size_of::<usize>()];
        let mut words: HeaplessVec<sel4_sys::seL4_Word, { MAX_MESSAGE_WORDS }> = HeaplessVec::new();
        for (index, byte) in text.iter().enumerate() {
            let offset = index % word_bytes;
            chunk[offset] = *byte;
            if offset + 1 == word_bytes {
                let value = usize::from_le_bytes(chunk) as sel4_sys::seL4_Word;
                words.push(value).expect("utf8 payload within limit");
                chunk.fill(0);
            }
        }
        if text.len() % word_bytes != 0 {
            let value = usize::from_le_bytes(chunk) as sel4_sys::seL4_Word;
            words.push(value).expect("utf8 payload within limit");
        }
        match preview_payload(words.as_slice()) {
            PayloadPreview::Utf8(text) => assert!(text.as_str().starts_with("hello world")),
            other => panic!("expected utf8 preview, got {other:?}"),
        }
    }

    #[test]
    fn preview_payload_emits_hex_for_binary() {
        let words = [usize::MAX; 2];
        match preview_payload(&words) {
            PayloadPreview::Hex(lines) => {
                assert!(!lines.is_empty());
                assert!(lines[0].starts_with("[staged hex] 0000:"));
            }
            other => panic!("expected hex preview, got {other:?}"),
        }
    }

    #[test]
    fn preview_payload_truncates_to_cap() {
        let words = [usize::MAX; MAX_MESSAGE_WORDS];
        match preview_payload(&words) {
            PayloadPreview::Hex(lines) => {
                assert_eq!(lines.len(), MAX_HEX_LINES);
                let last = lines.last().expect("at least one hex line");
                let expected_offset = (MAX_PAYLOAD_LOG_BYTES - HEX_CHUNK_BYTES) as u32;
                let mut expected = HeaplessString::<32>::new();
                let _ = write!(expected, "[staged hex] {expected_offset:04x}:");
                assert!(last.starts_with(expected.as_str()));
            }
            other => panic!("expected hex preview, got {other:?}"),
        }
    }
}

struct ConsoleAudit<'a, P: Platform> {
    console: &'a mut DebugConsole<'a, P>,
}

impl<'a, P: Platform> ConsoleAudit<'a, P> {
    fn new(console: &'a mut DebugConsole<'a, P>) -> Self {
        Self { console }
    }
}

impl<'a, P: Platform> AuditSink for ConsoleAudit<'a, P> {
    fn info(&mut self, message: &str) {
        self.console.writeln_prefixed(message);
    }

    fn denied(&mut self, message: &str) {
        self.console.writeln_prefixed(message);
    }
}
