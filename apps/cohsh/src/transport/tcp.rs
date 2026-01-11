// Author: Lukas Bower
// Purpose: Implement the TCP transport backend for the Cohesix shell console.
//! TCP transport backend for the Cohesix shell console.

use std::collections::VecDeque;
use std::env;
use std::io::{self, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use cohesix_proto::{role_label as proto_role_label, Role as ProtoRole};
use cohesix_ticket::{Role, TicketToken};
use log::{debug, error, info, trace, warn};
use secure9p_codec::SessionId;

use crate::proto::{parse_ack, AckStatus};
use crate::{Session, Transport};

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
/// Maximum number of acknowledgement lines retained between drains.
const MAX_PENDING_ACK: usize = 32;
const FRAME_ERROR_VERB: &str = "FRAME";

/// Return true when verbose TCP debugging is enabled via the environment.
pub fn tcp_debug_enabled() -> bool {
    env::var("COHSH_TCP_DEBUG")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

fn is_frame_error(ack: &crate::proto::Ack<'_>) -> bool {
    matches!(ack.status, AckStatus::Err) && ack.verb.eq_ignore_ascii_case(FRAME_ERROR_VERB)
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
    authenticated: bool,
    auth_state: AuthState,
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
            auth_token: "changeme".to_owned(),
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
            authenticated: false,
            auth_state: AuthState::Start,
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

    /// Override the authentication token expected by the remote listener.
    #[must_use]
    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = token.into();
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
                    return Err(anyhow!("connection closed during authentication"));
                }
            }
        }
    }

    fn ensure_authenticated(&mut self) -> Result<()> {
        if self.authenticated && self.stream.is_some() {
            return Ok(());
        }

        if self.requested_role.is_none() {
            if let Some(cache) = &self.session_cache {
                self.requested_role = Some(cache.role);
            }
        }

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
        self.stream = None;
        self.reader = None;
        self.last_probe = None;
        self.authenticated = false;
        self.set_auth_state(AuthState::Start);
    }

    fn send_line_raw(&mut self, line: &str) -> Result<(), io::Error> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "TCP transport not connected")
        })?;
        let total_len = line.as_bytes().len().saturating_add(4);
        let len: u32 = total_len
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
        stream.write_all(&len.to_le_bytes())?;
        stream.write_all(line.as_bytes())?;
        trace!(
            "[cohsh][tcp] wrote {} bytes in state {:?}",
            total_len,
            self.auth_state
        );
        stream.flush()
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

    fn read_line_internal(&mut self) -> Result<ReadStatus> {
        let reader = self
            .reader
            .as_mut()
            .context("attach to the TCP transport before reading")?;
        let mut len_buf = [0u8; 4];
        match reader.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                trace!("[cohsh][tcp] read timeout in state {:?}", self.auth_state);
                return Ok(ReadStatus::Timeout);
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
        let total_len = u32::from_le_bytes(len_buf) as usize;
        if total_len < 4 || total_len > secure9p_codec::MAX_MSIZE as usize {
            return Err(anyhow!("invalid frame length {total_len}"));
        }
        let payload_len = total_len.saturating_sub(4);
        let mut payload = vec![0u8; payload_len];
        match reader.read_exact(&mut payload) {
            Ok(()) => {}
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                trace!("[cohsh][tcp] read timeout in state {:?}", self.auth_state);
                return Ok(ReadStatus::Timeout);
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
        let line = String::from_utf8(payload).context("frame payload must be UTF-8")?;
        self.last_activity = Instant::now();
        trace!(
            "[cohsh][tcp] read {} bytes in state {:?}",
            line.len(),
            self.auth_state
        );
        Ok(ReadStatus::Line(line))
    }

    fn issue_heartbeat(&mut self) -> Result<HeartbeatOutcome> {
        let start = Instant::now();
        self.last_probe = Some(start);
        self.send_line("PING")?;
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
                ReadStatus::Timeout => continue,
                ReadStatus::Closed => {
                    self.last_probe = None;
                    return Ok(HeartbeatOutcome::Closed);
                }
            }
        }
    }

    fn next_protocol_line(&mut self) -> Result<Option<String>> {
        if self.reader.is_none() {
            self.ensure_authenticated()?;
        }
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
                    if self.last_activity.elapsed() >= self.heartbeat_interval {
                        match self.issue_heartbeat()? {
                            HeartbeatOutcome::Ack => continue,
                            HeartbeatOutcome::Line(line) => return Ok(Some(line)),
                            HeartbeatOutcome::Closed => return Ok(None),
                        }
                    }
                }
                ReadStatus::Closed => return Ok(None),
            }
        }
    }

    fn next_protocol_line_with_deadline(&mut self, deadline: Instant) -> Result<Option<String>> {
        if self.reader.is_none() {
            self.ensure_authenticated()?;
        }
        loop {
            if Instant::now() >= deadline {
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
                    if Instant::now() >= deadline {
                        return Err(anyhow!("timeout waiting for console response"));
                    }
                    if self.last_activity.elapsed() >= self.heartbeat_interval {
                        match self.issue_heartbeat()? {
                            HeartbeatOutcome::Ack => continue,
                            HeartbeatOutcome::Line(line) => return Ok(Some(line)),
                            HeartbeatOutcome::Closed => return Ok(None),
                        }
                    }
                }
                ReadStatus::Closed => return Ok(None),
            }
        }
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
            Self::role_label(cache.role),
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
        let trimmed = ticket.and_then(|value| {
            let candidate = value.trim();
            if candidate.is_empty() {
                None
            } else {
                Some(candidate.to_owned())
            }
        });
        match role {
            Role::Queen => {
                if let Some(value) = trimmed.as_deref() {
                    TicketToken::decode_unverified(value)
                        .map_err(|err| anyhow!("ticket is not a valid claims token: {err}"))?;
                }
                Ok(trimmed)
            }
            Role::WorkerHeartbeat | Role::WorkerGpu => {
                let value = trimmed.ok_or_else(|| {
                    anyhow!("role {:?} requires a non-empty ticket payload", role)
                })?;
                let claims = TicketToken::decode_unverified(&value)
                    .map_err(|err| anyhow!("ticket is not a valid claims token: {err}"))?;
                if claims.role != role {
                    return Err(anyhow!(
                        "ticket role {:?} does not match requested role {:?}",
                        claims.role,
                        role
                    ));
                }
                if claims.subject.as_deref().is_none() {
                    return Err(anyhow!(
                        "ticket for role {:?} must include a subject identity",
                        role
                    ));
                }
                Ok(Some(value))
            }
        }
    }

    fn proto_role_from_ticket(role: Role) -> ProtoRole {
        match role {
            Role::Queen => ProtoRole::Queen,
            Role::WorkerHeartbeat => ProtoRole::Worker,
            Role::WorkerGpu => ProtoRole::GpuWorker,
        }
    }

    fn role_label(role: Role) -> &'static str {
        proto_role_label(Self::proto_role_from_ticket(role))
    }

    fn trim_line(line: &str) -> String {
        line.trim_end_matches(['\r', '\n']).to_owned()
    }

    fn stream_command(&mut self, verb: &str, path: &str) -> Result<Vec<String>> {
        let command = format!("{verb} {path}");
        let mut attempts = 0usize;
        let mut lines = Vec::new();
        loop {
            self.send_line(&command)?;
            loop {
                match self.next_protocol_line()? {
                    Some(response) => {
                        if let Some(ack) = parse_ack(&response) {
                            let _ = self.record_ack(&response);
                            if is_frame_error(&ack) {
                                return Err(anyhow!("console frame rejected: {response}"));
                            }
                            if ack.verb.eq_ignore_ascii_case(verb)
                                && matches!(ack.status, AckStatus::Err)
                            {
                                return Err(anyhow!("{verb} failed: {response}"));
                            }
                            continue;
                        }
                        if response == "END" {
                            return Ok(lines);
                        }
                        if response.starts_with("ERR") {
                            return Err(anyhow!("{verb} failed: {response}"));
                        }
                        lines.push(response);
                    }
                    None => {
                        attempts += 1;
                        if attempts > self.max_retries {
                            return Err(anyhow!(
                                "connection dropped repeatedly while running {verb} {path}"
                            ));
                        }
                        self.recover_session()?;
                        break;
                    }
                }
            }
        }
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
}

