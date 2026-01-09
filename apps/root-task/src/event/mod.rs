// Author: Lukas Bower
// Purpose: Event pump coordinating serial, timer, networking, and IPC work for the root task.

//! Cooperative event pump coordinating serial, timer, networking, and IPC work.
//!
//! The pump intentionally avoids dynamic allocation so it can operate in the
//! seL4 environment while remaining testable under `cargo test`. Each polling
//! cycle progresses the serial console, dispatches timer ticks, advances the
//! networking stack (when enabled), and finally services IPC queues.
//!
//! Tracing: enable the `timer-trace` feature to log periodic timer ticks for
//! debugging long-running workloads. The default `dev-virt` profile keeps timers
//! silent to prioritise network instrumentation.

#[cfg(feature = "kernel")]
pub mod dispatch;
#[cfg(feature = "kernel")]
pub mod handlers;
#[cfg(feature = "kernel")]
pub mod op;

#[cfg(feature = "kernel")]
pub use dispatch::{dispatch_message, DispatchOutcome};
#[cfg(feature = "kernel")]
pub use handlers::{call_handler, Handler, HandlerError, HandlerResult, HandlerTable};
#[cfg(feature = "kernel")]
pub use op::BootstrapOp;

use core::cmp::min;
use core::fmt::{self, Write as FmtWrite};

use cohesix_proto::{role_label as proto_role_label, Role as ProtoRole};
use cohesix_ticket::Role;
use heapless::{String as HeaplessString, Vec as HeaplessVec};

use crate::console::proto::{render_ack, AckLine, AckStatus, LineFormatError};
use crate::console::{Command, CommandParser, ConsoleError, MAX_ROLE_LEN, MAX_TICKET_LEN};
#[cfg(feature = "kernel")]
use crate::bootstrap::log as boot_log;
#[cfg(feature = "kernel")]
use crate::debug_uart::debug_uart_str;
#[cfg(feature = "net-console")]
use crate::net::{
    NetConsoleEvent, NetDiagSnapshot, NetPoller, NetTelemetry, CONSOLE_QUEUE_DEPTH, NET_DIAG,
    NET_DIAG_FEATURED,
};
#[cfg(feature = "net-console")]
use crate::trace::{RateLimitKey, RateLimiter};
#[cfg(feature = "kernel")]
use crate::log_buffer;
#[cfg(feature = "kernel")]
use crate::ninedoor::{NineDoorBridge, NineDoorBridgeError};
#[cfg(feature = "kernel")]
use crate::sel4;
#[cfg(feature = "kernel")]
use crate::sel4::{BootInfoExt, BootInfoView};
use crate::serial::{SerialDriver, SerialPort, SerialTelemetry, DEFAULT_LINE_CAPACITY};
#[cfg(feature = "kernel")]
use sel4_sys::seL4_CPtr;

#[cfg(not(feature = "kernel"))]
fn debug_uart_str(_message: &str) {}

fn format_message(args: fmt::Arguments<'_>) -> HeaplessString<128> {
    let mut buf = HeaplessString::new();
    if FmtWrite::write_fmt(&mut buf, args).is_err() {
        // Truncated diagnostic; best-effort only.
    }
    buf
}

/// Trait used by the event pump to emit audit records.
pub trait AuditSink {
    /// Informational message emitted during pump initialisation or state changes.
    fn info(&mut self, message: &str);

    /// Audit entry emitted when a privileged action is denied.
    fn denied(&mut self, message: &str);
}

/// Tick emitted by a [`TimerSource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickEvent {
    /// Sequential tick identifier.
    pub tick: u64,
    /// Monotonic time of the tick in milliseconds.
    pub now_ms: u64,
}

#[cfg(feature = "kernel")]
const MAX_BOOTSTRAP_WORDS: usize = crate::sel4::MSG_MAX_WORDS;

#[cfg(feature = "kernel")]
const BOOTSTRAP_IDLE_SPINS: usize = 512;

const CONSOLE_BANNER: &str = "[Cohesix] Root console ready (type 'help' for commands)";
const CONSOLE_PROMPT: &str = "cohesix> ";
#[cfg(feature = "net-console")]
const NET_DIAG_HEARTBEAT_MS: u64 = 5_000;
#[cfg(feature = "net-console")]
const NET_DIAG_RATE_LIMIT_MS: u64 = 1_000;
#[cfg(feature = "net-console")]
const NET_DIAG_RATE_KINDS: usize = 1;
#[cfg(feature = "net-console")]
const NET_DIAG_HEARTBEAT_POLLS: u64 = 1_024;
#[cfg(feature = "net-console")]
const NET_DIAG_STUCK_MS: u64 = 3_000;

#[cfg_attr(not(any(test, feature = "kernel")), allow(dead_code))]
#[derive(Debug, Default)]
struct BootstrapBackoff {
    idle_spins: usize,
    limit: usize,
}

#[cfg_attr(not(any(test, feature = "kernel")), allow(dead_code))]
impl BootstrapBackoff {
    fn new(limit: usize) -> Self {
        Self {
            idle_spins: 0,
            limit,
        }
    }

    fn observe(&mut self, has_staged: bool) -> Option<usize> {
        if has_staged {
            self.idle_spins = 0;
            return None;
        }
        self.idle_spins = self.idle_spins.saturating_add(1);
        if self.idle_spins >= self.limit {
            Some(self.idle_spins)
        } else {
            None
        }
    }
}

#[cfg(feature = "kernel")]
#[derive(Clone)]
/// IPC message staged during bootstrap and replayed once the dispatcher is ready.
pub struct BootstrapMessage {
    /// Badge attached to the message capability.
    pub badge: sel4_sys::seL4_Word,
    /// Raw message info describing the word and capability counts.
    pub info: sel4_sys::seL4_MessageInfo,
    /// Payload words staged from the IPC buffer.
    pub payload: HeaplessVec<sel4_sys::seL4_Word, { MAX_BOOTSTRAP_WORDS }>,
}

#[cfg(feature = "kernel")]
impl fmt::Debug for BootstrapMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BootstrapMessage")
            .field("badge", &self.badge)
            .field("info_raw", &self.info.words)
            .field("payload", &self.payload)
            .finish()
    }
}

#[cfg(feature = "kernel")]
impl PartialEq for BootstrapMessage {
    fn eq(&self, other: &Self) -> bool {
        self.badge == other.badge
            && self.info.words == other.info.words
            && self.payload == other.payload
    }
}

#[cfg(feature = "kernel")]
impl Eq for BootstrapMessage {}

#[cfg(feature = "kernel")]
impl BootstrapMessage {
    /// Returns `true` when the staged payload contained no words.
    pub fn payload_is_empty(&self) -> bool {
        self.payload.is_empty()
    }
}

/// Timer abstraction used by the event pump.
pub trait TimerSource {
    /// Poll the timer for the next tick, if any.
    fn poll(&mut self, now_ms: u64) -> Option<TickEvent>;
}

/// IPC dispatcher invoked once per pump cycle.
pub trait IpcDispatcher {
    /// Service pending IPC messages.
    fn dispatch(&mut self, now_ms: u64);

    /// Called once the event pump has registered bootstrap handlers.
    fn handlers_ready(&mut self) {}

    #[cfg(feature = "kernel")]
    /// Retrieve the next staged bootstrap message, if any.
    fn take_bootstrap_message(&mut self) -> Option<BootstrapMessage> {
        None
    }

    #[cfg(feature = "kernel")]
    /// Poll the bootstrap endpoint, returning `true` when a message was staged.
    fn bootstrap_poll(&mut self, now_ms: u64) -> bool {
        let _ = now_ms;
        false
    }

    #[cfg(feature = "kernel")]
    /// Return `true` when a bootstrap message is currently staged.
    fn has_staged_bootstrap(&self) -> bool {
        false
    }
}

#[cfg(feature = "kernel")]
/// Handler invoked when the pump observes a staged bootstrap IPC message.
pub trait BootstrapMessageHandler {
    /// Process the staged message once it has been drained from the dispatcher.
    fn handle(&mut self, message: &BootstrapMessage, audit: &mut dyn AuditSink);
}

/// Capability validator consulted when privileged verbs execute.
pub trait CapabilityValidator {
    /// Validate that `ticket` grants the requested `role`.
    fn validate(&self, role: Role, ticket: Option<&str>) -> bool;
}

/// Error raised when registering capability tickets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketRegistryError {
    /// The ticket table reached its capacity.
    Capacity,
    /// Provided secret exceeded the allowed size.
    SecretTooLong,
}

#[derive(Debug)]
struct TicketRecord {
    role: Role,
    key: cohesix_ticket::TicketKey,
}

/// Deterministic capability table used by the authenticated console.
#[derive(Debug)]
pub struct TicketTable<const N: usize> {
    entries: HeaplessVec<TicketRecord, N>,
}

impl<const N: usize> TicketTable<N> {
    /// Create an empty ticket table.
    pub const fn new() -> Self {
        Self {
            entries: HeaplessVec::new(),
        }
    }

    /// Register a new ticket secret.
    pub fn register(&mut self, role: Role, secret: &str) -> Result<(), TicketRegistryError> {
        if secret.len() > MAX_TICKET_LEN {
            return Err(TicketRegistryError::SecretTooLong);
        }
        if self.entries.is_full() {
            return Err(TicketRegistryError::Capacity);
        }
        self.entries
            .push(TicketRecord {
                role,
                key: cohesix_ticket::TicketKey::from_secret(secret),
            })
            .map_err(|_| TicketRegistryError::Capacity)
    }
}

