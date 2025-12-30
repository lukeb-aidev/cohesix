// Author: Lukas Bower
// Purpose: seL4 bootstrap, capability setup, and runtime hand-off for root-task.
#![allow(dead_code)]
#![allow(unsafe_code)]

extern crate alloc;

use alloc::{borrow::ToOwned, boxed::Box, format, string::String};
use core::cell::RefCell;
use core::cmp;
use core::convert::TryFrom;
use core::fmt::{self, Write};
use core::ops::{Range, RangeInclusive};
use core::panic::PanicInfo;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[cfg(feature = "timers-arch-counter")]
use core::arch::asm;

#[cfg(all(feature = "kernel", target_arch = "aarch64"))]
use crate::arch::aarch64::timer::timer_freq_hz;
#[cfg(feature = "kernel")]
use crate::audit::boot as audit_boot;
use crate::boot::{bi_extra, ep, tcb, uart_pl011};
#[cfg(feature = "cap-probes")]
use crate::bootstrap::cspace::cspace_first_retypes;
use crate::bootstrap::cspace_sys;
use crate::bootstrap::hard_guard::{hard_guard_fail, HardGuardViolation};
use crate::bootstrap::{
    boot_tracer,
    bootinfo_snapshot::{BootInfoCanaryError, BootInfoSnapshot},
    cspace::{CSpaceCtx, CSpaceWindow, FirstRetypeResult},
    device_pt_pool, ensure_device_pt_pool, ipcbuf, layout, log as boot_log,
    phases::{
        canonical_bootinfo_view, snapshot_bootinfo, BootstrapPhase, BootstrapSequencer,
        FatalBootstrapError,
    },
    pick_untyped,
    retype::{retype_one, retype_selection},
    sel4_guard,
    state::{self, BootstrapReentry, BootstrapRunState as BootState},
    BootPhase, UntypedSelection,
};
use crate::console::proto::{render_ack, AckLine, AckStatus};
use crate::console::Console;
use crate::cspace::tuples::assert_ipc_buffer_matches_bootinfo;
use crate::cspace::CSpace;
use crate::debug_uart::debug_uart_str;
#[cfg(debug_assertions)]
use crate::event::EventPump;
use crate::event::{
    AuditSink, BootstrapMessage, BootstrapMessageHandler, IpcDispatcher, TickEvent, TicketTable,
    TimerSource,
};
use crate::guards;
use crate::hal::{HalError, Hardware, KernelHal};
use crate::manifest;
#[cfg(all(feature = "net-console", feature = "kernel"))]
use crate::net::{DefaultNetStack as NetStack, NetPoller, CONSOLE_TCP_PORT, DEFAULT_NET_BACKEND};
#[cfg(all(feature = "net-console", not(feature = "kernel")))]
use crate::net::{NetStack, CONSOLE_TCP_PORT};
#[cfg(feature = "kernel")]
use crate::ninedoor::NineDoorBridge;
use crate::platform::{Platform, SeL4Platform};
use crate::profile;
use crate::readiness;
use crate::sel4;
#[cfg(feature = "cap-probes")]
use crate::sel4::first_regular_untyped;
use crate::sel4::{
    bootinfo_debug_dump, error_name, root_endpoint, BootInfo, BootInfoExt, BootInfoView,
    DevicePtPool, KernelEnv, ReservedVaddrRanges, RetypeKind, RetypeStatus, IPC_PAGE_BYTES,
    MSG_MAX_WORDS,
};
use crate::serial::{
    pl011::{Pl011, Pl011Mmio},
    SerialPort, DEFAULT_LINE_CAPACITY, DEFAULT_RX_CAPACITY, DEFAULT_TX_CAPACITY,
};
use crate::uart::pl011::{self as early_uart, PL011_PADDR};
use heapless::{String as HeaplessString, Vec as HeaplessVec};
use spin::Mutex;

const EARLY_DUMP_LIMIT: usize = 512;
const DEVICE_FRAME_BITS: usize = 12;

#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
use sel4_panicking::{self, DebugSink};

fn debug_identify_boot_caps() {
    let guard_stage = "BootCaps.identify";
    for slot in 0u64..16u64 {
        let cap = slot as sel4_sys::seL4_CPtr;
        if cap == sel4_sys::seL4_CapNull {
            sel4_guard::uart_breadcrumb(
                guard_stage,
                "seL4_CapIdentify.skip",
                "slot=0x0000 reason=null-cap",
            );
            continue;
        }
        let guarded_cap = sel4_guard::guard_cptr(guard_stage, "cap-identify.slot", cap);
        let mut breadcrumb = HeaplessString::<128>::new();
        let _ = write!(breadcrumb, "slot=0x{slot:04x}", slot = guarded_cap);
        sel4_guard::uart_breadcrumb(guard_stage, "seL4_CapIdentify", breadcrumb.as_str());
        let ty = unsafe { sel4_sys::seL4_CapIdentify(guarded_cap) };
        log::info!(
            "[identify] slot=0x{slot:04x} ty=0x{ty:08x}",
            slot = guarded_cap
        );
    }
}

