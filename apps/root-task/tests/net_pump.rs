// Author: Lukas Bower
// Author: Lukas Bower
#![cfg(feature = "net-console")]

use std::str;

use cohesix_ticket::Role;
use root_task::event::{AuditSink, EventPump, IpcDispatcher, TickEvent, TicketTable, TimerSource};
use root_task::net::{NetStack, CONSOLE_QUEUE_DEPTH, CONSOLE_TCP_PORT};
use root_task::serial::{
    test_support::LoopbackSerial, SerialPort, DEFAULT_LINE_CAPACITY, DEFAULT_RX_CAPACITY,
    DEFAULT_TX_CAPACITY,
};
use smoltcp::wire::Ipv4Address;

struct MonotonicTimer {
    tick: u64,
    step_ms: u64,
}

impl MonotonicTimer {
    fn new(step_ms: u64) -> Self {
        Self { tick: 0, step_ms }
    }
}

impl TimerSource for MonotonicTimer {
    fn poll(&mut self, _now_ms: u64) -> Option<TickEvent> {
        self.tick = self.tick.saturating_add(1);
        Some(TickEvent {
            tick: self.tick,
            now_ms: self.tick.saturating_mul(self.step_ms),
        })
    }
}

struct NullIpc;

impl IpcDispatcher for NullIpc {
    fn dispatch(&mut self, _now_ms: u64) {}
}

struct AuditCapture {
    info: heapless::Vec<heapless::String<96>, 32>,
    denials: heapless::Vec<heapless::String<96>, 32>,
}

impl AuditCapture {
    fn new() -> Self {
        Self {
            info: heapless::Vec::new(),
            denials: heapless::Vec::new(),
        }
    }
}

impl AuditSink for AuditCapture {
    fn info(&mut self, message: &str) {
        let mut buf = heapless::String::new();
        let _ = buf.push_str(message);
        let _ = self.info.push(buf);
    }

    fn denied(&mut self, message: &str) {
        let mut buf = heapless::String::new();
        let _ = buf.push_str(message);
        let _ = self.denials.push(buf);
    }
}

type TestPump<'a> = EventPump<
    'a,
    LoopbackSerial<{ DEFAULT_RX_CAPACITY }>,
    MonotonicTimer,
    NullIpc,
    TicketTable<4>,
    { DEFAULT_RX_CAPACITY },
    { DEFAULT_TX_CAPACITY },
    { DEFAULT_LINE_CAPACITY },
>;

fn build_pump<'a>(
    serial: LoopbackSerial<{ DEFAULT_RX_CAPACITY }>,
    audit: &'a mut AuditCapture,
) -> TestPump<'a> {
    let port = SerialPort::new(serial);
    let timer = MonotonicTimer::new(5);
    let ipc = NullIpc;
    let mut tickets: TicketTable<4> = TicketTable::new();
    tickets.register(Role::Queen, "token").unwrap();
    EventPump::new(port, timer, ipc, tickets, audit)
}

#[test]
fn net_console_uses_default_port() {
    let serial = LoopbackSerial::<{ DEFAULT_RX_CAPACITY }>::new();
    let mut audit = AuditCapture::new();
    let mut pump = build_pump(serial, &mut audit);
    let (mut net, _handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 15));
    pump = pump.with_network(&mut net);

    let listen_port = pump
        .network_mut()
        .expect("network not attached")
        .console_listen_port();

    assert_eq!(listen_port, CONSOLE_TCP_PORT);
}

#[test]
fn network_lines_round_trip_acknowledgements() {
    let serial = LoopbackSerial::<{ DEFAULT_RX_CAPACITY }>::new();
    let mut audit = AuditCapture::new();
    let mut pump = build_pump(serial, &mut audit);
    let (mut net, handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 15));
    pump = pump.with_network(&mut net);

    {
        let net_iface = pump.network_mut().expect("network not attached");
        net_iface.inject_console_line("attach queen token\n");
        net_iface.inject_console_line("log\n");
    }

    for _ in 0..3 {
        pump.poll();
    }

    let auth = handle.pop_tx().expect("auth acknowledgement missing");
    assert_eq!(str::from_utf8(auth.as_slice()).unwrap(), "OK AUTH\r\n");
    let attach = handle.pop_tx().expect("attach acknowledgement missing");
    assert!(str::from_utf8(attach.as_slice())
        .unwrap()
        .starts_with("OK ATTACH"));
    let log = handle.pop_tx().expect("log acknowledgement missing");
    assert_eq!(str::from_utf8(log.as_slice()).unwrap(), "OK LOG\r\n");
    assert!(pump.metrics().accepted_commands >= 2);
    assert!(audit.info.iter().any(|line| line.contains("console: log")));
}