impl<const N: usize> Default for TicketTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> CapabilityValidator for TicketTable<N> {
    fn validate(&self, role: Role, ticket: Option<&str>) -> bool {
        let ticket = ticket.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        if role == Role::Queen && ticket.is_none() {
            return true;
        }
        let Some(ticket) = ticket else { return false };
        let key = self.entries.iter().find_map(|record| {
            (record.role == role).then_some(&record.key)
        });
        let Some(key) = key else { return false };
        let Ok(decoded) = cohesix_ticket::TicketToken::decode(ticket, key) else {
            return false;
        };
        decoded.claims().role == role
    }
}

/// Snapshot of event pump metrics used for diagnostics.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PumpMetrics {
    /// Number of console lines processed across serial and TCP transports.
    pub console_lines: u64,
    /// Commands rejected due to missing authentication.
    pub denied_commands: u64,
    /// Commands executed successfully.
    pub accepted_commands: u64,
    /// Timer ticks processed.
    pub timer_ticks: u64,
    #[cfg(feature = "kernel")]
    /// Bootstrap IPC messages processed.
    pub bootstrap_messages: u64,
}

/// Authenticated session state maintained by the pump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionRole {
    Queen,
    Worker,
}

impl SessionRole {
    fn from_role(role: Role) -> Option<Self> {
        match role {
            Role::Queen => Some(Self::Queen),
            Role::WorkerHeartbeat | Role::WorkerGpu => Some(Self::Worker),
        }
    }
}

/// Exponential back-off helper used when authentication repeatedly fails.
#[derive(Debug, Default, Clone, Copy)]
struct AuthThrottle {
    failures: u32,
    blocked_until_ms: u64,
}

impl AuthThrottle {
    const BASE_BACKOFF_MS: u64 = 250;
    const MAX_SHIFT: u32 = 8;

    fn register_failure(&mut self, now_ms: u64) {
        let shift = min(self.failures, Self::MAX_SHIFT);
        let delay = Self::BASE_BACKOFF_MS.saturating_mul(1u64 << shift);
        self.failures = self.failures.saturating_add(1);
        self.blocked_until_ms = now_ms.saturating_add(delay);
    }

    fn register_success(&mut self) {
        self.failures = 0;
        self.blocked_until_ms = 0;
    }

    fn check(&self, now_ms: u64) -> Result<(), u64> {
        if now_ms < self.blocked_until_ms {
            Err(self.blocked_until_ms.saturating_sub(now_ms))
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "net-console")]
#[derive(Clone, Copy)]
struct NetDiagLogSnapshot {
    snapshot: NetDiagSnapshot,
    link_up: bool,
    tx_drops: u32,
}

#[cfg(feature = "net-console")]
#[repr(u8)]
#[derive(Clone, Copy, Debug)]
enum NetDiagRateKind {
    Summary = 0,
}

#[cfg(feature = "net-console")]
impl RateLimitKey for NetDiagRateKind {
    const COUNT: usize = NET_DIAG_RATE_KINDS;

    fn index(self) -> usize {
        self as usize
    }
}

/// Networking integration exposed to the pump when the `net` feature is enabled.
/// Event pump orchestrating serial, timer, IPC, and optional networking work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConsoleInputSource {
    Serial,
    Net,
}

pub struct EventPump<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>
where
    D: SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    serial: SerialPort<D, RX, TX, LINE>,
    parser: CommandParser,
    timer: T,
    ipc: I,
    validator: V,
    audit: &'a mut dyn AuditSink,
    metrics: PumpMetrics,
    now_ms: u64,
    session: Option<SessionRole>,
    last_input_source: ConsoleInputSource,
    stream_end_pending: bool,
    throttle: AuthThrottle,
    #[cfg(feature = "net-console")]
    net: Option<&'a mut dyn NetPoller>,
    #[cfg(feature = "net-console")]
    last_net_diag_log_ms: Option<u64>,
    #[cfg(feature = "net-console")]
    last_net_diag_emitted: Option<NetDiagLogSnapshot>,
    #[cfg(feature = "net-console")]
    last_net_diag_snapshot: Option<NetDiagSnapshot>,
    #[cfg(feature = "net-console")]
    net_diag_limiter: RateLimiter<NET_DIAG_RATE_KINDS>,
    #[cfg(feature = "net-console")]
    net_diag_stuck_logged: bool,
    #[cfg(feature = "kernel")]
    ninedoor: Option<&'a mut NineDoorBridge>,
    #[cfg(feature = "kernel")]
    bootstrap_handler: Option<&'a mut dyn BootstrapMessageHandler>,
    #[cfg(feature = "kernel")]
    console_context: Option<ConsoleContext>,
    banner_emitted: bool,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy)]
struct ConsoleContext {
    bootinfo: BootInfoView,
    ep_slot: seL4_CPtr,
    uart_slot: Option<seL4_CPtr>,
}

