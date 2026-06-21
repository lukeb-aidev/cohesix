// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Userland hand-off and runtime wiring for console and networking surfaces.
// Author: Lukas Bower
//! Minimal userland entrypoints exposed by the root task.
#![allow(unsafe_code)]

use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(target_arch = "aarch64"))]
use core::sync::atomic::AtomicU64;

#[cfg(feature = "kernel")]
use crate::affinity;
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
#[cfg(all(feature = "kernel", feature = "net-console"))]
use crate::hal::KernelHal;
#[cfg(feature = "kernel")]
use crate::hal::KernelWifiDebugHandle;
#[cfg(not(feature = "kernel"))]
type KernelWifiDebugHandle = ();
use crate::ipc;
use crate::kernel::BootContext;
#[cfg(feature = "kernel")]
use crate::lifecycle;
#[cfg(feature = "kernel")]
use crate::log_buffer;
#[cfg(feature = "net-console")]
use crate::net::DefaultNetStack as NetStack;
#[cfg(all(feature = "kernel", feature = "net-console"))]
use crate::net::NetPoller;
use crate::platform::Platform;
use crate::profile;
use crate::sel4;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::serial::pl011::Pl011;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::uart::pl011;
use heapless::String as HeaplessString;

#[cfg(feature = "net-console")]
type NetStackHandle = NetStack;
#[cfg(not(feature = "net-console"))]
type NetStackHandle = ();

#[cfg(all(feature = "serial-console", feature = "kernel"))]
fn serial_console_uart_status(
    slot_present: bool,
    vaddr_present: bool,
    physical_driver_task_serial: bool,
) -> &'static str {
    if !vaddr_present {
        "unavailable"
    } else if physical_driver_task_serial && !slot_present {
        "driver-task-runtime"
    } else if !slot_present {
        "slot-missing"
    } else {
        "root-mapped"
    }
}

