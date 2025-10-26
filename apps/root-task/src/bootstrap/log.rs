// Author: Lukas Bower
#![allow(dead_code)]

use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use heapless::String as HeaplessString;
use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::sel4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    NotInitialised,
    AlreadyUserland,
}

#[repr(u8)]
enum LoggerState {
    Uninitialised = 0,
    Debug = 1,
    Userland = 2,
}

struct BootstrapLogger {
    state: AtomicU8,
}

impl BootstrapLogger {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(LoggerState::Uninitialised as u8),
        }
    }

    fn sink_state(&self) -> LoggerState {
        match self.state.load(Ordering::Acquire) {
            1 => LoggerState::Debug,
            2 => LoggerState::Userland,
            _ => LoggerState::Uninitialised,
        }
    }

    fn set_state(&self, state: LoggerState) {
        self.state.store(state as u8, Ordering::Release);
    }
}

impl Log for BootstrapLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let mut line: HeaplessString<192> = HeaplessString::new();
        let _ = write!(
            line,
            "[{level} {target}] {message}",
            level = record.level(),
            target = record.target(),
            message = record.args(),
        );

        self.emit(line.as_bytes());
    }

    fn flush(&self) {}
}

impl BootstrapLogger {
    fn emit(&self, bytes: &[u8]) {
        let sink = self.sink_state();
        match sink {
            LoggerState::Debug | LoggerState::Userland => {
                for &byte in bytes {
                    sel4::debug_put_char(byte as i32);
                }
                sel4::debug_put_char(b'\r' as i32);
                sel4::debug_put_char(b'\n' as i32);
            }
            LoggerState::Uninitialised => {}
        }
    }
}

static LOGGER: BootstrapLogger = BootstrapLogger::new();
static LOGGER_INSTALLED: AtomicBool = AtomicBool::new(false);

pub fn init_logger_bootstrap_only() {
    if LOGGER_INSTALLED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        log::set_logger(&LOGGER).expect("bootstrap logger install must succeed");
    }
    LOGGER.set_state(LoggerState::Debug);
    log::set_max_level(LevelFilter::Info);
}

pub fn switch_logger_to_userland() -> Result<(), Error> {
    let prev = LOGGER
        .state
        .compare_exchange(
            LoggerState::Debug as u8,
            LoggerState::Userland as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .map_err(|observed| match observed {
            0 => Error::NotInitialised,
            2 => Error::AlreadyUserland,
            _ => Error::NotInitialised,
        })?;
    let _ = prev;
    Ok(())
}
