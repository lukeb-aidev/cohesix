// Author: Lukas Bower
// Purpose: Userland hand-off and runtime wiring for console and networking surfaces.
//! Minimal userland entrypoints exposed by the root task.
#![allow(unsafe_code)]

use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(target_arch = "aarch64"))]
use core::sync::atomic::AtomicU64;

#[cfg(feature = "serial-console")]
use crate::boot::uart_pl011;
use crate::bootstrap::log as boot_log;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::console::CohesixConsole;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::console::Console as SerialConsole;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::debug_uart::debug_uart_str;
use crate::event::{
    AuditSink, BootstrapMessage, BootstrapMessageHandler, CapabilityValidator, EventPump,
    IpcDispatcher, TimerSource,
};
use crate::ipc;
use crate::kernel::BootContext;
#[cfg(feature = "net-console")]
use crate::net::DefaultNetStack as NetStack;
#[cfg(feature = "net-console")]
use crate::net::NetPoller;
use crate::platform::Platform;
use crate::profile;
use crate::sel4;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::serial::pl011::{Pl011, Pl011Mmio};
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::uart::pl011;
use heapless::String as HeaplessString;

#[cfg(feature = "net-console")]
type NetStackHandle = NetStack;
#[cfg(not(feature = "net-console"))]
type NetStackHandle = ();

/// Authoritative entrypoint for userland bring-up and runtime loops. Full boots
/// must always flow through this handoff so the serial root console comes up;
/// bootstrap-minimal remains a specialised debug mode only.
pub fn main(ctx: BootContext) -> ! {
    log::info!(
        target: "userland",
        "[userland] main: entered (serial_console={}, net={}, net_console={})",
        ctx.features.serial_console,
        ctx.features.net,
        ctx.features.net_console
    );
    boot_log::force_uart_line("[mark] bootstrap.runtime.enter");

    #[cfg(all(feature = "serial-console", feature = "kernel"))]
    let uart_base = ctx.uart_mmio.as_ref().map(Pl011Mmio::vaddr);

    let mut audit = LoggerAudit;
    let serial = ctx
        .serial
        .borrow_mut()
        .take()
        .expect("serial driver missing from BootContext");
    let timer = ctx
        .timer
        .borrow_mut()
        .take()
        .expect("timer missing from BootContext");
    let ipc = ctx
        .ipc
        .borrow_mut()
        .take()
        .expect("ipc dispatcher missing from BootContext");
    let tickets = ctx
        .tickets
        .borrow_mut()
        .take()
        .expect("ticket table missing from BootContext");
    let mut bootstrap_ipc = kernel_bootstrap_handler();

    #[cfg(feature = "net-console")]
    let mut net_stack = take_net_stack(&ctx);

    log::info!(
        target: "userland",
        "[userland] event-pump: building console runtime (serial + timer + ipc)"
    );
    // The event pump is the single source of truth for console I/O so the
    // PL011 UART and TCP transports both feed the same CLI engine.
    let mut pump = EventPump::new(serial, timer, ipc, tickets, &mut audit);
    log::info!(
        target: "userland",
        "[userland] event-pump: registering serial root console"
    );
    #[cfg(all(feature = "serial-console", feature = "kernel"))]
    debug_uart_str("[dbg] console: spawning root console task\n");
    #[cfg(all(feature = "serial-console", feature = "kernel"))]
    log::info!("[console] spawn: starting root console task on serial");
    pump = attach_kernel_console(pump, &ctx, bootstrap_ipc.as_mut());
    pump = attach_ninedoor_bridge(pump, &ctx);

    #[cfg(feature = "net-console")]
    {
        log::info!(
            target: "userland",
            "[userland] event-pump: attaching network (stack_available={})",
            net_stack.is_some()
        );
        // The TCP root console shares the serial CLI and follows cohsh's
        // transport handshake so clients see identical prompts and banners.
        log::info!(
            target: "net-console",
            "[net-console] starting TCP console listener on port {} (net={}, net_console={})",
            crate::net::CONSOLE_TCP_PORT,
            ctx.features.net,
            ctx.features.net_console
        );
        pump = attach_network(pump, net_stack.as_mut());
        if pump.net_console_enabled() {
            log::info!(
                target: "net-console",
                "[net-console] listening on 0.0.0.0:{}",
                crate::net::CONSOLE_TCP_PORT
            );
        }
    }

    #[cfg(all(feature = "serial-console", feature = "kernel"))]
    {
        log::info!(
            target: "root_task::kernel",
            "[boot] phase: RootShell.begin (uart_slot_present={}, uart_vaddr_present={})",
            ctx.uart_slot.is_some(),
            uart_base.is_some(),
        );
        if ctx.uart_slot.is_none() || uart_base.is_none() {
            log::warn!(
                target: "userland",
                "[userland] PL011 mapping unavailable; continuing with serial console anyway"
            );
        }
        log::info!(
            target: "userland",
            "[userland] event-pump: mapping PL011 for shared console I/O"
        );
        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: root-console.start.begin"
        );
        log::info!(target: "boot", "[boot] before starting root shell");
        pump.announce_console_ready();
        log::info!(target: "boot", "[boot] root shell starting");
        log::info!(target: "console", "[console] starting root CLI");
        pump.start_cli();
        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: root-console.start.ok"
        );
        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: queen.start.begin"
        );
        log::info!(target: "boot", "[boot] root shell started; entering event loop");
        log::info!(
            target: "root_task::kernel",
            "[boot] TimersAndIPC: queen.start.ok"
        );
        log::info!(target: "root_task::kernel", "[boot] phase: TimersAndIPC.end");
        boot_log::allow_ep_only_transport();
        pump.run();
    }

    #[cfg(not(all(feature = "serial-console", feature = "kernel")))]
    #[allow(clippy::diverging_sub_expression)]
    {
        boot_log::allow_ep_only_transport();
        pump.run();
    }
}

