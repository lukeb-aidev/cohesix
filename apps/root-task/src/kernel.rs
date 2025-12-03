// Author: Lukas Bower
#![allow(dead_code)]
#![allow(unsafe_code)]

extern crate alloc;

use alloc::boxed::Box;
use core::cell::RefCell;
use core::cmp;
use core::convert::TryFrom;
use core::fmt::{self, Write};
use core::ops::RangeInclusive;
use core::panic::PanicInfo;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

#[cfg(feature = "timers-arch-counter")]
use core::arch::asm;

use cohesix_ticket::Role;

#[cfg(all(feature = "kernel", target_arch = "aarch64"))]
use crate::arch::aarch64::timer::timer_freq_hz;
use crate::boot::{bi_extra, ep, tcb, uart_pl011};
#[cfg(feature = "cap-probes")]
use crate::bootstrap::cspace::cspace_first_retypes;
use crate::bootstrap::cspace_sys;
use crate::bootstrap::{
    boot_tracer,
    cspace::{CSpaceCtx, CSpaceWindow, FirstRetypeResult},
    device_pt_pool, ensure_device_pt_pool, ipcbuf, log as boot_log, pick_untyped,
    retype::{retype_one, retype_selection},
    BootPhase, UntypedSelection,
};
use crate::console::Console;
use crate::cspace::tuples::assert_ipc_buffer_matches_bootinfo;
use crate::cspace::CSpace;
use crate::debug_uart::debug_uart_str;
use crate::event::{
    AuditSink, BootstrapMessage, BootstrapMessageHandler, IpcDispatcher, TickEvent, TicketTable,
    TimerSource,
};
use crate::guards;
use crate::hal::{HalError, Hardware, KernelHal};
#[cfg(feature = "net-console")]
use crate::net::{init_net_console, NetConsoleError, NetStack, CONSOLE_TCP_PORT};
#[cfg(feature = "kernel")]
use crate::ninedoor::NineDoorBridge;
use crate::platform::{Platform, SeL4Platform};
use crate::sel4;
#[cfg(feature = "cap-probes")]
use crate::sel4::first_regular_untyped;
use crate::sel4::{
    bootinfo_debug_dump, error_name, root_endpoint, BootInfo, BootInfoExt, BootInfoView,
    DevicePtPool, KernelEnv, RetypeKind, RetypeStatus, IPC_PAGE_BYTES, MSG_MAX_WORDS,
};
use crate::serial::{
    pl011::Pl011, SerialPort, DEFAULT_LINE_CAPACITY, DEFAULT_RX_CAPACITY, DEFAULT_TX_CAPACITY,
};
use crate::uart::pl011::{self as early_uart, PL011_PADDR};
use heapless::{String as HeaplessString, Vec as HeaplessVec};

const EARLY_DUMP_LIMIT: usize = 512;
const DEVICE_FRAME_BITS: usize = 12;

#[cfg(all(feature = "kernel", not(sel4_config_printing)))]
use sel4_panicking::{self, DebugSink};

fn debug_identify_boot_caps() {
    for slot in 0u64..16u64 {
        let ty = unsafe { sel4_sys::seL4_CapIdentify(slot) };
        log::info!("[identify] slot=0x{slot:04x} ty=0x{ty:08x}");
    }
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

#[derive(Debug, PartialEq, Eq)]
enum BootState {
    Cold = 0,
    Booting = 1,
    Booted = 2,
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
    /// Timer initialisation failed.
    TimerInit(TimerError),
}

impl fmt::Display for BootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyBooted => f.write_str("bootstrap already invoked"),
            Self::TimerInit(err) => write!(f, "timer init failed: {err}"),
        }
    }
}

impl From<TimerError> for BootError {
    fn from(value: TimerError) -> Self {
        Self::TimerInit(value)
    }
}

struct BootStateGuard {
    committed: bool,
}

static BOOT_STATE: AtomicU8 = AtomicU8::new(BootState::Cold as u8);

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