/// Authoritative entrypoint for userland bring-up and runtime loops. Full boots
/// must always flow through this handoff so pre-root network gates and the
/// serial root console are ordered consistently; bootstrap-minimal remains a
/// specialised debug mode only.
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
    let uart_base = ctx.uart_mmio.as_ref().map(|mmio| mmio.vaddr());
    #[cfg(all(feature = "serial-console", feature = "kernel"))]
    let uart_backend = ctx
        .uart_mmio
        .as_ref()
        .map(|mmio| mmio.label())
        .unwrap_or("none");

    let mut audit = LoggerAudit;
    #[cfg(feature = "kernel")]
    {
        let now_ms = crate::hal::timebase().now_ms();
        let _ = lifecycle::init(now_ms);
    }
    let serial = ctx.serial.borrow_mut().take().unwrap_or_else(|| {
        log::warn!(
            target: "userland",
            "[userland] serial driver missing from BootContext; using no-op serial backend"
        );
        crate::serial::SerialPort::new(crate::serial::kernel_uart::KernelSerialDriver::null())
    });
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
    #[cfg(feature = "kernel")]
    let mut wifi_debug = KernelWifiDebugHandle::from_ptr(ctx.wifi_debug_hal_ptr);
    #[cfg(not(feature = "kernel"))]
    let mut wifi_debug: Option<KernelWifiDebugHandle> = None;

    #[cfg(feature = "net-console")]
    let mut net_stack = take_net_stack(&ctx);
    #[cfg(feature = "net-console")]
    let mut net_unavailable_detail = take_net_unavailable_detail(&ctx);
    #[cfg(feature = "net-console")]
    let mut net_deferred_config = take_net_deferred_config(&ctx);
    log::info!(
        target: "userland",
        "[userland] event-pump: building console runtime (serial + timer + ipc)"
    );
    // The event pump is the single source of truth for console I/O so UART and
    // TCP transports both feed the same CLI engine.
    let mut pump = EventPump::new(serial, timer, ipc, tickets, &mut audit);
    log::info!(
        target: "userland",
        "[userland] event-pump: registering serial root console"
    );
    #[cfg(all(feature = "serial-console", feature = "kernel"))]
    debug_uart_str("[dbg] console: spawning root console task\n");
    #[cfg(all(feature = "serial-console", feature = "kernel"))]
    log::info!("[console] spawn: starting root console task on serial");
    pump = attach_kernel_console(pump, &ctx, bootstrap_ipc.as_mut(), None);
    pump = attach_local_seat(pump, &ctx);
    pump = attach_ninedoor_bridge(pump, &ctx);
    #[cfg(feature = "kernel")]
    crate::hal::driver_task::emit_boot_contract_proof();

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
        log::info!(
            target: "net-console",
            "[net-console] attach before root console start"
        );
    }

    #[cfg(all(feature = "serial-console", feature = "kernel"))]
    {
        log::info!(
            target: "root_task::kernel",
            "[boot] phase: RootShell.begin (uart_slot_present={}, uart_vaddr_present={}, uart_backend={})",
            ctx.uart_slot.is_some(),
            uart_base.is_some(),
            uart_backend,
        );
        match serial_console_uart_status(
            ctx.uart_slot.is_some(),
            uart_base.is_some(),
            crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active(),
        ) {
            "driver-task-runtime" => log::info!(
                target: "userland",
                "[userland] UART cap owned by serial driver task; root console uses ring client"
            ),
            "unavailable" => log::warn!(
                target: "userland",
                "[userland] UART mapping unavailable; continuing with serial console anyway"
            ),
            "slot-missing" => log::warn!(
                target: "userland",
                "[userland] UART slot unavailable; serial console backend may be degraded"
            ),
            _ => {}
        }
        log::info!(
            target: "userland",
            "[userland] event-pump: using UART backend={} for shared console I/O",
            uart_backend
        );
        #[cfg(feature = "kernel")]
        if let Some(wifi_debug) = wifi_debug.as_mut() {
            pump = pump.with_wifi_debug(wifi_debug);
        }
        #[cfg(all(feature = "net-console", feature = "kernel"))]
        let mut start_deferred_net_after_prompt = false;
        #[cfg(all(feature = "net-console", feature = "kernel"))]
        {
            if net_stack.is_none() && net_deferred_config.is_some() {
                let resume_deferred_net_after_prompt =
                    deferred_net_console_resume_after_prompt_allowed(
                        crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active(),
                        crate::hal::driver_task::driver_task_runtime_proof().pointer_free_ipc_proof,
                    );
                if resume_deferred_net_after_prompt {
                    start_deferred_net_after_prompt = true;
                    boot_log::force_uart_line(
                        "[net-console] deferred resume scheduled reason=driver-startup-before-root-prompt action=delay-interactive-prompt",
                    );
                    log::info!(
                        target: "net-console",
                        "[net-console] deferred Wi-Fi resume scheduled before the interactive serial prompt"
                    );
                } else {
                    let skip_reason = deferred_net_console_after_prompt_skip_reason(
                        crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active(),
                        crate::hal::driver_task::driver_task_runtime_proof().pointer_free_ipc_proof,
                    );
                    let mut line = HeaplessString::<160>::new();
                    let _ = write!(
                        line,
                        "[net-console] deferred resume skipped reason={skip_reason} action=serial-diagnostics-only",
                    );
                    boot_log::force_uart_line(line.as_str());
                    log::warn!(
                        target: "net-console",
                        "[net-console] deferred resume skipped: {skip_reason}"
                    );
                    let mut detail = HeaplessString::<192>::new();
                    let _ = write!(detail, "deferred net start skipped: {skip_reason}",);
                    net_unavailable_detail = Some(detail);
                    let _ = net_deferred_config.take();
                }
            }
        }

        if start_deferred_net_after_prompt {
            #[cfg(feature = "net-console")]
            {
                pump = attach_network(pump, None, net_unavailable_detail.take());
            }
            start_root_console_starting(&mut pump);
            #[cfg(all(feature = "net-console", feature = "kernel"))]
            {
                if let Some(config) = net_deferred_config.take() {
                    boot_log::force_uart_line(
                        "[net-console] deferred resume reason=before-root-prompt action=start-wifi",
                    );
                    let local_seat_enabled = crate::generated::hardware_config().local_seat.enabled;
                    if local_seat_enabled {
                        boot_log::force_uart_line_raw(
                            "[trace] deferred Wi-Fi logs remain on serial before interactive prompt",
                        );
                    }
                    log::info!(
                        target: "net-console",
                        "[net-console] deferred resume before interactive serial prompt; starting Wi-Fi stack"
                    );
                    match init_deferred_net_console(config, ctx.wifi_debug_hal_ptr) {
                        Ok(mut stack) => {
                            emit_deferred_net_console_result(&stack, local_seat_enabled);
                            pump = attach_network(pump, Some(&mut stack), None);
                            if pump.net_console_enabled() {
                                log::info!(
                                target: "net-console",
                                "[net-console] listening on 0.0.0.0:{}",
                                    crate::net::CONSOLE_TCP_PORT
                                );
                            }
                            wait_for_net_console_before_root_console(&mut pump);
                            publish_root_console_ready(&mut pump);
                            enter_root_console_loop(pump);
                        }
                        Err(err) => {
                            let mut detail = HeaplessString::<192>::new();
                            let _ = write!(detail, "{err}");
                            emit_deferred_net_console_failure(&detail, local_seat_enabled);
                            pump = attach_network(pump, None, Some(detail));
                            publish_root_console_ready(&mut pump);
                            enter_root_console_loop(pump);
                        }
                    }
                }
            }
            publish_root_console_ready(&mut pump);
            enter_root_console_loop(pump);
        } else if let Some(mut active_net_stack) = net_stack.take() {
            #[cfg(feature = "net-console")]
            {
                pump = attach_network(
                    pump,
                    Some(&mut active_net_stack),
                    net_unavailable_detail.take(),
                );
                if pump.net_console_enabled() {
                    log::info!(
                        target: "net-console",
                        "[net-console] listening on 0.0.0.0:{}",
                        crate::net::CONSOLE_TCP_PORT
                    );
                }
            }
            #[cfg(all(feature = "net-console", feature = "kernel"))]
            wait_for_net_console_before_root_console(&mut pump);
            start_root_console_prompt(&mut pump);
            enter_root_console_loop(pump);
        } else {
            #[cfg(feature = "net-console")]
            {
                pump = attach_network(pump, None, net_unavailable_detail.take());
            }
            start_root_console_prompt(&mut pump);
            enter_root_console_loop(pump);
        }
    }

    #[cfg(not(all(feature = "serial-console", feature = "kernel")))]
    #[allow(clippy::diverging_sub_expression)]
    {
        boot_log::allow_ep_only_transport();
        pump.run();
    }
}

