// Author: Lukas Bower

#![cfg(feature = "kernel")]

use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use heapless::{String as HeaplessString, Vec as HeaplessVec};
use nb::Error as NbError;
use root_task::event::BootstrapOp;
use root_task::event::{
    dispatch_message, AuditSink, BootstrapMessage, BootstrapMessageHandler, DispatchOutcome,
    HandlerResult, HandlerTable, IpcDispatcher, TickEvent, TimerSource,
};
use root_task::guards;
use root_task::serial::{SerialDriver, SerialError};
use sel4_sys::seL4_MessageInfo;
use std::cell::RefCell;
use std::vec::Vec;

thread_local! {
    static ACTIVE_HANDLER: RefCell<Option<*mut RecordingHandlers>> = RefCell::new(None);
}

fn with_active<R>(f: impl FnOnce(&mut RecordingHandlers) -> R) -> R {
    ACTIVE_HANDLER.with(|slot| {
        let mut guard = slot.borrow_mut();
        let ptr = guard
            .as_ref()
            .copied()
            .expect("active handler not installed");
        // SAFETY: the pointer is installed with an exclusive reference and cleared
        // immediately after dispatch completes.
        let ctx = unsafe { &mut *ptr };
        f(ctx)
    })
}

fn attach_handler(words: &[sel4_sys::seL4_Word]) -> HandlerResult {
    with_active(|ctx| {
        ctx.attach_calls = ctx.attach_calls.saturating_add(1);
        ctx.record(words);
    });
    Ok(())
}

fn spawn_handler(words: &[sel4_sys::seL4_Word]) -> HandlerResult {
    with_active(|ctx| {
        ctx.spawn_calls = ctx.spawn_calls.saturating_add(1);
        ctx.record(words);
    });
    Ok(())
}

fn log_handler(words: &[sel4_sys::seL4_Word]) -> HandlerResult {
    with_active(|ctx| {
        ctx.log_calls = ctx.log_calls.saturating_add(1);
        ctx.record(words);
    });
    Ok(())
}

fn table() -> HandlerTable {
    HandlerTable::new(attach_handler, spawn_handler, log_handler)
}

fn ensure_text_bounds() {
    static INITIALISED: AtomicBool = AtomicBool::new(false);
    if INITIALISED.load(Ordering::Acquire) {
        return;
    }

    extern "C" {
        #[link_name = "_text"]
        static __text_start: u8;
        #[link_name = "_end"]
        static __text_end: u8;
    }

    let start = ptr::addr_of!(__text_start) as usize;
    let end = ptr::addr_of!(__text_end) as usize;
    guards::init_text_bounds(start, end);
    INITIALISED.store(true, Ordering::Release);
}

pub struct DummySerial;

impl embedded_io::ErrorType for DummySerial {
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

impl BootstrapMessageHandler for RecordingHandlers {
    fn handle(&mut self, message: &BootstrapMessage, audit: &mut dyn AuditSink) {
        let outcome = self.dispatch_payload(message.payload.as_slice());
        let mut summary = HeaplessString::<96>::new();
        let _ = summary.push_str("dispatch outcome=");
        let _ = summary.push_str(match outcome {
            DispatchOutcome::Empty => "empty",
            DispatchOutcome::Handled(BootstrapOp::Attach) => "attach",
            DispatchOutcome::Handled(BootstrapOp::Spawn) => "spawn",
            DispatchOutcome::Handled(BootstrapOp::Log) => "log",
            DispatchOutcome::BadCommand(_) => "bad",
        });
        audit.info(summary.as_str());
        let _ = self.outcomes.push(outcome);
    }
}

impl RecordingHandlers {
    fn dispatch_payload(&mut self, words: &[sel4_sys::seL4_Word]) -> DispatchOutcome {
        ACTIVE_HANDLER.with(|slot| {
            let mut guard = slot.borrow_mut();
            debug_assert!(guard.is_none(), "handler context already active");
            *guard = Some(self as *mut _);
        });
        ensure_text_bounds();
        let outcome = dispatch_message(words, &table());
        ACTIVE_HANDLER.with(|slot| {
            *slot.borrow_mut() = None;
        });
        outcome
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
