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
use crate::event::{CapabilityValidator, EventPump, IpcDispatcher, TimerSource};
use crate::ipc;
use crate::platform::Platform;
use crate::sel4;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::serial::pl011::Pl011;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::uart::pl011::PL011_VADDR;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use core::ptr::NonNull;

/// Authoritative entrypoint for userland bring-up and runtime loops.
pub fn main<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize, P>(
    pump: EventPump<'a, D, T, I, V, RX, TX, LINE>,
    _platform: &P,
) -> !
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
    P: Platform,
{
    ::log::info!(
        "[userland] bringup starting (serial_console={} net={} net_console={})",
        cfg!(feature = "serial-console"),
        cfg!(feature = "net"),
        cfg!(feature = "net-console")
    );

    deferred_bringup();

    ::log::info!("[userland] starting root console and net loop");
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
    deferred_bringup(); // quick, non-blocking, then return
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

pub fn deferred_bringup() {
    let ep = sel4::root_endpoint();
    if !ipc::ep_is_valid(ep) {
        ::log::info!("[userland] skipping bringup: root endpoint is null");
        return;
    }

    #[cfg(all(feature = "serial-console", feature = "kernel"))]
    {
        if cfg!(feature = "net-console") {
            ::log::info!(
                "[userland] net console enabled; deferring to kernel event loop for bringup"
            );
            return;
        }

        if let Some(uart_slot) = uart_pl011::uart_slot() {
            ::log::info!("[userland] starting root console loop");

            if let Some(base) = NonNull::new(PL011_VADDR as *mut u8) {
                let driver = Pl011::new(base);
                let console = SerialConsole::new(driver);
                let mut console = CohesixConsole::with_console(console, ep, uart_slot);
                console.run();
            } else {
                ::log::warn!("[userland] PL011 base vaddr unavailable; console not started");
            }
        } else {
            ::log::warn!("[userland] uart slot unavailable; console not started");
        }
    }

    #[cfg(not(all(feature = "serial-console", feature = "kernel")))]
    {
        ::log::info!("[userland] bringup complete (console features disabled)");
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