#[cfg(all(feature = "serial-console", feature = "kernel"))]
fn start_root_console_starting<
    'a,
    D,
    T,
    I,
    V,
    const RX: usize,
    const TX: usize,
    const LINE: usize,
>(
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    log::info!(
        target: "root_task::kernel",
        "[boot] TimersAndIPC: root-console.start.begin"
    );
    log::info!(target: "boot", "[boot] before starting root shell");
    log::info!(target: "boot", "[boot] root shell starting");
    log::info!(target: "console", "[console] starting root CLI");
    boot_log::force_uart_line_raw("[mark] root-console.start.begin");
    pump.start_cli();
    boot_log::force_uart_line_raw("[mark] root-console.start.ok");
}

#[cfg(all(feature = "serial-console", feature = "kernel"))]
fn publish_root_console_ready<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    pump.announce_console_ready();
    let now_ms = crate::hal::timebase().now_ms();
    let result = lifecycle::auto_boot_complete(now_ms);
    let line = match result {
        Ok(transition) => lifecycle::format_transition_log(&transition),
        Err(err) => lifecycle::format_denied_log(lifecycle::state(), "auto-boot", err),
    };
    log_buffer::append_log_line(line.as_str());
}

#[cfg(all(feature = "serial-console", feature = "kernel"))]
fn start_root_console_prompt<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    start_root_console_starting(pump);
    publish_root_console_ready(pump);
}

