// Author: Lukas Bower

#![cfg(all(feature = "kernel", feature = "serial-console"))]

use std::sync::{Mutex, OnceLock};

use root_task::boot::ep::publish_root_ep;
use root_task::ipc::{call_if_valid, send_if_valid, signal_if_valid};
use root_task::sel4;
use root_task::userland;
use sel4_sys::{self, seL4_MessageInfo};

struct CaptureLogger {
    records: Mutex<Vec<String>>,
}

impl CaptureLogger {
    fn init() -> &'static CaptureLogger {
        static LOGGER: OnceLock<&'static CaptureLogger> = OnceLock::new();
        LOGGER.get_or_init(|| {
            let logger = Box::leak(Box::new(CaptureLogger {
                records: Mutex::new(Vec::new()),
            }));
            log::set_max_level(log::LevelFilter::Trace);
            log::set_logger(logger).ok();
            logger
        })
    }

    fn reset(&self) {
        if let Ok(mut guard) = self.records.lock() {
            guard.clear();
        }
    }

    fn snapshot(&self) -> Vec<String> {
        self.records
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

impl log::Log for CaptureLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Trace
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if let Ok(mut guard) = self.records.lock() {
            guard.push(format!("{}", record.args()));
        }
    }

    fn flush(&self) {}
}

#[test]
fn minimal_console_never_uses_null_endpoint() {
    let logger = CaptureLogger::init();
    logger.reset();

    publish_root_ep(sel4_sys::seL4_CapNull);
    assert_eq!(sel4::root_endpoint(), sel4_sys::seL4_CapNull);

    userland::deferred_bringup();

    let msg = seL4_MessageInfo::new(0, 0, 0, 0);
    let reply = call_if_valid(sel4_sys::seL4_CapNull, msg);
    assert_eq!(reply.get_label(), 0);
    send_if_valid(sel4_sys::seL4_CapNull, msg);
    signal_if_valid(sel4_sys::seL4_CapNull);

    let logs = logger.snapshot();
    assert!(
        logs.iter()
            .any(|entry| entry.contains("[bringup] minimal; skipping IPC/queen handshake")),
        "expected minimal bring-up log in {logs:?}"
    );
    assert!(
        logs.iter()
            .any(|entry| entry.contains("[ipc] send skipped: null ep")),
        "expected send guard log in {logs:?}"
    );
    assert!(
        logs.iter()
            .any(|entry| entry.contains("[ipc] call skipped: null ep")),
        "expected call guard log in {logs:?}"
    );
    assert!(
        logs.iter()
            .any(|entry| entry.contains("[ipc] signal skipped: null ep")),
        "expected signal guard log in {logs:?}"
    );
}