#[inline(always)]
fn ranges_overlap(a: Range<usize>, b: Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

#[inline(always)]
fn align_down(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    value & !(align - 1)
}

#[inline(always)]
fn align_up(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    value.checked_add(align - 1).expect("alignment overflow") & !(align - 1)
}

#[inline(always)]
fn install_init_ipc_buffer(
    bootinfo_ref: &'static sel4_sys::seL4_BootInfo,
    reserved_vaddrs: &mut ReservedVaddrRanges,
    boot_guard: &mut BootStateGuard,
) -> Result<(NonNull<sel4_sys::seL4_IPCBuffer>, IpcBufferMode), FatalBootstrapError> {
    let ipc_buffer_ptr = match bootinfo_ref.ipc_buffer_ptr() {
        Some(ptr) => ptr,
        None => {
            crate::bootstrap::log::force_uart_line("[boot] ipcbuf missing from bootinfo");
            return Err(FatalBootstrapError::from_str(
                "ipc buffer pointer missing from bootinfo",
            ));
        }
    };

    let addr = ipc_buffer_ptr.as_ptr() as usize;
    if addr & (IPC_PAGE_BYTES - 1) != 0 {
        crate::bootstrap::log::force_uart_line("[boot] ipcbuf misaligned");
        let mut msg = HeaplessString::<96>::new();
        let _ = write!(msg, "ipc buffer not page aligned: 0x{addr:016x}");
        return Err(FatalBootstrapError::from_str(msg.as_str()));
    }

    let ipc_page_base = align_down(addr, IPC_PAGE_BYTES);
    reserved_vaddrs.reserve(
        &(ipc_page_base..ipc_page_base + IPC_PAGE_BYTES),
        "ipc-buffer",
    );

    unsafe {
        #[cfg(all(feature = "kernel", target_arch = "aarch64"))]
        {
            sel4_sys::tls_set_base(core::ptr::addr_of_mut!(TLS_IMAGE));
            debug_assert!(
                sel4_sys::tls_image_mut().is_some(),
                "TLS base must resolve to an image after installation",
            );
        }

        let mut breadcrumb = HeaplessString::<160>::new();
        let _ = write!(breadcrumb, "ipc_ptr=0x{addr:016x}", addr = addr);
        sel4_guard::uart_breadcrumb(
            "IPCInstall.install_init_ipc_buffer",
            "seL4_SetIPCBuffer",
            breadcrumb.as_str(),
        );
        sel4_sys::seL4_SetIPCBuffer(ipc_buffer_ptr.as_ptr());
    }

    let mut line = HeaplessString::<96>::new();
    let _ = write!(line, "[boot] ipcbuf installed early ptr=0x{addr:016x}");
    crate::bootstrap::log::force_uart_line(line.as_str());

    assert_ipc_buffer_matches_bootinfo(bootinfo_ref);
    boot_guard.record_ipc_buffer(Some(addr), Some(IpcBufferMode::InstalledEarly));
    boot_guard.record_invariant("ipc_buffer.installed");

    Ok((ipc_buffer_ptr, IpcBufferMode::InstalledEarly))
}

#[inline(always)]
fn ipcbuf_sanity_probe(bootinfo_ref: &sel4_sys::seL4_BootInfo) -> Result<(), FatalBootstrapError> {
    let depth = bootinfo_ref.init_cnode_depth() as sel4_sys::seL4_Word;
    let src = bootinfo_ref.init_tcb_cap();
    let empty_start = bootinfo_ref.empty_first_slot() as sel4_sys::seL4_CPtr;
    let empty_end = bootinfo_ref.empty_last_slot_excl() as sel4_sys::seL4_CPtr;
    let guard_stage = "IPCInstall.ipcbuf_sanity";

    if empty_start >= empty_end {
        crate::bootstrap::log::force_uart_line("[boot] ipcbuf sanity empty window invalid");
        return Err(FatalBootstrapError::from_str(
            "ipcbuf sanity failed: empty window invalid",
        ));
    }

    let mut dst = empty_start;
    let candidate = dst.saturating_add(1);
    if candidate < empty_end {
        dst = candidate;
    }

    if dst < empty_start || dst >= empty_end {
        crate::bootstrap::log::force_uart_line("[boot] ipcbuf sanity dst outside empty window");
        return Err(FatalBootstrapError::from_str(
            "ipcbuf sanity failed: dst outside empty window",
        ));
    }

    let init_cnode =
        sel4_guard::guard_cptr(guard_stage, "init_cnode", bootinfo_ref.init_cnode_cap());
    let guarded_dst = sel4_guard::guard_cptr(guard_stage, "ipcbuf.dst", dst);
    let guarded_src = sel4_guard::guard_cptr(guard_stage, "ipcbuf.src", src);
    let mut breadcrumb = HeaplessString::<200>::new();
    let _ = write!(
        breadcrumb,
        "root=0x{root:04x} dst=0x{dst:04x} src=0x{src:04x} depth={depth}",
        root = init_cnode,
        dst = guarded_dst,
        src = guarded_src,
        depth = depth
    );
    sel4_guard::uart_breadcrumb(guard_stage, "seL4_CNode_Copy", breadcrumb.as_str());
    let result = unsafe {
        sel4_sys::seL4_CNode_Copy(
            init_cnode,
            guarded_dst,
            depth,
            init_cnode,
            guarded_src,
            depth,
            sel4_sys::seL4_AllRights,
        )
    };

    let mut line = HeaplessString::<160>::new();
    let _ = write!(
        line,
        "[boot] ipcbuf sanity copy src=0x{src:04x} dst=0x{dst:04x} empty=[0x{empty_start:04x}..0x{empty_end:04x}) err={} ({})",
        result as i32,
        error_name(result)
    );
    crate::bootstrap::log::force_uart_line(line.as_str());

    if result != sel4_sys::seL4_NoError {
        if result == sel4_sys::seL4_DeleteFirst {
            crate::bootstrap::log::force_uart_line(
                "[boot] ipcbuf sanity dst not empty (bug: chose non-empty slot)",
            );
        }
        let mut line = HeaplessString::<112>::new();
        let _ = write!(
            line,
            "[boot] ipcbuf sanity failed err={} ({})",
            result as i32,
            error_name(result)
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
        return Err(FatalBootstrapError::from_str(
            "ipcbuf sanity failed: copy failed",
        ));
    }

    let mut delete_line = HeaplessString::<200>::new();
    let _ = write!(
        delete_line,
        "root=0x{root:04x} dst=0x{dst:04x} depth={depth}",
        root = init_cnode,
        dst = guarded_dst,
    );
    sel4_guard::uart_breadcrumb(guard_stage, "seL4_CNode_Delete", delete_line.as_str());
    let delete_result = unsafe { sel4_sys::seL4_CNode_Delete(init_cnode, guarded_dst, depth) };

    if delete_result != sel4_sys::seL4_NoError {
        let mut delete_line = HeaplessString::<144>::new();
        let _ = write!(
            delete_line,
            "[boot] ipcbuf sanity delete failed dst=0x{dst:04x} err={} ({})",
            delete_result as i32,
            error_name(delete_result)
        );
        crate::bootstrap::log::force_uart_line(delete_line.as_str());
        return Err(FatalBootstrapError::from_str("ipcbuf sanity delete failed"));
    }

    crate::bootstrap::log::force_uart_line("[boot] ipcbuf sanity ok (copy+delete)");
    Ok(())
}

/// Retypes a single notification object from the selected RAM-backed untyped and
/// installs it into the init CSpace window ([0x010f..0x2000)). The destination
/// slot is allocated from `CSpaceCtx`, ensuring it honours the init CSpace depth
/// (`initBits = 13`) and empty-range bounds reported by bootinfo.
fn bootstrap_notification(
    cs: &mut CSpaceCtx,
    selection: &mut UntypedSelection,
) -> Result<sel4_sys::seL4_CPtr, sel4_sys::seL4_Error> {
    let slot = retype_one(
        cs,
        selection.cap,
        sel4_sys::seL4_NotificationObject,
        sel4_sys::seL4_NotificationBits as u8,
    )?;

    selection.record_consumed(sel4_sys::seL4_NotificationBits as u8);

    log::info!(
        target: "root_task::bootstrap",
        "[boot] notification retyped ut=0x{ut:03x} slot=0x{slot:04x}",
        ut = selection.cap,
        slot = slot,
    );

    Ok(slot)
}

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

        let extra_bytes = view.extra_bytes();
        let approx_words = extra_bytes / core::mem::size_of::<sel4_sys::seL4_Word>();
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
            "{prefix}bootinfo.extraLen = {extra_bytes} bytes (~{approx_words} words)\r\n",
            prefix = Self::PREFIX,
            extra_bytes = extra_bytes,
            approx_words = approx_words,
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
            node_id = header.nodeID,
            nodes = header.numNodes,
            ipc = header.ipcBuffer as usize,
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

struct BootWatchdog {
    last_sequence: u64,
    stagnant_ticks: u32,
}

impl BootWatchdog {
    const STALL_LIMIT: u32 = 512;

    const fn new() -> Self {
        Self {
            last_sequence: 0,
            stagnant_ticks: 0,
        }
    }

    fn poll(&mut self) {
        let snapshot = boot_tracer().snapshot();
        if snapshot.sequence == self.last_sequence {
            self.stagnant_ticks = self.stagnant_ticks.saturating_add(1);
            if self.stagnant_ticks >= Self::STALL_LIMIT {
                let dest = snapshot.last_slot.unwrap_or(0);
                let total = if snapshot.progress_total == 0 {
                    1
                } else {
                    snapshot.progress_total
                };
                let mut line = heapless::String::<160>::new();
                let _ = write!(
                    line,
                    "[boot:wd] stalled? last={:?} dest=0x{dest:04x} done={}/{}",
                    snapshot.phase, snapshot.progress_done, total,
                );
                boot_log::force_uart_line(line.as_str());
                self.stagnant_ticks = 0;
            }
        } else {
            self.last_sequence = snapshot.sequence;
            self.stagnant_ticks = 0;
        }
    }
}

#[cfg(debug_assertions)]
fn log_text_span() {
    extern "C" {
        #[link_name = "_text"]
        static __text_start: u8;
        #[link_name = "_end"]
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
    let retype_ptr = cspace_sys::untyped_retype_into_init_root as usize;
    log::info!(
        "[dbg] anchors: handle_cmd={:#x} retype_call={:#x}",
        handle_ptr,
        retype_ptr
    );
}

#[cfg(not(target_arch = "aarch64"))]
compile_error!("root-task kernel build currently supports only aarch64 targets");

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

/// Capability summary exposed to the interactive console.
#[derive(Copy, Clone, Debug)]
pub struct ConsoleCaps {
    /// Init CNode capability pointer.
    pub init_cnode: crate::sel4::seL4_CPtr,
    /// Init VSpace capability pointer.
    pub init_vspace: crate::sel4::seL4_CPtr,
    /// Init TCB capability pointer.
    pub init_tcb: crate::sel4::seL4_CPtr,
    /// Slot containing the console endpoint minted during bootstrap.
    pub console_endpoint_slot: crate::sel4::seL4_CPtr,
    /// Optional slot where the init TCB capability was copied for diagnostics.
    pub tcb_copy_slot: Option<crate::sel4::seL4_CPtr>,
}

fn parse_hex(arg: &str) -> Option<usize> {
    let trimmed = arg.trim_start_matches("0x");
    usize::from_str_radix(trimmed, 16).ok()
}

const MAX_HEXDUMP_LEN: usize = 256;

/// Minimal blocking console loop used during early bring-up.
pub fn start_console(uart: Pl011, caps: ConsoleCaps) -> ! {
    let mut console = Console::new(uart);
    let _ = writeln!(console, "console ready");
    let mut buffer = [0u8; 256];

    loop {
        let _ = write!(console, "cohesix> ");
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

        let mut parts = line.split_whitespace();
        let command = parts.next().unwrap_or("");

        match command {
            "help" => {
                let _ = writeln!(
                    console,
                    "Commands: help, echo <s>, hexdump <addr> <len>, caps, reboot"
                );
            }
            "echo" => {
                let rest = line[command.len()..].trim_start();
                let _ = writeln!(console, "{}", rest);
            }
            "hexdump" => {
                let Some(addr_str) = parts.next() else {
                    let _ = writeln!(console, "usage: hexdump <addr> <len>");
                    continue;
                };
                let Some(len_str) = parts.next() else {
                    let _ = writeln!(console, "usage: hexdump <addr> <len>");
                    continue;
                };
                let Some(mut addr) = parse_hex(addr_str) else {
                    let _ = writeln!(console, "invalid address");
                    continue;
                };
                let Some(len_raw) = parse_hex(len_str) else {
                    let _ = writeln!(console, "invalid length");
                    continue;
                };
                let len = len_raw.min(MAX_HEXDUMP_LEN);
                if len == 0 {
                    let _ = writeln!(console, "length must be > 0");
                    continue;
                }
                if addr.checked_add(len).is_none() {
                    let _ = writeln!(console, "address overflow");
                    continue;
                }

                let mut remaining = len;
                while remaining > 0 {
                    let line_len = remaining.min(16);
                    let mut bytes = [0u8; 16];
                    for (index, slot) in bytes.iter_mut().take(line_len).enumerate() {
                        unsafe {
                            *slot = ptr::read_volatile((addr + index) as *const u8);
                        }
                    }
                    let _ = write!(console, "0x{addr:016x}: ");
                    for (index, byte) in bytes.iter().enumerate() {
                        if index < line_len {
                            let _ = write!(console, "{:02x} ", byte);
                        } else {
                            let _ = write!(console, "   ");
                        }
                    }
                    let _ = write!(console, " |");
                    for byte in bytes.iter().take(line_len) {
                        let ch = match byte {
                            0x20..=0x7e => char::from(*byte),
                            _ => '.',
                        };
                        let _ = write!(console, "{ch}");
                    }
                    let _ = writeln!(console, "|");
                    addr = addr.saturating_add(line_len);
                    remaining -= line_len;
                }
            }
            "caps" => {
                let mut line = HeaplessString::<64>::new();
                let _ = write!(
                    line,
                    "initCNode=0x{cnode:04x} vspace=0x{vspace:04x} tcb=0x{tcb:04x} ep_console=0x{ep:04x}",
                    cnode = caps.init_cnode,
                    vspace = caps.init_vspace,
                    tcb = caps.init_tcb,
                    ep = caps.console_endpoint_slot,
                );
                if let Some(copy_slot) = caps.tcb_copy_slot {
                    let _ = write!(line, " tcb_copy=0x{copy:04x}", copy = copy_slot);
                }
                let _ = writeln!(console, "{}", line.as_str());
            }
            "reboot" => {
                let _ = writeln!(console, "(stub) reboot not implemented");
            }
            _ => {
                let _ = writeln!(console, "unknown command: {}", line);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EarlyBootPhase {
    Begin,
    BootInfoView,
    CSpaceRecord,
    MemoryLayout,
    BootInfoSnapshot,
    IPCInstall,
}

impl EarlyBootPhase {
    fn label(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::BootInfoView => "bootinfo.view",
            Self::CSpaceRecord => "cspace.record",
            Self::MemoryLayout => "layout.build",
            Self::BootInfoSnapshot => "bootinfo.snapshot",
            Self::IPCInstall => "ipc.install",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IpcBufferMode {
    InstalledEarly,
    BootProvidedFallback,
}

impl IpcBufferMode {
    fn label(self) -> &'static str {
        match self {
            Self::InstalledEarly => "installed-early",
            Self::BootProvidedFallback => "boot-provided-fallback",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PrecommitReason {
    BootInfoView,
    BootInfoSnapshot,
    CSpaceRecord,
    Sequencer,
    MemoryLayout,
    IpcInstall,
    IpcSanity,
}

impl PrecommitReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::BootInfoView => "bootinfo-view",
            Self::BootInfoSnapshot => "bootinfo-snapshot",
            Self::CSpaceRecord => "cspace-record",
            Self::Sequencer => "sequencer",
            Self::MemoryLayout => "memory-layout",
            Self::IpcInstall => "ipc-install",
            Self::IpcSanity => "ipc-sanity",
        }
    }
}

/// Errors surfaced during timer bring-up.
#[derive(Debug, PartialEq, Eq)]
pub enum TimerError {
    /// The timer frequency could not be determined.
    FrequencyUnavailable,
    /// The underlying counter could not be sampled.
    CounterUnavailable,
    /// A zero or invalid period was provided.
    InvalidPeriod,
}

impl fmt::Display for TimerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrequencyUnavailable => f.write_str("timer frequency unavailable"),
            Self::CounterUnavailable => f.write_str("timer counter unavailable"),
            Self::InvalidPeriod => f.write_str("timer period invalid"),
        }
    }
}

/// Errors that can occur while initialising the root task runtime.
#[derive(Debug, PartialEq, Eq)]
pub enum BootError {
    /// Indicates the bootstrap path has already been executed for this boot.
    AlreadyBooted,
    /// Bootstrap invariants failed.
    Fatal(String),
    /// Timer initialisation failed.
    TimerInit(TimerError),
}

impl fmt::Display for BootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyBooted => f.write_str("bootstrap already invoked"),
            Self::Fatal(msg) => f.write_str(msg),
            Self::TimerInit(err) => write!(f, "timer init failed: {err}"),
        }
    }
}

impl From<TimerError> for BootError {
    fn from(value: TimerError) -> Self {
        Self::TimerInit(value)
    }
}

impl From<FatalBootstrapError> for BootError {
    fn from(value: FatalBootstrapError) -> Self {
        Self::Fatal(value.message().to_owned())
    }
}

#[derive(Default)]
struct AbortTelemetry {
    phase: &'static str,
    substep: &'static str,
    reason: &'static str,
    error_code: Option<i32>,
    last_mark: Option<&'static str>,
    last_invariant: Option<&'static str>,
    cspace_root: Option<sel4_sys::seL4_CPtr>,
    cspace_bits: Option<u8>,
    first_free: Option<sel4_sys::seL4_CPtr>,
    empty_start: Option<sel4_sys::seL4_CPtr>,
    empty_end: Option<sel4_sys::seL4_CPtr>,
    ep_ready: bool,
    root_ep: Option<sel4_sys::seL4_CPtr>,
    fault_ep: Option<sel4_sys::seL4_CPtr>,
    ipc_buffer: Option<usize>,
    ipcbuf_mode: Option<IpcBufferMode>,
    logger_switched: bool,
}

impl AbortTelemetry {
    fn emit(&self) {
        let phase = if self.phase.is_empty() {
            "unknown"
        } else {
            self.phase
        };
        let sub = if self.substep.is_empty() {
            "unspecified"
        } else {
            self.substep
        };
        let reason = if self.reason.is_empty() {
            "unspecified"
        } else {
            self.reason
        };
        let mut head = heapless::String::<160>::new();
        let _ = write!(head, "[boot:abort] phase={phase} sub={sub} reason={reason}");
        if let Some(code) = self.error_code {
            let _ = write!(head, "/{code}");
        }
        boot_log::force_uart_line(head.as_str());

        let mut cspace = heapless::String::<192>::new();
        let root = self.cspace_root.unwrap_or_default();
        let bits = self.cspace_bits.unwrap_or_default();
        let first_free = self.first_free.unwrap_or_default();
        let start = self.empty_start.unwrap_or(first_free);
        let end = self.empty_end.unwrap_or(first_free);
        let _ = write!(
            cspace,
            "[boot:abort] cspace root=0x{root:04x} bits={bits} first_free=0x{first_free:04x} empty=[0x{start:04x}..0x{end:04x})"
        );
        boot_log::force_uart_line(cspace.as_str());

        let mut endpoints = heapless::String::<192>::new();
        let ipcbuf = self.ipc_buffer.map(|ptr| {
            let mut tmp = heapless::String::<32>::new();
            let _ = write!(&mut tmp, "0x{ptr:016x}");
            tmp
        });
        let ipcbuf_label = ipcbuf.as_ref().map(|s| s.as_str()).unwrap_or("none");

        let root_ep = self.root_ep.unwrap_or_default();
        let fault_ep = self.fault_ep.unwrap_or_default();
        let _ = write!(
            endpoints,
            "[boot:abort] ep_ready={} root_ep=0x{root_ep:04x} ipcbuf={} fault_ep=0x{fault_ep:04x} logger_ep={}",
            self.ep_ready as u8,
            ipcbuf_label,
            self.logger_switched as u8,
        );
        boot_log::force_uart_line(endpoints.as_str());

        if let Some(mode) = self.ipcbuf_mode {
            let mut ipc_mode = heapless::String::<96>::new();
            let _ = write!(ipc_mode, "[boot:abort] ipcbuf_mode={}", mode.label());
            boot_log::force_uart_line(ipc_mode.as_str());
        }

        let mut mark_line = heapless::String::<160>::new();
        let _ = write!(
            mark_line,
            "[boot:abort] last_mark={} last_invariant={}",
            self.last_mark.unwrap_or("none"),
            self.last_invariant.unwrap_or("none"),
        );
        boot_log::force_uart_line(mark_line.as_str());
    }
}

fn log_precommit_exit<E: fmt::Display>(
    phase: EarlyBootPhase,
    sub: &'static str,
    fatal: bool,
    reason: PrecommitReason,
    sel4_err: Option<sel4_sys::seL4_Error>,
    err: E,
    file: &'static str,
    line_no: u32,
) {
    let mut line = HeaplessString::<192>::new();
    let sel4_err_label = sel4_err.map(crate::sel4::error_name).unwrap_or("none");
    let sel4_err_code = sel4_err.unwrap_or_default() as i32;
    let _ = write!(
        line,
        "[boot] early-exit phase={} sub={} fatal={} reason={} sel4_err={}/{} @{}:{} detail={}",
        phase.label(),
        sub,
        fatal as u8,
        reason.as_str(),
        sel4_err_label,
        sel4_err_code,
        file,
        line_no,
        err
    );
    boot_log::force_uart_line(line.as_str());
}

struct PostCommitState {
    failed: bool,
}

impl PostCommitState {
    fn new() -> Self {
        Self { failed: false }
    }

    fn flag_failure<E: fmt::Display>(&mut self, phase: &'static str, err: E) {
        let mut line = HeaplessString::<192>::new();
        let _ = write!(
            line,
            "[boot] post-commit bootstrap failed at {phase}: {err}; continuing in degraded mode",
            err = err
        );
        boot_log::force_uart_line(line.as_str());
        log::error!("{}", line.as_str());
        self.failed = true;
    }

    fn failed(&self) -> bool {
        self.failed
    }
}

struct BootstrapCommit {
    minimal_committed: bool,
    full_committed: bool,
    cspace_recorded: bool,
    telemetry: AbortTelemetry,
}

struct BootStateGuard {
    commit: BootstrapCommit,
    graceful_exit: bool,
}

/// Boot-time feature flags enabling optional subsystems.
#[derive(Debug, Clone, Copy)]
pub struct BootFeatures {
    /// Whether the PL011-backed serial console is enabled.
    pub serial_console: bool,
    /// Whether the networking stack is enabled.
    pub net: bool,
    /// Whether the TCP console / Secure9P surface is enabled.
    pub net_console: bool,
}

/// Control-plane endpoint used for LOG+CONTROL / queen bootstrap traffic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlEndpoint(sel4_sys::seL4_CPtr);

impl ControlEndpoint {
    #[must_use]
    pub fn raw(self) -> sel4_sys::seL4_CPtr {
        self.0
    }
}

/// Dedicated fault endpoint. Only valid as a target for `seL4_TCB_SetFaultHandler` and
/// `seL4_Recv` in the fault handler loop; it must never be used for normal IPC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultEndpoint(sel4_sys::seL4_CPtr);

impl FaultEndpoint {
    #[must_use]
    pub fn raw(self) -> sel4_sys::seL4_CPtr {
        self.0
    }

    #[must_use]
    pub fn is_valid(self) -> bool {
        self.0 != sel4_sys::seL4_CapNull
    }
}

/// Aggregated endpoints provisioned by the kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelEndpoints {
    pub control: ControlEndpoint,
    pub fault: FaultEndpoint,
}

impl KernelEndpoints {
    pub fn new(control: sel4_sys::seL4_CPtr, fault: sel4_sys::seL4_CPtr) -> Self {
        Self {
            control: ControlEndpoint(control),
            fault: FaultEndpoint(fault),
        }
    }
}

/// Aggregated bootstrap artefacts passed to userland for final bring-up.
pub struct BootContext {
    /// Bootinfo view captured during kernel bootstrap.
    pub bootinfo: BootInfoView,
    /// Bootinfo snapshot captured at bootstrap start to detect corruption.
    pub bootinfo_snapshot: BootInfoSnapshot,
    /// Feature flags summarising the current profile.
    pub features: BootFeatures,
    /// Root and fault endpoint bundle. The control endpoint handles LOG+CONTROL / queen
    /// bootstrap traffic; the fault endpoint is reserved exclusively for seL4 fault
    /// delivery.
    pub endpoints: KernelEndpoints,
    /// PL011 UART slot reserved for the serial console.
    pub uart_slot: Option<sel4_sys::seL4_CPtr>,
    /// Mapping metadata for the PL011 UART.
    pub uart_mmio: Option<Pl011Mmio>,
    pub(crate) serial: RefCell<
        Option<SerialPort<Pl011, DEFAULT_RX_CAPACITY, DEFAULT_TX_CAPACITY, DEFAULT_LINE_CAPACITY>>,
    >,
    pub(crate) timer: RefCell<Option<KernelTimer>>,
    pub(crate) ipc: RefCell<Option<KernelIpc>>,
    pub(crate) tickets: RefCell<Option<TicketTable<4>>>,
    #[cfg(feature = "net-console")]
    pub(crate) net_stack: RefCell<Option<NetStack>>,
    #[cfg(feature = "kernel")]
    pub(crate) ninedoor: RefCell<Option<&'static mut NineDoorBridge>>,
}

impl BootStateGuard {
    fn acquire() -> Result<Self, BootError> {
        match state::enter_once("BootStateGuard::acquire") {
            Ok(()) => Ok(Self {
                commit: BootstrapCommit {
                    minimal_committed: false,
                    full_committed: false,
                    cspace_recorded: false,
                    telemetry: AbortTelemetry::default(),
                },
                graceful_exit: false,
            }),
            Err(BootstrapReentry::AlreadyAttempted(run_state)) => {
                log::error!(
                    "[boot] bootstrap called twice; refusing re-entry (state={})",
                    run_state.label()
                );
                Err(BootError::AlreadyBooted)
            }
        }
    }

    fn record_phase(&mut self, phase: &'static str) {
        self.commit.telemetry.phase = phase;
    }

    fn record_substep(&mut self, substep: &'static str) {
        self.commit.telemetry.substep = substep;
    }

    fn record_reason(&mut self, reason: &'static str, error_code: Option<i32>) {
        self.commit.telemetry.reason = reason;
        self.commit.telemetry.error_code = error_code;
    }

    fn record_mark(&mut self, mark: &'static str) {
        self.commit.telemetry.last_mark = Some(mark);
    }

    fn record_invariant(&mut self, invariant: &'static str) {
        self.commit.telemetry.last_invariant = Some(invariant);
    }

    fn current_phase(&self) -> &'static str {
        self.commit.telemetry.phase
    }

    fn last_mark(&self) -> &'static str {
        self.commit.telemetry.last_mark.unwrap_or("none")
    }

    fn record_cspace(
        &mut self,
        root: sel4_sys::seL4_CPtr,
        bits: u8,
        first_free: sel4_sys::seL4_CPtr,
        empty: (sel4_sys::seL4_CPtr, sel4_sys::seL4_CPtr),
    ) {
        self.commit.telemetry.cspace_root = Some(root);
        self.commit.telemetry.cspace_bits = Some(bits);
        self.commit.telemetry.first_free = Some(first_free);
        self.commit.telemetry.empty_start = Some(empty.0);
        self.commit.telemetry.empty_end = Some(empty.1);
        self.commit.cspace_recorded = true;
    }

    fn record_endpoints(&mut self, root_ep: sel4_sys::seL4_CPtr, fault_ep: sel4_sys::seL4_CPtr) {
        self.commit.telemetry.root_ep = Some(root_ep);
        self.commit.telemetry.fault_ep = Some(fault_ep);
        self.commit.telemetry.ep_ready = root_ep != sel4_sys::seL4_CapNull;
    }

    fn record_ipc_buffer(&mut self, ipc_buffer: Option<usize>, mode: Option<IpcBufferMode>) {
        self.commit.telemetry.ipc_buffer = ipc_buffer;
        self.commit.telemetry.ipcbuf_mode = mode;
    }

    fn record_logger_switch(&mut self, ready: bool) {
        self.commit.telemetry.logger_switched = ready;
    }

    fn allow_graceful_exit(&mut self) {
        self.graceful_exit = true;
    }

    fn commit_minimal(&mut self) {
        self.commit.minimal_committed = true;
    }

    fn commit_full(&mut self) {
        if !self.commit.minimal_committed {
            self.commit_minimal();
        }
        self.commit.full_committed = true;
        state::mark_committed();
        crate::readiness::mark_bootstrap_committed();
    }
}

impl Drop for BootStateGuard {
    fn drop(&mut self) {
        if !self.commit.minimal_committed {
            if self.graceful_exit {
                log::warn!("[boot] bootstrap exited without committing (graceful exit allowed)");
                boot_log::force_uart_line(
                    "[boot] bootstrap exited without committing (graceful exit allowed)",
                );
                self.commit.telemetry.emit();
                state::mark_aborted();
                return;
            }
            if self.commit.cspace_recorded {
                let root = self.commit.telemetry.cspace_root.unwrap_or_default();
                let bits = self.commit.telemetry.cspace_bits.unwrap_or_default();
                let empty_start = self.commit.telemetry.empty_start.unwrap_or_default();
                let empty_end = self.commit.telemetry.empty_end.unwrap_or_default();
                if root == 0 && bits == 0 && empty_start == 0 && empty_end == 0 {
                    panic!("[boot] abort telemetry missing cspace after record");
                }
            }
            log::error!("[boot] bootstrap exited without committing; refusing to reset boot state");
            self.commit.telemetry.emit();
            state::mark_aborted();
            panic!("[boot] bootstrap aborted before commit");
        } else if !self.commit.full_committed {
            state::mark_aborted();
        }
    }
}

/// Root task entry point invoked by seL4 after kernel initialisation.
///
/// This is the only supported entry for the kernel build; prior refactors
/// accidentally bypassed userland by logging before the bootstrap logger was
/// installed and by leaving alternative stubs around. Keeping the hand-off
/// here ensures we always enter the event-pump userland path or loudly fall
/// back to the PL011 console when bootstrap fails.
pub fn start<P: Platform>(bootinfo: &'static BootInfo, platform: &P) -> ! {
    let boot_state = state::state();
    if boot_state != BootState::Cold {
        log::error!(
            "[kernel:entry] bootstrap re-entry detected (state={boot_state:?}); parking thread"
        );
        boot_log::force_uart_line("[kernel:entry] re-entry detected; parking thread");
        loop {
            sel4::yield_now();
        }
    }

    boot_log::force_uart_line("[kernel:entry] root-task entry reached");
    boot_log::force_uart_line("[MARK] boot_state=COLD");
    log::info!("[kernel:entry] root-task entry reached");
    log::info!(target: "kernel", "[kernel] boot entrypoint: starting bootstrap");
    let ctx = match bootstrap(platform, bootinfo) {
        Ok(ctx) => ctx,
        Err(err) => match err {
            BootError::Fatal(msg) => panic!("[bootstrap:fatal] {msg}"),
            _ => {
                log::error!("[kernel:entry] bootstrap failed: {err}");
                boot_log::force_uart_line("[kernel:entry] bootstrap failed; parking thread");
                log::error!(
                        "[kernel:entry] unable to construct BootContext; refusing to bypass userland handoff"
                    );
                loop {
                    sel4::yield_now();
                }
            }
        },
    };

    // Full boots must always proceed into userland; bootstrap-minimal remains a
    // diagnostic-only path that bypasses normal runtime handoff.
    log::info!("[kernel] handoff: calling userland::main");
    log::info!(
        "[kernel] bootstrap complete, handing off to userland runtime (serial_console={}, net={}, net_console={})",
        ctx.features.serial_console,
        ctx.features.net,
        ctx.features.net_console,
    );
    boot_log::force_uart_line("[kernel:entry] bootstrap complete; entering userland");

    crate::userland::main(ctx);
}

