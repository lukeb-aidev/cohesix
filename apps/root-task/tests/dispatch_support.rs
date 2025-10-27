// Author: Lukas Bower

#![cfg(feature = "kernel")]

use heapless::{String as HeaplessString, Vec as HeaplessVec};
use nb::Error as NbError;
use root_task::event::{
    dispatch_message, AuditSink, BootstrapMessage, BootstrapMessageHandler, DispatchOutcome,
    IpcDispatcher, TickEvent, TimerSource,
};
use root_task::event::{BootstrapHandlers, BootstrapOp};
use root_task::serial::{SerialDriver, SerialError};
use sel4_sys::seL4_MessageInfo;
use std::vec::Vec;

pub struct DummySerial;

impl embedded_io::Io for DummySerial {
    type Error = SerialError;
}

impl SerialDriver for DummySerial {
    fn read_byte(&mut self) -> nb::Result<u8, Self::Error> {
        Err(NbError::WouldBlock)
    }

    fn write_byte(&mut self, _byte: u8) -> nb::Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Default)]
pub struct NoopTimer;

impl TimerSource for NoopTimer {
    fn poll(&mut self, _now_ms: u64) -> Option<TickEvent> {
        None
    }
}

pub struct AuditCapture {
    pub entries: HeaplessVec<HeaplessString<128>, 8>,
}

impl AuditCapture {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: HeaplessVec::new(),
        }
    }
}

impl AuditSink for AuditCapture {
    fn info(&mut self, message: &str) {
        let mut line = HeaplessString::new();
        let _ = line.push_str(message);
        let _ = self.entries.push(line);
    }

    fn denied(&mut self, message: &str) {
        self.info(message);
    }
}

#[derive(Default)]
pub struct RecordingHandlers {
    pub outcomes: HeaplessVec<DispatchOutcome, 4>,
    pub attach_calls: usize,
    pub spawn_calls: usize,
    pub log_calls: usize,
    pub last_payload: HeaplessVec<sel4_sys::seL4_Word, 8>,
}

impl RecordingHandlers {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn record(&mut self, words: &[sel4_sys::seL4_Word]) {
        self.last_payload.clear();
        for &word in words.iter().take(self.last_payload.capacity()) {
            let _ = self.last_payload.push(word);
        }
    }
}

impl BootstrapHandlers for RecordingHandlers {
    fn on_attach(&mut self, words: &[sel4_sys::seL4_Word]) {
        self.attach_calls = self.attach_calls.saturating_add(1);
        self.record(words);
    }

    fn on_spawn(&mut self, words: &[sel4_sys::seL4_Word]) {
        self.spawn_calls = self.spawn_calls.saturating_add(1);
        self.record(words);
    }

    fn on_log(&mut self, words: &[sel4_sys::seL4_Word]) {
        self.log_calls = self.log_calls.saturating_add(1);
        self.record(words);
    }
}

impl BootstrapMessageHandler for RecordingHandlers {
    fn handle(&mut self, message: &BootstrapMessage, audit: &mut dyn AuditSink) {
        let outcome = dispatch_message(message.payload.as_slice(), self);
        let mut summary = HeaplessString::<96>::new();
        let _ = summary.push_str("dispatch outcome=");
        let _ = summary.push_str(match outcome {
            DispatchOutcome::Empty => "empty",
            DispatchOutcome::Handled(BootstrapOp::Attach) => "attach",
            DispatchOutcome::Handled(BootstrapOp::Spawn) => "spawn",
            DispatchOutcome::Handled(BootstrapOp::Log) => "log",
            DispatchOutcome::Unknown(_) => "unknown",
        });
        audit.info(summary.as_str());
        let _ = self.outcomes.push(outcome);
    }
}

pub struct TestDispatcher {
    staged: Option<BootstrapMessage>,
    ready: bool,
}

impl TestDispatcher {
    #[must_use]
    pub fn new(message: BootstrapMessage) -> Self {
        Self {
            staged: Some(message),
            ready: false,
        }
    }
}

impl IpcDispatcher for TestDispatcher {
    fn dispatch(&mut self, _now_ms: u64) {}

    fn handlers_ready(&mut self) {
        self.ready = true;
    }

    fn take_bootstrap_message(&mut self) -> Option<BootstrapMessage> {
        if self.ready {
            self.ready = false;
            self.staged.take()
        } else {
            None
        }
    }
}

pub fn build_message(
    words: &[sel4_sys::seL4_Word],
) -> (BootstrapMessage, Vec<sel4_sys::seL4_Word>) {
    let mut payload = HeaplessVec::new();
    for &word in words {
        payload.push(word).expect("payload respects kernel bound");
    }
    let copy = payload.iter().copied().collect::<Vec<_>>();
    let info = seL4_MessageInfo::new(0x77, 0, 0, payload.len() as u8);
    let message = BootstrapMessage {
        badge: 0xAA55AA55AA55AA55,
        info,
        payload,
    };
    (message, copy)
}
