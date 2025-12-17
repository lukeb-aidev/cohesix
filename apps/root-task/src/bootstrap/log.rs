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
use sel4_sys::{seL4_MessageInfo, seL4_Word};
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LogTransport {
    Uninitialised = 0,
    UartOnly = 1,
    UartMirroredEp = 2,
    EpOnly = 3,
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
            state: AtomicU8::new(LogTransport::Uninitialised as u8),
        }
    }

    fn transport(&self) -> LogTransport {
        match self.state.load(Ordering::Acquire) {
            1 => LogTransport::UartOnly,
            2 => LogTransport::UartMirroredEp,
            3 => LogTransport::EpOnly,
            _ => LogTransport::Uninitialised,
        }
    }

    fn set_transport(&self, transport: LogTransport) {
        let previous = self.state.swap(transport as u8, Ordering::AcqRel);
        if previous != transport as u8 {
            log_transport_marker(transport, latched_ep());
        }
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
        match self.transport() {
            LogTransport::Uninitialised => {}
            LogTransport::UartOnly => emit_uart(line),
            LogTransport::UartMirroredEp => {
                emit_uart(line);
                let _ = emit_ep(line);
            }
            LogTransport::EpOnly => {
                if emit_ep(line).is_err() {
                    record_drop();
                    revert_to_uart(b"[trace] EP log sink stalled; reverting to UART\r\n");
                    emit_uart(line);
                }
            }
        }
    }
}

static LOGGER: BootstrapLogger = BootstrapLogger::new();
static LOGGER_INSTALLED: AtomicBool = AtomicBool::new(false);
static LOGGER_EP: AtomicU32 = AtomicU32::new(0);
static EP_REQUESTED: AtomicBool = AtomicBool::new(false);
static EP_ATTACHED: AtomicBool = AtomicBool::new(false);
static BRIDGE_CREATED: AtomicBool = AtomicBool::new(false);
static EP_ONLY_PERMITTED: AtomicBool = AtomicBool::new(false);
static POST_COMMIT_IPC_UNLOCKED: AtomicBool = AtomicBool::new(false);
static PRECOMMIT_IPC_FORBIDDEN: AtomicU32 = AtomicU32::new(0);
static LOG_DROPS: AtomicU32 = AtomicU32::new(0);
const fn env_flag(value: Option<&'static str>) -> bool {
    match value {
        Some(val) => {
            let bytes = val.as_bytes();
            bytes.len() == 1 && bytes[0] == b'1'
        }
        None => false,
    }
}

const NO_BRIDGE_DEFAULT: bool = env_flag(option_env!("NO_BRIDGE")) || cfg!(feature = "dev-virt");

static NO_BRIDGE_MODE: AtomicBool = AtomicBool::new(NO_BRIDGE_DEFAULT);
static PING_TOKEN: AtomicU32 = AtomicU32::new(1);
static PING_ACK: AtomicU32 = AtomicU32::new(0);

#[cfg(feature = "kernel")]
static SEND_LOCK: Mutex<()> = Mutex::new(());

fn latched_ep() -> sel4_sys::seL4_CPtr {
    LOGGER_EP.load(Ordering::Acquire) as sel4_sys::seL4_CPtr
}

fn log_transport_marker(transport: LogTransport, ep: sel4_sys::seL4_CPtr) {
    let mut line = HeaplessString::<80>::new();
    match transport {
        LogTransport::Uninitialised => return,
        LogTransport::UartOnly => {
            let _ = write!(line, "log.transport=UART_ONLY");
        }
        LogTransport::UartMirroredEp => {
            let _ = write!(line, "log.transport=UART+EP_MIRROR ep=0x{ep:04x}");
        }
        LogTransport::EpOnly => {
            let _ = write!(line, "log.transport=EP_ONLY ep=0x{ep:04x}");
        }
    }
    force_uart_line(line.as_str());
}

fn ep_sink_permitted() -> bool {
    if !POST_COMMIT_IPC_UNLOCKED.load(Ordering::Acquire) {
        return false;
    }
    if !sel4::ipc_send_unlocked() {
        record_precommit_block("ipc-locked");
        return false;
    }
    if !sel4::ep_ready() || !sel4::ep_validated() {
        return false;
    }
    true
}

fn record_precommit_block(reason: &str) {
    let count = PRECOMMIT_IPC_FORBIDDEN
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    if count == 1 {
        let mut line = HeaplessString::<96>::new();
        let _ = write!(line, "[log] precommit_ipc_forbidden reason={reason}");
        force_uart_line(line.as_str());
    }
}