fn bootstrap<P: Platform>(
    platform: &P,
    bootinfo: &'static BootInfo,
) -> Result<BootContext, BootError> {
    #[cfg(all(feature = "kernel", not(sel4_config_printing)))]
    crate::sel4::install_debug_sink();

    let mut sequencer = BootstrapSequencer::new();
    let mut boot_guard = BootStateGuard::acquire()?;
    let mut early_phase = EarlyBootPhase::BootInfoView;

    let mut bootinfo_view = canonical_bootinfo_view(&mut sequencer, bootinfo).map_err(|err| {
        log_precommit_exit(
            early_phase,
            "canonical_bootinfo_view",
            true,
            PrecommitReason::BootInfoView,
            None,
            &err,
            file!(),
            line!(),
        );
        err
    })?;
    boot_guard.record_phase("BootInfoValidate");
    sel4_guard::install_bootinfo(&bootinfo_view);

    early_phase = EarlyBootPhase::MemoryLayout;
    sequencer
        .advance(BootstrapPhase::MemoryLayoutBuild)
        .map_err(|err| {
            log_precommit_exit(
                early_phase,
                "bootstrap_phase.MemoryLayoutBuild",
                true,
                PrecommitReason::MemoryLayout,
                None,
                &err,
                file!(),
                line!(),
            );
            err
        })?;
    boot_guard.record_phase("MemoryLayoutBuild");
    let layout_snapshot = layout::dump_and_sanity_check();

    let bootinfo_source_vaddr = bootinfo as *const _ as usize;
    let bootinfo_copy_vaddr = bootinfo_view.header() as *const _ as usize;

    let mut bootinfo_line = heapless::String::<160>::new();
    let _ = write!(
        bootinfo_line,
        "[bootinfo] kernel=0x{source:016x} snapshot=0x{copy:016x}",
        source = bootinfo_source_vaddr,
        copy = bootinfo_copy_vaddr,
    );
    boot_log::force_uart_line(bootinfo_line.as_str());
    log::info!("{}", bootinfo_line.as_str());

    let heap_range = layout_snapshot.heap_range();
    let bss_range = layout_snapshot.bss_range();
    let stack_range = layout_snapshot.stack_range();
    let device_range = crate::sel4::device_window_range();

    let bootinfo_page_base = align_down(bootinfo_source_vaddr, IPC_PAGE_BYTES);
    let header_len = bootinfo_view.header_bytes().len();
    let extra_len = bootinfo_view.extra_bytes();
    let bootinfo_total_bytes = header_len
        .checked_add(extra_len)
        .expect("bootinfo size overflow");
    let bootinfo_total_end = bootinfo_source_vaddr
        .checked_add(bootinfo_total_bytes)
        .expect("bootinfo end overflow");
    let bootinfo_range_end = align_up(bootinfo_total_end, IPC_PAGE_BYTES);
    let bootinfo_range = bootinfo_page_base..bootinfo_range_end;
    let bootinfo_pages = (bootinfo_range_end - bootinfo_page_base) / IPC_PAGE_BYTES;
    let mut reserved_vaddrs = ReservedVaddrRanges::new();
    reserved_vaddrs.reserve(&heap_range, "heap");
    reserved_vaddrs.reserve(&stack_range, "stack");
    reserved_vaddrs.reserve(&bootinfo_range, "bootinfo-frame");

    let mut reserved_line = heapless::String::<192>::new();
    let _ = write!(
        reserved_line,
        "[vaddr] reserved heap=[0x{heap_start:08x}..0x{heap_end:08x}) stack=[0x{stack_start:08x}..0x{stack_end:08x}) bootinfo=[0x{bi_start:08x}..0x{bi_end:08x})",
        heap_start = heap_range.start,
        heap_end = heap_range.end,
        stack_start = stack_range.start,
        stack_end = stack_range.end,
        bi_start = bootinfo_range.start,
        bi_end = bootinfo_range_end,
    );
    log::info!("{}", reserved_line.as_str());
    boot_log::force_uart_line(reserved_line.as_str());

    if ranges_overlap(heap_range.clone(), bss_range.clone()) {
        boot_log::force_uart_line("[alloc:init] heap overlaps .bss");
        panic!("heap overlaps .bss (heap={heap_range:?} bss={bss_range:?})");
    }
    if ranges_overlap(heap_range.clone(), stack_range.clone()) {
        boot_log::force_uart_line("[alloc:init] heap overlaps stack");
        panic!("heap overlaps stack (heap={heap_range:?} stack={stack_range:?})");
    }
    if ranges_overlap(heap_range.clone(), bootinfo_range.clone()) {
        boot_log::force_uart_line("[alloc:init] heap overlaps bootinfo frame");
        panic!("heap overlaps bootinfo frame (heap={heap_range:?} bootinfo={bootinfo_range:?})");
    }
    if ranges_overlap(device_range.clone(), heap_range.clone()) {
        boot_log::force_uart_line("[alloc:init] heap overlaps device window");
        panic!(
            "device page-table window overlaps heap (device={device_range:?} heap={heap_range:?})"
        );
    }
    if ranges_overlap(device_range.clone(), stack_range.clone()) {
        boot_log::force_uart_line("[alloc:init] stack overlaps device window");
        panic!(
            "device page-table window overlaps stack guard (device={device_range:?} stack={stack_range:?})"
        );
    }

    crate::alloc::init_heap(heap_range.clone());
    boot_guard.record_invariant("allocator.ready");

    boot_log::init_logger_bootstrap_only();

    crate::sel4::log_sel4_type_sanity();

    let mut bootinfo_snapshot_opt: Option<BootInfoSnapshot> = None;

    early_phase = EarlyBootPhase::BootInfoSnapshot;
    debug_uart_str("[breadcrumb] before bootinfo snapshot capture\r\n");
    let bootinfo_state = match snapshot_bootinfo(&bootinfo_view) {
        Ok(state) => Some(state),
        Err(err) => {
            log_precommit_exit(
                early_phase,
                "snapshot_bootinfo",
                false,
                PrecommitReason::BootInfoSnapshot,
                None,
                &err,
                file!(),
                line!(),
            );
            let mut line = HeaplessString::<192>::new();
            let _ = write!(
                line,
                "[bootinfo] snapshot failed: {}; continuing with validated view",
                err
            );
            boot_log::force_uart_line(line.as_str());
            log::warn!("{}", line.as_str());
            boot_guard.record_invariant("bootinfo.snapshot.degraded");
            match BootInfoSnapshot::from_view(&bootinfo_view) {
                Ok(snapshot) => {
                    bootinfo_snapshot_opt = Some(snapshot);
                    boot_guard.record_invariant("bootinfo.snapshot.fallback");
                }
                Err(snapshot_err) => {
                    let msg = format!(
                        "bootinfo snapshot fallback failed: {snapshot_err}",
                        snapshot_err = snapshot_err
                    );
                    boot_log::force_uart_line(&msg);
                    return Err(BootError::Fatal(msg));
                }
            }
            None
        }
    };

    let probe_canary = |mark: &'static str| {
        if let Some(state) = bootinfo_state {
            let _ = state.probe(mark);
        }
    };

    if let Some(state) = bootinfo_state {
        let bootinfo_snapshot = state.snapshot();
        match state.verify("snapshot", "[mark] bootinfo.snapshot") {
            Ok(()) => {}
            Err(BootInfoCanaryError::Snapshot { mark, error }) => {
                let msg = format!(
                    "bootinfo snapshot validation failed at {mark}: {error}",
                    mark = mark,
                    error = error
                );
                boot_log::force_uart_line(&msg);
                log::error!("{}", msg);
                return Err(BootError::Fatal(msg));
            }
            Err(BootInfoCanaryError::Diverged {
                mark,
                expected,
                observed,
            }) => {
                panic!(
                    "bootinfo snapshot diverged at {mark}: expected={expected:?} observed={observed:?}"
                );
            }
        }
        bootinfo_view = state.view();
        sel4_guard::install_bootinfo(&bootinfo_view);
        boot_guard.record_invariant("bootinfo.snapshot.ok");
        bootinfo_snapshot_opt = Some(bootinfo_snapshot);
        let mut snapshot_state_line = HeaplessString::<256>::new();
        let backing_ptr = bootinfo_snapshot.backing().as_ptr() as usize;
        let backing_len = bootinfo_snapshot.backing().len();
        let snapshot_pages = align_up(backing_len, IPC_PAGE_BYTES) / IPC_PAGE_BYTES;
        let region = state.snapshot_region();
        let _ = write!(
            snapshot_state_line,
            "[state] snapshot_region=[0x{start:016x}..0x{end:016x}) bootinfo=0x{bootinfo:016x} total=0x{total:08x} pages={boot_pages} backing=0x{back:016x} len=0x{len:08x} snap_pages={snap_pages}\r\n",
            start = region.start,
            end = region.end,
            bootinfo = bootinfo_source_vaddr,
            total = bootinfo_total_bytes,
            boot_pages = bootinfo_pages,
            back = backing_ptr,
            len = backing_len,
            snap_pages = snapshot_pages,
        );
        debug_uart_str(snapshot_state_line.as_str());
        let mut snapshot_line = heapless::String::<160>::new();
        let _ = write!(
            snapshot_line,
            "[bootinfo] snapshot region=[0x{start:016x}..0x{end:016x})",
            start = region.start,
            end = region.end
        );
        boot_log::force_uart_line(snapshot_line.as_str());
        if ranges_overlap(region.clone(), heap_range.clone()) {
            panic!("bootinfo snapshot overlaps heap snapshot={region:?} heap={heap_range:?}");
        }
        if ranges_overlap(region.clone(), stack_range.clone()) {
            panic!("bootinfo snapshot overlaps stack snapshot={region:?} stack={stack_range:?}");
        }
        if ranges_overlap(region.clone(), device_range.clone()) {
            panic!(
                "bootinfo snapshot overlaps device pt pool snapshot={region:?} device={device_range:?}"
            );
        }
        if ranges_overlap(region.clone(), bootinfo_range.clone()) {
            panic!(
                "bootinfo snapshot overlaps bootinfo frame snapshot={region:?} bootinfo={bootinfo_range:?}"
            );
        }

        probe_canary("[probe] after.snapshot.validation");
    }

    let bootinfo_snapshot = match bootinfo_snapshot_opt {
        Some(snapshot) => snapshot,
        None => {
            let msg = String::from("bootinfo snapshot unavailable after fallback attempts");
            boot_log::force_uart_line(msg.as_str());
            return Err(BootError::Fatal(msg));
        }
    };

    let mut build_line = heapless::String::<192>::new();
    let mut feature_report = heapless::String::<96>::new();
    for (idx, (label, enabled)) in [
        ("kernel", profile::KERNEL),
        ("bootstrap-trace", cfg!(feature = "bootstrap-trace")),
        ("serial-console", profile::SERIAL_CONSOLE),
        ("net", profile::NET),
        ("net-console", profile::NET_CONSOLE),
    ]
    .into_iter()
    .enumerate()
    {
        if idx > 0 {
            let _ = write!(feature_report, " ");
        }
        let _ = write!(feature_report, "{label}:{value}", value = enabled as u8);
    }
    let _ = write!(
        build_line,
        "[BUILD] {} {} features=[{}]",
        crate::built_info::GIT_HASH,
        crate::built_info::BUILD_TS,
        feature_report
    );
    debug_uart_str("[breadcrumb] before BUILD log\r\n");
    boot_log::force_uart_line(build_line.as_str());
    log::info!("{}", build_line.as_str());
    debug_uart_str("[breadcrumb] after BUILD log\r\n");

    boot_guard.record_phase("start");
    debug_assert_eq!(state::state(), BootState::Running, "bootstrap state drift",);
    boot_guard.record_mark("[mark] bootstrap.enter");
    boot_log::force_uart_line("[mark] bootstrap.enter");
    crate::bp!("bootstrap.begin");
    debug_uart_str("[breadcrumb] after bootstrap.begin\r\n");
    let mut pending_boot_phases = heapless::Vec::<BootPhase, 4>::new();
    let _ = pending_boot_phases.push(BootPhase::Begin);

    let check_bootinfo = |guard: &mut BootStateGuard, mark: &'static str| {
        guard.record_mark(mark);
        let phase_label = guard.current_phase();
        if let Some(state) = bootinfo_state {
            if let Err(err) = state.verify(phase_label, guard.last_mark()) {
                panic!("bootinfo canary tripped at {mark}: {err:?}");
            }
        }
    };
    let bootinfo_ref: &'static sel4_sys::seL4_BootInfo = bootinfo_view.header();
    early_phase = EarlyBootPhase::CSpaceRecord;
    debug_uart_str("[breadcrumb] before CSpaceRecord\r\n");
    sequencer
        .advance(BootstrapPhase::CSpaceRecord)
        .map_err(|err| {
            log_precommit_exit(
                early_phase,
                "bootstrap_phase.CSpaceRecord",
                true,
                PrecommitReason::CSpaceRecord,
                None,
                &err,
                file!(),
                line!(),
            );
            err
        })?;
    check_bootinfo(&mut boot_guard, "[mark] phase.CSpaceRecord");
    if let Err(err) = crate::bootstrap::cspace::ensure_canonical_root_alias(bootinfo_ref) {
        log_precommit_exit(
            early_phase,
            "cspace.ensure_canonical_root_alias",
            true,
            PrecommitReason::CSpaceRecord,
            Some(err),
            crate::sel4::error_name(err),
            file!(),
            line!(),
        );
        return Err(BootError::Fatal(format!(
            "failed to mint canonical init CNode alias: {} ({})",
            err,
            error_name(err),
        )));
    }
    boot_guard.record_invariant("cspace.alias.canonical");
    let (empty_start, empty_end) = bootinfo_view.init_cnode_empty_range();
    let cspace_window = CSpaceWindow::new(
        bootinfo_view.root_cnode_cap(),
        bootinfo_view.canonical_root_cap(),
        cspace_sys::bits_as_u8(usize::from(bootinfo_view.init_cnode_bits())),
        empty_start,
        empty_end,
        empty_start,
    );
    boot_guard.record_cspace(
        cspace_window.root,
        cspace_window.bits,
        cspace_window.first_free,
        (empty_start, empty_end),
    );
    readiness::mark_cspace_window_ready();
    boot_guard.record_phase("CSpaceRecord");
    probe_canary("[probe] after.CSpaceRecord");
    let (ipc_buffer_ptr, mut ipcbuf_mode) =
        install_init_ipc_buffer(bootinfo_ref, &mut reserved_vaddrs, &mut boot_guard).map_err(
            |err| {
                log_precommit_exit(
                    EarlyBootPhase::IPCInstall,
                    "install_init_ipc_buffer",
                    true,
                    PrecommitReason::IpcInstall,
                    None,
                    &err,
                    file!(),
                    line!(),
                );
                err
            },
        )?;
    ipcbuf_sanity_probe(bootinfo_ref).map_err(|err| {
        log_precommit_exit(
            EarlyBootPhase::IPCInstall,
            "ipcbuf_sanity_probe",
            true,
            PrecommitReason::IpcSanity,
            None,
            &err,
            file!(),
            line!(),
        );
        err
    })?;
    readiness::mark_ipc_buffer_installed();
    early_phase = EarlyBootPhase::IPCInstall;
    match sequencer.advance(BootstrapPhase::IPCInstall) {
        Ok(()) => {
            check_bootinfo(&mut boot_guard, "[mark] phase.IPCInstall");
        }
        Err(err) => {
            let fatal = cfg!(feature = "bootstrap-early-exit");
            log_precommit_exit(
                early_phase,
                "bootstrap_phase.IPCInstall",
                fatal,
                PrecommitReason::IpcInstall,
                None,
                &err,
                file!(),
                line!(),
            );
            boot_guard.record_reason("phase.IPCInstall", None);
            if fatal {
                boot_guard.allow_graceful_exit();
                return Err(err.into());
            }
        }
    }
    boot_guard.record_phase("IPCInstall");
    boot_guard.record_mark("[mark] ipc.install.ok");
    boot_log::force_uart_line("[mark] ipc.install.ok");
    probe_canary("[probe] after.IPCInstall");
    debug_uart_str("[breadcrumb] after ipcbuf sanity ok\r\n");
    let mut state_line = HeaplessString::<256>::new();
    let _ = write!(
        state_line,
        "[state] bootinfo=0x{bootinfo:016x} bootinfo_page=[0x{bi_start:016x}..0x{bi_end:016x}) ipcbuf=0x{ipc:016x} heap=[0x{heap_start:08x}..0x{heap_end:08x}) stack=[0x{stack_start:08x}..0x{stack_end:08x})\r\n",
        bootinfo = bootinfo_ref as *const _ as usize,
        bi_start = bootinfo_range.start,
        bi_end = bootinfo_range.end,
        ipc = ipc_buffer_ptr.as_ptr() as usize,
        heap_start = heap_range.start,
        heap_end = heap_range.end,
        stack_start = stack_range.start,
        stack_end = stack_range.end,
    );
    debug_uart_str(state_line.as_str());
    let mut console = DebugConsole::new(platform);

    #[inline(always)]
    fn report_first_retype_failure<P: Platform>(
        console: &mut DebugConsole<'_, P>,
        err: sel4_sys::seL4_Error,
    ) -> ! {
        let mut line = heapless::String::<160>::new();
        let _ = write!(
            line,
            "[boot] first retypes failed: {} ({})",
            err as i32,
            error_name(err),
        );
        console.writeln_prefixed(line.as_str());
        panic!("first retypes failed: {}", error_name(err));
    }

    extern "C" {
        #[link_name = "_text"]
        static __text_start: u8;
        #[link_name = "_end"]
        static __text_end: u8;
    }

    let text_start = core::ptr::addr_of!(__text_start) as usize;
    let text_end = core::ptr::addr_of!(__text_end) as usize;
    guards::init_text_bounds(text_start, text_end);

    #[cfg_attr(feature = "bootstrap-minimal", allow(unused_mut))]
    let mut boot_cspace = CSpace::from_bootinfo(bootinfo_ref);
    let boot_first_free = boot_cspace.next_free_slot();
    debug_assert_eq!(boot_first_free, cspace_window.first_free);
    debug_assert_eq!(boot_cspace.depth(), cspace_window.bits);
    let (_, empty_end) = bootinfo_view.init_cnode_empty_range();
    log::info!(
        "[rt-fix] cspace window [0x{start:04x}..0x{end:04x}), initBits={bits}, initCNode=0x{root:04x}",
        start = cspace_window.first_free,
        end = empty_end,
        bits = cspace_window.bits,
        root = cspace_window.root
    );
    match crate::bootstrap::cspace::prove_dest_path_with_bootinfo(bootinfo_ref, boot_first_free) {
        Ok(()) => log::info!("[probe] dest path OK at slot=0x{boot_first_free:04x}"),
        Err(err) => panic!(
            "dest path invalid: Copy BootInfo -> slot=0x{boot_first_free:04x} failed err={err}",
        ),
    }
    let _ = pending_boot_phases.push(BootPhase::CSpaceInit);

    audit_boot::emit_stage0_header(|line| console.writeln_prefixed(line));

    #[cfg(debug_assertions)]
    log_text_span();

    console.report_bootinfo(&bootinfo_view);

    let mut cs_line = heapless::String::<96>::new();
    let _ = write!(
        cs_line,
        "cs: root=0x{root:04x} bits={bits} first_free=0x{first_free:04x}",
        root = cspace_window.root,
        bits = cspace_window.bits,
        first_free = boot_first_free,
    );
    console.writeln_prefixed(cs_line.as_str());

    audit_boot::emit_version_banner(|line| console.writeln_prefixed(line));

    bootinfo_debug_dump(&bootinfo_view);

    log::info!(
        "[caps] Null={} TCB={} CNode={} VSpace={} IPCBuf={} BootInfo={}",
        sel4_sys::seL4_CapNull,
        sel4_sys::seL4_CapInitThreadTCB,
        sel4_sys::seL4_CapInitThreadCNode,
        sel4_sys::seL4_CapInitThreadVSpace,
        sel4_sys::seL4_CapInitThreadIPCBuffer,
        sel4_sys::seL4_CapBootInfoFrame,
    );
    debug_identify_boot_caps();
    cspace_sys::dump_init_cnode_slots(0..32);

    // Confirm the init CNode path using the kernel-advertised radix (`initBits = 13`)
    // and empty window before consuming slots inside `[empty_start..empty_end)`.
    if let Err(err) = cspace_sys::verify_root_cnode_slot(
        bootinfo_ref,
        cspace_window.first_free as sel4_sys::seL4_Word,
    ) {
        panic!(
            "init CNode path probe failed: slot=0x{:04x} err={} ({})",
            cspace_window.first_free,
            err,
            error_name(err),
        );
    }

    #[cfg(feature = "untyped-debug")]
    {
        crate::bootstrap::untyped::enumerate_and_plan(bootinfo_ref);
    }

    ensure_device_pt_pool(bootinfo_ref);

    #[cfg_attr(feature = "bootstrap-minimal", allow(unused_mut))]
    let mut kernel_env = KernelEnv::new(
        bootinfo_ref,
        device_pt_pool().map(DevicePtPool::from_config),
        reserved_vaddrs,
    );
    let extra_bytes = bootinfo_view.extra();
    let extra_range = bootinfo_view.extra_range();
    let mut extra_state_line = HeaplessString::<192>::new();
    let extra_ptr = extra_bytes.as_ptr() as usize;
    let _ = write!(
        extra_state_line,
        "[state] extra_range=[0x{start:016x}..0x{end:016x}) ptr=0x{ptr:016x} len={len}\r\n",
        start = extra_range.start,
        end = extra_range.end,
        ptr = extra_ptr,
        len = extra_bytes.len(),
    );
    debug_uart_str(extra_state_line.as_str());
    debug_uart_str("[breadcrumb] before DTB locate\r\n");
    let dtb_deferred = if !extra_bytes.is_empty() {
        console.writeln_prefixed("[boot] deferring DTB parse");
        let _ = pending_boot_phases.push(BootPhase::DTBParseDeferred);
        true
    } else {
        false
    };

    check_bootinfo(&mut boot_guard, "MARK 30");
    boot_log::force_uart_line("[MARK 30] after DTB deferred");

    check_bootinfo(&mut boot_guard, "MARK 31");
    boot_log::force_uart_line("[MARK 31] before canonical_cspace");
    #[cfg(feature = "canonical_cspace")]
    {
        crate::bootstrap::retype::canonical_cspace_console(bootinfo_ref);
    }

    check_bootinfo(&mut boot_guard, "MARK 32");
    boot_log::force_uart_line("[MARK 32] after canonical_cspace");

    check_bootinfo(&mut boot_guard, "MARK 33");
    boot_log::force_uart_line("[MARK 33] before cap-probes");
    #[cfg(feature = "cap-probes")]
    #[cfg_attr(feature = "bootstrap-minimal", allow(unused_variables))]
    let mut first_retypes: Option<FirstRetypeResult> = None;
    #[cfg(not(feature = "cap-probes"))]
    #[cfg_attr(feature = "bootstrap-minimal", allow(unused_variables))]
    let first_retypes: Option<FirstRetypeResult> = None;

    #[cfg(feature = "cap-probes")]
    {
        if let Some((first_ut_cap, _)) = bi_extra::first_regular_untyped_from_extra(bootinfo_ref) {
            match cspace_first_retypes(bootinfo_ref, &mut boot_cspace, first_ut_cap) {
                Ok(result) => first_retypes = Some(result),
                Err(err) => {
                    report_first_retype_failure(&mut console, err);
                }
            }
        } else if let Some(first_ut_cap) = first_regular_untyped(bootinfo_ref) {
            match cspace_first_retypes(bootinfo_ref, &mut boot_cspace, first_ut_cap) {
                Ok(result) => first_retypes = Some(result),
                Err(err) => {
                    report_first_retype_failure(&mut console, err);
                }
            }
        } else {
            console.writeln_prefixed("[boot] no RAM-backed untyped for proof retypes");
        }
    }

    check_bootinfo(&mut boot_guard, "MARK 34");
    boot_log::force_uart_line("[MARK 34] after cap-probes");

    check_bootinfo(&mut boot_guard, "MARK 35");
    boot_log::force_uart_line("[MARK 35] before ipc_vaddr");
    #[cfg_attr(feature = "bootstrap-minimal", allow(unused_variables))]
    let ipc_vaddr = Some(ipc_buffer_ptr.as_ptr() as usize);
    let ipc_frame = sel4_sys::seL4_CapInitThreadIPCBuffer;

    check_bootinfo(&mut boot_guard, "MARK 36");
    boot_log::force_uart_line("[MARK 36] before bootstrap-minimal");
    #[cfg(feature = "bootstrap-minimal")]
    {
        check_bootinfo(&mut boot_guard, "MARK 37");
        boot_log::force_uart_line("[MARK 37] enter bootstrap-minimal");
        log::warn!(
            "[boot] bootstrap-minimal: skipping EP retype/PL011 map/TCB copy; entering console"
        );
        crate::boot::ep::publish_root_ep(sel4_sys::seL4_CapNull);
        console.writeln_prefixed("[boot] bootstrap-minimal: entering console");
        boot_guard.record_substep("commit.minimal.path");
        boot_guard.commit_minimal();
        boot_log::unlock_post_commit_ipc_logging();
        let precommit_blocks = boot_log::precommit_ipc_forbidden();
        if precommit_blocks > 0 {
            let mut marker = HeaplessString::<96>::new();
            let _ = write!(marker, "[log] precommit_ipc_forbidden={precommit_blocks}");
            boot_log::force_uart_line(marker.as_str());
        }
        boot_log::force_uart_line("[console] serial fallback ready");
        if !crate::ipc::ep_is_valid(crate::sel4::root_endpoint()) {
            boot_log::force_uart_line(
                "[console] IPC disabled (root ep = null); use local commands only",
            );
        }
        crate::bootstrap::run_minimal(bootinfo_ref);
        crate::userland::start_console_or_cohsh(platform);
    }

    check_bootinfo(&mut boot_guard, "MARK 38");
    boot_log::force_uart_line("[MARK 38] before bootstrap_ep");
    boot_guard.record_mark("[mark] ep.bootstrap.begin");
    boot_log::force_uart_line("[mark] ep.bootstrap.begin");
    boot_guard.record_substep("bootstrap_ep.pre");
    let mut ep_report = crate::boot::ep::RootEpReport::default();
    let mut ep_probe = heapless::String::<160>::new();
    let _ = write!(
        ep_probe,
        "[boot] root-ep preflight ep_ready={} first_free=0x{slot:04x}",
        sel4::ep_ready() as u8,
        slot = boot_cspace.next_free_slot(),
    );
    boot_log::force_uart_line(ep_probe.as_str());
    let (ep_slot, boot_ep_ok) = match ep::bootstrap_ep(
        &bootinfo_snapshot,
        &mut boot_cspace,
        &mut ep_report,
    ) {
        Ok(slot) => {
            check_bootinfo(&mut boot_guard, "MARK 39");
            boot_log::force_uart_line("[MARK 39] after bootstrap_ep ok");
            (slot, true)
        }
        Err(err) => {
            crate::trace::trace_fail(b"bootstrap_ep", err);
            boot_guard.record_reason("bootstrap_ep", Some(err as i32));
            let mut line = heapless::String::<160>::new();
            let _ = write!(
                line,
                "bootstrap_ep failed: {} ({})",
                err as i32,
                error_name(err)
            );
            console.writeln_prefixed(line.as_str());
            #[cfg(feature = "strict-bootstrap")]
            {
                panic!("bootstrap_ep failed: {}", error_name(err));
            }
            #[cfg(not(feature = "strict-bootstrap"))]
            {
                log::error!(
                    "[fail:bootstrap_ep] err={} ({})",
                    err as i32,
                    error_name(err)
                );
                let fail_line = match err {
                    sel4_sys::seL4_FailedLookup => "[FAIL] bootstrap_ep err=FailedLookup",
                    sel4_sys::seL4_InvalidArgument => "[FAIL] bootstrap_ep err=InvalidArgument",
                    sel4_sys::seL4_InvalidCapability => "[FAIL] bootstrap_ep err=InvalidCapability",
                    sel4_sys::seL4_IllegalOperation => "[FAIL] bootstrap_ep err=IllegalOperation",
                    sel4_sys::seL4_RangeError => "[FAIL] bootstrap_ep err=RangeError",
                    _ => "[FAIL] bootstrap_ep err=UNKNOWN",
                };
                boot_log::force_uart_line(fail_line);
                let mut structured = heapless::String::<192>::new();
                let _ = write!(
                    structured,
                    "[boot:abort] ep_slot=0x{slot:04x} verify={verify:?} retype={retype:?} ident=0x{ident:04x}",
                    slot = ep_report.ep_slot,
                    verify = ep_report.verify_err,
                    retype = ep_report.retype_err,
                    ident = ep_report.slot_ident,
                );
                boot_log::force_uart_line(structured.as_str());
                let fallback_existing = sel4::ep_ready();
                let fallback_ident = sel4::debug_cap_identify(ep_report.ep_slot);
                let fallback_slot = if fallback_existing {
                    sel4::root_endpoint()
                } else if fallback_ident == sel4_sys::seL4_EndpointObject as sel4::seL4_Word {
                    ep_report.ep_slot
                } else {
                    sel4_sys::seL4_CapNull
                };
                if fallback_slot != sel4_sys::seL4_CapNull {
                    boot_log::force_uart_line(
                        "[boot] bootstrap_ep fallback: reusing existing endpoint",
                    );
                    crate::boot::ep::publish_root_ep(fallback_slot);
                    (fallback_slot, false)
                } else {
                    panic!(
                        "bootstrap_ep failed: {} ({}) without fallback",
                        err as i32,
                        error_name(err)
                    );
                }
            }
        }
    };
    let mut ep_status = heapless::String::<192>::new();
    let _ = write!(
        ep_status,
        "[boot] root-ep report slot=0x{slot:04x} verify={verify:?} retype={retype:?} ident=0x{ident:04x} preexisting={preexisting}",
        slot = ep_report.ep_slot,
        verify = ep_report.verify_err,
        retype = ep_report.retype_err,
        ident = ep_report.slot_ident,
        preexisting = ep_report.preexisting as u8,
    );
    boot_log::force_uart_line(ep_status.as_str());
    probe_canary("[probe] after.bootstrap_ep");

    if boot_ep_ok {
        let (empty_start, empty_end) = bootinfo_view.init_cnode_empty_range();
        if !(empty_start <= ep_slot && ep_slot < empty_end) {
            hard_guard_fail(
                "bootstrap_ep",
                HardGuardViolation::EPInvalidOrNotInEmptyWindow,
            );
        }

        let ident = sel4::debug_cap_identify(ep_slot) as u32;
        if ident == 0 {
            hard_guard_fail(
                "bootstrap_ep",
                HardGuardViolation::EPIdentifyInvalid { ident },
            );
        }

        sel4::set_ep_validated(true);
        boot_guard.record_invariant("root_ep.ready");
    } else {
        sel4::set_ep_validated(false);
        log::warn!(
            "[boot] continuing with existing root endpoint=0x{slot:04x}",
            slot = ep_slot
        );
    }

    boot_log::force_uart_line("[breadcrumb] before trace_ep");
    crate::trace::trace_ep(ep_slot);
    boot_log::force_uart_line("[breadcrumb] after trace_ep");

    boot_log::force_uart_line("[breadcrumb] before ep publish line");
    let mut ep_line = heapless::String::<96>::new();
    let _ = write!(
        ep_line,
        "[boot] root endpoint published ep=0x{ep:04x}",
        ep = ep_slot
    );
    console.writeln_prefixed(ep_line.as_str());
    let mut ep_mark = heapless::String::<128>::new();
    let _ = write!(
        ep_mark,
        "[mark] ep.publish.ok root_ep=0x{ep:04x}",
        ep = ep_slot
    );
    boot_guard.record_mark("[mark] ep.publish.ok");
    boot_log::force_uart_line(ep_mark.as_str());
    boot_log::force_uart_line("[breadcrumb] after ep publish line");

    // Boot tracer phase advancement must not run before the root EP exists,
    // because faults cannot be delivered and tracer internals may touch memory.
    debug_uart_str("[breadcrumb] before boot_tracer drain\r\n");
    boot_log::force_uart_line("[breadcrumb] before boot_tracer drain");
    for phase in pending_boot_phases.drain(..) {
        let phase_marker = match phase {
            BootPhase::Begin => "[breadcrumb] tracer phase=Begin",
            BootPhase::CSpaceInit => "[breadcrumb] tracer phase=CSpaceInit",
            BootPhase::UntypedEnumerate => "[breadcrumb] tracer phase=UntypedEnumerate",
            BootPhase::RetypeBegin => "[breadcrumb] tracer phase=RetypeBegin",
            BootPhase::RetypeProgress { .. } => "[breadcrumb] tracer phase=RetypeProgress",
            BootPhase::RetypeDone => "[breadcrumb] tracer phase=RetypeDone",
            BootPhase::DTBParseDeferred => "[breadcrumb] tracer phase=DTBParseDeferred",
            BootPhase::DTBParseDone => "[breadcrumb] tracer phase=DTBParseDone",
            BootPhase::EPAttachWait => "[breadcrumb] tracer phase=EPAttachWait",
            BootPhase::EPAttachOk => "[breadcrumb] tracer phase=EPAttachOk",
            BootPhase::HandOff => "[breadcrumb] tracer phase=HandOff",
        };
        boot_log::force_uart_line(phase_marker);
        boot_tracer().advance(phase);
    }
    boot_log::force_uart_line("[breadcrumb] after boot_tracer drain");

    let ipc_ptr_guard = ipc_buffer_ptr;
    debug_assert_eq!(
        (ipc_ptr_guard.as_ptr() as usize) & (IPC_PAGE_BYTES - 1),
        0,
        "IPC buffer alignment drifted",
    );

    check_bootinfo(&mut boot_guard, "MARK 40");
    boot_log::force_uart_line("[MARK 40] ipcbuf ready before tcb copy");

    debug_assert_eq!(ep_slot, root_endpoint());
    let tcb_copy_slot = if let Some(ref info) = first_retypes {
        info.tcb_copy_slot
    } else {
        crate::bp!("tcb.copy.begin");
        check_bootinfo(&mut boot_guard, "MARK 41");
        boot_log::force_uart_line("[MARK 41] before tcb copy");
        let copy_slot = tcb::bootstrap_copy_init_tcb(bootinfo_ref, &mut boot_cspace)
            .unwrap_or_else(|err| {
                panic!(
                    "copying init TCB capability failed: {} ({})",
                    err,
                    error_name(err)
                );
            });
        crate::bp!("tcb.copy.end");
        copy_slot
    };

    if let Some(ipc_vaddr) = ipc_vaddr {
        let ipc_view = match ipcbuf::install_ipc_buffer(
            &mut kernel_env,
            sel4_sys::seL4_CapInitThreadTCB,
            ipc_frame,
            ipc_vaddr,
        ) {
            Ok(view) => Some(view),
            Err(code) => {
                let err = code as sel4_sys::seL4_Error;
                log_precommit_exit(
                    EarlyBootPhase::IPCInstall,
                    "ipcbuf.install-ipc-buffer",
                    false,
                    PrecommitReason::IpcInstall,
                    Some(err),
                    crate::sel4::error_name(err),
                    file!(),
                    line!(),
                );
                let fallback_view = kernel_env
                    .ipc_buffer_view()
                    .or_else(|| Some(kernel_env.record_boot_ipc_buffer(ipc_frame, ipc_vaddr)));
                if let Some(view) = fallback_view {
                    ipcbuf_mode = IpcBufferMode::BootProvidedFallback;
                    Some(view)
                } else {
                    log_precommit_exit(
                        EarlyBootPhase::IPCInstall,
                        "ipcbuf.install-ipc-buffer.fatal",
                        true,
                        PrecommitReason::IpcInstall,
                        Some(err),
                        crate::sel4::error_name(err),
                        file!(),
                        line!(),
                    );
                    return Err(BootError::Fatal(format!(
                        "ipc buffer install failed without fallback: {} ({})",
                        code,
                        error_name(err)
                    )));
                }
            }
        };

        if let Some(view) = ipc_view {
            boot_guard.record_ipc_buffer(Some(ipc_vaddr), Some(ipcbuf_mode));
            let mut msg = heapless::String::<112>::new();
            let _ = write!(
                msg,
                "[boot] ipc buffer mapped @ 0x{vaddr:08x}",
                vaddr = view.vaddr(),
            );
            console.writeln_prefixed(msg.as_str());
        }
    } else {
        console.writeln_prefixed("bootinfo.ipcBuffer missing");
    }

    let mut fault_ep_slot = ep_slot;
    if ep_slot != sel4_sys::seL4_CapNull {
        match crate::boot::ep::bootstrap_fault_ep(&bootinfo_snapshot, &mut boot_cspace) {
            Ok(slot) => {
                fault_ep_slot = slot;
                log::info!(
                    target: "root_task::bootstrap",
                    "[boot] dedicated fault endpoint ready ep=0x{slot:04x}",
                    slot = slot
                );
            }
            Err(err) => {
                log::warn!(
                    target: "root_task::bootstrap",
                    "[boot] unable to create dedicated fault endpoint: {} ({}) — reusing root ep",
                    err as sel4_sys::seL4_Word,
                    error_name(err)
                );
            }
        }
    }

    boot_guard.record_endpoints(ep_slot, fault_ep_slot);
    let endpoints = KernelEndpoints::new(ep_slot, fault_ep_slot);
    let bootstrap_ipc = KernelIpc::new(endpoints.control, endpoints.fault);
    boot_guard.record_substep("commit.minimal.ready");
    boot_guard.commit_minimal();
    if sel4::ep_ready() && sel4::ep_validated() {
        sel4::unlock_ipc_send();
    } else {
        boot_log::force_uart_line("[ipc-guard] IPC remained locked at commit_minimal");
    }
    boot_log::unlock_post_commit_ipc_logging();
    let precommit_blocks = boot_log::precommit_ipc_forbidden();
    if precommit_blocks > 0 {
        let mut marker = HeaplessString::<96>::new();
        let _ = write!(marker, "[log] precommit_ipc_forbidden={precommit_blocks}");
        boot_log::force_uart_line(marker.as_str());
    }
    boot_log::force_uart_line("[boot] commit_minimal satisfied");
    let mut post_commit = PostCommitState::new();

    if fault_ep_slot != sel4_sys::seL4_CapNull {
        let guard_bits =
            sel4::word_bits().saturating_sub(bootinfo_ref.init_cnode_bits() as sel4_sys::seL4_Word);
        let guard_data = sel4::cap_data_guard(0, guard_bits);
        match install_fault_handler_for_tcb(
            &mut boot_cspace,
            sel4_sys::seL4_CapInitThreadTCB,
            fault_ep_slot,
            guard_data,
            "init-tcb",
        ) {
            Ok(badge) => {
                log::info!(
                    target: "root_task::bootstrap",
                    "[boot] fault handler installed tcb_slot=0x{slot:04x} ep=0x{ep:04x} badge=0x{badge:04x}",
                    slot = tcb_copy_slot,
                    ep = fault_ep_slot,
                    badge = badge,
                );
            }
            Err(fault_handler_err) => {
                let mut line = heapless::String::<200>::new();
                let _ = write!(
                    line,
                    "[boot] failed to install fault handler: {} ({})",
                    fault_handler_err as sel4_sys::seL4_Word,
                    error_name(fault_handler_err)
                );
                console.writeln_prefixed(line.as_str());
                boot_log::force_uart_line(line.as_str());
                post_commit.flag_failure("fault.handler.install", error_name(fault_handler_err));
            }
        }
    } else {
        log::warn!(
            target: "root_task::bootstrap",
            "[boot] skipping fault handler install: ep_slot is null (0x{ep:04x})",
            ep = ep_slot
        );
    }

    let mut cs = CSpaceCtx::new(bootinfo_view, boot_cspace);
    cs.tcb_copy_slot = tcb_copy_slot;
    // Track bootstrap slot usage by measuring the init CSpace cursor before
    // further retype activity. This covers the endpoint bootstrap, the TCB
    // copy, and any proof-of-life retypes that ran prior to entering the
    // retype plan, ensuring the HAL reserves every populated slot.
    let initial_consumed =
        (cs.next_candidate_slot() as usize).saturating_sub(boot_first_free as usize);
    let mut consumed_slots: usize = cmp::max(initial_consumed, 2);
    let mut retyped_objects: u32 = 0;

    debug_uart_str("[breadcrumb] before UntypedPlan\r\n");
    probe_canary("[probe] before.UntypedPlan");
    if let Err(err) = sequencer.advance(BootstrapPhase::UntypedPlan) {
        post_commit.flag_failure("phase.UntypedPlan", &err);
    } else {
        boot_guard.record_phase("UntypedPlan");
        boot_tracer().advance(BootPhase::UntypedEnumerate);
        check_bootinfo(&mut boot_guard, "[mark] phase.UntypedPlan");
    }
    let mut notification_selection =
        pick_untyped(bootinfo_ref, sel4_sys::seL4_NotificationBits as u8);
    probe_canary("[probe] after.UntypedPlan");

    if let Err(err) = sequencer.advance(BootstrapPhase::RetypeCommit) {
        post_commit.flag_failure("phase.RetypeCommit", &err);
    } else {
        boot_guard.record_phase("RetypeCommit");
        check_bootinfo(&mut boot_guard, "[mark] phase.RetypeCommit");
    }
    probe_canary("[probe] after.RetypeCommit");
    if let Err(err) = bootstrap_notification(&mut cs, &mut notification_selection) {
        let mut line = heapless::String::<160>::new();
        let err_code = err as i32;
        let err_name = error_name(err);
        let _ = write!(
            line,
            "[boot] notification retype failed ut=0x{ut:03x} err={err} ({name})",
            ut = notification_selection.cap,
            err = err_code,
            name = err_name
        );
        console.writeln_prefixed(line.as_str());
    } else {
        consumed_slots += 1;
        retyped_objects += 1;
    }

    let mut watchdog = BootWatchdog::new();
    match retype_selection(&mut cs, &mut notification_selection, || watchdog.poll()) {
        Ok(count) => {
            consumed_slots += count as usize;
            retyped_objects += count;
        }
        Err(err) if err == sel4_sys::seL4_NotEnoughMemory => {
            let mut line = heapless::String::<160>::new();
            let _ = write!(
                line,
                "[boot] retype plan exhausted untyped ut=0x{ut:03x}: {code} ({name})",
                ut = notification_selection.cap,
                code = err as i32,
                name = error_name(err)
            );
            console.writeln_prefixed(line.as_str());
        }
        Err(err) => {
            let mut line = heapless::String::<128>::new();
            let _ = write!(
                line,
                "[boot] retype plan failed: {} ({})",
                err as i32,
                error_name(err)
            );
            console.writeln_prefixed(line.as_str());
            post_commit.flag_failure("retype.plan", error_name(err));
        }
    }

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
            cs.root_cnode_copy_slot = sel4_sys::seL4_CapInitThreadCNode;
            let mut line = heapless::String::<160>::new();
            let _ = write!(
                line,
                "[boot] writable init CNode mint failed: {} ({}) — falling back to slot 0x{slot:04x}",
                err,
                error_name(err),
                slot = sel4_sys::seL4_CapInitThreadCNode
            );
            console.writeln_prefixed(line.as_str());
        }
    }

    let measured_consumed =
        (cs.next_candidate_slot() as usize).saturating_sub(boot_first_free as usize);
    if measured_consumed != consumed_slots {
        log::warn!(
            "[boot] reconciled bootstrap slot usage measured={measured_consumed} tracked={consumed_slots}",
        );
        consumed_slots = measured_consumed;
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

    kernel_env.record_untyped_bytes(
        notification_selection.index,
        notification_selection.used_bytes,
    );
    let mut hal = KernelHal::new(kernel_env);
    if consumed_slots > 0 {
        hal.consume_bootstrap_slots(consumed_slots);
    }

    #[cfg(feature = "kernel")]
    let ninedoor: &'static mut NineDoorBridge = {
        let bridge = Box::new(NineDoorBridge::new());
        Box::leak(bridge)
    };

    let pl011_paddr = usize::try_from(PL011_PADDR)
        .expect("PL011 physical address must fit within usize on this platform");
    let mut uart_slot: Option<sel4_sys::seL4_CPtr> = None;
    let mut uart_mmio: Option<Pl011Mmio> = None;
    let (uart_region, pl011_map_error) = match hal.map_device(pl011_paddr) {
        Ok(region) => (Some(region), None),
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
            if err == sel4_sys::seL4_NotEnoughMemory {
                log::error!(
                    "[pl011] device PageTable retype hit NotEnoughMemory; planner under-reserved RAM for device mappings"
                );
            }

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
                            "retype status=ok raw.untyped=0x{ucap:08x} raw.paddr=0x{paddr:08x} raw.size_bits={usize_bits} raw.slot=0x{slot:04x} raw.offset=0x{offset:04x} raw.depth={depth} raw.root=0x{root:04x} raw.node_index=0x{node_index:04x} obj_type={otype} obj_size_bits={obj_bits}",
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
                            "retype status={err}({code}) raw.untyped=0x{ucap:08x} raw.paddr=0x{paddr:08x} raw.size_bits={usize_bits} raw.slot=0x{slot:04x} raw.offset={offset:04x} raw.depth={depth} raw.root=0x{root:04x} raw.node_index=0x{node_index:04x} obj_type={otype} obj_size_bits={obj_bits}",
                            err = error_name(code),
                            code = code,
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

                let mut kind = heapless::String::<176>::new();
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

            match hal.device_coverage(pl011_paddr, DEVICE_FRAME_BITS) {
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

            post_commit.flag_failure("pl011.map.sel4", error_label);
            (None, Some(err))
        }
        Err(HalError::Unsupported(reason)) => {
            let mut line = heapless::String::<128>::new();
            let _ = write!(
                line,
                "map_device(0x{addr:08x}) unsupported: {reason}",
                addr = PL011_PADDR,
                reason = reason,
            );
            console.writeln_prefixed(line.as_str());

            (None, None)
        }
        Err(HalError::NoPci) => {
            let mut line = heapless::String::<128>::new();
            let _ = write!(
                line,
                "map_device(0x{addr:08x}) failed: pci unavailable",
                addr = PL011_PADDR,
            );
            console.writeln_prefixed(line.as_str());

            (None, None)
        }
        Err(HalError::InvalidPciAddress) => {
            let mut line = heapless::String::<128>::new();
            let _ = write!(
                line,
                "map_device(0x{addr:08x}) failed: invalid pci address",
                addr = PL011_PADDR,
            );
            console.writeln_prefixed(line.as_str());

            (None, None)
        }
        Err(HalError::PciBarUnavailable) => {
            let mut line = heapless::String::<128>::new();
            let _ = write!(
                line,
                "map_device(0x{addr:08x}) failed: pci bar unavailable",
                addr = PL011_PADDR,
            );
            console.writeln_prefixed(line.as_str());

            (None, None)
        }
    };

    if let Some(region) = uart_region {
        let mmio = Pl011Mmio::mapped(pl011_paddr, region.cap(), region.ptr());
        mmio.assert_page_coverage(1 << sel4::PAGE_BITS, 0x0ff);
        log::info!(
            target: "boot",
            "[uart:mmio] paddr=0x{paddr:08x} cap=0x{cap:04x} vaddr=0x{vaddr:016x} mapped={mapped}",
            paddr = mmio.paddr(),
            cap = mmio.cap().unwrap_or(sel4_sys::seL4_CapNull),
            vaddr = mmio.vaddr().as_ptr() as usize,
            mapped = mmio.is_mapped(),
        );

        let mut map_line = heapless::String::<128>::new();
        let mapped_vaddr = mmio.vaddr().as_ptr() as usize;
        let _ = write!(
            map_line,
            "[vspace:map] pl011 paddr=0x{paddr:08x} -> vaddr=0x{vaddr:016x} attrs=UNCACHED OK",
            vaddr = mapped_vaddr,
            paddr = PL011_PADDR,
        );
        console.writeln_prefixed(map_line.as_str());

        uart_pl011::publish_uart_slot(region.cap());
        early_uart::register_console_base(mapped_vaddr);
        uart_slot = Some(region.cap());
        uart_mmio = Some(mmio);
    } else {
        let label = pl011_map_error
            .map(error_name)
            .unwrap_or("mapping not available");
        post_commit.flag_failure("pl011.map", label);
        console.writeln_prefixed("[uart] PL011 unavailable; continuing without serial console");
    }

    let mut serial: Option<
        SerialPort<Pl011, DEFAULT_RX_CAPACITY, DEFAULT_TX_CAPACITY, DEFAULT_LINE_CAPACITY>,
    > = None;

    #[cfg(feature = "debug-input")]
    {
        if let Some(ref mmio) = uart_mmio {
            let mut driver = Pl011::new(mmio.vaddr());
            driver.init();
            console.writeln_prefixed("[uart] init OK");
            driver.write_str("[console] PL011 console online\n");
            driver.write_str("[cohesix:root-task] uart logger online\n");
            let console_caps = ConsoleCaps {
                init_cnode: bootinfo_ref.init_cnode_cap(),
                init_vspace: sel4_sys::seL4_CapInitThreadVSpace,
                init_tcb: bootinfo_ref.init_tcb_cap(),
                console_endpoint_slot: first_retypes
                    .as_ref()
                    .map(|info| info.endpoint_slot)
                    .unwrap_or(crate::sel4::seL4_CapNull),
                tcb_copy_slot: first_retypes.as_ref().map(|info| info.tcb_copy_slot),
            };
            start_console(driver, console_caps);
        } else {
            post_commit.flag_failure("uart.debug-input", "pl011 mapping unavailable");
        }
    }

    #[cfg(not(feature = "debug-input"))]
    {
        if let Some(ref mmio) = uart_mmio {
            let mut driver = Pl011::new(mmio.vaddr());
            driver.init();
            console.writeln_prefixed("[uart] init OK");
            driver.write_str("[console] PL011 console online\n");
            #[cfg(all(feature = "kernel", not(sel4_config_printing)))]
            {
                unsafe {
                    EARLY_UART_SINK = DebugSink {
                        context: mmio.vaddr().as_ptr().cast::<()>(),
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
                if emit_addr & 0b11 != 0 || emit_addr <= 0x1000 || ctx_addr <= 0x1000 {
                    post_commit.flag_failure("debug-sink", "debug sink address invalid");
                } else {
                    sel4_panicking::install_debug_sink(sink);
                }
            }
            driver.write_str("[cohesix:root-task] uart logger online\n");
            log::info!("[boot] after uart logger online");
            serial = Some(SerialPort::<
                _,
                DEFAULT_RX_CAPACITY,
                DEFAULT_TX_CAPACITY,
                DEFAULT_LINE_CAPACITY,
            >::new(driver));
        } else {
            console.writeln_prefixed("[uart] init skipped: PL011 mapping unavailable");
        }

        check_bootinfo(&mut boot_guard, "[mark] net.init.pre");
        #[cfg(all(feature = "net-console", feature = "kernel"))]
        let net_backend_label = DEFAULT_NET_BACKEND.label();
        #[cfg(all(feature = "net-console", feature = "kernel"))]
        let (net_stack, virtio_present, net_init_error) = {
            use crate::net::{init_net_console, NetConsoleError};

            if !sel4::ep_ready() || !sel4::ep_validated() {
                boot_log::force_uart_line("[net-console] disabled reason=no-root-ep err=0");
                log::warn!("[net-console] skipped: root endpoint not ready");
                (None, false, None)
            } else {
                let config = crate::net::ConsoleNetConfig::default();
                match init_net_console(&mut hal, config) {
                    Ok(stack) => {
                        let mac = stack.hardware_address();
                        let ip = stack.ipv4_address();
                        let port = stack.console_listen_port();
                        let mut ok_line = heapless::String::<160>::new();
                        let _ =
                            write!(ok_line, "[net-console] ready ip={ip} port={port} mac={mac}");
                        boot_log::force_uart_line(ok_line.as_str());
                        boot_guard.record_invariant("net-console.ready");
                        (Some(stack), cfg!(feature = "net-backend-virtio"), None)
                    }
                    Err(err) => {
                        let (reason, err_code) = match err {
                            NetConsoleError::NoDevice => ("no-device", "NoDevice"),
                            NetConsoleError::InvalidConfig(_) => {
                                ("invalid-config", "InvalidConfig")
                            }
                            NetConsoleError::Init(_) => ("init-error", "Init"),
                        };
                        let mut fail_line = heapless::String::<192>::new();
                        let _ = write!(
                            fail_line,
                            "[net-console] disabled reason={reason} err={err_code}"
                        );
                        boot_log::force_uart_line(fail_line.as_str());
                        log::warn!("{} detail={err}", fail_line.as_str());
                        let virtio_present = cfg!(feature = "net-backend-virtio")
                            && !matches!(err, NetConsoleError::NoDevice);
                        let mut detail = heapless::String::<192>::new();
                        let _ = write!(detail, "{err}");
                        (None, virtio_present, Some(detail))
                    }
                }
            }
        };
        #[cfg(all(feature = "net-console", not(feature = "kernel")))]
        let net_stack = None::<()>;
        #[cfg(not(feature = "net-console"))]
        let net_stack = None::<()>;
        check_bootinfo(&mut boot_guard, "[mark] net.init.post");
        log::info!("[boot] net-console init complete; continuing with timers and IPC");
        log::info!(target: "root_task::kernel", "[boot] phase: TimersAndIPC.begin");
        let (timer, ipc) = match run_timers_and_ipc_phase(endpoints, bootstrap_ipc) {
            Ok(ok) => ok,
            Err((err, ipc)) => {
                post_commit.flag_failure("timers+ipc", &err);
                let timer = KernelTimer::bypass(KERNEL_TIMER_PERIOD_MS);
                (timer, ipc)
            }
        };

        let mut tickets: TicketTable<4> = TicketTable::new();
        for spec in manifest::ticket_inventory() {
            let _ = tickets.register(spec.role, spec.token);
        }

        crate::bp!("spawn.worker.begin");
        crate::bp!("spawn.worker.end");

        crate::bp!("dtb.parse.begin");
        if dtb_deferred {
            console.writeln_prefixed("[boot] dtb locate skipped/failed: deferred");
        } else if !extra_bytes.is_empty() {
            match bi_extra::locate_dtb(extra_bytes, extra_range.clone()) {
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
                    }
                    Err(err) => {
                        let mut msg = heapless::String::<96>::new();
                        let _ = write!(msg, "[boot] dtb parse failed: {err}");
                        console.writeln_prefixed(msg.as_str());
                    }
                },
                Err(err) => {
                    let mut msg = heapless::String::<112>::new();
                    let _ = write!(msg, "[boot] dtb locate skipped/failed: {err}");
                    console.writeln_prefixed(msg.as_str());
                }
            }
        } else {
            console.writeln_prefixed("[boot] no dtb payload present");
        }
        crate::bp!("dtb.parse.end");
        boot_tracer().advance(BootPhase::DTBParseDone);

        debug_uart_str("[breadcrumb] before logger.switch\r\n");
        crate::bp!("logger.switch.begin");
        let logger_switch_ok = if cfg!(feature = "dev-virt") {
            log::info!(
                target: "root_task::kernel",
                "[boot] logger.switch: EP disabled in dev-virt (UART-only)"
            );
            false
        } else if let Err(err) = boot_log::switch_logger_to_userland() {
            log::error!("[boot] logger switch failed: {:?}", err);
            boot_guard.record_reason("logger.switch", None);
            post_commit.flag_failure("logger.switch", format_args!("{err:?}"));
            false
        } else {
            true
        };
        if logger_switch_ok {
            boot_guard.record_logger_switch(true);
            boot_guard.record_invariant("logger.switch.ok");
        }
        crate::bp!("logger.switch.end");
        debug_uart_str("[dbg] logger.switch complete; about to send bootstrap to EP 0x0130\n");
        if !boot_log::bridge_disabled() {
            boot_tracer().advance(BootPhase::EPAttachWait);
        }
        #[cfg(all(feature = "net-console", feature = "kernel"))]
        if let Some(net_stack) = net_stack.as_ref() {
            let mac = net_stack.hardware_address();
            let ip = net_stack.ipv4_address();
            let prefix = net_stack.prefix_len();
            let mut banner = heapless::String::<128>::new();
            if let Some(gw) = net_stack.gateway() {
                let _ = write!(
                    banner,
                    "[net] {net_backend_label} up mac={mac} ip={ip}/{prefix} gw={gw}"
                );
            } else {
                let _ = write!(
                    banner,
                    "[net] {net_backend_label} up mac={mac} ip={ip}/{prefix}"
                );
            }
            console.writeln_prefixed(banner.as_str());
            let mut listen = heapless::String::<64>::new();
            let _ = write!(listen, "[console] tcp listen :{CONSOLE_TCP_PORT}");
            console.writeln_prefixed(listen.as_str());
        } else {
            let detail = net_init_error
                .as_ref()
                .map(heapless::String::as_str)
                .unwrap_or("net stack init failed");
            log::warn!("[boot] net-console unavailable: net stack init failed ({detail})");
        }
        let caps_start = empty_start as u32;
        let caps_end = cs.next_candidate_slot();
        let caps_remaining = cs.remaining_capacity();
        let mut summary = heapless::String::<160>::new();
        let _ = write!(
            summary,
            "[boot:ok] retyped={retyped_objects} caps_used=[0x{caps_start:04x}..0x{caps_end:04x}) left={caps_remaining}",
        );
        boot_log::force_uart_line(summary.as_str());
        crate::bp!("bootstrap.done");
        boot_tracer().advance(BootPhase::HandOff);
        if let Err(err) = sequencer.advance(BootstrapPhase::UserlandHandoff) {
            post_commit.flag_failure("phase.UserlandHandoff", &err);
        } else {
            boot_guard.record_phase("UserlandHandoff");
            check_bootinfo(&mut boot_guard, "[mark] phase.UserlandHandoff");
        }
        boot_log::force_uart_line("[mark] bootstrap.commit");
        boot_guard.commit_full();
        if serial.is_some() {
            boot_log::force_uart_line("[console] serial fallback ready");
        } else {
            boot_log::force_uart_line("[console] serial fallback unavailable");
        }
        crate::bootstrap::run_minimal(bootinfo_ref);
        #[cfg(all(feature = "net-console", feature = "kernel"))]
        let virtio_present_flag = virtio_present;
        #[cfg(not(all(feature = "net-console", feature = "kernel")))]
        let virtio_present_flag = false;

        let features = BootFeatures {
            serial_console: profile::SERIAL_CONSOLE && serial.is_some(),
            net: net_stack.is_some(),
            net_console: profile::NET_CONSOLE && net_stack.is_some(),
        };

        log::info!(
            target: "boot",
            "[boot] net init: virtio_present={} net={} net_console={}",
            virtio_present_flag,
            features.net,
            features.net_console,
        );

        #[cfg(feature = "net-console")]
        let ctx = BootContext {
            bootinfo: bootinfo_view,
            bootinfo_snapshot,
            features,
            endpoints,
            uart_slot,
            uart_mmio,
            serial: RefCell::new(serial),
            timer: RefCell::new(Some(timer)),
            ipc: RefCell::new(Some(ipc)),
            tickets: RefCell::new(Some(tickets)),
            net_stack: RefCell::new(net_stack),
            #[cfg(feature = "kernel")]
            ninedoor: RefCell::new(Some(ninedoor)),
        };

        #[cfg(not(feature = "net-console"))]
        let ctx = BootContext {
            bootinfo: bootinfo_view,
            bootinfo_snapshot,
            features,
            endpoints,
            uart_slot,
            uart_mmio,
            serial: RefCell::new(serial),
            timer: RefCell::new(Some(timer)),
            ipc: RefCell::new(Some(ipc)),
            tickets: RefCell::new(Some(tickets)),
            #[cfg(feature = "kernel")]
            ninedoor: RefCell::new(None),
        };
        return Ok(ctx);
    }
}