impl Transport for TcpTransport {
    fn tcp_endpoint(&self) -> Option<(String, u16)> {
        Some((self.address.clone(), self.port))
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
            Self::role_label(role),
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
        let wait = self
            .timeout
            .checked_mul(u32::try_from(self.max_retries + 1).unwrap_or(u32::MAX))
            .unwrap_or(self.timeout)
            .saturating_add(self.heartbeat_interval);
        let now = Instant::now();
        let deadline = now.checked_add(wait).unwrap_or(now);
        loop {
            self.send_line("PING")?;
            match self.next_protocol_line_with_deadline(deadline) {
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

    fn tail(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        self.stream_command("TAIL", path)
    }

    fn read(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        self.stream_command("CAT", path)
    }

    fn list(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        self.stream_command("LS", path)
    }

    fn write(&mut self, _session: &Session, path: &str, payload: &[u8]) -> Result<()> {
        let payload_str = std::str::from_utf8(payload).context("payload must be UTF-8")?;
        let trimmed = payload_str.strip_suffix('\n').unwrap_or(payload_str);
        if trimmed.contains('\n') || trimmed.contains('\r') {
            return Err(anyhow!("echo payload must be a single line"));
        }
        let command = if trimmed.is_empty() {
            format!("ECHO {path}")
        } else {
            format!("ECHO {path} {trimmed}")
        };
        let mut attempts = 0usize;
        loop {
            self.send_line(&command)?;
                match self.next_protocol_line()? {
                    Some(response) => {
                        if let Some(ack) = parse_ack(&response) {
                            let _ = self.record_ack(&response);
                            if is_frame_error(&ack) {
                                return Err(anyhow!("echo failed: {response}"));
                            }
                            if ack.verb.eq_ignore_ascii_case("ECHO") {
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
                }
                None => {
                    attempts += 1;
                    if attempts > self.max_retries {
                        return Err(anyhow!(
                            "connection dropped repeatedly while writing to {path}"
                        ));
                    }
                    self.recover_session()?;
                }
            }
        }
    }

    fn quit(&mut self, _session: &Session) -> Result<()> {
        info!("audit quit.transport.begin");
        self.send_line("quit")?;
        let mut timeouts = 0usize;
        loop {
            match self.read_line_internal()? {
                ReadStatus::Line(line) => {
                    let trimmed = Self::trim_line(&line);
                    if let Some(ack) = parse_ack(trimmed.as_str()) {
                        let _ = self.record_ack(trimmed.as_str());
                        if ack.verb.eq_ignore_ascii_case("QUIT") {
                            if matches!(ack.status, AckStatus::Err) {
                                info!("audit quit.transport.end reason=err");
                                return Err(anyhow!("quit rejected: {trimmed}"));
                            }
                            info!("audit quit.transport.end reason=ack");
                            return Ok(());
                        }
                        continue;
                    }
                }
                ReadStatus::Timeout => {
                    timeouts += 1;
                    if timeouts > self.max_retries {
                        self.reset_connection();
                        info!("audit quit.transport.end reason=timeout");
                        return Ok(());
                    }
                }
                ReadStatus::Closed => {
                    self.reset_connection();
                    info!("audit quit.transport.end reason=closed");
                    return Ok(());
                }
            }
        }
    }

    fn drain_acknowledgements(&mut self) -> Vec<String> {
        self.pending_ack
            .drain(..)
            .map(AckOwned::into_line)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::io::Read;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use cohesix_proto::REASON_INVALID_TOKEN;
    use cohesix_ticket::{BudgetSpec, MountSpec, TicketClaims, TicketIssuer};

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

    #[test]
    fn ticket_validation_enforces_worker_requirements() {
        let issuer = TicketIssuer::new("worker-secret");
        let claims = TicketClaims::new(
            Role::WorkerHeartbeat,
            BudgetSpec::default_heartbeat(),
            Some("worker-1".to_string()),
            MountSpec::empty(),
            0,
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
        assert!(TcpTransport::normalise_ticket(
            Role::WorkerHeartbeat,
            Some(valid_token.as_str()),
        )
        .is_ok());
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
                        if trimmed == "AUTH changeme" {
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
            .with_auth_token("changeme");
        let session = transport.attach(Role::Queen, None).unwrap();
        let attach_ack = transport.drain_acknowledgements();
        assert!(attach_ack
            .iter()
            .any(|ack| ack.eq_ignore_ascii_case("OK AUTH")));
        assert!(attach_ack
            .iter()
            .any(|ack| ack.starts_with("OK ATTACH role=queen")));
        let logs = transport.tail(&session, "/log/queen.log").unwrap();
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
            .with_max_retries(1);
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
}