#[test]
fn auth_and_attach_survive_saturated_queue() {
    let serial = LoopbackSerial::<{ DEFAULT_RX_CAPACITY }>::new();
    let mut audit = AuditCapture::new();
    let mut pump = build_pump(serial, &mut audit);
    let (mut net, handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 99));
    pump = pump.with_network(&mut net);

    {
        let net_iface = pump.network_mut().expect("network not attached");
        for _ in 0..(CONSOLE_QUEUE_DEPTH * 6) {
            net_iface.send_console_line("NOISE");
        }
        net_iface.inject_console_line("attach queen token\n");
    }

    for _ in 0..6 {
        pump.poll();
    }

    let mut frames = heapless::Vec::<heapless::String<96>, 16>::new();
    while let Some(frame) = handle.pop_tx() {
        let as_str = str::from_utf8(frame.as_slice())
            .unwrap()
            .trim_end()
            .to_owned();
        let mut line = heapless::String::new();
        line.push_str(as_str.as_str()).unwrap();
        let _ = frames.push(line);
    }

    assert!(
        frames.iter().any(|line| line.starts_with("OK AUTH")),
        "auth acknowledgement missing ({frames:?})"
    );
    assert!(
        frames.iter().any(|line| line.starts_with("OK ATTACH")),
        "attach acknowledgement missing ({frames:?})"
    );
}

#[test]
fn attach_with_bad_ticket_is_rejected() {
    let serial = LoopbackSerial::<{ DEFAULT_RX_CAPACITY }>::new();
    let mut audit = AuditCapture::new();
    let mut pump = build_pump(serial, &mut audit);
    let (mut net, handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 55));
    pump = pump.with_network(&mut net);

    {
        let net_iface = pump.network_mut().expect("network not attached");
        net_iface.inject_console_line("attach queen wrong-token\n");
    }

    pump.poll();
    pump.poll();

    let auth = handle.pop_tx().expect("auth acknowledgement missing");
    assert_eq!(str::from_utf8(auth.as_slice()).unwrap(), "OK AUTH\r\n");
    let attach = handle.pop_tx().expect("attach acknowledgement missing");
    assert!(str::from_utf8(attach.as_slice())
        .unwrap()
        .starts_with("ERR ATTACH"));
    assert!(pump.metrics().denied_commands >= 1 || !audit.denials.is_empty());
}

#[test]
fn tx_queue_saturation_updates_telemetry() {
    let serial = LoopbackSerial::<{ DEFAULT_RX_CAPACITY }>::new();
    let mut audit = AuditCapture::new();
    let mut pump = build_pump(serial, &mut audit);
    let (mut net, handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 25));
    pump = pump.with_network(&mut net);

    {
        let net_iface = pump.network_mut().expect("network not attached");
        net_iface.inject_console_line("attach queen token\n");
    }

    pump.poll();

    {
        let net_iface = pump.network_mut().expect("network not attached");
        for _ in 0..64 {
            net_iface.send_console_line("OK SATURATE");
        }
    }

    for _ in 0..2 {
        pump.poll();
    }

    let mut drained = 0;
    while handle.pop_tx().is_some() {
        drained += 1;
    }
    assert!(drained > 0);
    assert!(pump.metrics().accepted_commands >= 1);
}

