// Author: Lukas Bower
//! Bootstrap logging backend that forwards diagnostics to the seL4 debug console or
//! the IPC endpoint once the NineDoor bridge is attached.
#![allow(dead_code)]

use core::cmp::min;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

use ::log::{Level, LevelFilter, Log, Metadata, Record};
use heapless::{String as HeaplessString, Vec as HeaplessVec};

use crate::event::{AuditSink, BootstrapOp};
use crate::sel4;

#[cfg(feature = "kernel")]
use sel4_sys::{seL4_MessageInfo, seL4_SetMR, seL4_Yield};
#[cfg(feature = "kernel")]
use spin::Mutex;

/// Errors raised when transitioning the bootstrap logger state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Logger has not been initialised with [`init_logger_bootstrap_only`].
    NotInitialised,
    /// Logger already transitioned to the userland sink.
    AlreadyUserland,
}

#[repr(u8)]
enum LoggerState {
    Uninitialised = 0,
    Uart = 1,
    Pending = 2,
    Userland = 3,
}

const FRAME_KIND_LINE: u8 = 0x01;
const FRAME_KIND_PING: u8 = 0x02;
const MAX_FRAME_LEN: usize = 192;
const PING_RETRIES: usize = 4096;

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
            1 => LoggerState::Uart,
            2 => LoggerState::Pending,
            3 => LoggerState::Userland,
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

        let mut formatted: HeaplessString<MAX_FRAME_LEN> = HeaplessString::new();
        let _ = write!(
            formatted,
            "[{level} {target}] {message}",
            level = record.level(),
            target = record.target(),
            message = record.args(),
        );

        let mut line = HeaplessVec::<u8, MAX_FRAME_LEN>::new();
        for &byte in formatted.as_bytes() {
            if line.push(byte).is_err() {
                break;
            }
        }
        let _ = line.extend_from_slice(b"\r\n");
        self.emit(line.as_slice());
    }

    fn flush(&self) {}
}

impl BootstrapLogger {
    fn emit(&self, line: &[u8]) {
        match self.sink_state() {
            LoggerState::Uninitialised => {}
            LoggerState::Uart => emit_uart(line),
            LoggerState::Pending => {
                emit_uart(line);
            }
            LoggerState::Userland => {
                if emit_ep(line).is_err() {
                    revert_to_uart(b"[trace] EP log sink stalled; reverting to UART\r\n");
                    emit_uart(line);
                }
            }
        }
    }
}

static LOGGER: BootstrapLogger = BootstrapLogger::new();
static LOGGER_INSTALLED: AtomicBool = AtomicBool::new(false);
static EP_REQUESTED: AtomicBool = AtomicBool::new(false);
static EP_ATTACHED: AtomicBool = AtomicBool::new(false);
static NO_BRIDGE_MODE: AtomicBool = AtomicBool::new(option_env!("NO_BRIDGE") == Some("1"));
static PING_TOKEN: AtomicU32 = AtomicU32::new(1);
static PING_ACK: AtomicU32 = AtomicU32::new(0);

#[cfg(feature = "kernel")]
static SEND_LOCK: Mutex<()> = Mutex::new(());

fn emit_uart(payload: &[u8]) {
    for &byte in payload {
        sel4::debug_put_char(byte as i32);
    }
}

fn emit_ep(payload: &[u8]) -> Result<(), ()> {
    #[cfg(feature = "kernel")]
    {
        if !sel4::ep_ready() {
            return Err(());
        }

        let mut frame = HeaplessVec::<u8, MAX_FRAME_LEN>::new();
        if frame.push(FRAME_KIND_LINE).is_err() {
            return Err(());
        }
        for &byte in payload {
            if frame.push(byte).is_err() {
                break;
            }
        }
        send_frame(frame.as_slice())
    }

    #[cfg(not(feature = "kernel"))]
    {
        let _ = payload;
        Err(())
    }
}

#[cfg(feature = "kernel")]
fn send_frame(payload: &[u8]) -> Result<(), ()> {
    let mut guard = SEND_LOCK.lock();
    let mut words = [0u64; crate::sel4::MSG_MAX_WORDS];
    let mut index = 0usize;

    words[index] = BootstrapOp::Log.encode();
    index += 1;
    words[index] = payload.len() as u64;
    index += 1;

    let mut offset = 0usize;
    while offset < payload.len() && index < words.len() {
        let remain = payload.len() - offset;
        let mut chunk = [0u8; core::mem::size_of::<u64>()];
        let copy_len = min(remain, chunk.len());
        chunk[..copy_len].copy_from_slice(&payload[offset..offset + copy_len]);
        words[index] = u64::from_le_bytes(chunk);
        offset += copy_len;
        index += 1;
    }

    if offset < payload.len() {
        drop(guard);
        return Err(());
    }

    for (slot, word) in words[..index].iter().enumerate() {
        unsafe {
            seL4_SetMR(slot, *word);
        }
    }

    let info = seL4_MessageInfo::new(0, 0, 0, index as u32);
    let result = sel4::send_guarded(info);
    drop(guard);
    result.map_err(|_| ())
}

#[cfg(not(feature = "kernel"))]
fn send_frame(_payload: &[u8]) -> Result<(), ()> {
    Err(())
}