#[cfg(all(feature = "serial-console", feature = "kernel"))]
fn enter_root_console_loop<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: EventPump<'a, D, T, I, V, RX, TX, LINE>,
) -> !
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
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

#[cfg(feature = "net-console")]
fn take_net_unavailable_detail(ctx: &BootContext) -> Option<HeaplessString<192>> {
    ctx.net_unavailable_detail.borrow_mut().take()
}

#[cfg(feature = "net-console")]
fn take_net_deferred_config(ctx: &BootContext) -> Option<crate::net::ConsoleNetConfig> {
    ctx.net_deferred_config.borrow_mut().take()
}

#[cfg(not(feature = "net-console"))]
fn take_net_stack(_ctx: &BootContext) -> Option<NetStackHandle> {
    None
}

#[cfg(not(feature = "net-console"))]
fn take_net_deferred_config(_ctx: &BootContext) -> Option<()> {
    None
}

#[cfg(all(feature = "net-console", feature = "kernel"))]
fn init_deferred_net_console(
    config: crate::net::ConsoleNetConfig,
    hal_ptr: usize,
) -> Result<NetStackHandle, crate::net::DefaultNetConsoleError> {
    if hal_ptr == 0 {
        return Err(crate::net::NetConsoleError::InvalidConfig(
            "deferred-hal-missing",
        ));
    }

    // SAFETY: `hal_ptr` is the leaked bootstrap `KernelHal` pointer already
    // used by the root-console Wi-Fi debug handle. The deferred resume runs
    // after the startup banner but before the prompt is published, so no Wi-Fi
    // debug command can concurrently borrow the HAL while the stack is created.
    let hal = unsafe { &mut *(hal_ptr as *mut KernelHal<'static>) };
    crate::net::init_net_console(hal, config)
}

#[cfg(all(feature = "net-console", feature = "kernel"))]
fn emit_deferred_net_console_result(stack: &NetStackHandle, local_seat_enabled: bool) {
    let status = stack.status_report();
    let mut line = HeaplessString::<192>::new();
    let state = match status.address_source {
        "wifi-host-eapol-pending" => "deferred pending",
        "wifi-host-eapol-required" => "deferred blocked",
        _ => "deferred ready",
    };
    let _ = write!(
        line,
        "[net-console] {state} backend={} active={} address_source={} dhcp={} port={}",
        status.backend,
        status.active_interface,
        status.address_source,
        status.dhcp_phase,
        crate::net::CONSOLE_TCP_PORT,
    );
    if local_seat_enabled {
        boot_log::force_uart_line_raw(line.as_str());
    } else {
        boot_log::force_uart_line(line.as_str());
    }
    log::info!(target: "net-console", "{}", line.as_str());
}

#[cfg(all(feature = "net-console", feature = "kernel"))]
fn emit_deferred_net_console_failure(detail: &HeaplessString<192>, local_seat_enabled: bool) {
    let mut line = HeaplessString::<224>::new();
    let _ = write!(
        line,
        "[net-console] deferred failed detail={}",
        detail.as_str(),
    );
    if local_seat_enabled {
        boot_log::force_uart_line_raw(line.as_str());
    } else {
        boot_log::force_uart_line(line.as_str());
    }
    log::warn!(target: "net-console", "{}", line.as_str());
}

#[cfg(all(feature = "net-console", feature = "kernel"))]
fn deferred_net_console_resume_after_prompt_allowed(
    physical_pi_owner_state: bool,
    pointer_free_ipc_proof: bool,
) -> bool {
    !physical_pi_owner_state || pointer_free_ipc_proof
}

#[cfg(all(feature = "net-console", feature = "kernel"))]
const fn deferred_net_console_after_prompt_skip_reason(
    physical_pi_owner_state: bool,
    pointer_free_ipc_proof: bool,
) -> &'static str {
    if physical_pi_owner_state && !pointer_free_ipc_proof {
        "driver-task-net-runtime-unproved"
    } else {
        "none"
    }
}

#[cfg(all(feature = "net-console", feature = "kernel"))]
const PRE_ROOT_NET_CONSOLE_WAIT_TIMEOUT_MS: u64 = 60_000;
#[cfg(all(feature = "net-console", feature = "kernel"))]
const PRE_ROOT_NET_CONSOLE_WAIT_STATUS_MS: u64 = 5_000;
#[cfg(all(feature = "net-console", feature = "kernel"))]
const PRE_ROOT_NET_CONSOLE_WAIT_POLL_LIMIT: u32 = 250_000;
#[cfg(all(feature = "net-console", feature = "kernel"))]
fn wait_for_net_console_before_root_console<
    'a,
    D,
    T,
    I,
    V,
    const RX: usize,
    const TX: usize,
    const LINE: usize,
>(
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    if !pump.net_console_enabled() || pump.net_console_ready_for_root() {
        return;
    }

    let start_ms = crate::hal::timebase().now_ms();
    let mut next_status_ms = PRE_ROOT_NET_CONSOLE_WAIT_STATUS_MS;
    let mut polls = 0u32;
    let active_interface = pump.net_console_active_interface().unwrap_or("net");
    let wifi_wait = active_interface == "wifi";
    if wifi_wait {
        boot_log::force_uart_line(
            "[net-console] root console bounded-wait reason=wifi-not-ready action=wait-for-wifi",
        );
        pump.publish_pre_root_boot_progress(
            "[boot] bounded Wi-Fi release before root console action=driver-settle",
        );
        log::info!(
            target: "net-console",
            "[net-console] root console running bounded Wi-Fi release before prompt"
        );
    } else {
        let mut line = HeaplessString::<160>::new();
        let _ = write!(
            line,
            "[net-console] root console bounded-wait reason=net-not-ready active={active_interface} action=wait-for-net",
        );
        boot_log::force_uart_line(line.as_str());
        pump.publish_pre_root_boot_progress(
            "[boot] bounded network release before root console action=driver-settle",
        );
        log::info!(
            target: "net-console",
            "[net-console] root console running bounded network release before prompt active={active_interface}"
        );
    }

    loop {
        if pump.net_console_ready_for_root() {
            let elapsed_ms = crate::hal::timebase().now_ms().saturating_sub(start_ms);
            let mut line = HeaplessString::<160>::new();
            let _ = write!(
                line,
                "[net-console] root console wait complete reason=net-ready action=start-serial-root-console elapsed_ms={elapsed_ms} polls={polls}",
            );
            boot_log::force_uart_line(line.as_str());
            log::info!(target: "net-console", "{}", line.as_str());
            return;
        }
        if let Some(reason) = pump.net_console_pre_root_serial_release_reason() {
            let elapsed_ms = crate::hal::timebase().now_ms().saturating_sub(start_ms);
            let mut line = HeaplessString::<176>::new();
            let _ = write!(
                line,
                "[net-console] root console wait ended reason={reason} action=start-serial-root-console elapsed_ms={elapsed_ms} polls={polls}",
            );
            boot_log::force_uart_line(line.as_str());
            if reason == "wifi-host-eapol-pending" {
                log::info!(target: "net-console", "{}", line.as_str());
            } else {
                log::warn!(target: "net-console", "{}", line.as_str());
            }
            return;
        }
        let elapsed_ms = crate::hal::timebase().now_ms().saturating_sub(start_ms);
        if elapsed_ms >= next_status_ms {
            let mut line = HeaplessString::<176>::new();
            let _ = write!(
                line,
                "[net-console] root console wait status elapsed_ms={elapsed_ms} polls={polls} action=continue-driver-settle",
            );
            boot_log::force_uart_line(line.as_str());
            pump.publish_pre_root_boot_progress(line.as_str());
            next_status_ms = next_status_ms.saturating_add(PRE_ROOT_NET_CONSOLE_WAIT_STATUS_MS);
        }
        if elapsed_ms >= PRE_ROOT_NET_CONSOLE_WAIT_TIMEOUT_MS
            || polls >= PRE_ROOT_NET_CONSOLE_WAIT_POLL_LIMIT
        {
            let mut line = HeaplessString::<192>::new();
            let reason = if wifi_wait {
                "wifi-not-ready-timeout"
            } else {
                "net-not-ready-timeout"
            };
            let _ = write!(
                line,
                "[net-console] root console wait ended reason={reason} action=start-serial-root-console elapsed_ms={elapsed_ms} polls={polls}",
            );
            boot_log::force_uart_line(line.as_str());
            log::warn!(target: "net-console", "{}", line.as_str());
            return;
        }

        pump.poll_pre_root_network();
        polls = polls.saturating_add(1);
        sel4::yield_now();
    }
}

#[cfg(feature = "kernel")]
fn attach_kernel_console<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    mut pump: EventPump<'a, D, T, I, V, RX, TX, LINE>,
    ctx: &BootContext,
    bootstrap_ipc: Option<&'a mut UserlandBootstrapHandler>,
    wifi_debug: Option<&'a mut KernelWifiDebugHandle>,
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
    if let Some(wifi_debug) = wifi_debug {
        pump = pump.with_wifi_debug(wifi_debug);
    }

    pump
}