fn maybe_enter_post_commit_transports() {
    if NO_BRIDGE_MODE.load(Ordering::Acquire) {
        return;
    }
    if !EP_REQUESTED.load(Ordering::Acquire) {
        return;
    }
    if !ep_sink_permitted() {
        return;
    }
    if BRIDGE_CREATED.load(Ordering::Acquire) {
        enter_mirrored_transport();
        if EP_ATTACHED.load(Ordering::Acquire) {
            try_enter_ep_only();
        }
    }
}

fn record_drop() {
    LOG_DROPS.fetch_add(1, Ordering::AcqRel);
}

fn emit_uart(payload: &[u8]) {
    for &byte in payload {
        sel4::debug_put_char(byte as i32);
    }
}

/// Emit a UART line regardless of the current logger transport.
///
/// This path deliberately avoids heap allocations, locks, or the `log` crate so
/// it can always make forward progress even if the primary logging backend is
/// stalled or unavailable.
pub fn force_uart_line(line: &str) {
    if line.trim().is_empty() {
        return;
    }

    if line.contains("serial fallback ready") {
        static SERIAL_FALLBACK_EMITTED: AtomicBool = AtomicBool::new(false);

        if SERIAL_FALLBACK_EMITTED.swap(true, Ordering::Relaxed) {
            return;
        }
    }

    emit_uart(line.as_bytes());
    emit_uart(b"\r\n");
}

fn emit_ep(payload: &[u8]) -> Result<(), ()> {
    if !ep_sink_permitted() {
        record_precommit_block("transport-gate");
        return Err(());
    }

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
    let guard = SEND_LOCK.lock();
    let mut words: [seL4_Word; crate::sel4::MSG_MAX_WORDS] = [0; crate::sel4::MSG_MAX_WORDS];
    let mut index = 0usize;

    words[index] = BootstrapOp::Log.encode();
    index += 1;
    words[index] = payload.len() as seL4_Word;
    index += 1;

    let mut offset = 0usize;
    while offset < payload.len() && index < words.len() {
        let remain = payload.len() - offset;
        let mut chunk = [0u8; core::mem::size_of::<seL4_Word>()];
        let copy_len = min(remain, chunk.len());
        chunk[..copy_len].copy_from_slice(&payload[offset..offset + copy_len]);
        words[index] = seL4_Word::from_le_bytes(chunk);
        offset += copy_len;
        index += 1;
    }

    if offset < payload.len() {
        drop(guard);
        record_drop();
        return Err(());
    }

    for (slot, word) in words[..index].iter().enumerate() {
        crate::sel4::set_message_register(slot, *word);
    }

    let info = seL4_MessageInfo::new(0, 0, 0, index as seL4_Word);
    let result = sel4::send_guarded(info);
    drop(guard);
    if result.is_err() {
        record_drop();
    }
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
            crate::sel4::yield_now();
        }
        false
    }

    #[cfg(not(feature = "kernel"))]
    {
        false
    }
}

fn revert_to_uart(reason: &[u8]) {
    LOGGER.set_transport(LogTransport::UartOnly);
    EP_REQUESTED.store(false, Ordering::Release);
    EP_ATTACHED.store(false, Ordering::Release);
    BRIDGE_CREATED.store(false, Ordering::Release);
    EP_ONLY_PERMITTED.store(false, Ordering::Release);
    LOGGER_EP.store(0, Ordering::Release);
    emit_uart(reason);
}

fn enter_mirrored_transport() {
    if NO_BRIDGE_MODE.load(Ordering::Acquire) {
        return;
    }
    if LOGGER.transport() == LogTransport::UartMirroredEp {
        return;
    }
    if !ep_sink_permitted() {
        record_precommit_block("ep-not-ready");
        return;
    }
    let ep = sel4::root_endpoint();
    LOGGER_EP.store(ep as u32, Ordering::Release);
    LOGGER.set_transport(LogTransport::UartMirroredEp);
    emit_uart(b"[trace] log transport: UART mirrored to EP\r\n");
}

fn try_enter_ep_only() {
    if NO_BRIDGE_MODE.load(Ordering::Acquire) {
        return;
    }
    if !EP_ONLY_PERMITTED.load(Ordering::Acquire) {
        return;
    }
    if !sel4::ep_validated() {
        return;
    }
    if !ep_sink_permitted() {
        record_precommit_block("ep-not-permitted");
        return;
    }
    if LOGGER.transport() != LogTransport::UartMirroredEp {
        return;
    }
    if !run_self_test() {
        emit_uart(b"[trace] EP log sink ping timeout; staying mirrored\r\n");
        return;
    }
    let ep = sel4::root_endpoint();
    LOGGER_EP.store(ep as u32, Ordering::Release);
    LOGGER.set_transport(LogTransport::EpOnly);
    let message = b"[trace] EP log sink attached; switching to EPOnly\r\n";
    let _ = emit_ep(message);
    emit_uart(message);
}

