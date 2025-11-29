// Author: Lukas Bower

//! Minimal userland entrypoints exposed by the root task.
#![allow(unsafe_code)]

use core::fmt::Write;

#[cfg(not(target_arch = "aarch64"))]
use core::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "serial-console")]
use crate::boot::uart_pl011;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::console::CohesixConsole;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::console::Console as SerialConsole;
use crate::event::{
    AuditSink, BootstrapMessage, BootstrapMessageHandler, CapabilityValidator, EventPump,
    IpcDispatcher, TimerSource,
};
use crate::ipc;
use crate::kernel::BootContext;
use crate::platform::Platform;
use crate::sel4;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::serial::pl011::Pl011;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::uart::pl011::PL011_VADDR;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use core::ptr::NonNull;
use heapless::String as HeaplessString;

/// Authoritative entrypoint for userland bring-up and runtime loops.
pub fn main(ctx: &BootContext) -> ! {
    log::info!(
        target: "userland",
        "Cohesix userland starting: serial_console={}, net={}, net_console={}",
        ctx.features.serial_console,
        ctx.features.net,
        ctx.features.net_console,
    );

    log::info!(
        target: "userland",
        "Console config: serial_console={}, net_console={}",
        ctx.features.serial_console,
        ctx.features.net_console,
    );

    deferred_bringup(ctx);

    let serial = ctx
        .serial
        .borrow_mut()
        .take()
        .expect("serial port unavailable");
    let timer = ctx
        .timer
        .borrow_mut()
        .take()
        .expect("kernel timer unavailable");
    let ipc = ctx
        .ipc
        .borrow_mut()
        .take()
        .expect("kernel IPC dispatcher unavailable");
    let tickets = ctx
        .tickets
        .borrow_mut()
        .take()
        .expect("ticket table unavailable");

    let mut audit = LoggerAudit;

    #[cfg(feature = "kernel")]
    let mut bootstrap_ipc = UserlandBootstrapHandler;

    #[cfg(feature = "net-console")]
    let mut net_stack_handle = ctx.net_stack.borrow_mut().take();

    let mut pump = EventPump::new(serial, timer, ipc, tickets, &mut audit);

    #[cfg(feature = "kernel")]
    {
        pump = pump.with_bootstrap_handler(&mut bootstrap_ipc);
    }

    #[cfg(feature = "kernel")]
    {
        if let Some(ninedoor) = ctx.ninedoor.borrow_mut().take() {
            pump = pump.with_ninedoor(ninedoor);
        }
        pump.announce_console_ready();
    }

    #[cfg(feature = "net-console")]
    {
        if let Some(net_stack) = net_stack_handle.as_mut() {
            pump = pump.with_network(net_stack);
        }
    }

    #[cfg(feature = "kernel")]
    {
        pump.start_cli();
        pump.emit_console_line("Cohesix console ready");
        log::info!(
            target: "userland",
            "Root shell: Cohesix console online on PL011",
        );
    }

    log::info!(target: "userland", "Cohesix userland entering main event loop");
    run_event_loop(pump);
}

/// Start the userland console or Cohesix shell over the serial transport.
#[allow(clippy::module_name_repetitions)]
pub fn start_console_or_cohsh<P: Platform>(platform: &P) -> ! {
    ::log::info!(
        "[userland] serial-console enabled: {}",
        cfg!(feature = "serial-console")
    );
    ::log::info!(
        "[userland] net-console enabled: {}",
        cfg!(feature = "net-console")
    );
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
            if let Some(base) = NonNull::new(PL011_VADDR as *mut u8) {
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

// ---- Deferred bring-up: defers to the kernel event loop when the TCP console
// is enabled, otherwise starts a standalone serial console.

pub fn deferred_bringup(ctx: &BootContext) {
    let ep = sel4::root_endpoint();
    if !ipc::ep_is_valid(ep) {
        log::info!(target: "userland", "[userland] skipping bringup: root endpoint is null");
        return;
    }

    #[cfg(all(feature = "serial-console", feature = "kernel"))]
    {
        if let Some(uart_slot) = ctx.uart_slot {
            log::info!(
                target: "userland",
                "[userland] deferred bringup ready (ep=0x{ep:04x} uart=0x{uart:04x})",
                ep = ep,
                uart = uart_slot,
            );
        } else {
            log::warn!(target: "userland", "[userland] uart slot unavailable during bringup");
        }
    }
}

fn run_event_loop<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    mut pump: EventPump<'a, D, T, I, V, RX, TX, LINE>,
) -> !
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    loop {
        pump.poll();
        sel4::yield_now();
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

struct UserlandBootstrapHandler;

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
        audit.info(summary.as_str());
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