impl<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>
    EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    /// Create a new event pump backed by the supplied subsystems.
    pub fn new(
        serial: SerialPort<D, RX, TX, LINE>,
        timer: T,
        ipc: I,
        validator: V,
        audit: &'a mut dyn AuditSink,
    ) -> Self {
        audit.info("event-pump: init serial");
        audit.info("event-pump: init timer");
        audit.info("event-pump: init ipc");
        Self {
            serial,
            parser: CommandParser::new(),
            timer,
            ipc,
            validator,
            audit,
            metrics: PumpMetrics::default(),
            now_ms: 0,
            session: None,
            last_input_source: ConsoleInputSource::Serial,
            stream_end_pending: false,
            throttle: AuthThrottle::default(),
            #[cfg(feature = "net-console")]
            net: None,
            #[cfg(feature = "net-console")]
            last_net_diag_log_ms: None,
            #[cfg(feature = "net-console")]
            last_net_diag_emitted: None,
            #[cfg(feature = "net-console")]
            last_net_diag_snapshot: None,
            #[cfg(feature = "net-console")]
            net_diag_limiter: RateLimiter::<NET_DIAG_RATE_KINDS>::new(NET_DIAG_RATE_LIMIT_MS),
            #[cfg(feature = "net-console")]
            net_diag_stuck_logged: false,
            #[cfg(feature = "kernel")]
            ninedoor: None,
            #[cfg(feature = "kernel")]
            bootstrap_handler: None,
            #[cfg(feature = "kernel")]
            console_context: None,
            banner_emitted: false,
        }
    }

    /// Attach a networking poller to the event pump.
    #[cfg(feature = "net-console")]
    pub fn with_network(mut self, net: &'a mut dyn NetPoller) -> Self {
        self.audit.info("event-pump: init network");
        self.net = Some(net);
        self
    }

    /// Attach a NineDoor handler to the event pump.
    #[cfg(feature = "kernel")]
    pub fn with_ninedoor(mut self, bridge: &'a mut NineDoorBridge) -> Self {
        self.ninedoor = Some(bridge);
        self
    }

    #[cfg(feature = "kernel")]
    /// Attach boot-time console metadata for diagnostic commands.
    pub fn with_console_context(
        mut self,
        bootinfo: BootInfoView,
        ep_slot: seL4_CPtr,
        uart_slot: Option<seL4_CPtr>,
    ) -> Self {
        self.console_context = Some(ConsoleContext {
            bootinfo,
            ep_slot,
            uart_slot,
        });
        self
    }

    #[cfg(feature = "kernel")]
    /// Attach a bootstrap IPC handler that consumes staged messages.
    pub fn with_bootstrap_handler(mut self, handler: &'a mut dyn BootstrapMessageHandler) -> Self {
        self.bootstrap_handler = Some(handler);
        self.ipc.handlers_ready();
        self
    }

    /// Execute a single cooperative polling cycle.
    pub fn poll(&mut self) {
        self.serial.poll_io();
        self.consume_serial();

        #[cfg(feature = "kernel")]
        let timebase_now_ms = crate::hal::timebase().now_ms();
        #[cfg(not(feature = "kernel"))]
        let timebase_now_ms = self.now_ms;

        if let Some(tick) = self.timer.poll(timebase_now_ms) {
            self.now_ms = tick.now_ms;
            self.metrics.timer_ticks = self.metrics.timer_ticks.saturating_add(1);
            crate::hal::set_timebase_now_ms(self.now_ms);
            #[cfg(feature = "timer-trace")]
            if tick.tick % 8_000 == 0 {
                let message = format_message(format_args!(
                    "timer: tick {} (now_ms={})",
                    tick.tick, self.now_ms
                ));
                self.audit.info(message.as_str());
            }
        } else {
            self.now_ms = timebase_now_ms;
        }

        #[cfg(feature = "net-console")]
        let net_poll = if let Some(net) = self.net.as_mut() {
            let activity = net.poll(self.now_ms);
            let telemetry = net.telemetry();
            let mut buffered: HeaplessVec<
                HeaplessString<DEFAULT_LINE_CAPACITY>,
                { CONSOLE_QUEUE_DEPTH },
            > = HeaplessVec::new();
            net.drain_console_lines(&mut |line| {
                let _ = buffered.push(line);
            });
            Some((activity, telemetry, buffered))
        } else {
            None
        };

        #[cfg(feature = "net-console")]
        if let Some((activity, telemetry, buffered)) = net_poll {
            if NET_DIAG_FEATURED {
                self.log_net_diag(telemetry);
            } else if activity {
                let message = format_message(format_args!(
                    "net: poll link_up={} tx_drops={}",
                    telemetry.link_up, telemetry.tx_drops
                ));
                self.audit.info(message.as_str());
            }
            for line in buffered {
                self.handle_network_line(line);
            }
            self.drain_net_console_events();
        }

        self.ipc.dispatch(self.now_ms);
        #[cfg(feature = "kernel")]
        self.drain_bootstrap_ipc();
    }

    #[cfg(feature = "net-console")]
    // Activity-only logging to prevent endless spam in steady state.
    fn should_log_net_diag(&self, snapshot: NetDiagSnapshot, telemetry: NetTelemetry) -> bool {
        let activity = self.last_net_diag_emitted.map_or(true, |prev| {
            Self::net_diag_changed(prev.snapshot, snapshot)
                || prev.link_up != telemetry.link_up
                || prev.tx_drops != telemetry.tx_drops
        });
        let heartbeat_poll = self.last_net_diag_emitted.map_or(false, |prev| {
            snapshot.poll_calls.saturating_sub(prev.snapshot.poll_calls) >= NET_DIAG_HEARTBEAT_POLLS
        });
        let heartbeat_time = self.last_net_diag_log_ms.map_or(false, |last| {
            self.now_ms.saturating_sub(last) >= NET_DIAG_HEARTBEAT_MS
        });

        activity || heartbeat_poll || heartbeat_time
    }

    #[cfg(feature = "net-console")]
    fn net_diag_changed(prev: NetDiagSnapshot, curr: NetDiagSnapshot) -> bool {
        let mut prev = prev;
        let mut curr = curr;
        prev.poll_calls = 0;
        curr.poll_calls = 0;
        prev != curr
    }

    #[cfg(feature = "net-console")]
    fn log_net_diag(&mut self, telemetry: NetTelemetry) {
        if !NET_DIAG_FEATURED {
            return;
        }
        let snapshot = NET_DIAG.snapshot();
        if self.should_log_net_diag(snapshot, telemetry) {
            if let Some(suppressed) =
                self.net_diag_limiter
                    .check(NetDiagRateKind::Summary, self.now_ms)
            {
                let line = format_message(format_args!(
                    "NETDIAG in_bytes={} out_bytes={} tx_drops={} link={} q_lines={} q_bytes={} q_drops={} q_wblk={} suppressed={}",
                    snapshot.bytes_read,
                    snapshot.bytes_written,
                    telemetry.tx_drops,
                    telemetry.link_up,
                    snapshot.outbound_queued_lines,
                    snapshot.outbound_queued_bytes,
                    snapshot.outbound_drops,
                    snapshot.outbound_would_block,
                    suppressed,
                ));
                self.audit.info(line.as_str());
                self.last_net_diag_log_ms = Some(self.now_ms);
                self.last_net_diag_emitted = Some(NetDiagLogSnapshot {
                    snapshot,
                    link_up: telemetry.link_up,
                    tx_drops: telemetry.tx_drops,
                });
            }
        }
        self.check_net_diag_progress(snapshot);
        self.last_net_diag_snapshot = Some(snapshot);
    }

    #[cfg(feature = "net-console")]
    fn check_net_diag_progress(&mut self, snapshot: NetDiagSnapshot) {
        if let Some(prev) = self.last_net_diag_snapshot {
            if snapshot.rx_used_seen != prev.rx_used_seen {
                self.net_diag_stuck_logged = false;
            }
            let poll_delta = snapshot.poll_calls.saturating_sub(prev.poll_calls);
            let irq_delta = snapshot.rx_irq_count.saturating_sub(prev.rx_irq_count);
            let last_progress_ms = NET_DIAG.last_rx_used_change_ms();
            if poll_delta > 0
                && irq_delta > 0
                && last_progress_ms > 0
                && self.now_ms.saturating_sub(last_progress_ms) >= NET_DIAG_STUCK_MS
                && !self.net_diag_stuck_logged
            {
                let warn_line = format_message(format_args!(
                    "NETDIAG warn: rx_used_stuck ms={} poll_delta={} irq_delta={} rx_used={}",
                    self.now_ms.saturating_sub(last_progress_ms),
                    poll_delta,
                    irq_delta,
                    snapshot.rx_used_seen
                ));
                self.audit.info(warn_line.as_str());
                self.net_diag_stuck_logged = true;
                NET_DIAG.mark_stuck_warned();
            }
        }
    }

    #[cfg(feature = "kernel")]
    /// Run the bootstrap probe loop until an IPC message has been staged.
    pub fn bootstrap_probe(&mut self) {
        log::trace!("B5: entering bootstrap probe loop");
        let mut backoff = BootstrapBackoff::new(BOOTSTRAP_IDLE_SPINS);
        loop {
            let handled_before = self.metrics.bootstrap_messages;
            if self.ipc.bootstrap_poll(self.now_ms) {
                self.drain_bootstrap_ipc();
            }
            self.poll();
            if self.metrics.bootstrap_messages != handled_before {
                break;
            }
            if let Some(spins) = backoff.observe(self.ipc.has_staged_bootstrap()) {
                let summary = format_message(format_args!(
                    "bootstrap-ipc: idle after {spins} polls; continuing"
                ));
                self.audit.info(summary.as_str());
                break;
            }
            crate::sel4::yield_now();
        }
    }

    #[cfg(feature = "kernel")]
    /// Emit console audit messages once the UART bridge is connected.
    pub fn announce_console_ready(&mut self) {
        if self.ninedoor.is_some() {
            boot_log::switch_logger_to_log_buffer();
        }
        self.audit.info("console: attach uart");
        if let Some(bridge) = self.ninedoor.as_mut() {
            match bridge.log_stream(&mut *self.audit) {
                Ok(()) => {
                    self.audit.info("console: log stream start");
                }
                Err(err) => {
                    let summary =
                        format_message(format_args!("console: log stream failed: {}", err));
                    self.audit.info(summary.as_str());
                }
            }
        } else {
            self.audit
                .info("console: log stream deferred (bridge unavailable)");
        }
    }

    #[cfg(feature = "kernel")]
    fn drain_bootstrap_ipc(&mut self) {
        while let Some(message) = self.ipc.take_bootstrap_message() {
            self.metrics.bootstrap_messages = self.metrics.bootstrap_messages.saturating_add(1);
            if let Some(handler) = self.bootstrap_handler.as_mut() {
                handler.handle(&message, &mut *self.audit);
            } else {
                let summary = format_message(format_args!(
                    "bootstrap-ipc: badge=0x{badge:016x} label=0x{label:08x} words={words}",
                    badge = message.badge,
                    label = message.info.words[0],
                    words = message.payload.len(),
                ));
                self.audit.info(summary.as_str());
            }
        }
    }

    /// Emit the interactive banner and initial prompt over the serial console.
    pub fn start_cli(&mut self) {
        debug_uart_str("[dbg] console: root console task entry\n");
        #[cfg(feature = "kernel")]
        if let Some(context) = self.console_context {
            log::info!(
                target: "root_task::console",
                "[console] starting root shell ep=0x{ep:04x} uart=0x{uart:04x}",
                ep = context.ep_slot,
                uart = context.uart_slot.unwrap_or(crate::sel4::seL4_CapNull),
            );
        }
        self.emit_serial_line(CONSOLE_BANNER);
        self.emit_serial_line("Cohesix console ready");
        self.emit_help_serial_only();
        #[cfg(feature = "net-console")]
        if let Some(net) = self.net.as_mut() {
            net.send_console_line(
                "[net-console] authenticate using AUTH <role> <token> to receive console output",
            );
        }
        debug_uart_str("[dbg] console: writing 'cohesix>' prompt\n");
        self.emit_prompt();
        self.serial.poll_io();
        if !self.banner_emitted {
            log::info!(target: "event", "[event] root console banner emitted");
            self.banner_emitted = true;
        }
    }

    /// Run the cooperative pump until shutdown.
    pub fn run(mut self) -> ! {
        log::info!(
            target: "event",
            "[event] pump starting: root_console={}, net_console_enabled={}, ninedoor_enabled={}",
            self.has_root_console(),
            self.net_console_enabled(),
            self.ninedoor_enabled(),
        );

        loop {
            self.poll();
            #[cfg(feature = "kernel")]
            sel4::yield_now();
            #[cfg(not(feature = "kernel"))]
            core::hint::spin_loop();
        }
    }

    /// Returns whether the root console is attached.
    pub fn has_root_console(&self) -> bool {
        true
    }

    /// Returns whether net-console handling is enabled.
    pub fn net_console_enabled(&self) -> bool {
        #[cfg(feature = "net-console")]
        {
            return self.net.is_some();
        }
        #[cfg(not(feature = "net-console"))]
        {
            false
        }
    }

    /// Returns whether the NineDoor bridge is enabled.
    pub fn ninedoor_enabled(&self) -> bool {
        #[cfg(feature = "kernel")]
        {
            return self.ninedoor.is_some();
        }
        #[cfg(not(feature = "kernel"))]
        {
            false
        }
    }

    /// Retrieve a snapshot of the current pump metrics.
    #[must_use]
    pub fn metrics(&self) -> PumpMetrics {
        self.metrics
    }

    /// Obtain the most recent serial telemetry.
    #[must_use]
    pub fn serial_telemetry(&self) -> SerialTelemetry {
        self.serial.telemetry()
    }

    /// Emit a console line to the serial console and any attached TCP clients.
    pub fn emit_console_line(&mut self, line: &str) {
        self.emit_serial_line(line);
        #[cfg(feature = "net-console")]
        if let Some(net) = self.net.as_mut() {
            net.send_console_line(line);
        }
    }

    fn emit_serial_line(&mut self, line: &str) {
        self.serial.enqueue_tx(line.as_bytes());
        self.serial.enqueue_tx(b"\r\n");
    }

    fn emit_prompt(&mut self) {
        self.serial.enqueue_tx(CONSOLE_PROMPT.as_bytes());
    }

    fn emit_help(&mut self) {
        self.emit_console_line("Commands:");
        self.emit_console_line("  help  - Show this help");
        self.emit_console_line("  bi    - Show bootinfo summary");
        self.emit_console_line("  caps  - Show capability slots");
        self.emit_console_line("  mem   - Show untyped summary");
        self.emit_console_line("  ping  - Respond with pong");
        self.emit_console_line("  nettest  - Run network self-test (dev-virt)");
        self.emit_console_line("  netstats - Show network counters");
        self.emit_console_line("  quit  - Exit the console session");
    }

    fn emit_help_serial_only(&mut self) {
        self.emit_serial_line("Commands:");
        self.emit_serial_line("  help  - Show this help");
        self.emit_serial_line("  bi    - Show bootinfo summary");
        self.emit_serial_line("  caps  - Show capability slots");
        self.emit_serial_line("  mem   - Show untyped summary");
        self.emit_serial_line("  ping  - Respond with pong");
        self.emit_serial_line("  nettest  - Run network self-test (dev-virt)");
        self.emit_serial_line("  netstats - Show network counters");
        self.emit_serial_line("  quit  - Exit the console session");
    }

    #[cfg(feature = "kernel")]
    fn emit_log_snapshot(&mut self) {
        let lines =
            log_buffer::snapshot_lines::<DEFAULT_LINE_CAPACITY, { log_buffer::LOG_SNAPSHOT_LINES }>();
        for line in lines {
            self.emit_console_line(line.as_str());
        }
    }

    #[cfg(feature = "kernel")]
    fn emit_bootinfo(&mut self) -> bool {
        let context = match self.console_context {
            Some(context) => context,
            None => return false,
        };
        let header = context.bootinfo.header();
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let _ = write!(
            line,
            "[bi] node_bits={} empty=[0x{:04x}..0x{:04x}) ",
            header.initThreadCNodeSizeBits, header.empty.start, header.empty.end,
        );
        if let Some(ptr) = header.ipc_buffer_ptr() {
            let addr = ptr.as_ptr() as usize;
            let width = core::mem::size_of::<usize>() * 2;
            let _ = write!(line, "ipc=0x{addr:0width$x}");
        } else {
            let _ = line.push_str("ipc=<none>");
        }
        self.emit_console_line(line.as_str());
        true
    }

    #[cfg(not(feature = "kernel"))]
    fn emit_bootinfo(&mut self) -> bool {
        let _ = self;
        false
    }

    #[cfg(feature = "kernel")]
    fn emit_caps(&mut self) -> bool {
        let context = match self.console_context {
            Some(context) => context,
            None => return false,
        };
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let _ = write!(
            line,
            "[caps] root=0x{:04x} ep=0x{:04x} uart=0x{:04x}",
            context.bootinfo.root_cnode_cap(),
            context.ep_slot,
            context.uart_slot.unwrap_or(sel4_sys::seL4_CapNull),
        );
        self.emit_console_line(line.as_str());
        true
    }

    #[cfg(not(feature = "kernel"))]
    fn emit_caps(&mut self) -> bool {
        let _ = self;
        false
    }

    #[cfg(feature = "kernel")]
    fn emit_mem(&mut self) -> bool {
        let context = match self.console_context {
            Some(context) => context,
            None => return false,
        };
        let header = context.bootinfo.header();
        let count = (header.untyped.end - header.untyped.start) as usize;
        let mut ram_ut = 0usize;
        for desc in header.untypedList.iter().take(count) {
            if desc.isDevice == 0 {
                ram_ut += 1;
            }
        }
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let _ = write!(
            line,
            "[mem] untyped caps={} ram_ut={} device_ut={}",
            count,
            ram_ut,
            count.saturating_sub(ram_ut),
        );
        self.emit_console_line(line.as_str());
        true
    }

    #[cfg(not(feature = "kernel"))]
    fn emit_mem(&mut self) -> bool {
        let _ = self;
        false
    }

    #[cfg(all(feature = "kernel", target_os = "none"))]
    fn emit_cache_log(&mut self, count: usize) {
        struct CacheLineWriter<
            'a,
            'b,
            D,
            T,
            I,
            V,
            const RX: usize,
            const TX: usize,
            const LINE: usize,
        >
        where
            D: SerialDriver,
            T: TimerSource,
            I: IpcDispatcher,
            V: CapabilityValidator,
        {
            pump: &'a mut EventPump<'b, D, T, I, V, RX, TX, LINE>,
        }

        impl<'a, 'b, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize> fmt::Write
            for CacheLineWriter<'a, 'b, D, T, I, V, RX, TX, LINE>
        where
            D: SerialDriver,
            T: TimerSource,
            I: IpcDispatcher,
            V: CapabilityValidator,
        {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                for line in s.split('\n') {
                    if line.is_empty() {
                        continue;
                    }
                    self.pump.emit_console_line(line);
                }
                Ok(())
            }
        }

        let mut writer = CacheLineWriter { pump: self };
        crate::hal::cache::write_recent_ops(&mut writer, count);
    }

    fn emit_ack(&mut self, status: AckStatus, verb: &str, detail: Option<&str>) {
        let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
        let ack_line = AckLine {
            status,
            verb,
            detail,
        };
        match render_ack(&mut line, &ack_line) {
            Ok(()) => self.emit_console_line(line.as_str()),
            Err(LineFormatError::Truncated) => {
                self.audit.denied("console ack truncated");
                self.emit_console_line("ERR PARSE reason=ack-truncated");
            }
        }
    }

    fn emit_ack_ok(&mut self, verb: &str, detail: Option<&str>) {
        self.emit_ack(AckStatus::Ok, verb, detail);
    }

    fn emit_ack_err(&mut self, verb: &str, detail: Option<&str>) {
        self.emit_ack(AckStatus::Err, verb, detail);
    }

    fn emit_auth_failure(&mut self, verb: &str) {
        self.emit_ack_err(verb, Some("reason=unauthenticated"));
    }

    fn handle_console_error(&mut self, err: ConsoleError) {
        let message = format_message(format_args!("console error: {}", err));
        self.audit.info(message.as_str());
        let detail = match err {
            ConsoleError::RateLimited(delay) => {
                format_message(format_args!("reason=rate-limited delay_ms={delay}"))
            }
            other => format_message(format_args!("reason={}", other)),
        };
        self.emit_ack_err("PARSE", Some(detail.as_str()));
    }

    fn consume_serial(&mut self) {
        while let Some(line) = self.serial.next_line() {
            self.last_input_source = ConsoleInputSource::Serial;
            self.process_console_line(&line);
        }
    }

    fn process_console_line(&mut self, line: &HeaplessString<LINE>) {
        self.metrics.console_lines = self.metrics.console_lines.saturating_add(1);
        if let Err(err) = self.feed_parser(line) {
            self.handle_console_error(err);
        }
        self.emit_prompt();
    }

    fn feed_parser(&mut self, line: &HeaplessString<LINE>) -> Result<(), ConsoleError> {
        for byte in line.as_bytes() {
            self.parser.push_byte(*byte)?;
        }
        if let Some(command) = self.parser.push_byte(b'\n')? {
            match self.handle_command(command) {
                Ok(()) => {}
                Err(err) => {
                    #[cfg(feature = "kernel")]
                    self.handle_dispatch_error(err);
                    #[cfg(not(feature = "kernel"))]
                    match err {}
                }
            }
        }
        Ok(())
    }

    #[cfg(feature = "net-console")]
    fn handle_network_line(&mut self, line: HeaplessString<DEFAULT_LINE_CAPACITY>) {
        let mut converted: HeaplessString<LINE> = HeaplessString::new();
        if converted.push_str(line.as_str()).is_err() {
            self.audit
                .denied("net console line exceeded maximum length");
            return;
        }
        self.last_input_source = ConsoleInputSource::Net;
        self.process_console_line(&converted);
    }

    #[cfg(feature = "net-console")]
    fn drain_net_console_events(&mut self) {
        if let Some(net) = self.net.as_mut() {
            net.drain_console_events(&mut |event| match event {
                NetConsoleEvent::Connected { conn_id, peer } => match peer {
                    Some(remote) => {
                        log::info!(
                            target: "net-console",
                            "[net-console] conn {}: established from {}",
                            conn_id,
                            remote
                        );
                    }
                    None => {
                        log::info!(
                            target: "net-console",
                            "[net-console] conn {}: established",
                            conn_id
                        );
                    }
                },
                NetConsoleEvent::Disconnected {
                    conn_id,
                    bytes_read,
                    bytes_written,
                } => {
                    log::info!(
                        target: "net-console",
                        "[net-console] conn {}: closed (bytes_read={}, bytes_written={})",
                        conn_id,
                        bytes_read,
                        bytes_written,
                    );
                }
            });
        }
    }

    #[inline(never)]
    pub(crate) fn handle_command(&mut self, command: Command) -> Result<(), CommandDispatchError> {
        #[cfg(feature = "kernel")]
        let command_clone = command.clone();
        #[cfg(feature = "kernel")]
        let mut forwarded = false;
        match command {
            Command::Help => {
                self.audit.info("console: help");
                self.metrics.accepted_commands += 1;
                self.emit_help();
                self.emit_ack_ok("HELP", None);
            }
            Command::BootInfo => {
                if self.emit_bootinfo() {
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok("BOOTINFO", None);
                } else {
                    self.metrics.denied_commands += 1;
                    self.emit_ack_err("BOOTINFO", Some("reason=unavailable"));
                }
            }
            Command::Caps => {
                if self.emit_caps() {
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok("CAPS", None);
                } else {
                    self.metrics.denied_commands += 1;
                    self.emit_ack_err("CAPS", Some("reason=unavailable"));
                }
            }
            Command::Mem => {
                if self.emit_mem() {
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok("MEM", None);
                } else {
                    self.metrics.denied_commands += 1;
                    self.emit_ack_err("MEM", Some("reason=unavailable"));
                }
            }
            Command::CacheLog { count } => {
                let count = usize::from(count.unwrap_or(64));
                #[cfg(all(feature = "kernel", target_os = "none"))]
                {
                    self.emit_cache_log(count);
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok("CACHELOG", None);
                }
                #[cfg(not(all(feature = "kernel", target_os = "none")))]
                {
                    let _ = count;
                    self.metrics.denied_commands += 1;
                    self.emit_ack_err("CACHELOG", Some("reason=unsupported"));
                }
            }
            Command::Ping => {
                if self.ensure_authenticated(SessionRole::Worker) {
                    self.audit.info("console: ping");
                    self.metrics.accepted_commands += 1;
                    self.emit_console_line("PONG");
                    self.emit_ack_ok("PING", Some("reply=pong"));
                } else {
                    self.emit_ack_err("PING", Some("reason=unauthenticated"));
                }
            }
            Command::NetTest => {
                #[cfg(feature = "net-console")]
                {
                    if let Some(net) = self.net.as_mut() {
                        if net.start_self_test(self.now_ms) {
                            self.metrics.accepted_commands += 1;
                            self.emit_console_line("[net-selftest] triggered");
                            self.emit_ack_ok("NETTEST", None);
                        } else {
                            self.metrics.denied_commands += 1;
                            self.emit_ack_err("NETTEST", Some("reason=unsupported"));
                        }
                    } else {
                        self.metrics.denied_commands += 1;
                        self.emit_ack_err("NETTEST", Some("reason=net-disabled"));
                    }
                }
                #[cfg(not(feature = "net-console"))]
                {
                    self.metrics.denied_commands += 1;
                    self.emit_ack_err("NETTEST", Some("reason=net-disabled"));
                }
            }
            Command::NetStats => {
                #[cfg(feature = "net-console")]
                {
                    if let Some(net) = self.net.as_mut() {
                        let stats = net.stats();
                        let report = net.self_test_report();
                        let line_one = format_message(format_args!(
                            "netstats: rx_pkts={} tx_pkts={} rx_used={} tx_used={} polls={}",
                            stats.rx_packets,
                            stats.tx_packets,
                            stats.rx_used_advances,
                            stats.tx_used_advances,
                            stats.smoltcp_polls
                        ));
                        let line_two = format_message(format_args!(
                            "netstats: udp_rx={} udp_tx={} tcp_accepts={} tcp_rx_bytes={} tcp_tx_bytes={}",
                            stats.udp_rx,
                            stats.udp_tx,
                            stats.tcp_accepts,
                            stats.tcp_rx_bytes,
                            stats.tcp_tx_bytes
                        ));
                        let line_three = format_message(format_args!(
                            "netstats: tcp_smoke_out={} tcp_smoke_out_failures={}",
                            stats.tcp_smoke_outbound, stats.tcp_smoke_outbound_failures
                        ));
                        let line_four = format_message(format_args!(
                            "netstats: tx_submit={} tx_complete={} tx_free={} tx_in_flight={} tx_double_submit={} tx_zero_len_attempt={}",
                            stats.tx_submit,
                            stats.tx_complete,
                            stats.tx_free,
                            stats.tx_in_flight,
                            stats.tx_double_submit,
                            stats.tx_zero_len_attempt
                        ));
                        let status_line = format_message(format_args!(
                            "nettest: enabled={} running={} last={:?}",
                            report.enabled, report.running, report.last_result
                        ));
                        self.emit_console_line(line_one.as_str());
                        self.emit_console_line(line_two.as_str());
                        self.emit_console_line(line_three.as_str());
                        self.emit_console_line(line_four.as_str());
                        self.emit_console_line(status_line.as_str());
                        self.metrics.accepted_commands += 1;
                        self.emit_ack_ok("NETSTATS", None);
                    } else {
                        self.metrics.denied_commands += 1;
                        self.emit_ack_err("NETSTATS", Some("reason=net-disabled"));
                    }
                }
                #[cfg(not(feature = "net-console"))]
                {
                    self.metrics.denied_commands += 1;
                    self.emit_ack_err("NETSTATS", Some("reason=net-disabled"));
                }
            }
            Command::Quit => {
                self.audit.info("console: quit");
                self.metrics.accepted_commands += 1;
                self.emit_ack_ok("QUIT", None);
                self.session = None;
            }
            Command::Attach { role, ticket } => {
                self.handle_attach(role, ticket);
                #[cfg(feature = "kernel")]
                {
                    forwarded = matches!(self.session, Some(_));
                }
            }
            Command::Tail { path } => {
                if self.ensure_authenticated(SessionRole::Worker) {
                    let message = format_message(format_args!("console: tail {}", path.as_str()));
                    self.audit.info(message.as_str());
                    self.metrics.accepted_commands += 1;
                    let detail = format_message(format_args!("path={}", path.as_str()));
                    self.emit_ack_ok("TAIL", Some(detail.as_str()));
                    self.stream_end_pending = true;
                    #[cfg(feature = "kernel")]
                    {
                        forwarded = true;
                    }
                } else {
                    self.emit_auth_failure("TAIL");
                }
            }
            Command::Cat { path } => {
                if self.ensure_authenticated(SessionRole::Worker) {
                    let message = format_message(format_args!("console: cat {}", path.as_str()));
                    self.audit.info(message.as_str());
                    self.metrics.accepted_commands += 1;
                    #[cfg(feature = "kernel")]
                    {
                        if let Some(bridge_ref) = self.ninedoor.as_mut() {
                            match bridge_ref.cat(path.as_str()) {
                                Ok(lines) => {
                                    let mut summary: HeaplessString<128> =
                                        HeaplessString::new();
                                    for (idx, line) in lines.iter().enumerate() {
                                        if idx > 0 {
                                            if summary.push('|').is_err() {
                                                self.emit_ack_err(
                                                    "CAT",
                                                    Some("reason=summary-too-long"),
                                                );
                                                return Ok(());
                                            }
                                        }
                                        if summary.push_str(line.as_str()).is_err() {
                                            self.emit_ack_err(
                                                "CAT",
                                                Some("reason=summary-too-long"),
                                            );
                                            return Ok(());
                                        }
                                    }
                                    let detail = format_message(format_args!(
                                        "path={} data={}",
                                        path.as_str(),
                                        summary.as_str()
                                    ));
                                    self.emit_ack_ok("CAT", Some(detail.as_str()));
                                    for line in lines {
                                        self.emit_console_line(line.as_str());
                                    }
                                    self.stream_end_pending = true;
                                }
                                Err(err) => {
                                    let detail = format_message(format_args!(
                                        "reason=ninedoor-error error={err}"
                                    ));
                                    self.emit_ack_err("CAT", Some(detail.as_str()));
                                }
                            }
                        } else {
                            self.emit_ack_err("CAT", Some("reason=ninedoor-unavailable"));
                        }
                    }
                    #[cfg(not(feature = "kernel"))]
                    {
                        self.emit_ack_err("CAT", Some("reason=ninedoor-unavailable"));
                    }
                } else {
                    self.emit_auth_failure("CAT");
                }
            }
            Command::Ls { path } => {
                if self.ensure_authenticated(SessionRole::Worker) {
                    let message =
                        format_message(format_args!("console: ls {} unsupported", path.as_str()));
                    self.audit.denied(message.as_str());
                    self.metrics.denied_commands += 1;
                    let detail = format_message(format_args!("reason=unsupported path={}", path));
                    self.emit_ack_err("LS", Some(detail.as_str()));
                } else {
                    self.emit_auth_failure("LS");
                }
            }
            Command::Log => {
                if self.ensure_authenticated(SessionRole::Queen) {
                    self.audit.info("console: log stream start");
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok("LOG", None);
                    self.stream_end_pending = true;
                    #[cfg(feature = "kernel")]
                    {
                        forwarded = true;
                    }
                } else {
                    self.emit_auth_failure("LOG");
                }
            }
            Command::Echo { path, payload } => {
                if self.ensure_authenticated(SessionRole::Queen) {
                    let message = format_message(format_args!(
                        "console: echo {} bytes={}",
                        path.as_str(),
                        payload.len()
                    ));
                    self.audit.info(message.as_str());
                    self.metrics.accepted_commands += 1;
                    #[cfg(feature = "kernel")]
                    {
                        if let Some(bridge_ref) = self.ninedoor.as_mut() {
                            match bridge_ref.echo(path.as_str(), payload.as_str()) {
                                Ok(()) => {
                                    let detail = format_message(format_args!(
                                        "path={} bytes={}",
                                        path.as_str(),
                                        payload.len()
                                    ));
                                    self.emit_ack_ok("ECHO", Some(detail.as_str()));
                                }
                                Err(err) => {
                                    let detail = format_message(format_args!(
                                        "reason=ninedoor-error error={err}"
                                    ));
                                    self.emit_ack_err("ECHO", Some(detail.as_str()));
                                }
                            }
                        } else {
                            self.emit_ack_err("ECHO", Some("reason=ninedoor-unavailable"));
                        }
                    }
                    #[cfg(not(feature = "kernel"))]
                    {
                        self.emit_ack_err("ECHO", Some("reason=ninedoor-unavailable"));
                    }
                } else {
                    self.emit_auth_failure("ECHO");
                }
            }
            Command::Spawn(payload) => {
                if self.ensure_authenticated(SessionRole::Queen) {
                    let message =
                        format_message(format_args!("console: spawn {}", payload.as_str()));
                    self.audit.info(message.as_str());
                    self.metrics.accepted_commands += 1;
                    let detail = format_message(format_args!("payload={}", payload.as_str()));
                    self.emit_ack_ok("SPAWN", Some(detail.as_str()));
                    #[cfg(feature = "kernel")]
                    {
                        forwarded = true;
                    }
                } else {
                    self.emit_auth_failure("SPAWN");
                }
            }
            Command::Kill(ident) => {
                if self.ensure_authenticated(SessionRole::Queen) {
                    let message = format_message(format_args!("console: kill {}", ident.as_str()));
                    self.audit.info(message.as_str());
                    self.metrics.accepted_commands += 1;
                    let detail = format_message(format_args!("id={}", ident.as_str()));
                    self.emit_ack_ok("KILL", Some(detail.as_str()));
                    #[cfg(feature = "kernel")]
                    {
                        forwarded = true;
                    }
                } else {
                    self.emit_auth_failure("KILL");
                }
            }
        }

        #[cfg(feature = "kernel")]
        if forwarded {
            if let Err(err) = self.forward_to_ninedoor(&command_clone) {
                self.stream_end_pending = false;
                return Err(err);
            }
        }

        #[cfg(feature = "kernel")]
        if self.stream_end_pending {
            match &command_clone {
                Command::Log => self.emit_log_snapshot(),
                Command::Tail { path } if path.as_str() == "/log/queen.log" => {
                    self.emit_log_snapshot();
                }
                _ => {}
            }
        }

        self.emit_stream_end_if_pending();

        Ok(())
    }

    #[cfg(feature = "kernel")]
    #[inline(never)]
    fn forward_to_ninedoor(&mut self, command: &Command) -> Result<(), CommandDispatchError> {
        #[cfg(debug_assertions)]
        {
            vtable_sentinel();
        }

        let verb = CommandVerb::from(command);

        let Some(bridge_ref) = self.ninedoor.as_mut() else {
            #[cfg(debug_assertions)]
            {
                log::warn!("attempted to forward {verb:?} without an attached NineDoor bridge");
            }
            return Err(CommandDispatchError::NineDoorUnavailable { verb });
        };

        let bridge = &mut **bridge_ref;

        match command {
            Command::Attach { role, ticket } => {
                let ticket_str = ticket.as_ref().map(|value| value.as_str());
                let audit = &mut *self.audit;
                bridge
                    .attach(role.as_str(), ticket_str, audit)
                    .map_err(|source| CommandDispatchError::Bridge { verb, source })?;
            }
            Command::Tail { path } => {
                let audit = &mut *self.audit;
                bridge
                    .tail(path.as_str(), audit)
                    .map_err(|source| CommandDispatchError::Bridge { verb, source })?;
            }
            Command::Log => {
                let audit = &mut *self.audit;
                bridge
                    .log_stream(audit)
                    .map_err(|source| CommandDispatchError::Bridge { verb, source })?;
            }
            Command::Spawn(payload) => {
                let audit = &mut *self.audit;
                bridge
                    .spawn(payload.as_str(), audit)
                    .map_err(|source| CommandDispatchError::Bridge { verb, source })?;
            }
            Command::Kill(identifier) => {
                let audit = &mut *self.audit;
                bridge
                    .kill(identifier.as_str(), audit)
                    .map_err(|source| CommandDispatchError::Bridge { verb, source })?;
            }
            Command::Help
            | Command::Quit
            | Command::BootInfo
            | Command::Caps
            | Command::Mem
            | Command::CacheLog { .. }
            | Command::Ping
            | Command::NetTest
            | Command::NetStats
            | Command::Cat { .. }
            | Command::Echo { .. }
            | Command::Ls { .. } => {
                return Err(CommandDispatchError::UnsupportedForNineDoor { verb });
            }
        }

        Ok(())
    }

    #[cfg(feature = "kernel")]
    fn handle_dispatch_error(&mut self, err: CommandDispatchError) {
        match err {
            CommandDispatchError::NineDoorUnavailable { verb } => {
                self.audit.denied("ninedoor unavailable");
                self.emit_console_line("ERR: NineDoor unavailable");
                self.emit_ack_err(verb.as_label(), Some("reason=ninedoor-unavailable"));
            }
            CommandDispatchError::UnsupportedForNineDoor { verb } => {
                self.audit.denied("ninedoor unsupported command");
                self.emit_console_line("ERR unsupported for NineDoor");
                self.emit_ack_err(verb.as_label(), Some("reason=unsupported"));
            }
            CommandDispatchError::Bridge { verb, source } => {
                let detail = format_message(format_args!("reason=ninedoor-error error={source}"));
                let audit_line = format_message(format_args!("ninedoor bridge error: {source}"));
                self.audit.denied(audit_line.as_str());
                self.emit_ack_err(verb.as_label(), Some(detail.as_str()));
            }
        }
    }

    fn emit_stream_end_if_pending(&mut self) {
        if self.stream_end_pending {
            self.stream_end_pending = false;
            self.emit_console_line("END");
        }
    }

    fn ensure_authenticated(&mut self, minimum: SessionRole) -> bool {
        match (self.session, minimum) {
            (Some(SessionRole::Queen), _) => true,
            (Some(SessionRole::Worker), SessionRole::Worker) => true,
            _ => {
                self.metrics.denied_commands += 1;
                self.audit.denied("unauthenticated command");
                false
            }
        }
    }

    #[inline(never)]
    fn handle_attach(
        &mut self,
        role: HeaplessString<{ MAX_ROLE_LEN }>,
        ticket: Option<HeaplessString<{ MAX_TICKET_LEN }>>,
    ) {
        if let Err(delay) = self.throttle.check(self.now_ms) {
            let message = format_message(format_args!("attach throttled ({} ms)", delay));
            self.audit.denied(message.as_str());
            self.metrics.denied_commands += 1;
            let detail = format_message(format_args!("reason=throttled delay_ms={delay}"));
            self.emit_ack_err("ATTACH", Some(detail.as_str()));
            return;
        }

        let Some(requested_role) = parse_role(&role) else {
            self.audit.denied("attach: invalid role");
            self.metrics.denied_commands += 1;
            self.emit_ack_err("ATTACH", Some("reason=invalid-role"));
            return;
        };

        let ticket_str = ticket.as_ref().map(|t| t.as_str());
        log::info!(
            target: "net-console",
            "[net-console] auth: parsed role={:?} ticket_present={}",
            requested_role,
            ticket_str.is_some()
        );
        let validated = self.validator.validate(requested_role, ticket_str);
        if let Err(err) = self.parser.record_login_attempt(validated, self.now_ms) {
            let message = format_message(format_args!("attach rate limited: {}", err));
            self.audit.denied(message.as_str());
            self.metrics.denied_commands += 1;
            let detail = match err {
                ConsoleError::RateLimited(delay) => {
                    format_message(format_args!("reason=rate-limited delay_ms={delay}"))
                }
                other => format_message(format_args!("reason={}", other)),
            };
            self.emit_ack_err("ATTACH", Some(detail.as_str()));
            return;
        }

        if validated {
            self.session = SessionRole::from_role(requested_role);
            self.metrics.accepted_commands += 1;
            self.throttle.register_success();
            let message = format_message(format_args!("attach accepted role={:?}", requested_role));
            self.audit.info(message.as_str());
            let role_label = proto_role_label(proto_role_from_ticket(requested_role));
            let detail = format_message(format_args!("role={role_label}"));
            self.emit_ack_ok("ATTACH", Some(detail.as_str()));
            log::info!(
                target: "net-console",
                "[net-console] auth: success; attaching session role={role_label}"
            );
        } else {
            self.throttle.register_failure(self.now_ms);
            self.metrics.denied_commands += 1;
            self.audit.denied("attach denied");
            log::warn!(
                target: "net-console",
                "[net-console] auth: failed validation for role={:?} ticket_present={}",
                requested_role,
                ticket_str.is_some()
            );
            self.emit_ack_err("ATTACH", Some("reason=denied"));
        }
    }
}

