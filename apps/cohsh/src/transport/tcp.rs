// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Implement the TCP transport backend for the Cohesix shell console.
// Author: Lukas Bower
//! TCP transport backend for the Cohesix shell console.

use std::collections::VecDeque;
use std::env;
use std::fs::OpenOptions;
use std::io::{self, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use cohesix_ticket::Role;
use cohsh_core::{normalize_ticket, role_label, TicketPolicy};
use fs2::FileExt;
use log::{debug, error, info, trace, warn};
use secure9p_codec::SessionId;

use crate::cat_chunks::reassemble_cat_chunks;
use crate::proto::{parse_ack, AckStatus};
use crate::{CohshRetryPolicy, Session, TcpConnectionInfo, Transport, TransportMetrics};

/// Default TCP timeout applied to socket operations.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);
/// Default heartbeat cadence used to keep TCP sessions alive.
const DEFAULT_HEARTBEAT: Duration = Duration::from_secs(15);
/// Initial retry back-off applied when the connection drops.
const DEFAULT_RETRY_BACKOFF: Duration = Duration::from_millis(200);
/// Maximum retry back-off when reconnecting to the console listener.
const DEFAULT_RETRY_CEILING: Duration = Duration::from_secs(2);
/// Maximum retries when sending commands or recovering after a disconnect.
const DEFAULT_MAX_RETRIES: usize = 3;
/// Empty default requires callers to configure auth explicitly before attach.
const DEFAULT_AUTH_TOKEN: &str = "";
/// Maximum number of acknowledgement lines retained between drains.
const MAX_PENDING_ACK: usize = 32;
const CONSOLE_COMMAND_TRAILING_READ_TIMEOUT: Duration = Duration::from_millis(100);
const CONSOLE_COMMAND_TRAILING_DRAIN_DEADLINE: Duration = Duration::from_millis(600);
const FRAME_ERROR_VERB: &str = "FRAME";
const CONSOLE_LOCK_ENV: &str = "COHSH_CONSOLE_LOCK";
const CONSOLE_LOCK_DISABLE_VALUES: &[&str] = &["0", "false", "off", "no"];
const INSECURE_PLACEHOLDER_TOKEN: &str = concat!("change", "me");
const TCP_BATCH_MAX_WRITE_BYTES: usize = 1_360;
const TCP_BATCH_MAX_FRAMES: usize = 4;

/// Return true when the supplied token is the documented insecure placeholder.
pub fn is_insecure_placeholder_token(token: &str) -> bool {
    token.trim() == INSECURE_PLACEHOLDER_TOKEN
}

/// Return true when verbose TCP debugging is enabled via the environment.
pub fn tcp_debug_enabled() -> bool {
    env::var("COHSH_TCP_DEBUG")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

fn is_frame_error(ack: &crate::proto::Ack<'_>) -> bool {
    matches!(ack.status, AckStatus::Err) && ack.verb.eq_ignore_ascii_case(FRAME_ERROR_VERB)
}

fn ack_path_matches(ack: &crate::proto::Ack<'_>, expected_path: &str) -> bool {
    let Some(detail) = ack.detail else {
        return true;
    };
    for token in detail.split_ascii_whitespace() {
        if let Some(path) = token.strip_prefix("path=") {
            return path == expected_path;
        }
    }
    true
}

#[derive(Debug, Clone)]
struct SessionCache {
    role: Role,
    ticket: Option<String>,
}

#[derive(Debug, Default)]
struct ConnectionTelemetry {
    connects: usize,
    reconnects: usize,
    heartbeats: usize,
}

impl ConnectionTelemetry {
    fn log_connect(&mut self, address: &str, port: u16) {
        self.connects += 1;
        eprintln!(
            "[cohsh][tcp] connected to {address}:{port} (connects={})",
            self.connects
        );
    }

    fn log_reconnect(&mut self, attempt: usize, delay: Duration) {
        self.reconnects += 1;
        eprintln!(
            "[cohsh][tcp] reconnect attempt #{attempt} (delay={}ms, total_reconnects={})",
            delay.as_millis(),
            self.reconnects
        );
    }

    fn log_disconnect(&self, error: &dyn std::error::Error) {
        eprintln!("[cohsh][tcp] connection lost: {error}");
    }

    fn log_heartbeat(&mut self, latency: Duration, verbose: bool) {
        self.heartbeats += 1;
        if !verbose {
            return;
        }
        eprintln!(
            "[cohsh][tcp] heartbeat acknowledged in {:?} (count={})",
            latency, self.heartbeats
        );
    }
}

enum ReadStatus {
    Line(String),
    Timeout,
    Closed,
}

enum HeartbeatOutcome {
    Ack,
    Line(String),
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthState {
    Start,
    Connected,
    AuthSent,
    WaitingAuthOk,
    AuthOk,
    AttachSent,
    WaitingAttachOk,
    Attached,
    Failed,
}

impl AuthState {
    fn log_transition(self, next: Self) {
        trace!("[cohsh][auth] {:?} -> {:?}", self, next);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AckOwned {
    status: AckStatus,
    verb: String,
    detail: Option<String>,
}

impl AckOwned {
    fn into_line(self) -> String {
        let mut line = format!(
            "{} {}",
            match self.status {
                AckStatus::Ok => "OK",
                AckStatus::Err => "ERR",
            },
            self.verb
        );
        if let Some(detail) = self.detail {
            if !detail.is_empty() {
                line.push(' ');
                line.push_str(&detail);
            }
        }
        line
    }
}

#[derive(Debug)]
struct ConsoleLock {
    _file: std::fs::File,
    _path: PathBuf,
}

impl ConsoleLock {
    fn acquire(host: &str, port: u16) -> Result<Option<Self>> {
        if !console_lock_enabled() {
            return Ok(None);
        }
        let path = console_lock_path(host, port);
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("failed to open console lock {}", path.display()))?;
        if let Err(err) = file.try_lock_exclusive() {
            if err.kind() == io::ErrorKind::WouldBlock {
                let owner = read_lock_owner(&path).unwrap_or_else(|| "unknown".to_owned());
                let detail = if owner.is_empty() {
                    String::new()
                } else {
                    format!(" (owner {owner})")
                };
                return Err(anyhow!(
                    "console busy: lock held at {}{}",
                    path.display(),
                    detail
                ));
            }
            return Err(anyhow!(
                "failed to lock console at {}: {err}",
                path.display()
            ));
        }
        let _ = file.set_len(0);
        let _ = writeln!(file, "pid={}", process::id());
        let _ = file.flush();
        Ok(Some(Self {
            _file: file,
            _path: path,
        }))
    }
}

fn console_lock_enabled() -> bool {
    if cfg!(test) {
        return false;
    }
    let Ok(value) = env::var(CONSOLE_LOCK_ENV) else {
        return true;
    };
    let lowered = value.trim().to_ascii_lowercase();
    !CONSOLE_LOCK_DISABLE_VALUES
        .iter()
        .any(|entry| *entry == lowered)
}

fn console_lock_path(host: &str, port: u16) -> PathBuf {
    let mut path = env::temp_dir();
    let host = sanitize_lock_component(host);
    path.push(format!("cohesix-console-{}-{}.lock", host, port));
    path
}

fn sanitize_lock_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn read_lock_owner(path: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Shared TCP transport wrapper that serialises access to a single connection.
#[derive(Clone)]
pub struct SharedTcpTransport {
    inner: Arc<Mutex<TcpTransport>>,
}

impl SharedTcpTransport {
    /// Create a new shared TCP transport wrapper.
    pub fn new(inner: Arc<Mutex<TcpTransport>>) -> Self {
        Self { inner }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TcpTransport> {
        self.inner
            .lock()
            .expect("shared TCP transport lock poisoned")
    }
}

/// Pooled TCP transport wrapper that reuses an attached connection.
#[derive(Clone)]
pub struct PooledTcpTransport {
    inner: Arc<Mutex<TcpTransport>>,
}

impl PooledTcpTransport {
    /// Create a pooled TCP transport wrapper for session pool use.
    pub fn new(inner: Arc<Mutex<TcpTransport>>) -> Self {
        Self { inner }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TcpTransport> {
        self.inner
            .lock()
            .expect("pooled TCP transport lock poisoned")
    }
}

/// TCP transport speaking the root-task console protocol.
#[derive(Debug)]
pub struct TcpTransport {
    address: String,
    port: u16,
    timeout: Duration,
    heartbeat_interval: Duration,
    retry_backoff: Duration,
    retry_ceiling: Duration,
    max_retries: usize,
    auth_token: String,
    allow_insecure_placeholder_token: bool,
    tcp_debug: bool,
    stream: Option<TcpStream>,
    reader: Option<BufReader<TcpStream>>,
    next_session_id: u64,
    last_activity: Instant,
    last_probe: Option<Instant>,
    session_cache: Option<SessionCache>,
    requested_role: Option<Role>,
    telemetry: ConnectionTelemetry,
    pending_ack: VecDeque<AckOwned>,
    rx_buf: Vec<u8>,
    pending_frame_len: Option<usize>,
    pending_timeouts: usize,
    zero_reads: usize,
    authenticated: bool,
    auth_state: AuthState,
    inject_short_write: Option<usize>,
    console_lock: Option<ConsoleLock>,
}

impl TcpTransport {
    fn set_auth_state(&mut self, next: AuthState) {
        if self.auth_state != next {
            info!("[cohsh][auth] state: {:?} -> {:?}", self.auth_state, next);
            self.auth_state.log_transition(next);
            self.auth_state = next;
        }
    }

    /// Create a new transport targeting the provided endpoint.
    pub fn new(address: impl Into<String>, port: u16) -> Self {
        Self {
            address: address.into(),
            port,
            timeout: DEFAULT_TIMEOUT,
            heartbeat_interval: DEFAULT_HEARTBEAT,
            retry_backoff: DEFAULT_RETRY_BACKOFF,
            retry_ceiling: DEFAULT_RETRY_CEILING,
            max_retries: DEFAULT_MAX_RETRIES,
            auth_token: DEFAULT_AUTH_TOKEN.to_owned(),
            allow_insecure_placeholder_token: false,
            tcp_debug: tcp_debug_enabled(),
            stream: None,
            reader: None,
            next_session_id: 2,
            last_activity: Instant::now(),
            last_probe: None,
            session_cache: None,
            requested_role: None,
            telemetry: ConnectionTelemetry::default(),
            pending_ack: VecDeque::new(),
            rx_buf: Vec::new(),
            pending_frame_len: None,
            pending_timeouts: 0,
            zero_reads: 0,
            authenticated: false,
            auth_state: AuthState::Start,
            inject_short_write: None,
            console_lock: None,
        }
    }

    /// Override the socket timeout used for read/write operations.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Override the heartbeat interval used to keep sessions alive.
    #[must_use]
    pub fn with_heartbeat_interval(mut self, interval: Duration) -> Self {
        self.heartbeat_interval = interval;
        self
    }

    /// Override the maximum retry attempts when recovering from disconnects.
    #[must_use]
    pub fn with_max_retries(mut self, attempts: usize) -> Self {
        self.max_retries = attempts.max(1);
        self
    }

    /// Apply retry scheduling policy derived from the manifest.
    #[must_use]
    pub fn with_retry_policy(mut self, policy: CohshRetryPolicy) -> Self {
        self.max_retries = policy.max_attempts as usize;
        self.retry_backoff = Duration::from_millis(policy.backoff_ms);
        self.retry_ceiling = Duration::from_millis(policy.ceiling_ms).max(self.retry_backoff);
        self.timeout = Duration::from_millis(policy.timeout_ms);
        self
    }

    /// Override the authentication token expected by the remote listener.
    #[must_use]
    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = token.into();
        self
    }

    /// Allow the documented placeholder token for local/dev callers.
    #[must_use]
    pub fn allow_insecure_placeholder_token(mut self, allow: bool) -> Self {
        self.allow_insecure_placeholder_token = allow;
        self
    }

    /// Enable or disable verbose TCP handshake logging.
    #[must_use]
    pub fn with_tcp_debug(mut self, enabled: bool) -> Self {
        self.tcp_debug = enabled || tcp_debug_enabled();
        self
    }

    /// Override the retry back-off parameters (base delay and ceiling).
    #[must_use]
    pub fn with_backoff(mut self, base: Duration, ceiling: Duration) -> Self {
        self.retry_backoff = base;
        self.retry_ceiling = ceiling.max(base);
        self
    }

    fn connect(&self) -> Result<TcpStream> {
        let socket_addr = (self.address.as_str(), self.port)
            .to_socket_addrs()
            .context("invalid TCP endpoint")?
            .next()
            .ok_or_else(|| anyhow!("no TCP addresses resolved"))?;
        if self.tcp_debug {
            info!(
                "[cohsh][tcp] connecting to {:?} for role={:?}",
                socket_addr, self.requested_role
            );
        }
        let stream = TcpStream::connect(socket_addr).with_context(|| {
            format!(
                "failed to connect to Cohesix TCP console at {}:{}",
                self.address, self.port
            )
        })?;
        if self.tcp_debug {
            if let Ok(local) = stream.local_addr() {
                info!("[cohsh][tcp] local_addr={:?}", local);
            }
            if let Ok(peer) = stream.peer_addr() {
                info!("[cohsh][tcp] peer_addr={:?}", peer);
            }
        }
        stream
            .set_read_timeout(Some(self.timeout))
            .context("failed to configure read timeout")?;
        stream
            .set_write_timeout(Some(self.timeout))
            .context("failed to configure write timeout")?;
        stream
            .set_nodelay(true)
            .context("failed to enable TCP_NODELAY")?;
        Ok(stream)
    }

    fn connect_with_backoff(&mut self) -> Result<()> {
        let mut attempt = 0usize;
        let mut delay = self.retry_backoff;
        if self.tcp_debug {
            info!(
                "[cohsh][tcp] connecting: addr={:?} role={:?} timeout={:?}",
                (self.address.as_str(), self.port),
                self.requested_role,
                self.timeout
            );
        }
        loop {
            match self.connect() {
                Ok(stream) => {
                    let reader_stream = stream
                        .try_clone()
                        .context("failed to clone TCP stream for reader")?;
                    self.reader = Some(BufReader::new(reader_stream));
                    self.stream = Some(stream);
                    self.authenticated = false;
                    self.reset_read_state();
                    self.last_activity = Instant::now();
                    self.set_auth_state(AuthState::Connected);
                    debug!(
                        "[cohsh][auth] state={:?} TCP connected to {}:{}",
                        self.auth_state, self.address, self.port
                    );
                    self.telemetry.log_connect(&self.address, self.port);
                    return Ok(());
                }
                Err(err) => {
                    if attempt >= self.max_retries {
                        return Err(err);
                    }
                    if self.tcp_debug {
                        error!("[cohsh][tcp] connect failed: {:?}", err);
                    }
                    self.telemetry.log_disconnect(err.as_ref());
                    attempt += 1;
                    self.telemetry.log_reconnect(attempt, delay);
                    thread::sleep(delay);
                    delay = Self::next_delay(delay, self.retry_ceiling);
                }
            }
        }
    }

    fn validate_auth_token(&self) -> Result<()> {
        let token = self.auth_token.trim();
        if token.is_empty() {
            return Err(anyhow!(
                "tcp auth token must be configured with --auth-token or COHSH_AUTH_TOKEN/COH_AUTH_TOKEN"
            ));
        }
        if token == INSECURE_PLACEHOLDER_TOKEN {
            if self.allow_insecure_placeholder_token {
                warn!("tcp auth token uses insecure placeholder token; set a real secret");
                return Ok(());
            }
            return Err(anyhow!(
                "tcp auth token uses insecure placeholder token; set a real secret"
            ));
        }
        Ok(())
    }

    fn perform_auth(&mut self) -> Result<()> {
        let auth_line = format!("AUTH {}", self.auth_token);
        let auth_start = Instant::now();
        let auth_bytes = auth_line.as_bytes();
        let dump_len = auth_bytes.len().min(32);
        info!(
            "[cohsh][auth] sending auth frame payload ({} bytes): {:02x?}",
            auth_bytes.len(),
            &auth_bytes[..dump_len]
        );
        debug!(
            "[cohsh][auth] auth frame bytes (len={}): {:02x?}",
            auth_bytes.len(),
            &auth_bytes[..dump_len]
        );
        if self.tcp_debug {
            info!(
                "[cohsh][tcp] sending auth frame payload ({} bytes): {:02x?}",
                auth_bytes.len(),
                &auth_bytes[..dump_len]
            );
            info!(
                "[cohsh][tcp] auth/handshake struct: magic=\"AUTH\" version=1 role={:?}",
                self.requested_role
            );
            info!(
                "[cohsh][tcp] expecting handshake response: magic=\"OK AUTH\" version=1 role={:?}",
                self.requested_role
            );
        }
        debug!(
            "[cohsh][auth] state={:?} send AUTH token_len={}",
            self.auth_state,
            self.auth_token.len()
        );
        self.set_auth_state(AuthState::AuthSent);
        self.send_line_raw(&auth_line)?;
        self.last_activity = Instant::now();
        self.set_auth_state(AuthState::WaitingAuthOk);
        let mut timeouts = 0usize;
        let mut total_bytes_read = 0usize;
        let total_bytes_written = auth_line.len().saturating_add(4);
        loop {
            if self.tcp_debug {
                info!(
                    "[cohsh][tcp] auth/handshake: waiting for server response (timeout={:?})",
                    self.timeout
                );
            }
            match self.read_line_internal()? {
                ReadStatus::Line(line) => {
                    let bytes = line.as_bytes();
                    let dump_len = bytes.len().min(64);
                    info!(
                        "[cohsh][auth] recv: {} bytes: {:02x?}",
                        bytes.len(),
                        &bytes[..dump_len]
                    );
                    let trimmed = Self::trim_line(&line);
                    total_bytes_read = total_bytes_read.saturating_add(line.len());
                    if let Some(ack) = parse_ack(&trimmed) {
                        if ack.verb.eq_ignore_ascii_case("AUTH") {
                            let _ = self.record_ack(&trimmed);
                            if matches!(ack.status, AckStatus::Err) {
                                self.set_auth_state(AuthState::Failed);
                                debug!(
                                    "[cohsh][auth] state={:?} recv AUTH rejection line={}",
                                    self.auth_state, trimmed
                                );
                                return Err(anyhow!("authentication rejected: {trimmed}"));
                            }
                            if matches!(ack.status, AckStatus::Ok) && ack.detail.is_none() {
                                self.authenticated = true;
                                self.last_activity = Instant::now();
                                self.set_auth_state(AuthState::AuthOk);
                                debug!("[cohsh][auth] state={:?} recv AUTH ok", self.auth_state);
                                return Ok(());
                            }
                            continue;
                        }
                        let _ = self.record_ack(&trimmed);
                        continue;
                    }
                }
                ReadStatus::Timeout => {
                    timeouts += 1;
                    if self.tcp_debug {
                        debug!("[cohsh][tcp] recv: 0 bytes (peer silent)");
                    }
                    if self.tcp_debug {
                        let deadline = self.timeout.saturating_mul(
                            u32::try_from(self.max_retries + 1).unwrap_or(u32::MAX),
                        );
                        info!(
                            "[cohsh][tcp] auth/handshake: timeout waiting for server response after {:?} (attempts={})",
                            deadline,
                            timeouts
                        );
                    }
                    debug!(
                        "[cohsh][auth] state={:?} authentication timeout attempt={}",
                        self.auth_state, timeouts
                    );
                    if timeouts > self.max_retries {
                        self.set_auth_state(AuthState::Failed);
                        warn!(
                            "[cohsh][auth] timeout in state {:?} (total_bytes_read={}, total_bytes_written={})",
                            self.auth_state,
                            total_bytes_read,
                            total_bytes_written,
                        );
                        if self.tcp_debug {
                            error!(
                                "[cohsh][tcp] auth/handshake timeout: state={:?}, bytes_read={}, bytes_written={}, elapsed={:?}",
                                self.auth_state,
                                total_bytes_read,
                                total_bytes_written,
                                auth_start.elapsed(),
                            );
                        }
                        if total_bytes_read > 0 {
                            return Err(anyhow!(
                                "authentication rejected: incomplete AUTH handshake (state={:?}, bytes_read={}, bytes_written={})",
                                self.auth_state,
                                total_bytes_read,
                                total_bytes_written
                            ));
                        }
                        return Err(anyhow!(
                            "authentication timed out waiting for server response (state={:?}, bytes_read={}, bytes_written={})",
                            self.auth_state,
                            total_bytes_read,
                            total_bytes_written
                        ));
                    }
                }
                ReadStatus::Closed => {
                    warn!(
                        "[cohsh][auth] recv error: connection closed (bytes_read={})",
                        total_bytes_read
                    );
                    if self.tcp_debug {
                        warn!(
                            "[cohsh][tcp] auth/handshake: server closed connection (EOF) after reading {} bytes",
                            total_bytes_read
                        );
                    }
                    self.set_auth_state(AuthState::Failed);
                    debug!(
                        "[cohsh][auth] state={:?} connection closed during authentication",
                        self.auth_state
                    );
                    return Err(anyhow!(
                        "authentication rejected: connection closed by server during AUTH handshake (bytes_read={}, bytes_written={})",
                        total_bytes_read,
                        total_bytes_written
                    ));
                }
            }
        }
    }

    fn ensure_authenticated(&mut self) -> Result<()> {
        if self.authenticated && self.stream.is_some() {
            return Ok(());
        }
        self.validate_auth_token()?;

        if self.requested_role.is_none() {
            if let Some(cache) = &self.session_cache {
                self.requested_role = Some(cache.role);
            }
        }

        self.ensure_console_lock()?;

        let mut attempt = 0usize;
        let mut delay = self.retry_backoff;
        loop {
            self.connect_with_backoff()?;
            match self.perform_auth() {
                Ok(()) => return Ok(()),
                Err(err) => {
                    let message = err.to_string();
                    let fatal_auth = message.contains("authentication rejected");
                    self.telemetry.log_disconnect(err.as_ref());
                    self.reset_connection();
                    attempt += 1;
                    if fatal_auth || attempt > self.max_retries {
                        self.set_auth_state(AuthState::Failed);
                        debug!(
                            "[cohsh][auth] state={:?} authentication failed fatal={} attempts={}",
                            self.auth_state, fatal_auth, attempt
                        );
                        return Err(anyhow!("authentication failed: {message}"));
                    }
                    debug!(
                        "[cohsh][auth] state={:?} retrying authentication attempt={}",
                        self.auth_state, attempt
                    );
                    self.telemetry.log_reconnect(attempt, delay);
                    thread::sleep(delay);
                    delay = Self::next_delay(delay, self.retry_ceiling);
                }
            }
        }
    }

    fn reset_connection(&mut self) {
        if let Some(stream) = self.stream.as_ref() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        if let Some(reader) = self.reader.as_ref() {
            let _ = reader.get_ref().shutdown(Shutdown::Both);
        }
        self.stream = None;
        self.reader = None;
        self.last_probe = None;
        self.authenticated = false;
        self.reset_read_state();
        self.set_auth_state(AuthState::Start);
    }

    fn reset_read_state(&mut self) {
        self.rx_buf.clear();
        self.pending_frame_len = None;
        self.pending_timeouts = 0;
        self.zero_reads = 0;
    }

    fn has_partial_frame(&self) -> bool {
        self.pending_frame_len.is_some() || !self.rx_buf.is_empty()
    }

    fn ensure_console_lock(&mut self) -> Result<()> {
        if self.console_lock.is_some() {
            return Ok(());
        }
        self.console_lock = ConsoleLock::acquire(&self.address, self.port)?;
        Ok(())
    }

    fn send_line_raw(&mut self, line: &str) -> Result<(), io::Error> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "TCP transport not connected")
        })?;
        let mut frame = Vec::with_capacity(line.len().saturating_add(4));
        Self::append_frame(&mut frame, line)?;
        if let Some(limit) = self.inject_short_write.take() {
            let to_write = limit.min(frame.len());
            let _ = stream.write(&frame[..to_write]);
            let _ = stream.flush();
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "injected short write",
            ));
        }
        stream.write_all(&frame)?;
        trace!(
            "[cohsh][tcp] wrote {} bytes in state {:?}",
            frame.len(),
            self.auth_state
        );
        stream.flush()
    }

    fn send_lines_raw(&mut self, lines: &[String]) -> Result<(), io::Error> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "TCP transport not connected")
        })?;
        let capacity = Self::batch_frame_bytes(lines)?;
        if lines.len() > TCP_BATCH_MAX_FRAMES
            || (lines.len() > 1 && capacity > TCP_BATCH_MAX_WRITE_BYTES)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TCP command batch exceeds bounded write chunk",
            ));
        }
        let mut frame = Vec::with_capacity(capacity);
        for line in lines {
            Self::append_frame(&mut frame, line)?;
        }
        if let Some(limit) = self.inject_short_write.take() {
            let to_write = limit.min(frame.len());
            let _ = stream.write(&frame[..to_write]);
            let _ = stream.flush();
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "injected short write",
            ));
        }
        stream.write_all(&frame)?;
        trace!(
            "[cohsh][tcp] wrote {} frames ({} bytes) in state {:?}",
            lines.len(),
            frame.len(),
            self.auth_state
        );
        stream.flush()
    }

    fn frame_len(line: &str) -> Result<usize, io::Error> {
        let total_len = line
            .len()
            .checked_add(4)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
        let _: u32 = total_len
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
        Ok(total_len)
    }

    fn batch_frame_bytes(lines: &[String]) -> Result<usize, io::Error> {
        lines.iter().try_fold(0usize, |total, line| {
            let frame_len = Self::frame_len(line)?;
            total
                .checked_add(frame_len)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "batch too large"))
        })
    }

    fn bounded_batch_end(lines: &[String], start: usize) -> Result<usize, io::Error> {
        if start >= lines.len() {
            return Ok(start);
        }
        let mut end = start;
        let mut bytes = 0usize;
        while end < lines.len() && end.saturating_sub(start) < TCP_BATCH_MAX_FRAMES {
            let frame_len = Self::frame_len(&lines[end])?;
            if end > start && bytes.saturating_add(frame_len) > TCP_BATCH_MAX_WRITE_BYTES {
                break;
            }
            bytes = bytes.saturating_add(frame_len);
            end += 1;
        }
        Ok(end.max(start + 1))
    }

    fn append_frame(frame: &mut Vec<u8>, line: &str) -> Result<(), io::Error> {
        let payload = line.as_bytes();
        let len: u32 = Self::frame_len(line)? as u32;
        frame.extend_from_slice(&len.to_le_bytes());
        frame.extend_from_slice(payload);
        Ok(())
    }

    fn send_line(&mut self, line: &str) -> Result<()> {
        let mut attempt = 0usize;
        let mut delay = self.retry_backoff;
        loop {
            self.ensure_authenticated()?;
            match self.send_line_raw(line) {
                Ok(()) => {
                    return Ok(());
                }
                Err(err) => {
                    self.telemetry.log_disconnect(&err);
                    self.reset_connection();
                    attempt += 1;
                    if attempt > self.max_retries {
                        return Err(anyhow!("failed to send command after retries: {err}"));
                    }
                    self.telemetry.log_reconnect(attempt, delay);
                    thread::sleep(delay);
                    delay = Self::next_delay(delay, self.retry_ceiling);
                }
            }
        }
    }

    fn send_line_attached(&mut self, line: &str) -> Result<()> {
        let mut attempt = 0usize;
        let mut delay = self.retry_backoff;
        loop {
            if self.session_cache.is_some() && self.auth_state != AuthState::Attached {
                self.recover_session()?;
            } else {
                self.ensure_authenticated()?;
            }
            match self.send_line_raw(line) {
                Ok(()) => {
                    return Ok(());
                }
                Err(err) => {
                    self.telemetry.log_disconnect(&err);
                    self.reset_connection();
                    attempt += 1;
                    if attempt > self.max_retries {
                        return Err(anyhow!("failed to send command after retries: {err}"));
                    }
                    self.telemetry.log_reconnect(attempt, delay);
                    thread::sleep(delay);
                    delay = Self::next_delay(delay, self.retry_ceiling);
                }
            }
        }
    }

    fn send_lines_attached(&mut self, lines: &[String]) -> Result<()> {
        let mut attempt = 0usize;
        let mut delay = self.retry_backoff;
        loop {
            if self.session_cache.is_some() && self.auth_state != AuthState::Attached {
                self.recover_session()?;
            } else {
                self.ensure_authenticated()?;
            }
            match self.send_lines_raw(lines) {
                Ok(()) => {
                    return Ok(());
                }
                Err(err) => {
                    self.telemetry.log_disconnect(&err);
                    self.reset_connection();
                    if lines.len() > 1 {
                        return Err(anyhow!(
                            "failed to send multi-frame command batch without retrying a potentially partial write: {err}"
                        ));
                    }
                    attempt += 1;
                    if attempt > self.max_retries {
                        return Err(anyhow!("failed to send command batch after retries: {err}"));
                    }
                    self.telemetry.log_reconnect(attempt, delay);
                    thread::sleep(delay);
                    delay = Self::next_delay(delay, self.retry_ceiling);
                }
            }
        }
    }

    fn read_line_internal(&mut self) -> Result<ReadStatus> {
        let reader = self
            .reader
            .as_mut()
            .context("attach to the TCP transport before reading")?;
        let mut saw_data = false;
        let mut temp = [0u8; 512];
        loop {
            if self.pending_frame_len.is_none() {
                if self.rx_buf.len() < 4 {
                    match reader.read(&mut temp) {
                        Ok(0) => {
                            self.zero_reads = self.zero_reads.saturating_add(1);
                            if self.zero_reads > self.max_retries.saturating_mul(4) {
                                self.telemetry.log_disconnect(&io::Error::new(
                                    io::ErrorKind::ConnectionReset,
                                    "connection closed by peer",
                                ));
                                return Ok(ReadStatus::Closed);
                            }
                            break;
                        }
                        Ok(read) => {
                            self.rx_buf.extend_from_slice(&temp[..read]);
                            saw_data = true;
                            self.zero_reads = 0;
                            continue;
                        }
                        Err(err)
                            if matches!(
                                err.kind(),
                                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                            ) =>
                        {
                            self.zero_reads = 0;
                            break;
                        }
                        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
                            self.telemetry.log_disconnect(&io::Error::new(
                                io::ErrorKind::ConnectionReset,
                                "connection closed by peer",
                            ));
                            return Ok(ReadStatus::Closed);
                        }
                        Err(err) => return Err(err.into()),
                    }
                }
                if self.rx_buf.len() >= 4 {
                    let total_len = u32::from_le_bytes([
                        self.rx_buf[0],
                        self.rx_buf[1],
                        self.rx_buf[2],
                        self.rx_buf[3],
                    ]) as usize;
                    if total_len < 4 || total_len > secure9p_codec::MAX_MSIZE as usize {
                        return Err(anyhow!("invalid frame length {total_len}"));
                    }
                    self.pending_frame_len = Some(total_len.saturating_sub(4));
                    self.rx_buf.drain(..4);
                    if total_len == 4 {
                        self.pending_frame_len = None;
                        self.pending_timeouts = 0;
                        self.last_activity = Instant::now();
                        return Ok(ReadStatus::Line(String::new()));
                    }
                }
            }

            if let Some(payload_len) = self.pending_frame_len {
                if self.rx_buf.len() < payload_len {
                    match reader.read(&mut temp) {
                        Ok(0) => {
                            self.zero_reads = self.zero_reads.saturating_add(1);
                            if self.zero_reads > self.max_retries.saturating_mul(4) {
                                self.telemetry.log_disconnect(&io::Error::new(
                                    io::ErrorKind::ConnectionReset,
                                    "connection closed by peer",
                                ));
                                return Ok(ReadStatus::Closed);
                            }
                            break;
                        }
                        Ok(read) => {
                            self.rx_buf.extend_from_slice(&temp[..read]);
                            saw_data = true;
                            self.zero_reads = 0;
                            continue;
                        }
                        Err(err)
                            if matches!(
                                err.kind(),
                                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                            ) =>
                        {
                            self.zero_reads = 0;
                            break;
                        }
                        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
                            self.telemetry.log_disconnect(&io::Error::new(
                                io::ErrorKind::ConnectionReset,
                                "connection closed by peer",
                            ));
                            return Ok(ReadStatus::Closed);
                        }
                        Err(err) => return Err(err.into()),
                    }
                }
                if self.rx_buf.len() >= payload_len {
                    let payload: Vec<u8> = self.rx_buf.drain(..payload_len).collect();
                    self.pending_frame_len = None;
                    self.pending_timeouts = 0;
                    self.last_activity = Instant::now();
                    let line = match String::from_utf8(payload) {
                        Ok(line) => line,
                        Err(err) => {
                            let payload = err.into_bytes();
                            let preview_len = payload.len().min(32);
                            warn!(
                                "[cohsh][tcp] invalid UTF-8 frame len={} first_bytes={:02x?}",
                                payload.len(),
                                &payload[..preview_len]
                            );
                            String::from_utf8_lossy(&payload).into_owned()
                        }
                    };
                    trace!(
                        "[cohsh][tcp] read {} bytes in state {:?}",
                        line.len(),
                        self.auth_state
                    );
                    return Ok(ReadStatus::Line(line));
                }
            }
        }

        if saw_data {
            self.pending_timeouts = 0;
            self.last_activity = Instant::now();
        } else if self.has_partial_frame() {
            self.pending_timeouts = self.pending_timeouts.saturating_add(1);
            if self.pending_timeouts > self.max_retries {
                self.telemetry.log_disconnect(&io::Error::new(
                    io::ErrorKind::TimedOut,
                    "timeout while reading console frame",
                ));
                self.reset_read_state();
                return Ok(ReadStatus::Timeout);
            }
        } else {
            self.pending_timeouts = 0;
        }

        trace!("[cohsh][tcp] read timeout in state {:?}", self.auth_state);
        Ok(ReadStatus::Timeout)
    }

    fn issue_heartbeat(&mut self) -> Result<HeartbeatOutcome> {
        let start = Instant::now();
        self.last_probe = Some(start);
        self.send_line("PING")?;
        let mut timeouts = 0usize;
        loop {
            match self.read_line_internal()? {
                ReadStatus::Line(line) => {
                    let trimmed = Self::trim_line(&line);
                    if trimmed == "PONG" {
                        let latency = self
                            .last_probe
                            .take()
                            .map(|probe| probe.elapsed())
                            .unwrap_or_else(|| start.elapsed());
                        self.telemetry.log_heartbeat(latency, self.tcp_debug);
                        self.last_activity = Instant::now();
                        return Ok(HeartbeatOutcome::Ack);
                    }
                    return Ok(HeartbeatOutcome::Line(trimmed));
                }
                ReadStatus::Timeout => {
                    timeouts = timeouts.saturating_add(1);
                    if timeouts > self.max_retries {
                        self.last_probe = None;
                        return Ok(HeartbeatOutcome::Closed);
                    }
                }
                ReadStatus::Closed => {
                    self.last_probe = None;
                    return Ok(HeartbeatOutcome::Closed);
                }
            }
        }
    }

    fn next_protocol_line(&mut self) -> Result<Option<String>> {
        self.next_protocol_line_with_heartbeat(true)
    }

    fn next_protocol_line_with_heartbeat(
        &mut self,
        allow_heartbeat: bool,
    ) -> Result<Option<String>> {
        if self.reader.is_none() {
            self.ensure_authenticated()?;
        }
        let mut timeouts = 0usize;
        loop {
            match self.read_line_internal()? {
                ReadStatus::Line(line) => {
                    let trimmed = Self::trim_line(&line);
                    if trimmed == "PONG" {
                        let latency = self
                            .last_probe
                            .take()
                            .map(|probe| probe.elapsed())
                            .unwrap_or_default();
                        self.telemetry.log_heartbeat(latency, self.tcp_debug);
                        self.last_activity = Instant::now();
                        continue;
                    }
                    trace!(
                        "[cohsh][auth] state={:?} recv line {}",
                        self.auth_state,
                        trimmed
                    );
                    return Ok(Some(trimmed));
                }
                ReadStatus::Timeout => {
                    if self.has_partial_frame() {
                        continue;
                    }
                    if allow_heartbeat && self.last_activity.elapsed() >= self.heartbeat_interval {
                        match self.issue_heartbeat()? {
                            HeartbeatOutcome::Ack => continue,
                            HeartbeatOutcome::Line(line) => return Ok(Some(line)),
                            HeartbeatOutcome::Closed => return Ok(None),
                        }
                    }
                    if !allow_heartbeat {
                        timeouts = timeouts.saturating_add(1);
                        if timeouts > self.max_retries {
                            return Ok(None);
                        }
                    }
                }
                ReadStatus::Closed => return Ok(None),
            }
        }
    }

    fn next_protocol_line_with_deadline(
        &mut self,
        deadline: Instant,
        allow_heartbeat: bool,
    ) -> Result<Option<String>> {
        if self.reader.is_none() {
            self.ensure_authenticated()?;
        }
        let mut timeouts = 0usize;
        loop {
            if Instant::now() >= deadline && !self.has_partial_frame() {
                return Err(anyhow!("timeout waiting for console response"));
            }
            match self.read_line_internal()? {
                ReadStatus::Line(line) => {
                    let trimmed = Self::trim_line(&line);
                    if trimmed == "PONG" {
                        let latency = self
                            .last_probe
                            .take()
                            .map(|probe| probe.elapsed())
                            .unwrap_or_default();
                        self.telemetry.log_heartbeat(latency, self.tcp_debug);
                        self.last_activity = Instant::now();
                        continue;
                    }
                    trace!(
                        "[cohsh][auth] state={:?} recv line {}",
                        self.auth_state,
                        trimmed
                    );
                    return Ok(Some(trimmed));
                }
                ReadStatus::Timeout => {
                    if self.has_partial_frame() {
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err(anyhow!("timeout waiting for console response"));
                    }
                    if allow_heartbeat && self.last_activity.elapsed() >= self.heartbeat_interval {
                        match self.issue_heartbeat()? {
                            HeartbeatOutcome::Ack => continue,
                            HeartbeatOutcome::Line(line) => return Ok(Some(line)),
                            HeartbeatOutcome::Closed => return Ok(None),
                        }
                    } else if !allow_heartbeat {
                        timeouts = timeouts.saturating_add(1);
                        if timeouts > self.max_retries {
                            return Ok(None);
                        }
                    }
                }
                ReadStatus::Closed => return Ok(None),
            }
        }
    }

    fn stream_wait(&self) -> Duration {
        self.timeout
    }

    fn stream_deadline(&self) -> Instant {
        let wait = self.stream_wait();
        Instant::now()
            .checked_add(wait)
            .unwrap_or_else(Instant::now)
    }

    fn recover_session(&mut self) -> Result<()> {
        let Some(cache) = self.session_cache.clone() else {
            return Err(anyhow!("TCP session dropped before any attach succeeded"));
        };
        self.requested_role = Some(cache.role);
        self.reset_connection();
        let err = anyhow!("connection closed by peer");
        self.telemetry.log_disconnect(err.as_ref());
        let attach_line = format!(
            "ATTACH {} {}",
            role_label(cache.role),
            cache.ticket.as_deref().unwrap_or("")
        );
        let mut attempt = 0usize;
        let mut delay = self.retry_backoff;
        loop {
            self.set_auth_state(AuthState::AttachSent);
            debug!(
                "[cohsh][auth] state={:?} re-send ATTACH role={:?} ticket_len={}",
                self.auth_state,
                cache.role,
                cache.ticket.as_ref().map(|value| value.len()).unwrap_or(0)
            );
            self.send_line(&attach_line)?;
            self.set_auth_state(AuthState::WaitingAttachOk);
            match self.next_protocol_line()? {
                Some(response) => {
                    let _ = self.record_ack(&response);
                    if response.starts_with("OK") {
                        self.set_auth_state(AuthState::Attached);
                        debug!(
                            "[cohsh][auth] state={:?} re-attach ok response={}",
                            self.auth_state, response
                        );
                        return Ok(());
                    }
                    self.set_auth_state(AuthState::Failed);
                    debug!(
                        "[cohsh][auth] state={:?} re-attach failed response={} attempt={}",
                        self.auth_state, response, attempt
                    );
                    return Err(anyhow!("re-attach failed: {response}"));
                }
                None => {
                    attempt += 1;
                    if attempt > self.max_retries {
                        self.set_auth_state(AuthState::Failed);
                        debug!(
                            "[cohsh][auth] state={:?} unable to re-establish TCP session attempts={}",
                            self.auth_state,
                            attempt
                        );
                        return Err(anyhow!("unable to re-establish TCP session"));
                    }
                    self.reset_connection();
                    debug!(
                        "[cohsh][auth] state={:?} waiting to retry attach attempt={}",
                        self.auth_state, attempt
                    );
                    self.telemetry.log_reconnect(attempt, delay);
                    thread::sleep(delay);
                    delay = Self::next_delay(delay, self.retry_ceiling);
                }
            }
        }
    }

    fn normalise_ticket(role: Role, ticket: Option<&str>) -> Result<Option<String>> {
        let ticket_check = normalize_ticket(role, ticket, TicketPolicy::tcp())
            .map_err(|err| Self::map_ticket_error(role, err))?;
        Ok(ticket_check.ticket.map(str::to_owned))
    }

    fn attach_pooled(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        let ticket_payload = Self::normalise_ticket(role, ticket)?;
        let reuse_attached_authority = self.authenticated
            && self.auth_state == AuthState::Attached
            && self.stream.is_some()
            && self.reader.is_some()
            && self.session_cache.as_ref().is_some_and(|cache| {
                cache.role == role && cache.ticket.as_deref() == ticket_payload.as_deref()
            });

        if reuse_attached_authority {
            let session = Session::new(SessionId::from_raw(self.next_session_id), role);
            self.next_session_id = self.next_session_id.wrapping_add(1);
            return Ok(session);
        }

        Transport::attach(self, role, ticket_payload.as_deref())
    }

    fn map_ticket_error(role: Role, err: cohsh_core::TicketError) -> anyhow::Error {
        match err {
            cohsh_core::TicketError::Missing => {
                anyhow!("role {:?} requires a non-empty ticket payload", role)
            }
            cohsh_core::TicketError::TooLong(max) => {
                anyhow!("ticket payload exceeds {max} bytes")
            }
            cohsh_core::TicketError::Invalid(inner) => {
                anyhow!("ticket is not a valid claims token: {inner}")
            }
            cohsh_core::TicketError::RoleMismatch { expected, found } => anyhow!(
                "ticket role {:?} does not match requested role {:?}",
                found,
                expected
            ),
            cohsh_core::TicketError::MissingSubject => {
                anyhow!("ticket for role {:?} must include a subject identity", role)
            }
        }
    }

    fn build_echo_command(path: &str, payload: &[u8]) -> Result<String> {
        let payload_str = std::str::from_utf8(payload).context("payload must be UTF-8")?;
        let trimmed = payload_str.strip_suffix('\n').unwrap_or(payload_str);
        if trimmed.contains('\n') || trimmed.contains('\r') {
            return Err(anyhow!("echo payload must be a single line"));
        }
        if trimmed.is_empty() {
            Ok(format!("ECHO {path}"))
        } else {
            Ok(format!("ECHO {path} {trimmed}"))
        }
    }

    fn trim_line(line: &str) -> String {
        line.trim_end_matches(['\r', '\n']).to_owned()
    }

    fn stream_command(&mut self, verb: &str, path: &str) -> Result<Vec<String>> {
        let command = format!("{verb} {path}");
        self.stream_command_line(verb, path, &command)
    }

    fn stream_command_line(
        &mut self,
        verb: &str,
        path: &str,
        command: &str,
    ) -> Result<Vec<String>> {
        let mut attempts = 0usize;
        let mut lines = Vec::new();
        let mut summary_line: Option<String> = None;
        loop {
            self.send_line_attached(command)?;
            loop {
                let deadline = self.stream_deadline();
                match self.next_protocol_line_with_deadline(deadline, false) {
                    Ok(Some(response)) => {
                        if let Some(ack) = parse_ack(&response) {
                            if is_frame_error(&ack) {
                                return Err(anyhow!("console frame rejected: {response}"));
                            }
                            if ack.verb.eq_ignore_ascii_case(verb) && ack_path_matches(&ack, path) {
                                let _ = self.record_ack(&response);
                                if matches!(ack.status, AckStatus::Err) {
                                    return Err(anyhow!("{verb} failed: {response}"));
                                }
                                if verb.eq_ignore_ascii_case("CAT") && summary_line.is_none() {
                                    if let Some(detail) = ack.detail {
                                        if let Some(idx) = detail.find("data=") {
                                            let summary = detail[idx + "data=".len()..].trim();
                                            if !summary.is_empty() {
                                                summary_line = Some(summary.to_owned());
                                            }
                                        }
                                    }
                                }
                                continue;
                            }
                            // Ignore acknowledgements unrelated to this stream command.
                            continue;
                        }
                        if response == "END" {
                            if lines.is_empty() {
                                if let Some(summary) = summary_line.take() {
                                    lines.push(summary);
                                }
                            }
                            return if verb.eq_ignore_ascii_case("CAT") {
                                reassemble_cat_chunks(lines)
                            } else {
                                Ok(lines)
                            };
                        }
                        lines.push(response);
                    }
                    Ok(None) => {
                        attempts += 1;
                        if attempts > self.max_retries {
                            return Err(anyhow!(
                                "connection dropped repeatedly while running {verb} {path}"
                            ));
                        }
                        self.recover_session()?;
                        break;
                    }
                    Err(err) => {
                        return Err(err).with_context(|| {
                            format!("timeout waiting for {verb} response on {path}")
                        });
                    }
                }
            }
        }
    }

    fn run_console_command(&mut self, command: &str, expected_ack: &str) -> Result<Vec<String>> {
        let mut attempts = 0usize;
        loop {
            self.send_line_attached(command)?;
            let deadline = self.stream_deadline();
            let mut lines = Vec::new();
            loop {
                match self.next_protocol_line_with_deadline(deadline, false) {
                    Ok(Some(response)) => {
                        if let Some(ack) = parse_ack(&response) {
                            let ack_matches = ack.verb.eq_ignore_ascii_case(expected_ack);
                            if is_frame_error(&ack) {
                                let _ = self.record_ack(&response);
                                return Err(anyhow!("console frame rejected: {response}"));
                            }
                            if ack_matches {
                                let _ = self.record_ack(&response);
                                if matches!(ack.status, AckStatus::Err) {
                                    return Err(anyhow!("{command} failed: {response}"));
                                }
                                if expected_ack.eq_ignore_ascii_case("REBOOT") {
                                    return Ok(lines);
                                }
                                self.drain_console_command_trailing_lines(&mut lines)?;
                                return Ok(lines);
                            }
                            let _ = self.record_ack(&response);
                            continue;
                        }
                        lines.push(response);
                    }
                    Ok(None) => {
                        attempts += 1;
                        if attempts > self.max_retries {
                            return Err(anyhow!(
                                "connection dropped repeatedly while running {command}"
                            ));
                        }
                        self.recover_session()?;
                        break;
                    }
                    Err(err) => {
                        return Err(err)
                            .with_context(|| format!("timeout waiting for {command} response"));
                    }
                }
            }
        }
    }

    fn drain_console_command_trailing_lines(&mut self, lines: &mut Vec<String>) -> Result<()> {
        self.configure_read_timeout(CONSOLE_COMMAND_TRAILING_READ_TIMEOUT)?;
        let deadline = Instant::now()
            .checked_add(CONSOLE_COMMAND_TRAILING_DRAIN_DEADLINE)
            .unwrap_or_else(Instant::now);
        let mut result = Ok(());
        loop {
            match self.next_protocol_line_with_deadline(deadline, false) {
                Ok(Some(response)) => {
                    if let Some(ack) = parse_ack(&response) {
                        let _ = self.record_ack(&response);
                        if is_frame_error(&ack) {
                            result = Err(anyhow!("console frame rejected: {response}"));
                            break;
                        }
                    } else {
                        lines.push(response);
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    if err
                        .to_string()
                        .contains("timeout waiting for console response")
                    {
                        break;
                    }
                    result = Err(err.context("failed while draining console command output"));
                    break;
                }
            }
        }
        let restore_result = self.configure_read_timeout(self.timeout);
        result?;
        restore_result
    }

    fn configure_read_timeout(&mut self, timeout: Duration) -> Result<()> {
        if let Some(stream) = self.stream.as_ref() {
            stream
                .set_read_timeout(Some(timeout))
                .context("failed to configure TCP stream read timeout")?;
        }
        if let Some(reader) = self.reader.as_ref() {
            reader
                .get_ref()
                .set_read_timeout(Some(timeout))
                .context("failed to configure TCP reader timeout")?;
        }
        Ok(())
    }

    fn record_ack(&mut self, line: &str) -> bool {
        let Some(ack) = parse_ack(line) else {
            return false;
        };
        if self.pending_ack.len() >= MAX_PENDING_ACK {
            self.pending_ack.pop_front();
        }
        self.pending_ack.push_back(AckOwned {
            status: ack.status,
            verb: ack.verb.to_owned(),
            detail: ack.detail.map(str::to_owned),
        });
        true
    }

    fn next_delay(current: Duration, ceiling: Duration) -> Duration {
        let doubled = current + current;
        if doubled > ceiling {
            ceiling
        } else {
            doubled
        }
    }

    fn read_quit_event(&mut self, deadline: Instant, phase: &str) -> Result<ReadStatus> {
        loop {
            if Instant::now() >= deadline {
                return Err(anyhow!("timeout waiting for {phase}"));
            }
            match self.read_line_internal()? {
                ReadStatus::Timeout => {
                    if Instant::now() >= deadline {
                        return Err(anyhow!("timeout waiting for {phase}"));
                    }
                }
                status => return Ok(status),
            }
        }
    }
}

