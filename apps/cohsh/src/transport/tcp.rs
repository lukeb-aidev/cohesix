// Author: Lukas Bower
//! TCP transport backend for the Cohesix shell console.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use cohesix_ticket::Role;
use log::{debug, trace};
use secure9p_wire::SessionId;

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

    fn log_heartbeat(&mut self, latency: Duration) {
        self.heartbeats += 1;
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
    VersionSent,
    WaitingVersion,
    AuthSent,
    WaitingAuthOk,
    AttachSent,
    WaitingAttachOk,
    Attached,
    Failed,
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
    stream: Option<TcpStream>,
    reader: Option<BufReader<TcpStream>>,
    next_session_id: u64,
    last_activity: Instant,
    last_probe: Option<Instant>,
    session_cache: Option<SessionCache>,
    telemetry: ConnectionTelemetry,
    pending_ack: VecDeque<AckOwned>,
    authenticated: bool,
    auth_state: AuthState,
}

impl TcpTransport {
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
            stream: None,
            reader: None,
            next_session_id: 2,
            last_activity: Instant::now(),
            last_probe: None,
            session_cache: None,
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
        let stream = TcpStream::connect(socket_addr).with_context(|| {
            format!(
                "failed to connect to Cohesix TCP console at {}:{}",
                self.address, self.port
            )
        })?;
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
                    self.auth_state = AuthState::Connected;
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
                    self.telemetry.log_disconnect(err.as_ref());
                    attempt += 1;
                    self.telemetry.log_reconnect(attempt, delay);
                    thread::sleep(delay);
                    delay = Self::next_delay(delay, self.retry_ceiling);
                }
            }
        }
    }

    fn await_auth_ready(&mut self) -> Result<()> {
        self.auth_state = AuthState::WaitingVersion;
        let mut timeouts = 0usize;
        loop {
            match self.read_line_internal()? {
                ReadStatus::Line(line) => {
                    let trimmed = Self::trim_line(&line);
                    if let Some(ack) = parse_ack(&trimmed) {
                        if ack.verb.eq_ignore_ascii_case("AUTH") {
                            self.record_ack(&trimmed);
                            if matches!(ack.status, AckStatus::Err) {
                                self.auth_state = AuthState::Failed;
                                debug!(
                                    "[cohsh][auth] state={:?} auth preface rejected line={}",
                                    self.auth_state, trimmed
                                );
                                return Err(anyhow!(
                                    "authentication rejected during greeting: {trimmed}"
                                ));
                            }
                            self.auth_state = AuthState::AuthRequested;
                            debug!(
                                "[cohsh][auth] state={:?} received auth challenge line={}",
                                self.auth_state, trimmed
                            );
                            return Ok(());
                        }
                        let _ = self.record_ack(&trimmed);
                        continue;
                    }
                }
                ReadStatus::Timeout => {
                    timeouts += 1;
                    debug!(
                        "[cohsh][auth] state={:?} waiting for auth challenge timeout={}",
                        self.auth_state, timeouts
                    );
                    if timeouts > self.max_retries {
                        self.auth_state = AuthState::Failed;
                        return Err(anyhow!(
                            "authentication timed out waiting for server greeting"
                        ));
                    }
                }
                ReadStatus::Closed => {
                    self.auth_state = AuthState::Failed;
                    debug!(
                        "[cohsh][auth] state={:?} connection closed before auth challenge",
                        self.auth_state
                    );
                    return Err(anyhow!(
                        "connection closed before authentication challenge received"
                    ));
                }
            }
        }
    }

    fn perform_auth(&mut self) -> Result<()> {
        self.await_auth_ready()?;
        let auth_line = format!("AUTH {}", self.auth_token);
        debug!(
            "[cohsh][auth] state={:?} send AUTH token_len={}",
            self.auth_state,
            self.auth_token.len()
        );
        self.auth_state = AuthState::AuthSent;
        self.send_line_raw(&auth_line)?;
        self.auth_state = AuthState::WaitingAuthOk;
        let mut timeouts = 0usize;
        loop {
            match self.read_line_internal()? {
                ReadStatus::Line(line) => {
                    let trimmed = Self::trim_line(&line);
                    if let Some(ack) = parse_ack(&trimmed) {
                        if ack.verb.eq_ignore_ascii_case("AUTH")
                            && matches!(ack.status, AckStatus::Ok)
                        {
                            self.record_ack(&trimmed);
                            self.authenticated = true;
                            self.last_activity = Instant::now();
                            self.auth_state = AuthState::AuthOk;
                            debug!("[cohsh][auth] state={:?} recv AUTH ok", self.auth_state);
                            return Ok(());
                        }
                        self.auth_state = AuthState::Failed;
                        debug!(
                            "[cohsh][auth] state={:?} recv AUTH rejection line={}",
                            self.auth_state, trimmed
                        );
                        return Err(anyhow!("authentication rejected: {trimmed}"));
                    }
                }
                ReadStatus::Timeout => {
                    timeouts += 1;
                    debug!(
                        "[cohsh][auth] state={:?} authentication timeout attempt={}",
                        self.auth_state, timeouts
                    );
                    if timeouts > self.max_retries {
                        self.auth_state = AuthState::Failed;
                        return Err(anyhow!("authentication timed out"));
                    }
                }
                ReadStatus::Closed => {
                    self.auth_state = AuthState::Failed;
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
                        self.auth_state = AuthState::Failed;
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
        self.auth_state = AuthState::Start;
    }

    fn send_line_raw(&mut self, line: &str) -> Result<(), io::Error> {
        let stream = self.stream.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "TCP transport not connected")
        })?;
        stream.write_all(line.as_bytes())?;
        stream.write_all(b"\n")?;
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
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                self.telemetry.log_disconnect(&io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "connection closed by peer",
                ));
                Ok(ReadStatus::Closed)
            }
            Ok(_) => {
                self.last_activity = Instant::now();
                Ok(ReadStatus::Line(line))
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                Ok(ReadStatus::Timeout)
            }
            Err(err) => Err(err.into()),
        }
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
                        self.telemetry.log_heartbeat(latency);
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
                        self.telemetry.log_heartbeat(latency);
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

    fn recover_session(&mut self) -> Result<()> {
        let Some(cache) = self.session_cache.clone() else {
            return Err(anyhow!("TCP session dropped before any attach succeeded"));
        };
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
            self.auth_state = AuthState::AttachSent;
            debug!(
                "[cohsh][auth] state={:?} re-send ATTACH role={:?} ticket_len={}",
                self.auth_state,
                cache.role,
                cache.ticket.as_ref().map(|value| value.len()).unwrap_or(0)
            );
            self.send_line(&attach_line)?;
            self.auth_state = AuthState::WaitingAttachOk;
            match self.next_protocol_line()? {
                Some(response) => {
                    let _ = self.record_ack(&response);
                    if response.starts_with("OK") {
                        self.auth_state = AuthState::Attached;
                        debug!(
                            "[cohsh][auth] state={:?} re-attach ok response={}",
                            self.auth_state, response
                        );
                        return Ok(());
                    }
                    self.auth_state = AuthState::Failed;
                    debug!(
                        "[cohsh][auth] state={:?} re-attach failed response={} attempt={}",
                        self.auth_state, response, attempt
                    );
                    return Err(anyhow!("re-attach failed: {response}"));
                }
                None => {
                    attempt += 1;
                    if attempt > self.max_retries {
                        self.auth_state = AuthState::Failed;
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
            Role::Queen => Ok(trimmed),
            Role::WorkerHeartbeat | Role::WorkerGpu => {
                let value = trimmed.ok_or_else(|| {
                    anyhow!("role {:?} requires a non-empty ticket payload", role)
                })?;
                if !Self::is_ticket_well_formed(&value) {
                    return Err(anyhow!(
                        "ticket must be 64 hexadecimal characters or base64 encoded"
                    ));
                }
                Ok(Some(value))
            }
        }
    }

    fn is_ticket_well_formed(value: &str) -> bool {
        let hex = value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit());
        let base64_len_ok = matches!(value.len(), 43 | 44 | 86 | 87 | 88);
        let base64_chars_ok = value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_'));
        hex || (base64_len_ok && base64_chars_ok)
    }

    fn role_label(role: Role) -> &'static str {
        match role {
            Role::Queen => "queen",
            Role::WorkerHeartbeat => "worker-heartbeat",
            Role::WorkerGpu => "worker-gpu",
        }
    }

    fn trim_line(line: &str) -> String {
        line.trim_end_matches(['\r', '\n']).to_owned()
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
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        let ticket_payload = Self::normalise_ticket(role, ticket)?;
        let ticket_len = ticket_payload
            .as_ref()
            .map(|value| value.len())
            .unwrap_or(0);
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
            self.auth_state = AuthState::AttachSent;
            debug!(
                "[cohsh][auth] state={:?} send ATTACH role={:?} ticket_len={}",
                self.auth_state, role, ticket_len
            );
            self.send_line(&attach_line)?;
            self.auth_state = AuthState::WaitingAttachOk;
            match self.next_protocol_line()? {
                Some(response) => {
                    let _ = self.record_ack(&response);
                    if !response.starts_with("OK") {
                        self.auth_state = AuthState::Failed;
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
                    self.auth_state = AuthState::Attached;
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
                        self.auth_state = AuthState::Failed;
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
                }
            }
        }
    }

    fn kind(&self) -> &'static str {
        "tcp"
    }

    fn ping(&mut self, _session: &Session) -> Result<String> {
        let mut attempts = 0usize;
        loop {
            self.send_line("PING")?;
            match self.next_protocol_line()? {
                Some(response) => {
                    if self.record_ack(&response) {
                        if response.starts_with("OK PING") {
                            return Ok("pong".to_owned());
                        }
                        if response.starts_with("ERR PING") {
                            return Err(anyhow!("ping failed: {response}"));
                        }
                        continue;
                    }
                    if response.eq_ignore_ascii_case("PONG") {
                        return Ok("pong".to_owned());
                    }
                }
                None => {
                    attempts += 1;
                    if attempts > self.max_retries {
                        return Err(anyhow!("connection dropped repeatedly while awaiting PING"));
                    }
                    self.recover_session()?;
                }
            }
        }
    }

    fn tail(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        let command = format!("TAIL {path}");
        let mut attempts = 0usize;
        let mut lines = Vec::new();
        loop {
            self.send_line(&command)?;
            loop {
                match self.next_protocol_line()? {
                    Some(response) => {
                        if self.record_ack(&response) {
                            if response.starts_with("ERR") {
                                return Err(anyhow!("tail failed: {response}"));
                            }
                            continue;
                        }
                        if response == "END" {
                            return Ok(lines);
                        }
                        if response.starts_with("ERR") {
                            return Err(anyhow!("tail failed: {response}"));
                        }
                        lines.push(response);
                    }
                    None => {
                        attempts += 1;
                        if attempts > self.max_retries {
                            return Err(anyhow!(
                                "connection dropped repeatedly while tailing {path}"
                            ));
                        }
                        self.recover_session()?;
                        break;
                    }
                }
            }
        }
    }

    fn write(&mut self, _session: &Session, path: &str, payload: &[u8]) -> Result<()> {
        let line = String::from_utf8(payload.to_vec()).context("payload must be UTF-8")?;
        Err(anyhow!(
            "writes are not yet supported over the TCP transport (path: {path}, payload: {line})"
        ))
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn ticket_validation_enforces_worker_requirements() {
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
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
        )
        .is_ok());
        assert!(TcpTransport::normalise_ticket(
            Role::WorkerHeartbeat,
            Some("MDEyMzQ1Njc4OUFCQ0RFRjAxMjM0NTY3ODlBQkNERUY="),
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
                writeln!(stream, "OK AUTH detail=present-token").unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    let trimmed = line.trim();
                    if trimmed.starts_with("AUTH ") {
                        if trimmed == "AUTH changeme" {
                            writeln!(stream, "OK AUTH").unwrap();
                        } else {
                            writeln!(stream, "ERR AUTH reason=invalid-token").unwrap();
                            break;
                        }
                    } else if trimmed.starts_with("ATTACH") {
                        writeln!(stream, "OK ATTACH role=queen").unwrap();
                    } else if trimmed.starts_with("TAIL") {
                        writeln!(stream, "OK TAIL path=/log/queen.log").unwrap();
                        if connection_barrier.load(Ordering::SeqCst) == 1 {
                            writeln!(stream, "line one").unwrap();
                            stream.flush().unwrap();
                            break;
                        } else {
                            writeln!(stream, "line two").unwrap();
                            writeln!(stream, "END").unwrap();
                        }
                    } else if trimmed == "PING" {
                        writeln!(stream, "PONG").unwrap();
                        writeln!(stream, "OK PING reply=pong").unwrap();
                    }
                    line.clear();
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
                writeln!(stream, "OK AUTH detail=present-token").unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    if line.trim().starts_with("AUTH ") {
                        writeln!(stream, "ERR AUTH reason=invalid-token").unwrap();
                        break;
                    }
                    line.clear();
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