/// Start the userland console or Cohesix shell over the serial transport.
#[allow(clippy::module_name_repetitions)]
pub fn start_console_or_cohsh<P: Platform>(platform: &P) -> ! {
    ::log::info!(
        "[userland] serial-console enabled: {}",
        profile::SERIAL_CONSOLE
    );
    ::log::info!("[userland] net-console enabled: {}", profile::NET_CONSOLE);
    serial_console::banner(platform);
    serial_console::run(platform)
}

/// Serial console fallback presented during early bring-up.
pub mod serial_console {
    use super::*;

    const HEARTBEAT_MS: u64 = 1_000;
    const PROMPT_REFRESH_HEARTBEATS: u64 = 10;

    struct PlatformWriter<'a, P: Platform> {
        platform: &'a P,
    }

    impl<'a, P: Platform> core::fmt::Write for PlatformWriter<'a, P> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for byte in s.as_bytes() {
                self.platform.putc(*byte);
            }
            Ok(())
        }
    }

    fn emit_prompt<P: Platform>(writer: &mut PlatformWriter<'_, P>) {
        let _ = write!(writer, "\r\n> ");
    }

    pub fn banner<P: Platform>(platform: &P) {
        let mut writer = PlatformWriter { platform };
        let _ = writeln!(writer);
        let _ = writeln!(writer, "[Cohesix] Root console ready. Type 'help'.");
        let ep = sel4::root_endpoint();
        if !ipc::ep_is_valid(ep) {
            let _ = writeln!(
                writer,
                "[console] IPC disabled (root ep = null); use local commands only"
            );
        } else {
            let _ = writeln!(
                writer,
                "[console] IPC enabled (root ep = 0x{ep:04x})",
                ep = ep
            );
        }
        let _ = write!(writer, "> ");
    }

    /// Run a minimal interactive loop that echoes input and keeps the prompt alive.
    pub fn run<P: Platform>(platform: &P) -> ! {
        #[cfg(all(feature = "kernel", feature = "serial-console"))]
        if let Some(uart_slot) = uart_pl011::uart_slot() {
            ::log::info!("[userland] starting PL011 root console bringup");
            let ep = sel4::root_endpoint();
            if let Some(base) = pl011::console_base() {
                let driver = Pl011::new(base);
                let console = SerialConsole::new(driver);
                let mut console = CohesixConsole::with_console(console, ep, uart_slot);
                console.run();
            }
            ::log::info!(
                "[userland] PL011 root console bringup done (this log should only appear if run() returns)"
            );
        }

        let mut writer = PlatformWriter { platform };

        let counter_frequency = counter_frequency();
        let mut last_heartbeat_tick = monotonic_ticks();
        let mut heartbeat_count: u64 = 0;

        loop {
            if let Some(byte) = platform.getc_nonblock() {
                heartbeat_count = 0;
                last_heartbeat_tick = monotonic_ticks();
                platform.putc(byte);
                if byte == b'\r' || byte == b'\n' {
                    emit_prompt(&mut writer);
                }
                continue;
            }

            sel4::yield_now();

            let now = monotonic_ticks();
            let elapsed_ticks = now.wrapping_sub(last_heartbeat_tick);
            if ticks_to_ms(elapsed_ticks, counter_frequency) < HEARTBEAT_MS {
                continue;
            }

            last_heartbeat_tick = now;
            heartbeat_count = heartbeat_count.wrapping_add(1);
            let _ = write!(writer, ".");
            if heartbeat_count % PROMPT_REFRESH_HEARTBEATS == 0 {
                emit_prompt(&mut writer);
            }
        }
    }
}

struct LoggerAudit;

impl AuditSink for LoggerAudit {
    fn info(&mut self, message: &str) {
        log::info!(target: "audit", "{message}");
    }

    fn denied(&mut self, message: &str) {
        log::warn!(target: "audit", "{message}");
    }
}

#[cfg(feature = "kernel")]
fn kernel_bootstrap_handler() -> Option<UserlandBootstrapHandler> {
    Some(UserlandBootstrapHandler)
}

