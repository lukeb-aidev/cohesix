// Author: Lukas Bower
#![allow(dead_code)]
#![allow(unsafe_code)]

use core::cell::UnsafeCell;
use core::fmt::{self, Write};
use core::mem::{self, MaybeUninit};
use core::panic::PanicInfo;
use core::ptr;

use cohesix_ticket::Role;

use crate::bootstrap::{
    cspace::{BootInfoView, CSpaceCtx},
    pick_untyped,
    retype::retype_one,
};
use crate::console::Console;
use crate::cspace::{cap_rights_read_write_grant, CSpace};
use crate::event::{AuditSink, EventPump, IpcDispatcher, TickEvent, TicketTable, TimerSource};
#[cfg(feature = "net-console")]
use crate::net::{NetStack, CONSOLE_TCP_PORT};
use crate::platform::{Platform, SeL4Platform};
use crate::sel4::{
    bootinfo_debug_dump, error_name, BootInfo, BootInfoExt, KernelEnv, RetypeKind, RetypeStatus,
};
use crate::serial::{
    pl011::Pl011, SerialPort, DEFAULT_LINE_CAPACITY, DEFAULT_RX_CAPACITY, DEFAULT_TX_CAPACITY,
};
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

    fn report_bootinfo(&mut self, info_ptr: *const BootInfoHeader) {
        if info_ptr.is_null() {
            self.writeln_prefixed("bootinfo pointer is NULL (kernel handover failed)");
            return;
        }

        let addr = info_ptr as usize;
        let header = unsafe { *info_ptr };
        let extra_words = header.extra_len;
        let header_bytes = core::mem::size_of::<BootInfoHeader>();
        let extra_bytes = extra_words * core::mem::size_of::<usize>();
        let extra_start = addr + header_bytes;
        let extra_end = extra_start + extra_bytes;

        let _ = write!(
            self,
            "{prefix}bootinfo @ 0x{addr:016x} (header {header_bytes} bytes)\r\n",
            prefix = Self::PREFIX,
            addr = addr,
            header_bytes = header_bytes,
        );
        let _ = write!(
            self,
            "{prefix}bootinfo.extraLen = {extra_words} words ({extra_bytes} bytes)\r\n",
            prefix = Self::PREFIX,
            extra_words = extra_words,
            extra_bytes = extra_bytes,
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
            node_id = header.node_id,
            nodes = header.num_nodes,
            ipc = header.ipc_buffer,
        );
    }
}

impl<'a, P: Platform> Write for DebugConsole<'a, P> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_raw(s.as_bytes());
        Ok(())
    }
}

/// Minimal projection of `seL4_BootInfo` used for early diagnostics.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct BootInfoHeader {
    extra_len: usize,
    node_id: usize,
    num_nodes: usize,
    num_io_pt_levels: usize,
    ipc_buffer: usize,
    init_thread_cnode_size_bits: usize,
    init_thread_domain: usize,
    extra_bi_pages: usize,
}

#[cfg(not(target_arch = "aarch64"))]
compile_error!("root-task kernel build currently supports only aarch64 targets");

const PL011_PADDR: usize = 0x0900_0000;
const DEVICE_FRAME_BITS: usize = 12;

#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
const PL011_DR_OFFSET: usize = 0x00;
#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
const PL011_FR_OFFSET: usize = 0x18;
#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
const PL011_FR_TXFF: u32 = 1 << 5;

#[cfg(target_arch = "aarch64")]
static mut TLS_IMAGE: sel4_sys::TlsImage = sel4_sys::TlsImage::new();