#[cfg(test)]
impl<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>
    EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    pub(crate) fn serial_mut(&mut self) -> &mut SerialPort<D, RX, TX, LINE> {
        &mut self.serial
    }
}

#[cfg(feature = "net-console")]
impl<'a, D, T, I, V, const RX: usize, const TX: usize, const LINE: usize>
    EventPump<'a, D, T, I, V, RX, TX, LINE>
where
    D: SerialDriver,
    T: TimerSource,
    I: IpcDispatcher,
    V: CapabilityValidator,
{
    /// Access the attached networking poller (test support only).
    pub fn network_mut(&mut self) -> Option<&mut (dyn NetPoller + 'a)> {
        self.net.as_deref_mut()
    }
}

fn parse_role(raw: &str) -> Option<Role> {
    match raw {
        value if value.eq_ignore_ascii_case(proto_role_label(ProtoRole::Queen)) => {
            Some(Role::Queen)
        }
        "worker" => Some(Role::WorkerHeartbeat),
        value if value.eq_ignore_ascii_case(proto_role_label(ProtoRole::Worker)) => {
            Some(Role::WorkerHeartbeat)
        }
        value if value.eq_ignore_ascii_case(proto_role_label(ProtoRole::GpuWorker)) => {
            Some(Role::WorkerGpu)
        }
        _ => None,
    }
}