impl Transport for TcpTransport {
    fn tcp_endpoint(&self) -> Option<(String, u16)> {
        Some((self.address.clone(), self.port))
    }

    fn tcp_connection_info(&self) -> Option<TcpConnectionInfo> {
        let local_addr = self
            .stream
            .as_ref()
            .and_then(|stream| stream.local_addr().ok())
            .map(|addr| addr.to_string());
        let peer_addr = self
            .stream
            .as_ref()
            .and_then(|stream| stream.peer_addr().ok())
            .map(|addr| addr.to_string());
        Some(TcpConnectionInfo {
            host: self.address.clone(),
            port: self.port,
            connected: self.stream.is_some(),
            local_addr,
            peer_addr,
        })
    }

    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        let ticket_payload = Self::normalise_ticket(role, ticket)?;
        let ticket_len = ticket_payload
            .as_ref()
            .map(|value| value.len())
            .unwrap_or(0);
        self.requested_role = Some(role);
        debug!(
            "[cohsh][auth] new session: role={:?} state={:?} ticket_len={}",
            role, self.auth_state, ticket_len
        );
        let attach_line = format!(
            "ATTACH {} {}",
            role_label(role),
            ticket_payload.as_deref().unwrap_or("")
        );
        let mut attempts = 0usize;
        let mut delay = self.retry_backoff;
        loop {
            self.set_auth_state(AuthState::AttachSent);
            debug!(
                "[cohsh][auth] state={:?} send ATTACH role={:?} ticket_len={}",
                self.auth_state, role, ticket_len
            );
            self.send_line(&attach_line)?;
            self.set_auth_state(AuthState::WaitingAttachOk);

            loop {
                match self.next_protocol_line()? {
                    Some(response) => {
                        let Some(ack) = parse_ack(&response) else {
                            self.set_auth_state(AuthState::Failed);
                            debug!(
                                "[cohsh][auth] state={:?} recv non-ack response={} attempts={}",
                                self.auth_state, response, attempts
                            );
                            return Err(anyhow!("remote attach failed: {response}"));
                        };

                        let _ = self.record_ack(&response);
                        if ack.verb.eq_ignore_ascii_case("AUTH")
                            && matches!(ack.status, AckStatus::Err)
                        {
                            self.set_auth_state(AuthState::Failed);
                            debug!(
                                "[cohsh][auth] state={:?} recv auth failure during attach response={} attempts={}",
                                self.auth_state, response, attempts
                            );
                            return Err(anyhow!("authentication failed: {response}"));
                        }

                        if !ack.verb.eq_ignore_ascii_case("ATTACH") {
                            debug!(
                                "[cohsh][auth] state={:?} ignoring non-attach ack response={}",
                                self.auth_state, response
                            );
                            continue;
                        }

                        if !matches!(ack.status, AckStatus::Ok) {
                            self.set_auth_state(AuthState::Failed);
                            debug!(
                                "[cohsh][auth] state={:?} recv attach error response={} attempts={}",
                                self.auth_state, response, attempts
                            );
                            return Err(anyhow!("remote attach failed: {response}"));
                        }

                        let session = Session::new(SessionId::from_raw(self.next_session_id), role);
                        self.next_session_id = self.next_session_id.wrapping_add(1);
                        self.session_cache = Some(SessionCache {
                            role,
                            ticket: ticket_payload.clone(),
                        });
                        self.set_auth_state(AuthState::Attached);
                        debug!(
                            "[cohsh][auth] state={:?} recv attach ok response={}",
                            self.auth_state, response
                        );
                        eprintln!("[cohsh][tcp] remote NineDoor ready as role {:?}", role);
                        return Ok(session);
                    }
                    None => {
                        attempts += 1;
                        if attempts > self.max_retries {
                            self.set_auth_state(AuthState::Failed);
                            debug!(
                                "[cohsh][auth] state={:?} attach acknowledgement missing attempts={}",
                                self.auth_state, attempts
                            );
                            return Err(anyhow!("unable to receive attach acknowledgement"));
                        }
                        let err = anyhow!("connection closed before attach acknowledgement");
                        self.telemetry.log_disconnect(err.as_ref());
                        self.reset_connection();
                        debug!(
                            "[cohsh][auth] state={:?} connection closed while waiting for attach attempts={}",
                            self.auth_state,
                            attempts
                        );
                        self.telemetry.log_reconnect(attempts, delay);
                        thread::sleep(delay);
                        delay = Self::next_delay(delay, self.retry_ceiling);
                        break;
                    }
                }
            }
        }
    }

    fn kind(&self) -> &'static str {
        "tcp"
    }

    fn ping(&mut self, _session: &Session) -> Result<String> {
        let mut attempts = 0usize;
        let deadline = self.stream_deadline();
        loop {
            self.send_line("PING")?;
            match self.next_protocol_line_with_deadline(deadline, false) {
                Ok(Some(response)) => {
                    if self.record_ack(&response) {
                        if response.starts_with("OK PING") {
                            return Ok("pong".to_owned());
                        }
                        if response.starts_with("ERR PING") {
                            return Ok("err".to_owned());
                        }
                        continue;
                    }
                    if response.eq_ignore_ascii_case("PONG") {
                        continue;
                    }
                }
                Ok(None) => {
                    attempts += 1;
                    if attempts > self.max_retries {
                        return Err(anyhow!("connection dropped repeatedly while awaiting PING"));
                    }
                    self.recover_session()?;
                }
                Err(err) => {
                    attempts += 1;
                    if attempts > self.max_retries {
                        return Err(anyhow!("ping timed out: {err}"));
                    }
                    self.recover_session()?;
                }
            }
        }
    }

    fn tail(&mut self, _session: &Session, path: &str, lines: Option<u16>) -> Result<Vec<String>> {
        let lines = crate::ensure_valid_tail_lines(lines)?;
        let command = match lines {
            Some(lines) => format!("TAIL {path} {lines}"),
            None => format!("TAIL {path}"),
        };
        self.stream_command_line("TAIL", path, &command)
    }

    fn read(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        self.stream_command("CAT", path)
    }

    fn list(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        self.stream_command("LS", path)
    }

    fn write(&mut self, _session: &Session, path: &str, payload: &[u8]) -> Result<()> {
        let command = Self::build_echo_command(path, payload)?;
        let mut attempts = 0usize;
        loop {
            self.send_line_attached(&command)?;
            let deadline = self.stream_deadline();
            loop {
                match self.next_protocol_line_with_deadline(deadline, false) {
                    Ok(Some(response)) => {
                        if let Some(ack) = parse_ack(&response) {
                            let _ = self.record_ack(&response);
                            if is_frame_error(&ack) {
                                return Err(anyhow!("echo failed: {response}"));
                            }
                            if matches!(ack.status, AckStatus::Err)
                                && ack.verb.eq_ignore_ascii_case("PARSE")
                            {
                                return Err(anyhow!("echo failed: {response}"));
                            }
                            if ack.verb.eq_ignore_ascii_case("ECHO") && ack_path_matches(&ack, path)
                            {
                                if matches!(ack.status, AckStatus::Ok) {
                                    return Ok(());
                                }
                                return Err(anyhow!("echo failed: {response}"));
                            }
                            continue;
                        }
                        if response.starts_with("ERR") {
                            return Err(anyhow!("echo failed: {response}"));
                        }
                        // Ignore unsolicited lines from prior streaming commands.
                    }
                    Ok(None) => {
                        attempts += 1;
                        if attempts > self.max_retries {
                            return Err(anyhow!(
                                "connection dropped repeatedly while writing to {path}"
                            ));
                        }
                        self.recover_session()?;
                        break;
                    }
                    Err(err) => {
                        attempts += 1;
                        if attempts > self.max_retries {
                            return Err(err)
                                .with_context(|| format!("timeout waiting for ECHO on {path}"));
                        }
                        self.recover_session()?;
                        break;
                    }
                }
            }
        }
    }

    fn console_command(
        &mut self,
        _session: &Session,
        command: &str,
        expected_ack: &str,
    ) -> Result<Vec<String>> {
        self.run_console_command(command, expected_ack)
    }

    fn write_batch(
        &mut self,
        _session: &Session,
        path: &str,
        payloads: &[Vec<u8>],
    ) -> Result<usize> {
        if payloads.is_empty() {
            return Ok(0);
        }
        let mut commands = Vec::with_capacity(payloads.len());
        for payload in payloads {
            commands.push(Self::build_echo_command(path, payload)?);
        }

        let mut acked = 0usize;
        let mut attempts = 0usize;
        while acked < commands.len() {
            let chunk_end = if self.inject_short_write.is_some() {
                acked.saturating_add(1).min(commands.len())
            } else {
                Self::bounded_batch_end(&commands, acked)
                    .map_err(|err| anyhow!("failed to bound TCP command batch: {err}"))?
            };
            self.send_lines_attached(&commands[acked..chunk_end])?;
            while acked < chunk_end {
                match self.next_protocol_line()? {
                    Some(response) => {
                        if let Some(ack) = parse_ack(&response) {
                            let _ = self.record_ack(&response);
                            if is_frame_error(&ack) {
                                return Err(anyhow!("echo failed: {response}"));
                            }
                            if matches!(ack.status, AckStatus::Err)
                                && ack.verb.eq_ignore_ascii_case("PARSE")
                            {
                                return Err(anyhow!("echo failed: {response}"));
                            }
                            if ack.verb.eq_ignore_ascii_case("ECHO") && ack_path_matches(&ack, path)
                            {
                                if matches!(ack.status, AckStatus::Ok) {
                                    acked = acked.saturating_add(1);
                                    continue;
                                }
                                return Err(anyhow!("echo failed: {response}"));
                            }
                            continue;
                        }
                        if response.starts_with("ERR") {
                            return Err(anyhow!("echo failed: {response}"));
                        }
                    }
                    None => {
                        attempts = attempts.saturating_add(1);
                        if attempts > self.max_retries {
                            return Err(anyhow!(
                                "connection dropped repeatedly while writing to {path}"
                            ));
                        }
                        if chunk_end.saturating_sub(acked) > 1 {
                            self.recover_session()?;
                            return Err(anyhow!(
                                "connection dropped before acknowledging TCP command batch; retry would risk duplicate writes to {path}"
                            ));
                        }
                        self.recover_session()?;
                        break;
                    }
                }
            }
        }
        Ok(acked)
    }

    fn quit(&mut self, _session: &Session) -> Result<()> {
        info!("audit quit.transport.begin");
        let result = (|| {
            if self.auth_state != AuthState::Attached
                || self.session_cache.is_none()
                || self.stream.is_none()
                || self.reader.is_none()
            {
                return Err(anyhow!("TCP QUIT requires one attached connection"));
            }

            let ack_deadline = self.stream_deadline();
            self.send_line_raw("quit")
                .context("failed to send QUIT on the attached connection")?;

            match self.read_quit_event(ack_deadline, "OK QUIT")? {
                ReadStatus::Line(line) => {
                    let line = Self::trim_line(&line);
                    if line != "OK QUIT" {
                        return Err(anyhow!("expected exact OK QUIT, received {line:?}"));
                    }
                    let _ = self.record_ack(&line);
                }
                ReadStatus::Closed => {
                    return Err(anyhow!("connection closed before OK QUIT"));
                }
                ReadStatus::Timeout => {
                    return Err(anyhow!("timeout waiting for OK QUIT"));
                }
            }

            self.stream
                .as_ref()
                .context("attached QUIT stream disappeared before half-close")?
                .shutdown(Shutdown::Write)
                .context("failed to half-close the attached QUIT connection")?;

            let close_deadline = self.stream_deadline();
            match self.read_quit_event(close_deadline, "QUIT peer close")? {
                ReadStatus::Closed if self.has_partial_frame() => Err(anyhow!(
                    "connection closed with a partial frame after OK QUIT"
                )),
                ReadStatus::Closed => Ok(()),
                ReadStatus::Line(line) => Err(anyhow!(
                    "received unexpected frame after OK QUIT: {:?}",
                    Self::trim_line(&line)
                )),
                ReadStatus::Timeout => Err(anyhow!("timeout waiting for QUIT peer close")),
            }
        })();

        self.session_cache = None;
        self.requested_role = None;
        self.reset_connection();
        match &result {
            Ok(()) => info!("audit quit.transport.end reason=peer-eof"),
            Err(err) => info!("audit quit.transport.end reason=error detail={err}"),
        }
        result
    }

    fn drain_acknowledgements(&mut self) -> Vec<String> {
        self.pending_ack
            .drain(..)
            .map(AckOwned::into_line)
            .collect()
    }

    fn metrics(&self) -> TransportMetrics {
        TransportMetrics {
            connects: self.telemetry.connects,
            reconnects: self.telemetry.reconnects,
            heartbeats: self.telemetry.heartbeats,
        }
    }

    fn inject_short_write(&mut self, bytes: usize) -> bool {
        self.inject_short_write = Some(bytes);
        true
    }
}