/// Installs the bootstrap logger and routes output to the seL4 debug console.
pub fn init_logger_bootstrap_only() {
    if LOGGER_INSTALLED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        if let Err(err) = ::log::set_logger(&LOGGER) {
            let mut line = HeaplessString::<80>::new();
            let _ = write!(line, "[log] install failed: {err:?}");
            force_uart_line(line.as_str());
        }
    }
    LOGGER.set_transport(LogTransport::UartOnly);
    LOGGER_EP.store(0, Ordering::Release);
    EP_REQUESTED.store(false, Ordering::Release);
    EP_ATTACHED.store(false, Ordering::Release);
    BRIDGE_CREATED.store(false, Ordering::Release);
    EP_ONLY_PERMITTED.store(false, Ordering::Release);
    POST_COMMIT_IPC_UNLOCKED.store(false, Ordering::Release);
    ::log::set_max_level(LevelFilter::Info);
}

/// Switches the logger sink to the userland channel once IPC is online.
pub fn switch_logger_to_userland() -> Result<(), Error> {
    if NO_BRIDGE_MODE.load(Ordering::Acquire) {
        return Ok(());
    }
    match LOGGER.transport() {
        LogTransport::Uninitialised => return Err(Error::NotInitialised),
        LogTransport::EpOnly | LogTransport::UartMirroredEp => return Err(Error::AlreadyUserland),
        LogTransport::UartOnly => {}
    }
    EP_REQUESTED.store(true, Ordering::Release);
    maybe_enter_post_commit_transports();
    Ok(())
}

/// Inform the logger that the NineDoor bridge capability has been created.
pub fn notify_bridge_created() {
    if NO_BRIDGE_MODE.load(Ordering::Acquire) {
        return;
    }
    BRIDGE_CREATED.store(true, Ordering::Release);
    maybe_enter_post_commit_transports();
}

/// Inform the logger that the NineDoor bridge has completed authentication.
pub fn notify_bridge_attached() {
    EP_ATTACHED.store(true, Ordering::Release);
    maybe_enter_post_commit_transports();
}

/// Inform the logger that the bridge is no longer attached.
pub fn notify_bridge_detached() {
    EP_ATTACHED.store(false, Ordering::Release);
    if matches!(
        LOGGER.transport(),
        LogTransport::EpOnly | LogTransport::UartMirroredEp
    ) {
        LOGGER_EP.store(0, Ordering::Release);
        LOGGER.set_transport(LogTransport::UartOnly);
        emit_uart(b"[trace] NineDoor detached; returning to UART\r\n");
    }
}

/// Allow the logger transport to switch to EP-only once userland is stable.
pub fn allow_ep_only_transport() {
    if NO_BRIDGE_MODE.load(Ordering::Acquire) {
        return;
    }
    EP_ONLY_PERMITTED.store(true, Ordering::Release);
    if LOGGER.transport() == LogTransport::UartMirroredEp {
        try_enter_ep_only();
    }
}

/// Enable IPC-backed logging once the root endpoint is validated and the boot
/// sequence has committed.
pub fn unlock_post_commit_ipc_logging() {
    POST_COMMIT_IPC_UNLOCKED.store(true, Ordering::Release);
    maybe_enter_post_commit_transports();
}

/// Toggle the no-bridge mode, forcing the logger to remain on the UART transport.
pub fn set_no_bridge_mode(enabled: bool) {
    NO_BRIDGE_MODE.store(enabled, Ordering::Release);
    if enabled {
        LOGGER.set_transport(LogTransport::UartOnly);
        EP_REQUESTED.store(false, Ordering::Release);
        EP_ATTACHED.store(false, Ordering::Release);
        BRIDGE_CREATED.store(false, Ordering::Release);
        EP_ONLY_PERMITTED.store(false, Ordering::Release);
    }
}

/// Returns `true` when the logger has switched exclusively to the EP transport.
pub fn ep_only_active() -> bool {
    matches!(LOGGER.transport(), LogTransport::EpOnly)
}

/// Returns `true` when the bridge transport has been disabled via environment configuration.
pub fn bridge_disabled() -> bool {
    NO_BRIDGE_MODE.load(Ordering::Acquire)
}

/// Returns the number of IPC attempts blocked while IPC logging was forbidden.
pub fn precommit_ipc_forbidden() -> u32 {
    PRECOMMIT_IPC_FORBIDDEN.load(Ordering::Acquire)
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