fn proto_role_from_ticket(role: Role) -> ProtoRole {
    match role {
        Role::Queen => ProtoRole::Queen,
        Role::WorkerHeartbeat => ProtoRole::Worker,
        Role::WorkerGpu => ProtoRole::GpuWorker,
    }
}

#[cfg(feature = "kernel")]
#[derive(Debug, Clone, Copy)]
pub(crate) enum CommandVerb {
    Attach,
    Tail,
    Cat,
    Ls,
    Echo,
    Log,
    CacheLog,
    Quit,
    Spawn,
    Kill,
    Help,
    BootInfo,
    Caps,
    Mem,
    Ping,
    NetTest,
    NetStats,
}

#[cfg(feature = "kernel")]
impl CommandVerb {
    fn as_label(self) -> &'static str {
        match self {
            Self::Attach => "ATTACH",
            Self::Tail => "TAIL",
            Self::Cat => "CAT",
            Self::Ls => "LS",
            Self::Echo => "ECHO",
            Self::Log => "LOG",
            Self::CacheLog => "CACHELOG",
            Self::Quit => "QUIT",
            Self::Spawn => "SPAWN",
            Self::Kill => "KILL",
            Self::Help => "HELP",
            Self::BootInfo => "BOOTINFO",
            Self::Caps => "CAPS",
            Self::Mem => "MEM",
            Self::Ping => "PING",
            Self::NetTest => "NETTEST",
            Self::NetStats => "NETSTATS",
        }
    }
}