/// Aggregated bootstrap artefacts passed to userland for final bring-up.
pub struct BootContext {
    /// Bootinfo view captured during kernel bootstrap.
    pub bootinfo: BootInfoView,
    /// Feature flags summarising the current profile.
    pub features: BootFeatures,
    /// Root endpoint slot shared with userland subsystems.
    pub ep_slot: sel4_sys::seL4_CPtr,
    /// PL011 UART slot reserved for the serial console.
    pub uart_slot: Option<sel4_sys::seL4_CPtr>,
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
        match BOOT_STATE.compare_exchange(
            BootState::Cold as u8,
            BootState::Booting as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(Self { committed: false }),
            Err(state) if state == BootState::Booting as u8 || state == BootState::Booted as u8 => {
                log::error!("[boot] bootstrap called twice; refusing re-entry");
                Err(BootError::AlreadyBooted)
            }
            Err(_) => unreachable!("invalid bootstrap state transition"),
        }
    }

    fn commit(&mut self) {
        BOOT_STATE.store(BootState::Booted as u8, Ordering::Release);
        self.committed = true;
    }
}

impl Drop for BootStateGuard {
    fn drop(&mut self) {
        if !self.committed {
            BOOT_STATE.store(BootState::Cold as u8, Ordering::Release);
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
    boot_log::force_uart_line("[kernel:entry] root-task entry reached");
    log::info!("[kernel:entry] root-task entry reached");
    log::info!(target: "kernel", "[kernel] boot entrypoint: starting bootstrap");
    let ctx = match bootstrap(platform, bootinfo) {
        Ok(ctx) => ctx,
        Err(err) => {
            log::error!("[kernel:entry] bootstrap failed: {err}");
            boot_log::force_uart_line("[kernel:entry] bootstrap failed; parking thread");
            log::error!(
                "[kernel:entry] unable to construct BootContext; refusing to bypass userland handoff"
            );
            loop {
                unsafe { sel4_sys::seL4_Yield() };
            }
        }
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

    crate::alloc::init_heap();

    boot_log::init_logger_bootstrap_only();

    crate::sel4::log_sel4_type_sanity();

    let mut build_line = heapless::String::<192>::new();
    let mut feature_report = heapless::String::<96>::new();
    for (idx, (label, enabled)) in [
        ("kernel", cfg!(feature = "kernel")),
        ("bootstrap-trace", cfg!(feature = "bootstrap-trace")),
        ("serial-console", cfg!(feature = "serial-console")),
        ("net", cfg!(feature = "net")),
        ("net-console", cfg!(feature = "net-console")),
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
    boot_log::force_uart_line(build_line.as_str());
    log::info!("{}", build_line.as_str());

    let mut boot_guard = BootStateGuard::acquire()?;
    debug_assert_eq!(
        BOOT_STATE.load(Ordering::Acquire),
        BootState::Booting as u8,
        "bootstrap state drift",
    );
    crate::bp!("bootstrap.begin");
    boot_tracer().advance(BootPhase::Begin);

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
    if let Err(err) = crate::bootstrap::cspace::ensure_canonical_root_alias(bootinfo_ref) {
        panic!(
            "failed to mint canonical init CNode alias: {} ({})",
            err,
            error_name(err),
        );
    }
    let cspace_window = CSpaceWindow::from_bootinfo(&bootinfo_view);
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
    boot_tracer().advance(BootPhase::CSpaceInit);

    log::info!("[kernel:entry] about to log stage0 entry");
    console.writeln_prefixed("entered from seL4 (stage0)");
    console.writeln_prefixed("Cohesix boot: root-task online");

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

    console.writeln_prefixed("Cohesix v0 (AArch64/virt)");

    bootinfo_debug_dump(&bootinfo_view);
    let ipc_buffer_ptr = bootinfo_ref.ipc_buffer_ptr();
    if let Some(ptr) = ipc_buffer_ptr {
        let addr = ptr.as_ptr() as usize;
        assert_eq!(
            addr & (IPC_PAGE_BYTES - 1),
            0,
            "IPC buffer must be page-aligned",
        );
        unsafe {
            sel4_sys::seL4_SetIPCBuffer(ptr.as_ptr());
        }
        assert_ipc_buffer_matches_bootinfo(bootinfo_ref);
    }

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
    );
    let extra_bytes = bootinfo_view.extra();
    if !extra_bytes.is_empty() {
        console.writeln_prefixed("[boot] deferring DTB parse");
        boot_tracer().advance(BootPhase::DTBParseDeferred);
    }

    #[cfg(feature = "canonical_cspace")]
    {
        crate::bootstrap::retype::canonical_cspace_console(bootinfo_ref);
    }

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

    #[cfg_attr(feature = "bootstrap-minimal", allow(unused_variables))]
    let ipc_vaddr = ipc_buffer_ptr.map(|ptr| ptr.as_ptr() as usize);
    let ipc_frame = sel4_sys::seL4_CapInitThreadIPCBuffer;

    #[cfg(feature = "bootstrap-minimal")]
    {
        log::warn!(
            "[boot] bootstrap-minimal: skipping EP retype/PL011 map/TCB copy; entering console"
        );
        crate::boot::ep::publish_root_ep(sel4_sys::seL4_CapNull);
        console.writeln_prefixed("[boot] bootstrap-minimal: entering console");
        boot_guard.commit();
        boot_log::force_uart_line("[console] serial fallback ready");
        if !crate::ipc::ep_is_valid(crate::sel4::root_endpoint()) {
            boot_log::force_uart_line(
                "[console] IPC disabled (root ep = null); use local commands only",
            );
        }
        crate::bootstrap::run_minimal(bootinfo_ref);
        crate::userland::start_console_or_cohsh(platform);
    }

    let (ep_slot, boot_ep_ok) = match ep::bootstrap_ep(&bootinfo_view, &mut boot_cspace) {
        Ok(slot) => (slot, true),
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
                (root_endpoint(), false)
            }
        }
    };

    if !boot_ep_ok {
        log::warn!(
            "[boot] continuing with existing root endpoint=0x{slot:04x}",
            slot = ep_slot
        );
    }

    crate::trace::trace_ep(ep_slot);

    let mut ep_line = heapless::String::<96>::new();
    let _ = write!(
        ep_line,
        "[boot] root endpoint published ep=0x{ep:04x}",
        ep = ep_slot
    );
    console.writeln_prefixed(ep_line.as_str());

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
    let tcb_copy_slot = if let Some(ref info) = first_retypes {
        info.tcb_copy_slot
    } else {
        crate::bp!("tcb.copy.begin");
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
                if err == sel4_sys::seL4_IllegalOperation {
                    log::warn!(
                        "[boot] ipc buffer re-bind not accepted by kernel; using boot-provided mapping only: err={} ({})",
                        err,
                        error_name(err)
                    );
                    let fallback_view = kernel_env
                        .ipc_buffer_view()
                        .or_else(|| Some(kernel_env.record_boot_ipc_buffer(ipc_frame, ipc_vaddr)));
                    fallback_view
                } else {
                    panic!("ipc buffer install failed: {} ({})", code, error_name(err));
                }
            }
        };

