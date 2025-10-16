// Author: Lukas Bower

//! Cooperative event pump coordinating serial, timer, networking, and IPC work.
//!
//! The pump intentionally avoids dynamic allocation so it can operate in the
//! seL4 environment while remaining testable under `cargo test`. Each polling
//! cycle progresses the serial console, dispatches timer ticks, advances the
//! networking stack (when enabled), and finally services IPC queues.

use core::cmp::min;
use core::fmt::{self, Write as FmtWrite};

use cohesix_ticket::Role;
use heapless::{String as HeaplessString, Vec as HeaplessVec};

use crate::console::{Command, CommandParser, ConsoleError, MAX_ROLE_LEN, MAX_TICKET_LEN};
#[cfg(feature = "net")]
use crate::net::{NetPoller, CONSOLE_QUEUE_DEPTH};
#[cfg(feature = "kernel")]
use crate::ninedoor::NineDoorHandler;
use crate::serial::{SerialDriver, SerialPort, SerialTelemetry, DEFAULT_LINE_CAPACITY};

fn format_message(args: fmt::Arguments<'_>) -> HeaplessString<128> {
    let mut buf = HeaplessString::new();
    let _ = FmtWrite::write_fmt(&mut buf, args);
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

/// Timer abstraction used by the event pump.
pub trait TimerSource {
    /// Poll the timer for the next tick, if any.
    fn poll(&mut self, now_ms: u64) -> Option<TickEvent>;
}

/// IPC dispatcher invoked once per pump cycle.
pub trait IpcDispatcher {
    /// Service pending IPC messages.
    fn dispatch(&mut self, now_ms: u64);
}

/// Capability validator consulted when privileged verbs execute.
pub trait CapabilityValidator {
    /// Validate that `ticket` grants the requested `role`.
    fn validate(&self, role: Role, ticket: Option<&str>) -> bool;
}

/// Error raised when registering capability tickets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketError {
    /// The ticket table reached its capacity.
    Capacity,
    /// Provided ticket exceeded the allowed size.
    TicketTooLong,
}

#[derive(Debug)]
struct TicketRecord {
    role: Role,
    token: HeaplessString<{ MAX_TICKET_LEN }>,
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

    /// Register a new ticket.
    pub fn register(&mut self, role: Role, ticket: &str) -> Result<(), TicketError> {
        if ticket.len() > MAX_TICKET_LEN {
            return Err(TicketError::TicketTooLong);
        }
        if self.entries.is_full() {
            return Err(TicketError::Capacity);
        }
        let mut token: HeaplessString<{ MAX_TICKET_LEN }> = HeaplessString::new();
        token
            .push_str(ticket)
            .map_err(|_| TicketError::TicketTooLong)?;
        self.entries
            .push(TicketRecord { role, token })
            .map_err(|_| TicketError::Capacity)
    }
}

impl<const N: usize> Default for TicketTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> CapabilityValidator for TicketTable<N> {
    fn validate(&self, role: Role, ticket: Option<&str>) -> bool {
        let Some(ticket) = ticket else { return false };
        self.entries
            .iter()
            .any(|record| record.role == role && record.token.as_str() == ticket)
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AckStatus {
    Ok,
    Err,
}

impl AckStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Err => "ERR",
        }
    }
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