#[cfg(feature = "kernel")]
impl From<&Command> for CommandVerb {
    fn from(command: &Command) -> Self {
        match command {
            Command::Attach { .. } => Self::Attach,
            Command::Tail { .. } => Self::Tail,
            Command::Cat { .. } => Self::Cat,
            Command::Ls { .. } => Self::Ls,
            Command::Echo { .. } => Self::Echo,
            Command::Log => Self::Log,
            Command::CacheLog { .. } => Self::CacheLog,
            Command::Quit => Self::Quit,
            Command::Spawn(_) => Self::Spawn,
            Command::Kill(_) => Self::Kill,
            Command::Help => Self::Help,
            Command::BootInfo => Self::BootInfo,
            Command::Caps => Self::Caps,
            Command::Mem => Self::Mem,
            Command::Ping => Self::Ping,
            Command::NetTest => Self::NetTest,
            Command::NetStats => Self::NetStats,
        }
    }
}

#[cfg(feature = "kernel")]
#[derive(Debug)]
pub(crate) enum CommandDispatchError {
    NineDoorUnavailable {
        verb: CommandVerb,
    },
    UnsupportedForNineDoor {
        verb: CommandVerb,
    },
    Bridge {
        verb: CommandVerb,
        source: NineDoorBridgeError,
    },
}