const KERNEL_TIMER_PERIOD_MS: u64 = 5;

#[cfg(feature = "bypass-timers-ipc")]
fn run_timers_and_ipc_phase(
    endpoints: KernelEndpoints,
    ipc: KernelIpc,
) -> Result<(KernelTimer, KernelIpc), (BootError, KernelIpc)> {
    log::warn!(
        target: "root_task::kernel",
        "[boot] TimersAndIPC: BYPASSED via feature 'bypass-timers-ipc'"
    );
    log::info!(
        target: "root_task::kernel",
        "[boot] TimersAndIPC: constructing placeholder timer period_ms={}",
        KERNEL_TIMER_PERIOD_MS
    );
    let timer = KernelTimer::bypass(KERNEL_TIMER_PERIOD_MS);
    log::info!(
        target: "root_task::kernel",
        "[boot] TimersAndIPC: constructing placeholder ipc dispatcher ep=0x{ep:04x}",
        ep = endpoints.control.raw()
    );
    log::info!(
        target: "root_task::kernel",
        "[boot] TimersAndIPC: reusing bootstrap dispatcher ep=0x{ep:04x}",
        ep = endpoints.control.raw()
    );
    Ok((timer, ipc))
}

#[cfg(not(feature = "bypass-timers-ipc"))]
fn run_timers_and_ipc_phase(
    endpoints: KernelEndpoints,
    ipc: KernelIpc,
) -> Result<(KernelTimer, KernelIpc), (BootError, KernelIpc)> {
    #[cfg(feature = "bypass-timers")]
    {
        log::warn!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: timer init BYPASSED via feature 'bypass-timers'"
        );
        let timer = KernelTimer::bypass(KERNEL_TIMER_PERIOD_MS);
        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: timers.bypass.end period_cycles={} last_cycles={} enabled={}",
            timer.period_cycles,
            timer.last_cycles,
            timer.enabled
        );

        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: ipc.init.begin ep=0x{ep:04x}",
            ep = endpoints.control.raw()
        );
        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: ipc.init.end ep=0x{ep:04x} staged={staged}",
            ep = endpoints.control.raw(),
            staged = ipc.staged_bootstrap.is_some()
        );

        return Ok((timer, ipc));
    }

    #[cfg(not(feature = "bypass-timers"))]
    {
        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: timers.init.begin period_ms={}",
            KERNEL_TIMER_PERIOD_MS
        );
        let timer = match KernelTimer::init(KERNEL_TIMER_PERIOD_MS) {
            Ok(timer) => timer,
            Err(err) => {
                log::error!(
                    target: "root_task::kernel",
                    "[boot] TimersAndIPC: timers.init.failed: {:?}",
                    err
                );
                return Err((err.into(), ipc));
            }
        };
        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: timers.init.ok period_cycles={} last_cycles={}",
            timer.period_cycles,
            timer.last_cycles
        );

        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: timer.worker.spawn.begin"
        );
        timer.spawn_worker();
        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: timer.worker.spawn.end"
        );

        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: ipc.init.begin ep=0x{ep:04x}",
            ep = endpoints.control.raw()
        );
        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: ipc.init.end ep=0x{ep:04x} staged={staged}",
            ep = endpoints.control.raw(),
            staged = ipc.staged_bootstrap.is_some()
        );

        return Ok((timer, ipc));
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

