// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Userland hand-off and runtime wiring for console and networking surfaces.
// Author: Lukas Bower
//! Minimal userland entrypoints exposed by the root task.
#![allow(unsafe_code)]

use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(any(not(target_arch = "aarch64"), not(feature = "timers-arch-counter")))]
use core::sync::atomic::AtomicU64;

#[cfg(feature = "kernel")]
#[cfg(feature = "serial-console")]
use crate::boot::uart_pl011;
use crate::bootstrap::log as boot_log;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::console::CohesixConsole;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::console::Console as SerialConsole;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::debug_uart::debug_uart_str;
#[cfg(all(feature = "kernel", feature = "net-console"))]
use crate::drivers::driver_task_net::{Cyw43BootstrapSupervisor, Cyw43BootstrapTurnOutcome};
#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
use crate::event::Cyw43BootstrapAtomicDecisionOutcome;
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
#[cfg(all(feature = "kernel", feature = "net-console"))]
use crate::rust_alloc::boxed::Box;
use crate::sel4;
#[cfg(all(feature = "serial-console", feature = "kernel"))]
use crate::serial::pl011::Pl011;
#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
use crate::serial::DEFAULT_LINE_CAPACITY;
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
    let serial = ctx
        .serial
        .borrow_mut()
        .take()
        .expect("operational serial driver missing from validated BootContext");
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
    #[cfg(all(
        feature = "net-console",
        feature = "kernel",
        target_arch = "aarch64",
        target_os = "none",
        sel4_config_kernel_mcs
    ))]
    let mut net_deferred_console_runtime = take_net_deferred_console_runtime(&ctx);
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
    attach_kernel_console(&mut pump, &ctx, bootstrap_ipc.as_mut(), None);
    attach_local_seat(&mut pump, &ctx);
    attach_ninedoor_bridge(&mut pump, &ctx);
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
            pump.attach_wifi_debug(wifi_debug);
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
                        "[net-console] deferred resume scheduled reason=driver-startup-before-root-prompt action=publish-prompt-then-supervise",
                    );
                    log::info!(
                        target: "net-console",
                        "[net-console] deferred Wi-Fi resume scheduled after the interactive serial/local-seat prompt"
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
                attach_network(&mut pump, None, net_unavailable_detail.take());
            }
            #[cfg(all(feature = "net-console", feature = "kernel"))]
            pump.defer_local_seat_hdmi_ready_until_cyw43_terminal();
            // Publish serial/local-seat before touching the potentially slow
            // physical Wi-Fi bootstrap. The single-attempt supervisor below
            // gives operator surfaces their own bounded phase between Driver
            // turns without creating another boot or hardware-service lane.
            start_root_console_prompt(&mut pump);
            #[cfg(all(feature = "net-console", feature = "kernel"))]
            {
                if let Some(config) = net_deferred_config.take() {
                    let resume_line = "[net-console] deferred resume reason=post-root-prompt action=start-persistent-wifi-supervisor";
                    if !pump.queue_cyw43_bootstrap_operator_line(resume_line) {
                        boot_log::force_uart_line(resume_line);
                    }
                    #[cfg(all(
                        target_arch = "aarch64",
                        target_os = "none",
                        sel4_config_kernel_mcs
                    ))]
                    enter_root_console_loop_with_deferred_net_supervisor(
                        &mut pump,
                        config,
                        net_deferred_console_runtime.take(),
                        &ctx,
                    );
                    #[cfg(not(all(
                        target_arch = "aarch64",
                        target_os = "none",
                        sel4_config_kernel_mcs
                    )))]
                    enter_root_console_loop_with_deferred_net_supervisor(&mut pump, config, &ctx);
                }
            }
            enter_root_console_loop(&mut pump, &ctx);
        } else if let Some(mut active_net_stack) = net_stack.take() {
            #[cfg(feature = "net-console")]
            {
                attach_network(
                    &mut pump,
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
            enter_root_console_loop(&mut pump, &ctx);
        } else {
            #[cfg(feature = "net-console")]
            {
                attach_network(&mut pump, None, net_unavailable_detail.take());
            }
            start_root_console_prompt(&mut pump);
            enter_root_console_loop(&mut pump, &ctx);
        }
    }

    #[cfg(not(all(feature = "serial-console", feature = "kernel")))]
    #[allow(clippy::diverging_sub_expression)]
    {
        boot_log::allow_ep_only_transport();
        activate_root_control_temporal_or_fail(&ctx);
        pump.run();
    }
}

fn activate_root_control_temporal_or_fail(ctx: &BootContext) {
    #[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
    {
        // Emit the boundary breadcrumb while the init TCB still owns its
        // bootstrap scheduling context. A synchronous UART write after the
        // configure call would itself consume the generated steady budget.
        boot_log::force_uart_line(
            "[critical] arming root-control steady-state SC at event-loop boundary",
        );
        match crate::hal::critical_tcb::activate_root_control_temporal_runtime(ctx.critical_runtime)
        {
            // Milestone 26e: surrender the activation-seam remainder
            // immediately after attaching the steady SC. The next
            // replenishment begins with the first containment probe or
            // EventPump phase; no output or helper may spend that first
            // admitted root-control budget here.
            Ok(()) => sel4::yield_now(),
            Err(error) => {
                log::error!(
                    target: "root_task::kernel",
                    "[critical] root-control steady SC activation failed: {error:?}"
                );
                boot_log::force_uart_line(
                    "[critical] root-control steady SC activation failed; fail-stop",
                );
                loop {
                    sel4::yield_now();
                }
            }
        }
    }
    #[cfg(not(all(feature = "kernel", sel4_config_kernel_mcs)))]
    let _ = ctx;
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
    if !pump.queue_cyw43_bootstrap_operator_line("[mark] root-console.start.ok") {
        boot_log::force_uart_line_raw("[mark] root-console.start.ok");
    }
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
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
    ctx: &BootContext,
) -> !
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    if crate::serial::serial_linked_runtime_transport_active() {
        for line in [
            "[boot] TimersAndIPC: root-console.start.ok",
            "[boot] TimersAndIPC: queen.start.begin",
            "[boot] root shell started; entering event loop",
            "[boot] TimersAndIPC: queen.start.ok",
            "[boot] phase: TimersAndIPC.end",
        ] {
            let _ = pump.queue_cyw43_bootstrap_operator_line(line);
        }
        boot_log::allow_ep_only_transport();
    } else {
        announce_root_console_loop_start();
    }
    activate_root_control_temporal_or_fail(ctx);
    let hal_ptr = ctx.wifi_debug_hal_ptr;
    #[cfg(feature = "net-console")]
    let mut deferred_wired_handoff_admission_logged = false;
    loop {
        #[cfg(any(
            feature = "net-console",
            all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs)
        ))]
        let mut recovery_turn = false;
        #[cfg(not(any(
            feature = "net-console",
            all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs)
        )))]
        let recovery_turn = false;
        #[cfg(feature = "net-console")]
        if hal_ptr != 0 {
            recovery_turn =
                with_deferred_root_hal(hal_ptr, |hal| pump.contain_faulted_direct_genet_pair(hal))
                    .unwrap_or(false);
        }
        #[cfg(feature = "net-console")]
        if !recovery_turn && hal_ptr != 0 {
            recovery_turn =
                with_deferred_root_hal(hal_ptr, |hal| pump.contain_faulted_console_network(hal))
                    .unwrap_or(false);
        }
        #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
        if !recovery_turn && hal_ptr != 0 {
            recovery_turn =
                with_deferred_root_hal(hal_ptr, |hal| pump.contain_faulted_ninedoor(hal))
                    .unwrap_or(false);
        }
        #[cfg(feature = "net-console")]
        if !deferred_wired_handoff_admission_logged
            && crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
            && pump.net_console_active_interface() == Some("wired")
        {
            deferred_wired_handoff_admission_logged = true;
            let mut line = HeaplessString::<192>::new();
            let _ = write!(
                line,
                "CONSOLE_NETWORK_HANDOFF_ADMISSION schema=v1 hal={} recovery={} net=attached action={}",
                if hal_ptr == 0 { "missing" } else { "present" },
                if recovery_turn { "reserved" } else { "idle" },
                if recovery_turn || hal_ptr == 0 {
                    "defer"
                } else {
                    "evaluate"
                },
            );
            boot_log::force_uart_line(line.as_str());
        }
        #[cfg(feature = "net-console")]
        if !recovery_turn && hal_ptr != 0 {
            recovery_turn = with_deferred_root_hal(hal_ptr, |hal| {
                pump.service_deferred_console_network_handoff(hal)
            })
            .unwrap_or(false);
        }
        let explicit_yield_required = if recovery_turn {
            true
        } else {
            pump.poll_root_control_quantum()
        };
        #[cfg(not(any(
            feature = "net-console",
            all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs)
        )))]
        let _ = hal_ptr;
        if explicit_yield_required {
            sel4::yield_now();
        }
    }
}

#[cfg(all(feature = "serial-console", feature = "kernel"))]
fn run_root_console_pump<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
) -> !
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    loop {
        if pump.poll_root_control_quantum() {
            sel4::yield_now();
        }
    }
}