#[cfg(not(feature = "kernel"))]
fn attach_kernel_console<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: EventPump<'a, D, T, I, V, RX, TX, LINE>,
    _ctx: &BootContext,
    _bootstrap_ipc: Option<&'a mut UserlandBootstrapHandler>,
    _wifi_debug: Option<&'a mut KernelWifiDebugHandle>,
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
fn attach_local_seat<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    mut pump: EventPump<'a, D, T, I, V, RX, TX, LINE>,
    ctx: &BootContext,
) -> EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    if let Some(runtime) = ctx.local_seat.borrow_mut().take() {
        pump = pump.with_local_seat(runtime);
    }
    pump
}

#[cfg(not(feature = "kernel"))]
fn attach_local_seat<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
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
        let policy = affinity::policy();
        pump = affinity::with_role_affinity(affinity::AffinityRole::NineDoor, 0, &policy, || {
            pump.with_ninedoor(ninedoor)
        });
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
    net_unavailable_detail: Option<HeaplessString<192>>,
) -> EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    pump = pump.with_network_unavailable_detail(net_unavailable_detail);
    if let Some(net_stack) = net_stack_handle {
        pump = pump.with_network(net_stack);
    }

    pump
}

#[cfg(not(feature = "net-console"))]
fn attach_network<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: EventPump<'a, D, T, I, V, RX, TX, LINE>,
    _net_stack_handle: Option<&'a mut NetStackHandle>,
    _net_unavailable_detail: Option<HeaplessString<192>>,
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
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    pump.announce_console_ready();
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
        "Root shell: Cohesix console online on UART",
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

#[cfg(test)]
mod tests {
    #[cfg(all(feature = "net-console", feature = "kernel"))]
    #[test]
    fn deferred_net_console_after_prompt_still_requires_driver_task_ring_proof_on_pi() {
        assert!(!super::deferred_net_console_resume_after_prompt_allowed(
            true, false
        ));
        assert!(super::deferred_net_console_resume_after_prompt_allowed(
            true, true
        ));
        assert!(super::deferred_net_console_resume_after_prompt_allowed(
            false, false
        ));
        assert_eq!(
            super::deferred_net_console_after_prompt_skip_reason(true, false),
            "driver-task-net-runtime-unproved"
        );
    }

    #[cfg(all(feature = "serial-console", feature = "kernel"))]
    #[test]
    fn serial_console_uart_status_names_driver_task_ownership() {
        assert_eq!(
            super::serial_console_uart_status(false, true, true),
            "driver-task-runtime"
        );
        assert_eq!(
            super::serial_console_uart_status(false, true, false),
            "slot-missing"
        );
        assert_eq!(
            super::serial_console_uart_status(true, true, true),
            "root-mapped"
        );
        assert_eq!(
            super::serial_console_uart_status(true, false, true),
            "unavailable"
        );
    }
}
