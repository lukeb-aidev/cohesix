// Author: Lukas Bower
// Purpose: Event pump integration tests covering serial and timer coordination.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cohesix_ticket::Role;
use embedded_io::ErrorType;
use nb::Error as NbError;
use root_task::event::{AuditSink, EventPump, IpcDispatcher, TickEvent, TicketTable, TimerSource};
use root_task::serial::{
    SerialDriver, SerialError, SerialPort, DEFAULT_LINE_CAPACITY, DEFAULT_RX_CAPACITY,
    DEFAULT_TX_CAPACITY,
};

struct DeterministicTimer {
    ticks: heapless::Vec<TickEvent, 8>,
    index: usize,
}

impl DeterministicTimer {
    fn from_ticks(ticks: &[TickEvent]) -> Self {
        let mut buf = heapless::Vec::new();
        for tick in ticks.iter().copied() {
            let _ = buf.push(tick);
        }
        Self {
            ticks: buf,
            index: 0,
        }
    }
}

impl TimerSource for DeterministicTimer {
    fn poll(&mut self, _now_ms: u64) -> Option<TickEvent> {
        if self.index >= self.ticks.len() {
            return None;
        }
        let tick = self.ticks[self.index];
        self.index += 1;
        Some(tick)
    }
}

struct CountingIpc {
    calls: usize,
}

impl CountingIpc {
    fn new() -> Self {
        Self { calls: 0 }
    }
}

impl IpcDispatcher for CountingIpc {
    fn dispatch(&mut self, _now_ms: u64) {
        self.calls += 1;
    }
}

struct AuditCapture {
    entries: heapless::Vec<heapless::String<64>, 32>,
    denials: heapless::Vec<heapless::String<64>, 32>,
}

impl AuditCapture {
    fn new() -> Self {
        Self {
            entries: heapless::Vec::new(),
            denials: heapless::Vec::new(),
        }
    }
}

impl AuditSink for AuditCapture {
    fn info(&mut self, message: &str) {
        let mut buf = heapless::String::new();
        let _ = buf.push_str(message);
        let _ = self.entries.push(buf);
    }

    fn denied(&mut self, message: &str) {
        let mut buf = heapless::String::new();
        let _ = buf.push_str(message);
        let _ = self.denials.push(buf);
    }
}

type DefaultSerialPort<D> =
    SerialPort<D, { DEFAULT_RX_CAPACITY }, { DEFAULT_TX_CAPACITY }, { DEFAULT_LINE_CAPACITY }>;

#[derive(Clone)]
struct SharedSerial {
    rx: Arc<Mutex<VecDeque<u8>>>,
    tx: Arc<Mutex<VecDeque<u8>>>,
}

impl SharedSerial {
    fn new() -> Self {
        Self {
            rx: Arc::new(Mutex::new(VecDeque::new())),
            tx: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    fn push_rx(&self, data: &[u8]) {
        let mut guard = self.rx.lock().unwrap();
        for &byte in data {
            guard.push_back(byte);
        }
    }

    fn drain_tx(&self) -> Vec<u8> {
        let mut guard = self.tx.lock().unwrap();
        guard.drain(..).collect()
    }
}

impl ErrorType for SharedSerial {
    type Error = SerialError;
}

impl SerialDriver for SharedSerial {
    fn read_byte(&mut self) -> nb::Result<u8, Self::Error> {
        let mut guard = self.rx.lock().unwrap();
        guard.pop_front().ok_or(NbError::WouldBlock)
    }

    fn write_byte(&mut self, byte: u8) -> nb::Result<(), Self::Error> {
        let mut guard = self.tx.lock().unwrap();
        guard.push_back(byte);
        Ok(())
    }
}

#[test]
fn event_pump_services_serial_and_timer() {
    let driver = SharedSerial::new();
    let handle = driver.clone();
    let serial: DefaultSerialPort<_> = SerialPort::new(driver);
    let timer = DeterministicTimer::from_ticks(&[
        TickEvent { tick: 1, now_ms: 5 },
        TickEvent {
            tick: 2,
            now_ms: 10,
        },
    ]);
    let ipc = CountingIpc::new();
    let mut tickets: TicketTable<4> = TicketTable::new();
    tickets.register(Role::Queen, "token").unwrap();
    let mut audit = AuditCapture::new();
    let mut pump = EventPump::new(serial, timer, ipc, tickets, &mut audit);
    handle.push_rx(b"attach queen token\nlog\n");
    pump.poll();
    let metrics = pump.metrics();
    drop(pump);
    assert!(metrics.accepted_commands >= 2);
    assert_eq!(metrics.timer_ticks, 1);
    assert!(audit.entries.iter().any(|entry| entry.contains("log")));
}

#[test]
fn event_pump_tracks_denials_and_backpressure() {
    let driver = SharedSerial::new();
    let mut serial: SerialPort<_, 8, 8, 32> = SerialPort::new(driver);
    serial.enqueue_tx(b"1234567890");
    let timer = DeterministicTimer::from_ticks(&[TickEvent { tick: 1, now_ms: 1 }]);
    let ipc = CountingIpc::new();
    let tickets: TicketTable<1> = TicketTable::new();
    let mut audit = AuditCapture::new();
    let mut pump = EventPump::new(serial, timer, ipc, tickets, &mut audit);
    pump.poll();
    let telemetry = pump.serial_telemetry();
    drop(pump);
    assert!(telemetry.tx_backpressure >= 1);
}

#[test]
fn authentication_pressure_does_not_block_timer() {
    let driver = SharedSerial::new();
    let handle = driver.clone();
    let serial: DefaultSerialPort<_> = SerialPort::new(driver);
    let timer = DeterministicTimer::from_ticks(&[
        TickEvent { tick: 1, now_ms: 4 },
        TickEvent { tick: 2, now_ms: 8 },
    ]);
    let ipc = CountingIpc::new();
    let tickets: TicketTable<1> = TicketTable::new();
    let mut audit = AuditCapture::new();
    let mut pump = EventPump::new(serial, timer, ipc, tickets, &mut audit);
    handle.push_rx(b"attach queen nope\nattach queen nope\nattach queen nope\n");
    pump.poll();
    pump.poll();
    let metrics = pump.metrics();
    drop(pump);
    assert!(metrics.timer_ticks >= 2);
    assert!(metrics.denied_commands >= 1);
    assert!(audit.denials.iter().any(|entry| entry.contains("attach")));
}

#[test]
fn startup_banner_and_prompt_emit_once() {
    let driver = SharedSerial::new();
    let handle = driver.clone();
    let serial: DefaultSerialPort<_> = SerialPort::new(driver);
    let timer = DeterministicTimer::from_ticks(&[]);
    let ipc = CountingIpc::new();
    let mut tickets: TicketTable<4> = TicketTable::new();
    tickets.register(Role::Queen, "token").unwrap();
    let mut audit = AuditCapture::new();
    let mut pump = EventPump::new(serial, timer, ipc, tickets, &mut audit);

    pump.start_cli();

    let rendered = String::from_utf8(handle.drain_tx()).expect("utf8 transcript");
    assert!(
        rendered.contains("[Cohesix] Root console ready (type 'help' for commands)\r\nCohesix console ready\r\nCommands:"),
        "banner and help must appear contiguously: {rendered:?}"
    );
    assert_eq!(
        rendered.matches("cohesix> ").count(),
        1,
        "prompt must appear exactly once"
    );
    assert!(
        rendered.ends_with("cohesix> "),
        "prompt must terminate transcript: {rendered:?}"
    );
}
