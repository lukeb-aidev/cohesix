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
#[cfg(all(feature = "kernel", feature = "net-console"))]
use crate::drivers::driver_task_net::{Cyw43BootstrapSupervisor, Cyw43BootstrapTurnOutcome};
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
            // physical Wi-Fi bootstrap. The supervisor below polls these
            // operator surfaces between complete, fenced retry attempts.
            start_root_console_prompt(&mut pump);
            #[cfg(all(feature = "net-console", feature = "kernel"))]
            {
                if let Some(config) = net_deferred_config.take() {
                    let resume_line = "[net-console] deferred resume reason=post-root-prompt action=start-persistent-wifi-supervisor";
                    if !pump.queue_cyw43_bootstrap_operator_line(resume_line) {
                        boot_log::force_uart_line(resume_line);
                    }
                    enter_root_console_loop_with_deferred_net_supervisor(
                        &mut pump,
                        config,
                        ctx.wifi_debug_hal_ptr,
                    );
                }
            }
            enter_root_console_loop(&mut pump);
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
            enter_root_console_loop(&mut pump);
        } else {
            #[cfg(feature = "net-console")]
            {
                attach_network(&mut pump, None, net_unavailable_detail.take());
            }
            start_root_console_prompt(&mut pump);
            enter_root_console_loop(&mut pump);
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
    run_root_console_pump(pump);
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
        pump.poll();
        sel4::yield_now();
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
// Linux brcmfmac bounds SDIO access-error persistence at five. Cohesix uses
// that finite budget as an analogue for one whole retained supervisor episode;
// this does not assert that Linux retries its complete bootstrap identically.
const CYW43_BOOTSTRAP_MAX_ATTEMPTS: u32 = 5;

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
const CYW43_BOOTSTRAP_RETRY_BACKOFF_MS: [u64; 4] = [1_000, 2_000, 4_000, 8_000];

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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredNetSupervisorTerminal {
    RetryBudgetExhausted,
    PermanentAttachedRecoveryFailure,
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
        Some(DeferredNetSupervisorTerminal::PermanentAttachedRecoveryFailure)
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeferredNetRetrySchedule {
    transient_failures: u32,
    next_attempt_ms: u64,
    status_sequence: u64,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
impl DeferredNetRetrySchedule {
    const fn new(now_ms: u64) -> Self {
        Self {
            transient_failures: 0,
            next_attempt_ms: now_ms,
            status_sequence: 0,
        }
    }

    const fn attempt_due(self, now_ms: u64) -> bool {
        !self.exhausted() && now_ms >= self.next_attempt_ms
    }

    const fn attempt_number(self) -> u32 {
        let next = self.transient_failures.saturating_add(1);
        if next > CYW43_BOOTSTRAP_MAX_ATTEMPTS {
            CYW43_BOOTSTRAP_MAX_ATTEMPTS
        } else {
            next
        }
    }

    const fn exhausted(self) -> bool {
        self.transient_failures >= CYW43_BOOTSTRAP_MAX_ATTEMPTS
    }

    fn record_transient_failure(&mut self, now_ms: u64) -> Option<u64> {
        if self.exhausted() {
            return None;
        }
        let failed_attempt = self.transient_failures;
        self.transient_failures = self.transient_failures.saturating_add(1);
        if self.exhausted() {
            self.next_attempt_ms = u64::MAX;
            return None;
        }
        let index = usize::try_from(failed_attempt)
            .unwrap_or(usize::MAX)
            .min(CYW43_BOOTSTRAP_RETRY_BACKOFF_MS.len() - 1);
        let delay_ms = CYW43_BOOTSTRAP_RETRY_BACKOFF_MS[index];
        self.next_attempt_ms = now_ms.saturating_add(delay_ms);
        Some(delay_ms)
    }

    fn reset_attempt_budget(&mut self, now_ms: u64) {
        self.transient_failures = 0;
        self.next_attempt_ms = now_ms;
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
    },
    Recovering {
        failed_attempt: u32,
        deadline_ms: u64,
    },
    Ready {
        generation: u32,
        attempt: u32,
        deadline_ms: u64,
        gate10_complete: bool,
    },
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
    },
    Ready,
    Retracted {
        generation: u32,
    },
    Fail {
        generation: u32,
        blocker: &'static str,
    },
    Deadline {
        generation: u32,
        deadline_ms: u64,
    },
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredGate8RecoveryBudget {
    AlreadyRecorded,
    Backoff { delay_ms: u64, next_attempt_ms: u64 },
    Exhausted,
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
    /// A linked-pair repair may complete while the same outer attempt remains
    /// active. In that case the original absolute deadline is retained: an
    /// internal recovery cannot manufacture a fresh unbounded wait.
    fn enter_stabilizing(&mut self, attempt: u32, now_ms: u64) -> u64 {
        match *self {
            Self::Stabilizing {
                attempt: active_attempt,
                deadline_ms,
            } if active_attempt == attempt => {
                return deadline_ms;
            }
            Self::Ready {
                attempt,
                deadline_ms,
                gate10_complete: false,
                ..
            } => {
                *self = Self::Stabilizing {
                    attempt,
                    deadline_ms,
                };
                return deadline_ms;
            }
            Self::Detached
            | Self::Stabilizing { .. }
            | Self::Recovering { .. }
            | Self::Ready { .. } => {}
        }
        let deadline_ms = now_ms.saturating_add(CYW43_GATE8_STABILIZATION_TIMEOUT_MS);
        *self = Self::Stabilizing {
            attempt,
            deadline_ms,
        };
        deadline_ms
    }

    fn begin_recovery(&mut self, failed_attempt: u32) -> bool {
        let Self::Stabilizing {
            attempt,
            deadline_ms,
        } = *self
        else {
            return false;
        };
        if attempt != failed_attempt {
            return false;
        }
        *self = Self::Recovering {
            failed_attempt,
            deadline_ms,
        };
        true
    }

    fn observe(
        &mut self,
        attempt: u32,
        now_ms: u64,
        accepted_proof_still_stable: bool,
        diagnostic: crate::drivers::driver_task_net::Cyw43Gate8Diagnostic,
    ) -> DeferredGate8Observation {
        if matches!(self, Self::Recovering { .. }) {
            return DeferredGate8Observation::Pending;
        }
        if let Self::Ready { generation, .. } = *self {
            if accepted_proof_still_stable
                && diagnostic.stable()
                && diagnostic.generation == generation
            {
                return DeferredGate8Observation::Ready;
            }
            self.enter_stabilizing(attempt, now_ms);
            return DeferredGate8Observation::Retracted { generation };
        }

        let deadline_ms = self.enter_stabilizing(attempt, now_ms);
        if diagnostic.frontier_status()
            == crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Fail
        {
            let blocker = diagnostic
                .subgates
                .iter()
                .find(|subgate| {
                    subgate.status == crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Fail
                })
                .map_or("gate8-failed", |subgate| subgate.blocker);
            return DeferredGate8Observation::Fail {
                generation: diagnostic.generation,
                blocker,
            };
        }
        if now_ms >= deadline_ms {
            return DeferredGate8Observation::Deadline {
                generation: diagnostic.generation,
                deadline_ms,
            };
        }
        if diagnostic.stable() {
            return DeferredGate8Observation::Publish {
                generation: diagnostic.generation,
            };
        }
        DeferredGate8Observation::Pending
    }

    fn accept_ready(&mut self, generation: u32) -> bool {
        let Self::Stabilizing {
            attempt,
            deadline_ms,
        } = *self
        else {
            return false;
        };
        *self = Self::Ready {
            generation,
            attempt,
            deadline_ms,
            gate10_complete: false,
        };
        true
    }

    fn mark_gate10_complete(&mut self, generation: u32) -> bool {
        let Self::Ready {
            generation: ready_generation,
            gate10_complete,
            ..
        } = self
        else {
            return false;
        };
        if *ready_generation != generation {
            return false;
        }
        *gate10_complete = true;
        true
    }

    const fn deadline_ms(self) -> Option<u64> {
        match self {
            Self::Stabilizing { deadline_ms, .. }
            | Self::Recovering { deadline_ms, .. }
            | Self::Ready { deadline_ms, .. } => Some(deadline_ms),
            Self::Detached => None,
        }
    }

    fn consume_failure(
        &mut self,
        failed_attempt: u32,
        now_ms: u64,
        retry_schedule: &mut DeferredNetRetrySchedule,
    ) -> DeferredGate8RecoveryBudget {
        if !self.begin_recovery(failed_attempt) {
            return DeferredGate8RecoveryBudget::AlreadyRecorded;
        }
        match retry_schedule.record_transient_failure(now_ms) {
            Some(delay_ms) => DeferredGate8RecoveryBudget::Backoff {
                delay_ms,
                next_attempt_ms: retry_schedule.next_attempt_ms,
            },
            None => DeferredGate8RecoveryBudget::Exhausted,
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
    Backoff,
    Stabilizing,
    Ready,
    Exhausted,
    Permanent,
}

#[cfg(all(
    feature = "serial-console",
    feature = "kernel",
    feature = "net-console"
))]
impl DeferredNetSupervisorStatus {
    const ALL: [Self; 8] = [
        Self::Preflight,
        Self::Begin,
        Self::Recovery,
        Self::Backoff,
        Self::Stabilizing,
        Self::Ready,
        Self::Exhausted,
        Self::Permanent,
    ];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::Begin => "begin",
            Self::Recovery => "recovery",
            Self::Backoff => "backoff",
            Self::Stabilizing => "stabilizing",
            Self::Ready => "ready",
            Self::Exhausted => "exhausted",
            Self::Permanent => "permanent",
        }
    }

    const fn valid_attempt(self, attempt: u32) -> bool {
        match self {
            Self::Preflight => attempt == 0,
            _ => attempt != 0 && attempt <= CYW43_BOOTSTRAP_MAX_ATTEMPTS,
        }
    }

    const fn releases_hdmi_console_ready(self) -> bool {
        matches!(self, Self::Ready | Self::Exhausted | Self::Permanent)
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
    let semantic_backoff_ms = if matches!(
        status,
        DeferredNetSupervisorStatus::Preflight | DeferredNetSupervisorStatus::Backoff
    ) {
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
    backoff_ms: u64,
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
        DeferredNetSupervisorStatus::Begin => write!(
            line,
            "[drivers] WiFi bootstrap attempt {attempt}/{CYW43_BOOTSTRAP_MAX_ATTEMPTS} starting"
        )
        .is_err(),
        DeferredNetSupervisorStatus::Recovery => write!(
            line,
            "[drivers] WiFi recovery attempt {attempt}/{CYW43_BOOTSTRAP_MAX_ATTEMPTS} starting"
        )
        .is_err(),
        DeferredNetSupervisorStatus::Backoff => write!(
            line,
            "[drivers] WiFi attempt {attempt}/{CYW43_BOOTSTRAP_MAX_ATTEMPTS} paused; retry in {backoff_ms} ms"
        )
        .is_err(),
        DeferredNetSupervisorStatus::Stabilizing => line
            .push_str("[drivers] WiFi transport attached; Gate 8 association security stabilizing")
            .is_err(),
        DeferredNetSupervisorStatus::Ready => line
            .push_str(crate::local_seat::CYW43_GATE8_READY_HDMI_LINE)
            .is_err(),
        DeferredNetSupervisorStatus::Exhausted => write!(
            line,
            "[drivers] WiFi unavailable after {CYW43_BOOTSTRAP_MAX_ATTEMPTS} attempts; diagnostics remain active"
        )
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
fn emit_deferred_net_gate8_ready_transaction<
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
    next_attempt_ms: u64,
    local_seat_enabled: bool,
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
    let serial_ready = !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
        || crate::serial::serial_linked_runtime_transport_active();
    let Some(semantic) = format_deferred_net_bootstrap_supervisor_semantic_status(
        attempt,
        DeferredNetSupervisorStatus::Ready,
        0,
        next_attempt_ms,
        serial_ready,
        local_seat_enabled,
    ) else {
        return false;
    };
    let Some(serial_terminal) =
        format_deferred_net_bootstrap_supervisor_linked_route(semantic.as_str(), console_sequence)
    else {
        return false;
    };
    let Some(hdmi_terminal) = format_deferred_net_bootstrap_supervisor_display_status(
        attempt,
        DeferredNetSupervisorStatus::Ready,
        0,
        serial_ready,
    ) else {
        return false;
    };
    pump.queue_cyw43_gate8_ready_transaction(
        lines.as_slice(),
        serial_terminal.as_str(),
        hdmi_terminal.as_str(),
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
>(
    pump: &mut EventPump<'a, D, T, I, V, RX, TX, LINE>,
    diagnostic: crate::drivers::driver_task_net::Cyw43Gate8Diagnostic,
    recovery_line: &str,
) -> bool
where
    D: crate::serial::SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    let Some(snapshot_lines) = format_deferred_net_gate8_snapshot_lines(diagnostic) else {
        return false;
    };
    let mut lines = heapless::Vec::<HeaplessString<DEFAULT_LINE_CAPACITY>, 9>::new();
    for line in snapshot_lines {
        if lines.push(line).is_err() {
            return false;
        }
    }
    let mut recovery = HeaplessString::new();
    if recovery.push_str(recovery_line).is_err() || lines.push(recovery).is_err() {
        return false;
    }
    // A terminal attempt publishes exactly one passive eight-line snapshot
    // immediately followed by its recovery boundary. The pair cannot restart
    // until the complete causal batch and one following Backoff, Exhausted, or
    // Permanent supervisor terminal slot are retained.
    pump.queue_cyw43_bootstrap_operator_lines_atomic(lines.as_slice(), 1)
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
fn with_deferred_net_hal<R>(
    hal_ptr: usize,
    operation: impl FnOnce(&mut KernelHal<'static>) -> R,
) -> Option<R> {
    let hal_ptr = core::ptr::NonNull::new(hal_ptr as *mut KernelHal<'static>)?;
    // SAFETY: kernel bootstrap leaks this `KernelHal` for the root-task
    // lifetime. The deferred supervisor is single-threaded and calls this
    // helper only after its prior operation and the EventPump operator turn
    // have returned, so each mutable borrow is bounded to this closure.
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
    hal_ptr: usize,
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
    let local_seat_enabled = crate::generated::hardware_config().local_seat.enabled;
    let mut retry_schedule = DeferredNetRetrySchedule::new(crate::hal::timebase().now_ms());
    if hal_ptr == 0 {
        let mut detail = HeaplessString::<192>::new();
        let _ = detail.push_str("deferred HAL pointer missing");
        emit_deferred_net_console_failure(pump, &detail, true);
        emit_deferred_net_bootstrap_supervisor_status(
            pump,
            retry_schedule.next_status_sequence(),
            1,
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
                retry_schedule.next_status_sequence(),
                1,
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
    let mut attempt_active = false;
    let mut network_attached = false;
    let mut gate8_lifecycle = DeferredGate8Lifecycle::new();
    let mut terminal_mode = None;
    let mut wifi_operation_started = false;
    let mut serial_retry = DeferredSerialRouteRetry::new(crate::hal::timebase().now_ms());
    let mut turn_status = DeferredCyw43TurnStatus::new();
    let mut supervisor_phase = DeferredCyw43SupervisorPhase::Operator;

    loop {
        if !deferred_net_supervisor_driver_turn_allowed(terminal_mode) {
            // A finite failed Wi-Fi episode must not become a second boot
            // failure. Ordinary EventPump ownership keeps serial, local-seat,
            // HDMI, diagnostics, authentication, and reboot live, while the
            // terminal state prevents another child operation. An attached
            // poisoned stack was quarantined before this mode was entered.
            pump.poll();
            sel4::yield_now();
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
                            retry_schedule.next_status_sequence(),
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
                                retry_schedule.next_status_sequence(),
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
            sel4::yield_now();
            continue;
        }

        // Once the stack is attached, the retained supervisor remains its
        // lifetime recovery owner. Ordinary network work gets one outer pump
        // turn only while the linked pair is ready and no sticky recovery
        // signal is pending. Gate 8 then consumes one passive snapshot. Its
        // deadline belongs to the outer attempt and survives an internal pair
        // recovery; neither polling nor a replay can silently renew it.
        if network_attached && bootstrap.is_ready() {
            if crate::drivers::driver_task_net::cyw43_recovery_required() {
                // Sticky recovery forbids ordinary network/SDIO work, but it
                // must not freeze the retained serial lane. One hardware-free
                // supervisor turn drains proof capacity so the atomic
                // 8a-through-8h plus Recovery transaction can be retained
                // before the pair restart begins.
                pump.poll_cyw43_bootstrap_supervisor_event_turn();
            } else {
                // NetStack is attached, but no ordinary poll for this
                // generation may run until the single handoff commit rejects
                // stale tokens and publishes its loss baseline. Valid
                // current-generation backlog remains queued for the
                // immediately following consumer turn.
                let handoff_committed = bootstrap.ready_generation().is_some_and(|generation| {
                    crate::drivers::driver_task_net::commit_cyw43_data_handoff_if_ready(generation)
                });
                if handoff_committed {
                    pump.poll();
                } else {
                    // Preserve operator liveness without consuming network
                    // data or issuing an alternate physical-driver operation.
                    pump.poll_cyw43_bootstrap_supervisor_event_turn();
                }
            }
            let stability_now_ms = crate::hal::timebase().now_ms();
            let recovery_required = crate::drivers::driver_task_net::cyw43_recovery_required();
            let attempt = retry_schedule.attempt_number();
            let diagnostic = crate::drivers::driver_task_net::cyw43_gate8_diagnostic();
            let observation = gate8_lifecycle.observe(
                attempt,
                stability_now_ms,
                bootstrap.gate8_generation_still_stable(),
                diagnostic,
            );
            let mut failure = None;
            match observation {
                DeferredGate8Observation::Pending => {}
                DeferredGate8Observation::Publish { generation } if !recovery_required => {
                    // The driver revalidates and commits this exact passive
                    // snapshot before it can become visible as readiness
                    // proof. The eight-line queue operation is atomic and
                    // reserves the immediately following Ready record. Any
                    // publication rejection retracts the accepted generation,
                    // so the next turn must capture and validate a fresh copy.
                    if !bootstrap.mark_gate8_generation_stable(diagnostic) {
                        crate::log_buffer::append_log_line(
                            "CYW43_GATE8_SNAPSHOT_COMMIT status=rejected action=retry-fresh-snapshot",
                        );
                        sel4::yield_now();
                        continue;
                    }
                    let ready_sequence = retry_schedule.next_status_sequence();
                    let ready_queued = emit_deferred_net_gate8_ready_transaction(
                        pump,
                        diagnostic,
                        ready_sequence,
                        attempt,
                        stability_now_ms,
                        local_seat_enabled,
                    );
                    if ready_queued && gate8_lifecycle.accept_ready(generation) {
                        crate::log_buffer::append_log_line(
                            "[net-console] CYW43 Gate 8 stable for current generation",
                        );
                    } else {
                        let _ = bootstrap.retract_gate8_generation(generation);
                        crate::log_buffer::append_log_line(
                            "CYW43_GATE8_READY_TRANSACTION status=failed action=retract-and-retry-fresh-snapshot",
                        );
                    }
                }
                DeferredGate8Observation::Publish { .. } => {}
                DeferredGate8Observation::Ready if !recovery_required => {
                    if bootstrap.mark_ready_generation_stable(
                        pump.net_console_cyw43_gate10_proven_for_root(),
                    ) {
                        if gate8_lifecycle.mark_gate10_complete(diagnostic.generation) {
                            retry_schedule.reset_attempt_budget(stability_now_ms);
                            crate::log_buffer::append_log_line(
                                "[net-console] CYW43 recovery budget reset after same-generation nettest/TCP/cohsh proof",
                            );
                        } else {
                            crate::log_buffer::append_log_line(
                                "CYW43_GATE10_LIFECYCLE status=generation-mismatch action=preserve-retry-budget",
                            );
                        }
                    }
                }
                DeferredGate8Observation::Ready => {}
                DeferredGate8Observation::Retracted { generation } => {
                    let _ = bootstrap.retract_gate8_generation(generation);
                    pump.defer_local_seat_hdmi_ready_until_cyw43_terminal();
                    let deadline_ms = match gate8_lifecycle.deadline_ms() {
                        Some(deadline_ms) => deadline_ms,
                        None => stability_now_ms,
                    };
                    emit_deferred_net_bootstrap_supervisor_status(
                        pump,
                        retry_schedule.next_status_sequence(),
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
                DeferredGate8Observation::Fail {
                    generation,
                    blocker,
                } => {
                    let deadline_ms = match gate8_lifecycle.deadline_ms() {
                        Some(deadline_ms) => deadline_ms,
                        None => stability_now_ms,
                    };
                    failure = Some((generation, blocker, deadline_ms));
                }
                DeferredGate8Observation::Deadline {
                    generation,
                    deadline_ms,
                } => {
                    failure = Some((generation, "gate8-stabilization-deadline", deadline_ms));
                }
            }

            if let Some((generation, blocker, deadline_ms)) = failure {
                let mut line = HeaplessString::<224>::new();
                let _ = write!(
                    line,
                    "CYW43_GATE8_RECOVERY attempt={} generation={} blocker={} deadline_ms={} action=pair-restart",
                    attempt, generation, blocker, deadline_ms,
                );
                if !emit_deferred_net_gate8_failure_transaction(pump, diagnostic, line.as_str()) {
                    crate::log_buffer::append_log_line(
                        "CYW43_GATE8_FAILURE_TRANSACTION status=not-retained action=wait-for-serial-capacity",
                    );
                    sel4::yield_now();
                    continue;
                }
                if !bootstrap.request_gate8_stabilization_recovery(blocker) {
                    emit_deferred_net_bootstrap_supervisor_status(
                        pump,
                        retry_schedule.next_status_sequence(),
                        attempt,
                        DeferredNetSupervisorStatus::Permanent,
                        0,
                        stability_now_ms,
                        local_seat_enabled,
                        false,
                    );
                    pump.quarantine_network_service_after_cyw43_exhaustion();
                    terminal_mode =
                        Some(DeferredNetSupervisorTerminal::PermanentAttachedRecoveryFailure);
                    attempt_active = false;
                    sel4::yield_now();
                    continue;
                }
                match gate8_lifecycle.consume_failure(
                    attempt,
                    stability_now_ms,
                    &mut retry_schedule,
                ) {
                    DeferredGate8RecoveryBudget::AlreadyRecorded => {}
                    DeferredGate8RecoveryBudget::Backoff {
                        delay_ms,
                        next_attempt_ms,
                    } => {
                        emit_deferred_net_bootstrap_supervisor_status(
                            pump,
                            retry_schedule.next_status_sequence(),
                            attempt,
                            DeferredNetSupervisorStatus::Backoff,
                            delay_ms,
                            next_attempt_ms,
                            local_seat_enabled,
                            false,
                        );
                        attempt_active = false;
                    }
                    DeferredGate8RecoveryBudget::Exhausted => {
                        emit_deferred_net_bootstrap_supervisor_status(
                            pump,
                            retry_schedule.next_status_sequence(),
                            attempt,
                            DeferredNetSupervisorStatus::Exhausted,
                            0,
                            retry_schedule.next_attempt_ms,
                            local_seat_enabled,
                            false,
                        );
                        pump.quarantine_network_service_after_cyw43_exhaustion();
                        terminal_mode = Some(DeferredNetSupervisorTerminal::RetryBudgetExhausted);
                        attempt_active = false;
                    }
                }
                sel4::yield_now();
                continue;
            }

            if !recovery_required {
                sel4::yield_now();
                continue;
            }
        }

        if supervisor_phase == DeferredCyw43SupervisorPhase::Operator {
            // Operator service and CYW43/SDIO service occupy distinct outer
            // turns. Even an active serial command therefore cannot compose
            // with a Wi-Fi child operation in one scheduler iteration.
            pump.poll_cyw43_bootstrap_supervisor_event_turn();
            if !pump.cyw43_bootstrap_may_begin() {
                sel4::yield_now();
                continue;
            }
            let now_ms = crate::hal::timebase().now_ms();
            if !attempt_active {
                if !retry_schedule.attempt_due(now_ms) {
                    sel4::yield_now();
                    continue;
                }
                emit_deferred_net_bootstrap_supervisor_status(
                    pump,
                    retry_schedule.next_status_sequence(),
                    retry_schedule.attempt_number(),
                    if network_attached {
                        DeferredNetSupervisorStatus::Recovery
                    } else {
                        DeferredNetSupervisorStatus::Begin
                    },
                    0,
                    now_ms,
                    local_seat_enabled,
                    false,
                );
                attempt_active = true;
            }
            supervisor_phase = DeferredCyw43SupervisorPhase::Driver;
            sel4::yield_now();
            continue;
        }

        let now_ms = crate::hal::timebase().now_ms();
        let attempt = retry_schedule.attempt_number();
        wifi_operation_started = true;
        crate::drivers::driver_task_net::begin_cyw43_outer_event_turn();
        let Some(turn) = with_deferred_net_hal(hal_ptr, |hal| bootstrap.service_turn(hal)) else {
            // The entry check above proves this unreachable unless the
            // retained bootstrap pointer was corrupted after validation.
            run_root_console_pump(pump);
        };
        supervisor_phase = DeferredCyw43SupervisorPhase::Operator;
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
                    let mut operator_line = HeaplessString::<192>::new();
                    let _ = write!(
                        operator_line,
                        "CYW43_BOOTSTRAP_TURN attempt={} turn={} stage={} operation={} repeat={}",
                        attempt, turn_id, stage, operation_executed, repeat,
                    );
                    // `with_deferred_net_hal` has returned, so this only queues
                    // bytes for the independent linked serial runtime. The next
                    // outer EventPump turn performs the actual flush.
                    if !pump.queue_cyw43_bootstrap_operator_line(operator_line.as_str()) {
                        turn_status.retry_last();
                    }
                }
            }
            Cyw43BootstrapTurnOutcome::Complete => {
                turn_status.reset();
                if !bootstrap.is_ready() || bootstrap.ready_generation().is_none() {
                    let mut detail = HeaplessString::<192>::new();
                    let _ = detail.push_str("retained supervisor completed without generation");
                    emit_deferred_net_console_failure(pump, &detail, false);
                    emit_deferred_net_bootstrap_supervisor_status(
                        pump,
                        retry_schedule.next_status_sequence(),
                        attempt,
                        DeferredNetSupervisorStatus::Permanent,
                        0,
                        now_ms,
                        local_seat_enabled,
                        false,
                    );
                    if let Some(mode) = permanent_failure_terminal_mode(network_attached) {
                        pump.quarantine_network_service_after_cyw43_exhaustion();
                        terminal_mode = Some(mode);
                        sel4::yield_now();
                        continue;
                    }
                    run_root_console_pump(pump);
                }
                if network_attached {
                    let retracted_generation = match gate8_lifecycle {
                        DeferredGate8Lifecycle::Ready { generation, .. } => Some(generation),
                        DeferredGate8Lifecycle::Detached
                        | DeferredGate8Lifecycle::Stabilizing { .. }
                        | DeferredGate8Lifecycle::Recovering { .. } => None,
                    };
                    let gate8_deadline_ms = gate8_lifecycle.enter_stabilizing(attempt, now_ms);
                    if let Some(generation) = retracted_generation {
                        let _ = bootstrap.retract_gate8_generation(generation);
                        pump.defer_local_seat_hdmi_ready_until_cyw43_terminal();
                    }
                    emit_deferred_net_bootstrap_supervisor_status(
                        pump,
                        retry_schedule.next_status_sequence(),
                        attempt,
                        DeferredNetSupervisorStatus::Stabilizing,
                        0,
                        gate8_deadline_ms,
                        local_seat_enabled,
                        false,
                    );
                    attempt_active = false;
                    sel4::yield_now();
                    continue;
                }
                let Some(stack_result) = with_deferred_net_hal(hal_ptr, |hal| {
                    crate::net::finish_cyw43_net_console_after_bootstrap(hal, bootstrap.config())
                }) else {
                    // The same validated leaked pointer is reused only after
                    // the retained operation above has released its borrow.
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
                            retry_schedule.next_status_sequence(),
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
                        retry_schedule.next_status_sequence(),
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
                    retry_schedule.next_status_sequence(),
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
                turn_status.reset();
                let err = crate::net::map_cyw43_bootstrap_error(driver_error);
                let failure_now_ms = crate::hal::timebase().now_ms();
                let mut detail = HeaplessString::<192>::new();
                let _ = write!(detail, "{err}");
                emit_deferred_net_console_failure(pump, &detail, false);
                if crate::net::cyw43_net_console_bootstrap_error_retryable(&err) {
                    if let Some(delay_ms) = retry_schedule.record_transient_failure(failure_now_ms)
                    {
                        emit_deferred_net_bootstrap_supervisor_status(
                            pump,
                            retry_schedule.next_status_sequence(),
                            attempt,
                            DeferredNetSupervisorStatus::Backoff,
                            delay_ms,
                            retry_schedule.next_attempt_ms,
                            local_seat_enabled,
                            false,
                        );
                        bootstrap.reset_for_attempt(config);
                    } else {
                        emit_deferred_net_bootstrap_supervisor_status(
                            pump,
                            retry_schedule.next_status_sequence(),
                            attempt,
                            DeferredNetSupervisorStatus::Exhausted,
                            0,
                            retry_schedule.next_attempt_ms,
                            local_seat_enabled,
                            false,
                        );
                        pump.quarantine_network_service_after_cyw43_exhaustion();
                        terminal_mode = Some(DeferredNetSupervisorTerminal::RetryBudgetExhausted);
                    }
                    attempt_active = false;
                } else {
                    emit_deferred_net_bootstrap_supervisor_status(
                        pump,
                        retry_schedule.next_status_sequence(),
                        attempt,
                        DeferredNetSupervisorStatus::Permanent,
                        0,
                        failure_now_ms,
                        local_seat_enabled,
                        false,
                    );
                    if let Some(mode) = permanent_failure_terminal_mode(network_attached) {
                        pump.quarantine_network_service_after_cyw43_exhaustion();
                        terminal_mode = Some(mode);
                    } else {
                        run_root_console_pump(pump);
                    }
                }
            }
        }
        sel4::yield_now();
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
        let policy = affinity::policy();
        affinity::with_role_affinity(affinity::AffinityRole::NineDoor, 0, &policy, || {
            pump.attach_ninedoor(ninedoor);
        });
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
                super::CYW43_BOOTSTRAP_MAX_ATTEMPTS
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
                "[drivers] WiFi bootstrap attempt 1/5 starting",
            ),
            (
                2,
                super::DeferredNetSupervisorStatus::Recovery,
                0,
                true,
                "[drivers] WiFi recovery attempt 2/5 starting",
            ),
            (
                3,
                super::DeferredNetSupervisorStatus::Backoff,
                u64::MAX,
                true,
                "[drivers] WiFi attempt 3/5 paused; retry in 18446744073709551615 ms",
            ),
            (
                4,
                super::DeferredNetSupervisorStatus::Stabilizing,
                0,
                true,
                "[drivers] WiFi transport attached; Gate 8 association security stabilizing",
            ),
            (
                4,
                super::DeferredNetSupervisorStatus::Ready,
                0,
                true,
                "[drivers] WiFi Gate 8 stable; DHCP and TCP continuing",
            ),
            (
                5,
                super::DeferredNetSupervisorStatus::Exhausted,
                0,
                true,
                "[drivers] WiFi unavailable after 5 attempts; diagnostics remain active",
            ),
            (
                5,
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
        assert!(!super::DeferredNetSupervisorStatus::Backoff.releases_hdmi_console_ready());
        assert!(!super::DeferredNetSupervisorStatus::Stabilizing.releases_hdmi_console_ready());
        assert!(super::DeferredNetSupervisorStatus::Ready.releases_hdmi_console_ready());
        assert!(super::DeferredNetSupervisorStatus::Exhausted.releases_hdmi_console_ready());
        assert!(super::DeferredNetSupervisorStatus::Permanent.releases_hdmi_console_ready());
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn cyw43_bootstrap_supervisor_stops_after_five_attempts() {
        let mut supervisor = super::DeferredNetRetrySchedule::new(100);
        assert!(supervisor.attempt_due(100));
        assert_eq!(supervisor.attempt_number(), 1);
        assert!(!supervisor.exhausted());

        let expected = [1_000, 2_000, 4_000, 8_000];
        let mut now_ms = 100;
        for (index, expected_delay) in expected.into_iter().enumerate() {
            let delay = supervisor.record_transient_failure(now_ms);
            assert_eq!(delay, Some(expected_delay));
            assert!(!supervisor.attempt_due(supervisor.next_attempt_ms.saturating_sub(1)));
            assert!(supervisor.attempt_due(supervisor.next_attempt_ms));
            assert_eq!(supervisor.attempt_number(), index as u32 + 2);
            assert!(!supervisor.exhausted());
            now_ms = supervisor.next_attempt_ms;
        }

        assert_eq!(supervisor.attempt_number(), 5);
        assert_eq!(supervisor.record_transient_failure(now_ms), None);
        assert!(supervisor.exhausted());
        assert_eq!(supervisor.next_attempt_ms, u64::MAX);
        assert!(!supervisor.attempt_due(u64::MAX));
        assert_eq!(supervisor.record_transient_failure(u64::MAX), None);
        assert_eq!(supervisor.attempt_number(), 5);
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn successful_cyw43_episode_resets_the_next_recovery_budget() {
        let mut supervisor = super::DeferredNetRetrySchedule::new(100);
        assert_eq!(supervisor.record_transient_failure(100), Some(1_000));
        assert_eq!(supervisor.record_transient_failure(1_100), Some(2_000));
        assert_eq!(supervisor.attempt_number(), 3);

        supervisor.reset_attempt_budget(3_100);

        assert_eq!(supervisor.attempt_number(), 1);
        assert!(supervisor.attempt_due(3_100));
        assert!(!supervisor.exhausted());
        assert_eq!(supervisor.record_transient_failure(3_100), Some(1_000));
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
        let mut lifecycle = super::DeferredGate8Lifecycle::new();

        assert_eq!(
            lifecycle.observe(1, 100, false, pending),
            super::DeferredGate8Observation::Pending,
        );
        assert_eq!(lifecycle.deadline_ms(), Some(90_100));
        assert_eq!(lifecycle.enter_stabilizing(1, 80_000), 90_100);
        assert_eq!(
            lifecycle.observe(1, 90_099, false, pending),
            super::DeferredGate8Observation::Pending,
        );
        assert_eq!(
            lifecycle.observe(1, 90_100, false, pending),
            super::DeferredGate8Observation::Deadline {
                generation: 9,
                deadline_ms: 90_100,
            },
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_failure_consumes_one_outer_attempt_and_cannot_double_count() {
        let failed = gate8_lifecycle_snapshot(
            4,
            9,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Fail,
            "association-terminal-failure",
        );
        let mut lifecycle = super::DeferredGate8Lifecycle::new();
        let mut retry_schedule = super::DeferredNetRetrySchedule::new(100);

        assert_eq!(
            lifecycle.observe(1, 100, false, failed),
            super::DeferredGate8Observation::Fail {
                generation: 9,
                blocker: "association-terminal-failure",
            },
        );
        assert_eq!(
            lifecycle.consume_failure(1, 100, &mut retry_schedule),
            super::DeferredGate8RecoveryBudget::Backoff {
                delay_ms: 1_000,
                next_attempt_ms: 1_100,
            },
        );
        assert_eq!(retry_schedule.attempt_number(), 2);
        assert_eq!(
            lifecycle.consume_failure(1, 101, &mut retry_schedule),
            super::DeferredGate8RecoveryBudget::AlreadyRecorded,
        );
        assert_eq!(retry_schedule.attempt_number(), 2);
        assert_eq!(
            lifecycle.observe(2, 1_100, false, failed),
            super::DeferredGate8Observation::Pending,
            "the recovering state cannot consume another attempt before the sole supervisor completes",
        );

        assert_eq!(lifecycle.enter_stabilizing(2, 1_100), 91_100);
        assert_eq!(lifecycle.deadline_ms(), Some(91_100));
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_failures_exhaust_exactly_five_outer_attempts() {
        let mut lifecycle = super::DeferredGate8Lifecycle::new();
        let mut retry_schedule = super::DeferredNetRetrySchedule::new(0);
        let mut now_ms = 0;

        for attempt in 1..=super::CYW43_BOOTSTRAP_MAX_ATTEMPTS {
            lifecycle.enter_stabilizing(attempt, now_ms);
            let budget = lifecycle.consume_failure(attempt, now_ms, &mut retry_schedule);
            if attempt < super::CYW43_BOOTSTRAP_MAX_ATTEMPTS {
                let super::DeferredGate8RecoveryBudget::Backoff {
                    next_attempt_ms, ..
                } = budget
                else {
                    panic!("attempt {attempt} must schedule one bounded recovery");
                };
                now_ms = next_attempt_ms;
            } else {
                assert_eq!(budget, super::DeferredGate8RecoveryBudget::Exhausted);
            }
        }

        assert!(retry_schedule.exhausted());
        assert_eq!(retry_schedule.attempt_number(), 5);
        assert_eq!(
            lifecycle.consume_failure(5, now_ms, &mut retry_schedule),
            super::DeferredGate8RecoveryBudget::AlreadyRecorded,
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_ready_requires_fresh_publication_and_retracts_on_proof_loss() {
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
        let mut lifecycle = super::DeferredGate8Lifecycle::new();

        assert_eq!(
            lifecycle.observe(1, 1_000, false, stable),
            super::DeferredGate8Observation::Publish { generation: 12 },
        );
        assert_eq!(
            lifecycle.observe(1, 1_001, false, stable),
            super::DeferredGate8Observation::Publish { generation: 12 },
            "failed publication leaves readiness uncommitted and requires a fresh observation",
        );
        assert!(lifecycle.accept_ready(12));
        assert_eq!(
            lifecycle.observe(1, 1_002, true, stable),
            super::DeferredGate8Observation::Ready,
        );
        assert_eq!(
            lifecycle.observe(1, 1_003, false, pending_same_generation),
            super::DeferredGate8Observation::Retracted { generation: 12 },
            "loss of exact proof retracts Ready even without a generation change",
        );
        assert_eq!(
            lifecycle.deadline_ms(),
            Some(91_000),
            "pre-Gate10 retraction must retain the original absolute deadline",
        );
        assert_eq!(
            lifecycle.observe(1, 1_004, false, stable),
            super::DeferredGate8Observation::Publish { generation: 12 },
        );
        assert!(lifecycle.accept_ready(12));
        assert!(lifecycle.mark_gate10_complete(12));
        assert_eq!(
            lifecycle.observe(1, 2_000, false, pending_same_generation),
            super::DeferredGate8Observation::Retracted { generation: 12 },
        );
        assert_eq!(
            lifecycle.deadline_ms(),
            Some(92_000),
            "proof loss after Gate10 begins a fresh bounded recovery episode",
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn gate8_internal_recovery_from_ready_preserves_pre_gate10_deadline() {
        let stable = gate8_lifecycle_snapshot(
            8,
            12,
            crate::drivers::driver_task_net::Cyw43Gate8SubgateStatus::Pass,
            "none",
        );
        let mut lifecycle = super::DeferredGate8Lifecycle::new();

        assert_eq!(
            lifecycle.observe(1, 100, false, stable),
            super::DeferredGate8Observation::Publish { generation: 12 },
        );
        assert!(lifecycle.accept_ready(12));
        assert_eq!(
            lifecycle.enter_stabilizing(1, 50_000),
            90_100,
            "same-attempt recovery before Gate10 cannot renew the absolute deadline",
        );

        assert_eq!(
            lifecycle.observe(1, 50_001, false, stable),
            super::DeferredGate8Observation::Publish { generation: 12 },
        );
        assert!(lifecycle.accept_ready(12));
        assert!(lifecycle.mark_gate10_complete(12));
        assert_eq!(
            lifecycle.enter_stabilizing(1, 60_000),
            150_000,
            "a later recovery after Gate10 starts a new bounded episode",
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
        let mut lifecycle = super::DeferredGate8Lifecycle::new();
        assert_eq!(lifecycle.enter_stabilizing(3, 500), 90_500);

        assert_eq!(
            lifecycle.observe(3, 90_500, false, stable),
            super::DeferredGate8Observation::Deadline {
                generation: 12,
                deadline_ms: 90_500,
            },
        );
    }

    #[cfg(all(
        feature = "serial-console",
        feature = "kernel",
        feature = "net-console"
    ))]
    #[test]
    fn permanent_attached_recovery_failure_never_reenters_supervisor_driver_turns() {
        assert_eq!(super::permanent_failure_terminal_mode(false), None);
        let terminal = super::permanent_failure_terminal_mode(true);
        assert_eq!(
            terminal,
            Some(super::DeferredNetSupervisorTerminal::PermanentAttachedRecoveryFailure),
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