#[cfg(not(feature = "kernel"))]
fn kernel_bootstrap_handler() -> Option<UserlandBootstrapHandler> {
    None
}

#[cfg(feature = "net-console")]
fn take_net_stack(ctx: &BootContext) -> Option<NetStackHandle> {
    ctx.net_stack.borrow_mut().take()
}

#[cfg(not(feature = "net-console"))]
fn take_net_stack(_ctx: &BootContext) -> Option<NetStackHandle> {
    None
}

#[cfg(feature = "kernel")]
fn attach_kernel_console<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    mut pump: EventPump<'a, D, T, I, V, RX, TX, LINE>,
    ctx: &BootContext,
    bootstrap_ipc: Option<&'a mut UserlandBootstrapHandler>,
) -> EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    if let Some(handler) = bootstrap_ipc {
        pump = pump.with_console_context(ctx.bootinfo, ctx.endpoints.control.raw(), ctx.uart_slot);
        pump = pump.with_bootstrap_handler(handler);
    }

    pump
}

#[cfg(not(feature = "kernel"))]
fn attach_kernel_console<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: EventPump<'a, D, T, I, V, RX, TX, LINE>,
    _ctx: &BootContext,
    _bootstrap_ipc: Option<&'a mut UserlandBootstrapHandler>,
) -> EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    pump
}

#[cfg(feature = "kernel")]
fn attach_ninedoor_bridge<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    mut pump: EventPump<'a, D, T, I, V, RX, TX, LINE>,
    ctx: &BootContext,
) -> EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    if let Some(ninedoor) = ctx.ninedoor.borrow_mut().take() {
        pump = pump.with_ninedoor(ninedoor);
    }

    pump
}

#[cfg(not(feature = "kernel"))]
fn attach_ninedoor_bridge<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: EventPump<'a, D, T, I, V, RX, TX, LINE>,
    _ctx: &BootContext,
) -> EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    pump
}

#[cfg(feature = "net-console")]
fn attach_network<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    mut pump: EventPump<'a, D, T, I, V, RX, TX, LINE>,
    net_stack_handle: Option<&'a mut NetStackHandle>,
) -> EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    if let Some(net_stack) = net_stack_handle {
        pump = pump.with_network(net_stack);
    }

    pump
}

#[cfg(not(feature = "net-console"))]
fn attach_network<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: EventPump<'a, D, T, I, V, RX, TX, LINE>,
    _net_stack_handle: Option<&'a mut NetStackHandle>,
) -> EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    pump
}

#[cfg(feature = "kernel")]
fn announce_console_ready<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    pump.announce_console_ready();
}

#[cfg(not(feature = "kernel"))]
fn announce_console_ready<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    _pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
}

#[cfg(feature = "kernel")]
fn start_kernel_cli<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    log::info!("[console] spawn: root console task requested (start_cli)");
    pump.start_cli();
    log::info!(
        target: "userland",
        "Root shell: Cohesix console online on PL011",
    );
}

#[cfg(not(feature = "kernel"))]
fn start_kernel_cli<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    _pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
}

struct UserlandBootstrapHandler;

static USERLAND_BOOTSTRAP_ONCE: AtomicBool = AtomicBool::new(false);

impl BootstrapMessageHandler for UserlandBootstrapHandler {
    fn handle(&mut self, message: &BootstrapMessage, audit: &mut dyn AuditSink) {
        let mut summary = HeaplessString::<128>::new();
        let _ = write!(
            summary,
            "[ipc] bootstrap dispatch badge=0x{badge:016x} label=0x{label:08x} words={words}",
            badge = message.badge,
            label = message.info.words[0],
            words = message.payload.len(),
        );
        let log_once = !USERLAND_BOOTSTRAP_ONCE.swap(true, Ordering::Relaxed);
        if log_once {
            audit.info(summary.as_str());
            log::debug!("[audit] {}", summary.as_str());
        } else {
            log::debug!("[audit] {}", summary.as_str());
        }
        crate::bootstrap::log::process_ep_payload(message.payload.as_slice(), audit);
    }
}

#[inline]
fn ticks_to_ms(delta: u64, freq: u64) -> u64 {
    if freq == 0 {
        return 0;
    }
    ((delta as u128) * 1_000u128 / freq as u128) as u64
}

#[inline]
fn monotonic_ticks() -> u64 {
    #[cfg(target_arch = "aarch64")]
    {
        read_cntpct()
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }
}

#[inline]
fn counter_frequency() -> u64 {
    #[cfg(target_arch = "aarch64")]
    {
        read_cntfrq()
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        1
    }
}

#[cfg(target_arch = "aarch64")]
#[inline]
fn read_cntpct() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mrs {value}, cntpct_el0", value = out(reg) value);
    }
    value
}

#[cfg(target_arch = "aarch64")]
#[inline]
fn read_cntfrq() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mrs {value}, cntfrq_el0", value = out(reg) value);
    }
    value
}