impl Transport for SharedTcpTransport {
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        let mut inner = self.lock();
        inner.attach(role, ticket)
    }

    fn kind(&self) -> &'static str {
        "tcp"
    }

    fn ping(&mut self, session: &Session) -> Result<String> {
        let mut inner = self.lock();
        inner.ping(session)
    }

    fn tail(&mut self, session: &Session, path: &str, lines: Option<u16>) -> Result<Vec<String>> {
        let mut inner = self.lock();
        inner.tail(session, path, lines)
    }

    fn read(&mut self, session: &Session, path: &str) -> Result<Vec<String>> {
        let mut inner = self.lock();
        inner.read(session, path)
    }

    fn list(&mut self, session: &Session, path: &str) -> Result<Vec<String>> {
        let mut inner = self.lock();
        inner.list(session, path)
    }

    fn write(&mut self, session: &Session, path: &str, payload: &[u8]) -> Result<()> {
        let mut inner = self.lock();
        inner.write(session, path, payload)
    }

    fn console_command(
        &mut self,
        session: &Session,
        command: &str,
        expected_ack: &str,
    ) -> Result<Vec<String>> {
        let mut inner = self.lock();
        inner.console_command(session, command, expected_ack)
    }

    fn write_batch(
        &mut self,
        session: &Session,
        path: &str,
        payloads: &[Vec<u8>],
    ) -> Result<usize> {
        let mut inner = self.lock();
        inner.write_batch(session, path, payloads)
    }

    fn quit(&mut self, session: &Session) -> Result<()> {
        let mut inner = self.lock();
        inner.quit(session)
    }

    fn drain_acknowledgements(&mut self) -> Vec<String> {
        let mut inner = self.lock();
        inner.drain_acknowledgements()
    }

    fn metrics(&self) -> TransportMetrics {
        let inner = self.lock();
        inner.metrics()
    }

    fn inject_short_write(&mut self, bytes: usize) -> bool {
        let mut inner = self.lock();
        inner.inject_short_write(bytes)
    }

    fn tcp_endpoint(&self) -> Option<(String, u16)> {
        let inner = self.lock();
        inner.tcp_endpoint()
    }

    fn tcp_connection_info(&self) -> Option<TcpConnectionInfo> {
        let inner = self.lock();
        inner.tcp_connection_info()
    }
}