#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
unsafe fn pl011_debug_emit(context: *mut (), byte: u8) {
    let base = context.cast::<u8>();
    let dr = base.add(PL011_DR_OFFSET).cast::<u32>();
    let fr = base.add(PL011_FR_OFFSET).cast::<u32>();

    while ptr::read_volatile(fr) & PL011_FR_TXFF != 0 {
        core::hint::spin_loop();
    }

    ptr::write_volatile(dr, u32::from(byte));
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

/// Root task entry point invoked by seL4 after kernel initialisation.
pub fn start<P: Platform>(bootinfo: &'static BootInfo, platform: &P) -> ! {
    bootstrap(platform, bootinfo)
}

fn bootstrap<P: Platform>(platform: &P, bootinfo: &'static BootInfo) -> ! {
    #[cfg(all(feature = "kernel", not(sel4_config_printing)))]
    sel4::install_debug_sink();

    let mut console = DebugConsole::new(platform);
    console.writeln_prefixed("entered from seL4 (stage0)");
    console.writeln_prefixed("Cohesix boot: root-task online");

    let bootinfo_ptr = bootinfo as *const BootInfo as *const BootInfoHeader;
    console.report_bootinfo(bootinfo_ptr);

    console.writeln_prefixed("Cohesix v0 (AArch64/virt)");

    let bootinfo_ref: &'static sel4_sys::seL4_BootInfo = bootinfo;
    bootinfo_debug_dump(bootinfo_ref);

    unsafe {
        #[cfg(all(feature = "kernel", target_arch = "aarch64"))]
        {
            sel4_sys::tls_set_base(core::ptr::addr_of_mut!(TLS_IMAGE));
            debug_assert!(
                sel4_sys::tls_image_mut().is_some(),
                "TLS base must resolve to an image after installation",
            );
        }

        let (ipc_ptr, used_fallback) = initialise_ipc_buffer_with(bootinfo_ref, |ptr| {
            install_ipc_buffer(ptr);
        });

        if used_fallback {
            console.writeln_prefixed("bootinfo.ipcBuffer missing; using static fallback");
        }

        let mut msg = heapless::String::<64>::new();
        let _ = write!(msg, "ipc buffer ptr=0x{:016x}", ipc_ptr as usize);
        console.writeln_prefixed(msg.as_str());
    }

    let mut boot_cspace = CSpace::from_bootinfo(bootinfo_ref);
    let rights = cap_rights_read_write_grant();

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
    if copy_err != sel4_sys::seL4_NoError {
        panic!(
            "copying init TCB capability failed: {} ({})",
            copy_err,
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

    let cnode_copy_slot = match boot_cspace.alloc_slot() {
        Ok(slot) => slot,
        Err(err) => {
            panic!(
                "failed to allocate init CSpace slot for CNode mint: {} ({})",
                err,
                error_name(err)
            );
        }
    };
    let cnode_src_slot = bootinfo_ref.init_cnode_cap();
    let mint_err = boot_cspace.mint_here(cnode_copy_slot, cnode_src_slot, rights, 0);
    if mint_err != sel4_sys::seL4_NoError {
        panic!(
            "failed to mint writable init CNode capability: {} ({})",
            mint_err,
            error_name(mint_err)
        );
    } else {
        log::info!(
            "[cnode] mint root=0x{root:04x} dst=0x{dst:04x} src=0x{src:04x} depth={depth}",
            root = boot_cspace.root(),
            dst = cnode_copy_slot,
            src = cnode_src_slot,
            depth = boot_cspace.depth()
        );
    }

    let bi_view = BootInfoView::from(bootinfo_ref);
    let mut cs = CSpaceCtx::new(bi_view, boot_cspace);
    cs.tcb_copy_slot = tcb_copy_slot;
    cs.root_cnode_copy_slot = cnode_copy_slot;

    let endpoint_untyped = pick_untyped(bootinfo_ref, sel4_sys::seL4_EndpointBits as u8);
    let endpoint_slot = cs.first_free.saturating_add(2);
    let endpoint_err = cs.retype_to_slot(
        endpoint_untyped,
        sel4_sys::seL4_ObjectType::seL4_EndpointObject as sel4_sys::seL4_Word,
        0,
        endpoint_slot,
    );
    if endpoint_err != sel4_sys::seL4_NoError {
        panic!(
            "failed to retype endpoint into init CSpace: {} ({})",
            endpoint_err,
            error_name(endpoint_err)
        );
    }

    let mut consumed_slots: usize = 3;

    let notification_untyped = pick_untyped(bootinfo_ref, sel4_sys::seL4_NotificationBits as u8);

    let notification_slot = retype_one(
        &mut cs,
        notification_untyped,
        sel4_sys::seL4_ObjectType::seL4_NotificationObject,
        0,
    )
    .expect("failed to retype notification into init CSpace");
    consumed_slots += 1;
    let _ = endpoint_slot;
    let _ = notification_slot;

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
    let mut env = KernelEnv::new(bootinfo_ref);
    if consumed_slots > 0 {
        env.consume_bootstrap_slots(consumed_slots);
    }

    #[cfg(feature = "kernel")]
    let mut ninedoor = crate::ninedoor::NineDoorBridge::new();

    let uart_region = match env.map_device(PL011_PADDR) {
        Ok(region) => region,
        Err(err) => {
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

            let snapshot = env.snapshot();
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

            match env.device_coverage(PL011_PADDR, DEVICE_FRAME_BITS) {
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
        let sink = DebugSink {
            context: uart_region.ptr().as_ptr().cast::<()>(),
            emit: pl011_debug_emit,
        };
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
        let mut net_stack = NetStack::new(&mut env).expect("virtio-net device not found");
        #[cfg(all(feature = "net-console", not(feature = "kernel")))]
        let (mut net_stack, _) = NetStack::new(Ipv4Address::new(10, 0, 0, 2));
        let timer = KernelTimer::new(5);
        let ipc = KernelIpc;
        let mut tickets: TicketTable<4> = TicketTable::new();
        let _ = tickets.register(Role::Queen, "bootstrap");
        let _ = tickets.register(Role::WorkerHeartbeat, "worker");
        let _ = tickets.register(Role::WorkerGpu, "worker-gpu");
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
        let mut pump = EventPump::new(serial, timer, ipc, tickets, &mut audit);

        #[cfg(feature = "kernel")]
        {
            pump = pump.with_ninedoor(&mut ninedoor);
        }

        #[cfg(feature = "net-console")]
        {
            pump = pump.with_network(&mut net_stack);
        }

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

struct KernelIpc;

impl IpcDispatcher for KernelIpc {
    fn dispatch(&mut self, _now_ms: u64) {}
}

struct ConsoleAudit<'a, P: Platform> {
    console: &'a mut DebugConsole<'a, P>,
}

impl<'a, P: Platform> ConsoleAudit<'a, P> {
    fn new(console: &'a mut DebugConsole<'a, P>) -> Self {
        Self { console }
    }
}

#[inline(always)]
fn install_ipc_buffer(ptr: *mut sel4_sys::seL4_IPCBuffer) {
    debug_assert!(!ptr.is_null());
    unsafe {
        sel4_sys::seL4_SetIPCBuffer(ptr);
    }
}

struct FallbackIpcBuffer {
    buffer: UnsafeCell<MaybeUninit<sel4_sys::seL4_IPCBuffer>>,
}

impl FallbackIpcBuffer {
    const fn new() -> Self {
        Self {
            buffer: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    unsafe fn zeroed_ptr(&self) -> *mut sel4_sys::seL4_IPCBuffer {
        let ptr = (*self.buffer.get()).as_mut_ptr();
        zero_ipc_buffer(ptr);
        ptr
    }
}

unsafe impl Sync for FallbackIpcBuffer {}

unsafe fn initialise_ipc_buffer_with<F>(
    bootinfo: &sel4_sys::seL4_BootInfo,
    setter: F,
) -> (*mut sel4_sys::seL4_IPCBuffer, bool)
where
    F: Fn(*mut sel4_sys::seL4_IPCBuffer),
{
    static FALLBACK_IPC_BUFFER: FallbackIpcBuffer = FallbackIpcBuffer::new();

    let raw_ptr = bootinfo.ipcBuffer as *mut sel4_sys::seL4_IPCBuffer;
    let (buffer_ptr, used_fallback) = if raw_ptr.is_null() {
        let ptr = FALLBACK_IPC_BUFFER.zeroed_ptr();
        (ptr, true)
    } else {
        zero_ipc_buffer(raw_ptr);
        (raw_ptr, false)
    };

    setter(buffer_ptr);
    (buffer_ptr, used_fallback)
}

unsafe fn zero_ipc_buffer(ptr: *mut sel4_sys::seL4_IPCBuffer) {
    ptr::write_bytes(
        ptr.cast::<u8>(),
        0,
        mem::size_of::<sel4_sys::seL4_IPCBuffer>(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::MaybeUninit;

    #[test]
    fn initialise_ipc_buffer_prefers_bootinfo_pointer() {
        let mut bootinfo: sel4_sys::seL4_BootInfo = unsafe { core::mem::zeroed() };
        let mut backing = MaybeUninit::<sel4_sys::seL4_IPCBuffer>::uninit();
        let buffer_ptr = backing.as_mut_ptr();
        bootinfo.ipcBuffer = buffer_ptr as usize;

        let mut captured: *mut sel4_sys::seL4_IPCBuffer = core::ptr::null_mut();
        let (ptr, fallback) = unsafe {
            initialise_ipc_buffer_with(&bootinfo, |ipc_ptr| {
                captured = ipc_ptr;
            })
        };

        assert_eq!(ptr, buffer_ptr);
        assert_eq!(captured, buffer_ptr);
        assert!(!fallback);
    }

    #[test]
    fn initialise_ipc_buffer_uses_static_fallback_when_missing() {
        let bootinfo: sel4_sys::seL4_BootInfo = unsafe { core::mem::zeroed() };
        let mut captured: *mut sel4_sys::seL4_IPCBuffer = core::ptr::null_mut();
        let (ptr, fallback) = unsafe {
            initialise_ipc_buffer_with(&bootinfo, |ipc_ptr| {
                captured = ipc_ptr;
            })
        };

        assert!(fallback);
        assert_eq!(ptr, captured);
        assert!(!ptr.is_null());
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
