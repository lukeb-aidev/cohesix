// Author: Lukas Bower
#![cfg(all(not(feature = "kernel"), not(target_os = "none")))]
#![allow(clippy::module_name_repetitions)]

use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result as AnyhowResult};
use cohesix_ticket::Role;
use heapless::Vec as HeaplessVec;

use crate::event::{
    AuditSink, EventPump, IpcDispatcher, PumpMetrics, TickEvent, TicketTable, TimerSource,
};
use crate::serial::{
    SerialDriver, SerialError, SerialPort, DEFAULT_LINE_CAPACITY, DEFAULT_RX_CAPACITY,
    DEFAULT_TX_CAPACITY,
};

/// Result alias used throughout the host-mode simulation.
pub type Result<T> = AnyhowResult<T>;

/// Entry point for host-mode execution of the root task simulation.
pub fn main() -> Result<()> {
    let stdout = io::stdout();
    run_with_writer(stdout.lock())
}

/// Runs the host simulation while emitting audit records to the supplied writer.
pub fn run_with_writer<W: Write>(mut writer: W) -> Result<()> {
    let (driver, injector) = HostSerial::new();
    seed_console_script(&injector);

    let serial: SerialPort<
        _,
        { DEFAULT_RX_CAPACITY },
        { DEFAULT_TX_CAPACITY },
        { DEFAULT_LINE_CAPACITY },
    > = SerialPort::new(driver);
    let timer = SleepTimer::new(Duration::from_millis(25), TICK_LIMIT);
    let ipc = HostIpc;
    let mut tickets: TicketTable<8> = TicketTable::new();
    tickets
        .register(Role::Queen, QUEEN_TICKET)
        .map_err(|err| anyhow!("failed to register queen ticket: {err:?}"))?;
    tickets
        .register(Role::WorkerHeartbeat, WORKER_TICKET)
        .map_err(|err| anyhow!("failed to register worker ticket: {err:?}"))?;

    let mut audit = WriterAudit::new(&mut writer);
    let mut pump = EventPump::new(serial, timer, ipc, tickets, &mut audit);

    let mut metrics = PumpMetrics::default();
    for _ in 0..MAX_CYCLES {
        pump.poll();
        metrics = pump.metrics();
        if metrics.timer_ticks >= TICK_LIMIT && metrics.console_lines >= SCRIPT.len() as u64 {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }

    if metrics.timer_ticks < TICK_LIMIT {
        return Err(anyhow!(
            "timer did not reach expected tick count ({} < {})",
            metrics.timer_ticks,
            TICK_LIMIT
        ));
    }

    if metrics.console_lines < SCRIPT.len() as u64 {
        return Err(anyhow!(
            "not all scripted console lines were processed ({} < {})",
            metrics.console_lines,
            SCRIPT.len()
        ));
    }

    writer.flush()?;
    Ok(())
}

/// Number of timer ticks expected from the host simulation loop.
const TICK_LIMIT: u64 = 3;

/// Maximum pump iterations permitted for the scripted simulation.
const MAX_CYCLES: usize = 512;

const QUEEN_TICKET: &str = "queen-bootstrap";
const WORKER_TICKET: &str = "worker-ticket";

/// Scripted console commands used to exercise the event pump.
const SCRIPT: &[&str] = &[
    "help",
    "attach queen queen-bootstrap",
    "log",
    "spawn {\"spawn\":\"heartbeat\",\"ticks\":5}",
    "attach worker worker-ticket",
    "tail /worker/self/telemetry",
    "quit",
];

fn seed_console_script(injector: &SerialInjector) {
    for line in SCRIPT {
        injector.push_rx(line.as_bytes());
        injector.push_rx(b"\n");
    }
}

/// Sleep-backed timer that emits ticks at a fixed interval.
struct SleepTimer {
    period: Duration,
    limit: u64,
    emitted: u64,
    next_deadline: Instant,
    elapsed_ms: u64,
}

impl SleepTimer {
    fn new(period: Duration, limit: u64) -> Self {
        let now = Instant::now();
        Self {
            period,
            limit,
            emitted: 0,
            next_deadline: now + period,
            elapsed_ms: 0,
        }
    }

    fn period_ms(&self) -> u64 {
        self.period.as_millis() as u64
    }
}

impl TimerSource for SleepTimer {
    fn poll(&mut self, _now_ms: u64) -> Option<TickEvent> {
        if self.emitted >= self.limit {
            return None;
        }
        let now = Instant::now();
        if now < self.next_deadline {
            return None;
        }
        self.emitted += 1;
        self.next_deadline = now + self.period;
        self.elapsed_ms = self.elapsed_ms.saturating_add(self.period_ms());
        Some(TickEvent {
            tick: self.emitted,
            now_ms: self.elapsed_ms,
        })
    }
}

#[derive(Default)]
struct HostIpc;

impl IpcDispatcher for HostIpc {
    fn dispatch(&mut self, _now_ms: u64) {}
}

struct WriterAudit<'a, W: Write> {
    writer: &'a mut W,
    buffer: HeaplessVec<u8, 256>,
}