pub(crate) struct KernelTimer {
    tick: u64,
    period_ms: u64,
    period_cycles: u64,
    last_cycles: u64,
    backend: TimerBackend,
    enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimerBackend {
    ArchCounterPollOnly,
    DummySoftTimer,
}

#[cfg(feature = "timers-arch-counter")]
const TIMER_BACKEND: TimerBackend = TimerBackend::ArchCounterPollOnly;

#[cfg(not(feature = "timers-arch-counter"))]
const TIMER_BACKEND: TimerBackend = TimerBackend::DummySoftTimer;

impl TimerBackend {
    fn label(&self) -> &'static str {
        match self {
            Self::ArchCounterPollOnly => "architected counter poll-only",
            Self::DummySoftTimer => "DummySoftTimer backend (architected counter disabled)",
        }
    }
}

static DUMMY_CYCLE_COUNTER: AtomicU64 = AtomicU64::new(0);

impl KernelTimer {
    pub(crate) fn init(period_ms: u64) -> Result<Self, TimerError> {
        log::info!(
            target: "root_task::kernel::timer",
            "[timers] init: begin period_ms={}",
            period_ms
        );
        if period_ms == 0 {
            log::error!(
                target: "root_task::kernel::timer",
                "[timers] init: invalid period_ms=0"
            );
            return Err(TimerError::InvalidPeriod);
        }

        let freq_hz = timer_freq_hz();
        if freq_hz == 0 {
            log::error!(
                target: "root_task::kernel::timer",
                "[timers] init: timer frequency unavailable"
            );
            return Err(TimerError::FrequencyUnavailable);
        }
        log::info!(
            target: "root_task::kernel::timer",
            "[timers] init: timer_freq_hz={} Hz",
            freq_hz
        );

        log::info!(
            target: "root_task::kernel::timer",
            "[timers] init: compute period_cycles begin period_ms={period_ms}",
            period_ms = period_ms
        );
        let clamped_period = period_ms.max(1);
        let ticks_per_period = freq_hz
            .saturating_mul(clamped_period)
            .checked_div(1_000)
            .ok_or(TimerError::InvalidPeriod)?;
        log::info!(
            target: "root_task::kernel::timer",
            "[timers] init: ticks_per_period={}",
            ticks_per_period
        );
        let period_cycles = compute_period_cycles(freq_hz, clamped_period);
        log::info!(
            target: "root_task::kernel::timer",
            "[timers] init: computed period_cycles={}",
            period_cycles
        );

        let backend = TIMER_BACKEND;
        log::info!(
            target: "root_task::kernel::timer",
            "[timers] init: configuring timer source ({})",
            backend.label()
        );

        let last_cycles = match backend {
            TimerBackend::ArchCounterPollOnly => {
                log::info!(
                    target: "root_task::kernel::timer",
                    "[timers] init: snapshot cntpct begin",
                );
                let last_cycles = read_cntpct();
                if last_cycles == 0 {
                    log::error!(
                        target: "root_task::kernel::timer",
                        "[timers] init: cntpct read returned 0"
                    );
                    return Err(TimerError::CounterUnavailable);
                }
                log::info!(
                    target: "root_task::kernel::timer",
                    "[timers] init: baseline cntpct={} (poll-only)",
                    last_cycles
                );
                last_cycles
            }
            TimerBackend::DummySoftTimer => {
                log::info!(
                    target: "root_task::kernel::timer",
                    "[timers] init: using dummy software counter; snapshots will not read CNT registers",
                );
                let baseline = read_dummy_cycles(period_cycles);
                log::info!(
                    target: "root_task::kernel::timer",
                    "[timers] init: baseline dummy counter={} (poll-only)",
                    baseline
                );
                baseline
            }
        };
        log::info!(
            target: "root_task::kernel::timer",
            "[timers] init: done; timers online (non-blocking)",
        );
        Ok(Self {
            tick: 0,
            period_ms: period_ms.max(1),
            period_cycles,
            last_cycles,
            backend,
            enabled: true,
        })
    }