impl Transport for PooledTcpTransport {
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        let mut inner = self.lock();
        inner.attach_pooled(role, ticket)
    }

    fn kind(&self) -> &'static str {
        "tcp"
    }

    fn ping(&mut self, session: &Session) -> Result<String> {
        let mut inner = self.lock();
        inner.ping(session)
    }

    fn tail(&mut self, session: &Session, path: &str, lines: Option<u16>) -> Result<Vec<String>> {
        let mut inner = self.lock();
        inner.tail(session, path, lines)
    }

    fn read(&mut self, session: &Session, path: &str) -> Result<Vec<String>> {
        let mut inner = self.lock();
        inner.read(session, path)
    }

    fn list(&mut self, session: &Session, path: &str) -> Result<Vec<String>> {
        let mut inner = self.lock();
        inner.list(session, path)
    }

    fn write(&mut self, session: &Session, path: &str, payload: &[u8]) -> Result<()> {
        let mut inner = self.lock();
        inner.write(session, path, payload)
    }

    fn console_command(
        &mut self,
        session: &Session,
        command: &str,
        expected_ack: &str,
    ) -> Result<Vec<String>> {
        let mut inner = self.lock();
        inner.console_command(session, command, expected_ack)
    }

    fn write_batch(
        &mut self,
        session: &Session,
        path: &str,
        payloads: &[Vec<u8>],
    ) -> Result<usize> {
        let mut inner = self.lock();
        inner.write_batch(session, path, payloads)
    }

    fn quit(&mut self, _session: &Session) -> Result<()> {
        Ok(())
    }

    fn drain_acknowledgements(&mut self) -> Vec<String> {
        let mut inner = self.lock();
        inner.drain_acknowledgements()
    }

    fn metrics(&self) -> TransportMetrics {
        let inner = self.lock();
        inner.metrics()
    }

    fn inject_short_write(&mut self, bytes: usize) -> bool {
        let mut inner = self.lock();
        inner.inject_short_write(bytes)
    }

    fn tcp_endpoint(&self) -> Option<(String, u16)> {
        let inner = self.lock();
        inner.tcp_endpoint()
    }

    fn tcp_connection_info(&self) -> Option<TcpConnectionInfo> {
        let inner = self.lock();
        inner.tcp_connection_info()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use cohesix_proto::REASON_INVALID_TOKEN;
    use cohesix_ticket::{BudgetSpec, MountSpec, TicketClaims, TicketIssuer};
    use sha2::{Digest, Sha256};

    use crate::{PoolKind, SessionPool, TransportFactory};

    const TEST_AUTH_TOKEN: &str = "test-auth-token";

    fn write_frame(stream: &mut TcpStream, line: &str) {
        let total_len = line.len().saturating_add(4) as u32;
        stream.write_all(&total_len.to_le_bytes()).unwrap();
        stream.write_all(line.as_bytes()).unwrap();
    }

    fn read_frame(reader: &mut BufReader<TcpStream>) -> Option<String> {
        let mut len_buf = [0u8; 4];
        if reader.read_exact(&mut len_buf).is_err() {
            return None;
        }
        let total_len = u32::from_le_bytes(len_buf) as usize;
        if total_len < 4 {
            return None;
        }
        let payload_len = total_len - 4;
        let mut payload = vec![0u8; payload_len];
        if reader.read_exact(&mut payload).is_err() {
            return None;
        }
        String::from_utf8(payload).ok()
    }

    fn attached_transport(port: u16, timeout: Duration) -> (TcpTransport, Session) {
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(timeout)).unwrap();
        stream.set_write_timeout(Some(timeout)).unwrap();
        let reader_stream = stream.try_clone().unwrap();
        reader_stream.set_read_timeout(Some(timeout)).unwrap();
        let mut transport = TcpTransport::new("127.0.0.1", port)
            .with_timeout(timeout)
            .with_max_retries(1)
            .with_auth_token(TEST_AUTH_TOKEN);
        transport.stream = Some(stream);
        transport.reader = Some(BufReader::new(reader_stream));
        transport.authenticated = true;
        transport.auth_state = AuthState::Attached;
        transport.requested_role = Some(Role::Queen);
        transport.session_cache = Some(SessionCache {
            role: Role::Queen,
            ticket: None,
        });
        (transport, Session::new(SessionId::from_raw(2), Role::Queen))
    }

    fn write_frame_split(stream: &mut TcpStream, line: &str, delay: Duration) {
        let total_len = line.len().saturating_add(4) as u32;
        stream.write_all(&total_len.to_le_bytes()).unwrap();
        stream.flush().unwrap();
        thread::sleep(delay);
        stream.write_all(line.as_bytes()).unwrap();
        stream.flush().unwrap();
    }

    fn cat_chunk_frames(value: &str, chunk_bytes: usize) -> Vec<String> {
        let digest = hex::encode(Sha256::digest(value.as_bytes()));
        let chunks = value.as_bytes().chunks(chunk_bytes).collect::<Vec<_>>();
        chunks
            .iter()
            .enumerate()
            .map(|(sequence, chunk)| {
                format!(
                    "C1:{sequence:04x}:{:04x}:{digest}:{}",
                    chunks.len(),
                    core::str::from_utf8(chunk).expect("ASCII CAT fixture")
                )
            })
            .collect()
    }

    #[test]
    fn ticket_validation_enforces_worker_requirements() {
        let issuer = TicketIssuer::new("worker-secret");
        let claims = TicketClaims::new(
            Role::WorkerHeartbeat,
            BudgetSpec::default_heartbeat(),
            Some("worker-1".to_string()),
            MountSpec::empty(),
            unix_time_ms(),
        );
        let valid_token = issuer.issue(claims).unwrap().encode().unwrap();
        assert!(TcpTransport::normalise_ticket(Role::Queen, None)
            .unwrap()
            .is_none());
        assert!(TcpTransport::normalise_ticket(Role::Queen, Some(""))
            .unwrap()
            .is_none());
        assert!(TcpTransport::normalise_ticket(Role::WorkerHeartbeat, None).is_err());
        assert!(TcpTransport::normalise_ticket(Role::WorkerGpu, Some("  ")).is_err());
        assert!(
            TcpTransport::normalise_ticket(Role::WorkerHeartbeat, Some(valid_token.as_str()),)
                .is_ok()
        );
    }

    #[test]
    fn pooled_tcp_uses_one_wire_attach_for_twenty_four_exact_authority_leases() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let observed = Arc::new(Mutex::new(Vec::<String>::new()));
        let server_observed = Arc::clone(&observed);
        let (stop_tx, stop_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_millis(50)))
                .unwrap();
            write_frame(&mut stream, "OK AUTH detail=present-token");
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                let Some(line) = read_frame(&mut reader) else {
                    continue;
                };
                server_observed.lock().unwrap().push(line.clone());
                let trimmed = line.trim();
                if trimmed == format!("AUTH {TEST_AUTH_TOKEN}") {
                    write_frame(&mut stream, "OK AUTH");
                } else if trimmed.starts_with("ATTACH queen") {
                    write_frame(&mut stream, "OK ATTACH role=queen");
                } else if trimmed == "PING" {
                    write_frame(&mut stream, "PONG");
                    write_frame(&mut stream, "OK PING reply=pong");
                } else {
                    panic!("unexpected pooled TCP frame: {trimmed}");
                }
            }
        });

        let shared = Arc::new(Mutex::new(
            TcpTransport::new("127.0.0.1", port)
                .with_timeout(Duration::from_millis(250))
                .with_max_retries(1)
                .with_auth_token(TEST_AUTH_TOKEN),
        ));
        let factory_shared = Arc::clone(&shared);
        let factory: Arc<dyn TransportFactory> = Arc::new(move || {
            Ok(
                Box::new(PooledTcpTransport::new(Arc::clone(&factory_shared)))
                    as Box<dyn Transport + Send>,
            )
        });
        let pool = SessionPool::new(1, 24, factory);
        pool.attach(Role::Queen, None).unwrap();

        let mut leases = Vec::new();
        let mut session_ids = HashSet::new();
        for _ in 0..24 {
            let lease = pool.checkout(PoolKind::Telemetry).unwrap();
            assert!(session_ids.insert(lease.session().id().into_raw()));
            leases.push(lease);
        }
        assert_eq!(session_ids.len(), 24);
        match pool.checkout(PoolKind::Telemetry) {
            Ok(_) => panic!("twenty-fifth telemetry checkout must exhaust the pool"),
            Err(error) => assert_eq!(error.to_string(), "session pool exhausted for Telemetry"),
        }

        let ping_session = leases[0].session().clone();
        assert_eq!(
            leases[0].transport_mut().ping(&ping_session).unwrap(),
            "pong"
        );
        let initial_frames = observed.lock().unwrap().clone();
        assert_eq!(
            initial_frames
                .iter()
                .filter(|line| line.starts_with("AUTH "))
                .count(),
            1
        );
        assert_eq!(
            initial_frames
                .iter()
                .filter(|line| line.starts_with("ATTACH "))
                .count(),
            1
        );
        assert_eq!(initial_frames.last().map(String::as_str), Some("PING"));

        let changed_ticket = TicketIssuer::new("changed-queen-authority")
            .issue(TicketClaims::new(
                Role::Queen,
                BudgetSpec::unbounded(),
                None,
                MountSpec::empty(),
                unix_time_ms(),
            ))
            .unwrap()
            .encode()
            .unwrap();
        let mut changed_authority = PooledTcpTransport::new(Arc::clone(&shared));
        changed_authority
            .attach(Role::Queen, Some(changed_ticket.as_str()))
            .unwrap();
        assert_eq!(
            observed
                .lock()
                .unwrap()
                .iter()
                .filter(|line| line.starts_with("ATTACH "))
                .count(),
            2,
            "a changed normalized authority must perform a real ATTACH"
        );

        drop(leases);
        pool.shutdown();
        stop_tx.send(()).unwrap();
        server.join().unwrap();
        assert!(observed
            .lock()
            .unwrap()
            .iter()
            .all(|line| !line.eq_ignore_ascii_case("QUIT")));
    }

    #[test]
    fn stream_wait_excludes_heartbeat_and_retry_window() {
        let transport = TcpTransport::new("127.0.0.1", 31337)
            .with_timeout(Duration::from_millis(250))
            .with_heartbeat_interval(Duration::from_secs(30))
            .with_max_retries(8);
        assert_eq!(transport.stream_wait(), Duration::from_millis(250));
    }

    #[test]
    fn quit_requires_exact_ack_then_half_closes_and_observes_peer_eof() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let command = read_frame(&mut reader).expect("QUIT command");
            write_frame(&mut stream, "OK QUIT");
            let mut trailing = [0u8; 1];
            assert_eq!(
                stream.read(&mut trailing).unwrap(),
                0,
                "client must half-close after reading OK QUIT"
            );
            command
        });

        let (mut transport, session) = attached_transport(port, Duration::from_millis(250));
        transport
            .quit(&session)
            .expect("exact QUIT close handshake");
        assert_eq!(transport.drain_acknowledgements(), vec!["OK QUIT"]);
        assert_eq!(transport.metrics().reconnects, 0);
        assert_eq!(server.join().unwrap(), "quit");
    }

    #[test]
    fn quit_rejects_peer_close_before_ack_without_reconnecting() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            read_frame(&mut reader).expect("QUIT command")
        });

        let (mut transport, session) = attached_transport(port, Duration::from_millis(250));
        let error = transport
            .quit(&session)
            .expect_err("EOF before OK QUIT must fail");
        assert!(error.to_string().contains("closed before OK QUIT"));
        assert_eq!(transport.metrics().reconnects, 0);
        assert_eq!(server.join().unwrap(), "quit");
    }

    #[test]
    fn quit_rejects_timeout_before_ack() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (release_tx, release_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let command = read_frame(&mut reader).expect("QUIT command");
            release_rx.recv().unwrap();
            command
        });

        let (mut transport, session) = attached_transport(port, Duration::from_millis(50));
        let error = transport
            .quit(&session)
            .expect_err("missing OK QUIT must time out");
        assert!(error.to_string().contains("timeout waiting for OK QUIT"));
        assert_eq!(transport.metrics().reconnects, 0);
        release_tx.send(()).unwrap();
        assert_eq!(server.join().unwrap(), "quit");
    }

    #[test]
    fn quit_rejects_post_ack_frame() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let command = read_frame(&mut reader).expect("QUIT command");
            write_frame(&mut stream, "OK QUIT");
            write_frame(&mut stream, "OK PING reply=pong");
            let mut trailing = [0u8; 1];
            assert_eq!(stream.read(&mut trailing).unwrap(), 0);
            command
        });

        let (mut transport, session) = attached_transport(port, Duration::from_millis(250));
        let error = transport
            .quit(&session)
            .expect_err("frame after OK QUIT must fail");
        assert!(error.to_string().contains("unexpected frame after OK QUIT"));
        assert_eq!(transport.metrics().reconnects, 0);
        assert_eq!(server.join().unwrap(), "quit");
    }

    #[test]
    fn quit_rejects_missing_peer_eof_after_exact_ack() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (release_tx, release_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let command = read_frame(&mut reader).expect("QUIT command");
            write_frame(&mut stream, "OK QUIT");
            let mut trailing = [0u8; 1];
            assert_eq!(stream.read(&mut trailing).unwrap(), 0);
            release_rx.recv().unwrap();
            command
        });

        let (mut transport, session) = attached_transport(port, Duration::from_millis(50));
        let error = transport
            .quit(&session)
            .expect_err("missing peer EOF must fail");
        assert!(error
            .to_string()
            .contains("timeout waiting for QUIT peer close"));
        assert_eq!(transport.drain_acknowledgements(), vec!["OK QUIT"]);
        assert_eq!(transport.metrics().reconnects, 0);
        release_tx.send(()).unwrap();
        assert_eq!(server.join().unwrap(), "quit");
    }

    #[test]
    fn batch_sender_writes_back_to_back_frames() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let lines = vec![
            "ECHO /log/queen.log first".to_owned(),
            "ECHO /log/queen.log second".to_owned(),
        ];
        let mut expected = Vec::new();
        for line in &lines {
            TcpTransport::append_frame(&mut expected, line).unwrap();
        }
        let expected_len = expected.len();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut observed = vec![0u8; expected_len];
            stream.read_exact(&mut observed).unwrap();
            observed
        });

        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut transport = TcpTransport::new("127.0.0.1", port);
        transport.stream = Some(stream);
        transport.auth_state = AuthState::Attached;
        transport.send_lines_raw(&lines).unwrap();

        let observed = server.join().unwrap();
        assert_eq!(observed, expected);
    }

    #[test]
    fn cat_transport_transparently_reassembles_bounded_chunk_frames() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let canonical = format!(
            "{{\"schema\":\"host-ticket-result/v2\",\"message\":\"{}\"}}",
            "x".repeat(500)
        );
        let frames = cat_chunk_frames(canonical.as_str(), 176);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            assert_eq!(
                read_frame(&mut reader).as_deref(),
                Some("CAT /host/tickets/status")
            );
            for frame in frames {
                write_frame(&mut stream, frame.as_str());
            }
            write_frame(&mut stream, "END");
        });

        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut transport = TcpTransport::new("127.0.0.1", port);
        transport.stream = Some(stream);
        transport.reader = Some(reader);
        transport.authenticated = true;
        transport.auth_state = AuthState::Attached;
        let output = transport
            .stream_command("CAT", "/host/tickets/status")
            .expect("CAT reassembly");
        assert_eq!(output, vec![canonical]);
        server.join().unwrap();
    }

    #[test]
    fn write_batch_waits_for_acknowledgements_between_bounded_chunks() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let payload_count = TCP_BATCH_MAX_FRAMES + 2;
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            write_frame(&mut stream, "OK AUTH detail=present-token");
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut echoes = 0usize;
            while let Some(line) = read_frame(&mut reader) {
                let trimmed = line.trim();
                if trimmed == format!("AUTH {TEST_AUTH_TOKEN}") {
                    write_frame(&mut stream, "OK AUTH");
                } else if trimmed.starts_with("ATTACH") {
                    write_frame(&mut stream, "OK ATTACH role=queen");
                } else if trimmed.starts_with("ECHO /log/queen.log ") {
                    echoes = echoes.saturating_add(1);
                    if echoes == TCP_BATCH_MAX_FRAMES {
                        reader
                            .get_mut()
                            .set_read_timeout(Some(Duration::from_millis(75)))
                            .unwrap();
                        assert!(
                            read_frame(&mut reader).is_none(),
                            "client sent the next TCP command chunk before ACKs"
                        );
                        reader
                            .get_mut()
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        for _ in 0..TCP_BATCH_MAX_FRAMES {
                            write_frame(&mut stream, "OK ECHO path=/log/queen.log bytes=1");
                        }
                    } else if echoes > TCP_BATCH_MAX_FRAMES {
                        write_frame(&mut stream, "OK ECHO path=/log/queen.log bytes=1");
                        if echoes == payload_count {
                            break;
                        }
                    }
                } else if trimmed == "PING" {
                    write_frame(&mut stream, "PONG");
                    write_frame(&mut stream, "OK PING reply=pong");
                }
            }
            echoes
        });

        let mut transport = TcpTransport::new("127.0.0.1", port)
            .with_timeout(Duration::from_millis(100))
            .with_max_retries(2)
            .with_auth_token(TEST_AUTH_TOKEN);
        let session = transport.attach(Role::Queen, None).unwrap();
        let payloads = (0..payload_count)
            .map(|index| format!("{index}").into_bytes())
            .collect::<Vec<_>>();
        let written = transport
            .write_batch(&session, "/log/queen.log", &payloads)
            .unwrap();
        assert_eq!(written, payload_count);
        assert_eq!(server.join().unwrap(), payload_count);
    }

    #[test]
    fn write_batch_short_write_retry_does_not_duplicate_payloads() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_server = Arc::clone(&received);
        let server = thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                let mut stream = stream.unwrap();
                write_frame(&mut stream, "OK AUTH detail=present-token");
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                while let Some(line) = read_frame(&mut reader) {
                    let trimmed = line.trim();
                    if trimmed == format!("AUTH {TEST_AUTH_TOKEN}") {
                        write_frame(&mut stream, "OK AUTH");
                    } else if trimmed.starts_with("ATTACH") {
                        write_frame(&mut stream, "OK ATTACH role=queen");
                    } else if trimmed.starts_with("ECHO /log/queen.log ") {
                        let payload = trimmed.splitn(3, ' ').nth(2).unwrap_or_default().to_owned();
                        received_server.lock().unwrap().push(payload);
                        write_frame(&mut stream, "OK ECHO path=/log/queen.log bytes=1");
                        if received_server.lock().unwrap().len() == 2 {
                            return;
                        }
                    } else if trimmed == "PING" {
                        write_frame(&mut stream, "PONG");
                        write_frame(&mut stream, "OK PING reply=pong");
                    }
                }
            }
        });

        let mut transport = TcpTransport::new("127.0.0.1", port)
            .with_timeout(Duration::from_millis(100))
            .with_max_retries(3)
            .with_auth_token(TEST_AUTH_TOKEN);
        let session = transport.attach(Role::Queen, None).unwrap();
        assert!(transport.inject_short_write(8));
        let payloads = vec![b"batch-one".to_vec(), b"batch-two".to_vec()];
        let written = transport
            .write_batch(&session, "/log/queen.log", &payloads)
            .unwrap();
        assert_eq!(written, payloads.len());
        let observed = received.lock().unwrap().clone();
        assert_eq!(
            observed,
            vec!["batch-one".to_owned(), "batch-two".to_owned()]
        );
        assert!(transport.metrics().reconnects > 0);
        drop(transport);
        server.join().unwrap();
    }

    fn unix_time_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    #[test]
    fn attaches_and_tails_with_reconnect_and_heartbeat() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let connection_count = Arc::new(AtomicUsize::new(0));
        let connection_barrier = Arc::clone(&connection_count);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                connection_barrier.fetch_add(1, Ordering::SeqCst);
                write_frame(&mut stream, "OK AUTH detail=present-token");
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                while let Some(line) = read_frame(&mut reader) {
                    let trimmed = line.trim();
                    if trimmed.starts_with("AUTH ") {
                        if trimmed == format!("AUTH {TEST_AUTH_TOKEN}") {
                            write_frame(&mut stream, "OK AUTH");
                        } else {
                            write_frame(
                                &mut stream,
                                format!("ERR AUTH reason={REASON_INVALID_TOKEN}").as_str(),
                            );
                            break;
                        }
                    } else if trimmed.starts_with("ATTACH") {
                        write_frame(&mut stream, "OK ATTACH role=queen");
                    } else if trimmed.starts_with("TAIL") {
                        write_frame(&mut stream, "OK TAIL path=/log/queen.log");
                        if connection_barrier.load(Ordering::SeqCst) == 1 {
                            write_frame(&mut stream, "line one");
                            stream.flush().unwrap();
                            break;
                        } else {
                            write_frame(&mut stream, "line two");
                            write_frame(&mut stream, "END");
                        }
                    } else if trimmed == "PING" {
                        write_frame(&mut stream, "PONG");
                        write_frame(&mut stream, "OK PING reply=pong");
                    }
                }
            }
        });

        let mut transport = TcpTransport::new("127.0.0.1", port)
            .with_timeout(Duration::from_millis(100))
            .with_heartbeat_interval(Duration::from_millis(50))
            .with_max_retries(4)
            .with_auth_token(TEST_AUTH_TOKEN);
        let session = transport.attach(Role::Queen, None).unwrap();
        let attach_ack = transport.drain_acknowledgements();
        assert!(attach_ack
            .iter()
            .any(|ack| ack.eq_ignore_ascii_case("OK AUTH")));
        assert!(attach_ack
            .iter()
            .any(|ack| ack.starts_with("OK ATTACH role=queen")));
        let logs = transport.tail(&session, "/log/queen.log", None).unwrap();
        assert_eq!(logs, vec!["line one".to_owned(), "line two".to_owned()]);
        let tail_ack = transport.drain_acknowledgements();
        assert!(tail_ack
            .iter()
            .any(|ack| ack.starts_with("OK TAIL path=/log/queen.log")));
        let ping_response = transport.ping(&session).unwrap();
        assert_eq!(ping_response, "pong");
        let ping_ack = transport.drain_acknowledgements();
        assert!(ping_ack
            .iter()
            .any(|ack| ack.starts_with("OK PING reply=pong")));
    }

    #[test]
    fn connection_errors_include_endpoint() {
        let guard = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = guard.local_addr().unwrap().port();
        drop(guard);

        let mut transport = TcpTransport::new("127.0.0.1", port)
            .with_timeout(Duration::from_millis(200))
            .with_max_retries(1)
            .with_auth_token(TEST_AUTH_TOKEN);
        let err = transport
            .attach(Role::Queen, None)
            .expect_err("connection should fail with no listener");
        assert!(err
            .to_string()
            .contains("failed to connect to Cohesix TCP console"));
    }

    #[test]
    fn invalid_auth_triggers_clean_error() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let mut stream = stream.unwrap();
                write_frame(&mut stream, "OK AUTH detail=present-token");
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                while let Some(line) = read_frame(&mut reader) {
                    if line.trim().starts_with("AUTH ") {
                        write_frame(
                            &mut stream,
                            format!("ERR AUTH reason={REASON_INVALID_TOKEN}").as_str(),
                        );
                        break;
                    }
                }
            }
        });

        let mut transport = TcpTransport::new("127.0.0.1", port)
            .with_timeout(Duration::from_millis(100))
            .with_max_retries(1)
            .with_auth_token("wrong");
        let err = transport
            .attach(Role::Queen, None)
            .expect_err("attach should fail on bad auth");
        assert!(err.to_string().contains("authentication failed"));
    }

    #[test]
    fn auth_close_without_ack_is_classified_as_rejection() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let mut stream = stream.unwrap();
                write_frame(&mut stream, "OK AUTH detail=present-token");
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                while let Some(line) = read_frame(&mut reader) {
                    if line.trim().starts_with("AUTH ") {
                        // Simulate a server that terminates authentication immediately
                        // without emitting an ACK frame.
                        let _ = stream.shutdown(Shutdown::Both);
                        break;
                    }
                }
            }
        });

        let mut transport = TcpTransport::new("127.0.0.1", port)
            .with_timeout(Duration::from_millis(100))
            .with_max_retries(1)
            .with_auth_token(TEST_AUTH_TOKEN);
        let err = transport
            .attach(Role::Queen, None)
            .expect_err("attach should fail when auth closes without ack");
        assert!(err.to_string().contains("authentication failed"));
        assert!(err.to_string().contains("authentication rejected"));
    }

    #[test]
    fn missing_auth_token_rejected_before_connect() {
        let mut transport = TcpTransport::new("127.0.0.1", 31337).with_max_retries(1);
        let err = transport
            .attach(Role::Queen, None)
            .expect_err("missing token must fail");
        assert!(err.to_string().contains("must be configured"));
    }

    #[test]
    fn placeholder_auth_token_rejected_before_connect() {
        let mut transport = TcpTransport::new("127.0.0.1", 31337)
            .with_max_retries(1)
            .with_auth_token(INSECURE_PLACEHOLDER_TOKEN);
        let err = transport
            .attach(Role::Queen, None)
            .expect_err("placeholder token must fail");
        assert!(err.to_string().contains("insecure placeholder"));
    }

    #[test]
    fn placeholder_auth_token_allowed_when_explicitly_enabled() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let mut stream = stream.unwrap();
                write_frame(&mut stream, "OK AUTH detail=present-token");
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                while let Some(line) = read_frame(&mut reader) {
                    let trimmed = line.trim();
                    if trimmed == format!("AUTH {INSECURE_PLACEHOLDER_TOKEN}") {
                        write_frame(&mut stream, "OK AUTH");
                    } else if trimmed.starts_with("ATTACH") {
                        write_frame(&mut stream, "OK ATTACH role=queen");
                        write_frame(&mut stream, "END");
                        break;
                    }
                }
            }
        });

        let mut transport = TcpTransport::new("127.0.0.1", port)
            .with_timeout(Duration::from_millis(100))
            .with_max_retries(1)
            .with_auth_token(INSECURE_PLACEHOLDER_TOKEN)
            .allow_insecure_placeholder_token(true);
        let session = transport
            .attach(Role::Queen, None)
            .expect("placeholder token should attach when explicitly allowed");
        assert_eq!(session.role, Role::Queen);
        let attach_ack = transport.drain_acknowledgements();
        assert!(attach_ack
            .iter()
            .any(|ack| ack.eq_ignore_ascii_case("OK AUTH")));
        assert!(attach_ack
            .iter()
            .any(|ack| ack.starts_with("OK ATTACH role=queen")));
    }

    #[test]
    fn partial_frames_survive_timeouts() {
        let listener_result = TcpListener::bind(("127.0.0.1", 0));
        assert!(
            listener_result.is_ok(),
            "listener bind failed: {:?}",
            listener_result.err()
        );
        let Some(listener) = listener_result.ok() else {
            return;
        };
        let local_addr_result = listener.local_addr();
        assert!(
            local_addr_result.is_ok(),
            "listener local_addr failed: {:?}",
            local_addr_result.err()
        );
        let Some(local_addr) = local_addr_result.ok() else {
            return;
        };
        let port = local_addr.port();
        thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let Ok(mut stream) = stream else {
                    return;
                };
                write_frame(&mut stream, "OK AUTH detail=present-token");
                let Ok(cloned) = stream.try_clone() else {
                    return;
                };
                let mut reader = BufReader::new(cloned);
                while let Some(line) = read_frame(&mut reader) {
                    let trimmed = line.trim();
                    if trimmed.starts_with("AUTH ") {
                        write_frame(&mut stream, "OK AUTH");
                    } else if trimmed.starts_with("ATTACH") {
                        write_frame(&mut stream, "OK ATTACH role=queen");
                    } else if trimmed.starts_with("LS ") {
                        write_frame_split(
                            &mut stream,
                            "OK LS path=/proc/tests entries=1",
                            Duration::from_millis(150),
                        );
                        write_frame(&mut stream, "selftest_quick.coh");
                        write_frame(&mut stream, "END");
                        break;
                    } else if trimmed == "PING" {
                        write_frame(&mut stream, "PONG");
                        write_frame(&mut stream, "OK PING reply=pong");
                    }
                }
            }
        });

        let mut transport = TcpTransport::new("127.0.0.1", port)
            .with_timeout(Duration::from_millis(50))
            // The server deliberately crosses multiple socket timeouts. Keep
            // enough retry headroom that scheduler contention cannot turn the
            // partial-frame invariant into a wall-clock race.
            .with_max_retries(8)
            .with_auth_token(TEST_AUTH_TOKEN);
        let session_result = transport.attach(Role::Queen, None);
        assert!(
            session_result.is_ok(),
            "attach failed: {:?}",
            session_result.err()
        );
        let Some(session) = session_result.ok() else {
            return;
        };
        let entries_result = transport.list(&session, "/proc/tests");
        assert!(
            entries_result.is_ok(),
            "list failed: {:?}",
            entries_result.err()
        );
        let Some(entries) = entries_result.ok() else {
            return;
        };
        assert_eq!(entries, vec!["selftest_quick.coh".to_owned()]);
    }

    #[test]
    fn write_ignores_echo_ack_for_other_path() {
        let listener_result = TcpListener::bind(("127.0.0.1", 0));
        assert!(
            listener_result.is_ok(),
            "listener bind failed: {:?}",
            listener_result.err()
        );
        let Some(listener) = listener_result.ok() else {
            return;
        };
        let local_addr_result = listener.local_addr();
        assert!(
            local_addr_result.is_ok(),
            "listener local_addr failed: {:?}",
            local_addr_result.err()
        );
        let Some(local_addr) = local_addr_result.ok() else {
            return;
        };
        let port = local_addr.port();
        thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let Ok(mut stream) = stream else {
                    return;
                };
                write_frame(&mut stream, "OK AUTH detail=present-token");
                let Ok(cloned) = stream.try_clone() else {
                    return;
                };
                let mut reader = BufReader::new(cloned);
                while let Some(line) = read_frame(&mut reader) {
                    let trimmed = line.trim();
                    if trimmed.starts_with("AUTH ") {
                        write_frame(&mut stream, "OK AUTH");
                    } else if trimmed.starts_with("ATTACH") {
                        write_frame(&mut stream, "OK ATTACH role=queen");
                    } else if trimmed.starts_with("ECHO /actions/queue ") {
                        // Emit a stale /queen/ctl ack before the matching /actions/queue ack.
                        write_frame(
                            &mut stream,
                            "ERR ECHO reason=policy detail=denied path=/queen/ctl error=EPERM",
                        );
                        write_frame(&mut stream, "OK ECHO path=/actions/queue bytes=64");
                        break;
                    } else if trimmed == "PING" {
                        write_frame(&mut stream, "PONG");
                        write_frame(&mut stream, "OK PING reply=pong");
                    }
                }
            }
        });

        let mut transport = TcpTransport::new("127.0.0.1", port)
            .with_timeout(Duration::from_millis(100))
            .with_max_retries(3)
            .with_auth_token(TEST_AUTH_TOKEN);
        let session_result = transport.attach(Role::Queen, None);
        assert!(
            session_result.is_ok(),
            "attach failed: {:?}",
            session_result.err()
        );
        let Some(session) = session_result.ok() else {
            return;
        };
        let write_result = transport.write(
            &session,
            "/actions/queue",
            br#"{"id":"a","target":"/queen/ctl","decision":"approve"}"#,
        );
        assert!(
            write_result.is_ok(),
            "write should ignore stale ack from another path: {:?}",
            write_result.err()
        );
    }
}