        if let Some(view) = ipc_view {
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

    if ep_slot != sel4_sys::seL4_CapNull {
        let guard_bits =
            sel4::word_bits().saturating_sub(bootinfo_ref.init_cnode_bits() as sel4_sys::seL4_Word);
        let guard_data = sel4::cap_data_guard(0, guard_bits);
        let fault_handler_err = unsafe {
            sel4_sys::seL4_TCB_SetFaultHandler(
                sel4_sys::seL4_CapInitThreadTCB,
                ep_slot,
                bootinfo_ref.init_cnode_cap(),
                guard_data,
                sel4_sys::seL4_CapInitThreadVSpace,
                0,
            )
        };

        if fault_handler_err != sel4_sys::seL4_NoError {
            let mut line = heapless::String::<200>::new();
            let _ = write!(
                line,
                "[boot] failed to install fault handler: {} ({}) — continuing without custom fault handler",
                fault_handler_err as sel4_sys::seL4_Word,
                error_name(fault_handler_err)
            );
            console.writeln_prefixed(line.as_str());
        } else {
            log::info!(
                target: "root_task::bootstrap",
                "[boot] fault handler installed tcb_slot=0x{slot:04x} ep=0x{ep:04x}",
                slot = tcb_copy_slot,
                ep = ep_slot
            );
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

    boot_tracer().advance(BootPhase::UntypedEnumerate);
    let mut notification_selection =
        pick_untyped(bootinfo_ref, sel4_sys::seL4_NotificationBits as u8);

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
            panic!("retype plan failed: {}", error_name(err));
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
                            "retype status={err}({code}) raw.untyped=0x{ucap:08x} raw.paddr=0x{paddr:08x} raw.size_bits={usize_bits} raw.slot=0x{slot:04x} raw.offset={offset} raw.depth={depth} raw.root=0x{root:04x} raw.node_index=0x{node_index:04x} obj_type={otype} obj_size_bits={obj_bits}",
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

            log::error!(
                "[pl011] UART map failed with {label} ({code}); halting because device console is required",
                label = error_label,
                code = error_code,
            );
            panic!(
                "device mapping for PL011 failed: {} ({})",
                error_label, error_code
            );
        }
    };

    let uart_ptr = uart_region
        .as_ref()
        .map(|region| region.ptr())
        .unwrap_or_else(|| {
            core::ptr::NonNull::new(early_uart::PL011_VADDR as *mut u8)
                .expect("PL011 virtual address must be non-null")
        });

    let mut map_line = heapless::String::<128>::new();
    if uart_region.is_some() {
        let mapped_vaddr = uart_ptr.as_ptr() as usize;
        let _ = write!(
            map_line,
            "[vspace:map] pl011 paddr=0x{paddr:08x} -> vaddr=0x{vaddr:016x} attrs=UNCACHED OK",
            vaddr = mapped_vaddr,
            paddr = PL011_PADDR,
        );
    } else {
        let fallback_vaddr = uart_ptr.as_ptr() as usize;
        let _ = write!(
            map_line,
            "[vspace:map] pl011 mapping unavailable (err={label}); fallback vaddr=0x{vaddr:016x}",
            vaddr = fallback_vaddr,
            label = pl011_map_error.map(error_name).unwrap_or("unknown"),
        );
    }
    console.writeln_prefixed(map_line.as_str());

    if let Some(region) = uart_region.as_ref() {
        uart_pl011::publish_uart_slot(region.cap());
        early_uart::register_console_base(region.ptr().as_ptr() as usize);
    }

    let mut driver = Pl011::new(uart_ptr);
    driver.init();
    console.writeln_prefixed("[uart] init OK");
    if uart_region.is_some() {
        driver.write_str("[console] PL011 console online\n");
    }
    #[cfg(all(feature = "kernel", not(sel4_config_printing)))]
    {
        unsafe {
            EARLY_UART_SINK = DebugSink {
                context: uart_ptr.as_ptr().cast::<()>(),
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
    log::info!("[boot] after uart logger online");

    let uart_slot = uart_region.as_ref().map(|region| region.cap());

    #[cfg(feature = "debug-input")]
    {
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
    }

    #[cfg(not(feature = "debug-input"))]
    {
        let serial =
            SerialPort::<_, DEFAULT_RX_CAPACITY, DEFAULT_TX_CAPACITY, DEFAULT_LINE_CAPACITY>::new(
                driver,
            );

        #[cfg(all(feature = "net-console", feature = "kernel"))]
        let net_stack = {
            log::info!("[boot] net-console: probing virtio-net");
            log::info!("[net-console] init: enter");
            match init_net_console(&mut hal) {
                Ok(stack) => {
                    log::info!("[boot] net-console: init ok; handle registered");
                    log::info!(
                        "[net-console] init: success; tcp console will be available on port {CONSOLE_TCP_PORT}"
                    );
                    Some(stack)
                }
                Err(NetConsoleError::NoDevice) => {
                    log::error!(
                        "[boot] net-console: init failed: no virtio-net device; continuing WITHOUT TCP console"
                    );
                    None
                }
                Err(err) => {
                    log::error!(
                        "[boot] net-console: init failed: {:?}; continuing WITHOUT TCP console",
                        err
                    );
                    None
                }
            }
        };
        #[cfg(all(feature = "net-console", not(feature = "kernel")))]
        let (net_stack, _) = NetStack::new(Ipv4Address::new(10, 0, 0, 2));
        log::info!("[boot] net-console init complete; continuing with timers and IPC");
        log::info!(target: "root_task::kernel", "[boot] phase: TimersAndIPC.begin");
        let (timer, ipc) = run_timers_and_ipc_phase(ep_slot).map_err(|err| {
            log::error!(
                target: "root_task::kernel",
                "[boot] TimersAndIPC: failed during bootstrap: {:?}",
                err
            );
            err
        })?;

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
        boot_tracer().advance(BootPhase::DTBParseDone);

        crate::bp!("logger.switch.begin");
        if cfg!(feature = "dev-virt") {
            log::info!(
                target: "root_task::kernel",
                "[boot] logger.switch: EP disabled in dev-virt (UART-only)"
            );
        } else if let Err(err) = boot_log::switch_logger_to_userland() {
            log::error!("[boot] logger switch failed: {:?}", err);
            panic!("logger switch failed: {err:?}");
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
                let _ = write!(banner, "[net] virtio up mac={mac} ip={ip}/{prefix} gw={gw}");
            } else {
                let _ = write!(banner, "[net] virtio up mac={mac} ip={ip}/{prefix}");
            }
            console.writeln_prefixed(banner.as_str());
            let mut listen = heapless::String::<64>::new();
            let _ = write!(listen, "[console] tcp listen :{CONSOLE_TCP_PORT}");
            console.writeln_prefixed(listen.as_str());
        } else {
            log::warn!("[boot] net-console unavailable: virtio-net did not initialise");
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
        boot_guard.commit();
        boot_log::force_uart_line("[console] serial fallback ready");
        crate::bootstrap::run_minimal(bootinfo_ref);
        let features = BootFeatures {
            serial_console: cfg!(feature = "serial-console"),
            net: cfg!(feature = "net") && net_stack.is_some(),
            net_console: cfg!(feature = "net-console") && net_stack.is_some(),
        };

        #[cfg(feature = "net-console")]
        let ctx = BootContext {
            bootinfo: bootinfo_view,
            features,
            ep_slot,
            uart_slot,
            serial: RefCell::new(Some(serial)),
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
            features,
            ep_slot,
            uart_slot,
            serial: RefCell::new(Some(serial)),
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
    ep_slot: sel4_sys::seL4_CPtr,
) -> Result<(KernelTimer, KernelIpc), BootError> {
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
        ep = ep_slot
    );
    let ipc = KernelIpc::new(ep_slot);
    Ok((timer, ipc))
}

#[cfg(not(feature = "bypass-timers-ipc"))]
fn run_timers_and_ipc_phase(
    ep_slot: sel4_sys::seL4_CPtr,
) -> Result<(KernelTimer, KernelIpc), BootError> {
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
            ep = ep_slot
        );
        let ipc = KernelIpc::new(ep_slot);
        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: ipc.init.end ep=0x{ep:04x} staged={staged}",
            ep = ep_slot,
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
        let timer = KernelTimer::init(KERNEL_TIMER_PERIOD_MS).map_err(|err| {
            log::error!(
                target: "root_task::kernel",
                "[boot] TimersAndIPC: timers.init.failed: {:?}",
                err
            );
            err
        })?;
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
            ep = ep_slot
        );
        let ipc = KernelIpc::new(ep_slot);
        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: ipc.init.end ep=0x{ep:04x} staged={staged}",
            ep = ep_slot,
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum EpMessageKind {
    Fault { length_valid: bool },
    BootstrapControl,
    LogControl,
    Control { label: u64 },
    Unknown { label: u64, length: usize },
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

fn classify_ep_message(info: &sel4_sys::seL4_MessageInfo) -> EpMessageKind {
    let label = info.label();
    let length = info.length() as usize;

    if is_fault_label(label) {
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

fn log_fault_message(info: &sel4_sys::seL4_MessageInfo, badge: sel4_sys::seL4_Word) -> bool {
    let fault_tag = info.label();
    let length = info.length() as usize;

    let mut regs = [0u64; MAX_FAULT_REGS];
    let len = cmp::min(length, regs.len());
    for idx in 0..len {
        regs[idx] = unsafe { sel4_sys::seL4_GetMR(idx as i32) };
    }

    let decoded_tag = regs[0] & 0xf;
    if decoded_tag != fault_tag {
        static FAULT_TAG_MISMATCH_LOGGED: AtomicBool = AtomicBool::new(false);
        if !FAULT_TAG_MISMATCH_LOGGED.swap(true, Ordering::Relaxed) {
            log::error!(
                target: "root_task::kernel::fault",
                "[fault] tag mismatch badge=0x{badge:04x} label=0x{label:08x} decoded=0x{decoded:08x} regs={regs:?}",
                badge = badge,
                label = fault_tag,
                decoded = decoded_tag,
                regs = &regs[..len],
            );
        }
        return false;
    }

    let ip_hint = regs
        .get(5)
        .copied()
        .unwrap_or_else(|| regs.get(4).copied().unwrap_or(0));
    let sp_hint = regs.get(4).copied().unwrap_or_default();
    log::error!(
        target: "root_task::kernel::fault",
        "[fault] received fault: badge=0x{badge:04x} label=0x{label:08x} ip_hint=0x{ip:016x} sp_hint=0x{sp:016x} len={len}",
        badge = badge,
        label = fault_tag,
        ip = ip_hint,
        sp = sp_hint,
        len = len,
    );

    match decoded_tag {
        FAULT_TAG_UNKNOWN_SYSCALL => {
            let fault_ip = regs.get(5).copied().unwrap_or_default();
            let sp = regs.get(4).copied().unwrap_or_default();
            let lr = regs.get(3).copied().unwrap_or_default();
            let spsr = regs.get(2).copied().unwrap_or_default();
            let syscall = regs.get(1).copied().unwrap_or_default();
            log::error!(
                target: "root_task::kernel::fault",
                "[fault] unknown syscall badge=0x{badge:04x} ip=0x{fault_ip:016x} sp=0x{sp:016x} lr=0x{lr:016x} spsr=0x{spsr:016x} syscall=0x{syscall:x}",
                badge = badge,
                fault_ip = fault_ip,
                sp = sp,
                lr = lr,
                spsr = spsr,
                syscall = syscall,
            );
        }
        FAULT_TAG_USER_EXCEPTION => {
            let fault_ip = regs.get(5).copied().unwrap_or_default();
            let stack = regs.get(4).copied().unwrap_or_default();
            let spsr = regs.get(3).copied().unwrap_or_default();
            let number = regs.get(2).copied().unwrap_or_default();
            let code = regs.get(1).copied().unwrap_or_default();
            log::error!(
                target: "root_task::kernel::fault",
                "[fault] user exception badge=0x{badge:04x} ip=0x{fault_ip:016x} stack=0x{stack:016x} spsr=0x{spsr:016x} number={number} code=0x{code:x}",
                badge = badge,
                fault_ip = fault_ip,
                stack = stack,
                spsr = spsr,
                number = number,
                code = code,
            );
        }
        FAULT_TAG_VMFAULT => {
            let ip = regs.get(4).copied().unwrap_or_default();
            let addr = regs.get(3).copied().unwrap_or_default();
            let prefetch = regs.get(2).copied().unwrap_or_default();
            let fsr = regs.get(1).copied().unwrap_or_default();
            log::error!(
                target: "root_task::kernel::fault",
                "[fault] vmfault badge=0x{badge:04x} ip=0x{ip:016x} addr=0x{addr:016x} prefetch={prefetch} fsr=0x{fsr:08x}",
                badge = badge,
                ip = ip,
                addr = addr,
                prefetch = prefetch,
                fsr = fsr,
            );
        }
        FAULT_TAG_CAP => {
            log::error!(
                target: "root_task::kernel::fault",
                "[fault] cap fault badge=0x{badge:04x} regs={regs:?}",
                badge = badge,
                regs = &regs[..len],
            );
        }
        FAULT_TAG_NULL => {
            log::warn!(
                target: "root_task::kernel::fault",
                "[fault] null fault badge=0x{badge:04x} regs={regs:?}",
                badge = badge,
                regs = &regs[..len],
            );
        }
        _ => {
            log::error!(
                target: "root_task::kernel::fault",
                "[fault] unrecognised fault tag={decoded_tag} badge=0x{badge:04x} regs={regs:?}",
                decoded_tag = decoded_tag,
                badge = badge,
                regs = &regs[..len],
            );
        }
    }

    true
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
    endpoint: sel4_sys::seL4_CPtr,
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
    pub(crate) fn new(endpoint: sel4_sys::seL4_CPtr) -> Self {
        log::info!(
            "[ipc] root EP installed at slot=0x{ep:04x} (role=LOG+CONTROL / QUEEN bootstrap)",
            ep = endpoint
        );
        log::info!(
            "[ipc] EP 0x{ep:04x} loop online; waiting for messages",
            ep = endpoint
        );
        let cpuid = current_node_id();
        log::info!(
            "[ipc] EP 0x{ep:04x}: dispatcher thread initialised on core={cpuid}",
            ep = endpoint,
            cpuid = cpuid,
        );
        Self {
            endpoint,
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
            ep = self.endpoint,
            label = label,
        );
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
        let info = unsafe { sel4_sys::seL4_Poll(self.endpoint, &mut badge) };
        if !Self::message_present(&info, badge) {
            if bootstrap {
                log::trace!(
                    "[ipc] bootstrap poll idle ep=0x{ep:04x} now={now_ms} badge=0x{badge:016x}",
                    ep = self.endpoint,
                    now_ms = now_ms,
                    badge = badge,
                );
            }
            return false;
        }

        let msg_len = info.length();
        let kind = classify_ep_message(&info);
        if bootstrap {
            log::trace!(
                "B5.recv ret badge=0x{badge:016x} info=0x{info:08x} len={msg_len}",
                badge = badge,
                info = info.words[0]
            );
        } else if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "[ipc] poll ep=0x{ep:04x} badge=0x{badge:016x} info=0x{info:08x} now_ms={now_ms}",
                ep = self.endpoint,
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
                    ep = self.endpoint,
                    label = info.label(),
                );
            } else {
                log::debug!(
                    "[ipc] bootstrap dispatch ep=0x{ep:04x} label=0x{label:08x} len={msg_len}",
                    ep = self.endpoint,
                    label = info.label(),
                    msg_len = info.length(),
                );
            }
        }

        match kind {
            EpMessageKind::Fault { length_valid } => {
                if !length_valid {
                    static FAULT_LENGTH_WARNED: AtomicBool = AtomicBool::new(false);
                    if !FAULT_LENGTH_WARNED.swap(true, Ordering::Relaxed) {
                        log::warn!(
                            target: "root_task::kernel::fault",
                            "[fault] suspicious fault length badge=0x{badge:04x} label=0x{label:08x} len={len}",
                            badge = badge,
                            label = info.label(),
                            len = info.length(),
                        );
                    }
                }
                if log_fault_message(&info, badge) {
                    return true;
                }
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
                    ep = self.endpoint,
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
                    ep = self.endpoint,
                    badge = badge,
                    label = label,
                    len = info.length(),
                );
                return false;
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
                ep = self.endpoint,
                badge = message.badge,
                info = message.info.words[0],
                words = message.payload.len(),
            );
            log_bootstrap_payload(message.payload.as_slice());
        }
        log::debug!(
            "[ipc] staged → forwarded ep=0x{ep:04x} badge=0x{badge:016x}",
            ep = self.endpoint,
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
                ep = self.endpoint
            );
            self.fault_loop_announced = true;
        }
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