#[cfg(not(feature = "kernel"))]
pub(crate) type CommandDispatchError = core::convert::Infallible;

#[cfg(feature = "kernel")]
#[cfg_attr(not(debug_assertions), allow(dead_code))]
#[inline(never)]
extern "C" fn vtable_sentinel() {}

#[cfg(test)]
mod tests {
    use super::*;
    use cohesix_ticket::{BudgetSpec, MountSpec, TicketClaims, TicketIssuer};
    #[cfg(feature = "net-console")]
    use crate::net::NetTelemetry;
    #[cfg(feature = "kernel")]
    use crate::ninedoor::NineDoorBridge;
    use crate::serial::test_support::LoopbackSerial;
    use crate::serial::SerialPort;

    struct TestTimer {
        ticks: HeaplessVec<TickEvent, 8>,
        index: usize,
    }

    impl TestTimer {
        fn single(tick: TickEvent) -> Self {
            let mut ticks = HeaplessVec::new();
            let _ = ticks.push(tick);
            Self { ticks, index: 0 }
        }

        fn repeated(count: usize, spacing_ms: u64) -> Self {
            let mut ticks = HeaplessVec::new();
            for i in 0..count {
                let _ = ticks.push(TickEvent {
                    tick: (i + 1) as u64,
                    now_ms: (i as u64 + 1) * spacing_ms,
                });
            }
            Self { ticks, index: 0 }
        }
    }

    impl TimerSource for TestTimer {
        fn poll(&mut self, _now_ms: u64) -> Option<TickEvent> {
            if self.index >= self.ticks.len() {
                return None;
            }
            let tick = self.ticks[self.index];
            self.index += 1;
            Some(tick)
        }
    }

    #[test]
    fn bootstrap_backoff_triggers_once_limit_reached() {
        let mut backoff = BootstrapBackoff::new(3);
        assert_eq!(backoff.observe(false), None);
        assert_eq!(backoff.observe(false), None);
        assert_eq!(backoff.observe(false), Some(3));
    }

    #[test]
    fn bootstrap_backoff_resets_when_message_staged() {
        let mut backoff = BootstrapBackoff::new(2);
        assert_eq!(backoff.observe(false), None);
        assert_eq!(backoff.observe(true), None);
        assert_eq!(backoff.observe(false), None);
        assert_eq!(backoff.observe(false), Some(2));
    }

    struct NullIpc;

    impl IpcDispatcher for NullIpc {
        fn dispatch(&mut self, _now_ms: u64) {}
    }

    #[cfg(feature = "kernel")]
    struct StubIpc {
        dispatched: bool,
        message: Option<BootstrapMessage>,
    }

    #[cfg(feature = "kernel")]
    impl StubIpc {
        fn new(message: BootstrapMessage) -> Self {
            Self {
                dispatched: false,
                message: Some(message),
            }
        }
    }

    #[cfg(feature = "kernel")]
    impl IpcDispatcher for StubIpc {
        fn dispatch(&mut self, _now_ms: u64) {
            self.dispatched = true;
        }

        fn take_bootstrap_message(&mut self) -> Option<BootstrapMessage> {
            if self.dispatched {
                self.message.take()
            } else {
                None
            }
        }
    }

    #[cfg(feature = "kernel")]
    struct ProbeIpc {
        staged: Option<BootstrapMessage>,
        pending: Option<BootstrapMessage>,
        polls: u32,
    }

    #[cfg(feature = "kernel")]
    impl ProbeIpc {
        fn new(message: BootstrapMessage) -> Self {
            Self {
                staged: None,
                pending: Some(message),
                polls: 0,
            }
        }
    }

    #[cfg(feature = "kernel")]
    impl IpcDispatcher for ProbeIpc {
        fn dispatch(&mut self, _now_ms: u64) {
            if self.staged.is_none() {
                self.staged = self.pending.take();
            }
        }

        fn take_bootstrap_message(&mut self) -> Option<BootstrapMessage> {
            self.staged.take()
        }

        fn bootstrap_poll(&mut self, _now_ms: u64) -> bool {
            self.polls = self.polls.saturating_add(1);
            if self.polls > 1 {
                panic!("bootstrap probe failed to observe drained message");
            }
            false
        }
    }

    struct AuditLog {
        entries: heapless::Vec<HeaplessString<64>, 32>,
        denials: heapless::Vec<HeaplessString<64>, 32>,
    }

    impl AuditLog {
        fn new() -> Self {
            Self {
                entries: heapless::Vec::new(),
                denials: heapless::Vec::new(),
            }
        }
    }

    fn issue_token(secret: &str, role: Role) -> String {
        let budget = match role {
            Role::Queen => BudgetSpec::unbounded(),
            Role::WorkerHeartbeat => BudgetSpec::default_heartbeat(),
            Role::WorkerGpu => BudgetSpec::default_gpu(),
        };
        let issuer = TicketIssuer::new(secret);
        let claims = TicketClaims::new(role, budget, None, MountSpec::empty(), 0);
        issuer.issue(claims).unwrap().encode().unwrap()
    }

    impl AuditSink for AuditLog {
        fn info(&mut self, message: &str) {
            let mut buf = HeaplessString::new();
            let _ = buf.push_str(message);
            let _ = self.entries.push(buf);
        }

        fn denied(&mut self, message: &str) {
            let mut buf = HeaplessString::new();
            let _ = buf.push_str(message);
            let _ = self.denials.push(buf);
        }
    }

    #[test]
    fn pump_bootstrap_logs_subsystems() {
        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, 32>::new(driver);
        let timer = TestTimer::single(TickEvent {
            tick: 1,
            now_ms: 10,
        });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        pump.poll();
        let metrics = pump.metrics();
        drop(pump);
        assert!(audit.entries.iter().any(|e| e.contains("event-pump")));
        assert_eq!(metrics.timer_ticks, 1);
    }

    #[test]
    fn timer_tick_publishes_hal_timebase() {
        crate::hal::set_timebase_now_ms(0);

        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 5 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);

        pump.poll();

        assert_eq!(crate::hal::timebase().now_ms(), 5);