    pub(crate) fn bypass(period_ms: u64) -> Self {
        log::warn!(
            target: "root_task::kernel::timer",
            "[timers] BYPASS: constructing inert timer period_ms={}",
            period_ms
        );
        Self {
            tick: 0,
            period_ms: period_ms.max(1),
            period_cycles: 1,
            last_cycles: 0,
            backend: TimerBackend::DummySoftTimer,
            enabled: false,
        }
    }

    pub(crate) fn spawn_worker(&self) {
        log::info!(
            target: "root_task::kernel::timer",
            "[timers] worker: spawn requested (deferred wait loop)",
        );
        log::info!(
            target: "root_task::kernel::timer",
            "[timers] worker: cooperative polling (no blocking wait in init)",
        );
    }
}

impl TimerSource for KernelTimer {
    fn poll(&mut self, now_ms: u64) -> Option<TickEvent> {
        if !self.enabled {
            log::trace!(
                target: "root_task::kernel::timer",
                "[timers] poll: bypassed (timer disabled)"
            );
            return None;
        }

        let current = self.snapshot_cycles();
        let elapsed = current.wrapping_sub(self.last_cycles);
        if elapsed < self.period_cycles {
            return None;
        }

        let ticks = core::cmp::max(1, elapsed / self.period_cycles);
        let overshoot = elapsed % self.period_cycles;
        self.last_cycles = current.wrapping_sub(overshoot);
        self.tick = self.tick.saturating_add(ticks);

        let delta_ms = self.period_ms.saturating_mul(ticks);
        let updated_now = now_ms.saturating_add(delta_ms);
        Some(TickEvent {
            tick: self.tick,
            now_ms: updated_now,
        })
    }
}

impl KernelTimer {
    fn snapshot_cycles(&self) -> u64 {
        match self.backend {
            TimerBackend::ArchCounterPollOnly => read_cntpct(),
            TimerBackend::DummySoftTimer => read_dummy_cycles(self.period_cycles),
        }
    }
}

fn compute_period_cycles(freq_hz: u64, period_ms: u64) -> u64 {
    if freq_hz == 0 {
        return 1;
    }

    let clamped_period = period_ms.max(1);
    let cycles = ((freq_hz as u128) * (clamped_period as u128) / 1_000u128) as u64;
    cycles.max(1)
}

#[cfg(feature = "timers-arch-counter")]
fn read_cntpct() -> u64 {
    let value: u64;
    unsafe {
        asm!("mrs {value}, cntpct_el0", value = out(reg) value, options(nomem, preserves_flags));
    }
    value
}

#[cfg(not(feature = "timers-arch-counter"))]
fn read_cntpct() -> u64 {
    0
}