#[cfg(all(feature = "serial-console", feature = "kernel"))]
fn announce_root_console_loop_start() {
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
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
// Linux brcmfmac performs one probe/bind episode and keeps retries local to
// the owning operation. Cohesix likewise admits exactly one production boot
// episode and one cold physical pair. A pre-service-ready fault drains and
// fences its exact owner, then fails closed without starting pair 2. Only the
// later same-generation DHCP/listener terminal may arm one separately bounded
// runtime recovery episode.
const CYW43_BOOTSTRAP_ATTEMPT: u32 = 1;

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const CYW43_GATE8_STABILIZATION_TIMEOUT_MS: u64 = 90_000;

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const fn response_time_us_to_clock_ms(response_time_us: u32) -> Option<u64> {
    if response_time_us == 0 {
        return None;
    }
    match (response_time_us as u64).checked_add(999) {
        Some(us) => Some(us / 1_000),
        None => None,
    }
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn child_ready_response_bound_ms(tasks: &[crate::generated::TemporalTaskConfig]) -> Option<u64> {
    fn admitted_response_ms(
        tasks: &[crate::generated::TemporalTaskConfig],
        id: &str,
    ) -> Option<u64> {
        let task = tasks.iter().find(|task| {
            task.id == id
                && task.admitted
                && task.execution == crate::generated::TemporalExecution::Active
        })?;
        response_time_us_to_clock_ms(task.response_time_us)
    }

    // Round each independently: the child publication and root observation
    // occur on separate millisecond-clock observations and each can consume
    // its own partial millisecond.
    admitted_response_ms(tasks, "console-network-service")?
        .checked_add(admitted_response_ms(tasks, "root-control")?)
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn cyw43_child_ready_response_bound_ms() -> Option<u64> {
    child_ready_response_bound_ms(crate::generated::temporal_tasks())
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const SERIAL_LINKED_RUNTIME_RETRY_MS: u64 = 250;

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeferredSerialRouteRetry {
    next_probe_ms: u64,
    blocked_reported: bool,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
impl DeferredSerialRouteRetry {
    const fn new(now_ms: u64) -> Self {
        Self {
            next_probe_ms: now_ms,
            blocked_reported: false,
        }
    }

    const fn probe_due(self, now_ms: u64) -> bool {
        now_ms >= self.next_probe_ms
    }

    fn record_missing_proof(&mut self, now_ms: u64) -> bool {
        self.next_probe_ms = now_ms.saturating_add(SERIAL_LINKED_RUNTIME_RETRY_MS);
        let emit_blocked = !self.blocked_reported;
        self.blocked_reported = true;
        emit_blocked
    }

    fn record_ready(&mut self) {
        self.blocked_reported = false;
    }
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeferredCyw43TurnStatus {
    stage: Option<&'static str>,
    repeats: u64,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
impl DeferredCyw43TurnStatus {
    const fn new() -> Self {
        Self {
            stage: None,
            repeats: 0,
        }
    }

    fn observe(&mut self, stage: &'static str) -> Option<u64> {
        if self.stage == Some(stage) {
            self.repeats = self.repeats.saturating_add(1);
        } else {
            self.stage = Some(stage);
            self.repeats = 1;
        }
        self.repeats.is_power_of_two().then_some(self.repeats)
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    fn retry_last(&mut self) {
        self.repeats = self.repeats.saturating_sub(1);
    }
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredCyw43SupervisorPhase {
    Operator,
    Driver,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const fn deferred_cyw43_activation_reserve_us(
    budget_us: u32,
    wcet_us: u32,
    admitted: bool,
) -> Option<u64> {
    if !admitted || wcet_us == 0 || budget_us <= wcet_us {
        return None;
    }
    Some((budget_us - wcet_us) as u64)
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn deferred_cyw43_activation_reserve_from_manifest_us() -> Option<u64> {
    let root_control = crate::generated::temporal_tasks()
        .iter()
        .find(|task| task.id == "root-control")?;
    deferred_cyw43_activation_reserve_us(
        root_control.budget_us,
        root_control.wcet_us,
        root_control.admitted,
    )
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const DEFERRED_CYW43_ACTIVATION_MAX_PRODUCTIVE_UNITS: u8 = 64;

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredCyw43ActivationClock {
    Unstarted,
    Timed { started_ticks: u64, counter_hz: u64 },
    Invalid,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeferredCyw43ActivationWindow {
    clock: DeferredCyw43ActivationClock,
    logical_turns: u8,
    productive_units: u8,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
impl DeferredCyw43ActivationWindow {
    const fn new() -> Self {
        Self {
            clock: DeferredCyw43ActivationClock::Unstarted,
            logical_turns: 0,
            productive_units: 0,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    fn begin(&mut self, now_ticks: u64, counter_hz: u64) {
        if self.clock != DeferredCyw43ActivationClock::Unstarted {
            return;
        }
        self.clock = if now_ticks == 0 || counter_hz == 0 {
            DeferredCyw43ActivationClock::Invalid
        } else {
            DeferredCyw43ActivationClock::Timed {
                started_ticks: now_ticks,
                counter_hz,
            }
        };
    }

    fn turn_admitted(
        &mut self,
        now_ticks: u64,
        counter_hz: u64,
        activation_reserve_us: Option<u64>,
    ) -> bool {
        if self.productive_units >= DEFERRED_CYW43_ACTIVATION_MAX_PRODUCTIVE_UNITS {
            return false;
        }
        self.begin(now_ticks, counter_hz);
        let Some(activation_reserve_us) = activation_reserve_us else {
            return self.logical_turns == 0;
        };
        match self.clock {
            DeferredCyw43ActivationClock::Unstarted => false,
            DeferredCyw43ActivationClock::Timed {
                started_ticks,
                counter_hz: started_hz,
            } => {
                if now_ticks == 0
                    || counter_hz == 0
                    || counter_hz != started_hz
                    || now_ticks < started_ticks
                {
                    return false;
                }
                let elapsed_ticks = u128::from(now_ticks - started_ticks);
                let elapsed_scaled = elapsed_ticks.saturating_mul(1_000_000u128);
                let reserve_scaled =
                    u128::from(started_hz).saturating_mul(u128::from(activation_reserve_us));
                elapsed_scaled < reserve_scaled
            }
            DeferredCyw43ActivationClock::Invalid => self.logical_turns == 0,
        }
    }

    fn record_operator_turn(&mut self) {
        self.logical_turns = self.logical_turns.saturating_add(1);
    }

    fn record_driver_turn(&mut self, operation_executed: bool) {
        self.logical_turns = self.logical_turns.saturating_add(1);
        if operation_executed {
            self.productive_units = self.productive_units.saturating_add(1);
        }
    }

    fn record_attached_network_turn(&mut self, productive: bool) {
        self.logical_turns = self.logical_turns.saturating_add(1);
        if productive {
            self.productive_units = self.productive_units.saturating_add(1);
        }
    }
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn deferred_cyw43_yield_and_reset(window: &mut DeferredCyw43ActivationWindow) {
    sel4::yield_now();
    window.reset();
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredCyw43AttachedTurn {
    NetworkControl,
    ConsoleHandoff,
    CanonicalWait,
    RecoverySupervisor,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const fn deferred_cyw43_attached_turn(
    recovery_required: bool,
    canonical_parent: crate::drivers::driver_task_net::Cyw43CanonicalParentCut,
    console_handoff_authorized: bool,
) -> DeferredCyw43AttachedTurn {
    if !recovery_required {
        if console_handoff_authorized {
            return DeferredCyw43AttachedTurn::ConsoleHandoff;
        }
        return DeferredCyw43AttachedTurn::NetworkControl;
    }
    if canonical_parent.runnable() {
        return DeferredCyw43AttachedTurn::NetworkControl;
    }
    if canonical_parent.waiting() {
        return DeferredCyw43AttachedTurn::CanonicalWait;
    }
    DeferredCyw43AttachedTurn::RecoverySupervisor
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const fn deferred_cyw43_post_network_control_handoff_due(
    recovery_required: bool,
    console_handoff_pending: bool,
) -> bool {
    !recovery_required && console_handoff_pending
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredCyw43SupervisorTurn {
    Operator,
    Driver,
    Blocked,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredCyw43McsContinuation {
    Continue,
    Yield,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const fn deferred_cyw43_mcs_continuation(
    turn: DeferredCyw43SupervisorTurn,
    next_phase: DeferredCyw43SupervisorPhase,
    operation_executed: bool,
    operator_admitted_before: bool,
    network_attached: bool,
) -> DeferredCyw43McsContinuation {
    match (
        turn,
        next_phase,
        operation_executed,
        operator_admitted_before,
        network_attached,
    ) {
        (
            DeferredCyw43SupervisorTurn::Operator,
            DeferredCyw43SupervisorPhase::Driver,
            false,
            true,
            false,
        )
        | (
            DeferredCyw43SupervisorTurn::Driver,
            DeferredCyw43SupervisorPhase::Operator,
            true,
            _,
            false,
        ) => DeferredCyw43McsContinuation::Continue,
        (DeferredCyw43SupervisorTurn::Operator, _, _, _, _)
        | (DeferredCyw43SupervisorTurn::Driver, _, _, _, _)
        | (DeferredCyw43SupervisorTurn::Blocked, _, _, _, _) => DeferredCyw43McsContinuation::Yield,
    }
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const fn deferred_cyw43_supervisor_phase_step(
    phase: DeferredCyw43SupervisorPhase,
    may_begin: bool,
    driver_turn_due: bool,
) -> (DeferredCyw43SupervisorTurn, DeferredCyw43SupervisorPhase) {
    match (phase, may_begin, driver_turn_due) {
        (DeferredCyw43SupervisorPhase::Operator, true, true) => (
            DeferredCyw43SupervisorTurn::Operator,
            DeferredCyw43SupervisorPhase::Driver,
        ),
        (DeferredCyw43SupervisorPhase::Operator, _, false)
        | (DeferredCyw43SupervisorPhase::Operator, false, true) => (
            DeferredCyw43SupervisorTurn::Operator,
            DeferredCyw43SupervisorPhase::Operator,
        ),
        (DeferredCyw43SupervisorPhase::Driver, true, true) => (
            DeferredCyw43SupervisorTurn::Driver,
            DeferredCyw43SupervisorPhase::Operator,
        ),
        (DeferredCyw43SupervisorPhase::Driver, _, false)
        | (DeferredCyw43SupervisorPhase::Driver, false, true) => (
            DeferredCyw43SupervisorTurn::Blocked,
            DeferredCyw43SupervisorPhase::Operator,
        ),
    }
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn service_deferred_cyw43_bootstrap_sideband_condition<F>(stable_copy_and_ack: F) -> bool
where
    F: FnOnce() -> bool,
{
    // This return value is diagnostic only. The sequence-last batch/ACK
    // records remain the complete authority; a consumed notification or a
    // failed stable read neither authorizes Driver nor creates retry history.
    stable_copy_and_ack()
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn run_deferred_cyw43_attached_network_control_turn<Poll, Recovery, Diagnostic, Record, Commit>(
    mut poll: Poll,
    mut recovery_required: Recovery,
    mut diagnostic: Diagnostic,
    mut record: Record,
    mut commit: Commit,
) -> bool
where
    Poll: FnMut() -> bool,
    Recovery: FnMut() -> bool,
    Diagnostic: FnMut() -> crate::drivers::driver_task_net::Cyw43Gate8Diagnostic,
    Record: FnMut(crate::drivers::driver_task_net::Cyw43Gate8Diagnostic),
    Commit: FnMut(u32) -> bool,
{
    record(diagnostic());
    let productive_network_successor = poll();
    if recovery_required() {
        return false;
    }
    let candidate = diagnostic();
    let _ = commit(candidate.generation);
    productive_network_successor
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredNetSupervisorTerminal {
    BootstrapFailed,
    PermanentAttachedWifiFailure,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const fn permanent_failure_terminal_mode(
    network_attached: bool,
) -> Option<DeferredNetSupervisorTerminal> {
    if network_attached {
        Some(DeferredNetSupervisorTerminal::PermanentAttachedWifiFailure)
    } else {
        None
    }
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const fn deferred_net_supervisor_driver_turn_allowed(
    terminal_mode: Option<DeferredNetSupervisorTerminal>,
) -> bool {
    terminal_mode.is_none()
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const fn gate8_terminal_pending_cancels_for_recovery(
    terminal_decision_committed: bool,
    recovery_required: bool,
) -> bool {
    !terminal_decision_committed && recovery_required
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const fn gate8_terminal_pending_yields_to_finite_owner(
    terminal_decision_committed: bool,
    finite_terminal_pending: bool,
) -> bool {
    !terminal_decision_committed && finite_terminal_pending
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const fn gate8_terminal_decision_cut_open(
    recovery_required: bool,
    finite_terminal_pending: bool,
) -> bool {
    !recovery_required && !finite_terminal_pending
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn commit_gate8_terminal_decision<Retract>(
    terminal_decision_committed: &mut bool,
    generation: u32,
    recovery_required: bool,
    finite_terminal_pending: bool,
    retract: Retract,
) -> bool
where
    Retract: FnOnce(u32) -> bool,
{
    if *terminal_decision_committed {
        return true;
    }
    if !gate8_terminal_decision_cut_open(recovery_required, finite_terminal_pending)
        || !retract(generation)
    {
        return false;
    }
    *terminal_decision_committed = true;
    true
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeferredNetSupervisorSequence {
    status_sequence: u64,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
impl DeferredNetSupervisorSequence {
    const fn new() -> Self {
        Self { status_sequence: 0 }
    }

    fn next_status_sequence(&mut self) -> u64 {
        self.status_sequence = self.status_sequence.saturating_add(1);
        self.status_sequence
    }
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredGate8Lifecycle {
    Detached,
    Stabilizing {
        attempt: u32,
        deadline_ms: u64,
        candidate: Option<DeferredGate8Candidate>,
    },
    Committed {
        generation: u32,
        attempt: u32,
        deadline_ms: u64,
        service_phase: DeferredGate8ServicePhase,
    },
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredGate8ServicePhase {
    AwaitingDhcp,
    HandoffAuthorized {
        response_bound_ms: u64,
    },
    AwaitingReady {
        publication_not_before_ms: u64,
        deadline_ms: u64,
        deadline_observation_taken: bool,
    },
    Ready,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredGate8HandoffAdmission {
    Authorized { generation: u32 },
    NotPending,
    RecoveryRequired,
    ProofDrift { generation: u32 },
    Deadline { generation: u32, deadline_ms: u64 },
    InvalidAuthority { generation: u32 },
    InvalidPhase,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredGate8HandoffRevalidation {
    Authorized { generation: u32 },
    NotPending,
    RecoveryRequired,
    ProofDrift { generation: u32 },
    InvalidPhase,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeferredGate8ChildReadyPlan {
    generation: u32,
    response_bound_ms: u64,
    publication_not_before_ms: u64,
    counter_hz: u64,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeferredGate8AbsoluteClockSample {
    counter_ticks: u64,
    counter_hz: u64,
    generated_timer_hz: u64,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
impl DeferredGate8AbsoluteClockSample {
    fn absolute_ms(self) -> Option<u64> {
        if self.counter_ticks == 0
            || self.counter_hz == 0
            || self.generated_timer_hz == 0
            || self.counter_hz != self.generated_timer_hz
        {
            return None;
        }
        let seconds = self.counter_ticks / self.counter_hz;
        let remainder = self.counter_ticks % self.counter_hz;
        let absolute_ms = u128::from(seconds).checked_mul(1_000)?.checked_add(
            u128::from(remainder)
                .checked_mul(1_000)?
                .checked_div(u128::from(self.counter_hz))?,
        )?;
        u64::try_from(absolute_ms).ok().filter(|value| *value != 0)
    }
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn deferred_gate8_absolute_clock_sample() -> DeferredGate8AbsoluteClockSample {
    DeferredGate8AbsoluteClockSample {
        counter_ticks: monotonic_ticks(),
        counter_hz: counter_frequency(),
        generated_timer_hz: crate::generated::console_network_service_config().timer_clock_hz,
    }
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const fn child_ready_publication_in_window(
    publication_not_before_ms: u64,
    deadline_ms: u64,
    published_ms: u64,
) -> bool {
    published_ms != 0
        && publication_not_before_ms != 0
        && publication_not_before_ms <= published_ms
        && published_ms < deadline_ms
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredGate8ChildReadyPreparation {
    Prepared(DeferredGate8ChildReadyPlan),
    GenerationDrift,
    InvalidPhase,
    AlreadyArmed,
    ClockInvalid,
    ClockOverflow,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredGate8ChildReadyArm {
    Armed { deadline_ms: u64 },
    RecoveryRequired,
    GenerationDrift,
    InvalidPhase,
    AlreadyArmed,
    ClockInvalid,
    ClockOverflow,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredGate8ChildReadyPreNetworkAction {
    Continue,
    ObservePublicationOnly {
        generation: u32,
        deadline_ms: u64,
    },
    Fail {
        generation: u32,
        blocker: &'static str,
        deadline_ms: u64,
    },
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeferredGate8Candidate {
    pair_scrub_epoch: u64,
    generation: u32,
    publication_receipt: crate::drivers::driver_task_net::Cyw43Gate8PublicationReceipt,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeferredGate8TerminalPending {
    attempt: u32,
    generation: u32,
    deadline_ms: u64,
    blocker: &'static str,
    diagnostic: crate::drivers::driver_task_net::Cyw43Gate8Diagnostic,
    status_sequence: u64,
    terminal_decision_committed: bool,
    failure_transaction_retained: bool,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct DeferredCyw43RecoveryDiagnosticPending {
    recovery: Option<crate::drivers::driver_task_net::Cyw43DeferredRecoveryDiagnostic>,
    live_generation: u32,
    operator_line: HeaplessString<DEFAULT_LINE_CAPACITY>,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredGate8Observation {
    Pending,
    Publish {
        generation: u32,
        publication_receipt: crate::drivers::driver_task_net::Cyw43Gate8PublicationReceipt,
    },
    Committed,
    ServiceReady,
    Retracted {
        generation: u32,
    },
    Deadline {
        generation: u32,
        deadline_ms: u64,
        blocker: &'static str,
    },
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
impl DeferredGate8Lifecycle {
    const fn new() -> Self {
        Self::Detached
    }

    /// Enter one outer attempt's bounded stabilization window.
    ///
    /// Gate 8 reproof before service readiness retains the original absolute
    /// deadline. Once the same generation reaches DHCP/listener readiness, a
    /// later runtime recovery is a distinct bounded service episode.
    fn enter_stabilizing(&mut self, attempt: u32, now_ms: u64) -> u64 {
        match *self {
            Self::Stabilizing {
                attempt: active_attempt,
                deadline_ms,
                ..
            } if active_attempt == attempt => {
                return deadline_ms;
            }
            Self::Committed {
                attempt,
                deadline_ms,
                service_phase:
                    DeferredGate8ServicePhase::AwaitingDhcp
                    | DeferredGate8ServicePhase::HandoffAuthorized { .. }
                    | DeferredGate8ServicePhase::AwaitingReady { .. },
                ..
            } => {
                *self = Self::Stabilizing {
                    attempt,
                    deadline_ms,
                    candidate: None,
                };
                return deadline_ms;
            }
            Self::Detached | Self::Stabilizing { .. } | Self::Committed { .. } => {}
        }
        let deadline_ms = now_ms.saturating_add(CYW43_GATE8_STABILIZATION_TIMEOUT_MS);
        *self = Self::Stabilizing {
            attempt,
            deadline_ms,
            candidate: None,
        };
        deadline_ms
    }

    fn observe(
        &mut self,
        attempt: u32,
        now_ms: u64,
        accepted_generation_operational: bool,
        publication_receipt: Option<crate::drivers::driver_task_net::Cyw43Gate8PublicationReceipt>,
        diagnostic: crate::drivers::driver_task_net::Cyw43Gate8Diagnostic,
    ) -> DeferredGate8Observation {
        if let Self::Committed {
            generation,
            service_phase,
            ..
        } = *self
        {
            if accepted_generation_operational && diagnostic.generation == generation {
                return if service_phase == DeferredGate8ServicePhase::Ready {
                    DeferredGate8Observation::ServiceReady
                } else {
                    DeferredGate8Observation::Committed
                };
            }
            if matches!(
                service_phase,
                DeferredGate8ServicePhase::AwaitingReady { .. }
            ) {
                return DeferredGate8Observation::Deadline {
                    generation,
                    deadline_ms: now_ms,
                    blocker: "child-ready-generation-drift",
                };
            }
            self.enter_stabilizing(attempt, now_ms);
            return DeferredGate8Observation::Retracted { generation };
        }

        let deadline_ms = self.enter_stabilizing(attempt, now_ms);
        if now_ms >= deadline_ms {
            let blocker = diagnostic
                .subgates
                .iter()
                .find(|subgate| {
                    subgate.status != crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pass
                })
                .map_or(
                    if publication_receipt.is_some() {
                        "gate8-stabilization-deadline"
                    } else {
                        "gate8-publication-quiescence"
                    },
                    |subgate| subgate.blocker,
                );
            return DeferredGate8Observation::Deadline {
                generation: diagnostic.generation,
                deadline_ms,
                blocker,
            };
        }
        let candidate = DeferredGate8Candidate {
            pair_scrub_epoch: diagnostic.pair_scrub_epoch,
            generation: diagnostic.generation,
            publication_receipt: match publication_receipt {
                Some(receipt)
                    if receipt.pair_scrub_epoch == diagnostic.pair_scrub_epoch
                        && receipt.generation == diagnostic.generation =>
                {
                    receipt
                }
                Some(_) | None => {
                    if let Self::Stabilizing {
                        candidate: retained_candidate,
                        ..
                    } = self
                    {
                        *retained_candidate = None;
                    }
                    return DeferredGate8Observation::Pending;
                }
            },
        };
        if diagnostic.stable() {
            let Self::Stabilizing {
                candidate: retained_candidate,
                ..
            } = self
            else {
                return DeferredGate8Observation::Pending;
            };
            if *retained_candidate == Some(candidate) {
                return DeferredGate8Observation::Publish {
                    generation: diagnostic.generation,
                    publication_receipt: candidate.publication_receipt,
                };
            }
            *retained_candidate = Some(candidate);
            return DeferredGate8Observation::Pending;
        }
        if let Self::Stabilizing {
            candidate: retained_candidate,
            ..
        } = self
        {
            *retained_candidate = None;
        }
        DeferredGate8Observation::Pending
    }

    fn reject_publication(&mut self, generation: u32) -> bool {
        let Self::Stabilizing {
            candidate: retained_candidate,
            ..
        } = self
        else {
            return false;
        };
        if !retained_candidate.is_some_and(|candidate| candidate.generation == generation) {
            return false;
        }
        *retained_candidate = None;
        true
    }

    fn accept_commit(&mut self, generation: u32) -> bool {
        let Self::Stabilizing {
            attempt,
            deadline_ms,
            candidate:
                Some(DeferredGate8Candidate {
                    generation: candidate_generation,
                    ..
                }),
        } = *self
        else {
            return false;
        };
        if candidate_generation != generation {
            return false;
        }
        *self = Self::Committed {
            generation,
            attempt,
            deadline_ms,
            service_phase: DeferredGate8ServicePhase::AwaitingDhcp,
        };
        true
    }

    /// Admit exactly one child handoff from a DHCP-producing control turn.
    ///
    /// `control_turn_started_ms` is sampled before the bounded network-control
    /// turn. A turn that starts at or after the original deadline is late; a
    /// turn that starts before it may publish DHCP and schedule exactly one
    /// exclusive handoff even if its post-turn clock sample crosses the bound.
    fn authorize_handoff(
        &mut self,
        control_turn_started_ms: u64,
        observed_generation: u32,
        generation_operational: bool,
        recovery_required: bool,
        dhcp_pending: bool,
        response_bound_ms: Option<u64>,
    ) -> DeferredGate8HandoffAdmission {
        let Self::Committed {
            generation,
            deadline_ms,
            service_phase,
            ..
        } = self
        else {
            return DeferredGate8HandoffAdmission::InvalidPhase;
        };
        if *service_phase != DeferredGate8ServicePhase::AwaitingDhcp {
            return DeferredGate8HandoffAdmission::InvalidPhase;
        }
        if recovery_required {
            return DeferredGate8HandoffAdmission::RecoveryRequired;
        }
        if !dhcp_pending {
            return DeferredGate8HandoffAdmission::NotPending;
        }
        if !generation_operational || observed_generation != *generation {
            return DeferredGate8HandoffAdmission::ProofDrift {
                generation: *generation,
            };
        }
        if control_turn_started_ms >= *deadline_ms {
            return DeferredGate8HandoffAdmission::Deadline {
                generation: *generation,
                deadline_ms: *deadline_ms,
            };
        }
        let Some(response_bound_ms) = response_bound_ms.filter(|bound| *bound != 0) else {
            return DeferredGate8HandoffAdmission::InvalidAuthority {
                generation: *generation,
            };
        };
        *service_phase = DeferredGate8ServicePhase::HandoffAuthorized { response_bound_ms };
        DeferredGate8HandoffAdmission::Authorized {
            generation: *generation,
        }
    }

    const fn handoff_authorization_pending(self) -> bool {
        matches!(
            self,
            Self::Committed {
                service_phase: DeferredGate8ServicePhase::HandoffAuthorized { .. },
                ..
            }
        )
    }

    const fn child_ready_wait_pending(self) -> bool {
        matches!(
            self,
            Self::Committed {
                service_phase: DeferredGate8ServicePhase::AwaitingReady { .. },
                ..
            }
        )
    }

    fn revalidate_handoff(
        &mut self,
        observed_generation: u32,
        generation_operational: bool,
        recovery_required: bool,
        dhcp_pending: bool,
    ) -> DeferredGate8HandoffRevalidation {
        let Self::Committed {
            generation,
            service_phase,
            ..
        } = self
        else {
            return DeferredGate8HandoffRevalidation::InvalidPhase;
        };
        if !matches!(
            *service_phase,
            DeferredGate8ServicePhase::HandoffAuthorized { .. }
        ) {
            return DeferredGate8HandoffRevalidation::InvalidPhase;
        }
        let outcome = if recovery_required {
            DeferredGate8HandoffRevalidation::RecoveryRequired
        } else if !dhcp_pending {
            DeferredGate8HandoffRevalidation::NotPending
        } else if !generation_operational || observed_generation != *generation {
            DeferredGate8HandoffRevalidation::ProofDrift {
                generation: *generation,
            }
        } else {
            return DeferredGate8HandoffRevalidation::Authorized {
                generation: *generation,
            };
        };
        *service_phase = DeferredGate8ServicePhase::AwaitingDhcp;
        outcome
    }

    fn prepare_child_ready_wait(
        self,
        pre_resume_clock: DeferredGate8AbsoluteClockSample,
        generation: u32,
    ) -> DeferredGate8ChildReadyPreparation {
        let Self::Committed {
            generation: committed_generation,
            service_phase,
            ..
        } = self
        else {
            return DeferredGate8ChildReadyPreparation::InvalidPhase;
        };
        if committed_generation != generation {
            return DeferredGate8ChildReadyPreparation::GenerationDrift;
        }
        let response_bound_ms = match service_phase {
            DeferredGate8ServicePhase::HandoffAuthorized { response_bound_ms } => response_bound_ms,
            DeferredGate8ServicePhase::AwaitingReady { .. } | DeferredGate8ServicePhase::Ready => {
                return DeferredGate8ChildReadyPreparation::AlreadyArmed;
            }
            DeferredGate8ServicePhase::AwaitingDhcp => {
                return DeferredGate8ChildReadyPreparation::InvalidPhase;
            }
        };
        let Some(publication_not_before_ms) = pre_resume_clock.absolute_ms() else {
            return DeferredGate8ChildReadyPreparation::ClockInvalid;
        };
        if publication_not_before_ms
            .checked_add(response_bound_ms)
            .is_none()
        {
            return DeferredGate8ChildReadyPreparation::ClockOverflow;
        }
        DeferredGate8ChildReadyPreparation::Prepared(DeferredGate8ChildReadyPlan {
            generation,
            response_bound_ms,
            publication_not_before_ms,
            counter_hz: pre_resume_clock.counter_hz,
        })
    }

    fn arm_child_ready_wait(
        &mut self,
        plan: DeferredGate8ChildReadyPlan,
        post_resume_clock: DeferredGate8AbsoluteClockSample,
        observed_generation: u32,
        generation_operational: bool,
        recovery_required: bool,
    ) -> DeferredGate8ChildReadyArm {
        if recovery_required {
            return DeferredGate8ChildReadyArm::RecoveryRequired;
        }
        if !generation_operational || observed_generation != plan.generation {
            return DeferredGate8ChildReadyArm::GenerationDrift;
        }
        let Self::Committed {
            generation,
            service_phase,
            ..
        } = self
        else {
            return DeferredGate8ChildReadyArm::InvalidPhase;
        };
        if *generation != plan.generation {
            return DeferredGate8ChildReadyArm::GenerationDrift;
        }
        if post_resume_clock.counter_hz != plan.counter_hz {
            return DeferredGate8ChildReadyArm::ClockInvalid;
        }
        let Some(post_resume_ms) = post_resume_clock.absolute_ms() else {
            return DeferredGate8ChildReadyArm::ClockInvalid;
        };
        if post_resume_ms < plan.publication_not_before_ms {
            return DeferredGate8ChildReadyArm::ClockInvalid;
        }
        let Some(deadline_ms) = post_resume_ms.checked_add(plan.response_bound_ms) else {
            return DeferredGate8ChildReadyArm::ClockOverflow;
        };
        match *service_phase {
            DeferredGate8ServicePhase::HandoffAuthorized { response_bound_ms }
                if response_bound_ms == plan.response_bound_ms =>
            {
                *service_phase = DeferredGate8ServicePhase::AwaitingReady {
                    publication_not_before_ms: plan.publication_not_before_ms,
                    deadline_ms,
                    deadline_observation_taken: false,
                };
                DeferredGate8ChildReadyArm::Armed { deadline_ms }
            }
            DeferredGate8ServicePhase::AwaitingReady { .. } | DeferredGate8ServicePhase::Ready => {
                DeferredGate8ChildReadyArm::AlreadyArmed
            }
            DeferredGate8ServicePhase::AwaitingDhcp
            | DeferredGate8ServicePhase::HandoffAuthorized { .. } => {
                DeferredGate8ChildReadyArm::InvalidPhase
            }
        }
    }

    fn mark_service_ready(&mut self, generation: u32, published_ms: u64) -> bool {
        let Self::Committed {
            generation: committed_generation,
            service_phase,
            ..
        } = self
        else {
            return false;
        };
        let DeferredGate8ServicePhase::AwaitingReady {
            publication_not_before_ms,
            deadline_ms,
            ..
        } = *service_phase
        else {
            return false;
        };
        if *committed_generation != generation
            || !child_ready_publication_in_window(
                publication_not_before_ms,
                deadline_ms,
                published_ms,
            )
        {
            return false;
        }
        *service_phase = DeferredGate8ServicePhase::Ready;
        true
    }

    fn child_ready_publication_on_time(self, generation: u32, published_ms: Option<u64>) -> bool {
        matches!(
            (self, published_ms),
            (
                Self::Committed {
                    generation: committed_generation,
                    service_phase:
                        DeferredGate8ServicePhase::AwaitingReady {
                            publication_not_before_ms,
                            deadline_ms,
                            ..
                        },
                    ..
                },
                Some(published_ms),
            ) if committed_generation == generation
                && child_ready_publication_in_window(
                    publication_not_before_ms,
                    deadline_ms,
                    published_ms,
                )
        )
    }

    fn service_readiness_deadline_expired(
        self,
        generation: u32,
        policy_now_ms: u64,
        child_ready_now_ms: Option<u64>,
    ) -> Option<u64> {
        match self {
            Self::Committed {
                generation: committed_generation,
                deadline_ms,
                service_phase,
                ..
            } if committed_generation == generation => match service_phase {
                DeferredGate8ServicePhase::AwaitingDhcp
                | DeferredGate8ServicePhase::HandoffAuthorized { .. }
                    if policy_now_ms >= deadline_ms =>
                {
                    Some(deadline_ms)
                }
                DeferredGate8ServicePhase::AwaitingReady {
                    deadline_ms: child_ready_deadline_ms,
                    ..
                } if child_ready_now_ms.is_none_or(|now_ms| now_ms >= child_ready_deadline_ms) => {
                    Some(child_ready_deadline_ms)
                }
                DeferredGate8ServicePhase::AwaitingDhcp
                | DeferredGate8ServicePhase::HandoffAuthorized { .. }
                | DeferredGate8ServicePhase::AwaitingReady { .. }
                | DeferredGate8ServicePhase::Ready => None,
            },
            Self::Detached | Self::Stabilizing { .. } | Self::Committed { .. } => None,
        }
    }

    fn child_ready_pre_network_action(
        &mut self,
        now_ms: Option<u64>,
        observed_generation: u32,
        generation_operational: bool,
        recovery_required: bool,
        ready_published_ms: Option<u64>,
    ) -> DeferredGate8ChildReadyPreNetworkAction {
        if recovery_required {
            return DeferredGate8ChildReadyPreNetworkAction::Continue;
        }
        let Self::Committed {
            generation,
            service_phase:
                DeferredGate8ServicePhase::AwaitingReady {
                    publication_not_before_ms,
                    deadline_ms,
                    deadline_observation_taken,
                },
            ..
        } = self
        else {
            return DeferredGate8ChildReadyPreNetworkAction::Continue;
        };
        let Some(now_ms) = now_ms else {
            return DeferredGate8ChildReadyPreNetworkAction::Fail {
                generation: *generation,
                blocker: "child-ready-clock-invalid",
                deadline_ms: *deadline_ms,
            };
        };
        if !generation_operational || observed_generation != *generation {
            return DeferredGate8ChildReadyPreNetworkAction::Fail {
                generation: *generation,
                blocker: "child-ready-generation-drift",
                deadline_ms: now_ms,
            };
        }
        if now_ms < *deadline_ms {
            return DeferredGate8ChildReadyPreNetworkAction::Continue;
        }
        if ready_published_ms.is_some_and(|published_ms| {
            child_ready_publication_in_window(
                *publication_not_before_ms,
                *deadline_ms,
                published_ms,
            )
        }) {
            return DeferredGate8ChildReadyPreNetworkAction::Continue;
        }
        if !*deadline_observation_taken {
            *deadline_observation_taken = true;
            return DeferredGate8ChildReadyPreNetworkAction::ObservePublicationOnly {
                generation: *generation,
                deadline_ms: *deadline_ms,
            };
        }
        DeferredGate8ChildReadyPreNetworkAction::Fail {
            generation: *generation,
            blocker: "service-readiness-deadline",
            deadline_ms: *deadline_ms,
        }
    }

    fn child_ready_post_observation_failure(
        self,
        generation: u32,
        ready_published_ms: Option<u64>,
    ) -> Option<(u32, &'static str, u64)> {
        let Self::Committed {
            generation: committed_generation,
            service_phase:
                DeferredGate8ServicePhase::AwaitingReady {
                    publication_not_before_ms,
                    deadline_ms,
                    ..
                },
            ..
        } = self
        else {
            return Some((generation, "child-ready-state-invalid", 0));
        };
        if committed_generation != generation {
            return Some((
                committed_generation,
                "child-ready-generation-drift",
                deadline_ms,
            ));
        }
        (!ready_published_ms.is_some_and(|published_ms| {
            child_ready_publication_in_window(publication_not_before_ms, deadline_ms, published_ms)
        }))
        .then_some((generation, "service-readiness-deadline", deadline_ms))
    }

    fn deadline_ms(self) -> Option<u64> {
        match self {
            Self::Detached => None,
            Self::Stabilizing { deadline_ms, .. } => Some(deadline_ms),
            Self::Committed {
                deadline_ms,
                service_phase,
                ..
            } => Some(match service_phase {
                DeferredGate8ServicePhase::AwaitingReady {
                    deadline_ms: child_ready_deadline_ms,
                    ..
                } => child_ready_deadline_ms,
                DeferredGate8ServicePhase::AwaitingDhcp
                | DeferredGate8ServicePhase::HandoffAuthorized { .. }
                | DeferredGate8ServicePhase::Ready => deadline_ms,
            }),
        }
    }
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn emit_deferred_net_operator_line<
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
    line: &str,
    raw_fallback_allowed: bool,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    if !pump.queue_cyw43_bootstrap_operator_line(line) {
        if raw_fallback_allowed {
            boot_log::force_uart_line_raw_and_log(line);
        } else if !crate::serial::serial_linked_runtime_transport_active() {
            // Root UART ownership has already crossed the irreversible
            // linked-runtime boundary. Preserve diagnostics in qlog without
            // reacquiring UART MMIO or inventing a fallback transport.
            crate::log_buffer::append_log_line(line);
        }
    }
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeferredNetSupervisorStatus {
    Preflight,
    Begin,
    Recovery,
    Stabilizing,
    Ready,
    Failed,
    Permanent,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
impl DeferredNetSupervisorStatus {
    const ALL: [Self; 7] = [
        Self::Preflight,
        Self::Begin,
        Self::Recovery,
        Self::Stabilizing,
        Self::Ready,
        Self::Failed,
        Self::Permanent,
    ];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::Begin => "begin",
            Self::Recovery => "recovery",
            Self::Stabilizing => "stabilizing",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Permanent => "permanent",
        }
    }

    const fn valid_attempt(self, attempt: u32) -> bool {
        match self {
            Self::Preflight => attempt == 0,
            _ => attempt == CYW43_BOOTSTRAP_ATTEMPT,
        }
    }

    const fn releases_hdmi_console_ready(self) -> bool {
        // Gate 8 commit is a separate nonterminal record. Supervisor Ready is
        // now the same-generation DHCP/listener terminal; failures likewise
        // release the local console with explicit unavailable feedback.
        matches!(self, Self::Ready | Self::Failed | Self::Permanent)
    }
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const fn deferred_cyw43_supervisor_start_status(
    network_attached: bool,
    service_ready_published: bool,
) -> Option<DeferredNetSupervisorStatus> {
    match (network_attached, service_ready_published) {
        (false, _) => Some(DeferredNetSupervisorStatus::Begin),
        (true, true) => Some(DeferredNetSupervisorStatus::Recovery),
        // Before exact DHCP/listener service readiness the supervisor may
        // drain, fence, and poison an uncertain owner, but the zero restart
        // budget forbids calling that terminal cleanup a recovery episode.
        (true, false) => None,
    }
}

#[cfg(all(
    test,
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
pub(crate) fn format_deferred_net_bootstrap_supervisor_status(
    console_sequence: u64,
    attempt: u32,
    status: DeferredNetSupervisorStatus,
    backoff_ms: u64,
    next_attempt_ms: u64,
    serial_ready: bool,
    local_seat_enabled: bool,
) -> Option<HeaplessString<DEFAULT_LINE_CAPACITY>> {
    let semantic = format_deferred_net_bootstrap_supervisor_semantic_status(
        attempt,
        status,
        backoff_ms,
        next_attempt_ms,
        serial_ready,
        local_seat_enabled,
    )?;
    format_deferred_net_bootstrap_supervisor_linked_route(semantic.as_str(), console_sequence)
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn format_deferred_net_bootstrap_supervisor_semantic_status(
    attempt: u32,
    status: DeferredNetSupervisorStatus,
    backoff_ms: u64,
    next_attempt_ms: u64,
    serial_ready: bool,
    local_seat_enabled: bool,
) -> Option<HeaplessString<DEFAULT_LINE_CAPACITY>> {
    if !status.valid_attempt(attempt) {
        return None;
    }
    let semantic_backoff_ms = if matches!(status, DeferredNetSupervisorStatus::Preflight) {
        backoff_ms
    } else {
        0
    };
    let mut line = HeaplessString::new();
    if write!(
        line,
        "CYW43_BOOTSTRAP_SUPERVISOR attempt={} status={} backoff_ms={} next_attempt_ms={} serial={} local_seat={} recovery=full",
        attempt,
        status.as_str(),
        semantic_backoff_ms,
        next_attempt_ms,
        if serial_ready { "ready" } else { "blocked" },
        if local_seat_enabled {
            "enabled"
        } else {
            "disabled"
        },
    )
    .is_err()
    {
        return None;
    }
    Some(line)
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
pub(crate) fn format_deferred_net_bootstrap_supervisor_display_status(
    attempt: u32,
    status: DeferredNetSupervisorStatus,
    _backoff_ms: u64,
    serial_ready: bool,
) -> Option<HeaplessString<DEFAULT_LINE_CAPACITY>> {
    if !status.valid_attempt(attempt) {
        return None;
    }
    let mut line = HeaplessString::new();
    let failed = match status {
        DeferredNetSupervisorStatus::Preflight if serial_ready => {
            line.push_str("[drivers] WiFi bootstrap pending; operator diagnostics available")
                .is_err()
        }
        DeferredNetSupervisorStatus::Preflight => {
            line.push_str("[drivers] WiFi waiting for safe serial path; bootstrap paused")
                .is_err()
        }
        DeferredNetSupervisorStatus::Begin => line
            .push_str("[drivers] WiFi bootstrap starting (single production attempt)")
            .is_err(),
        DeferredNetSupervisorStatus::Recovery => line
            .push_str("[drivers] WiFi restoring previously ready CYW43/SDIO service")
            .is_err(),
        DeferredNetSupervisorStatus::Stabilizing => line
            .push_str("[drivers] WiFi transport attached; Gate 8 association security stabilizing")
            .is_err(),
        DeferredNetSupervisorStatus::Ready => line
            .push_str("[drivers] WiFi ready to use: DHCP bound; TCP console listening")
            .is_err(),
        DeferredNetSupervisorStatus::Failed => line
            .push_str("[drivers] WiFi startup failed; diagnostics remain active")
            .is_err(),
        DeferredNetSupervisorStatus::Permanent => line
            .push_str(
                "[drivers] WiFi unavailable: non-retryable startup failure; diagnostics remain active",
            )
            .is_err(),
    };
    if failed {
        return None;
    }
    Some(line)
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn format_deferred_net_bootstrap_supervisor_linked_route(
    semantic: &str,
    console_sequence: u64,
) -> Option<HeaplessString<DEFAULT_LINE_CAPACITY>> {
    let mut line = HeaplessString::new();
    if line.push_str(semantic).is_err() {
        return None;
    }
    if write!(
        line,
        " console_seq={} telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes",
        console_sequence,
    )
    .is_err()
    {
        return None;
    }
    Some(line)
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn format_deferred_net_gate8_subgate(
    pair_scrub_epoch: u64,
    generation: u32,
    subgate: crate::drivers::driver_task_net::Cyw43Gate8SubgateDiagnostic,
) -> Option<HeaplessString<DEFAULT_LINE_CAPACITY>> {
    let mut line = HeaplessString::new();
    if write!(
        line,
        "wifi: gate 8 subgate={} status={} pair_epoch={} generation={} blocker={}",
        subgate.token,
        subgate.status.as_str(),
        pair_scrub_epoch,
        generation,
        subgate.blocker,
    )
    .is_err()
    {
        return None;
    }
    Some(line)
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn format_deferred_net_gate8_snapshot_lines(
    diagnostic: crate::drivers::driver_task_net::Cyw43Gate8Diagnostic,
) -> Option<heapless::Vec<HeaplessString<DEFAULT_LINE_CAPACITY>, 8>> {
    let mut lines = heapless::Vec::<HeaplessString<DEFAULT_LINE_CAPACITY>, 8>::new();
    for subgate in diagnostic.subgates {
        let line = format_deferred_net_gate8_subgate(
            diagnostic.pair_scrub_epoch,
            diagnostic.generation,
            subgate,
        )?;
        lines.push(line).ok()?;
    }
    Some(lines)
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn format_deferred_net_gate8_commit(
    diagnostic: crate::drivers::driver_task_net::Cyw43Gate8Diagnostic,
    attempt: u32,
    deadline_ms: u64,
    console_sequence: u64,
) -> Option<HeaplessString<DEFAULT_LINE_CAPACITY>> {
    let mut line = HeaplessString::new();
    if write!(
        line,
        "CYW43_GATE8_COMMIT attempt={} status=ready pair_epoch={} generation={} deadline_ms={} console_seq={} telemetry_sinks=serial+qlog+hdmi consumer=data",
        attempt,
        diagnostic.pair_scrub_epoch,
        diagnostic.generation,
        deadline_ms,
        console_sequence,
    )
    .is_err()
    {
        return None;
    }
    Some(line)
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn emit_deferred_net_gate8_commit_transaction<
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
    diagnostic: crate::drivers::driver_task_net::Cyw43Gate8Diagnostic,
    console_sequence: u64,
    attempt: u32,
    deadline_ms: u64,
) -> bool
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    if !diagnostic.stable() {
        return false;
    }
    let Some(lines) = format_deferred_net_gate8_snapshot_lines(diagnostic) else {
        return false;
    };
    let Some(commit) =
        format_deferred_net_gate8_commit(diagnostic, attempt, deadline_ms, console_sequence)
    else {
        return false;
    };
    pump.queue_cyw43_gate8_commit_transaction(
        diagnostic.pair_scrub_epoch,
        diagnostic.generation,
        lines.as_slice(),
        commit.as_str(),
        crate::local_seat::CYW43_GATE8_READY_HDMI_LINE,
    )
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn emit_deferred_net_gate8_failure_transaction<
    'a,
    D,
    T,
    I,
    V,
    const RX: usize,
    const TX: usize,
    const LINE: usize,
    Decision,
>(
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
    diagnostic: crate::drivers::driver_task_net::Cyw43Gate8Diagnostic,
    terminal_line: &str,
    decide: Decision,
) -> Cyw43BootstrapAtomicDecisionOutcome
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
    Decision: FnOnce() -> bool,
{
    let Some(snapshot_lines) = format_deferred_net_gate8_snapshot_lines(diagnostic) else {
        return Cyw43BootstrapAtomicDecisionOutcome::PreflightBlocked;
    };
    let mut lines = heapless::Vec::<HeaplessString<DEFAULT_LINE_CAPACITY>, 9>::new();
    for line in snapshot_lines {
        if lines.push(line).is_err() {
            return Cyw43BootstrapAtomicDecisionOutcome::PreflightBlocked;
        }
    }
    let mut terminal = HeaplessString::new();
    if terminal.push_str(terminal_line).is_err() || lines.push(terminal).is_err() {
        return Cyw43BootstrapAtomicDecisionOutcome::PreflightBlocked;
    }
    // Formatting and retained capacity are proven before the caller takes its
    // final recovery-versus-terminal decision. Once that explicit cut commits,
    // the complete causal batch is appended immediately and reserves the
    // following Permanent supervisor slot.
    pump.queue_cyw43_bootstrap_operator_lines_atomic_with_decision(lines.as_slice(), 1, decide)
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn emit_deferred_net_bootstrap_supervisor_status<
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
    console_sequence: u64,
    attempt: u32,
    status: DeferredNetSupervisorStatus,
    backoff_ms: u64,
    next_attempt_ms: u64,
    local_seat_enabled: bool,
    raw_fallback_allowed: bool,
) -> bool
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    let serial_ready = !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
        || crate::serial::serial_linked_runtime_transport_active();
    let Some(semantic) = format_deferred_net_bootstrap_supervisor_semantic_status(
        attempt,
        status,
        backoff_ms,
        next_attempt_ms,
        serial_ready,
        local_seat_enabled,
    ) else {
        emit_deferred_net_operator_line(
            pump,
            "CYW43_BOOTSTRAP_STATUS_FORMAT_ERROR action=operator-diagnostics acceptance=red",
            raw_fallback_allowed,
        );
        return false;
    };
    let Some(linked_line) =
        format_deferred_net_bootstrap_supervisor_linked_route(semantic.as_str(), console_sequence)
    else {
        emit_deferred_net_operator_line(
            pump,
            "CYW43_BOOTSTRAP_STATUS_FORMAT_ERROR action=operator-diagnostics acceptance=red",
            raw_fallback_allowed,
        );
        return false;
    };
    let Some(display_line) = format_deferred_net_bootstrap_supervisor_display_status(
        attempt,
        status,
        backoff_ms,
        serial_ready,
    ) else {
        emit_deferred_net_operator_line(
            pump,
            "CYW43_BOOTSTRAP_STATUS_FORMAT_ERROR action=operator-diagnostics acceptance=red",
            raw_fallback_allowed,
        );
        return false;
    };
    // Preserve the full-fidelity record without a logger prefix. Once the
    // linked route is active this is enqueue-only; its next ordinary operator
    // turn performs the flush, separately from any CYW43 operation. Before
    // cutover, pass only the semantic payload to the raw route because that
    // route appends its own ordering suffix.
    let queued = pump.queue_cyw43_bootstrap_supervisor_status_with_terminal(
        linked_line.as_str(),
        display_line.as_str(),
        status.releases_hdmi_console_ready(),
    );
    if queued {
        return true;
    }
    if raw_fallback_allowed {
        let raw_console_sequence = match u32::try_from(console_sequence) {
            Ok(sequence) => sequence,
            Err(_) => u32::MAX,
        };
        boot_log::force_uart_line_raw_and_log_with_console_seq(
            semantic.as_str(),
            raw_console_sequence,
        );
        return true;
    }
    if !crate::serial::serial_linked_runtime_transport_active() {
        // The released generation must never reclaim raw UART. Keep the
        // exact sequenced record available to authenticated diagnostics.
        crate::log_buffer::append_log_line(linked_line.as_str());
    }
    false
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn emit_deferred_net_boot_service_ready_transaction<
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
    console_sequence: u64,
    generation: u32,
    now_ms: u64,
    local_seat_enabled: bool,
) -> bool
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    let serial_ready = !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
        || crate::serial::serial_linked_runtime_transport_active();
    let Some(semantic) = format_deferred_net_bootstrap_supervisor_semantic_status(
        CYW43_BOOTSTRAP_ATTEMPT,
        DeferredNetSupervisorStatus::Ready,
        0,
        now_ms,
        serial_ready,
        local_seat_enabled,
    ) else {
        return false;
    };
    let Some(linked_line) =
        format_deferred_net_bootstrap_supervisor_linked_route(semantic.as_str(), console_sequence)
    else {
        return false;
    };
    pump.queue_cyw43_service_ready_transaction(generation, linked_line.as_str())
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn emit_deferred_net_runtime_service_ready_transaction<
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
    console_sequence: u64,
    generation: u32,
) -> bool
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    let Some(line) = format_deferred_net_runtime_service_ready(generation, console_sequence) else {
        return false;
    };
    pump.queue_cyw43_service_ready_transaction(generation, line.as_str())
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn format_deferred_net_runtime_service_ready(
    generation: u32,
    console_sequence: u64,
) -> Option<HeaplessString<DEFAULT_LINE_CAPACITY>> {
    let mut line = HeaplessString::new();
    if write!(
        line,
        "CYW43_RUNTIME_RECOVERY status=ready generation={} console_seq={} telemetry_sinks=serial+qlog+hdmi",
        generation, console_sequence,
    )
    .is_err()
    {
        return None;
    }
    Some(line)
}

#[cfg(all(feature = "serial-console", feature = "kernel"))]
fn with_deferred_root_hal<R>(
    hal_ptr: usize,
    operation: impl FnOnce(&mut KernelHal<'static>) -> R,
) -> Option<R> {
    let hal_ptr = core::ptr::NonNull::new(hal_ptr as *mut KernelHal<'static>)?;
    // SAFETY: kernel bootstrap leaks this `KernelHal` for the root-task
    // lifetime. Root-control is single-threaded and invokes service-owner
    // containment only from an exclusive pre-pump recovery turn; deferred
    // bootstrap likewise starts after the prior outer turn has returned. Each
    // mutable borrow is bounded to this closure and never overlaps the
    // EventPump or another child-backend operation.
    Some(unsafe { operation(&mut *hal_ptr.as_ptr()) })
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
fn enter_root_console_loop_with_deferred_net_supervisor<
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
    config: crate::net::ConsoleNetConfig,
    #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
    mut net_deferred_console_runtime: Option<
        crate::hal::console_network::ConsoleNetworkRuntime,
    >,
    ctx: &BootContext,
) -> !
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    if crate::serial::serial_linked_runtime_transport_active() {
        for line in [
            "[boot] TimersAndIPC: root-console.start.ok",
            "[boot] TimersAndIPC: queen.start.begin",
            "[boot] root shell started; entering event loop",
            "[boot] TimersAndIPC: queen.start.ok",
            "[boot] phase: TimersAndIPC.end",
        ] {
            let _ = pump.queue_cyw43_bootstrap_operator_line(line);
        }
        boot_log::allow_ep_only_transport();
    } else {
        announce_root_console_loop_start();
    }
    activate_root_control_temporal_or_fail(ctx);
    let hal_ptr = ctx.wifi_debug_hal_ptr;
    let local_seat_enabled = crate::generated::hardware_config().local_seat.enabled;
    let mut supervisor_sequence = DeferredNetSupervisorSequence::new();
    if hal_ptr == 0 {
        let mut detail = HeaplessString::<192>::new();
        let _ = detail.push_str("deferred HAL pointer missing");
        emit_deferred_net_console_failure(pump, &detail, true);
        emit_deferred_net_bootstrap_supervisor_status(
            pump,
            supervisor_sequence.next_status_sequence(),
            CYW43_BOOTSTRAP_ATTEMPT,
            DeferredNetSupervisorStatus::Permanent,
            0,
            crate::hal::timebase().now_ms(),
            local_seat_enabled,
            true,
        );
        run_root_console_pump(pump);
    }
    let config = match crate::net::prepare_cyw43_net_console_config(config) {
        Ok(config) => config,
        Err(err) => {
            let mut detail = HeaplessString::<192>::new();
            let _ = write!(detail, "{err}");
            emit_deferred_net_console_failure(pump, &detail, true);
            emit_deferred_net_bootstrap_supervisor_status(
                pump,
                supervisor_sequence.next_status_sequence(),
                CYW43_BOOTSTRAP_ATTEMPT,
                DeferredNetSupervisorStatus::Permanent,
                0,
                crate::hal::timebase().now_ms(),
                local_seat_enabled,
                true,
            );
            run_root_console_pump(pump);
        }
    };
    crate::drivers::driver_task_net::begin_cyw43_bootstrap_causal_fault_capture();
    let mut bootstrap = Cyw43BootstrapSupervisor::new(config);
    bootstrap.enforce_single_bootstrap_pair();
    let mut attempt_active = false;
    let mut network_attached = false;
    let mut gate8_lifecycle = DeferredGate8Lifecycle::new();
    let mut bootstrap_service_ready_published = false;
    let mut terminal_mode = None;
    let mut gate8_terminal_pending: Option<DeferredGate8TerminalPending> = None;
    let mut recovery_diagnostic_pending: Option<DeferredCyw43RecoveryDiagnosticPending> = None;
    let mut wifi_operation_started = false;
    let mut serial_retry = DeferredSerialRouteRetry::new(crate::hal::timebase().now_ms());
    let mut turn_status = DeferredCyw43TurnStatus::new();
    let mut supervisor_phase = DeferredCyw43SupervisorPhase::Operator;
    let mut activation_window = DeferredCyw43ActivationWindow::new();
    let activation_reserve_us = deferred_cyw43_activation_reserve_from_manifest_us();

    'supervisor: loop {
        activation_window.begin(monotonic_ticks(), counter_frequency());
        if activation_window.logical_turns != 0
            && !activation_window.turn_admitted(
                monotonic_ticks(),
                counter_frequency(),
                activation_reserve_us,
            )
        {
            deferred_cyw43_yield_and_reset(&mut activation_window);
            continue;
        }
        if let Some(pending) = recovery_diagnostic_pending.as_ref() {
            if !pump.queue_cyw43_pair_recovery_diagnostic_transaction(
                pending.recovery,
                pending.live_generation,
                pending.operator_line.as_str(),
            ) {
                // This output-only turn releases retained serial capacity. Do
                // not enter generation poison until the complete causal batch
                // has a teardown-independent home.
                pump.poll_cyw43_bootstrap_supervisor_event_turn();
                deferred_cyw43_yield_and_reset(&mut activation_window);
                continue;
            }
            recovery_diagnostic_pending = None;
        }

        if let Some(mut pending) = gate8_terminal_pending {
            // Capacity and formatting are proven without mutation before the
            // final typed-recovery probe. If that probe is clear, the explicit
            // terminal decision cut commits immediately before atomic batch
            // retention. No later child operation may run while the batch or
            // its adjacent Permanent status waits for output service.
            if gate8_terminal_pending_yields_to_finite_owner(
                pending.terminal_decision_committed,
                crate::drivers::driver_task_net::cyw43_finite_lifecycle_cut_blocking(),
            ) {
                // A sequence-last finite terminal committed before the cut is
                // part of the already-admitted bus transaction. Give the
                // ordinary EventPump Network phase another bounded turn to
                // retire that exact owner; do not reset the Gate 8 deadline or
                // admit any fresh operation.
                gate8_terminal_pending = None;
                deferred_cyw43_yield_and_reset(&mut activation_window);
                continue;
            }
            if gate8_terminal_pending_cancels_for_recovery(
                pending.terminal_decision_committed,
                crate::drivers::driver_task_net::cyw43_recovery_required(),
            ) {
                gate8_terminal_pending = None;
                deferred_cyw43_yield_and_reset(&mut activation_window);
                continue;
            }
            if !pending.failure_transaction_retained {
                let mut line = HeaplessString::<224>::new();
                let _ = write!(
                    line,
                    "CYW43_GATE8_TERMINAL attempt={} generation={} blocker={} deadline_ms={} action=quarantine",
                    pending.attempt,
                    pending.generation,
                    pending.blocker,
                    pending.deadline_ms,
                );
                let outcome = emit_deferred_net_gate8_failure_transaction(
                    pump,
                    pending.diagnostic,
                    line.as_str(),
                    || {
                        commit_gate8_terminal_decision(
                            &mut pending.terminal_decision_committed,
                            pending.generation,
                            crate::drivers::driver_task_net::cyw43_recovery_required(),
                            crate::drivers::driver_task_net::cyw43_finite_lifecycle_cut_blocking(),
                            crate::drivers::driver_task_net::retract_cyw43_gate8_data_consumer,
                        )
                    },
                );
                match outcome {
                    Cyw43BootstrapAtomicDecisionOutcome::PreflightBlocked => {
                        gate8_terminal_pending = Some(pending);
                        crate::log_buffer::append_log_line(
                            "CYW43_GATE8_FAILURE_TRANSACTION status=preflight-blocked action=wait-for-serial-capacity",
                        );
                        pump.poll_cyw43_bootstrap_supervisor_event_turn();
                        deferred_cyw43_yield_and_reset(&mut activation_window);
                        continue;
                    }
                    Cyw43BootstrapAtomicDecisionOutcome::DecisionDeclined => {
                        gate8_terminal_pending = None;
                        deferred_cyw43_yield_and_reset(&mut activation_window);
                        continue;
                    }
                    Cyw43BootstrapAtomicDecisionOutcome::Retained => {
                        debug_assert!(pending.terminal_decision_committed);
                        pending.failure_transaction_retained = true;
                        gate8_terminal_pending = Some(pending);
                    }
                    Cyw43BootstrapAtomicDecisionOutcome::RetentionInvariantFailed => {
                        debug_assert!(pending.terminal_decision_committed);
                        gate8_terminal_pending = Some(pending);
                        crate::log_buffer::append_log_line(
                            "CYW43_GATE8_FAILURE_TRANSACTION status=retention-invariant-failed action=preserve-terminal-decision",
                        );
                        pump.poll_cyw43_bootstrap_supervisor_event_turn();
                        deferred_cyw43_yield_and_reset(&mut activation_window);
                        continue;
                    }
                }
            }
            if !emit_deferred_net_bootstrap_supervisor_status(
                pump,
                pending.status_sequence,
                pending.attempt,
                DeferredNetSupervisorStatus::Permanent,
                0,
                crate::hal::timebase().now_ms(),
                local_seat_enabled,
                false,
            ) {
                pump.poll_cyw43_bootstrap_supervisor_event_turn();
                deferred_cyw43_yield_and_reset(&mut activation_window);
                continue;
            }
            pump.quarantine_network_service_after_cyw43_terminal_failure();
            gate8_terminal_pending = None;
            terminal_mode = Some(DeferredNetSupervisorTerminal::PermanentAttachedWifiFailure);
            attempt_active = false;
            deferred_cyw43_yield_and_reset(&mut activation_window);
            continue;
        }

        if gate8_terminal_pending.is_some() {
            // The branch above is exhaustive today. Keep this fail-closed
            // guard so a future retained-output branch cannot accidentally
            // fall through to ordinary NetStack/CYW43 polling.
            pump.poll_cyw43_bootstrap_supervisor_event_turn();
            deferred_cyw43_yield_and_reset(&mut activation_window);
            continue;
        }

        if !deferred_net_supervisor_driver_turn_allowed(terminal_mode) {
            // A finite failed Wi-Fi episode must not become a second boot
            // failure. Ordinary EventPump ownership keeps serial, local-seat,
            // HDMI, diagnostics, authentication, and reboot live, while the
            // terminal state prevents another child operation. An attached
            // poisoned stack was quarantined before this mode was entered.
            if pump.poll_root_control_quantum() {
                deferred_cyw43_yield_and_reset(&mut activation_window);
            }
            continue;
        }

        if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
            && !crate::serial::serial_linked_runtime_transport_active()
        {
            let serial_now_ms = crate::hal::timebase().now_ms();
            if wifi_operation_started {
                // Once any Wi-Fi child operation has run, never re-enter the
                // generic/current-TCB UART or USB/HDMI paths. This linked-only
                // turn retains timer, IPC, and reboot dispatch. A lost linked
                // serial generation fails closed and is never replaced by root
                // UART MMIO.
                pump.poll_cyw43_bootstrap_supervisor_event_turn();
            } else {
                let report_due = serial_retry.probe_due(serial_now_ms);
                match pump.poll_serial_linked_runtime_cutover_after_prompt() {
                    crate::event::SerialLinkedRuntimeCutoverTurn::Complete => {
                        serial_retry.record_ready();
                        emit_deferred_net_bootstrap_supervisor_status(
                            pump,
                            supervisor_sequence.next_status_sequence(),
                            0,
                            DeferredNetSupervisorStatus::Preflight,
                            0,
                            serial_now_ms,
                            local_seat_enabled,
                            false,
                        );
                    }
                    crate::event::SerialLinkedRuntimeCutoverTurn::DrainPending
                    | crate::event::SerialLinkedRuntimeCutoverTurn::AttachPending
                    | crate::event::SerialLinkedRuntimeCutoverTurn::Failed => {
                        if report_due && serial_retry.record_missing_proof(serial_now_ms) {
                            emit_deferred_net_bootstrap_supervisor_status(
                                pump,
                                supervisor_sequence.next_status_sequence(),
                                0,
                                DeferredNetSupervisorStatus::Preflight,
                                SERIAL_LINKED_RUNTIME_RETRY_MS,
                                serial_retry.next_probe_ms,
                                local_seat_enabled,
                                pump.serial_root_uart_cutover_owner_active(),
                            );
                        }
                    }
                }
            }
            deferred_cyw43_yield_and_reset(&mut activation_window);
            continue;
        }

        // Once the stack is attached, the retained supervisor remains its
        // lifetime recovery owner. The ordinary NetStack turn is also the sole
        // association/control progress lane, so it must run while Gate 8 is
        // pending. The CYW43 device boundary fences smoltcp RX/TX and DHCP
        // until a post-turn commit publishes the current logical generation.
        // Gate 8 then consumes a fresh passive snapshot. Its deadline belongs
        // to the one cold-pair attempt. A pre-service fault may drain and fence
        // its exact owner but cannot start another pair or renew the deadline.
        let recovery_required = crate::drivers::driver_task_net::cyw43_recovery_required();
        // Healthy attached traffic enters EventPump immediately, where one
        // outer-turn routing snapshot owns this proof. Reconstruct the full
        // immutable parent only when the bootstrap/recovery decision needs it;
        // otherwise this pre-pump read is redundant hot-path housekeeping.
        let canonical_parent = if recovery_required || !bootstrap.is_ready() {
            crate::drivers::driver_task_net::cyw43_canonical_parent_cut()
        } else {
            crate::drivers::driver_task_net::Cyw43CanonicalParentCut::Absent
        };
        let console_handoff_authorized = if gate8_lifecycle.handoff_authorization_pending() {
            let diagnostic = crate::drivers::driver_task_net::cyw43_gate8_diagnostic();
            matches!(
                gate8_lifecycle.revalidate_handoff(
                    diagnostic.generation,
                    bootstrap.gate8_generation_still_operational(diagnostic),
                    recovery_required,
                    pump.deferred_console_network_handoff_pending(),
                ),
                DeferredGate8HandoffRevalidation::Authorized { .. }
            )
        } else {
            false
        };
        if network_attached && (bootstrap.is_ready() || canonical_parent.retains_canonical_owner())
        {
            'attached_network_control: {
                let mut child_publication_only_turn = false;
                let mut handoff_terminal_failure = None;
                let service_ready_before_control = bootstrap_service_ready_published;
                let mut productive_network_successor = false;
                if !recovery_required && gate8_lifecycle.child_ready_wait_pending() {
                    let pre_network_now_ms = deferred_gate8_absolute_clock_sample().absolute_ms();
                    let pre_network_diagnostic =
                        crate::drivers::driver_task_net::cyw43_gate8_diagnostic();
                    match gate8_lifecycle.child_ready_pre_network_action(
                        pre_network_now_ms,
                        pre_network_diagnostic.generation,
                        bootstrap.gate8_generation_still_operational(pre_network_diagnostic),
                        recovery_required,
                        pump.isolated_child_ready_published_ms(),
                    ) {
                        DeferredGate8ChildReadyPreNetworkAction::Continue => {}
                        DeferredGate8ChildReadyPreNetworkAction::ObservePublicationOnly {
                            generation,
                            ..
                        } => {
                            // This exact deadline arbitration observes only the
                            // child's durable shared-page publication. It does
                            // not advance network policy, poll CYW43/SDIO, or
                            // compose another Network unit in this root turn.
                            let _ = pump.poll_isolated_child_publication_only();
                            child_publication_only_turn = true;
                            handoff_terminal_failure = gate8_lifecycle
                                .child_ready_post_observation_failure(
                                    generation,
                                    pump.isolated_child_ready_published_ms(),
                                );
                        }
                        DeferredGate8ChildReadyPreNetworkAction::Fail {
                            generation,
                            blocker,
                            deadline_ms,
                        } => {
                            handoff_terminal_failure = Some((generation, blocker, deadline_ms));
                        }
                    }
                }
                if handoff_terminal_failure.is_none() && !child_publication_only_turn {
                    match deferred_cyw43_attached_turn(
                        recovery_required,
                        canonical_parent,
                        console_handoff_authorized,
                    ) {
                        DeferredCyw43AttachedTurn::NetworkControl => {
                            if !activation_window.turn_admitted(
                                monotonic_ticks(),
                                counter_frequency(),
                                activation_reserve_us,
                            ) {
                                deferred_cyw43_yield_and_reset(&mut activation_window);
                                continue 'supervisor;
                            }
                            let control_turn_started_ms = crate::hal::timebase().now_ms();
                            productive_network_successor =
                                run_deferred_cyw43_attached_network_control_turn(
                                || pump.poll_deferred_cyw43_attached_network_control_turn(),
                                crate::drivers::driver_task_net::cyw43_recovery_required,
                                crate::drivers::driver_task_net::cyw43_gate8_diagnostic,
                                crate::drivers::driver_task_net::record_cyw43_pre_recovery_gate8,
                                crate::drivers::driver_task_net::commit_cyw43_data_handoff_if_ready,
                            );
                            activation_window
                                .record_attached_network_turn(productive_network_successor);
                            // The poll above may start Join and advance the logical
                            // connection generation. Commit only that post-poll
                            // generation; the bootstrap generation names the
                            // independently retained firmware/control pair.
                            let post_poll_recovery =
                                crate::drivers::driver_task_net::cyw43_recovery_required();
                            if deferred_cyw43_post_network_control_handoff_due(
                                post_poll_recovery,
                                pump.deferred_console_network_handoff_pending(),
                            ) {
                                let diagnostic =
                                    crate::drivers::driver_task_net::cyw43_gate8_diagnostic();
                                let generation_operational =
                                    bootstrap.gate8_generation_still_operational(diagnostic);
                                match gate8_lifecycle.authorize_handoff(
                                    control_turn_started_ms,
                                    diagnostic.generation,
                                    generation_operational,
                                    post_poll_recovery,
                                    true,
                                    cyw43_child_ready_response_bound_ms(),
                                ) {
                                    DeferredGate8HandoffAdmission::Authorized { .. } => {
                                        // DHCP became bound in a control turn that
                                        // began inside the original Gate 8 bound.
                                        // The exact-generation authorization alone
                                        // admits one exclusive following handoff.
                                        deferred_cyw43_yield_and_reset(&mut activation_window);
                                        continue 'supervisor;
                                    }
                                    DeferredGate8HandoffAdmission::InvalidAuthority {
                                        generation,
                                    } => {
                                        let now_ms = crate::hal::timebase().now_ms();
                                        crate::log_buffer::append_log_line(
                                        "CYW43_CHILD_READY_BOUND status=invalid action=fail-closed",
                                    );
                                        handoff_terminal_failure = Some((
                                            generation,
                                            "child-ready-response-bound-invalid",
                                            now_ms,
                                        ));
                                    }
                                    DeferredGate8HandoffAdmission::NotPending
                                    | DeferredGate8HandoffAdmission::RecoveryRequired
                                    | DeferredGate8HandoffAdmission::ProofDrift { .. }
                                    | DeferredGate8HandoffAdmission::Deadline { .. }
                                    | DeferredGate8HandoffAdmission::InvalidPhase => {}
                                }
                            }
                        }
                        DeferredCyw43AttachedTurn::ConsoleHandoff => {
                            // DHCP became bound on an earlier NetworkControl turn.
                            // Finalize and resume the pre-registered child now,
                            // without sharing this HAL-authority turn with CYW43
                            // polling. A transition error leaves the stack in its
                            // existing failed state; there is no root-owned TCP
                            // fallback. A later NetworkControl turn alone may
                            // consume the isolated child's Ready event.
                            let diagnostic =
                                crate::drivers::driver_task_net::cyw43_gate8_diagnostic();
                            let handoff_recovery =
                                crate::drivers::driver_task_net::cyw43_recovery_required();
                            if let DeferredGate8HandoffRevalidation::Authorized { generation } =
                                gate8_lifecycle.revalidate_handoff(
                                    diagnostic.generation,
                                    bootstrap.gate8_generation_still_operational(diagnostic),
                                    handoff_recovery,
                                    pump.deferred_console_network_handoff_pending(),
                                )
                            {
                                let now_ms = crate::hal::timebase().now_ms();
                                let pre_resume_clock = deferred_gate8_absolute_clock_sample();
                                match gate8_lifecycle
                                    .prepare_child_ready_wait(pre_resume_clock, generation)
                                {
                                    DeferredGate8ChildReadyPreparation::Prepared(plan) => {
                                        let Some(handoff_completed) =
                                            with_deferred_root_hal(hal_ptr, |hal| {
                                                pump.service_deferred_console_network_handoff(hal)
                                            })
                                        else {
                                            run_root_console_pump(pump);
                                        };
                                        if handoff_completed {
                                            let post_handoff_recovery = crate::drivers::driver_task_net::cyw43_recovery_required();
                                            let post_handoff_diagnostic =
                                            crate::drivers::driver_task_net::cyw43_gate8_diagnostic(
                                            );
                                            let post_handoff_now_ms =
                                                crate::hal::timebase().now_ms();
                                            let post_resume_clock =
                                                deferred_gate8_absolute_clock_sample();
                                            match gate8_lifecycle.arm_child_ready_wait(
                                                plan,
                                                post_resume_clock,
                                                post_handoff_diagnostic.generation,
                                                bootstrap.gate8_generation_still_operational(
                                                    post_handoff_diagnostic,
                                                ),
                                                post_handoff_recovery,
                                            ) {
                                                DeferredGate8ChildReadyArm::Armed { .. } => {
                                                    deferred_cyw43_yield_and_reset(
                                                        &mut activation_window,
                                                    );
                                                    continue 'supervisor;
                                                }
                                                DeferredGate8ChildReadyArm::RecoveryRequired => {}
                                                DeferredGate8ChildReadyArm::GenerationDrift => {
                                                    handoff_terminal_failure = Some((
                                                        generation,
                                                        "child-ready-generation-drift",
                                                        crate::hal::timebase().now_ms(),
                                                    ));
                                                }
                                                DeferredGate8ChildReadyArm::InvalidPhase
                                                | DeferredGate8ChildReadyArm::AlreadyArmed => {
                                                    handoff_terminal_failure = Some((
                                                        generation,
                                                        "child-ready-state-invalid",
                                                        crate::hal::timebase().now_ms(),
                                                    ));
                                                }
                                                DeferredGate8ChildReadyArm::ClockInvalid => {
                                                    crate::log_buffer::append_log_line(
                                                    "CYW43_CHILD_READY_CLOCK status=invalid action=fail-closed",
                                                );
                                                    handoff_terminal_failure = Some((
                                                        generation,
                                                        "child-ready-clock-invalid",
                                                        post_handoff_now_ms,
                                                    ));
                                                }
                                                DeferredGate8ChildReadyArm::ClockOverflow => {
                                                    crate::log_buffer::append_log_line(
                                                    "CYW43_CHILD_READY_BOUND status=overflow action=fail-closed",
                                                );
                                                    handoff_terminal_failure = Some((
                                                        generation,
                                                        "child-ready-clock-overflow",
                                                        post_handoff_now_ms,
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                    DeferredGate8ChildReadyPreparation::ClockInvalid => {
                                        crate::log_buffer::append_log_line(
                                            "CYW43_CHILD_READY_CLOCK status=invalid action=fail-closed",
                                        );
                                        handoff_terminal_failure =
                                            Some((generation, "child-ready-clock-invalid", now_ms));
                                    }
                                    DeferredGate8ChildReadyPreparation::ClockOverflow => {
                                        crate::log_buffer::append_log_line(
                                        "CYW43_CHILD_READY_BOUND status=overflow action=fail-closed",
                                    );
                                        handoff_terminal_failure = Some((
                                            generation,
                                            "child-ready-clock-overflow",
                                            now_ms,
                                        ));
                                    }
                                    DeferredGate8ChildReadyPreparation::GenerationDrift => {
                                        handoff_terminal_failure = Some((
                                            generation,
                                            "child-ready-generation-drift",
                                            now_ms,
                                        ));
                                    }
                                    DeferredGate8ChildReadyPreparation::InvalidPhase
                                    | DeferredGate8ChildReadyPreparation::AlreadyArmed => {
                                        handoff_terminal_failure =
                                            Some((generation, "child-ready-state-invalid", now_ms));
                                    }
                                }
                            }
                        }
                        DeferredCyw43AttachedTurn::CanonicalWait
                        | DeferredCyw43AttachedTurn::RecoverySupervisor => {
                            // The common phase alternator below owns recovery
                            // operator and driver turns. A canonical wait keeps the
                            // bootstrap supervisor in Complete, so that alternator
                            // performs only operator service plus a durable parent
                            // recheck until the exact continuation becomes visible.
                            break 'attached_network_control;
                        }
                    }
                }
                if crate::drivers::driver_task_net::cyw43_recovery_required() {
                    // The ordinary network turn discovered the transport edge.
                    // Yield now so recovery begins in a fresh guarded activation;
                    // its Operator and Driver remain distinct logical turns and
                    // cannot compose with this Network unit.
                    deferred_cyw43_yield_and_reset(&mut activation_window);
                    continue 'supervisor;
                }
                let stability_now_ms = crate::hal::timebase().now_ms();
                let mut recovery_required =
                    crate::drivers::driver_task_net::cyw43_recovery_required();
                let attempt = CYW43_BOOTSTRAP_ATTEMPT;
                let diagnostic = crate::drivers::driver_task_net::cyw43_gate8_diagnostic();
                let publication_receipt =
                    crate::drivers::driver_task_net::cyw43_gate8_publication_receipt(
                        diagnostic.generation,
                    );
                recovery_required |= crate::drivers::driver_task_net::cyw43_recovery_required();
                if !recovery_required {
                    crate::drivers::driver_task_net::record_cyw43_pre_recovery_gate8(diagnostic);
                }
                let observation = gate8_lifecycle.observe(
                    attempt,
                    stability_now_ms,
                    bootstrap.gate8_generation_still_operational(diagnostic),
                    publication_receipt,
                    diagnostic,
                );
                let mut terminal_failure = handoff_terminal_failure;
                match observation {
                    DeferredGate8Observation::Pending => {}
                    DeferredGate8Observation::Publish {
                        generation,
                        publication_receipt,
                    } if !recovery_required => {
                        // The driver revalidates and commits this exact passive
                        // snapshot before it can become visible as Gate 8
                        // proof. The eight-line queue operation and its
                        // immediately following commit are atomic. Any
                        // publication rejection retracts the accepted generation,
                        // so the next turn must capture and validate a fresh copy.
                        if !bootstrap.mark_gate8_generation_stable(diagnostic) {
                            let _ = gate8_lifecycle.reject_publication(generation);
                            crate::log_buffer::append_log_line(
                                "CYW43_GATE8_SNAPSHOT_COMMIT status=rejected action=retry-fresh-snapshot",
                            );
                            deferred_cyw43_yield_and_reset(&mut activation_window);
                            continue 'supervisor;
                        }
                        // Reserve lifecycle and consumer state before mutating
                        // the qlog/serial/HDMI Gate 8 commit queue. Gate 8 opens
                        // the data consumer but remains nonterminal until this
                        // exact generation later proves DHCP plus listener
                        // readiness. The EventPump transaction preflights all
                        // retained capacity before appending its first record.
                        let lifecycle_accepted = gate8_lifecycle.accept_commit(generation);
                        let consumer_published = lifecycle_accepted
                            && crate::drivers::driver_task_net::publish_cyw43_gate8_data_consumer(
                                generation,
                                publication_receipt,
                            );
                        let commit_deadline_ms =
                            gate8_lifecycle.deadline_ms().unwrap_or(stability_now_ms);
                        let commit_sequence = supervisor_sequence.next_status_sequence();
                        let commit_queued = consumer_published
                            && emit_deferred_net_gate8_commit_transaction(
                                pump,
                                diagnostic,
                                commit_sequence,
                                attempt,
                                commit_deadline_ms,
                            );
                        if commit_queued {
                            if !bootstrap.commit_gate8_publication(generation) {
                                crate::log_buffer::append_log_line(
                                "CYW43_GATE8_READY_DIAGNOSTIC_COMMIT status=rejected action=retain-first-cause",
                            );
                            }
                            crate::log_buffer::append_log_line(
                                "[net-console] CYW43 Gate 8 stable for current generation",
                            );
                        } else {
                            let _ =
                                crate::drivers::driver_task_net::retract_cyw43_gate8_data_consumer(
                                    generation,
                                );
                            let _ = bootstrap.retract_gate8_generation(generation);
                            let _ = gate8_lifecycle.enter_stabilizing(attempt, stability_now_ms);
                            crate::log_buffer::append_log_line(
                            "CYW43_GATE8_COMMIT_TRANSACTION status=failed consumer=blocked action=retract-and-retry-fresh-snapshot",
                        );
                        }
                    }
                    DeferredGate8Observation::Publish { .. } => {}
                    DeferredGate8Observation::Committed
                        if !recovery_required && terminal_failure.is_none() =>
                    {
                        let generation = diagnostic.generation;
                        let ready_published_ms = pump.isolated_child_ready_published_ms();
                        if pump.net_console_cyw43_boot_service_ready_for_root(generation)
                            && gate8_lifecycle
                                .child_ready_publication_on_time(generation, ready_published_ms)
                        {
                            let ready_sequence = supervisor_sequence.next_status_sequence();
                            let runtime_recovery = bootstrap_service_ready_published;
                            let ready_queued = if runtime_recovery {
                                emit_deferred_net_runtime_service_ready_transaction(
                                    pump,
                                    ready_sequence,
                                    generation,
                                )
                            } else {
                                emit_deferred_net_boot_service_ready_transaction(
                                    pump,
                                    ready_sequence,
                                    generation,
                                    stability_now_ms,
                                    local_seat_enabled,
                                )
                            };
                            if !ready_queued {
                                deferred_cyw43_yield_and_reset(&mut activation_window);
                                continue 'supervisor;
                            }
                            let lifecycle_committed =
                                ready_published_ms.is_some_and(|published_ms| {
                                    gate8_lifecycle.mark_service_ready(generation, published_ms)
                                });
                            debug_assert!(
                                lifecycle_committed,
                                "service Ready follows the retained Gate 8 generation"
                            );
                            if lifecycle_committed {
                                bootstrap_service_ready_published = true;
                                if bootstrap
                                    .admit_runtime_pair_recovery_after_service_ready(generation)
                                {
                                    crate::log_buffer::append_log_line(if runtime_recovery {
                                        "[net-console] CYW43 runtime service restored; bounded recovery re-armed"
                                    } else {
                                        "[net-console] CYW43 bootstrap service ready; bounded runtime recovery armed"
                                    });
                                } else {
                                    crate::log_buffer::append_log_line(
                                        "CYW43_RUNTIME_RECOVERY_ADMISSION status=rejected action=retain-service-ready-without-recovery-budget",
                                    );
                                }
                            }
                        } else if let Some(deadline_ms) = gate8_lifecycle
                            .service_readiness_deadline_expired(
                                generation,
                                stability_now_ms,
                                deferred_gate8_absolute_clock_sample().absolute_ms(),
                            )
                        {
                            if !crate::drivers::driver_task_net::cyw43_finite_lifecycle_cut_blocking(
                            ) {
                                terminal_failure =
                                    Some((generation, "service-readiness-deadline", deadline_ms));
                            }
                        }
                    }
                    DeferredGate8Observation::Committed
                    | DeferredGate8Observation::ServiceReady => {}
                    DeferredGate8Observation::Retracted { generation } => {
                        let _ = crate::drivers::driver_task_net::retract_cyw43_gate8_data_consumer(
                            generation,
                        );
                        let _ = bootstrap.retract_gate8_generation(generation);
                        pump.defer_local_seat_hdmi_ready_until_cyw43_terminal();
                        let deadline_ms = match gate8_lifecycle.deadline_ms() {
                            Some(deadline_ms) => deadline_ms,
                            None => stability_now_ms,
                        };
                        emit_deferred_net_bootstrap_supervisor_status(
                            pump,
                            supervisor_sequence.next_status_sequence(),
                            attempt,
                            DeferredNetSupervisorStatus::Stabilizing,
                            0,
                            deadline_ms,
                            local_seat_enabled,
                            false,
                        );
                        let mut line = HeaplessString::<192>::new();
                        let _ = write!(
                        line,
                        "CYW43_GATE8_READY_RETRACTED attempt={} generation={} deadline_ms={} action=fresh-proof",
                        attempt, generation, deadline_ms,
                    );
                        emit_deferred_net_operator_line(pump, line.as_str(), false);
                    }
                    DeferredGate8Observation::Deadline {
                        generation,
                        deadline_ms,
                        blocker,
                    } => {
                        if !recovery_required
                            && !crate::drivers::driver_task_net::cyw43_finite_lifecycle_cut_blocking(
                            )
                        {
                            terminal_failure = Some((generation, blocker, deadline_ms));
                        }
                    }
                }

                if let Some((generation, blocker, deadline_ms)) = terminal_failure {
                    if crate::drivers::driver_task_net::cyw43_recovery_required()
                        || crate::drivers::driver_task_net::cyw43_finite_lifecycle_cut_blocking()
                    {
                        // A typed transport edge discovered after the first
                        // passive snapshot retains recovery authority. The next
                        // supervisor section will service it without publishing a
                        // contradictory logical terminal record.
                        deferred_cyw43_yield_and_reset(&mut activation_window);
                        continue 'supervisor;
                    }
                    gate8_terminal_pending = Some(DeferredGate8TerminalPending {
                        attempt,
                        generation,
                        deadline_ms,
                        blocker,
                        diagnostic,
                        status_sequence: supervisor_sequence.next_status_sequence(),
                        terminal_decision_committed: false,
                        failure_transaction_retained: false,
                    });
                    deferred_cyw43_yield_and_reset(&mut activation_window);
                    continue 'supervisor;
                }

                recovery_required |= crate::drivers::driver_task_net::cyw43_recovery_required();
                if !recovery_required {
                    if service_ready_before_control
                        && bootstrap_service_ready_published
                        && productive_network_successor
                    {
                        // EventPump proved that this exact attached Network
                        // unit made progress and retained either its immediate
                        // Network successor or the exact completed response
                        // rotation. Re-enter loop-top arbitration under the
                        // same generated-reserve guard before another Network
                        // unit; all other outcomes restore the explicit yield.
                        continue 'supervisor;
                    }
                    deferred_cyw43_yield_and_reset(&mut activation_window);
                    continue 'supervisor;
                }
            }
        }

        if supervisor_phase == DeferredCyw43SupervisorPhase::Operator {
            if !activation_window.turn_admitted(
                monotonic_ticks(),
                counter_frequency(),
                activation_reserve_us,
            ) {
                deferred_cyw43_yield_and_reset(&mut activation_window);
                continue;
            }
            // Operator service remains one logical condition-before-operation
            // turn. A guarded activation may preserve the current refill for
            // its immediately following one-operation Driver turn, but never
            // schedules Driver twice without another Operator reconciliation.
            let may_begin_before = pump.cyw43_bootstrap_may_begin();
            pump.poll_cyw43_bootstrap_supervisor_event_turn();
            activation_window.record_operator_turn();
            let may_begin = pump.cyw43_bootstrap_may_begin();
            if may_begin {
                let _ = service_deferred_cyw43_bootstrap_sideband_condition(|| {
                    crate::drivers::driver_task_net::consume_cyw43_persistent_sideband_rx_batch()
                });
            }
            // The operator turn is the condition-before-sleep boundary. The
            // persistent parent has no root continuation edge, so durable
            // terminal/fault state alone can reopen Driver after it waits.
            let driver_turn_due = bootstrap.driver_turn_due();
            let (operator_turn, next_phase) =
                deferred_cyw43_supervisor_phase_step(supervisor_phase, may_begin, driver_turn_due);
            let continuation = deferred_cyw43_mcs_continuation(
                operator_turn,
                next_phase,
                false,
                may_begin_before,
                network_attached,
            );
            supervisor_phase = next_phase;
            if !may_begin {
                deferred_cyw43_yield_and_reset(&mut activation_window);
                continue;
            }
            let now_ms = crate::hal::timebase().now_ms();
            if !attempt_active {
                if let Some(status) = deferred_cyw43_supervisor_start_status(
                    network_attached,
                    bootstrap_service_ready_published,
                ) {
                    emit_deferred_net_bootstrap_supervisor_status(
                        pump,
                        supervisor_sequence.next_status_sequence(),
                        CYW43_BOOTSTRAP_ATTEMPT,
                        status,
                        0,
                        now_ms,
                        local_seat_enabled,
                        false,
                    );
                }
                attempt_active = true;
            }
            if continuation == DeferredCyw43McsContinuation::Yield {
                deferred_cyw43_yield_and_reset(&mut activation_window);
                continue;
            }
        }

        if !activation_window.turn_admitted(
            monotonic_ticks(),
            counter_frequency(),
            activation_reserve_us,
        ) {
            deferred_cyw43_yield_and_reset(&mut activation_window);
            continue;
        }
        let may_begin = pump.cyw43_bootstrap_may_begin();
        let driver_turn_due = bootstrap.driver_turn_due();
        let (driver_turn, next_phase) =
            deferred_cyw43_supervisor_phase_step(supervisor_phase, may_begin, driver_turn_due);
        supervisor_phase = next_phase;
        if driver_turn != DeferredCyw43SupervisorTurn::Driver {
            // Recheck both admission and the durable parent condition in
            // Driver phase. A reboot, linked-serial cut, or persistent wait
            // that became visible after the preceding operator turn returns
            // to Operator without issuing a child operation.
            deferred_cyw43_yield_and_reset(&mut activation_window);
            continue;
        }

        let now_ms = crate::hal::timebase().now_ms();
        let attempt = CYW43_BOOTSTRAP_ATTEMPT;
        wifi_operation_started = true;
        let turn = {
            crate::drivers::driver_task_net::begin_cyw43_outer_event_turn();
            let _cyw43_outer_event_turn =
                crate::drivers::driver_task_net::cyw43_outer_event_turn_finalizer();
            let Some(turn) = with_deferred_root_hal(hal_ptr, |hal| bootstrap.service_turn(hal))
            else {
                // The entry check above proves this unreachable unless the
                // retained bootstrap pointer was corrupted after validation.
                run_root_console_pump(pump);
            };
            turn
        };
        let operation_executed = matches!(
            turn,
            Cyw43BootstrapTurnOutcome::Pending {
                operation_executed: true,
                ..
            }
        );
        activation_window.record_driver_turn(operation_executed);
        let continuation = deferred_cyw43_mcs_continuation(
            driver_turn,
            supervisor_phase,
            operation_executed,
            true,
            network_attached,
        );
        match turn {
            Cyw43BootstrapTurnOutcome::Pending {
                turn_id,
                stage,
                operation_executed,
            } => {
                let mut line = HeaplessString::<192>::new();
                let _ = write!(
                    line,
                    "[net-console] retained CYW43 turn={} stage={} operation={}",
                    turn_id, stage, operation_executed,
                );
                crate::log_buffer::append_log_line(line.as_str());
                if let Some(repeat) = turn_status.observe(stage) {
                    let mut operator_line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
                    let _ = write!(
                        operator_line,
                        "CYW43_BOOTSTRAP_TURN attempt={} turn={} stage={} operation={} repeat={}",
                        attempt, turn_id, stage, operation_executed, repeat,
                    );
                    // `with_deferred_root_hal` has returned, so this only queues
                    // bytes for the independent linked serial runtime. The next
                    // outer EventPump turn performs the actual flush.
                    if stage == "cyw43-pair-recovery-signalled" {
                        let recovery =
                            crate::drivers::driver_task_net::cyw43_deferred_recovery_diagnostic();
                        let live_generation =
                            crate::drivers::driver_task_net::cyw43_association_diagnostic()
                                .generation;
                        let pending = DeferredCyw43RecoveryDiagnosticPending {
                            recovery,
                            live_generation,
                            operator_line,
                        };
                        if !pump.queue_cyw43_pair_recovery_diagnostic_transaction(
                            pending.recovery,
                            pending.live_generation,
                            pending.operator_line.as_str(),
                        ) {
                            recovery_diagnostic_pending = Some(pending);
                        }
                    } else if !pump.queue_cyw43_bootstrap_operator_line(operator_line.as_str()) {
                        turn_status.retry_last();
                    }
                }
            }
            Cyw43BootstrapTurnOutcome::Complete => {
                supervisor_phase = DeferredCyw43SupervisorPhase::Operator;
                turn_status.reset();
                if !bootstrap.is_ready() || bootstrap.ready_generation().is_none() {
                    let mut detail = HeaplessString::<192>::new();
                    let _ = detail.push_str("retained supervisor completed without generation");
                    emit_deferred_net_console_failure(pump, &detail, false);
                    emit_deferred_net_bootstrap_supervisor_status(
                        pump,
                        supervisor_sequence.next_status_sequence(),
                        attempt,
                        DeferredNetSupervisorStatus::Permanent,
                        0,
                        now_ms,
                        local_seat_enabled,
                        false,
                    );
                    if let Some(mode) = permanent_failure_terminal_mode(network_attached) {
                        pump.quarantine_network_service_after_cyw43_terminal_failure();
                        terminal_mode = Some(mode);
                        deferred_cyw43_yield_and_reset(&mut activation_window);
                        continue;
                    }
                    run_root_console_pump(pump);
                }
                if network_attached {
                    let retracted_generation = match gate8_lifecycle {
                        DeferredGate8Lifecycle::Committed { generation, .. } => Some(generation),
                        DeferredGate8Lifecycle::Detached
                        | DeferredGate8Lifecycle::Stabilizing { .. } => None,
                    };
                    let gate8_deadline_ms = gate8_lifecycle.enter_stabilizing(attempt, now_ms);
                    if let Some(generation) = retracted_generation {
                        let _ = crate::drivers::driver_task_net::retract_cyw43_gate8_data_consumer(
                            generation,
                        );
                        let _ = bootstrap.retract_gate8_generation(generation);
                        pump.defer_local_seat_hdmi_ready_until_cyw43_terminal();
                    }
                    emit_deferred_net_bootstrap_supervisor_status(
                        pump,
                        supervisor_sequence.next_status_sequence(),
                        attempt,
                        DeferredNetSupervisorStatus::Stabilizing,
                        0,
                        gate8_deadline_ms,
                        local_seat_enabled,
                        false,
                    );
                    attempt_active = false;
                    deferred_cyw43_yield_and_reset(&mut activation_window);
                    continue;
                }
                #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
                let console_runtime = {
                    let Some(console_runtime) = net_deferred_console_runtime.take() else {
                        let mut detail = HeaplessString::<192>::new();
                        let _ = detail.push_str("deferred isolated console shell missing");
                        emit_deferred_net_console_failure(pump, &detail, false);
                        run_root_console_pump(pump);
                    };
                    console_runtime
                };
                #[cfg(all(target_arch = "aarch64", target_os = "none", sel4_config_kernel_mcs))]
                let Some(stack_result) = with_deferred_root_hal(hal_ptr, |hal| {
                    crate::net::finish_cyw43_net_console_after_bootstrap(
                        hal,
                        bootstrap.config(),
                        console_runtime,
                    )
                }) else {
                    // The same validated leaked pointer is reused only after
                    // the retained operation above has released its borrow.
                    run_root_console_pump(pump);
                };
                #[cfg(not(all(
                    target_arch = "aarch64",
                    target_os = "none",
                    sel4_config_kernel_mcs
                )))]
                let Some(stack_result) = with_deferred_root_hal(hal_ptr, |hal| {
                    crate::net::finish_cyw43_net_console_after_bootstrap(hal, bootstrap.config())
                }) else {
                    run_root_console_pump(pump);
                };
                let stack = match stack_result {
                    Ok(stack) => stack,
                    Err(err) => {
                        let mut detail = HeaplessString::<192>::new();
                        let _ = write!(detail, "{err}");
                        emit_deferred_net_console_failure(pump, &detail, false);
                        emit_deferred_net_bootstrap_supervisor_status(
                            pump,
                            supervisor_sequence.next_status_sequence(),
                            attempt,
                            DeferredNetSupervisorStatus::Permanent,
                            0,
                            now_ms,
                            local_seat_enabled,
                            false,
                        );
                        run_root_console_pump(pump);
                    }
                };
                emit_deferred_net_console_result(pump, &stack);
                let stack: &'static mut NetStackHandle = Box::leak(Box::new(stack));
                if !pump.attach_network_after_bootstrap(stack) {
                    let mut detail = HeaplessString::<192>::new();
                    let _ = detail.push_str("deferred network attach rejected: stack already live");
                    emit_deferred_net_console_failure(pump, &detail, false);
                    emit_deferred_net_bootstrap_supervisor_status(
                        pump,
                        supervisor_sequence.next_status_sequence(),
                        attempt,
                        DeferredNetSupervisorStatus::Permanent,
                        0,
                        now_ms,
                        local_seat_enabled,
                        false,
                    );
                    run_root_console_pump(pump);
                }
                let gate8_deadline_ms = gate8_lifecycle.enter_stabilizing(attempt, now_ms);
                emit_deferred_net_bootstrap_supervisor_status(
                    pump,
                    supervisor_sequence.next_status_sequence(),
                    attempt,
                    DeferredNetSupervisorStatus::Stabilizing,
                    0,
                    gate8_deadline_ms,
                    local_seat_enabled,
                    false,
                );
                let mut line = HeaplessString::<160>::new();
                let _ = write!(
                    line,
                    "[net-console] stack attached on 0.0.0.0:{} after bootstrap attempt {}; Gate 8 stabilizing until {} ms",
                    crate::net::CONSOLE_TCP_PORT,
                    attempt,
                    gate8_deadline_ms,
                );
                crate::log_buffer::append_log_line(line.as_str());
                network_attached = true;
                attempt_active = false;
            }
            Cyw43BootstrapTurnOutcome::Failed(driver_error) => {
                supervisor_phase = DeferredCyw43SupervisorPhase::Operator;
                turn_status.reset();
                let err = crate::net::map_cyw43_bootstrap_error(driver_error);
                let failure_now_ms = crate::hal::timebase().now_ms();
                let mut detail = HeaplessString::<192>::new();
                let _ = write!(detail, "{err}");
                emit_deferred_net_console_failure(pump, &detail, false);
                if crate::net::cyw43_net_console_bootstrap_error_is_transient(&err) {
                    emit_deferred_net_bootstrap_supervisor_status(
                        pump,
                        supervisor_sequence.next_status_sequence(),
                        attempt,
                        DeferredNetSupervisorStatus::Failed,
                        0,
                        u64::MAX,
                        local_seat_enabled,
                        false,
                    );
                    pump.quarantine_network_service_after_cyw43_terminal_failure();
                    terminal_mode = Some(DeferredNetSupervisorTerminal::BootstrapFailed);
                    attempt_active = false;
                } else {
                    emit_deferred_net_bootstrap_supervisor_status(
                        pump,
                        supervisor_sequence.next_status_sequence(),
                        attempt,
                        DeferredNetSupervisorStatus::Permanent,
                        0,
                        failure_now_ms,
                        local_seat_enabled,
                        false,
                    );
                    if let Some(mode) = permanent_failure_terminal_mode(network_attached) {
                        pump.quarantine_network_service_after_cyw43_terminal_failure();
                        terminal_mode = Some(mode);
                    } else {
                        run_root_console_pump(pump);
                    }
                }
            }
        }
        if continuation == DeferredCyw43McsContinuation::Continue {
            // The scoped outer-event finalizer above has retired the exact
            // one-operation lease. Re-enter only at loop-top fault/EventPump
            // arbitration while retaining this guarded MCS activation.
            continue 'supervisor;
        }
        deferred_cyw43_yield_and_reset(&mut activation_window);
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
fn take_net_stack(ctx: &BootContext) -> Option<Box<NetStackHandle>> {
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

#[cfg(all(
    feature = "net-console",
    feature = "kernel",
    target_arch = "aarch64",
    target_os = "none",
    sel4_config_kernel_mcs
))]
fn take_net_deferred_console_runtime(
    ctx: &BootContext,
) -> Option<crate::hal::console_network::ConsoleNetworkRuntime> {
    ctx.net_deferred_console_runtime.borrow_mut().take()
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
fn emit_deferred_net_console_result<
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
    stack: &NetStackHandle,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    let status = stack.status_report();
    let mut line = HeaplessString::<192>::new();
    let state = match status.address_source {
        "wifi-host-eapol-pending" => "deferred pending",
        "wifi-host-eapol-required" => "deferred blocked",
        "wifi-data-rx-admission-blocked" => "deferred blocked",
        _ => "deferred ready",
    };
    let _ = write!(
        line,
        "[net-console] {state} profile_backend={} active_driver={} active={} address_source={} dhcp={} tcp_ready={} port={}",
        status.profile_backend,
        status.active_driver,
        status.active_interface,
        status.address_source,
        status.dhcp_phase,
        if status.tcp_ready { "yes" } else { "no" },
        crate::net::CONSOLE_TCP_PORT,
    );
    emit_deferred_net_operator_line(pump, line.as_str(), false);
}

#[cfg(all(feature = "net-console", feature = "kernel"))]
fn emit_deferred_net_console_failure<
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
    detail: &HeaplessString<192>,
    raw_fallback_allowed: bool,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    let mut line = HeaplessString::<224>::new();
    let _ = write!(
        line,
        "[net-console] deferred failed detail={}",
        detail.as_str(),
    );
    emit_deferred_net_operator_line(pump, line.as_str(), raw_fallback_allowed);
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
const PRE_ROOT_NET_CONSOLE_SLOW_POLL_TRACE_MS: u64 = 1_000;
#[cfg(all(feature = "net-console", feature = "kernel"))]
const PRE_ROOT_NET_CONSOLE_SLOW_POLL_TRACE_LIMIT: u8 = 4;
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
    let mut slow_poll_traces = 0u8;
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
            if matches!(reason, "wifi-host-eapol-pending" | "wired-address-ready") {
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

        let poll_start_ms = crate::hal::timebase().now_ms();
        pump.poll_pre_root_network();
        polls = polls.saturating_add(1);
        let poll_elapsed_ms = crate::hal::timebase()
            .now_ms()
            .saturating_sub(poll_start_ms);
        if wifi_wait
            && poll_elapsed_ms >= PRE_ROOT_NET_CONSOLE_SLOW_POLL_TRACE_MS
            && slow_poll_traces < PRE_ROOT_NET_CONSOLE_SLOW_POLL_TRACE_LIMIT
        {
            slow_poll_traces = slow_poll_traces.saturating_add(1);
            let mut line = HeaplessString::<192>::new();
            let _ = write!(
                line,
                "[net-console] root console wait slow-poll poll_ms={} total_polls={} action=continue-driver-settle",
                poll_elapsed_ms, polls,
            );
            boot_log::force_uart_line(line.as_str());
            pump.publish_pre_root_boot_progress(line.as_str());
        }
        if pump.net_console_ready_for_root()
            || pump.net_console_pre_root_serial_release_reason().is_some()
        {
            continue;
        }
        sel4::yield_now();
    }
}

#[cfg(feature = "kernel")]
fn attach_kernel_console<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
    ctx: &BootContext,
    bootstrap_ipc: Option<&'a mut UserlandBootstrapHandler>,
    wifi_debug: Option<&'a mut KernelWifiDebugHandle>,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    if let Some(handler) = bootstrap_ipc {
        pump.attach_console_context(ctx.bootinfo, ctx.endpoints.control.raw(), ctx.uart_slot);
        pump.attach_bootstrap_handler(handler);
    }
    if let Some(wifi_debug) = wifi_debug {
        pump.attach_wifi_debug(wifi_debug);
    }
}

#[cfg(not(feature = "kernel"))]
fn attach_kernel_console<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    _pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
    _ctx: &BootContext,
    _bootstrap_ipc: Option<&'a mut UserlandBootstrapHandler>,
    _wifi_debug: Option<&'a mut KernelWifiDebugHandle>,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
}

#[cfg(feature = "kernel")]
fn attach_local_seat<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
    ctx: &BootContext,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    if let Some(runtime) = ctx.local_seat.borrow_mut().take() {
        pump.attach_local_seat(runtime);
    }
}

#[cfg(not(feature = "kernel"))]
fn attach_local_seat<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    _pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
    _ctx: &BootContext,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
}

#[cfg(feature = "kernel")]
fn attach_ninedoor_bridge<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
    ctx: &BootContext,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    if let Some(ninedoor) = ctx.ninedoor.borrow_mut().take() {
        // This only transfers root-owned bridge bookkeeping into the event
        // pump. The isolated passive NineDoor child already executes on its
        // compiler-selected MCS donation path; moving the init TCB here would
        // introduce a second, classic-SMP placement mechanism.
        pump.attach_ninedoor(ninedoor);
    }
}

#[cfg(not(feature = "kernel"))]
fn attach_ninedoor_bridge<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    _pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
    _ctx: &BootContext,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
}

#[cfg(feature = "net-console")]
fn attach_network<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
    net_stack_handle: Option<&'a mut NetStackHandle>,
    net_unavailable_detail: Option<HeaplessString<192>>,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    pump.set_network_unavailable_detail(net_unavailable_detail);
    if let Some(net_stack) = net_stack_handle {
        pump.attach_initial_network(net_stack);
    }
}

#[cfg(not(feature = "net-console"))]
fn attach_network<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>(
    _pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
    _net_stack_handle: Option<&'a mut NetStackHandle>,
    _net_unavailable_detail: Option<HeaplessString<192>>,
) where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
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
        if let Ok(observation) = crate::worker_authority::observe_endpoint_badge(message.badge) {
            let mut worker_summary = HeaplessString::<160>::new();
            let _ = write!(
                worker_summary,
                "[worker-ipc] action={} role={} epoch={} badge=0x{badge:016x}",
                worker_endpoint_action_label(observation.action),
                worker_role_label(observation.role),
                observation.epoch,
                badge = observation.badge
            );
            audit.info(worker_summary.as_str());
            log::debug!("[audit] {}", worker_summary.as_str());
        }
        crate::bootstrap::log::process_ep_payload(message.payload.as_slice(), audit);
    }
}

fn worker_endpoint_action_label(
    action: crate::worker_authority::WorkerEndpointAction,
) -> &'static str {
    match action {
        crate::worker_authority::WorkerEndpointAction::Attach => "attach",
        crate::worker_authority::WorkerEndpointAction::Telemetry => "telemetry",
        crate::worker_authority::WorkerEndpointAction::LeaseRenewal => "lease-renewal",
        crate::worker_authority::WorkerEndpointAction::Receipt => "receipt",
        crate::worker_authority::WorkerEndpointAction::Revoke => "revoke",
    }
}

fn worker_role_label(role: cohesix_ticket::Role) -> &'static str {
    match role {
        cohesix_ticket::Role::Queen => "queen",
        cohesix_ticket::Role::WorkerHeartbeat => "worker-heartbeat",
        cohesix_ticket::Role::WorkerGpu => "worker-gpu",
        cohesix_ticket::Role::WorkerBus => "worker-bus",
        cohesix_ticket::Role::WorkerLora => "worker-lora",
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
    #[cfg(all(target_arch = "aarch64", feature = "timers-arch-counter"))]
    {
        crate::arch::aarch64::timer::timer_counter_ticks()
    }
    #[cfg(not(all(target_arch = "aarch64", feature = "timers-arch-counter")))]
    {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }
}

#[inline]
fn counter_frequency() -> u64 {
    #[cfg(all(target_arch = "aarch64", feature = "timers-arch-counter"))]
    {
        crate::arch::aarch64::timer::timer_freq_hz()
    }
    #[cfg(not(all(target_arch = "aarch64", feature = "timers-arch-counter")))]
    {
        1
    }
}

#[cfg(test)]
mod tests {
    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    fn gate8_lifecycle_snapshot(
        pair_scrub_epoch: u64,
        generation: u32,
        frontier_status: crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus,
        blocker: &'static str,
    ) -> crate::drivers::driver_task_net::Cyw43Gate8Diagnostic {
        let pass = crate::drivers::driver_task_net::Cyw43Gate8SubgateDiagnostic {
            token: "8a-pair-generation",
            status: crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pass,
            blocker: "none",
        };
        let mut subgates = [pass; 8];
        if frontier_status != crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pass {
            subgates[7] = crate::drivers::driver_task_net::Cyw43Gate8SubgateDiagnostic {
                token: "8h-data-admission",
                status: frontier_status,
                blocker,
            };
        }
        crate::drivers::driver_task_net::Cyw43Gate8Diagnostic {
            pair_scrub_epoch,
            generation,
            subgates,
            current_work_pending: frontier_status
                == crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pending,
        }
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    const fn gate8_absolute_clock_ms(ms: u64) -> super::DeferredGate8AbsoluteClockSample {
        super::DeferredGate8AbsoluteClockSample {
            counter_ticks: ms,
            counter_hz: 1_000,
            generated_timer_hz: 1_000,
        }
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    const fn gate8_lifecycle_publication_receipt(
        pair_scrub_epoch: u64,
        generation: u32,
        dpc_epoch: u32,
        dpc_producer: u32,
    ) -> crate::drivers::driver_task_net::Cyw43Gate8PublicationReceipt {
        crate::drivers::driver_task_net::Cyw43Gate8PublicationReceipt {
            pair_scrub_epoch,
            generation,
            dpc_epoch,
            dpc_producer,
            dpc_overruns: 0,
            dpc_ack_failures: 0,
        }
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    fn committed_gate8_lifecycle(
        started_ms: u64,
        generation: u32,
    ) -> super::DeferredGate8Lifecycle {
        let stable = gate8_lifecycle_snapshot(
            8,
            generation,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pass,
            "none",
        );
        let receipt = Some(gate8_lifecycle_publication_receipt(8, generation, 1, 0));
        let mut lifecycle = super::DeferredGate8Lifecycle::new();
        assert_eq!(
            lifecycle.observe(1, started_ms, false, receipt, stable),
            super::DeferredGate8Observation::Pending,
        );
        assert!(matches!(
            lifecycle.observe(1, started_ms + 1, false, receipt, stable),
            super::DeferredGate8Observation::Publish {
                generation: published,
                ..
            } if published == generation
        ));
        assert!(lifecycle.accept_commit(generation));
        lifecycle
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    fn authorize_and_arm_gate8_handoff(
        lifecycle: &mut super::DeferredGate8Lifecycle,
        generation: u32,
        control_turn_started_ms: u64,
        activation_completed_ms: u64,
    ) {
        assert_eq!(
            lifecycle.authorize_handoff(
                control_turn_started_ms,
                generation,
                true,
                false,
                true,
                Some(15),
            ),
            super::DeferredGate8HandoffAdmission::Authorized { generation },
        );
        let plan = match lifecycle
            .prepare_child_ready_wait(gate8_absolute_clock_ms(activation_completed_ms), generation)
        {
            super::DeferredGate8ChildReadyPreparation::Prepared(plan) => plan,
            outcome => panic!("expected a child Ready plan, got {outcome:?}"),
        };
        assert!(matches!(
            lifecycle.arm_child_ready_wait(
                plan,
                gate8_absolute_clock_ms(activation_completed_ms),
                generation,
                true,
                false,
            ),
            super::DeferredGate8ChildReadyArm::Armed { .. }
        ));
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn cyw43_supervisor_pre_cutover_raw_fallback_receives_semantic_only() {
        let semantic = super::format_deferred_net_bootstrap_supervisor_semantic_status(
            0,
            super::DeferredNetSupervisorStatus::Preflight,
            super::SERIAL_LINKED_RUNTIME_RETRY_MS,
            250,
            false,
            true,
        )
        .expect("preflight supervisor semantic record must fit");

        assert!(semantic.ends_with("serial=blocked local_seat=enabled recovery=full"));
        for routing_field in ["console_seq=", "telemetry_sinks=", "prompt_refresh="] {
            assert_eq!(
                semantic.matches(routing_field).count(),
                0,
                "the raw fallback must append {routing_field} itself: {semantic}",
            );
        }
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn cyw43_supervisor_post_cutover_route_has_one_ordering_suffix() {
        let line = super::format_deferred_net_bootstrap_supervisor_status(
            17,
            1,
            super::DeferredNetSupervisorStatus::Begin,
            0,
            42,
            true,
            true,
        )
        .expect("linked supervisor record must fit");

        for routing_field in ["console_seq=", "telemetry_sinks=", "prompt_refresh="] {
            assert_eq!(
                line.matches(routing_field).count(),
                1,
                "the linked route must append {routing_field} exactly once: {line}",
            );
        }
        assert!(
            line.ends_with("console_seq=17 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes")
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn cyw43_supervisor_status_wire_record_is_lossless_at_every_numeric_bound() {
        let mut maximum_len = 0;
        for status in super::DeferredNetSupervisorStatus::ALL {
            let attempt = if status == super::DeferredNetSupervisorStatus::Preflight {
                0
            } else {
                super::CYW43_BOOTSTRAP_ATTEMPT
            };
            let line = super::format_deferred_net_bootstrap_supervisor_status(
                u64::MAX,
                attempt,
                status,
                u64::MAX,
                u64::MAX,
                false,
                false,
            )
            .expect("bounded supervisor status must fit the serial record");
            maximum_len = maximum_len.max(line.len());
            assert!(
                line.len() <= crate::serial::DEFAULT_LINE_CAPACITY,
                "status={status:?} len={} line={line}",
                line.len(),
            );
            assert!(line.ends_with(
                "recovery=full console_seq=18446744073709551615 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes"
            ));
        }
        assert_eq!(maximum_len, crate::serial::DEFAULT_LINE_CAPACITY);
        assert!(super::format_deferred_net_bootstrap_supervisor_status(
            1,
            0,
            super::DeferredNetSupervisorStatus::Begin,
            0,
            0,
            true,
            true,
        )
        .is_none());
        assert!(super::format_deferred_net_bootstrap_supervisor_status(
            1,
            1,
            super::DeferredNetSupervisorStatus::Preflight,
            0,
            0,
            true,
            true,
        )
        .is_none());
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn cyw43_gate8_subgate_wire_record_is_exact_and_bounded() {
        let line = super::format_deferred_net_gate8_subgate(
            u64::MAX,
            u32::MAX,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateDiagnostic {
                token: "8g-post-key-maintenance",
                status: crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pending,
                blocker: "logical-control-owner-active",
            },
        )
        .expect("Gate 8 subgate record must fit");

        assert_eq!(
            line.as_str(),
            "wifi: gate 8 subgate=8g-post-key-maintenance status=pending pair_epoch=18446744073709551615 generation=4294967295 blocker=logical-control-owner-active"
        );
        assert!(line.len() <= crate::serial::DEFAULT_LINE_CAPACITY);
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn cyw43_gate8_commit_is_distinct_from_supervisor_ready_and_bounded() {
        let diagnostic = gate8_lifecycle_snapshot(
            u64::MAX,
            u32::MAX,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pass,
            "none",
        );
        let line = super::format_deferred_net_gate8_commit(
            diagnostic,
            super::CYW43_BOOTSTRAP_ATTEMPT,
            u64::MAX,
            u64::MAX,
        )
        .expect("Gate 8 commit record must fit");

        assert!(line.starts_with("CYW43_GATE8_COMMIT attempt=1 status=ready "));
        assert!(line.contains("pair_epoch=18446744073709551615"));
        assert!(line.contains("generation=4294967295"));
        assert!(line.contains("deadline_ms=18446744073709551615"));
        assert!(!line.contains("CYW43_BOOTSTRAP_SUPERVISOR"));
        assert!(line.len() <= crate::serial::DEFAULT_LINE_CAPACITY);
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn cyw43_runtime_service_ready_cannot_duplicate_bootstrap_ready_schema() {
        let line = super::format_deferred_net_runtime_service_ready(u32::MAX, u64::MAX)
            .expect("runtime service-ready record must fit");

        assert_eq!(
            line.as_str(),
            "CYW43_RUNTIME_RECOVERY status=ready generation=4294967295 console_seq=18446744073709551615 telemetry_sinks=serial+qlog+hdmi",
        );
        assert!(!line.contains("CYW43_BOOTSTRAP_SUPERVISOR"));
        assert!(!line.contains("CYW43_GATE8_COMMIT"));
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn cyw43_supervisor_display_status_is_concise_and_machine_record_free() {
        for (attempt, status, backoff_ms, serial_ready, expected) in [
            (
                0,
                super::DeferredNetSupervisorStatus::Preflight,
                0,
                true,
                "[drivers] WiFi bootstrap pending; operator diagnostics available",
            ),
            (
                0,
                super::DeferredNetSupervisorStatus::Preflight,
                0,
                false,
                "[drivers] WiFi waiting for safe serial path; bootstrap paused",
            ),
            (
                1,
                super::DeferredNetSupervisorStatus::Begin,
                0,
                true,
                "[drivers] WiFi bootstrap starting (single production attempt)",
            ),
            (
                1,
                super::DeferredNetSupervisorStatus::Recovery,
                0,
                true,
                "[drivers] WiFi restoring previously ready CYW43/SDIO service",
            ),
            (
                1,
                super::DeferredNetSupervisorStatus::Stabilizing,
                0,
                true,
                "[drivers] WiFi transport attached; Gate 8 association security stabilizing",
            ),
            (
                1,
                super::DeferredNetSupervisorStatus::Ready,
                0,
                true,
                "[drivers] WiFi ready to use: DHCP bound; TCP console listening",
            ),
            (
                1,
                super::DeferredNetSupervisorStatus::Failed,
                0,
                true,
                "[drivers] WiFi startup failed; diagnostics remain active",
            ),
            (
                1,
                super::DeferredNetSupervisorStatus::Permanent,
                0,
                true,
                "[drivers] WiFi unavailable: non-retryable startup failure; diagnostics remain active",
            ),
        ] {
            let line = super::format_deferred_net_bootstrap_supervisor_display_status(
                attempt,
                status,
                backoff_ms,
                serial_ready,
            )
            .expect("bounded display status must fit");
            assert_eq!(line.as_str(), expected);
            assert!(!line.contains("CYW43_BOOTSTRAP_SUPERVISOR"));
        }
        assert!(!super::DeferredNetSupervisorStatus::Preflight.releases_hdmi_console_ready());
        assert!(!super::DeferredNetSupervisorStatus::Begin.releases_hdmi_console_ready());
        assert!(!super::DeferredNetSupervisorStatus::Recovery.releases_hdmi_console_ready());
        assert!(!super::DeferredNetSupervisorStatus::Stabilizing.releases_hdmi_console_ready());
        assert!(super::DeferredNetSupervisorStatus::Ready.releases_hdmi_console_ready());
        assert!(super::DeferredNetSupervisorStatus::Failed.releases_hdmi_console_ready());
        assert!(super::DeferredNetSupervisorStatus::Permanent.releases_hdmi_console_ready());
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn cyw43_recovery_status_requires_previously_ready_service() {
        assert_eq!(
            super::deferred_cyw43_supervisor_start_status(false, false),
            Some(super::DeferredNetSupervisorStatus::Begin),
        );
        assert_eq!(
            super::deferred_cyw43_supervisor_start_status(false, true),
            Some(super::DeferredNetSupervisorStatus::Begin),
            "an unattached lifetime can only publish the sole bootstrap begin",
        );
        assert_eq!(
            super::deferred_cyw43_supervisor_start_status(true, false),
            None,
            "pre-service terminal cleanup must not claim a recovery episode",
        );
        assert_eq!(
            super::deferred_cyw43_supervisor_start_status(true, true),
            Some(super::DeferredNetSupervisorStatus::Recovery),
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn cyw43_bootstrap_supervisor_has_one_attempt_and_no_backoff_state() {
        let mut sequence = super::DeferredNetSupervisorSequence::new();
        assert_eq!(sequence.next_status_sequence(), 1);
        assert_eq!(sequence.next_status_sequence(), 2);

        assert!(super::format_deferred_net_bootstrap_supervisor_status(
            1,
            super::CYW43_BOOTSTRAP_ATTEMPT,
            super::DeferredNetSupervisorStatus::Failed,
            0,
            u64::MAX,
            true,
            true,
        )
        .is_some());
        assert!(super::format_deferred_net_bootstrap_supervisor_status(
            2,
            2,
            super::DeferredNetSupervisorStatus::Begin,
            0,
            0,
            true,
            true,
        )
        .is_none());
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_stabilization_deadline_is_absolute_within_one_outer_attempt() {
        let pending = gate8_lifecycle_snapshot(
            4,
            9,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pending,
            "association-event-pending",
        );
        let receipt = Some(gate8_lifecycle_publication_receipt(4, 9, 1, 0));
        let mut lifecycle = super::DeferredGate8Lifecycle::new();

        assert_eq!(
            lifecycle.observe(1, 100, false, receipt, pending),
            super::DeferredGate8Observation::Pending,
        );
        assert_eq!(lifecycle.deadline_ms(), Some(90_100));
        assert_eq!(lifecycle.enter_stabilizing(1, 80_000), 90_100);
        assert_eq!(
            lifecycle.observe(1, 90_099, false, receipt, pending),
            super::DeferredGate8Observation::Pending,
        );
        assert_eq!(
            lifecycle.observe(1, 90_100, false, receipt, pending),
            super::DeferredGate8Observation::Deadline {
                generation: 9,
                deadline_ms: 90_100,
                blocker: "association-event-pending",
            },
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_protocol_failure_waits_for_deadline_without_opening_pair_repair() {
        let failed = gate8_lifecycle_snapshot(
            4,
            9,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Fail,
            "association-terminal-failure",
        );
        let receipt = Some(gate8_lifecycle_publication_receipt(4, 9, 1, 0));
        let mut lifecycle = super::DeferredGate8Lifecycle::new();

        assert_eq!(
            lifecycle.observe(1, 100, false, receipt, failed),
            super::DeferredGate8Observation::Pending,
            "a logical gate failure remains inside its bounded gate-local policy lane",
        );
        assert_eq!(lifecycle.enter_stabilizing(1, 1_100), 90_100);
        assert_eq!(lifecycle.deadline_ms(), Some(90_100));
        assert_eq!(
            lifecycle.observe(1, 90_100, false, receipt, failed),
            super::DeferredGate8Observation::Deadline {
                generation: 9,
                deadline_ms: 90_100,
                blocker: "association-terminal-failure",
            },
            "deadline exhaustion is terminal policy, not authority to reset the SDIO pair",
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_commit_requires_fresh_quiescence_and_retracts_on_proof_loss() {
        let stable = gate8_lifecycle_snapshot(
            8,
            12,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pass,
            "none",
        );
        let pending_same_generation = gate8_lifecycle_snapshot(
            8,
            12,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pending,
            "maintenance-owner-active",
        );
        let receipt = gate8_lifecycle_publication_receipt(8, 12, 1, 0);
        let producer_advanced = gate8_lifecycle_publication_receipt(8, 12, 1, 1);
        let epoch_advanced = gate8_lifecycle_publication_receipt(8, 12, 2, 1);
        let counters_advanced = crate::drivers::driver_task_net::Cyw43Gate8PublicationReceipt {
            dpc_overruns: 1,
            dpc_ack_failures: 1,
            ..epoch_advanced
        };
        let mut lifecycle = super::DeferredGate8Lifecycle::new();

        assert_eq!(
            lifecycle.observe(1, 1_000, false, Some(receipt), stable),
            super::DeferredGate8Observation::Pending,
            "the first exact quiescent snapshot only arms the commit candidate",
        );
        assert_eq!(
            lifecycle.observe(1, 1_001, false, None, stable),
            super::DeferredGate8Observation::Pending,
            "owner activity between stable samples clears the commit candidate",
        );
        assert_eq!(
            lifecycle.observe(1, 1_002, false, Some(receipt), stable),
            super::DeferredGate8Observation::Pending,
            "quiescence must be observed afresh after intervening owner activity",
        );
        assert_eq!(
            lifecycle.observe(1, 1_003, false, Some(producer_advanced), stable),
            super::DeferredGate8Observation::Pending,
            "DPC producer movement between samples resets the commit candidate",
        );
        assert_eq!(
            lifecycle.observe(1, 1_004, false, Some(epoch_advanced), stable),
            super::DeferredGate8Observation::Pending,
            "DPC epoch movement also resets the commit candidate",
        );
        assert_eq!(
            lifecycle.observe(1, 1_005, false, Some(counters_advanced), stable),
            super::DeferredGate8Observation::Pending,
            "cumulative DPC history movement also resets the commit candidate",
        );
        assert_eq!(
            lifecycle.observe(1, 1_006, false, Some(counters_advanced), stable),
            super::DeferredGate8Observation::Publish {
                generation: 12,
                publication_receipt: counters_advanced,
            },
            "a second unchanged ordinary control turn may publish",
        );
        assert!(lifecycle.reject_publication(12));
        assert_eq!(
            lifecycle.observe(1, 1_007, false, Some(counters_advanced), stable),
            super::DeferredGate8Observation::Pending,
            "failed publication resets the candidate and requires a fresh pair of snapshots",
        );
        assert_eq!(
            lifecycle.observe(1, 1_008, false, Some(counters_advanced), stable),
            super::DeferredGate8Observation::Publish {
                generation: 12,
                publication_receipt: counters_advanced,
            },
        );
        assert!(lifecycle.accept_commit(12));
        assert_eq!(
            lifecycle.observe(1, 1_009, true, None, stable),
            super::DeferredGate8Observation::Committed,
            "ordinary post-publication DPC/data ownership does not retract the Gate 8 commit",
        );
        assert_eq!(
            lifecycle.observe(
                1,
                1_010,
                false,
                Some(counters_advanced),
                pending_same_generation,
            ),
            super::DeferredGate8Observation::Retracted { generation: 12 },
            "loss of exact proof retracts the commit even without a generation change",
        );
        assert_eq!(
            lifecycle.deadline_ms(),
            Some(91_000),
            "pre-service-ready retraction must retain the original absolute deadline",
        );
        assert_eq!(
            lifecycle.observe(1, 1_011, false, Some(counters_advanced), stable),
            super::DeferredGate8Observation::Pending,
        );
        assert_eq!(
            lifecycle.observe(1, 1_012, false, Some(counters_advanced), stable),
            super::DeferredGate8Observation::Publish {
                generation: 12,
                publication_receipt: counters_advanced,
            },
        );
        assert!(lifecycle.accept_commit(12));
        authorize_and_arm_gate8_handoff(&mut lifecycle, 12, 1_012, 1_013);
        assert!(lifecycle.mark_service_ready(12, 1_027));
        assert!(!lifecycle.mark_service_ready(12, 1_027));
        assert_eq!(
            lifecycle.observe(1, 1_013, true, None, stable),
            super::DeferredGate8Observation::ServiceReady,
            "the terminal service-ready cut is distinct from Gate 8 commit",
        );
        assert_eq!(
            lifecycle.observe(
                1,
                2_000,
                false,
                Some(counters_advanced),
                pending_same_generation,
            ),
            super::DeferredGate8Observation::Retracted { generation: 12 },
        );
        assert_eq!(
            lifecycle.deadline_ms(),
            Some(92_000),
            "proof loss after service readiness begins a fresh bounded runtime episode",
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_reproof_preserves_pre_service_deadline_and_runtime_is_distinct() {
        let stable = gate8_lifecycle_snapshot(
            8,
            12,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pass,
            "none",
        );
        let receipt = gate8_lifecycle_publication_receipt(8, 12, 1, 0);
        let mut lifecycle = super::DeferredGate8Lifecycle::new();

        assert_eq!(
            lifecycle.observe(1, 100, false, Some(receipt), stable),
            super::DeferredGate8Observation::Pending,
        );
        assert_eq!(
            lifecycle.observe(1, 101, false, Some(receipt), stable),
            super::DeferredGate8Observation::Publish {
                generation: 12,
                publication_receipt: receipt,
            },
        );
        assert!(lifecycle.accept_commit(12));
        assert_eq!(
            lifecycle.enter_stabilizing(1, 50_000),
            90_100,
            "Gate 8 reproof before service readiness cannot renew the boot deadline",
        );

        assert_eq!(
            lifecycle.observe(1, 50_001, false, Some(receipt), stable),
            super::DeferredGate8Observation::Pending,
        );
        assert_eq!(
            lifecycle.observe(1, 50_002, false, Some(receipt), stable),
            super::DeferredGate8Observation::Publish {
                generation: 12,
                publication_receipt: receipt,
            },
        );
        assert!(lifecycle.accept_commit(12));
        authorize_and_arm_gate8_handoff(&mut lifecycle, 12, 50_002, 50_003);
        assert!(lifecycle.mark_service_ready(12, 50_017));
        assert_eq!(
            lifecycle.enter_stabilizing(1, 60_000),
            150_000,
            "a later recovery after service readiness starts a distinct bounded runtime episode",
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_commit_without_dhcp_listener_expires_the_original_boot_deadline() {
        let stable = gate8_lifecycle_snapshot(
            8,
            12,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pass,
            "none",
        );
        let receipt = gate8_lifecycle_publication_receipt(8, 12, 1, 0);
        let mut lifecycle = super::DeferredGate8Lifecycle::new();

        assert_eq!(
            lifecycle.observe(1, 100, false, Some(receipt), stable),
            super::DeferredGate8Observation::Pending,
        );
        assert_eq!(
            lifecycle.observe(1, 101, false, Some(receipt), stable),
            super::DeferredGate8Observation::Publish {
                generation: 12,
                publication_receipt: receipt,
            },
        );
        assert!(lifecycle.accept_commit(12));
        assert_eq!(
            lifecycle.service_readiness_deadline_expired(12, 90_099, None),
            None,
        );
        assert_eq!(
            lifecycle.service_readiness_deadline_expired(12, 90_100, None),
            Some(90_100),
            "pre-handoff DHCP/listener readiness retains the original boot deadline",
        );
        let mut ready_lifecycle = committed_gate8_lifecycle(100, 12);
        authorize_and_arm_gate8_handoff(&mut ready_lifecycle, 12, 80_000, 80_001);
        assert!(ready_lifecycle.mark_service_ready(12, 80_015));
        assert_eq!(
            ready_lifecycle.service_readiness_deadline_expired(12, u64::MAX, None),
            None,
            "service readiness converts later failures into runtime recovery rather than boot timeout",
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_post_handoff_ready_wait_uses_two_admitted_response_bounds() {
        assert_eq!(super::response_time_us_to_clock_ms(8_100), Some(9));
        assert_eq!(super::response_time_us_to_clock_ms(5_100), Some(6));
        assert_eq!(
            super::response_time_us_to_clock_ms(8_100)
                .and_then(|child| super::response_time_us_to_clock_ms(5_100)
                    .and_then(|root| child.checked_add(root))),
            Some(15),
            "separate millisecond rounding covers the child and root response observations",
        );
        assert_eq!(super::response_time_us_to_clock_ms(0), None);

        let mut lifecycle = committed_gate8_lifecycle(100, 12);
        assert_eq!(
            lifecycle.authorize_handoff(90_099, 12, true, false, true, Some(15)),
            super::DeferredGate8HandoffAdmission::Authorized { generation: 12 },
        );
        let plan = match lifecycle.prepare_child_ready_wait(gate8_absolute_clock_ms(90_100), 12) {
            super::DeferredGate8ChildReadyPreparation::Prepared(plan) => plan,
            outcome => panic!("expected a child Ready plan, got {outcome:?}"),
        };
        assert_eq!(
            lifecycle.arm_child_ready_wait(plan, gate8_absolute_clock_ms(90_103), 12, true, false,),
            super::DeferredGate8ChildReadyArm::Armed {
                deadline_ms: 90_118,
            },
            "the Ready bound starts after activation rather than during descriptor finalization",
        );
        assert_eq!(lifecycle.deadline_ms(), Some(90_118));
        assert_eq!(
            lifecycle.service_readiness_deadline_expired(12, 0, Some(90_117)),
            None,
            "an exact-generation Ready may still be admitted before the derived boundary",
        );
        assert!(lifecycle.mark_service_ready(12, 90_117));
        assert_eq!(
            lifecycle.service_readiness_deadline_expired(12, u64::MAX, None),
            None,
            "exact-generation Ready before the derived boundary closes bootstrap",
        );

        let mut between_resume_and_return = committed_gate8_lifecycle(100, 12);
        assert!(matches!(
            between_resume_and_return.authorize_handoff(90_099, 12, true, false, true, Some(15),),
            super::DeferredGate8HandoffAdmission::Authorized { .. }
        ));
        let between_plan = match between_resume_and_return
            .prepare_child_ready_wait(gate8_absolute_clock_ms(90_100), 12)
        {
            super::DeferredGate8ChildReadyPreparation::Prepared(plan) => plan,
            outcome => panic!("expected a child Ready plan, got {outcome:?}"),
        };
        assert!(matches!(
            between_resume_and_return.arm_child_ready_wait(
                between_plan,
                gate8_absolute_clock_ms(90_103),
                12,
                true,
                false,
            ),
            super::DeferredGate8ChildReadyArm::Armed {
                deadline_ms: 90_118,
            }
        ));
        assert!(
            between_resume_and_return.mark_service_ready(12, 90_101),
            "a Ready published after pre-resume sampling but before root returns from ResumeTCB is valid",
        );

        let mut missing_ready = committed_gate8_lifecycle(100, 12);
        assert_eq!(
            missing_ready.authorize_handoff(90_099, 12, true, false, true, Some(15)),
            super::DeferredGate8HandoffAdmission::Authorized { generation: 12 },
        );
        let missing_plan =
            match missing_ready.prepare_child_ready_wait(gate8_absolute_clock_ms(90_100), 12) {
                super::DeferredGate8ChildReadyPreparation::Prepared(plan) => plan,
                outcome => panic!("expected a child Ready plan, got {outcome:?}"),
            };
        assert!(matches!(
            missing_ready.arm_child_ready_wait(
                missing_plan,
                gate8_absolute_clock_ms(90_103),
                12,
                true,
                false,
            ),
            super::DeferredGate8ChildReadyArm::Armed { .. }
        ));
        assert!(!missing_ready.mark_service_ready(13, 90_117));
        assert_eq!(
            missing_ready.service_readiness_deadline_expired(12, 0, Some(90_118)),
            Some(90_118),
            "missing or wrong-generation Ready fails at the exact derived boundary",
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_post_handoff_invalid_bound_fails_closed_immediately() {
        let mut missing = committed_gate8_lifecycle(100, 12);
        assert_eq!(
            missing.authorize_handoff(500, 12, true, false, true, None),
            super::DeferredGate8HandoffAdmission::InvalidAuthority { generation: 12 },
        );
        assert!(!missing.handoff_authorization_pending());

        let mut zero = committed_gate8_lifecycle(100, 12);
        assert_eq!(
            zero.authorize_handoff(500, 12, true, false, true, Some(0)),
            super::DeferredGate8HandoffAdmission::InvalidAuthority { generation: 12 },
        );
        assert!(!zero.handoff_authorization_pending());

        for invalid_clock in [
            super::DeferredGate8AbsoluteClockSample {
                counter_ticks: 0,
                counter_hz: 1_000,
                generated_timer_hz: 1_000,
            },
            super::DeferredGate8AbsoluteClockSample {
                counter_ticks: 500,
                counter_hz: 0,
                generated_timer_hz: 1_000,
            },
            super::DeferredGate8AbsoluteClockSample {
                counter_ticks: 500,
                counter_hz: 1_000,
                generated_timer_hz: 999,
            },
        ] {
            let mut invalid = committed_gate8_lifecycle(100, 12);
            assert!(matches!(
                invalid.authorize_handoff(500, 12, true, false, true, Some(15)),
                super::DeferredGate8HandoffAdmission::Authorized { .. }
            ));
            assert_eq!(
                invalid.prepare_child_ready_wait(invalid_clock, 12),
                super::DeferredGate8ChildReadyPreparation::ClockInvalid,
            );
        }

        let mut invalid_post = committed_gate8_lifecycle(100, 12);
        assert!(matches!(
            invalid_post.authorize_handoff(500, 12, true, false, true, Some(15)),
            super::DeferredGate8HandoffAdmission::Authorized { .. }
        ));
        let invalid_post_plan =
            match invalid_post.prepare_child_ready_wait(gate8_absolute_clock_ms(501), 12) {
                super::DeferredGate8ChildReadyPreparation::Prepared(plan) => plan,
                outcome => panic!("expected a child Ready plan, got {outcome:?}"),
            };
        for invalid_clock in [
            gate8_absolute_clock_ms(500),
            super::DeferredGate8AbsoluteClockSample {
                counter_ticks: 502,
                counter_hz: 2_000,
                generated_timer_hz: 2_000,
            },
            super::DeferredGate8AbsoluteClockSample {
                counter_ticks: 502,
                counter_hz: 1_000,
                generated_timer_hz: 999,
            },
        ] {
            assert_eq!(
                invalid_post.arm_child_ready_wait(
                    invalid_post_plan,
                    invalid_clock,
                    12,
                    true,
                    false,
                ),
                super::DeferredGate8ChildReadyArm::ClockInvalid,
                "zero, backward, drifted, or generated/runtime-mismatched absolute clocks fail closed",
            );
        }

        let mut overflow = committed_gate8_lifecycle(100, 12);
        assert!(matches!(
            overflow.authorize_handoff(500, 12, true, false, true, Some(15)),
            super::DeferredGate8HandoffAdmission::Authorized { .. }
        ));
        assert_eq!(
            overflow.prepare_child_ready_wait(gate8_absolute_clock_ms(u64::MAX), 12),
            super::DeferredGate8ChildReadyPreparation::ClockOverflow,
        );
        let plan =
            match overflow.prepare_child_ready_wait(gate8_absolute_clock_ms(u64::MAX - 15), 12) {
                super::DeferredGate8ChildReadyPreparation::Prepared(plan) => plan,
                outcome => panic!("expected a boundary-safe Ready plan, got {outcome:?}"),
            };
        assert_eq!(
            overflow
                .arm_child_ready_wait(plan, gate8_absolute_clock_ms(u64::MAX), 12, true, false,),
            super::DeferredGate8ChildReadyArm::ClockOverflow,
            "clock advance during activation must fail before the child poll",
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_handoff_authorization_preserves_deadline_generation_and_recovery() {
        let mut on_time = committed_gate8_lifecycle(100, 12);
        assert_eq!(
            on_time.authorize_handoff(90_099, 12, true, false, true, Some(15)),
            super::DeferredGate8HandoffAdmission::Authorized { generation: 12 },
        );
        assert_eq!(
            on_time.revalidate_handoff(13, false, false, true),
            super::DeferredGate8HandoffRevalidation::ProofDrift { generation: 12 },
        );
        assert!(!on_time.handoff_authorization_pending());

        let mut at_deadline = committed_gate8_lifecycle(100, 12);
        assert_eq!(
            at_deadline.authorize_handoff(90_100, 12, true, false, true, Some(15)),
            super::DeferredGate8HandoffAdmission::Deadline {
                generation: 12,
                deadline_ms: 90_100,
            },
            "a control turn beginning at the expiry boundary is already late",
        );
        assert!(!at_deadline.handoff_authorization_pending());

        let mut late = committed_gate8_lifecycle(100, 12);
        assert!(matches!(
            late.authorize_handoff(90_101, 12, true, false, true, Some(15)),
            super::DeferredGate8HandoffAdmission::Deadline { .. }
        ));
        assert!(!late.handoff_authorization_pending());

        let mut drifted = committed_gate8_lifecycle(100, 12);
        assert_eq!(
            drifted.authorize_handoff(500, 13, false, false, true, Some(15)),
            super::DeferredGate8HandoffAdmission::ProofDrift { generation: 12 },
        );

        let mut recovery = committed_gate8_lifecycle(100, 12);
        assert_eq!(
            recovery.authorize_handoff(500, 12, true, true, true, Some(15)),
            super::DeferredGate8HandoffAdmission::RecoveryRequired,
        );
        assert!(!recovery.handoff_authorization_pending());
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_child_ready_arm_rejects_every_invalid_transition() {
        let awaiting_dhcp = committed_gate8_lifecycle(100, 12);
        assert_eq!(
            awaiting_dhcp.prepare_child_ready_wait(gate8_absolute_clock_ms(500), 12),
            super::DeferredGate8ChildReadyPreparation::InvalidPhase,
        );

        let mut lifecycle = committed_gate8_lifecycle(100, 12);
        assert!(matches!(
            lifecycle.authorize_handoff(500, 12, true, false, true, Some(15)),
            super::DeferredGate8HandoffAdmission::Authorized { .. }
        ));
        let plan = match lifecycle.prepare_child_ready_wait(gate8_absolute_clock_ms(501), 12) {
            super::DeferredGate8ChildReadyPreparation::Prepared(plan) => plan,
            outcome => panic!("expected a child Ready plan, got {outcome:?}"),
        };
        assert_eq!(
            lifecycle.arm_child_ready_wait(plan, gate8_absolute_clock_ms(502), 12, true, true,),
            super::DeferredGate8ChildReadyArm::RecoveryRequired,
        );
        assert_eq!(
            lifecycle.arm_child_ready_wait(plan, gate8_absolute_clock_ms(502), 13, false, false,),
            super::DeferredGate8ChildReadyArm::GenerationDrift,
        );
        assert_eq!(
            lifecycle.arm_child_ready_wait(plan, gate8_absolute_clock_ms(502), 12, true, false,),
            super::DeferredGate8ChildReadyArm::Armed { deadline_ms: 517 },
        );
        assert_eq!(
            lifecycle.arm_child_ready_wait(plan, gate8_absolute_clock_ms(502), 12, true, false,),
            super::DeferredGate8ChildReadyArm::AlreadyArmed,
        );
        assert_eq!(
            lifecycle.child_ready_pre_network_action(Some(518), 13, false, true, None),
            super::DeferredGate8ChildReadyPreNetworkAction::Continue,
            "transport recovery must pre-empt an expired or drifted Ready wait",
        );
        assert_eq!(
            super::deferred_cyw43_attached_turn(
                true,
                crate::drivers::driver_task_net::Cyw43CanonicalParentCut::Absent,
                false,
            ),
            super::DeferredCyw43AttachedTurn::RecoverySupervisor,
        );
        assert_eq!(
            lifecycle.child_ready_pre_network_action(Some(516), 13, false, false, None),
            super::DeferredGate8ChildReadyPreNetworkAction::Fail {
                generation: 12,
                blocker: "child-ready-generation-drift",
                deadline_ms: 516,
            },
        );
        assert_eq!(
            lifecycle.child_ready_pre_network_action(Some(518), 12, true, false, None),
            super::DeferredGate8ChildReadyPreNetworkAction::ObservePublicationOnly {
                generation: 12,
                deadline_ms: 517,
            },
            "the exact boundary admits one final durable-publication observation",
        );
        assert_eq!(
            lifecycle.child_ready_post_observation_failure(12, None),
            Some((12, "service-readiness-deadline", 517)),
        );
        assert_eq!(
            lifecycle.child_ready_pre_network_action(Some(519), 12, true, false, None),
            super::DeferredGate8ChildReadyPreNetworkAction::Fail {
                generation: 12,
                blocker: "service-readiness-deadline",
                deadline_ms: 517,
            },
            "a missing Ready cannot obtain a second deadline observation",
        );
        let drifted = gate8_lifecycle_snapshot(
            8,
            13,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pass,
            "none",
        );
        assert_eq!(
            lifecycle.observe(1, 503, false, None, drifted),
            super::DeferredGate8Observation::Deadline {
                generation: 12,
                deadline_ms: 503,
                blocker: "child-ready-generation-drift",
            },
            "proof drift after activation must terminalize before another child poll",
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_child_ready_deadline_uses_one_strict_publication_arbitration() {
        fn armed() -> super::DeferredGate8Lifecycle {
            let mut lifecycle = committed_gate8_lifecycle(100, 12);
            authorize_and_arm_gate8_handoff(&mut lifecycle, 12, 500, 502);
            assert_eq!(lifecycle.deadline_ms(), Some(517));
            lifecycle
        }

        let mut on_time_late_observed = armed();
        assert_eq!(
            on_time_late_observed.child_ready_pre_network_action(Some(518), 12, true, false, None,),
            super::DeferredGate8ChildReadyPreNetworkAction::ObservePublicationOnly {
                generation: 12,
                deadline_ms: 517,
            },
        );
        assert_eq!(
            on_time_late_observed.child_ready_post_observation_failure(12, Some(516)),
            None,
            "an ABI-validated pre-deadline publication survives delayed root observation",
        );
        assert!(on_time_late_observed.child_ready_publication_on_time(12, Some(516)));
        assert!(on_time_late_observed.mark_service_ready(12, 516));

        let mut exact_lower_bound = armed();
        assert!(exact_lower_bound.child_ready_publication_on_time(12, Some(502)));
        assert!(exact_lower_bound.mark_service_ready(12, 502));

        for published_ms in [0, 501] {
            let mut pre_arm = armed();
            assert!(matches!(
                pre_arm.child_ready_pre_network_action(Some(518), 12, true, false, None),
                super::DeferredGate8ChildReadyPreNetworkAction::ObservePublicationOnly { .. }
            ));
            assert_eq!(
                pre_arm.child_ready_post_observation_failure(12, Some(published_ms)),
                Some((12, "service-readiness-deadline", 517)),
                "a zero or pre-resume Ready timestamp cannot satisfy this handoff",
            );
            assert!(!pre_arm.child_ready_publication_on_time(12, Some(published_ms)));
            assert!(!pre_arm.mark_service_ready(12, published_ms));
        }

        for published_ms in [517, 518] {
            let mut late = armed();
            assert!(matches!(
                late.child_ready_pre_network_action(Some(518), 12, true, false, None),
                super::DeferredGate8ChildReadyPreNetworkAction::ObservePublicationOnly { .. }
            ));
            assert_eq!(
                late.child_ready_post_observation_failure(12, Some(published_ms)),
                Some((12, "service-readiness-deadline", 517)),
                "Ready at or after the exact boundary remains terminal",
            );
            assert!(!late.child_ready_publication_on_time(12, Some(published_ms)));
            assert!(!late.mark_service_ready(12, published_ms));
        }

        let mut missing = armed();
        assert!(matches!(
            missing.child_ready_pre_network_action(Some(517), 12, true, false, None),
            super::DeferredGate8ChildReadyPreNetworkAction::ObservePublicationOnly { .. }
        ));
        assert_eq!(
            missing.child_ready_post_observation_failure(12, None),
            Some((12, "service-readiness-deadline", 517)),
        );
        assert_eq!(
            missing.child_ready_pre_network_action(Some(518), 12, true, false, None),
            super::DeferredGate8ChildReadyPreNetworkAction::Fail {
                generation: 12,
                blocker: "service-readiness-deadline",
                deadline_ms: 517,
            },
            "the deadline arbitration is consumed exactly once",
        );

        let mut drifted = armed();
        assert_eq!(
            drifted.child_ready_pre_network_action(Some(516), 13, false, false, None),
            super::DeferredGate8ChildReadyPreNetworkAction::Fail {
                generation: 12,
                blocker: "child-ready-generation-drift",
                deadline_ms: 516,
            },
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn selected_child_ready_authority_requires_both_active_admitted_tasks() {
        let generated = crate::generated::temporal_tasks();
        assert_eq!(
            super::cyw43_child_ready_response_bound_ms(),
            super::child_ready_response_bound_ms(generated),
            "the runtime wrapper must resolve the currently selected generated profile",
        );

        let mut child = *generated
            .iter()
            .find(|task| task.id == "console-network-service")
            .expect("selected profile must declare console-network-service");
        let mut root = *generated
            .iter()
            .find(|task| task.id == "root-control")
            .expect("selected profile must declare root-control");
        child.response_time_us = 8_100;
        root.response_time_us = 5_100;
        let exact = [child, root];
        assert_eq!(super::child_ready_response_bound_ms(&exact), Some(15));
        assert_eq!(super::child_ready_response_bound_ms(&[root]), None);
        assert_eq!(super::child_ready_response_bound_ms(&[child]), None);

        let mut inactive = child;
        inactive.execution = crate::generated::TemporalExecution::Passive;
        assert_eq!(
            super::child_ready_response_bound_ms(&[inactive, root]),
            None,
        );
        let mut not_admitted = child;
        not_admitted.admitted = false;
        assert_eq!(
            super::child_ready_response_bound_ms(&[not_admitted, root]),
            None,
        );
        let mut zero = child;
        zero.response_time_us = 0;
        assert_eq!(super::child_ready_response_bound_ms(&[zero, root]), None);
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_late_post_secure_maintenance_preserves_ready_data_continuity() {
        let stable = gate8_lifecycle_snapshot(
            8,
            12,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pass,
            "none",
        );
        let mut maintenance = stable;
        maintenance.subgates[6] = crate::drivers::driver_task_net::Cyw43Gate8SubgateDiagnostic {
            token: "8g-post-key-maintenance",
            status: crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pending,
            blocker: "host-eapol-owner-active",
        };
        maintenance.subgates[7] = crate::drivers::driver_task_net::Cyw43Gate8SubgateDiagnostic {
            token: "8h-data-admission",
            status: crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pending,
            blocker: "8g-post-key-maintenance",
        };
        maintenance.current_work_pending = true;
        let receipt = Some(gate8_lifecycle_publication_receipt(8, 12, 1, 0));
        let mut lifecycle = super::DeferredGate8Lifecycle::new();

        assert_eq!(
            lifecycle.observe(1, 100, false, receipt, stable),
            super::DeferredGate8Observation::Pending,
        );
        assert_eq!(
            lifecycle.observe(1, 101, false, receipt, stable),
            super::DeferredGate8Observation::Publish {
                generation: 12,
                publication_receipt: receipt.expect("receipt must be present"),
            },
        );
        assert!(lifecycle.accept_commit(12));

        assert_eq!(
            lifecycle.observe(1, 180_000, true, None, maintenance),
            super::DeferredGate8Observation::Committed,
            "bounded same-pair post-secure maintenance keeps the published data lane ready",
        );
        assert_eq!(
            lifecycle.deadline_ms(),
            Some(90_100),
            "maintenance does not create or renew a Gate 8 boot deadline",
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_stable_snapshot_at_the_absolute_deadline_fails_closed() {
        let stable = gate8_lifecycle_snapshot(
            8,
            12,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pass,
            "none",
        );
        let receipt = Some(gate8_lifecycle_publication_receipt(8, 12, 1, 0));
        let mut lifecycle = super::DeferredGate8Lifecycle::new();
        assert_eq!(lifecycle.enter_stabilizing(1, 500), 90_500);

        assert_eq!(
            lifecycle.observe(1, 90_500, false, receipt, stable),
            super::DeferredGate8Observation::Deadline {
                generation: 12,
                deadline_ms: 90_500,
                blocker: "gate8-stabilization-deadline",
            },
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn permanent_attached_wifi_failure_never_reenters_supervisor_driver_turns() {
        assert_eq!(super::permanent_failure_terminal_mode(false), None);
        let terminal = super::permanent_failure_terminal_mode(true);
        assert_eq!(
            terminal,
            Some(super::DeferredNetSupervisorTerminal::PermanentAttachedWifiFailure),
        );
        for _ in 0..4_096 {
            assert!(!super::deferred_net_supervisor_driver_turn_allowed(
                terminal
            ));
        }
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_terminal_pending_linearizes_at_explicit_decision_cut() {
        use core::cell::Cell;

        assert!(super::gate8_terminal_pending_cancels_for_recovery(
            false, true
        ));
        assert!(!super::gate8_terminal_pending_cancels_for_recovery(
            false, false
        ));
        assert!(
            !super::gate8_terminal_pending_cancels_for_recovery(true, true),
            "a typed recovery cannot reopen driver work after the explicit terminal decision commits",
        );
        assert!(!super::gate8_terminal_pending_cancels_for_recovery(
            true, false
        ));
        assert!(super::gate8_terminal_pending_yields_to_finite_owner(
            false, true
        ));
        assert!(!super::gate8_terminal_pending_yields_to_finite_owner(
            true, true
        ));
        assert!(!super::gate8_terminal_pending_yields_to_finite_owner(
            false, false
        ));
        assert!(super::gate8_terminal_decision_cut_open(false, false));
        assert!(!super::gate8_terminal_decision_cut_open(true, false));
        assert!(!super::gate8_terminal_decision_cut_open(false, true));
        assert!(!super::gate8_terminal_decision_cut_open(true, true));

        let retractions = Cell::new(0u32);
        let mut committed = false;
        assert!(!super::commit_gate8_terminal_decision(
            &mut committed,
            12,
            true,
            false,
            |_| {
                retractions.set(retractions.get().saturating_add(1));
                true
            },
        ));
        assert!(!committed);
        assert_eq!(retractions.get(), 0, "recovery retains the data consumer");
        assert!(!super::commit_gate8_terminal_decision(
            &mut committed,
            12,
            false,
            true,
            |_| {
                retractions.set(retractions.get().saturating_add(1));
                true
            },
        ));
        assert!(!committed);
        assert_eq!(
            retractions.get(),
            0,
            "an unfinished finite owner retains the data consumer"
        );
        assert!(super::commit_gate8_terminal_decision(
            &mut committed,
            12,
            false,
            false,
            |_| {
                retractions.set(retractions.get().saturating_add(1));
                true
            },
        ));
        assert!(committed);
        assert_eq!(retractions.get(), 1);
        assert!(super::commit_gate8_terminal_decision(
            &mut committed,
            12,
            true,
            true,
            |_| {
                retractions.set(retractions.get().saturating_add(1));
                true
            },
        ));
        assert_eq!(
            retractions.get(),
            1,
            "a retained decision cannot retract the exact consumer twice"
        );

        let mut stale_generation_committed = false;
        assert!(!super::commit_gate8_terminal_decision(
            &mut stale_generation_committed,
            12,
            false,
            false,
            |_| false,
        ));
        assert!(
            !stale_generation_committed,
            "a newer publication owner must defeat the stale terminal cut"
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn attached_gate8_network_turn_polls_before_committing_the_fresh_logical_generation() {
        use core::cell::Cell;

        assert_eq!(
            super::deferred_cyw43_attached_turn(
                false,
                crate::drivers::driver_task_net::Cyw43CanonicalParentCut::Absent,
                false,
            ),
            super::DeferredCyw43AttachedTurn::NetworkControl,
        );
        assert_eq!(
            super::deferred_cyw43_attached_turn(
                true,
                crate::drivers::driver_task_net::Cyw43CanonicalParentCut::Absent,
                false,
            ),
            super::DeferredCyw43AttachedTurn::RecoverySupervisor,
        );
        assert_eq!(
            super::deferred_cyw43_attached_turn(
                true,
                crate::drivers::driver_task_net::Cyw43CanonicalParentCut::Runnable {
                    generation: 7,
                    request: 64,
                },
                false,
            ),
            super::DeferredCyw43AttachedTurn::NetworkControl,
            "a committed exact terminal keeps its canonical policy turn",
        );

        let stage = Cell::new(0u8);
        let logical_generation = Cell::new(0u32);
        let diagnostic_calls = Cell::new(0u8);
        let committed = Cell::new(false);
        let productive_successor = super::run_deferred_cyw43_attached_network_control_turn(
            || {
                assert_eq!(stage.get(), 1, "pre-poll evidence must be retained first");
                assert!(!committed.get(), "handoff cannot commit before the poll");
                logical_generation.set(23);
                stage.set(2);
                true
            },
            || {
                assert_eq!(stage.get(), 2, "recovery is checked after the poll");
                stage.set(3);
                false
            },
            || {
                let call = diagnostic_calls.get();
                diagnostic_calls.set(call + 1);
                match call {
                    0 => assert_eq!(stage.get(), 0),
                    1 => assert_eq!(stage.get(), 3),
                    _ => panic!("one turn takes exactly two Gate 8 snapshots"),
                }
                gate8_lifecycle_snapshot(
                    7,
                    logical_generation.get(),
                    crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pending,
                    "data-handoff-commit-pending",
                )
            },
            |diagnostic| {
                assert_eq!(stage.get(), 0);
                assert_eq!(diagnostic.generation, 0);
                stage.set(1);
            },
            |generation| {
                assert_eq!(stage.get(), 3);
                assert_eq!(generation, 23, "commit must use the post-poll generation");
                committed.set(true);
                stage.set(4);
                true
            },
        );
        assert_eq!(stage.get(), 4);
        assert_eq!(diagnostic_calls.get(), 2);
        assert!(committed.get());
        assert!(productive_successor);

        let recovery_stage = Cell::new(0u8);
        let recovery_diagnostic_calls = Cell::new(0u8);
        let recovery_commit_called = Cell::new(false);
        let recovery_successor = super::run_deferred_cyw43_attached_network_control_turn(
            || {
                assert_eq!(recovery_stage.get(), 1);
                recovery_stage.set(2);
                true
            },
            || {
                assert_eq!(recovery_stage.get(), 2);
                recovery_stage.set(3);
                true
            },
            || {
                recovery_diagnostic_calls.set(recovery_diagnostic_calls.get() + 1);
                gate8_lifecycle_snapshot(
                    7,
                    0,
                    crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pending,
                    "join-submit-pending",
                )
            },
            |_| recovery_stage.set(1),
            |_| {
                recovery_commit_called.set(true);
                true
            },
        );
        assert_eq!(recovery_stage.get(), 3);
        assert_eq!(recovery_diagnostic_calls.get(), 1);
        assert!(
            !recovery_commit_called.get(),
            "post-poll recovery forbids handoff commit"
        );
        assert!(
            !recovery_successor,
            "a recovery edge rejects a productive attached successor"
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn attached_console_handoff_gets_one_exclusive_turn_after_dhcp() {
        use crate::drivers::driver_task_net::Cyw43CanonicalParentCut;

        assert_eq!(
            super::deferred_cyw43_attached_turn(false, Cyw43CanonicalParentCut::Absent, true,),
            super::DeferredCyw43AttachedTurn::ConsoleHandoff,
            "only lifecycle-authorized DHCP truth may schedule the deferred child handoff",
        );
        assert_eq!(
            super::deferred_cyw43_attached_turn(true, Cyw43CanonicalParentCut::Absent, true,),
            super::DeferredCyw43AttachedTurn::RecoverySupervisor,
            "a transport edge must pre-empt a pending handoff",
        );
        assert_eq!(
            super::deferred_cyw43_attached_turn(
                true,
                Cyw43CanonicalParentCut::Waiting {
                    generation: 7,
                    request: 64,
                },
                true,
            ),
            super::DeferredCyw43AttachedTurn::CanonicalWait,
            "a retained canonical operation must finish before handoff",
        );
        assert_eq!(
            super::deferred_cyw43_attached_turn(
                true,
                Cyw43CanonicalParentCut::Runnable {
                    generation: 7,
                    request: 64,
                },
                true,
            ),
            super::DeferredCyw43AttachedTurn::NetworkControl,
            "a runnable canonical operation keeps its exact network turn",
        );
        assert!(super::deferred_cyw43_post_network_control_handoff_due(
            false, true,
        ));
        assert!(
            !super::deferred_cyw43_post_network_control_handoff_due(true, true),
            "a newly visible transport edge must pre-empt post-poll handoff",
        );
        assert!(
            !super::deferred_cyw43_post_network_control_handoff_due(false, false),
            "an expired deadline without exact DHCP eligibility remains terminal",
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn attached_recovery_scheduler_is_network_then_operator_then_driver() {
        assert_eq!(
            super::deferred_cyw43_attached_turn(
                false,
                crate::drivers::driver_task_net::Cyw43CanonicalParentCut::Absent,
                false,
            ),
            super::DeferredCyw43AttachedTurn::NetworkControl,
        );
        assert_eq!(
            super::deferred_cyw43_attached_turn(
                true,
                crate::drivers::driver_task_net::Cyw43CanonicalParentCut::Absent,
                false,
            ),
            super::DeferredCyw43AttachedTurn::RecoverySupervisor,
            "typed recovery leaves both hardware lanes to the common phase alternator",
        );
        assert_eq!(
            super::deferred_cyw43_attached_turn(
                true,
                crate::drivers::driver_task_net::Cyw43CanonicalParentCut::Waiting {
                    generation: 7,
                    request: 64,
                },
                false,
            ),
            super::DeferredCyw43AttachedTurn::CanonicalWait,
            "an exact waiting parent rotates through operator/recheck only",
        );

        let (operator, after_operator) = super::deferred_cyw43_supervisor_phase_step(
            super::DeferredCyw43SupervisorPhase::Operator,
            true,
            true,
        );
        assert_eq!(operator, super::DeferredCyw43SupervisorTurn::Operator);
        assert_eq!(after_operator, super::DeferredCyw43SupervisorPhase::Driver);

        let (driver, after_driver) =
            super::deferred_cyw43_supervisor_phase_step(after_operator, true, true);
        assert_eq!(driver, super::DeferredCyw43SupervisorTurn::Driver);
        assert_eq!(after_driver, super::DeferredCyw43SupervisorPhase::Operator);

        let (blocked, after_blocked) = super::deferred_cyw43_supervisor_phase_step(
            super::DeferredCyw43SupervisorPhase::Driver,
            false,
            true,
        );
        assert_eq!(blocked, super::DeferredCyw43SupervisorTurn::Blocked);
        assert_eq!(
            after_blocked,
            super::DeferredCyw43SupervisorPhase::Operator,
            "a reboot or lost serial proof must return Driver to Operator without child work",
        );

        let (waiting, after_waiting) = super::deferred_cyw43_supervisor_phase_step(
            super::DeferredCyw43SupervisorPhase::Operator,
            true,
            false,
        );
        assert_eq!(waiting, super::DeferredCyw43SupervisorTurn::Operator);
        assert_eq!(
            after_waiting,
            super::DeferredCyw43SupervisorPhase::Operator,
            "a durable persistent wait must not schedule repeated Driver turns",
        );

        let (resumed, after_resumed) =
            super::deferred_cyw43_supervisor_phase_step(after_waiting, true, true);
        assert_eq!(resumed, super::DeferredCyw43SupervisorTurn::Operator);
        assert_eq!(after_resumed, super::DeferredCyw43SupervisorPhase::Driver);
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn pi_wifi_supervisor_never_schedules_consecutive_driver_turns() {
        let (operator, mut phase) = super::deferred_cyw43_supervisor_phase_step(
            super::DeferredCyw43SupervisorPhase::Operator,
            true,
            true,
        );
        assert_eq!(operator, super::DeferredCyw43SupervisorTurn::Operator);
        assert_eq!(phase, super::DeferredCyw43SupervisorPhase::Driver);
        for _ in 0..128 {
            let (turn, next) = super::deferred_cyw43_supervisor_phase_step(phase, true, true);
            match phase {
                super::DeferredCyw43SupervisorPhase::Operator => {
                    assert_eq!(turn, super::DeferredCyw43SupervisorTurn::Operator);
                    assert_eq!(next, super::DeferredCyw43SupervisorPhase::Driver);
                }
                super::DeferredCyw43SupervisorPhase::Driver => {
                    assert_eq!(turn, super::DeferredCyw43SupervisorTurn::Driver);
                    assert_eq!(next, super::DeferredCyw43SupervisorPhase::Operator);
                }
            }
            phase = next;
        }
        assert_eq!(phase, super::DeferredCyw43SupervisorPhase::Driver);
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn pi_wifi_activation_guard_reserves_epilogue_and_fails_closed() {
        let pi_reserve = super::deferred_cyw43_activation_reserve_us(2_750, 2_500, true);
        assert_eq!(pi_reserve, Some(250));
        assert_eq!(
            super::deferred_cyw43_activation_reserve_us(2_750, 0, true),
            None
        );
        assert_eq!(
            super::deferred_cyw43_activation_reserve_us(2_500, 2_500, true),
            None
        );
        assert_eq!(
            super::deferred_cyw43_activation_reserve_us(2_750, 2_500, false),
            None
        );

        let mut window = super::DeferredCyw43ActivationWindow::new();
        assert!(window.turn_admitted(100, 1_000_000, pi_reserve));
        window.record_operator_turn();
        assert!(window.turn_admitted(349, 1_000_000, pi_reserve));
        assert!(
            !window.turn_admitted(350, 1_000_000, pi_reserve),
            "the exact 250 us Pi reserve leaves the complete declared 2,500 us WCET before the 2,750 us SC boundary",
        );

        window.reset();
        assert!(window.turn_admitted(500, 54_000_000, pi_reserve));
        for invalid in [
            (499, 54_000_000),
            (501, 0),
            (501, 24_000_000),
            (0, 54_000_000),
        ] {
            assert!(
                !window.turn_admitted(invalid.0, invalid.1, pi_reserve),
                "backward, zero, or frequency-drifted counters fail closed: {invalid:?}",
            );
        }

        window.reset();
        assert!(
            window.turn_admitted(0, 0, pi_reserve),
            "an invalid counter retains one legacy logical turn for liveness",
        );
        window.record_operator_turn();
        assert!(
            !window.turn_admitted(1, 1, pi_reserve),
            "an invalid activation cannot upgrade after spending its sole logical turn",
        );
        window.reset();
        assert!(window.turn_admitted(0, 54_000_000, None));
        window.record_driver_turn(false);
        assert!(!window.turn_admitted(0, 54_000_000, None));

        window.reset();
        assert!(window.turn_admitted(1, 54_000_000, pi_reserve));
        for _ in 0..super::DEFERRED_CYW43_ACTIVATION_MAX_PRODUCTIVE_UNITS {
            window.record_driver_turn(true);
        }
        assert!(
            !window.turn_admitted(2, 54_000_000, pi_reserve),
            "the hard productive-turn cap ends even a sub-guard activation",
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn pi_wifi_mcs_continuation_accepts_only_productive_alternation() {
        use super::{
            DeferredCyw43McsContinuation as Continuation, DeferredCyw43SupervisorPhase as Phase,
            DeferredCyw43SupervisorTurn as Turn,
        };

        assert_eq!(
            super::deferred_cyw43_mcs_continuation(
                Turn::Operator,
                Phase::Driver,
                false,
                true,
                false,
            ),
            Continuation::Continue,
        );
        assert_eq!(
            super::deferred_cyw43_mcs_continuation(
                Turn::Driver,
                Phase::Operator,
                true,
                true,
                false,
            ),
            Continuation::Continue,
        );
        for (turn, next, operation_executed, operator_admitted_before, network_attached) in [
            (Turn::Operator, Phase::Operator, false, true, false),
            (Turn::Operator, Phase::Driver, false, false, false),
            (Turn::Operator, Phase::Driver, true, true, false),
            (Turn::Driver, Phase::Operator, false, true, false),
            (Turn::Driver, Phase::Driver, true, true, false),
            (Turn::Blocked, Phase::Operator, false, true, false),
            (Turn::Blocked, Phase::Driver, true, true, false),
            (Turn::Operator, Phase::Driver, false, true, true),
            (Turn::Driver, Phase::Operator, true, true, true),
        ] {
            assert_eq!(
                super::deferred_cyw43_mcs_continuation(
                    turn,
                    next,
                    operation_executed,
                    operator_admitted_before,
                    network_attached,
                ),
                Continuation::Yield,
                "blocked, nonproductive, terminal, and nonalternating cuts yield",
            );
        }
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn bootstrap_operator_rechecks_durable_sideband_without_granting_driver() {
        use core::cell::Cell;

        let stable_reads = Cell::new(0u8);
        assert!(super::service_deferred_cyw43_bootstrap_sideband_condition(
            || {
                stable_reads.set(stable_reads.get().saturating_add(1));
                true
            },
        ));
        assert_eq!(
            stable_reads.get(),
            1,
            "one operator turn takes one stable read"
        );

        let (turn, next) = super::deferred_cyw43_supervisor_phase_step(
            super::DeferredCyw43SupervisorPhase::Operator,
            true,
            false,
        );
        assert_eq!(turn, super::DeferredCyw43SupervisorTurn::Operator);
        assert_eq!(
            next,
            super::DeferredCyw43SupervisorPhase::Operator,
            "a sideband ACK is a child hint, not parent Driver authority",
        );

        assert!(!super::service_deferred_cyw43_bootstrap_sideband_condition(
            || {
                stable_reads.set(stable_reads.get().saturating_add(1));
                false
            },
        ));
        assert_eq!(
            stable_reads.get(),
            2,
            "absence is rechecked without a latch"
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn linked_serial_proof_miss_retries_without_abandoning_wifi_supervision() {
        let mut retry = super::DeferredSerialRouteRetry::new(100);
        assert!(retry.probe_due(100));
        assert!(retry.record_missing_proof(100));
        assert!(!retry.probe_due(349));
        assert!(retry.probe_due(350));
        assert!(!retry.record_missing_proof(350));
        assert!(retry.probe_due(600));
        retry.record_ready();
        assert!(retry.record_missing_proof(600));
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn cyw43_turn_status_reports_transitions_and_power_of_two_repeats() {
        let mut status = super::DeferredCyw43TurnStatus::new();
        assert_eq!(status.observe("sdio-engine-init"), Some(1));
        assert_eq!(status.observe("sdio-engine-init"), Some(2));
        assert_eq!(status.observe("sdio-engine-init"), None);
        assert_eq!(status.observe("sdio-engine-init"), Some(4));
        status.retry_last();
        assert_eq!(status.observe("sdio-engine-init"), Some(4));
        assert_eq!(status.observe("cyw43-engine-init"), Some(1));
        status.retry_last();
        assert_eq!(status.observe("cyw43-engine-init"), Some(1));
        status.reset();
        assert_eq!(status.observe("cyw43-firmware-transport"), Some(1));
    }

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

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn production_console_and_wifi_retained_state_fit_one_root_stack_budget() {
        type ProductionPump<'a> = crate::event::EventPump<
            'a,
            crate::serial::kernel_uart::KernelSerialDriver,
            crate::kernel::KernelTimer,
            crate::kernel::KernelIpc,
            crate::event::TicketTable<{ crate::generated::TICKET_COUNT }>,
            { crate::serial::DEFAULT_RX_CAPACITY },
            { crate::serial::DEFAULT_TX_CAPACITY },
            { crate::serial::DEFAULT_LINE_CAPACITY },
        >;

        const RETAINED_STATE_BUDGET: usize = 128 * 1024;
        let retained = core::mem::size_of::<ProductionPump<'static>>()
            + core::mem::size_of::<super::Cyw43BootstrapSupervisor>();
        assert!(
            retained <= RETAINED_STATE_BUDGET,
            "retained console and Wi-Fi state must leave at least half of the 256-KiB root stack for bounded call frames"
        );
    }
}