        crate::hal::set_timebase_now_ms(0);
    }

    #[test]
    fn authentication_throttles_failures() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::repeated(3, 5);
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "secret").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        let driver = pump.serial_mut().driver_mut();
        driver.push_rx(b"attach queen wrong\nattach queen wrong\n");
        pump.poll();
        drop(pump);
        assert!(audit.denials.iter().any(|line| line.contains("attach")));
        assert!(!audit.denials.is_empty());
    }

    #[test]
    fn queen_attach_without_ticket_is_permitted() {
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "bootstrap").unwrap();
        assert!(store.validate(Role::Queen, None));
        assert!(store.validate(Role::Queen, Some("   ")));
    }

    #[test]
    fn worker_roles_still_require_tickets() {
        let mut store: TicketTable<4> = TicketTable::new();
        store
            .register(Role::WorkerHeartbeat, "worker-ticket")
            .unwrap();
        assert!(!store.validate(Role::WorkerHeartbeat, None));
        assert!(!store.validate(Role::WorkerHeartbeat, Some("  ")));
        let token = issue_token("worker-ticket", Role::WorkerHeartbeat);
        assert!(store.validate(Role::WorkerHeartbeat, Some(token.as_str())));
    }

    #[cfg(feature = "kernel")]
    struct CaptureBootstrap {
        messages: heapless::Vec<BootstrapMessage, 4>,
    }

    #[cfg(feature = "kernel")]
    impl CaptureBootstrap {
        fn new() -> Self {
            Self {
                messages: heapless::Vec::new(),
            }
        }
    }

    #[cfg(feature = "kernel")]
    impl BootstrapMessageHandler for CaptureBootstrap {
        fn handle(&mut self, message: &BootstrapMessage, audit: &mut dyn AuditSink) {
            let mut line = HeaplessString::<96>::new();
            let _ = line.push_str("handler bootstrap badge=");
            let _ = write!(line, "0x{:016x}", message.badge);
            audit.info(line.as_str());
            let _ = self.messages.push(message.clone());
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn bootstrap_handler_receives_staged_message() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 5 });

        let mut payload: HeaplessVec<sel4_sys::seL4_Word, { MAX_BOOTSTRAP_WORDS }> =
            HeaplessVec::new();
        let _ = payload.push(0x1234);
        let message = BootstrapMessage {
            badge: 0xDEAD,
            info: sel4_sys::seL4_MessageInfo::new(0xCA, 0, 0, 1),
            payload,
        };
        let ipc = StubIpc::new(message.clone());
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let handler = &mut CaptureBootstrap::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_bootstrap_handler(handler);

        pump.poll();

        assert_eq!(handler.messages.len(), 1);
        assert_eq!(handler.messages[0].badge, 0xDEAD);
        assert_eq!(handler.messages[0].payload.as_slice(), &[0x1234]);
        assert!(audit
            .entries
            .iter()
            .any(|entry| entry.contains("handler bootstrap")));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn bootstrap_probe_exits_after_poll_consumes_message() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 5 });

        let mut payload: HeaplessVec<sel4_sys::seL4_Word, { MAX_BOOTSTRAP_WORDS }> =
            HeaplessVec::new();
        let _ = payload.push(0xC0DE);
        let message = BootstrapMessage {
            badge: 0xBEEF,
            info: sel4_sys::seL4_MessageInfo::new(0xAA, 0, 0, 1),
            payload,
        };

        let ipc = ProbeIpc::new(message.clone());
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pass").unwrap();
        let mut audit = AuditLog::new();
        let handler = &mut CaptureBootstrap::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_bootstrap_handler(handler);

        pump.bootstrap_probe();

        let metrics = pump.metrics();
        drop(pump);

        assert_eq!(handler.messages.len(), 1);
        assert_eq!(handler.messages[0], message);
        assert_eq!(metrics.bootstrap_messages, 1);
    }

    #[test]
    fn successful_attach_allows_privileged_commands() {
        let driver = LoopbackSerial::<64>::new();
        let serial = SerialPort::<_, 64, 64, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ok").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        let driver = pump.serial_mut().driver_mut();
        let token = issue_token("ok", Role::Queen);
        let line = format!("attach queen {token}\nlog\n");
        driver.push_rx(line.as_bytes());
        pump.poll();
        drop(pump);
        assert!(audit
            .entries
            .iter()
            .any(|entry| entry.contains("log stream")));
    }

    #[cfg(feature = "net-console")]
    #[test]
    fn network_lines_feed_parser() {
        struct FakeNet {
            lines: heapless::Vec<HeaplessString<DEFAULT_LINE_CAPACITY>, 4>,
            sent: heapless::Vec<HeaplessString<DEFAULT_LINE_CAPACITY>, 4>,
        }

        impl FakeNet {
            fn new() -> Self {
                Self {
                    lines: heapless::Vec::new(),
                    sent: heapless::Vec::new(),
                }
            }
        }

        impl NetPoller for FakeNet {
            fn poll(&mut self, _now_ms: u64) -> bool {
                true
            }

            fn telemetry(&self) -> NetTelemetry {
                NetTelemetry {
                    link_up: true,
                    tx_drops: 0,
                    last_poll_ms: 0,
                }
            }

            fn drain_console_lines(
                &mut self,
                visitor: &mut dyn FnMut(HeaplessString<DEFAULT_LINE_CAPACITY>),
            ) {
                while !self.lines.is_empty() {
                    let line = self.lines.remove(0);
                    visitor(line);
                }
            }

            fn send_console_line(&mut self, line: &str) {
                let mut buf = HeaplessString::new();
                if buf.push_str(line).is_err() {
                    return;
                }
                let _ = self.sent.push(buf);
            }
        }

        let driver = LoopbackSerial::<16>::new();
        let serial = SerialPort::<_, 16, 16, 32>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "net").unwrap();
        let mut audit = AuditLog::new();
        let mut net = FakeNet::new();
        let mut line = HeaplessString::new();
        let token = issue_token("net", Role::Queen);
        line.push_str(format!("attach queen {token}").as_str())
            .unwrap();
        net.lines.push(line).unwrap();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit).with_network(&mut net);
        pump.poll();
        drop(pump);
        assert!(audit
            .entries
            .iter()
            .any(|entry| entry.contains("attach accepted")));
        assert!(net
            .sent
            .iter()
            .any(|line| line.as_str().starts_with("OK ATTACH")));
    }

    #[test]
    fn console_acknowledgements_emit_expected_lines() {
        let driver = LoopbackSerial::<256>::new();
        let serial = SerialPort::<_, 128, 128, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        {
            let driver = pump.serial_mut().driver_mut();
            let token = issue_token("ticket", Role::Queen);
            let line = format!("log\nattach queen {token}\nlog\n");
            driver.push_rx(line.as_bytes());
        }
        pump.poll();
        pump.poll();
        pump.poll();
        let tx = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let transcript: Vec<u8> = tx.into_iter().collect();
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(
            rendered.contains("ERR LOG reason=unauthenticated"),
            "{rendered}"
        );
        assert!(rendered.contains("OK ATTACH role=queen"), "{rendered}");
        assert!(rendered.contains("OK LOG"), "{rendered}");
    }

    #[test]
    fn tail_command_emits_end_sentinel() {
        let driver = LoopbackSerial::<256>::new();
        let serial = SerialPort::<_, 128, 128, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "queen-ticket").unwrap();
        store
            .register(Role::WorkerHeartbeat, "worker-ticket")
            .unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        {
            let driver = pump.serial_mut().driver_mut();
            let worker_token = issue_token("worker-ticket", Role::WorkerHeartbeat);
            let line = format!("attach worker {worker_token}\n");
            driver.push_rx(line.as_bytes());
            driver.push_rx(b"tail /log/queen.log\n");
        }
        pump.poll();
        pump.poll();
        let transcript = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let rendered = String::from_utf8(transcript.into_iter().collect())
            .expect("serial output must be utf8");
        assert!(
            rendered.contains("OK ATTACH role=worker-heartbeat"),
            "{rendered}"
        );
        assert!(
            rendered.contains("OK TAIL path=/log/queen.log"),
            "{rendered}"
        );
        assert!(rendered.contains("END\r\n"), "{rendered}");
    }

    #[test]
    fn log_command_emits_end_sentinel_and_quit_clears_session() {
        let driver = LoopbackSerial::<128>::new();
        let serial = SerialPort::<_, 128, 128, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "ticket").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        {
            let driver = pump.serial_mut().driver_mut();
            let token = issue_token("ticket", Role::Queen);
            let line = format!("attach queen {token}\n");
            driver.push_rx(line.as_bytes());
            driver.push_rx(b"log\n");
            driver.push_rx(b"quit\n");
            driver.push_rx(b"log\n");
        }
        pump.poll();
        pump.poll();
        pump.poll();
        pump.poll();
        let mut rendered = String::new();
        loop {
            pump.serial_mut().poll_io();
            let transcript = {
                let driver = pump.serial_mut().driver_mut();
                driver.drain_tx()
            };
            if transcript.is_empty() {
                break;
            }
            rendered.push_str(
                String::from_utf8(transcript.into_iter().collect())
                    .expect("serial output must be utf8")
                    .as_str(),
            );
        }
        assert!(rendered.contains("OK ATTACH role=queen"), "{rendered}");
        assert!(rendered.contains("OK LOG"), "{rendered}");
        assert!(rendered.contains("END\r\n"), "{rendered}");
        assert!(rendered.contains("OK QUIT"), "{rendered}");
        assert!(
            rendered.contains("ERR LOG reason=unauthenticated"),
            "{rendered}"
        );
    }

    #[test]
    fn ping_generates_pong_ack() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 32>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pong").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
        pump.session = Some(SessionRole::Queen);
        {
            let driver = pump.serial_mut().driver_mut();
            driver.push_rx(b"PING\n");
        }
        pump.poll();
        pump.poll();
        let tx = {
            let driver = pump.serial_mut().driver_mut();
            driver.drain_tx()
        };
        let transcript: Vec<u8> = tx.into_iter().collect();
        let rendered = String::from_utf8(transcript).expect("serial output must be utf8");
        assert!(rendered.contains("PONG"), "{rendered}");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn forwards_commands_to_ninedoor_bridge() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 5 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "secret").unwrap();
        let mut audit = AuditLog::new();
        let mut bridge = NineDoorBridge::new();
        let mut pump =
            EventPump::new(serial, timer, ipc, store, &mut audit).with_ninedoor(&mut bridge);

        pump.session = Some(SessionRole::Queen);
        pump.handle_command(Command::Log)
            .expect("forward log to NineDoor");

        assert!(audit
            .entries
            .iter()
            .any(|entry| entry.contains("nine-door: log stream requested")));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn error_when_forwarding_without_ninedoor() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 64>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 5 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "secret").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);

        pump.session = Some(SessionRole::Queen);
        let result = pump.handle_command(Command::Log);

        match result {
            Err(CommandDispatchError::NineDoorUnavailable { verb }) => {
                assert_eq!(verb.as_label(), "LOG");
            }
            other => panic!("unexpected result: {other:?}"),
        }

        assert!(audit
            .denials
            .iter()
            .any(|entry| entry.contains("ninedoor unavailable")));
    }
}