impl<'a, W: Write> WriterAudit<'a, W> {
    fn new(writer: &'a mut W) -> Self {
        Self {
            writer,
            buffer: HeaplessVec::new(),
        }
    }

    fn emit(&mut self, prefix: &str, message: &str) {
        self.buffer.clear();
        let _ = self.buffer.extend_from_slice(prefix.as_bytes());
        let _ = self.buffer.extend_from_slice(message.as_bytes());
        let _ = self.buffer.extend_from_slice(b"\n");
        let _ = self.writer.write_all(&self.buffer);
    }
}

impl<W: Write> AuditSink for WriterAudit<'_, W> {
    fn info(&mut self, message: &str) {
        self.emit("[host] ", message);
    }

    fn denied(&mut self, message: &str) {
        self.emit("[host][denied] ", message);
    }
}

/// Handle allowing scripted input to be injected into the serial driver.
#[derive(Clone)]
struct SerialInjector {
    inner: std::sync::Arc<std::sync::Mutex<HeaplessVec<u8, 1024>>>,
}

impl SerialInjector {
    fn push_rx(&self, bytes: &[u8]) {
        let mut guard = self.inner.lock().expect("serial injector poisoned");
        for &byte in bytes {
            let _ = guard.push(byte);
        }
    }
}

struct HostSerial {
    rx: std::sync::Arc<std::sync::Mutex<HeaplessVec<u8, 1024>>>,
    tx: std::sync::Arc<std::sync::Mutex<HeaplessVec<u8, 1024>>>,
}

impl HostSerial {
    fn new() -> (Self, SerialInjector) {
        let rx = std::sync::Arc::new(std::sync::Mutex::new(HeaplessVec::new()));
        let tx = std::sync::Arc::new(std::sync::Mutex::new(HeaplessVec::new()));
        let injector = SerialInjector { inner: rx.clone() };
        (Self { rx, tx }, injector)
    }

    fn dequeue_rx(&self) -> Option<u8> {
        let mut guard = self.rx.lock().expect("serial rx poisoned");
        if guard.is_empty() {
            None
        } else {
            Some(guard.remove(0))
        }
    }
}

impl embedded_io::ErrorType for HostSerial {
    type Error = SerialError;
}

impl SerialDriver for HostSerial {
    fn read_byte(&mut self) -> nb::Result<u8, SerialError> {
        self.dequeue_rx().ok_or(nb::Error::WouldBlock)
    }

    fn write_byte(&mut self, byte: u8) -> nb::Result<(), SerialError> {
        let mut guard = self.tx.lock().expect("serial tx poisoned");
        if guard.push(byte).is_err() {
            return Err(nb::Error::WouldBlock);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_run_emits_expected_audit_lines() {
        let mut log = Vec::new();
        run_with_writer(&mut log).expect("host run must succeed");
        let transcript = String::from_utf8(log).expect("log must be utf8");
        assert!(transcript.contains("event-pump: init serial"));
        assert!(transcript.contains("console: spawn"));
        assert!(transcript.contains("attach accepted role=Queen"));
        assert!(transcript.contains("console: tail"));
    }
}