fn read_dummy_cycles(period_cycles: u64) -> u64 {
    let step = period_cycles.max(1);
    DUMMY_CYCLE_COUNTER.fetch_add(step, Ordering::Relaxed)
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

fn summarize_preview(preview: PayloadPreview) -> Option<HeaplessString<128>> {
    match preview {
        PayloadPreview::Empty => None,
        PayloadPreview::Utf8(text) => {
            let mut summary = HeaplessString::<128>::new();
            let _ = summary.push_str("utf8=\"");
            let mut written = 0usize;
            for ch in text.chars() {
                if written >= 48 {
                    let _ = summary.push_str("…");
                    break;
                }
                if summary.push(ch).is_err() {
                    break;
                }
                written += 1;
            }
            let _ = summary.push('"');
            Some(summary)
        }
        PayloadPreview::Hex(lines) => {
            let mut summary = HeaplessString::<128>::new();
            let first = lines.first().map(|line| line.as_str()).unwrap_or("");
            let _ = write!(summary, "hex-lines={} first={first}", lines.len());
            Some(summary)
        }
    }
}

fn log_bootstrap_payload(words: &[sel4_sys::seL4_Word]) {
    if words.is_empty() {
        return;
    }

    if !log::log_enabled!(log::Level::Debug) {
        return;
    }

    match preview_payload(words) {
        PayloadPreview::Empty => {}
        PayloadPreview::Utf8(text) => {
            log::debug!("[staged utf8] {}", text.as_str());
        }
        PayloadPreview::Hex(lines) => {
            if log::log_enabled!(log::Level::Trace) {
                for line in lines {
                    log::trace!("{}", line.as_str());
                }
            } else {
                let byte_len = words.len() * core::mem::size_of::<sel4_sys::seL4_Word>();
                log::debug!("[staged hex] {byte_len} bytes (hex dump suppressed)");
            }
        }
    }
}

const FAULT_TAG_NULL: u64 = 0;
const FAULT_TAG_CAP: u64 = 1;
const FAULT_TAG_UNKNOWN_SYSCALL: u64 = 2;
const FAULT_TAG_USER_EXCEPTION: u64 = 3;
const FAULT_TAG_DEBUG_EXCEPTION: u64 = 4;
const FAULT_TAG_VMFAULT: u64 = 5;
const FAULT_TAG_VGIC_MAINTENANCE: u64 = 6;
const FAULT_TAG_VCPU: u64 = 7;
const FAULT_TAG_VPPI: u64 = 8;
const FAULT_TAG_TIMEOUT: u64 = 9;
const CONTROL_LABEL_LOG_AND_BOOTSTRAP: u64 = 0;
const CONTROL_LABEL_HEARTBEAT: u64 = 0xB2;
const MAX_FAULT_REGS: usize = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FaultSource {
    badge: sel4_sys::seL4_Word,
    name: &'static str,
    tcb_cap: sel4_sys::seL4_CPtr,
    entry: Option<usize>,
    stack_range: Option<(usize, usize)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FaultContext {
    badge: sel4_sys::seL4_Word,
    fault_type: u64,
    ip: u64,
    sp: u64,
    regs: [u64; MAX_FAULT_REGS],
    len: usize,
}

struct StrayTracker {
    signatures: HeaplessVec<(sel4_sys::seL4_Word, u64), 32>,
    overflowed: bool,
}

impl StrayTracker {
    const fn new() -> Self {
        Self {
            signatures: HeaplessVec::new(),
            overflowed: false,
        }
    }

    fn first_observation(&mut self, badge: sel4_sys::seL4_Word, label: u64) -> bool {
        let signature = (badge, label);
        if self.signatures.contains(&signature) {
            return false;
        }

        if self.signatures.is_full() {
            self.overflowed = true;
            let _ = self.signatures.remove(0);
        }

        let _ = self.signatures.push(signature);

        true
    }

    fn overflowed(&self) -> bool {
        self.overflowed
    }
}

struct FaultRegistry {
    next_badge: AtomicU64,
    sources: Mutex<HeaplessVec<FaultSource, 8>>,
    fatal_badges: Mutex<HeaplessVec<sel4_sys::seL4_Word, 8>>,
    unknown_badges: Mutex<HeaplessVec<sel4_sys::seL4_Word, 8>>,
    stray_signatures: Mutex<StrayTracker>,
    suppressed_badges: Mutex<HeaplessVec<sel4_sys::seL4_Word, 8>>,
    tallies: Mutex<HeaplessVec<(sel4_sys::seL4_Word, u32), 8>>,
}

static FAULT_REGISTRY: FaultRegistry = FaultRegistry::new();
static STRAY_FAULT_WARNED: AtomicBool = AtomicBool::new(false);

impl FaultRegistry {
    const fn new() -> Self {
        Self {
            next_badge: AtomicU64::new(1),
            sources: Mutex::new(HeaplessVec::new()),
            fatal_badges: Mutex::new(HeaplessVec::new()),
            unknown_badges: Mutex::new(HeaplessVec::new()),
            stray_signatures: Mutex::new(StrayTracker::new()),
            suppressed_badges: Mutex::new(HeaplessVec::new()),
            tallies: Mutex::new(HeaplessVec::new()),
        }
    }

    fn alloc_badge(&self) -> sel4_sys::seL4_Word {
        let badge = self.next_badge.fetch_add(1, Ordering::Relaxed);
        if badge == 0 {
            return self.next_badge.fetch_add(1, Ordering::Relaxed);
        }
        badge
    }

    fn register_source(
        &self,
        badge: sel4_sys::seL4_Word,
        name: &'static str,
        tcb_cap: sel4_sys::seL4_CPtr,
        entry: Option<usize>,
        stack_range: Option<(usize, usize)>,
    ) -> FaultSource {
        assert!(badge != 0, "fault badge must be non-zero");
        let mut sources = self.sources.lock();
        if let Some(existing) = sources.iter_mut().find(|entry| entry.badge == badge) {
            existing.name = name;
            existing.tcb_cap = tcb_cap;
            existing.entry = entry;
            existing.stack_range = stack_range;
            *existing
        } else {
            let source = FaultSource {
                badge,
                name,
                tcb_cap,
                entry,
                stack_range,
            };
            let _ = sources.push(source);
            source
        }
    }

    fn lookup(&self, badge: sel4_sys::seL4_Word) -> Option<FaultSource> {
        self.sources
            .lock()
            .iter()
            .copied()
            .find(|source| source.badge == badge)
    }

    fn mark_fatal(&self, badge: sel4_sys::seL4_Word) {
        let mut fatal = self.fatal_badges.lock();
        if !fatal.contains(&badge) {
            let _ = fatal.push(badge);
        }
    }

    fn mark_unknown(&self, badge: sel4_sys::seL4_Word) {
        let mut unknown = self.unknown_badges.lock();
        if !unknown.contains(&badge) {
            let _ = unknown.push(badge);
        }
        self.mark_fatal(badge);
    }

    fn is_fatal(&self, badge: sel4_sys::seL4_Word) -> bool {
        self.fatal_badges.lock().contains(&badge)
    }

    fn is_unknown(&self, badge: sel4_sys::seL4_Word) -> bool {
        self.unknown_badges.lock().contains(&badge)
    }

    fn mark_stray(&self, badge: sel4_sys::seL4_Word, label: u64) -> bool {
        self.stray_signatures.lock().first_observation(badge, label)
    }

    fn stray_overflowed(&self) -> bool {
        self.stray_signatures.lock().overflowed()
    }

    fn mark_suppressed(&self, badge: sel4_sys::seL4_Word) -> bool {
        let mut suppressed = self.suppressed_badges.lock();
        if suppressed.contains(&badge) {
            return false;
        }
        let _ = suppressed.push(badge);
        true
    }

    fn record_occurrence(&self, badge: sel4_sys::seL4_Word) -> u32 {
        let mut tallies = self.tallies.lock();
        if let Some(entry) = tallies.iter_mut().find(|entry| entry.0 == badge) {
            entry.1 = entry.1.saturating_add(1);
            entry.1
        } else {
            let _ = tallies.push((badge, 1));
            1
        }
    }
}

fn alloc_fault_badge() -> sel4_sys::seL4_Word {
    FAULT_REGISTRY.alloc_badge()
}

fn register_fault_source(
    badge: sel4_sys::seL4_Word,
    name: &'static str,
    tcb_cap: sel4_sys::seL4_CPtr,
    entry: Option<usize>,
    stack_range: Option<(usize, usize)>,
) -> FaultSource {
    let source = FAULT_REGISTRY.register_source(badge, name, tcb_cap, entry, stack_range);
    if let Some((stack_base, stack_top)) = source.stack_range {
        log::info!(
            target: "root_task::kernel::fault",
            "[tcb] spawn {name}: badge=0x{badge:04x} tcb_cap=0x{tcb:04x} entry=0x{entry:016x} sp=0x{stack_base:016x}-0x{stack_top:016x}",
            badge = badge,
            tcb = source.tcb_cap,
            entry = source.entry.unwrap_or_default(),
            stack_base = stack_base,
            stack_top = stack_top,
            name = name,
        );
    } else {
        log::info!(
            target: "root_task::kernel::fault",
            "[tcb] spawn {name}: badge=0x{badge:04x} tcb_cap=0x{tcb:04x} entry={entry:#018x?} sp=<unknown>",
            badge = badge,
            tcb = source.tcb_cap,
            entry = source.entry,
            name = name,
        );
    }
    source
}

fn lookup_fault_source(badge: sel4_sys::seL4_Word) -> Option<FaultSource> {
    FAULT_REGISTRY.lookup(badge)
}

fn record_fault_occurrence(badge: sel4_sys::seL4_Word) -> u32 {
    FAULT_REGISTRY.record_occurrence(badge)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum EpMessageKind {
    Fault { length_valid: bool },
    BootstrapControl,
    LogControl,
    Control { label: u64 },
    Unknown { label: u64, length: usize },
}

fn mint_fault_cap_for_tcb(
    cspace: &mut CSpace,
    fault_ep_slot: sel4_sys::seL4_CPtr,
    badge: sel4_sys::seL4_Word,
) -> Result<sel4_sys::seL4_CPtr, sel4_sys::seL4_Error> {
    let slot = cspace.alloc_slot()?;
    let rights = crate::cspace::cap_rights_read_write_grant();
    let err = cspace.mint_here(slot, cspace.root(), fault_ep_slot, rights, badge);
    if err != sel4_sys::seL4_NoError {
        cspace.release_slot(slot);
        return Err(err);
    }

    log::info!(
        target: "root_task::kernel::fault",
        "[fault] minted badged fault cap slot=0x{slot:04x} ep=0x{ep:04x} badge=0x{badge:04x}",
        slot = slot,
        ep = fault_ep_slot,
        badge = badge,
    );

    Ok(slot)
}

fn install_fault_handler_for_tcb(
    cspace: &mut CSpace,
    tcb_slot: sel4_sys::seL4_CPtr,
    fault_ep_slot: sel4_sys::seL4_CPtr,
    guard_data: sel4_sys::seL4_Word,
    label: &'static str,
) -> Result<sel4_sys::seL4_Word, sel4_sys::seL4_Error> {
    let badge = alloc_fault_badge();
    let badged_cap = mint_fault_cap_for_tcb(cspace, fault_ep_slot, badge)?;
    let handler_err = unsafe {
        sel4_sys::seL4_TCB_SetFaultHandler(
            tcb_slot,
            badged_cap,
            cspace.root(),
            guard_data,
            sel4_sys::seL4_CapInitThreadVSpace,
            0,
        )
    };

    if handler_err != sel4_sys::seL4_NoError {
        cspace.release_slot(badged_cap);
        return Err(handler_err);
    }

    log::info!(
        target: "root_task::kernel::fault",
        "[fault] installed handler for {label}: tcb=0x{tcb:04x} badge=0x{badge:04x} fault_cap=0x{cap:04x}",
        tcb = tcb_slot,
        badge = badge,
        cap = badged_cap,
    );

    register_fault_source(badge, label, tcb_slot, None, None);
    Ok(badge)
}

fn fault_tag_name(tag: u64) -> &'static str {
    match tag {
        FAULT_TAG_NULL => "null",
        FAULT_TAG_CAP => "cap",
        FAULT_TAG_UNKNOWN_SYSCALL => "unknown-syscall",
        FAULT_TAG_USER_EXCEPTION => "user-exception",
        FAULT_TAG_VMFAULT => "vmfault",
        FAULT_TAG_DEBUG_EXCEPTION => "debug-exception",
        FAULT_TAG_VGIC_MAINTENANCE => "vgic-maintenance",
        FAULT_TAG_VCPU => "vcpu",
        FAULT_TAG_VPPI => "vppi",
        FAULT_TAG_TIMEOUT => "timeout",
        _ => "unknown",
    }
}

fn is_fault_label(label: u64) -> bool {
    matches!(
        label,
        FAULT_TAG_NULL
            | FAULT_TAG_CAP
            | FAULT_TAG_UNKNOWN_SYSCALL
            | FAULT_TAG_USER_EXCEPTION
            | FAULT_TAG_VMFAULT
            | FAULT_TAG_DEBUG_EXCEPTION
            | FAULT_TAG_VGIC_MAINTENANCE
            | FAULT_TAG_VCPU
            | FAULT_TAG_VPPI
            | FAULT_TAG_TIMEOUT
    )
}

fn fault_length_range(label: u64) -> Option<RangeInclusive<usize>> {
    match label {
        FAULT_TAG_NULL => Some(0..=0),
        FAULT_TAG_CAP => Some(2..=MAX_FAULT_REGS),
        FAULT_TAG_UNKNOWN_SYSCALL => Some(6..=MAX_FAULT_REGS),
        FAULT_TAG_USER_EXCEPTION => Some(6..=MAX_FAULT_REGS),
        FAULT_TAG_VMFAULT => Some(3..=MAX_FAULT_REGS),
        FAULT_TAG_DEBUG_EXCEPTION => Some(5..=MAX_FAULT_REGS),
        FAULT_TAG_VGIC_MAINTENANCE => Some(2..=MAX_FAULT_REGS),
        FAULT_TAG_VCPU => Some(5..=MAX_FAULT_REGS),
        FAULT_TAG_VPPI => Some(2..=MAX_FAULT_REGS),
        FAULT_TAG_TIMEOUT => Some(1..=MAX_FAULT_REGS),
        _ => None,
    }
}

fn fault_layout_valid(label: u64, length: usize) -> bool {
    fault_length_range(label)
        .map(|range| range.contains(&length))
        .unwrap_or(false)
}

fn classify_ep_message(
    info: &sel4_sys::seL4_MessageInfo,
    allow_fault_labels: bool,
) -> EpMessageKind {
    let label = info.label();
    let length = info.length() as usize;

    if allow_fault_labels && is_fault_label(label) {
        return EpMessageKind::Fault {
            length_valid: fault_layout_valid(label, length),
        };
    }

    match label {
        CONTROL_LABEL_LOG_AND_BOOTSTRAP => {
            if length == 0 {
                EpMessageKind::BootstrapControl
            } else {
                EpMessageKind::LogControl
            }
        }
        CONTROL_LABEL_HEARTBEAT => EpMessageKind::Control { label },
        _ => EpMessageKind::Unknown { label, length },
    }
}

fn decode_fault_context(
    info: &sel4_sys::seL4_MessageInfo,
    badge: sel4_sys::seL4_Word,
    source: Option<FaultSource>,
    count: u32,
) -> Option<FaultContext> {
    let fault_tag = info.label();
    let length = info.length() as usize;

    if !is_fault_label(fault_tag) || !fault_layout_valid(fault_tag, length) {
        return None;
    }

    let mut regs = [0u64; MAX_FAULT_REGS];
    let len = cmp::min(length, regs.len());
    for idx in 0..len {
        regs[idx] = unsafe { sel4_sys::seL4_GetMR(idx as i32) };
    }

    let mut ip = regs.first().copied().unwrap_or_default();
    let mut sp = regs.get(1).copied().unwrap_or_default();
    let tag_name = fault_tag_name(fault_tag);
    let source_desc = source
        .map(|src| format!("{} (tcb_cap=0x{:04x})", src.name, src.tcb_cap))
        .unwrap_or_else(|| format!("unregistered (badge=0x{badge:04x})"));

    match fault_tag {
        FAULT_TAG_UNKNOWN_SYSCALL => {
            ip = regs
                .get(sel4_sys::seL4_UnknownSyscall_FaultIP as usize)
                .copied()
                .unwrap_or_default();
            sp = regs
                .get(sel4_sys::seL4_UnknownSyscall_SP as usize)
                .copied()
                .unwrap_or_default();
            let lr = regs
                .get(sel4_sys::seL4_UnknownSyscall_LR as usize)
                .copied()
                .unwrap_or_default();
            let spsr = regs
                .get(sel4_sys::seL4_UnknownSyscall_SPSR as usize)
                .copied()
                .unwrap_or_default();
            let syscall = regs
                .get(sel4_sys::seL4_UnknownSyscall_Syscall as usize)
                .copied()
                .unwrap_or_default();
            log::error!(
                target: "root_task::kernel::fault",
                "[fault] {tag_name} badge=0x{badge:04x} ip=0x{ip:016x} sp=0x{sp:016x} lr=0x{lr:016x} spsr=0x{spsr:016x} syscall=0x{syscall:x} source={source_desc} count={count}",
                badge = badge,
                tag_name = tag_name,
                ip = ip,
                sp = sp,
                lr = lr,
                spsr = spsr,
                syscall = syscall,
                source_desc = source_desc,
                count = count,
            );
        }
        FAULT_TAG_USER_EXCEPTION => {
            ip = regs
                .get(sel4_sys::seL4_UserException_FaultIP as usize)
                .copied()
                .unwrap_or_default();
            sp = regs
                .get(sel4_sys::seL4_UserException_SP as usize)
                .copied()
                .unwrap_or_default();
            let spsr = regs
                .get(sel4_sys::seL4_UserException_SPSR as usize)
                .copied()
                .unwrap_or_default();
            let number = regs
                .get(sel4_sys::seL4_UserException_Number as usize)
                .copied()
                .unwrap_or_default();
            let code = regs
                .get(sel4_sys::seL4_UserException_Code as usize)
                .copied()
                .unwrap_or_default();
            log::error!(
                target: "root_task::kernel::fault",
                "[fault] {tag_name} badge=0x{badge:04x} ip=0x{ip:016x} stack=0x{sp:016x} spsr=0x{spsr:016x} number={number} code=0x{code:x} source={source_desc} count={count}",
                badge = badge,
                tag_name = tag_name,
                ip = ip,
                sp = sp,
                spsr = spsr,
                number = number,
                code = code,
                source_desc = source_desc,
                count = count,
            );
        }
        FAULT_TAG_VMFAULT => {
            ip = regs
                .get(sel4_sys::seL4_VMFault_IP as usize)
                .copied()
                .unwrap_or_default();
            let addr = regs
                .get(sel4_sys::seL4_VMFault_Addr as usize)
                .copied()
                .unwrap_or_default();
            let prefetch = regs
                .get(sel4_sys::seL4_VMFault_PrefetchFault as usize)
                .copied()
                .unwrap_or_default();
            let fsr = regs
                .get(sel4_sys::seL4_VMFault_FSR as usize)
                .copied()
                .unwrap_or_default();
            log::error!(
                target: "root_task::kernel::fault",
                "[fault] {tag_name} badge=0x{badge:04x} ip=0x{ip:016x} addr=0x{addr:016x} prefetch={prefetch} fsr=0x{fsr:08x} source={source_desc} count={count}",
                badge = badge,
                tag_name = tag_name,
                ip = ip,
                addr = addr,
                prefetch = prefetch,
                fsr = fsr,
                source_desc = source_desc,
                count = count,
            );
        }
        FAULT_TAG_CAP => {
            ip = regs
                .get(sel4_sys::seL4_CapFault_IP as usize)
                .copied()
                .unwrap_or_default();
            let addr = regs
                .get(sel4_sys::seL4_CapFault_Addr as usize)
                .copied()
                .unwrap_or_default();
            let in_recv = regs
                .get(sel4_sys::seL4_CapFault_InRecvPhase as usize)
                .copied()
                .unwrap_or_default();
            let lookup = regs
                .get(sel4_sys::seL4_CapFault_LookupFailureType as usize)
                .copied()
                .unwrap_or_default();
            log::error!(
                target: "root_task::kernel::fault",
                "[fault] {tag_name} badge=0x{badge:04x} ip=0x{ip:016x} addr=0x{addr:016x} in_recv={in_recv} lookup={lookup} regs={regs:?} source={source_desc} count={count}",
                badge = badge,
                tag_name = tag_name,
                ip = ip,
                addr = addr,
                lookup = lookup,
                in_recv = in_recv,
                regs = &regs[..len],
                source_desc = source_desc,
                count = count,
            );
        }
        _ => {
            log::error!(
                target: "root_task::kernel::fault",
                "[fault] {tag_name} badge=0x{badge:04x} ip=0x{ip:016x} sp=0x{sp:016x} regs={regs:?} source={source_desc} count={count}",
                badge = badge,
                tag_name = tag_name,
                ip = ip,
                sp = sp,
                regs = &regs[..len],
                source_desc = source_desc,
                count = count,
            );
        }
    }

    Some(FaultContext {
        badge,
        fault_type: fault_tag,
        ip,
        sp,
        regs,
        len,
    })
}

fn suspend_fault_source(source: &FaultSource, context: &FaultContext) {
    let result = unsafe { sel4_sys::seL4_TCB_Suspend(source.tcb_cap) };
    if result == sel4_sys::seL4_NoError {
        log::error!(
            target: "root_task::kernel::fault",
            "[fault] suspended TCB source={label} badge=0x{badge:04x} tcb=0x{tcb:04x} ip=0x{ip:016x} sp=0x{sp:016x}",
            label = source.name,
            badge = source.badge,
            tcb = source.tcb_cap,
            ip = context.ip,
            sp = context.sp,
        );
    } else {
        log::error!(
            target: "root_task::kernel::fault",
            "[fault] failed to suspend TCB source={label} badge=0x{badge:04x} tcb=0x{tcb:04x} err={result} ({name})",
            label = source.name,
            badge = source.badge,
            tcb = source.tcb_cap,
            result = result,
            name = error_name(result),
        );
    }
}

fn handle_fatal_fault(context: FaultContext, source: Option<FaultSource>) {
    if FAULT_REGISTRY.is_fatal(context.badge) {
        if FAULT_REGISTRY.mark_suppressed(context.badge) {
            log::warn!(
                target: "root_task::kernel::fault",
                "[fault] suppressing handling for previously-fatal sender badge=0x{badge:04x} ip=0x{ip:016x} sp=0x{sp:016x}",
                badge = context.badge,
                ip = context.ip,
                sp = context.sp,
            );
        }
        return;
    }

    let tag_name = fault_tag_name(context.fault_type);
    if let Some(source) = source {
        log::error!(
            target: "root_task::kernel::fault",
            "[fault] {label}: fatal fault badge=0x{badge:04x} type={tag_name} ip=0x{ip:016x} sp=0x{sp:016x}",
            label = source.name,
            badge = context.badge,
            tag_name = tag_name,
            ip = context.ip,
            sp = context.sp,
        );
        suspend_fault_source(&source, &context);
        FAULT_REGISTRY.mark_fatal(context.badge);
    } else {
        if !FAULT_REGISTRY.is_unknown(context.badge) {
            log::error!(
                target: "root_task::kernel::fault",
                "[fault] unknown source badge=0x{badge:04x} type={tag_name} ip=0x{ip:016x} sp=0x{sp:016x} regs={regs:?}",
                badge = context.badge,
                tag_name = tag_name,
                ip = context.ip,
                sp = context.sp,
                regs = &context.regs[..context.len],
            );
        } else {
            log::warn!(
                target: "root_task::kernel::fault",
                "[fault] previously-unknown fatal source badge=0x{badge:04x} ip=0x{ip:016x} sp=0x{sp:016x}",
                badge = context.badge,
                ip = context.ip,
                sp = context.sp,
            );
        }
        FAULT_REGISTRY.mark_unknown(context.badge);
    }
}

struct StagedMessage {
    badge: sel4_sys::seL4_Word,
    info: sel4_sys::seL4_MessageInfo,
    payload: HeaplessVec<sel4_sys::seL4_Word, { MAX_MESSAGE_WORDS }>,
}

impl StagedMessage {
    fn new(info: sel4_sys::seL4_MessageInfo, badge: sel4_sys::seL4_Word) -> Self {
        let payload = copy_message_words(info, |index| {
            let mr_index: i32 = index
                .try_into()
                .expect("message register index must fit in i32");
            unsafe { sel4_sys::seL4_GetMR(mr_index) }
        });
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

static BOOTSTRAP_STAGE_LOG_ONCE: AtomicBool = AtomicBool::new(false);
static BOOTSTRAP_DISPATCH_LOG_ONCE: AtomicBool = AtomicBool::new(false);
static BOOTSTRAP_DISPATCH_STREAM_SEEN: AtomicBool = AtomicBool::new(false);

pub(crate) struct KernelIpc {
    control_ep: ControlEndpoint,
    fault_endpoint: FaultEndpoint,
    staged_bootstrap: Option<StagedMessage>,
    staged_forwarded: bool,
    handlers_ready: bool,
    fault_loop_announced: bool,
    debug_uart_announced: bool,
    control_labels_logged: HeaplessVec<u64, 4>,
}

fn current_node_id() -> sel4_sys::seL4_NodeId {
    unsafe {
        sel4_sys::bootinfo
            .as_ref()
            .map_or(0, |bootinfo| (*bootinfo).nodeID)
    }
}

impl KernelIpc {
    pub(crate) fn new(control_ep: ControlEndpoint, fault_endpoint: FaultEndpoint) -> Self {
        log::info!(
            "[ipc] root EP installed at slot=0x{ep:04x} (role=LOG+CONTROL / QUEEN bootstrap)",
            ep = control_ep.raw()
        );
        log::info!(
            "[ipc] EP 0x{ep:04x} loop online; waiting for messages",
            ep = control_ep.raw()
        );
        if fault_endpoint.is_valid() {
            log::info!(
                "[ipc] fault EP installed at slot=0x{ep:04x} (dedicated fault handler)",
                ep = fault_endpoint.raw()
            );
        }
        let cpuid = current_node_id();
        log::info!(
            "[ipc] EP 0x{ep:04x}: dispatcher thread initialised on core={cpuid}",
            ep = control_ep.raw(),
            cpuid = cpuid,
        );
        Self {
            control_ep,
            fault_endpoint,
            staged_bootstrap: None,
            staged_forwarded: false,
            handlers_ready: false,
            fault_loop_announced: false,
            debug_uart_announced: false,
            control_labels_logged: HeaplessVec::new(),
        }
    }

    fn message_present(info: &sel4_sys::seL4_MessageInfo, badge: sel4_sys::seL4_Word) -> bool {
        badge != 0
            || info.length() != 0
            || info.label() != 0
            || info.extra_caps() != 0
            || info.caps_unwrapped() != 0
    }

    fn reply_empty() {
        sel4::reply(sel4_sys::seL4_MessageInfo::new(0, 0, 0, 0));
    }

    fn warn_fault_length(label: u64, len: usize, badge: sel4_sys::seL4_Word) {
        if FAULT_REGISTRY.mark_stray(badge, label) {
            log::warn!(
                target: "root_task::kernel::fault",
                "[fault] suspicious fault length badge=0x{badge:04x} label=0x{label:08x} len={len}",
                badge = badge,
                label = label,
                len = len,
            );
        }
    }

    fn warn_stray_fault_once() {
        if STRAY_FAULT_WARNED
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            log::warn!(
                target: "root_task::kernel::fault",
                "[fault] stray or non-fault messages observed on fault EP; suppressing repeated WARN logs"
            );
        }
    }

    fn log_stray_fault(
        &self,
        info: &sel4_sys::seL4_MessageInfo,
        badge: sel4_sys::seL4_Word,
        label: u64,
        length: usize,
    ) {
        let payload =
            copy_message_words(*info, |index| unsafe { sel4_sys::seL4_GetMR(index as i32) });
        let preview = preview_payload(payload.as_slice());
        let summary = summarize_preview(preview);
        let overflow = FAULT_REGISTRY.stray_overflowed();
        let source = lookup_fault_source(badge);

        match summary {
            Some(detail) => {
                match source {
                    Some(src) => log::warn!(
                        target: "root_task::kernel::fault",
                        "[fault] stray message on fault EP: badge=0x{badge:04x} label=0x{label:08x} len={len} payload={detail}{overflow} src={name} tcb=0x{tcb:04x}",
                        badge = badge,
                        label = label,
                        len = length,
                        detail = detail,
                        overflow = if overflow { " (tracker overflow)" } else { "" },
                        name = src.name,
                        tcb = src.tcb_cap,
                    ),
                    None => log::warn!(
                        target: "root_task::kernel::fault",
                        "[fault] stray message on fault EP: badge=0x{badge:04x} label=0x{label:08x} len={len} payload={detail}{overflow}",
                        badge = badge,
                        label = label,
                        len = length,
                        overflow = if overflow { " (tracker overflow)" } else { "" },
                    );
                }
            }
            None => {
                match source {
                    Some(src) => log::warn!(
                        target: "root_task::kernel::fault",
                        "[fault] stray message on fault EP: badge=0x{badge:04x} label=0x{label:08x} len={len}{overflow} src={name} tcb=0x{tcb:04x}",
                        badge = badge,
                        label = label,
                        len = length,
                        overflow = if overflow { " (tracker overflow)" } else { "" },
                        name = src.name,
                        tcb = src.tcb_cap,
                    ),
                    None => log::warn!(
                        target: "root_task::kernel::fault",
                        "[fault] stray message on fault EP: badge=0x{badge:04x} label=0x{label:08x} len={len}{overflow}",
                        badge = badge,
                        label = label,
                        len = length,
                        overflow = if overflow { " (tracker overflow)" } else { "" },
                    ),
                }
            }
        }
    }

    fn reply_line(bytes: &[u8]) {
        let word_bytes = core::mem::size_of::<sel4_sys::seL4_Word>();
        let mut words_written: usize = 0;
        for (index, chunk) in bytes.chunks(word_bytes).enumerate().take(MAX_MESSAGE_WORDS) {
            let mut buf = [0u8; core::mem::size_of::<sel4_sys::seL4_Word>()];
            buf[..chunk.len()].copy_from_slice(chunk);
            let value = sel4_sys::seL4_Word::from_le_bytes(buf);
            unsafe {
                sel4_sys::seL4_SetMR(index as i32, value);
            }
            words_written = words_written.saturating_add(1);
        }

        let info = sel4_sys::seL4_MessageInfo::new(0, 0, 0, words_written as sel4_sys::seL4_Word);
        sel4::reply(info);
    }

    fn reply_control_ack(&self, verb: &str, detail: Option<&str>) {
        let mut line: HeaplessString<{ crate::serial::DEFAULT_LINE_CAPACITY }> =
            HeaplessString::new();
        let ack = AckLine {
            status: AckStatus::Ok,
            verb,
            detail,
        };
        if render_ack(&mut line, &ack).is_err() {
            line.clear();
            let _ = line.push_str("OK ");
            let _ = line.push_str(verb);
        }
        let _ = line.push_str("\n");
        Self::reply_line(line.as_bytes());
        log::debug!(
            target: "root_task::kernel::fault",
            "[control] replied ack verb={verb} detail={detail:?} len={}",
            line.len()
        );
    }

    fn handle_unknown_fault_msg(
        badge: sel4_sys::seL4_Word,
        label: sel4_sys::seL4_Word,
        len: usize,
    ) {
        // HARD MUTE: temporarily ignore unknown control/fault messages to prevent log storms while
        // we debug other subsystems. Intentionally no logging here.
        let _ = (badge, label, len);
    }

    fn log_control_stream(&mut self, label: u64) {
        if self.control_labels_logged.iter().any(|seen| *seen == label) {
            return;
        }

        let _ = self.control_labels_logged.push(label);
        log::info!(
            "[ipc] EP 0x{ep:04x}: control stream active (label=0x{label:02X})",
            ep = self.control_ep.raw(),
            label = label,
        );
    }

    fn poll_fault_endpoint(&mut self, _now_ms: u64) {
        if !self.fault_endpoint.is_valid() {
            return;
        }

        loop {
            let mut badge: sel4_sys::seL4_Word = 0;
            let info = unsafe { sel4_sys::seL4_NBRecv(self.fault_endpoint.raw(), &mut badge) };
            if !Self::message_present(&info, badge) {
                return;
            }

            let label = info.label();
            let length = info.length() as usize;

            if badge == 0 || !is_fault_label(label) {
                Self::warn_stray_fault_once();
                if FAULT_REGISTRY.mark_stray(badge, label) {
                    self.log_stray_fault(&info, badge, label, length);
                }
                Self::reply_empty();
                continue;
            }

            if !fault_layout_valid(label, length) {
                if FAULT_REGISTRY.mark_stray(badge, label) {
                    log::warn!(
                        target: "root_task::kernel::fault",
                        "[fault] invalid fault layout on fault EP badge=0x{badge:04x} label=0x{label:08x} len={len}; dropping",
                        badge = badge,
                        label = label,
                        len = length,
                    );
                }
                continue;
            }

            if FAULT_REGISTRY.is_fatal(badge) {
                if FAULT_REGISTRY.mark_suppressed(badge) {
                    log::warn!(
                        target: "root_task::kernel::fault",
                        "[fault] ignoring message from known-fatal badge=0x{badge:04x}",
                        badge = badge,
                    );
                }
                continue;
            }
            let count = record_fault_occurrence(badge);
            let source = lookup_fault_source(badge);
            if let Some(context) = decode_fault_context(&info, badge, source, count) {
                handle_fatal_fault(context, source);
            }
        }
    }

    fn poll_endpoint(&mut self, now_ms: u64, bootstrap: bool) -> bool {
        if self.staged_bootstrap.is_some() {
            return true;
        }

        if !self.debug_uart_announced {
            debug_uart_str("[dbg] EP 0x0130: dispatcher loop about to recv\n");
            self.debug_uart_announced = true;
        }
        let mut badge: sel4_sys::seL4_Word = 0;
        let info = unsafe { sel4_sys::seL4_Poll(self.control_ep.raw(), &mut badge) };
        if !Self::message_present(&info, badge) {
            if bootstrap {
                log::trace!(
                    "[ipc] bootstrap poll idle ep=0x{ep:04x} now={now_ms} badge=0x{badge:016x}",
                    ep = self.control_ep.raw(),
                    now_ms = now_ms,
                    badge = badge,
                );
            }
            return false;
        }

        let msg_len = info.length();
        let kind = classify_ep_message(&info, false);
        if bootstrap {
            log::trace!(
                "B5.recv ret badge=0x{badge:016x} info=0x{info:08x} len={msg_len}",
                badge = badge,
                info = info.words[0]
            );
        } else if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "[ipc] poll ep=0x{ep:04x} badge=0x{badge:016x} info=0x{info:08x} now_ms={now_ms}",
                ep = self.control_ep.raw(),
                badge = badge,
                info = info.words[0],
                now_ms = now_ms,
            );
        }

        let staged = StagedMessage::new(info, badge);

        if bootstrap {
            let first_bootstrap = !BOOTSTRAP_DISPATCH_STREAM_SEEN.swap(true, Ordering::Relaxed);
            if first_bootstrap {
                log::info!(
                    "[ipc] bootstrap dispatch stream active on ep=0x{ep:04x} (label=0x{label:08x})",
                    ep = self.control_ep.raw(),
                    label = info.label(),
                );
            } else {
                log::debug!(
                    "[ipc] bootstrap dispatch ep=0x{ep:04x} label=0x{label:08x} len={msg_len}",
                    ep = self.control_ep.raw(),
                    label = info.label(),
                    msg_len = info.length(),
                );
            }
        }

        match kind {
            EpMessageKind::Fault { length_valid } => {
                if !length_valid {
                    Self::warn_fault_length(info.label(), info.length() as usize, badge);
                }
                if FAULT_REGISTRY.is_fatal(badge) {
                    if FAULT_REGISTRY.mark_suppressed(badge) {
                        log::warn!(
                            target: "root_task::kernel::fault",
                            "[fault] ignoring message from known-fatal badge=0x{badge:04x}",
                            badge = badge,
                        );
                    }
                    return true;
                }
                let count = record_fault_occurrence(badge);
                let source = lookup_fault_source(badge);
                if let Some(context) = decode_fault_context(&info, badge, source, count) {
                    handle_fatal_fault(context, source);
                }
                return true;
            }
            EpMessageKind::BootstrapControl | EpMessageKind::LogControl => {
                self.log_control_stream(info.label());
                if self.try_stage_bootstrap(&staged) {
                    self.staged_bootstrap = Some(staged);
                    self.staged_forwarded = false;
                    return true;
                }
                log::trace!(
                    "[ipc] control ep=0x{ep:04x} ignored message badge=0x{badge:016x} label=0x{label:08x} len={len}",
                    ep = self.control_ep.raw(),
                    badge = badge,
                    label = info.label(),
                    len = info.length(),
                );
            }
            EpMessageKind::Control { label } => {
                self.log_control_stream(label);
                #[cfg(feature = "control-trace")]
                log::trace!(
                    target: "root_task::kernel::fault",
                    "[ipc] control ep=0x{ep:04x} badge=0x{badge:016x} label=0x{label:08x} len={len}",
                    ep = self.control_ep.raw(),
                    badge = badge,
                    label = label,
                    len = info.length(),
                );
                self.reply_control_ack("AUTH", Some("control"));
                return true;
            }
            EpMessageKind::Unknown { label, length } => {
                Self::handle_unknown_fault_msg(badge, label as sel4_sys::seL4_Word, length);
                return false;
            }
        }

        false
    }

    fn try_stage_bootstrap(&self, message: &StagedMessage) -> bool {
        if is_fault_label(message.info.label()) {
            return false;
        }

        true
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
            // Periodic control-plane messages can flood dev-virt logs; log once at info
            // then demote subsequent events to debug.
            BOOTSTRAP_STAGE_LOG_ONCE.swap(true, Ordering::Relaxed);
            log::debug!(
                "[ipc] bootstrap staged ep=0x{ep:04x} badge=0x{badge:016x} info=0x{info:08x} words={words}",
                ep = self.control_ep.raw(),
                badge = message.badge,
                info = message.info.words[0],
                words = message.payload.len(),
            );
            log_bootstrap_payload(message.payload.as_slice());
        }
        log::debug!(
            "[ipc] staged → forwarded ep=0x{ep:04x} badge=0x{badge:016x}",
            ep = self.control_ep.raw(),
            badge = message.badge,
        );
        self.staged_forwarded = true;
    }
}