fn run_self_test() -> bool {
    #[cfg(feature = "kernel")]
    {
        if !sel4::ep_ready() {
            return false;
        }

        let token = PING_TOKEN.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        let mut frame = [0u8; 1 + core::mem::size_of::<u32>()];
        frame[0] = FRAME_KIND_PING;
        frame[1..].copy_from_slice(&token.to_le_bytes());
        if send_frame(&frame).is_err() {
            return false;
        }

        for _ in 0..PING_RETRIES {
            if PING_ACK.load(Ordering::Acquire) == token {
                return true;
            }
            unsafe {
                seL4_Yield();
            }
        }
        false
    }

    #[cfg(not(feature = "kernel"))]
    {
        false
    }
}

fn revert_to_uart(reason: &[u8]) {
    LOGGER.set_state(LoggerState::Uart);
    EP_REQUESTED.store(false, Ordering::Release);
    EP_ATTACHED.store(false, Ordering::Release);
    emit_uart(reason);
}

fn complete_transition() {
    if NO_BRIDGE_MODE.load(Ordering::Acquire) {
        revert_to_uart(b"[trace] log bridge disabled; UART only\r\n");
        return;
    }

    if LOGGER.sink_state() != LoggerState::Pending {
        return;
    }

    emit_uart(b"[trace] switching log transport: UART -> EP\r\n");

    if run_self_test() {
        LOGGER.set_state(LoggerState::Userland);
        let _ = emit_ep(b"[trace] EP log sink attached\r\n");
        emit_uart(b"[trace] EP log sink attached\r\n");
    } else {
        revert_to_uart(b"[trace] EP log sink ping timeout; reverting to UART\r\n");
    }
}

/// Installs the bootstrap logger and routes output to the seL4 debug console.
pub fn init_logger_bootstrap_only() {
    if LOGGER_INSTALLED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        ::log::set_logger(&LOGGER).expect("bootstrap logger install must succeed");
    }
    LOGGER.set_state(LoggerState::Uart);
    ::log::set_max_level(LevelFilter::Info);
}

/// Switches the logger sink to the userland channel once IPC is online.
pub fn switch_logger_to_userland() -> Result<(), Error> {
    if NO_BRIDGE_MODE.load(Ordering::Acquire) {
        return Ok(());
    }

    LOGGER
        .state
        .compare_exchange(
            LoggerState::Uart as u8,
            LoggerState::Pending as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .map_err(|observed| match observed {
            0 => Error::NotInitialised,
            2 | 3 => Error::AlreadyUserland,
            _ => Error::NotInitialised,
        })?;
    EP_REQUESTED.store(true, Ordering::Release);
    if EP_ATTACHED.load(Ordering::Acquire) {
        complete_transition();
    }
    Ok(())
}

/// Inform the logger that the NineDoor bridge has completed authentication.
pub fn notify_bridge_attached() {
    EP_ATTACHED.store(true, Ordering::Release);
    if EP_REQUESTED.load(Ordering::Acquire) {
        complete_transition();
    }
}

/// Inform the logger that the bridge is no longer attached.
pub fn notify_bridge_detached() {
    EP_ATTACHED.store(false, Ordering::Release);
    if LOGGER.sink_state() == LoggerState::Userland {
        revert_to_uart(b"[trace] NineDoor detached; returning to UART\r\n");
    }
}

/// Toggle the no-bridge mode, forcing the logger to remain on the UART transport.
pub fn set_no_bridge_mode(enabled: bool) {
    NO_BRIDGE_MODE.store(enabled, Ordering::Release);
    if enabled {
        LOGGER.set_state(LoggerState::Uart);
    }
}

/// Decode an IPC payload emitted by the EP log sink and surface the payload via the audit sink.
#[cfg(feature = "kernel")]
pub fn process_ep_payload(payload: &[sel4_sys::seL4_Word], audit: &mut dyn AuditSink) {
    if payload.is_empty() {
        return;
    }

    if payload[0] != BootstrapOp::Log.encode() {
        return;
    }

    let Some(&len_word) = payload.get(1) else {
        return;
    };
    let expected = min(len_word as usize, MAX_FRAME_LEN);
    let mut bytes = HeaplessVec::<u8, MAX_FRAME_LEN>::new();
    let mut offset = 0usize;

    for word in payload.iter().skip(2) {
        for byte in word.to_le_bytes() {
            if offset >= expected {
                break;
            }
            if bytes.push(byte).is_err() {
                break;
            }
            offset += 1;
        }
        if offset >= expected {
            break;
        }
    }

    if bytes.is_empty() {
        return;
    }

    match bytes[0] {
        FRAME_KIND_LINE => {
            let line = &bytes[1..];
            if let Ok(text) = core::str::from_utf8(line) {
                audit.info(text);
            }
        }
        FRAME_KIND_PING => {
            if bytes.len() >= 1 + core::mem::size_of::<u32>() {
                let mut token_bytes = [0u8; core::mem::size_of::<u32>()];
                token_bytes.copy_from_slice(&bytes[1..1 + core::mem::size_of::<u32>()]);
                let token = u32::from_le_bytes(token_bytes);
                PING_ACK.store(token, Ordering::Release);
            }
        }
        _ => {}
    }
}

#[cfg(not(feature = "kernel"))]
pub fn process_ep_payload(_payload: &[sel4_sys::seL4_Word], _audit: &mut dyn AuditSink) {}