#[test]
fn ping_requires_session_before_ack() {
    let serial = LoopbackSerial::<{ DEFAULT_RX_CAPACITY }>::new();
    let mut audit = AuditCapture::new();
    let mut pump = build_pump(serial, &mut audit);
    let (mut net, handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 77));
    pump = pump.with_network(&mut net);

    {
        let net_iface = pump.network_mut().expect("network not attached");
        net_iface.inject_console_line("ping\n");
    }

    pump.poll();
    pump.poll();

    let mut frames = heapless::Vec::<heapless::String<96>, 8>::new();
    while let Some(frame) = handle.pop_tx() {
        let as_str = str::from_utf8(frame.as_slice())
            .unwrap()
            .trim_end()
            .to_owned();
        let mut line = heapless::String::new();
        line.push_str(as_str.as_str()).unwrap();
        frames.push(line).unwrap();
    }

    assert!(
        frames.iter().any(|line| line.starts_with("OK AUTH")),
        "auth acknowledgement missing ({frames:?})"
    );
    assert!(
        frames
            .iter()
            .any(|line| line.starts_with("ERR PING reason=unauthenticated")),
        "frames captured: {frames:?}"
    );
    assert!(pump.metrics().denied_commands >= 1 || !audit.denials.is_empty());
}

#[test]
fn ping_round_trips_after_attach() {
    let serial = LoopbackSerial::<{ DEFAULT_RX_CAPACITY }>::new();
    let mut audit = AuditCapture::new();
    let mut pump = build_pump(serial, &mut audit);
    let (mut net, handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 88));
    pump = pump.with_network(&mut net);

    {
        let net_iface = pump.network_mut().expect("network not attached");
        net_iface.inject_console_line("attach queen token\n");
        net_iface.inject_console_line("ping\n");
    }

    for _ in 0..6 {
        pump.poll();
    }

    let mut frames = heapless::Vec::<heapless::String<96>, 12>::new();
    while let Some(frame) = handle.pop_tx() {
        let as_str = str::from_utf8(frame.as_slice())
            .unwrap()
            .trim_end()
            .to_owned();
        let mut line = heapless::String::new();
        line.push_str(as_str.as_str()).unwrap();
        frames.push(line).unwrap();
    }

    let auth_seen = frames.iter().position(|line| line.starts_with("OK AUTH"));
    let attach_seen = frames.iter().position(|line| line.starts_with("OK ATTACH"));
    let pong_seen = frames.iter().position(|line| line.starts_with("PONG"));
    let ack_seen = frames
        .iter()
        .position(|line| line.starts_with("OK PING reply=pong"));

    assert!(
        auth_seen.is_some(),
        "auth acknowledgement missing ({frames:?})"
    );
    assert!(
        attach_seen.is_some(),
        "attach acknowledgement missing ({frames:?})"
    );
    assert!(
        pong_seen.is_some(),
        "PONG console line missing ({frames:?})"
    );
    assert!(
        ack_seen.is_some(),
        "ping acknowledgement missing ({frames:?})"
    );
    if let (Some(auth), Some(attach)) = (auth_seen, attach_seen) {
        assert!(auth <= attach, "attach emitted before auth: {frames:?}");
    }
    assert!(pump.metrics().accepted_commands >= 2);
    assert!(audit.info.iter().any(|line| line.contains("console: ping")));
}

#[test]
fn pump_survives_force_reset() {
    let serial = LoopbackSerial::<{ DEFAULT_RX_CAPACITY }>::new();
    let mut audit = AuditCapture::new();
    let mut pump = build_pump(serial, &mut audit);
    let (mut net, handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 45));
    pump = pump.with_network(&mut net);

    {
        let net_iface = pump.network_mut().expect("network not attached");
        net_iface.inject_console_line("attach queen token\n");
        net_iface.inject_console_line("log\n");
    }

    pump.poll();
    pump.poll();

    while handle.pop_tx().is_some() {}
    handle.reset();

    {
        let net_iface = pump.network_mut().expect("network not attached");
        net_iface.reset();
        net_iface.inject_console_line("attach queen token\n");
        net_iface.inject_console_line("tail /log/queen.log\n");
    }

    pump.poll();
    pump.poll();

    let auth = handle
        .pop_tx()
        .expect("auth acknowledgement missing after reset");
    assert_eq!(str::from_utf8(auth.as_slice()).unwrap(), "OK AUTH\r\n");
    let attach = handle
        .pop_tx()
        .expect("attach acknowledgement missing after reset");
    assert!(str::from_utf8(attach.as_slice())
        .unwrap()
        .starts_with("OK ATTACH"));
    let tail = handle
        .pop_tx()
        .expect("tail acknowledgement missing after reset");
    assert!(str::from_utf8(tail.as_slice())
        .unwrap()
        .starts_with("OK TAIL"));
}