impl IpcDispatcher for KernelIpc {
    fn dispatch(&mut self, now_ms: u64) {
        if !self.fault_loop_announced {
            log::info!(
                "[fault] handler loop online; waiting for fault messages (ep=0x{ep:04x})",
                ep = if self.fault_endpoint.is_valid() {
                    self.fault_endpoint.raw()
                } else {
                    self.control_ep.raw()
                }
            );
            self.fault_loop_announced = true;
        }
        self.poll_fault_endpoint(now_ms);
        let _ = self.poll_endpoint(now_ms, false);
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

    fn bootstrap_poll(&mut self, now_ms: u64) -> bool {
        self.poll_fault_endpoint(now_ms);
        self.poll_endpoint(now_ms, true)
    }

    fn has_staged_bootstrap(&self) -> bool {
        self.staged_bootstrap.is_some()
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
        let log_once = !BOOTSTRAP_DISPATCH_LOG_ONCE.swap(true, Ordering::Relaxed);
        if log_once {
            audit.info(summary.as_str());
            log::debug!("[audit] {}", summary.as_str());
        } else {
            log::debug!("[audit] {}", summary.as_str());
        }

        if !log_once {
            log::debug!(
                "[audit] [ipc] bootstrap dispatch repeated; demoting to debug to prevent log spam"
            );
        }

        crate::bootstrap::log::process_ep_payload(message.payload.as_slice(), audit);

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
        bounded_message_words, copy_message_words, preview_payload, ControlEndpoint, FaultEndpoint,
        KernelIpc, PayloadPreview, StagedMessage, HEX_CHUNK_BYTES, MAX_HEX_LINES,
        MAX_MESSAGE_WORDS, MAX_PAYLOAD_LOG_BYTES,
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

        let mut ipc = KernelIpc::new(
            ControlEndpoint(0x200),
            FaultEndpoint(sel4_sys::seL4_CapNull),
        );
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
    fn stray_tracker_suppresses_duplicates_after_overflow() {
        let mut tracker = StrayTracker::new();

        for idx in 0..32u64 {
            assert!(tracker.first_observation(idx, idx));
        }

        assert!(!tracker.first_observation(1, 1));
        assert!(tracker.first_observation(0xDEAD, 0x0E));
        assert!(tracker.overflowed());
        assert!(!tracker.first_observation(0xDEAD, 0x0E));
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