/// Networking integration exposed to the pump when the `net` feature is enabled.
/// Event pump orchestrating serial, timer, IPC, and optional networking work.
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
    throttle: AuthThrottle,
    #[cfg(feature = "net")]
    net: Option<&'a mut dyn NetPoller>,
    #[cfg(feature = "kernel")]
    ninedoor: Option<&'a mut dyn NineDoorHandler>,
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
            throttle: AuthThrottle::default(),
            #[cfg(feature = "net")]
            net: None,
            #[cfg(feature = "kernel")]
            ninedoor: None,
        }
    }

    /// Attach a networking poller to the event pump.
    #[cfg(feature = "net")]
    pub fn with_network(mut self, net: &'a mut dyn NetPoller) -> Self {
        self.audit.info("event-pump: init network");
        self.net = Some(net);
        self
    }

    /// Attach a NineDoor handler to the event pump.
    #[cfg(feature = "kernel")]
    pub fn with_ninedoor(mut self, handler: &'a mut dyn NineDoorHandler) -> Self {
        self.ninedoor = Some(handler);
        self
    }

    /// Execute a single cooperative polling cycle.
    pub fn poll(&mut self) {
        self.serial.poll_io();
        self.consume_serial();

        if let Some(tick) = self.timer.poll(self.now_ms) {
            self.now_ms = tick.now_ms;
            self.metrics.timer_ticks = self.metrics.timer_ticks.saturating_add(1);
            let message = format_message(format_args!("timer: tick {}", tick.tick));
            self.audit.info(message.as_str());
        }

        #[cfg(feature = "net")]
        if let Some(net) = self.net.as_mut() {
            if net.poll(self.now_ms) {
                let telemetry = net.telemetry();
                let message = format_message(format_args!(
                    "net: poll link_up={} tx_drops={}",
                    telemetry.link_up, telemetry.tx_drops
                ));
                self.audit.info(message.as_str());
            }
            let mut buffered: HeaplessVec<
                HeaplessString<DEFAULT_LINE_CAPACITY>,
                { CONSOLE_QUEUE_DEPTH },
            > = HeaplessVec::new();
            net.drain_console_lines(&mut |line| {
                let _ = buffered.push(line);
            });
            for line in buffered {
                self.handle_network_line(line);
            }
        }

        self.ipc.dispatch(self.now_ms);
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

    fn emit_console_line(&mut self, line: &str) {
        self.serial.enqueue_tx(line.as_bytes());
        self.serial.enqueue_tx(b"\r\n");
        #[cfg(feature = "net")]
        if let Some(net) = self.net.as_mut() {
            net.send_console_line(line);
        }
    }

    fn emit_ack(&mut self, status: AckStatus, verb: &str, detail: Option<&str>) {
        let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
        let _ = line.push_str(status.label());
        let _ = line.push(' ');
        let _ = line.push_str(verb);
        if let Some(extra) = detail {
            if !extra.is_empty() {
                let _ = line.push(' ');
                let _ = line.push_str(extra);
            }
        }
        self.emit_console_line(line.as_str());
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
            self.process_console_line(&line);
        }
    }

    fn process_console_line(&mut self, line: &HeaplessString<LINE>) {
        self.metrics.console_lines = self.metrics.console_lines.saturating_add(1);
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("ping") {
            self.emit_console_line("PONG");
            return;
        }
        if let Err(err) = self.feed_parser(line) {
            self.handle_console_error(err);
        }
    }

    fn feed_parser(&mut self, line: &HeaplessString<LINE>) -> Result<(), ConsoleError> {
        for byte in line.as_bytes() {
            self.parser.push_byte(*byte)?;
        }
        if let Some(command) = self.parser.push_byte(b'\n')? {
            self.handle_command(command);
        }
        Ok(())
    }

    #[cfg(feature = "net")]
    fn handle_network_line(&mut self, line: HeaplessString<DEFAULT_LINE_CAPACITY>) {
        let mut converted: HeaplessString<LINE> = HeaplessString::new();
        if converted.push_str(line.as_str()).is_err() {
            self.audit
                .denied("net console line exceeded maximum length");
            return;
        }
        self.process_console_line(&converted);
    }

    fn handle_command(&mut self, command: Command) {
        #[cfg(feature = "kernel")]
        let command_clone = command.clone();
        #[cfg(feature = "kernel")]
        let mut forwarded = false;
        match command {
            Command::Help => {
                self.audit.info("console: help");
                self.metrics.accepted_commands += 1;
                self.emit_ack_ok("HELP", None);
            }
            Command::Quit => {
                self.audit.info("console: quit");
                self.metrics.accepted_commands += 1;
                self.emit_ack_ok("QUIT", None);
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
                    #[cfg(feature = "kernel")]
                    {
                        forwarded = true;
                    }
                } else {
                    self.emit_auth_failure("TAIL");
                }
            }
            Command::Log => {
                if self.ensure_authenticated(SessionRole::Queen) {
                    self.audit.info("console: log stream start");
                    self.metrics.accepted_commands += 1;
                    self.emit_ack_ok("LOG", None);
                    #[cfg(feature = "kernel")]
                    {
                        forwarded = true;
                    }
                } else {
                    self.emit_auth_failure("LOG");
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
            self.forward_to_ninedoor(&command_clone);
        }
    }

    #[cfg(feature = "kernel")]
    fn forward_to_ninedoor(&mut self, command: &Command) {
        if let Some(handler) = self.ninedoor.as_mut() {
            handler.handle(command, &mut *self.audit);
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
            let role_label = match requested_role {
                Role::Queen => "queen",
                Role::WorkerHeartbeat => "worker-heartbeat",
                Role::WorkerGpu => "worker-gpu",
            };
            let detail = format_message(format_args!("role={role_label}"));
            self.emit_ack_ok("ATTACH", Some(detail.as_str()));
        } else {
            self.throttle.register_failure(self.now_ms);
            self.metrics.denied_commands += 1;
            self.audit.denied("attach denied");
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

fn parse_role(raw: &str) -> Option<Role> {
    match raw {
        value if value.eq_ignore_ascii_case("queen") => Some(Role::Queen),
        value if value.eq_ignore_ascii_case("worker") => Some(Role::WorkerHeartbeat),
        value if value.eq_ignore_ascii_case("worker-gpu") => Some(Role::WorkerGpu),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "net")]
    use crate::net::NetTelemetry;
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

    struct NullIpc;

    impl IpcDispatcher for NullIpc {
        fn dispatch(&mut self, _now_ms: u64) {}
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
        driver.push_rx(b"attach queen ok\nlog\n");
        pump.poll();
        drop(pump);
        assert!(audit
            .entries
            .iter()
            .any(|entry| entry.contains("log stream")));
    }

    #[cfg(feature = "net")]
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
        line.push_str("attach queen net").unwrap();
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
            driver.push_rx(b"log\nattach queen ticket\nlog\n");
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
    fn ping_generates_pong_ack() {
        let driver = LoopbackSerial::<32>::new();
        let serial = SerialPort::<_, 32, 32, 32>::new(driver);
        let timer = TestTimer::single(TickEvent { tick: 1, now_ms: 1 });
        let ipc = NullIpc;
        let mut store: TicketTable<4> = TicketTable::new();
        store.register(Role::Queen, "pong").unwrap();
        let mut audit = AuditLog::new();
        let mut pump = EventPump::new(serial, timer, ipc, store, &mut audit);
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
}
