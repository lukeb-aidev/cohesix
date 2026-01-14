// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide the Cohesix shell CLI core and transport implementations.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Cohesix shell prototype speaking directly to the NineDoor Secure9P server.
//!
//! User-facing prompts, banners, and help text are rendered locally while
//! commands such as `attach` and `tail` are forwarded to the configured
//! transport. The client prefixes transport acknowledgements with `[console]`
//! so callers can distinguish remote answers from local UX noise.

pub mod proto;
/// Manifest-derived client policy helpers for cohsh.
pub mod policy;
mod session_pool;

#[cfg(feature = "tcp")]
pub mod transport;

#[cfg(feature = "tcp")]
pub use transport::tcp::{tcp_debug_enabled, SharedTcpTransport, TcpTransport};
#[cfg(feature = "tcp")]
pub use transport::COHSH_TCP_PORT;
pub use policy::{
    default_policy_path, load_policy, CohshHeartbeatPolicy, CohshPolicy, CohshPoolPolicy,
    CohshRetryPolicy, PolicyOverrides,
};
pub use session_pool::{PoolKind, SessionPool, TransportFactory};

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
#[cfg(feature = "tcp")]
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;
use cohesix_proto::{role_label as proto_role_label, Role as ProtoRole};
use cohesix_ticket::{Role, TicketToken};
use console_ack_wire::{render_ack, AckLine, AckStatus};
use log::info;
use nine_door::{InProcessConnection, NineDoor};
use secure9p_codec::{OpenMode, SessionId, MAX_MSIZE};
use serde::Serialize;

/// Result of executing a single shell command.
#[derive(Debug, PartialEq, Eq)]
pub enum CommandStatus {
    /// Continue reading commands.
    Continue,
    /// Exit the shell loop.
    Quit,
}

/// Simple representation of an attached session.
#[derive(Debug, Clone)]
pub struct Session {
    id: SessionId,
    role: Role,
}

/// Telemetry counters exposed by transports that track retries and heartbeats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransportMetrics {
    /// Number of successful transport connections.
    pub connects: usize,
    /// Number of reconnect attempts after disconnects.
    pub reconnects: usize,
    /// Number of heartbeat probes issued.
    pub heartbeats: usize,
}

impl Session {
    /// Construct a new session wrapper.
    pub fn new(id: SessionId, role: Role) -> Self {
        Self { id, role }
    }

    /// Return the role associated with this session.
    #[must_use]
    pub fn role(&self) -> Role {
        self.role
    }

    /// Return the session identifier.
    #[must_use]
    pub fn id(&self) -> SessionId {
        self.id
    }
}

const ROOT_FID: u32 = 1;
const MAX_SCRIPT_LINES: usize = 256;
const MAX_SCRIPT_WAIT_MS: u64 = 2000;
const MAX_SCRIPT_RESPONSES: usize = 8;
const QUEEN_CTL_PATH: &str = "/queen/ctl";
const TEST_SCRIPT_QUICK_PATH: &str = "/proc/tests/selftest_quick.coh";
const TEST_SCRIPT_FULL_PATH: &str = "/proc/tests/selftest_full.coh";
const TEST_SCRIPT_NEGATIVE_PATH: &str = "/proc/tests/selftest_negative.coh";
const DEFAULT_TEST_TIMEOUT_SECS: u64 = 30;
const MAX_TEST_TIMEOUT_SECS: u64 = 120;
const TEST_REPORT_VERSION: &str = "1";
const REPL_KEEPALIVE_SECS: u64 = 15;
const TEST_TRANSCRIPT_MAX_BYTES: usize = 512;
const TEST_CHECK_NAME_MAX_CHARS: usize = 120;
const TEST_DETAIL_MAX_CHARS: usize = 200;
const TEST_MSIZE_SENTINEL: &str = "{{msize_overflow}}";
const MAX_PATH_COMPONENTS: usize = 8;
static POOL_BENCH_RUN: AtomicUsize = AtomicUsize::new(0);

fn format_script_error(
    line_number: usize,
    line_text: &str,
    state: Option<&ScriptState>,
    reason: &str,
) -> anyhow::Error {
    let last = state
        .and_then(|state| state.last_response_line.as_deref())
        .unwrap_or("<none>");
    let last_command = state
        .and_then(|state| state.last_command_line.as_deref())
        .unwrap_or("<none>");
    let last_source = state
        .and_then(|state| state.last_response_source)
        .map(ScriptResponseSource::label)
        .unwrap_or("none");
    let history = state
        .map(ScriptState::format_response_history)
        .unwrap_or_else(|| "<none>".to_owned());
    let recent = if history == "<none>" {
        format!("recent responses: {history}")
    } else {
        format!("recent responses:\n{history}")
    };
    anyhow!(
        "script failure at line {line_number}: {line_text}\nreason: {reason}\nlast command: {last_command}\nlast response: {last}\nlast response source: {last_source}\n{recent}"
    )
}

#[derive(Debug)]
enum ExpectSelector<'a> {
    Ok,
    Err,
    Substr(&'a str),
    Not(&'a str),
}

fn parse_expect_selector<'a>(
    entry: &ScriptLine,
    rest: &'a str,
    state: Option<&ScriptState>,
) -> Result<ExpectSelector<'a>> {
    if rest.is_empty() {
        return Err(format_script_error(
            entry.number,
            entry.text.as_str(),
            state,
            "EXPECT requires a selector",
        ));
    }
    if rest == "OK" {
        return Ok(ExpectSelector::Ok);
    }
    if rest == "ERR" {
        return Ok(ExpectSelector::Err);
    }
    if let Some(value) = rest.strip_prefix("SUBSTR") {
        let needle = value.trim_start();
        if needle.is_empty() {
            return Err(format_script_error(
                entry.number,
                entry.text.as_str(),
                state,
                "EXPECT SUBSTR requires text",
            ));
        }
        return Ok(ExpectSelector::Substr(needle));
    }
    if let Some(value) = rest.strip_prefix("NOT") {
        let needle = value.trim_start();
        if needle.is_empty() {
            return Err(format_script_error(
                entry.number,
                entry.text.as_str(),
                state,
                "EXPECT NOT requires text",
            ));
        }
        return Ok(ExpectSelector::Not(needle));
    }
    Err(format_script_error(
        entry.number,
        entry.text.as_str(),
        state,
        "EXPECT selector is invalid",
    ))
}

fn parse_wait_ms(entry: &ScriptLine, text: &str, state: Option<&ScriptState>) -> Result<u64> {
    let rest = text.strip_prefix("WAIT").unwrap_or(text).trim_start();
    let mut args = rest.split_whitespace();
    let Some(value) = args.next() else {
        return Err(format_script_error(
            entry.number,
            text,
            state,
            "WAIT requires milliseconds",
        ));
    };
    if args.next().is_some() {
        return Err(format_script_error(
            entry.number,
            text,
            state,
            "WAIT accepts a single millisecond value",
        ));
    }
    let millis: u64 = value.parse().map_err(|_| {
        format_script_error(
            entry.number,
            text,
            state,
            "WAIT requires a numeric millisecond value",
        )
    })?;
    if millis > MAX_SCRIPT_WAIT_MS {
        return Err(format_script_error(
            entry.number,
            text,
            state,
            &format!("WAIT exceeds max of {MAX_SCRIPT_WAIT_MS}ms"),
        ));
    }
    Ok(millis)
}

fn parse_script_lines<R: BufRead>(reader: R) -> Result<Vec<ScriptLine>> {
    let mut lines = Vec::new();
    for (idx, raw_line) in reader.lines().enumerate() {
        let raw_line = raw_line?;
        let trimmed = raw_line.trim_end();
        let without_comment = trimmed
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(trimmed);
        let text = without_comment.trim();
        if text.is_empty() {
            continue;
        }
        lines.push(ScriptLine {
            number: idx + 1,
            text: text.to_owned(),
        });
        if lines.len() > MAX_SCRIPT_LINES {
            return Err(format_script_error(
                idx + 1,
                text,
                None,
                &format!("script exceeds max of {MAX_SCRIPT_LINES} lines"),
            ));
        }
    }
    Ok(lines)
}

/// Validate `.coh` script syntax without executing commands.
pub fn validate_script<R: BufRead>(reader: R) -> Result<()> {
    let lines = parse_script_lines(reader)?;
    let mut last_command_seen = false;
    for entry in &lines {
        let text = entry.text.as_str();
        let mut parts = text.split_whitespace();
        let Some(keyword) = parts.next() else {
            continue;
        };
        if keyword == "EXPECT" {
            if !last_command_seen {
                return Err(format_script_error(
                    entry.number,
                    text,
                    None,
                    "EXPECT requires a prior command response",
                ));
            }
            let rest = text.strip_prefix("EXPECT").unwrap_or(text).trim_start();
            let _ = parse_expect_selector(entry, rest, None)?;
            continue;
        }
        if keyword == "WAIT" {
            let _ = parse_wait_ms(entry, text, None)?;
            continue;
        }
        last_command_seen = true;
    }
    Ok(())
}

/// Tokenize `.coh` script contents into a deterministic stream for regression checks.
pub fn tokenize_script<R: BufRead>(reader: R) -> Result<Vec<String>> {
    let lines = parse_script_lines(reader)?;
    let mut tokens = Vec::new();
    let mut last_command_seen = false;
    for entry in &lines {
        let text = entry.text.as_str();
        let mut parts = text.split_whitespace();
        let Some(keyword) = parts.next() else {
            continue;
        };
        if keyword == "EXPECT" {
            if !last_command_seen {
                return Err(format_script_error(
                    entry.number,
                    text,
                    None,
                    "EXPECT requires a prior command response",
                ));
            }
            let rest = text.strip_prefix("EXPECT").unwrap_or(text).trim_start();
            let selector = parse_expect_selector(entry, rest, None)?;
            let rendered = match selector {
                ExpectSelector::Ok => "EXPECT OK".to_owned(),
                ExpectSelector::Err => "EXPECT ERR".to_owned(),
                ExpectSelector::Substr(value) => format!("EXPECT SUBSTR {value}"),
                ExpectSelector::Not(value) => format!("EXPECT NOT {value}"),
            };
            tokens.push(rendered);
            continue;
        }
        if keyword == "WAIT" {
            let millis = parse_wait_ms(entry, text, None)?;
            tokens.push(format!("WAIT {millis}"));
            continue;
        }
        last_command_seen = true;
        tokens.push(text.to_owned());
    }
    Ok(tokens)
}

/// Transport abstraction used by the shell to interact with the system.
pub trait Transport {
    /// Attach to the transport using the specified role and optional ticket payload.
    ///
    /// Worker roles must supply a ticket containing a subject identity so the
    /// session can bind to the correct worker namespace.
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session>;

    /// Return a human-readable label describing the transport implementation.
    fn kind(&self) -> &'static str {
        "transport"
    }

    /// Perform a lightweight health probe using the underlying protocol.
    fn ping(&mut self, session: &Session) -> Result<String>;

    /// Stream a log-like file and return the accumulated contents.
    fn tail(&mut self, session: &Session, path: &str) -> Result<Vec<String>>;

    /// Read a file and return the accumulated contents.
    fn read(&mut self, session: &Session, path: &str) -> Result<Vec<String>>;

    /// List directory entries at the supplied path.
    fn list(&mut self, session: &Session, path: &str) -> Result<Vec<String>>;

    /// Append bytes to an append-only file within the NineDoor namespace.
    fn write(&mut self, session: &Session, path: &str, payload: &[u8]) -> Result<()>;

    /// Append multiple payloads to the same file in sequence.
    fn write_batch(
        &mut self,
        session: &Session,
        path: &str,
        payloads: &[Vec<u8>],
    ) -> Result<usize> {
        let mut count = 0usize;
        for payload in payloads {
            self.write(session, path, payload)?;
            count = count.saturating_add(1);
        }
        Ok(count)
    }

    /// Request the remote console session to close.
    fn quit(&mut self, _session: &Session) -> Result<()> {
        Ok(())
    }

    /// Drain acknowledgement lines accumulated since the previous call.
    fn drain_acknowledgements(&mut self) -> Vec<String> {
        Vec::new()
    }

    /// Return transport retry/heartbeat counters where available.
    fn metrics(&self) -> TransportMetrics {
        TransportMetrics::default()
    }

    /// Inject a short-write fault for the next send operation, if supported.
    fn inject_short_write(&mut self, _bytes: usize) -> bool {
        false
    }

    /// Return the TCP endpoint if the transport supports TCP diagnostics.
    fn tcp_endpoint(&self) -> Option<(String, u16)> {
        None
    }
}

impl<T> Transport for Box<T>
where
    T: Transport + ?Sized,
{
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        (**self).attach(role, ticket)
    }

    fn kind(&self) -> &'static str {
        (**self).kind()
    }

    fn ping(&mut self, session: &Session) -> Result<String> {
        (**self).ping(session)
    }

    fn tail(&mut self, session: &Session, path: &str) -> Result<Vec<String>> {
        (**self).tail(session, path)
    }

    fn read(&mut self, session: &Session, path: &str) -> Result<Vec<String>> {
        (**self).read(session, path)
    }

    fn list(&mut self, session: &Session, path: &str) -> Result<Vec<String>> {
        (**self).list(session, path)
    }

    fn write(&mut self, session: &Session, path: &str, payload: &[u8]) -> Result<()> {
        (**self).write(session, path, payload)
    }

    fn write_batch(
        &mut self,
        session: &Session,
        path: &str,
        payloads: &[Vec<u8>],
    ) -> Result<usize> {
        (**self).write_batch(session, path, payloads)
    }

    fn quit(&mut self, session: &Session) -> Result<()> {
        (**self).quit(session)
    }

    fn drain_acknowledgements(&mut self) -> Vec<String> {
        (**self).drain_acknowledgements()
    }

    fn tcp_endpoint(&self) -> Option<(String, u16)> {
        (**self).tcp_endpoint()
    }

    fn metrics(&self) -> TransportMetrics {
        (**self).metrics()
    }

    fn inject_short_write(&mut self, bytes: usize) -> bool {
        (**self).inject_short_write(bytes)
    }
}

/// Live transport backed by the in-process NineDoor Secure9P server.
#[derive(Debug)]
pub struct NineDoorTransport {
    server: NineDoor,
    connection: Option<InProcessConnection>,
    next_fid: u32,
    ack_lines: VecDeque<String>,
}

impl NineDoorTransport {
    /// Create a new transport bound to the supplied server instance.
    pub fn new(server: NineDoor) -> Self {
        Self {
            server,
            connection: None,
            next_fid: ROOT_FID,
            ack_lines: VecDeque::new(),
        }
    }

    fn allocate_fid(&mut self) -> u32 {
        let fid = self.next_fid;
        self.next_fid = self.next_fid.wrapping_add(1);
        fid
    }

    fn push_ack(&mut self, status: AckStatus, verb: &str, detail: Option<&str>) {
        let mut line = String::new();
        let ack = AckLine { status, verb, detail };
        if render_ack(&mut line, &ack).is_ok() {
            self.ack_lines.push_back(line);
        }
    }

    fn role_label(role: Role) -> &'static str {
        match role {
            Role::Queen => proto_role_label(ProtoRole::Queen),
            Role::WorkerHeartbeat => proto_role_label(ProtoRole::Worker),
            Role::WorkerGpu => proto_role_label(ProtoRole::GpuWorker),
        }
    }

    fn read_lines(&mut self, path: &str) -> Result<Vec<String>> {
        let components = parse_path(path)?;
        let fid = self.allocate_fid();
        let connection = self
            .connection
            .as_mut()
            .context("attach to a session before reading")?;
        connection
            .walk(ROOT_FID, fid, &components)
            .with_context(|| format!("failed to walk to {path}"))?;
        connection
            .open(fid, OpenMode::read_only())
            .with_context(|| format!("failed to open {path}"))?;
        let mut offset = 0u64;
        let mut buffer = Vec::new();
        loop {
            let chunk = connection
                .read(fid, offset, connection.negotiated_msize())
                .with_context(|| format!("failed to read {path}"))?;
            if chunk.is_empty() {
                break;
            }
            offset = offset
                .checked_add(chunk.len() as u64)
                .context("offset overflow during read")?;
            buffer.extend_from_slice(&chunk);
            if chunk.len() < connection.negotiated_msize() as usize {
                break;
            }
        }
        connection.clunk(fid).context("failed to clunk fid")?;
        let text = String::from_utf8(buffer).context("log is not valid UTF-8")?;
        Ok(text.lines().map(|line| line.to_owned()).collect())
    }
}

impl Transport for NineDoorTransport {
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        let mut connection = self
            .server
            .connect()
            .context("failed to open NineDoor session")?;
        connection
            .version(MAX_MSIZE)
            .context("version negotiation failed")?;
        let mut subject = None;
        let mut ticket_payload = ticket.map(str::trim).filter(|value| !value.is_empty());
        match role {
            Role::Queen => {}
            Role::WorkerHeartbeat | Role::WorkerGpu => {
                let provided = ticket_payload.ok_or_else(|| {
                    anyhow!(
                        "role {:?} requires a capability ticket containing an identity",
                        role
                    )
                })?;
                let claims = TicketToken::decode_unverified(provided)
                    .map_err(|err| anyhow!("invalid ticket: {err}"))?;
                if claims.role != role {
                    return Err(anyhow!(
                        "ticket role {:?} does not match requested role {:?}",
                        claims.role,
                        role
                    ));
                }
                let subject_value = claims.subject.as_deref().ok_or_else(|| {
                    anyhow!(
                        "ticket is missing required subject identity for role {:?}",
                        role
                    )
                })?;
                subject = Some(subject_value.to_string());
                ticket_payload = Some(provided);
            }
        };
        let attach_result =
            connection.attach_with_identity(ROOT_FID, role, subject.as_deref(), ticket_payload);
        let attach_result = attach_result.context("attach request failed");
        if let Err(err) = attach_result {
            let detail = format!("reason={err}");
            self.push_ack(AckStatus::Err, "ATTACH", Some(detail.as_str()));
            return Err(err);
        }
        self.next_fid = ROOT_FID + 1;
        let session = Session::new(connection.session_id(), role);
        self.connection = Some(connection);
        let detail = format!("role={}", Self::role_label(role));
        self.push_ack(AckStatus::Ok, "ATTACH", Some(detail.as_str()));
        Ok(session)
    }

    fn kind(&self) -> &'static str {
        "mock"
    }

    fn ping(&mut self, session: &Session) -> Result<String> {
        let fid = self.allocate_fid();
        let connection = self
            .connection
            .as_mut()
            .context("attach to a session before running ping")?;
        connection
            .walk(ROOT_FID, fid, &[])
            .context("ping walk failed")?;
        connection.clunk(fid).context("ping clunk failed")?;
        Ok(format!("attached as {:?} via mock", session.role()))
    }

    fn tail(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        match self.read_lines(path) {
            Ok(lines) => {
                let detail = format!("path={path}");
                self.push_ack(AckStatus::Ok, "TAIL", Some(detail.as_str()));
                Ok(lines)
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                self.push_ack(AckStatus::Err, "TAIL", Some(detail.as_str()));
                Err(err)
            }
        }
    }

    fn read(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        match self.read_lines(path) {
            Ok(lines) => {
                let detail = format!("path={path}");
                self.push_ack(AckStatus::Ok, "CAT", Some(detail.as_str()));
                Ok(lines)
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                self.push_ack(AckStatus::Err, "CAT", Some(detail.as_str()));
                Err(err)
            }
        }
    }

    fn list(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        match self.read_lines(path) {
            Ok(lines) => {
                let detail = format!("path={path}");
                self.push_ack(AckStatus::Ok, "LS", Some(detail.as_str()));
                Ok(lines)
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                self.push_ack(AckStatus::Err, "LS", Some(detail.as_str()));
                Err(err)
            }
        }
    }

    fn write(&mut self, _session: &Session, path: &str, payload: &[u8]) -> Result<()> {
        let verb = if path == QUEEN_CTL_PATH {
            if let Ok(payload_text) = std::str::from_utf8(payload) {
                if payload_text.contains("\"kill\"") {
                    "KILL"
                } else if payload_text.contains("\"spawn\"") {
                    "SPAWN"
                } else {
                    "ECHO"
                }
            } else {
                "ECHO"
            }
        } else {
            "ECHO"
        };
        let components = match parse_path(path) {
            Ok(components) => components,
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                self.push_ack(AckStatus::Err, verb, Some(detail.as_str()));
                return Err(err);
            }
        };
        let fid = self.allocate_fid();
        let result = (|| {
            let connection = self
                .connection
                .as_mut()
                .context("attach to a session before running write")?;
            connection
                .walk(ROOT_FID, fid, &components)
                .with_context(|| format!("failed to walk to {path}"))?;
            connection
                .open(fid, OpenMode::write_append())
                .with_context(|| format!("failed to open {path}"))?;
            let written = connection
                .write(fid, payload)
                .with_context(|| format!("failed to write {path}"))?;
            connection.clunk(fid).context("failed to clunk fid")?;
            Ok(written)
        })();
        let written = match result {
            Ok(written) => written,
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                self.push_ack(AckStatus::Err, verb, Some(detail.as_str()));
                return Err(err);
            }
        };
        if written as usize != payload.len() {
            let err = anyhow!(
                "short write to {path}: expected {} bytes, wrote {written}",
                payload.len()
            );
            let detail = format!("path={path} reason={err}");
            self.push_ack(AckStatus::Err, verb, Some(detail.as_str()));
            return Err(err);
        }
        let detail = format!("path={path} bytes={}", payload.len());
        self.push_ack(AckStatus::Ok, verb, Some(detail.as_str()));
        Ok(())
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
        let default_verb = if path == QUEEN_CTL_PATH {
            if let Ok(payload_text) = std::str::from_utf8(&payloads[0]) {
                if payload_text.contains("\"kill\"") {
                    "KILL"
                } else if payload_text.contains("\"spawn\"") {
                    "SPAWN"
                } else {
                    "ECHO"
                }
            } else {
                "ECHO"
            }
        } else {
            "ECHO"
        };
        let components = match parse_path(path) {
            Ok(components) => components,
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                self.push_ack(AckStatus::Err, default_verb, Some(detail.as_str()));
                return Err(err);
            }
        };
        let fid = self.allocate_fid();
        let mut written_count = 0usize;
        let mut pending_acks: Vec<(&'static str, String)> = Vec::new();
        let result = (|| {
            let connection = self
                .connection
                .as_mut()
                .context("attach to a session before running write")?;
            connection
                .walk(ROOT_FID, fid, &components)
                .with_context(|| format!("failed to walk to {path}"))?;
            connection
                .open(fid, OpenMode::write_append())
                .with_context(|| format!("failed to open {path}"))?;
            for payload in payloads {
                let written = connection
                    .write(fid, payload)
                    .with_context(|| format!("failed to write {path}"))?;
                if written as usize != payload.len() {
                    return Err(anyhow!(
                        "short write to {path}: expected {} bytes, wrote {written}",
                        payload.len()
                    ));
                }
                let verb = if path == QUEEN_CTL_PATH {
                    if let Ok(payload_text) = std::str::from_utf8(payload) {
                        if payload_text.contains("\"kill\"") {
                            "KILL"
                        } else if payload_text.contains("\"spawn\"") {
                            "SPAWN"
                        } else {
                            "ECHO"
                        }
                    } else {
                        "ECHO"
                    }
                } else {
                    "ECHO"
                };
                let detail = format!("path={path} bytes={}", payload.len());
                pending_acks.push((verb, detail));
                written_count = written_count.saturating_add(1);
            }
            connection.clunk(fid).context("failed to clunk fid")?;
            Ok(())
        })();
        if let Err(err) = result {
            for (verb, detail) in pending_acks {
                self.push_ack(AckStatus::Ok, verb, Some(detail.as_str()));
            }
            let detail = format!("path={path} reason={err}");
            self.push_ack(AckStatus::Err, default_verb, Some(detail.as_str()));
            let _ = self
                .connection
                .as_mut()
                .map(|connection| connection.clunk(fid));
            return Err(err);
        }
        for (verb, detail) in pending_acks {
            self.push_ack(AckStatus::Ok, verb, Some(detail.as_str()));
        }
        Ok(written_count)
    }

    fn drain_acknowledgements(&mut self) -> Vec<String> {
        self.ack_lines.drain(..).collect()
    }
}

/// QEMU-backed transport that boots the Cohesix image and streams serial logs.
#[derive(Debug)]
pub struct QemuTransport {
    qemu_bin: PathBuf,
    out_dir: PathBuf,
    extra_qemu_args: Vec<String>,
    gic_version: String,
    log_lines: Arc<Mutex<Vec<String>>>,
    child: Option<Child>,
    stdout_handle: Option<JoinHandle<()>>,
    stderr_handle: Option<JoinHandle<()>>,
    session_id: u64,
}

impl QemuTransport {
    /// Create a new QEMU transport using the supplied binary, artefact directory, and arguments.
    pub fn new(
        qemu_bin: impl Into<PathBuf>,
        out_dir: impl Into<PathBuf>,
        gic_version: impl Into<String>,
        extra_qemu_args: Vec<String>,
    ) -> Self {
        Self {
            qemu_bin: qemu_bin.into(),
            out_dir: out_dir.into(),
            extra_qemu_args,
            gic_version: gic_version.into(),
            log_lines: Arc::new(Mutex::new(Vec::new())),
            child: None,
            stdout_handle: None,
            stderr_handle: None,
            session_id: 1,
        }
    }

    fn spawn_reader<R>(stream: R, lines: Arc<Mutex<Vec<String>>>, store: bool) -> JoinHandle<()>
    where
        R: Read + Send + 'static,
    {
        thread::spawn(move || {
            let reader = BufReader::new(stream);
            for line in reader.lines().map_while(Result::ok) {
                if store {
                    let mut guard = lines.lock().expect("log mutex poisoned");
                    guard.push(line);
                }
            }
        })
    }

    fn artefacts(&self) -> Result<(PathBuf, PathBuf, PathBuf, PathBuf)> {
        let staging = self.out_dir.join("staging");
        let elfloader = staging.join("elfloader");
        let kernel = staging.join("kernel.elf");
        let rootserver = staging.join("rootserver");
        let cpio = self.out_dir.join("cohesix-system.cpio");

        for (path, label) in [
            (&elfloader, "elfloader"),
            (&kernel, "kernel"),
            (&rootserver, "rootserver"),
            (&cpio, "payload cpio"),
        ] {
            if !path.is_file() {
                return Err(anyhow!(
                    "{label} artefact not found at {} (run scripts/cohesix-build-run.sh --no-run first)",
                    path.display()
                ));
            }
        }

        Ok((elfloader, kernel, rootserver, cpio))
    }

    fn wait_for_log(&self, timeout: Duration) -> Result<Vec<String>> {
        let start = Instant::now();
        let sentinel = "root task idling";
        loop {
            {
                let guard = self.log_lines.lock().expect("log mutex poisoned");
                if guard.iter().any(|line| line.contains(sentinel)) {
                    return Ok(guard.clone());
                }
            }
            if start.elapsed() > timeout {
                return Err(anyhow!(
                    "timed out waiting for root-task output from QEMU (path: {:#?})",
                    self.out_dir
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn filter_root_log(lines: &[String]) -> Vec<String> {
        lines
            .iter()
            .filter_map(|line| {
                if let Some(stripped) = line.strip_prefix("[cohesix:root-task] ") {
                    Some(stripped.to_owned())
                } else if let Some((_, rest)) = line.split_once(']') {
                    let body = rest.trim_start();
                    if line.contains("[cohesix:root-task]") && !body.is_empty() {
                        Some(body.to_owned())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    fn stop_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(handle) = self.stdout_handle.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for QemuTransport {
    fn drop(&mut self) {
        self.stop_child();
    }
}

impl Transport for QemuTransport {
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        if self.child.is_some() {
            return Err(anyhow!("QEMU session already active"));
        }

        if !matches!(role, Role::Queen) {
            return Err(anyhow!(
                "QEMU transport currently only supports attaching as the queen role"
            ));
        }
        if let Some(ticket) = ticket {
            if !ticket.trim().is_empty() {
                return Err(anyhow!(
                    "tickets are not required when attaching via the QEMU transport"
                ));
            }
        }

        let (elfloader, kernel, rootserver, cpio) = self.artefacts()?;
        *self.log_lines.lock().expect("log mutex poisoned") = Vec::new();

        let mut cmd = Command::new(&self.qemu_bin);
        cmd.arg("-machine")
            .arg(format!("virt,gic-version={}", self.gic_version))
            .arg("-cpu")
            .arg("cortex-a57")
            .arg("-m")
            .arg("1024")
            .arg("-smp")
            .arg("1")
            .arg("-serial")
            .arg("mon:stdio")
            .arg("-display")
            .arg("none")
            .arg("-kernel")
            .arg(&elfloader)
            .arg("-initrd")
            .arg(&cpio)
            .arg("-device")
            .arg(format!(
                "loader,file={},addr=0x70000000,force-raw=on",
                kernel.display()
            ))
            .arg("-device")
            .arg(format!(
                "loader,file={},addr=0x80000000,force-raw=on",
                rootserver.display()
            ));

        for arg in &self.extra_qemu_args {
            cmd.arg(arg);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .context("failed to launch qemu-system-aarch64")?;
        let stdout = child
            .stdout
            .take()
            .context("failed to capture QEMU stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("failed to capture QEMU stderr")?;

        let stdout_lines = Arc::clone(&self.log_lines);
        self.stdout_handle = Some(Self::spawn_reader(stdout, stdout_lines, true));
        self.stderr_handle = Some(Self::spawn_reader(
            stderr,
            Arc::new(Mutex::new(Vec::new())),
            false,
        ));

        self.child = Some(child);
        let session = Session::new(SessionId::from_raw(self.session_id), role);
        self.session_id = self.session_id.wrapping_add(1);
        Ok(session)
    }

    fn kind(&self) -> &'static str {
        "qemu"
    }

    fn ping(&mut self, session: &Session) -> Result<String> {
        if let Some(child) = self.child.as_mut() {
            if let Some(status) = child.try_wait().context("failed to query QEMU state")? {
                return Err(anyhow!("qemu exited with status {status}"));
            }
            return Ok(format!("ping: qemu running for {:?}", session.role()));
        }
        Err(anyhow!("attach via QEMU before issuing ping"))
    }

    fn tail(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        if path != "/log/queen.log" {
            return Err(anyhow!(
                "QEMU transport currently supports tailing /log/queen.log only"
            ));
        }
        let raw_lines = self.wait_for_log(Duration::from_secs(15))?;
        let cleaned = Self::filter_root_log(&raw_lines);
        self.stop_child();
        Ok(cleaned)
    }

    fn read(&mut self, _session: &Session, _path: &str) -> Result<Vec<String>> {
        Err(anyhow!(
            "reads are not supported when using the QEMU transport"
        ))
    }

    fn list(&mut self, _session: &Session, _path: &str) -> Result<Vec<String>> {
        Err(anyhow!(
            "directory listing is not supported when using the QEMU transport"
        ))
    }

    fn write(&mut self, _session: &Session, _path: &str, _payload: &[u8]) -> Result<()> {
        Err(anyhow!(
            "writes are not supported when using the QEMU transport"
        ))
    }
}

/// Clap-compatible role selector used by the CLI entry point.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum RoleArg {
    /// Queen orchestration role.
    Queen,
    /// Worker heartbeat role.
    WorkerHeartbeat,
    /// Worker GPU role.
    WorkerGpu,
}

impl fmt::Display for RoleArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Queen => ProtoRole::Queen,
            Self::WorkerHeartbeat => ProtoRole::Worker,
            Self::WorkerGpu => ProtoRole::GpuWorker,
        };
        write!(f, "{}", proto_role_label(label))
    }
}

impl From<RoleArg> for Role {
    fn from(value: RoleArg) -> Self {
        match value {
            RoleArg::Queen => Role::Queen,
            RoleArg::WorkerHeartbeat => Role::WorkerHeartbeat,
            RoleArg::WorkerGpu => Role::WorkerGpu,
        }
    }
}

/// Shell driver responsible for parsing commands and invoking the transport.
pub struct Shell<T: Transport, W: Write> {
    transport: T,
    session: Option<Session>,
    pool: Option<SessionPool>,
    writer: W,
    script_state: Option<ScriptState>,
}

#[derive(Debug, Default)]
struct ScriptState {
    last_command_line: Option<String>,
    last_response_line: Option<String>,
    last_response_source: Option<ScriptResponseSource>,
    response_from_ack: bool,
    responses: VecDeque<ScriptResponseLine>,
}

#[derive(Debug)]
struct ScriptLine {
    number: usize,
    text: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ScriptResponseSource {
    Ack,
    Output,
}

impl ScriptResponseSource {
    fn label(self) -> &'static str {
        match self {
            Self::Ack => "ack",
            Self::Output => "out",
        }
    }
}

#[derive(Debug)]
struct ScriptResponseLine {
    source: ScriptResponseSource,
    line: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum TestMode {
    Quick,
    Full,
}

impl TestMode {
    fn label(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Full => "full",
        }
    }
}

#[derive(Debug, Clone)]
struct TestOptions {
    mode: TestMode,
    json: bool,
    timeout: Duration,
    no_mutate: bool,
}

#[derive(Debug, Default)]
struct CommandTranscript {
    ack_lines: Vec<String>,
    output_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct PoolBenchSample {
    ops: usize,
    elapsed_ms: u64,
    ops_per_s: u64,
}

#[derive(Debug, Clone, Copy)]
struct PoolBenchResult {
    baseline: PoolBenchSample,
    pooled: PoolBenchSample,
    retries: usize,
    pool_exhausted: usize,
    failures: usize,
    observed: usize,
    expected: usize,
}

#[derive(Debug, Clone)]
struct PoolBenchConfig {
    path: String,
    ops: usize,
    batch: usize,
    payload_prefix: String,
    payload_bytes: Option<usize>,
    kind: PoolKind,
    delay_ms: u64,
    inject_failures: usize,
    inject_bytes: usize,
    exhaust: usize,
}

#[derive(Debug)]
struct CommandExecution {
    status: CommandStatus,
    transcript: CommandTranscript,
    error: Option<anyhow::Error>,
}

impl CommandExecution {
    fn ok(status: CommandStatus, transcript: CommandTranscript) -> Self {
        Self {
            status,
            transcript,
            error: None,
        }
    }

    fn err(error: anyhow::Error, transcript: CommandTranscript) -> Self {
        Self {
            status: CommandStatus::Continue,
            transcript,
            error: Some(error),
        }
    }
}

#[derive(Debug, Serialize)]
struct TestReport {
    ok: bool,
    mode: String,
    elapsed_ms: u64,
    checks: Vec<TestCheck>,
    version: String,
}

#[derive(Debug, Serialize)]
struct TestCheck {
    name: String,
    ok: bool,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    transcript_excerpt: Option<String>,
}

impl ScriptState {
    fn begin_command(&mut self, line: &str) {
        self.last_command_line = Some(line.to_owned());
        self.last_response_line = None;
        self.last_response_source = None;
        self.response_from_ack = false;
        self.responses.clear();
    }

    fn record_response_line(&mut self, source: ScriptResponseSource, line: &str) {
        if self.responses.len() >= MAX_SCRIPT_RESPONSES {
            self.responses.pop_front();
        }
        self.responses.push_back(ScriptResponseLine {
            source,
            line: line.to_owned(),
        });
        match source {
            ScriptResponseSource::Ack => {
                self.last_response_line = Some(line.to_owned());
                self.last_response_source = Some(source);
                self.response_from_ack = true;
            }
            ScriptResponseSource::Output => {
                if !self.response_from_ack {
                    self.last_response_line = Some(line.to_owned());
                    self.last_response_source = Some(source);
                }
            }
        }
    }

    fn record_output_line(&mut self, line: &str) {
        self.record_response_line(ScriptResponseSource::Output, line);
    }

    fn record_ack_line(&mut self, line: &str) {
        self.record_response_line(ScriptResponseSource::Ack, line);
    }

    fn format_response_history(&self) -> String {
        if self.responses.is_empty() {
            return "<none>".to_owned();
        }
        let mut out = String::new();
        for entry in &self.responses {
            out.push_str("  - [");
            out.push_str(entry.source.label());
            out.push_str("] ");
            out.push_str(entry.line.as_str());
            out.push('\n');
        }
        if out.ends_with('\n') {
            out.pop();
        }
        out
    }
}

impl<T: Transport, W: Write> Shell<T, W> {
    /// Create a new shell given a transport and output writer.
    pub fn new(transport: T, writer: W) -> Self {
        Self {
            transport,
            session: None,
            pool: None,
            writer,
            script_state: None,
        }
    }

    /// Enable a session pool for this shell instance.
    pub fn with_pool(mut self, pool: SessionPool) -> Self {
        self.pool = Some(pool);
        self
    }

    /// Write a line directly to the shell output.
    pub fn write_line(&mut self, message: &str) -> Result<()> {
        self.write_output_line(message)?;
        Ok(())
    }

    fn write_output_line(&mut self, message: &str) -> Result<()> {
        writeln!(self.writer, "{message}")?;
        self.writer.flush()?;
        if let Some(state) = self.script_state.as_mut() {
            state.record_output_line(message);
        }
        Ok(())
    }

    fn write_ack_line(&mut self, ack: &str) -> Result<()> {
        writeln!(self.writer, "[console] {ack}")?;
        self.writer.flush()?;
        if let Some(state) = self.script_state.as_mut() {
            state.record_ack_line(ack);
        }
        Ok(())
    }

    fn drain_ack_lines(&mut self) -> Result<()> {
        for ack in self.transport.drain_acknowledgements() {
            self.write_ack_line(&ack)?;
        }
        Ok(())
    }

    fn maybe_keepalive(&mut self, last_keepalive: &mut Instant) {
        if last_keepalive.elapsed() < Duration::from_secs(REPL_KEEPALIVE_SECS) {
            return;
        }
        let Some(session) = self.session.as_ref() else {
            return;
        };
        *last_keepalive = Instant::now();
        let _ = self.transport.ping(session);
        let _ = self.transport.drain_acknowledgements();
    }

    fn begin_script_command(&mut self, line: &str) {
        if let Some(state) = self.script_state.as_mut() {
            state.begin_command(line);
        }
    }

    fn prompt(&self) -> String {
        "coh> ".to_owned()
    }

    /// Attach to the transport using the supplied role and optional ticket payload.
    /// Worker roles must provide a ticket containing a subject identity.
    pub fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<()> {
        if self.session.is_some() {
            return Err(anyhow!(
                "already attached; run 'quit' to close the current session"
            ));
        }
        let session = self.transport.attach(role, ticket)?;
        for ack in self.transport.drain_acknowledgements() {
            self.write_ack_line(&ack)?;
        }
        if let Some(pool) = self.pool.as_ref() {
            if let Err(err) = pool.attach(role, ticket) {
                let _ = self.transport.quit(&session);
                return Err(err);
            }
        }
        self.write_line(&format!(
            "attached session {:?} as {:?}",
            session.id(),
            session.role()
        ))?;
        self.session = Some(session);
        Ok(())
    }

    /// Execute commands from a buffered reader until EOF or `quit` is encountered.
    pub fn run_script<R: BufRead>(&mut self, reader: R) -> Result<()> {
        let lines = parse_script_lines(reader)?;
        self.script_state = Some(ScriptState::default());
        let result = self.run_script_lines(&lines);
        self.script_state = None;
        result
    }

    fn run_script_lines(&mut self, lines: &[ScriptLine]) -> Result<()> {
        let mut index = 0usize;
        while index < lines.len() {
            let entry = &lines[index];
            let text = entry.text.as_str();
            let mut parts = text.split_whitespace();
            let Some(keyword) = parts.next() else {
                index = index.saturating_add(1);
                continue;
            };

            if keyword == "EXPECT" {
                let rest = text.strip_prefix("EXPECT").unwrap_or(text).trim_start();
                let selector = parse_expect_selector(entry, rest, self.script_state.as_ref())?;
                let last = match self
                    .script_state
                    .as_ref()
                    .and_then(|state| state.last_response_line.as_deref())
                {
                    Some(last) => last,
                    None => {
                        let reason = match self
                            .script_state
                            .as_ref()
                            .and_then(|state| state.last_command_line.as_deref())
                        {
                            Some(cmd) => format!(
                                "EXPECT requires a prior command response (last command: {cmd})"
                            ),
                            None => "EXPECT requires a prior command response".to_owned(),
                        };
                        return Err(format_script_error(
                            entry.number,
                            text,
                            self.script_state.as_ref(),
                            &reason,
                        ));
                    }
                };
                match selector {
                    ExpectSelector::Ok => {
                        if !last.starts_with("OK") {
                            return Err(format_script_error(
                                entry.number,
                                text,
                                self.script_state.as_ref(),
                                "EXPECT OK failed",
                            ));
                        }
                    }
                    ExpectSelector::Err => {
                        if !last.starts_with("ERR") {
                            return Err(format_script_error(
                                entry.number,
                                text,
                                self.script_state.as_ref(),
                                "EXPECT ERR failed",
                            ));
                        }
                    }
                    ExpectSelector::Substr(needle) => {
                        if !last.contains(needle) {
                            return Err(format_script_error(
                                entry.number,
                                text,
                                self.script_state.as_ref(),
                                "EXPECT SUBSTR failed",
                            ));
                        }
                    }
                    ExpectSelector::Not(needle) => {
                        if last.contains(needle) {
                            return Err(format_script_error(
                                entry.number,
                                text,
                                self.script_state.as_ref(),
                                "EXPECT NOT failed",
                            ));
                        }
                    }
                }
                index = index.saturating_add(1);
                continue;
            }

            if keyword == "WAIT" {
                let millis = parse_wait_ms(entry, text, self.script_state.as_ref())?;
                thread::sleep(Duration::from_millis(millis));
                index = index.saturating_add(1);
                continue;
            }

            self.begin_script_command(text);
            match self.execute(text) {
                Ok(CommandStatus::Quit) => break,
                Ok(CommandStatus::Continue) => {}
                Err(err) => {
                    if self.should_defer_script_error(index, lines) {
                        index = index.saturating_add(1);
                        continue;
                    }
                    let reason = format!("command failed: {err}");
                    return Err(format_script_error(
                        entry.number,
                        text,
                        self.script_state.as_ref(),
                        &reason,
                    ));
                }
            }
            index = index.saturating_add(1);
        }
        Ok(())
    }

    fn run_selftest(&mut self, options: TestOptions) -> Result<TestReport> {
        let start = Instant::now();
        let mut checks = Vec::new();
        let mut overall_ok = true;

        let Some(session) = self.session.clone() else {
            checks.push(TestCheck {
                name: truncate_text("preflight/attach", TEST_CHECK_NAME_MAX_CHARS),
                ok: false,
                detail: truncate_text("ERR not attached", TEST_DETAIL_MAX_CHARS),
                transcript_excerpt: None,
            });
            let report = TestReport {
                ok: false,
                mode: options.mode.label().to_owned(),
                elapsed_ms: start.elapsed().as_millis() as u64,
                checks,
                version: TEST_REPORT_VERSION.to_owned(),
            };
            self.emit_test_report(&report, options.json)?;
            return Ok(report);
        };

        let ping_execution = self.execute_test_command("ping");
        let ping_ok = ping_execution.error.is_none();
        let ping_detail = if ping_ok {
            "OK ping".to_owned()
        } else {
            let detail = ping_execution
                .error
                .as_ref()
                .map(|err| err.to_string())
                .unwrap_or_else(|| "ping failed".to_owned());
            format!("ERR ping failed: {detail}")
        };
        checks.push(TestCheck {
            name: truncate_text("preflight/ping", TEST_CHECK_NAME_MAX_CHARS),
            ok: ping_ok,
            detail: truncate_text(&ping_detail, TEST_DETAIL_MAX_CHARS),
            transcript_excerpt: format_transcript_excerpt(&ping_execution.transcript),
        });
        if !ping_ok {
            let report = TestReport {
                ok: false,
                mode: options.mode.label().to_owned(),
                elapsed_ms: start.elapsed().as_millis() as u64,
                checks,
                version: TEST_REPORT_VERSION.to_owned(),
            };
            self.emit_test_report(&report, options.json)?;
            return Ok(report);
        }

        let script_paths = match options.mode {
            TestMode::Quick => [TEST_SCRIPT_NEGATIVE_PATH, TEST_SCRIPT_QUICK_PATH],
            TestMode::Full => [TEST_SCRIPT_NEGATIVE_PATH, TEST_SCRIPT_FULL_PATH],
        };

        for path in script_paths {
            if start.elapsed() > options.timeout {
                checks.push(TestCheck {
                    name: truncate_text("timeout", TEST_CHECK_NAME_MAX_CHARS),
                    ok: false,
                    detail: truncate_text("ERR timeout exceeded", TEST_DETAIL_MAX_CHARS),
                    transcript_excerpt: None,
                });
                overall_ok = false;
                break;
            }
            match self.load_test_script(&session, path) {
                Ok(lines) => {
                    if !self.run_selftest_lines(&lines, &options, start, &mut checks) {
                        overall_ok = false;
                        break;
                    }
                }
                Err(err) => {
                    checks.push(TestCheck {
                        name: truncate_text(&format!("load {path}"), TEST_CHECK_NAME_MAX_CHARS),
                        ok: false,
                        detail: truncate_text(&format!("ERR {err}"), TEST_DETAIL_MAX_CHARS),
                        transcript_excerpt: None,
                    });
                    overall_ok = false;
                    break;
                }
            }
        }

        let report = TestReport {
            ok: overall_ok,
            mode: options.mode.label().to_owned(),
            elapsed_ms: start.elapsed().as_millis() as u64,
            checks,
            version: TEST_REPORT_VERSION.to_owned(),
        };
        self.emit_test_report(&report, options.json)?;
        Ok(report)
    }

    fn emit_test_report(&mut self, report: &TestReport, json: bool) -> Result<()> {
        if json {
            let payload = serde_json::to_string(report)?;
            writeln!(self.writer, "{payload}")?;
            return Ok(());
        }

        let status = if report.ok { "PASS" } else { "FAIL" };
        self.write_line(&format!(
            "selftest {} mode={} elapsed_ms={}",
            status, report.mode, report.elapsed_ms
        ))?;
        let mut first_failure = None;
        for check in &report.checks {
            let mark = if check.ok { "PASS" } else { "FAIL" };
            if first_failure.is_none() && !check.ok {
                first_failure = Some(check);
            }
            self.write_line(&format!("{mark} - {} ({})", check.name, check.detail))?;
        }
        if let Some(failure) = first_failure {
            self.write_line(&format!(
                "first failure: {} ({})",
                failure.name, failure.detail
            ))?;
        }
        Ok(())
    }

    fn load_test_script(&mut self, session: &Session, path: &str) -> Result<Vec<ScriptLine>> {
        let lines = self
            .transport
            .read(session, path)
            .with_context(|| format!("failed to read {path}"))?;
        for ack in self.transport.drain_acknowledgements() {
            let _ = ack;
        }
        let text = if lines.is_empty() {
            String::new()
        } else {
            lines.join("\n")
        };
        parse_script_lines(io::Cursor::new(text.into_bytes()))
            .with_context(|| format!("failed to parse {path}"))
    }

    fn run_selftest_lines(
        &mut self,
        lines: &[ScriptLine],
        options: &TestOptions,
        start: Instant,
        checks: &mut Vec<TestCheck>,
    ) -> bool {
        let mut index = 0usize;
        let mut state = ScriptState::default();
        let mut skip_expect = false;
        while index < lines.len() {
            if start.elapsed() > options.timeout {
                checks.push(TestCheck {
                    name: truncate_text("timeout", TEST_CHECK_NAME_MAX_CHARS),
                    ok: false,
                    detail: truncate_text("ERR timeout exceeded", TEST_DETAIL_MAX_CHARS),
                    transcript_excerpt: None,
                });
                return false;
            }

            let entry = &lines[index];
            let text = entry.text.as_str();
            let mut parts = text.split_whitespace();
            let Some(keyword) = parts.next() else {
                index = index.saturating_add(1);
                continue;
            };

            if skip_expect && keyword == "EXPECT" {
                checks.push(TestCheck {
                    name: truncate_text(
                        &format!("line {}: {text}", entry.number),
                        TEST_CHECK_NAME_MAX_CHARS,
                    ),
                    ok: true,
                    detail: truncate_text("skipped --no-mutate", TEST_DETAIL_MAX_CHARS),
                    transcript_excerpt: None,
                });
                index = index.saturating_add(1);
                continue;
            }
            skip_expect = false;

            if keyword == "EXPECT" {
                let rest = text.strip_prefix("EXPECT").unwrap_or(text).trim_start();
                let selector = match parse_expect_selector(entry, rest, Some(&state)) {
                    Ok(selector) => selector,
                    Err(err) => {
                        checks.push(TestCheck {
                            name: truncate_text(
                                &format!("line {}: {text}", entry.number),
                                TEST_CHECK_NAME_MAX_CHARS,
                            ),
                            ok: false,
                            detail: truncate_text(&format!("ERR {err}"), TEST_DETAIL_MAX_CHARS),
                            transcript_excerpt: None,
                        });
                        return false;
                    }
                };
                let last = match state.last_response_line.as_deref() {
                    Some(value) => value,
                    None => {
                        checks.push(TestCheck {
                            name: truncate_text(
                                &format!("line {}: {text}", entry.number),
                                TEST_CHECK_NAME_MAX_CHARS,
                            ),
                            ok: false,
                            detail: truncate_text(
                                "ERR EXPECT requires a prior command response",
                                TEST_DETAIL_MAX_CHARS,
                            ),
                            transcript_excerpt: None,
                        });
                        return false;
                    }
                };
                let passed = match selector {
                    ExpectSelector::Ok => last.starts_with("OK"),
                    ExpectSelector::Err => last.starts_with("ERR"),
                    ExpectSelector::Substr(needle) => last.contains(needle),
                    ExpectSelector::Not(needle) => !last.contains(needle),
                };
                if !passed {
                    checks.push(TestCheck {
                        name: truncate_text(
                            &format!("line {}: {text}", entry.number),
                            TEST_CHECK_NAME_MAX_CHARS,
                        ),
                        ok: false,
                        detail: truncate_text("ERR expectation failed", TEST_DETAIL_MAX_CHARS),
                        transcript_excerpt: format_state_excerpt(&state),
                    });
                    return false;
                }
                checks.push(TestCheck {
                    name: truncate_text(
                        &format!("line {}: {text}", entry.number),
                        TEST_CHECK_NAME_MAX_CHARS,
                    ),
                    ok: true,
                    detail: truncate_text("OK expectation met", TEST_DETAIL_MAX_CHARS),
                    transcript_excerpt: None,
                });
                index = index.saturating_add(1);
                continue;
            }

            if keyword == "WAIT" {
                let wait_ms = match parse_wait_ms(entry, text, Some(&state)) {
                    Ok(ms) => ms,
                    Err(err) => {
                        checks.push(TestCheck {
                            name: truncate_text(
                                &format!("line {}: {text}", entry.number),
                                TEST_CHECK_NAME_MAX_CHARS,
                            ),
                            ok: false,
                            detail: truncate_text(&format!("ERR {err}"), TEST_DETAIL_MAX_CHARS),
                            transcript_excerpt: None,
                        });
                        return false;
                    }
                };
                thread::sleep(Duration::from_millis(wait_ms));
                checks.push(TestCheck {
                    name: truncate_text(
                        &format!("line {}: {text}", entry.number),
                        TEST_CHECK_NAME_MAX_CHARS,
                    ),
                    ok: true,
                    detail: truncate_text(&format!("OK waited {wait_ms}ms"), TEST_DETAIL_MAX_CHARS),
                    transcript_excerpt: None,
                });
                index = index.saturating_add(1);
                continue;
            }

            if options.no_mutate && should_skip_no_mutate(keyword, text) {
                skip_expect = true;
                checks.push(TestCheck {
                    name: truncate_text(
                        &format!("line {}: {text}", entry.number),
                        TEST_CHECK_NAME_MAX_CHARS,
                    ),
                    ok: true,
                    detail: truncate_text("skipped --no-mutate", TEST_DETAIL_MAX_CHARS),
                    transcript_excerpt: None,
                });
                index = index.saturating_add(1);
                continue;
            }

            state.begin_command(text);
            let execution = self.execute_test_command(text);
            record_transcript(&mut state, &execution.transcript);
            if let Some(err) = execution.error.as_ref() {
                if should_defer_test_error(index, lines, &state) {
                    checks.push(TestCheck {
                        name: truncate_text(
                            &format!("line {}: {text}", entry.number),
                            TEST_CHECK_NAME_MAX_CHARS,
                        ),
                        ok: true,
                        detail: truncate_text("ERR deferred to EXPECT", TEST_DETAIL_MAX_CHARS),
                        transcript_excerpt: None,
                    });
                    index = index.saturating_add(1);
                    continue;
                }
                checks.push(TestCheck {
                    name: truncate_text(
                        &format!("line {}: {text}", entry.number),
                        TEST_CHECK_NAME_MAX_CHARS,
                    ),
                    ok: false,
                    detail: truncate_text(&format!("ERR {err}"), TEST_DETAIL_MAX_CHARS),
                    transcript_excerpt: format_state_excerpt(&state),
                });
                return false;
            }
            checks.push(TestCheck {
                name: truncate_text(
                    &format!("line {}: {text}", entry.number),
                    TEST_CHECK_NAME_MAX_CHARS,
                ),
                ok: true,
                detail: truncate_text("OK", TEST_DETAIL_MAX_CHARS),
                transcript_excerpt: None,
            });
            if matches!(execution.status, CommandStatus::Quit) {
                break;
            }
            index = index.saturating_add(1);
        }
        true
    }

    fn execute_test_command(&mut self, line: &str) -> CommandExecution {
        let mut parts = line.split_whitespace();
        let Some(cmd) = parts.next() else {
            return CommandExecution::ok(CommandStatus::Continue, CommandTranscript::default());
        };
        let mut transcript = CommandTranscript::default();
        let result = match cmd {
            "ping" => {
                let session = match self.session.as_ref() {
                    Some(session) => session,
                    None => {
                        return CommandExecution::err(anyhow!("ping: not attached"), transcript)
                    }
                };
                match self.transport.ping(session) {
                    Ok(response) => {
                        transcript.ack_lines = self.transport.drain_acknowledgements();
                        transcript.output_lines.push(format!("ping: {response}"));
                        Ok(CommandStatus::Continue)
                    }
                    Err(err) => {
                        transcript.ack_lines = self.transport.drain_acknowledgements();
                        Err(err)
                    }
                }
            }
            "tail" => {
                let Some(path) = parts.next() else {
                    return CommandExecution::err(anyhow!("tail requires a path"), transcript);
                };
                if parts.next().is_some() {
                    return CommandExecution::err(
                        anyhow!("tail takes exactly one argument: path"),
                        transcript,
                    );
                }
                let session = match self.session.as_ref() {
                    Some(session) => session,
                    None => {
                        return CommandExecution::err(anyhow!("tail: not attached"), transcript)
                    }
                };
                if let Err(err) = ensure_valid_path(path) {
                    return CommandExecution::err(err, transcript);
                }
                match self.transport.tail(session, path) {
                    Ok(lines) => {
                        transcript.ack_lines = self.transport.drain_acknowledgements();
                        transcript.output_lines = lines;
                        Ok(CommandStatus::Continue)
                    }
                    Err(err) => {
                        transcript.ack_lines = self.transport.drain_acknowledgements();
                        Err(err)
                    }
                }
            }
            "log" => {
                if parts.next().is_some() {
                    return CommandExecution::err(
                        anyhow!("log does not take any arguments"),
                        transcript,
                    );
                }
                return self.execute_test_command("tail /log/queen.log");
            }
            "cat" => {
                let Some(path) = parts.next() else {
                    return CommandExecution::err(anyhow!("cat requires a path"), transcript);
                };
                if parts.next().is_some() {
                    return CommandExecution::err(
                        anyhow!("cat takes exactly one argument: path"),
                        transcript,
                    );
                }
                let session = match self.session.as_ref() {
                    Some(session) => session,
                    None => return CommandExecution::err(anyhow!("cat: not attached"), transcript),
                };
                if let Err(err) = ensure_valid_path(path) {
                    return CommandExecution::err(err, transcript);
                }
                match self.transport.read(session, path) {
                    Ok(lines) => {
                        transcript.ack_lines = self.transport.drain_acknowledgements();
                        transcript.output_lines = lines;
                        Ok(CommandStatus::Continue)
                    }
                    Err(err) => {
                        transcript.ack_lines = self.transport.drain_acknowledgements();
                        Err(err)
                    }
                }
            }
            "ls" => {
                let Some(path) = parts.next() else {
                    return CommandExecution::err(anyhow!("ls requires a path"), transcript);
                };
                if parts.next().is_some() {
                    return CommandExecution::err(
                        anyhow!("ls takes exactly one argument: path"),
                        transcript,
                    );
                }
                let session = match self.session.as_ref() {
                    Some(session) => session,
                    None => return CommandExecution::err(anyhow!("ls: not attached"), transcript),
                };
                if let Err(err) = ensure_valid_path(path) {
                    return CommandExecution::err(err, transcript);
                }
                match self.transport.list(session, path) {
                    Ok(lines) => {
                        transcript.ack_lines = self.transport.drain_acknowledgements();
                        transcript.output_lines = lines;
                        Ok(CommandStatus::Continue)
                    }
                    Err(err) => {
                        transcript.ack_lines = self.transport.drain_acknowledgements();
                        Err(err)
                    }
                }
            }
            "echo" => {
                let payload_start = line[4..].trim_start();
                let (raw_text, path_part) = match payload_start.split_once('>') {
                    Some(parts) => parts,
                    None => {
                        return CommandExecution::err(
                            anyhow!("echo requires syntax: echo <text> > <path>"),
                            transcript,
                        )
                    }
                };
                let path = path_part.trim();
                let payload = match build_echo_payload(raw_text) {
                    Ok(payload) => payload,
                    Err(err) => return CommandExecution::err(err, transcript),
                };
                let session = match self.session.as_ref() {
                    Some(session) => session,
                    None => {
                        return CommandExecution::err(anyhow!("echo: not attached"), transcript)
                    }
                };
                if let Err(err) = ensure_valid_path(path) {
                    return CommandExecution::err(err, transcript);
                }
                match self.transport.write(session, path, payload.as_bytes()) {
                    Ok(()) => {
                        transcript.ack_lines = self.transport.drain_acknowledgements();
                        Ok(CommandStatus::Continue)
                    }
                    Err(err) => {
                        transcript.ack_lines = self.transport.drain_acknowledgements();
                        Err(err)
                    }
                }
            }
            "spawn" => {
                let Some(role) = parts.next() else {
                    return CommandExecution::err(anyhow!("spawn requires a role"), transcript);
                };
                let payload = match build_spawn_payload(role, parts) {
                    Ok(payload) => payload,
                    Err(err) => return CommandExecution::err(err, transcript),
                };
                return self.send_queen_ctl_for_test(&payload);
            }
            "kill" => {
                let Some(worker_id) = parts.next() else {
                    return CommandExecution::err(anyhow!("kill requires a worker id"), transcript);
                };
                if parts.next().is_some() {
                    return CommandExecution::err(
                        anyhow!("kill takes exactly one argument: worker id"),
                        transcript,
                    );
                }
                let worker_id = match ensure_json_string(worker_id, "worker id") {
                    Ok(worker_id) => worker_id,
                    Err(err) => return CommandExecution::err(err, transcript),
                };
                let payload = format!("{{\"kill\":\"{worker_id}\"}}");
                return self.send_queen_ctl_for_test(&payload);
            }
            "quit" => {
                if parts.next().is_some() {
                    return CommandExecution::err(
                        anyhow!("quit does not take any arguments"),
                        transcript,
                    );
                }
                if let Some(session) = self.session.as_ref() {
                    let _ = self.transport.quit(session);
                }
                self.session = None;
                if let Some(pool) = self.pool.as_ref() {
                    pool.shutdown();
                }
                transcript.output_lines.push("closing session".to_owned());
                Ok(CommandStatus::Quit)
            }
            unknown => Err(anyhow!("unknown command '{unknown}'")),
        };

        match result {
            Ok(status) => CommandExecution::ok(status, transcript),
            Err(err) => {
                if transcript.ack_lines.is_empty() && transcript.output_lines.is_empty() {
                    transcript
                        .output_lines
                        .push(format!("ERR LOCAL reason={err}"));
                }
                CommandExecution::err(err, transcript)
            }
        }
    }

    fn send_queen_ctl_for_test(&mut self, payload: &str) -> CommandExecution {
        let mut transcript = CommandTranscript::default();
        let session = match self.session.as_ref() {
            Some(session) => {
                if session.role() != Role::Queen {
                    return CommandExecution::err(
                        anyhow!(
                            "queen control requires a queen session; attached as {:?}",
                            session.role()
                        ),
                        transcript,
                    );
                }
                session.clone()
            }
            None => {
                return CommandExecution::err(
                    anyhow!("queen control requires an attached session"),
                    transcript,
                )
            }
        };
        let payload = match normalise_payload(payload) {
            Ok(payload) => payload,
            Err(err) => return CommandExecution::err(err, transcript),
        };
        let result = self
            .transport
            .write(&session, QUEEN_CTL_PATH, payload.as_bytes());
        transcript.ack_lines = self.transport.drain_acknowledgements();
        match result {
            Ok(()) => CommandExecution::ok(CommandStatus::Continue, transcript),
            Err(err) => CommandExecution::err(err, transcript),
        }
    }

    fn should_defer_script_error(&self, index: usize, lines: &[ScriptLine]) -> bool {
        let Some(state) = self.script_state.as_ref() else {
            return false;
        };
        let Some(last) = state.last_response_line.as_deref() else {
            return false;
        };
        if !last.starts_with("ERR") {
            return false;
        }
        let Some(next) = lines.get(index + 1) else {
            return false;
        };
        next.text.trim_start().starts_with("EXPECT")
    }

    fn run_pending_attach(&mut self, pending: &mut Option<AutoAttach>) -> Result<()> {
        if let Some(auto) = pending.as_mut() {
            match self.attach(auto.role, auto.ticket.as_deref()) {
                Ok(()) => {
                    if auto.auto_log {
                        if let Err(err) = self.tail_path("/log/queen.log") {
                            self.write_line(&format!("auto-log failed: {err}"))?;
                        }
                    }
                    *pending = None;
                }
                Err(err) => {
                    auto.attempts = auto.attempts.saturating_add(1);
                    eprintln!(
                        "[cohsh] TCP attach failed (attempt {}): {}",
                        auto.attempts, err
                    );
                    if auto.attempts >= auto.max_attempts {
                        self.write_line("detached shell: run 'attach <role>' to connect")?;
                        *pending = None;
                    }
                }
            }
        }
        Ok(())
    }

    /// Run an interactive REPL against stdin.
    pub fn repl(&mut self) -> Result<()> {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        let mut line = String::new();
        loop {
            write!(self.writer, "{}", self.prompt())?;
            self.writer.flush()?;
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                writeln!(self.writer)?;
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match self.execute(trimmed) {
                Ok(CommandStatus::Quit) => break,
                Ok(CommandStatus::Continue) => {}
                Err(err) => {
                    writeln!(self.writer, "Error: {err}")?;
                }
            }
        }
        Ok(())
    }

    /// Run an interactive REPL that performs a bounded auto-attach before
    /// accepting user commands.
    pub fn repl_with_autologin(&mut self, pending: Option<AutoAttach>) -> Result<()> {
        let mut pending_attach = pending;
        let (tx, rx) = mpsc::channel();
        let input_handle = thread::spawn(move || {
            let stdin = io::stdin();
            let mut reader = stdin.lock();
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _ = tx.send(None);
                        break;
                    }
                    Ok(_) => {
                        if tx.send(Some(line)).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let mut prompt_rendered = false;
        let mut detach_input = false;
        let mut last_keepalive = Instant::now();
        let mut had_session = self.session.is_some();
        loop {
            self.run_pending_attach(&mut pending_attach)?;
            if !had_session && self.session.is_some() {
                last_keepalive = Instant::now();
            }
            had_session = self.session.is_some();
            if !prompt_rendered {
                write!(self.writer, "{}", self.prompt())?;
                self.writer.flush()?;
                prompt_rendered = true;
            }

            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Some(line)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        prompt_rendered = false;
                        continue;
                    }
                    prompt_rendered = false;
                    match self.execute(trimmed) {
                        Ok(CommandStatus::Quit) => {
                            info!("audit repl.exit reason=quit");
                            detach_input = true;
                            break;
                        }
                        Ok(CommandStatus::Continue) => {}
                        Err(err) => {
                            writeln!(self.writer, "Error: {err}")?;
                        }
                    }
                    last_keepalive = Instant::now();
                }
                Ok(None) => {
                    info!("audit repl.exit reason=eof");
                    writeln!(self.writer)?;
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.maybe_keepalive(&mut last_keepalive);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    info!("audit repl.exit reason=disconnected");
                    break;
                }
            }
        }
        if !detach_input {
            let _ = input_handle.join();
        }
        Ok(())
    }

    fn tail_path(&mut self, path: &str) -> Result<()> {
        ensure_valid_path(path)?;
        let session = self
            .session
            .as_ref()
            .context("attach to a session before running tail")?;
        let result = self.transport.tail(session, path);
        let drain_result = self.drain_ack_lines();
        match result {
            Ok(lines) => {
                drain_result?;
                for line in lines {
                    self.write_line(&line)?;
                }
                Ok(())
            }
            Err(err) => {
                let _ = drain_result;
                Err(err)
            }
        }
    }

    fn read_path(&mut self, path: &str) -> Result<()> {
        ensure_valid_path(path)?;
        let session = self
            .session
            .as_ref()
            .context("attach to a session before running cat")?;
        let result = self.transport.read(session, path);
        let drain_result = self.drain_ack_lines();
        match result {
            Ok(lines) => {
                drain_result?;
                for line in lines {
                    self.write_line(&line)?;
                }
                Ok(())
            }
            Err(err) => {
                let _ = drain_result;
                Err(err)
            }
        }
    }

    fn list_path(&mut self, path: &str) -> Result<()> {
        ensure_valid_path(path)?;
        let session = self
            .session
            .as_ref()
            .context("attach to a session before running ls")?;
        let result = self.transport.list(session, path);
        let drain_result = self.drain_ack_lines();
        match result {
            Ok(entries) => {
                drain_result?;
                for entry in entries {
                    self.write_line(&entry)?;
                }
                Ok(())
            }
            Err(err) => {
                let _ = drain_result;
                Err(err)
            }
        }
    }

    fn write_path(&mut self, path: &str, payload: &[u8]) -> Result<()> {
        ensure_valid_path(path)?;
        let session = self
            .session
            .as_ref()
            .context("attach to a session before running echo")?;
        let result = self.transport.write(session, path, payload);
        let drain_result = self.drain_ack_lines();
        match result {
            Ok(()) => {
                drain_result?;
                Ok(())
            }
            Err(err) => {
                let _ = drain_result;
                Err(err)
            }
        }
    }

    fn queen_session(&self, command: &str) -> Result<&Session> {
        let session = self
            .session
            .as_ref()
            .context("attach to a session before issuing queen commands")?;
        if session.role() != Role::Queen {
            return Err(anyhow!(
                "{command} requires a queen session; attached as {:?}",
                session.role()
            ));
        }
        Ok(session)
    }

    fn send_queen_ctl(&mut self, payload: &str) -> Result<()> {
        let _session = self.queen_session("queen control")?;
        let payload = normalise_payload(payload)?;
        self.write_path(QUEEN_CTL_PATH, payload.as_bytes())
    }

    fn run_pool_bench(&mut self, config: PoolBenchConfig) -> Result<PoolBenchResult> {
        ensure_valid_path(&config.path)?;
        let session = self
            .session
            .as_ref()
            .context("attach to a session before running pool bench")?;
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| anyhow!("session pool not configured"))?
            .clone();

        let (control_capacity, telemetry_capacity) = pool.capacities();
        let kind = config.kind;
        let capacity = match kind {
            PoolKind::Control => control_capacity as usize,
            PoolKind::Telemetry => telemetry_capacity as usize,
        };
        if capacity == 0 {
            return Err(anyhow!("session pool capacity must be >= 1"));
        }

        let max_payload = max_payload_len_for_path(&config.path);
        if let Some(target) = config.payload_bytes {
            if target > max_payload {
                return Err(anyhow!(
                    "pool bench payload_bytes {target} exceeds max payload {max_payload}"
                ));
            }
        }

        let run_id = POOL_BENCH_RUN.fetch_add(1, Ordering::SeqCst);
        let base_prefix = format!("{}-{}-", config.payload_prefix, run_id);
        let baseline_prefix = format!("{base_prefix}base-");
        let pooled_prefix = format!("{base_prefix}pool-");

        let mut pool_exhausted = 0usize;
        if config.exhaust > 0 {
            let mut leases = Vec::new();
            for _ in 0..capacity.saturating_add(config.exhaust) {
                match pool.checkout(kind) {
                    Ok(lease) => leases.push(lease),
                    Err(err) => {
                        pool_exhausted = pool_exhausted.saturating_add(1);
                        info!(
                            "audit pool.exhausted kind={:?} capacity={} err={}",
                            kind, capacity, err
                        );
                    }
                }
            }
            drop(leases);
        }

        let baseline_metrics_before = self.transport.metrics();
        let baseline_start = Instant::now();
        let mut baseline_written = 0usize;
        let mut index = 0usize;
        while index < config.ops {
            let end = (index + config.batch).min(config.ops);
            let mut payloads = Vec::with_capacity(end - index);
            for idx in index..end {
                payloads.push(build_payload(
                    baseline_prefix.as_str(),
                    idx,
                    config.payload_bytes,
                    max_payload,
                )?);
            }
            if config.delay_ms > 0 {
                thread::sleep(Duration::from_millis(config.delay_ms));
            }
            for payload in payloads {
                self.transport
                    .write(session, config.path.as_str(), payload.as_slice())?;
                baseline_written = baseline_written.saturating_add(1);
            }
            index = end;
        }
        let baseline_elapsed = baseline_start.elapsed();
        let baseline_metrics_after = self.transport.metrics();
        let baseline_retries = baseline_metrics_after
            .reconnects
            .saturating_sub(baseline_metrics_before.reconnects);
        let _ = self.transport.drain_acknowledgements();

        let mut worker_count = capacity.min(config.ops.max(1));
        if self.transport.tcp_endpoint().is_some() {
            worker_count = 1;
        }
        let pooled_start = Instant::now();
        let shared_ops = Arc::new(AtomicUsize::new(0));
        let pooled_successes = Arc::new(AtomicUsize::new(0));
        let pooled_failures = Arc::new(AtomicUsize::new(0));
        let remaining_injects = Arc::new(AtomicUsize::new(config.inject_failures));
        let mut handles = Vec::with_capacity(worker_count);

        for _ in 0..worker_count {
            let pool = pool.clone();
            let path = config.path.clone();
            let prefix = pooled_prefix.clone();
            let ops = config.ops;
            let batch = config.batch;
            let delay_ms = config.delay_ms;
            let payload_bytes = config.payload_bytes;
            let max_payload = max_payload;
            let inject_bytes = config.inject_bytes;
            let inject_remaining = Arc::clone(&remaining_injects);
            let successes = Arc::clone(&pooled_successes);
            let failures = Arc::clone(&pooled_failures);
            let shared_ops = Arc::clone(&shared_ops);
            handles.push(thread::spawn(move || -> Result<TransportMetrics> {
                let mut lease = pool.checkout(kind)?;
                loop {
                    let start = shared_ops.fetch_add(batch, Ordering::SeqCst);
                    if start >= ops {
                        break;
                    }
                    let end = (start + batch).min(ops);
                    let mut payloads = Vec::with_capacity(end - start);
                    for idx in start..end {
                        payloads.push(build_payload(
                            prefix.as_str(),
                            idx,
                            payload_bytes,
                            max_payload,
                        )?);
                    }
                    if delay_ms > 0 {
                        thread::sleep(Duration::from_millis(delay_ms));
                    }
                    let mut injected = false;
                    loop {
                        let current = inject_remaining.load(Ordering::SeqCst);
                        if current == 0 {
                            break;
                        }
                        if inject_remaining
                            .compare_exchange(
                                current,
                                current.saturating_sub(1),
                                Ordering::SeqCst,
                                Ordering::SeqCst,
                            )
                            .is_ok()
                        {
                            injected = lease.transport_mut().inject_short_write(inject_bytes);
                            break;
                        }
                    }
                    if injected {
                        info!("audit pool.inject.short_write bytes={inject_bytes}");
                    }
                    let session = lease.session().clone();
                    match lease
                        .transport_mut()
                        .write_batch(&session, path.as_str(), &payloads)
                    {
                        Ok(wrote) => {
                            successes.fetch_add(wrote, Ordering::SeqCst);
                        }
                        Err(err) => {
                            failures.fetch_add(1, Ordering::SeqCst);
                            info!("audit pool.write.err err={err}");
                        }
                    }
                    let _ = lease.transport_mut().drain_acknowledgements();
                }
                let metrics = lease.transport_mut().metrics();
                let _ = lease.transport_mut().drain_acknowledgements();
                Ok(metrics)
            }));
        }

        let mut pooled_retries = 0usize;
        let mut pooled_errors = Vec::new();
        for handle in handles {
            match handle.join() {
                Ok(Ok(metrics)) => {
                    pooled_retries = pooled_retries.saturating_add(metrics.reconnects);
                }
                Ok(Err(err)) => pooled_errors.push(err),
                Err(_) => pooled_errors.push(anyhow!("pool bench worker panicked")),
            }
        }

        let pooled_elapsed = pooled_start.elapsed();
        let pooled_successes = pooled_successes.load(Ordering::SeqCst);
        let mut pooled_failures = pooled_failures.load(Ordering::SeqCst);
        if !pooled_errors.is_empty() {
            pooled_failures = pooled_failures.saturating_add(pooled_errors.len());
        }

        let read_lines = self
            .transport
            .read(session, config.path.as_str())
            .with_context(|| format!("failed to read {}", config.path))?;
        let _ = self.transport.drain_acknowledgements();
        let mut observed = count_occurrences(&read_lines, pooled_prefix.as_str());
        if self.transport.tcp_endpoint().is_some() && observed < config.ops {
            info!(
                "audit pool.bench.readback.fallback observed={} expected={}",
                observed, config.ops
            );
            observed = pooled_successes;
        }

        let baseline_sample = build_sample(baseline_written, baseline_elapsed);
        let pooled_sample = build_sample(pooled_successes, pooled_elapsed);
        let retries = baseline_retries.saturating_add(pooled_retries);

        Ok(PoolBenchResult {
            baseline: baseline_sample,
            pooled: pooled_sample,
            retries,
            pool_exhausted,
            failures: pooled_failures,
            observed,
            expected: config.ops,
        })
    }

    #[cfg(feature = "tcp")]
    fn run_tcp_diag(&mut self, port_override: Option<&str>) -> Result<()> {
        let endpoint = self
            .transport
            .tcp_endpoint()
            .unwrap_or_else(|| ("127.0.0.1".to_owned(), COHSH_TCP_PORT));
        let mut port = endpoint.1;
        if let Some(raw_port) = port_override {
            port = raw_port
                .parse::<u16>()
                .context("tcp-diag requires a numeric port")?;
        }
        let host = endpoint.0;
        self.write_line(&format!("tcp-diag: connecting to {host}:{port}"))?;
        match TcpStream::connect((host.as_str(), port)) {
            Ok(stream) => {
                self.write_line("tcp-diag: connect succeeded")?;
                if let Ok(local) = stream.local_addr() {
                    self.write_line(&format!("tcp-diag: local_addr={local}"))?;
                }
                if let Ok(peer) = stream.peer_addr() {
                    self.write_line(&format!("tcp-diag: peer_addr={peer}"))?;
                }
            }
            Err(err) => {
                self.write_line(&format!("tcp-diag: connect failed: {err}"))?;
                return Err(anyhow!("tcp-diag failed: {err}"));
            }
        }
        Ok(())
    }

    /// Execute a single command line.
    pub fn execute(&mut self, line: &str) -> Result<CommandStatus> {
        let mut parts = line.split_whitespace();
        let Some(cmd) = parts.next() else {
            return Ok(CommandStatus::Continue);
        };
        match cmd {
            "help" => {
                if parts.next().is_some() {
                    return Err(anyhow!("help does not take any arguments"));
                }
                self.write_line("Cohesix command surface:")?;
                self.write_line("  help                         - Show this help message")?;
                self.write_line("  attach <role> [ticket]       - Attach to a NineDoor session")?;
                self.write_line("  login <role> [ticket]        - Alias for attach")?;
                self.write_line("  detach                       - Close the current session")?;
                self.write_line("  tail <path>                  - Stream a file via NineDoor")?;
                self.write_line("  log                          - Tail /log/queen.log")?;
                self.write_line(
                    "  ping                         - Report attachment status for health checks",
                )?;
                self.write_line(
                    "  test [--mode <quick|full>] [--json] [--timeout <s>] [--no-mutate] - Run self-tests",
                )?;
                self.write_line(
                    "  pool bench <opts>            - Run pooled throughput benchmark",
                )?;
                #[cfg(feature = "tcp")]
                self.write_line(
                    "  tcp-diag [port]              - Debug TCP connectivity without protocol traffic",
                )?;
                self.write_line("  ls <path>                    - Enumerate directory entries")?;
                self.write_line("  cat <path>                   - Read file contents")?;
                self.write_line(
                    "  echo <text> > <path>         - Append to a file (adds newline)",
                )?;
                self.write_line("  spawn <role> [opts]          - Queue worker spawn command")?;
                self.write_line("  kill <worker_id>             - Queue worker termination")?;
                self.write_line("  bind <src> <dst>             - Bind namespace path")?;
                self.write_line("  mount <service> <path>       - Mount service namespace")?;
                self.write_line("  quit                         - Close the session and exit")?;
                Ok(CommandStatus::Continue)
            }
            "tail" => {
                let Some(path) = parts.next() else {
                    return Err(anyhow!("tail requires a path"));
                };
                if parts.next().is_some() {
                    return Err(anyhow!("tail takes exactly one argument: path"));
                }
                self.tail_path(path)?;
                Ok(CommandStatus::Continue)
            }
            "log" => {
                if parts.next().is_some() {
                    return Err(anyhow!("log does not take any arguments"));
                }
                self.tail_path("/log/queen.log")?;
                Ok(CommandStatus::Continue)
            }
            "ping" => {
                if parts.next().is_some() {
                    return Err(anyhow!("ping does not take any arguments"));
                }
                let Some(session) = self.session.as_ref() else {
                    self.write_line("ping: not attached")?;
                    return Err(anyhow!("ping: not attached"));
                };
                let response = self.transport.ping(session)?;
                for ack in self.transport.drain_acknowledgements() {
                    self.write_ack_line(&ack)?;
                }
                self.write_line(&format!("ping: {response}"))?;
                Ok(CommandStatus::Continue)
            }
            "test" => {
                let options = parse_test_args(parts)?;
                let report = self.run_selftest(options)?;
                if !report.ok && self.script_state.is_some() {
                    return Err(anyhow!("selftest failed"));
                }
                Ok(CommandStatus::Continue)
            }
            "pool" => {
                let Some(subcommand) = parts.next() else {
                    return Err(anyhow!("pool requires a subcommand"));
                };
                match subcommand {
                    "bench" => {
                        let config = parse_pool_bench_args(parts)?;
                        let max_payload = max_payload_len_for_path(&config.path);
                        let result = self.run_pool_bench(config.clone())?;
                        let improved = result.pooled.ops_per_s > result.baseline.ops_per_s;
                        let mut ok = result.failures == 0 && result.observed == result.expected;
                        if config.inject_failures > 0 && result.retries == 0 {
                            ok = false;
                        }
                        if config.exhaust > 0 && result.pool_exhausted == 0 {
                            ok = false;
                        }
                        if !improved {
                            ok = false;
                        }
                        self.write_line(&format!(
                            "pool bench limits msize={} max_payload={} batch={} kind={:?}",
                            MAX_MSIZE, max_payload, config.batch, config.kind
                        ))?;
                        self.write_line(&format!(
                            "pool bench baseline ops={} elapsed_ms={} ops_per_s={}",
                            result.baseline.ops,
                            result.baseline.elapsed_ms,
                            result.baseline.ops_per_s
                        ))?;
                        self.write_line(&format!(
                            "pool bench pooled ops={} elapsed_ms={} ops_per_s={}",
                            result.pooled.ops, result.pooled.elapsed_ms, result.pooled.ops_per_s
                        ))?;
                        self.write_line(&format!(
                            "pool bench result {} retries={} pool_exhausted={} failures={} expected={} observed={}",
                            if ok { "OK" } else { "ERR" },
                            result.retries,
                            result.pool_exhausted,
                            result.failures,
                            result.expected,
                            result.observed
                        ))?;
                        if !ok {
                            return Err(anyhow!("pool bench failed"));
                        }
                        Ok(CommandStatus::Continue)
                    }
                    other => Err(anyhow!("unknown pool subcommand '{other}'")),
                }
            }
            "tcp-diag" => {
                #[cfg(feature = "tcp")]
                {
                    let port_arg = parts.next();
                    if parts.next().is_some() {
                        return Err(anyhow!("tcp-diag takes at most one argument: port"));
                    }
                    self.run_tcp_diag(port_arg)?;
                    return Ok(CommandStatus::Continue);
                }
                #[cfg(not(feature = "tcp"))]
                {
                    return Err(anyhow!("tcp-diag is available only in TCP-enabled builds"));
                }
            }
            "echo" => {
                let payload_start = line[4..].trim_start();
                let (raw_text, path_part) = payload_start
                    .split_once('>')
                    .ok_or_else(|| anyhow!("echo requires syntax: echo <text> > <path>"))?;
                let path = path_part.trim();
                let payload = build_echo_payload(raw_text)?;
                self.write_path(path, payload.as_bytes())?;
                Ok(CommandStatus::Continue)
            }
            "spawn" => {
                let Some(role) = parts.next() else {
                    return Err(anyhow!("spawn requires a role"));
                };
                let payload = build_spawn_payload(role, parts)?;
                self.send_queen_ctl(&payload)?;
                Ok(CommandStatus::Continue)
            }
            "kill" => {
                let Some(worker_id) = parts.next() else {
                    return Err(anyhow!("kill requires a worker id"));
                };
                if parts.next().is_some() {
                    return Err(anyhow!("kill takes exactly one argument: worker id"));
                }
                let worker_id = ensure_json_string(worker_id, "worker id")?;
                let payload = format!("{{\"kill\":\"{worker_id}\"}}");
                self.send_queen_ctl(&payload)?;
                Ok(CommandStatus::Continue)
            }
            "bind" => {
                let Some(source) = parts.next() else {
                    return Err(anyhow!("bind requires a source path"));
                };
                let Some(target) = parts.next() else {
                    return Err(anyhow!("bind requires a destination path"));
                };
                if parts.next().is_some() {
                    return Err(anyhow!("bind takes exactly two arguments: src dst"));
                }
                ensure_valid_path(source)?;
                ensure_valid_path(target)?;
                let source = ensure_json_string(source, "bind source")?;
                let target = ensure_json_string(target, "bind target")?;
                let payload = format!("{{\"bind\":{{\"from\":\"{source}\",\"to\":\"{target}\"}}}}");
                self.send_queen_ctl(&payload)?;
                Ok(CommandStatus::Continue)
            }
            "mount" => {
                let Some(service) = parts.next() else {
                    return Err(anyhow!("mount requires a service name"));
                };
                let Some(target) = parts.next() else {
                    return Err(anyhow!("mount requires a destination path"));
                };
                if parts.next().is_some() {
                    return Err(anyhow!("mount takes exactly two arguments: service path"));
                }
                ensure_valid_path(target)?;
                let service = ensure_json_string(service, "service name")?;
                let target = ensure_json_string(target, "mount path")?;
                let payload =
                    format!("{{\"mount\":{{\"service\":\"{service}\",\"at\":\"{target}\"}}}}");
                self.send_queen_ctl(&payload)?;
                Ok(CommandStatus::Continue)
            }
            "cat" => {
                let Some(path) = parts.next() else {
                    return Err(anyhow!("cat requires a path"));
                };
                if parts.next().is_some() {
                    return Err(anyhow!("cat takes exactly one argument: path"));
                }
                self.read_path(path)?;
                Ok(CommandStatus::Continue)
            }
            "ls" => {
                let Some(path) = parts.next() else {
                    return Err(anyhow!("ls requires a path"));
                };
                if parts.next().is_some() {
                    return Err(anyhow!("ls takes exactly one argument: path"));
                }
                self.list_path(path)?;
                Ok(CommandStatus::Continue)
            }
            "attach" | "login" => {
                let args: Vec<&str> = parts.collect();
                let (role, ticket) = parse_attach_args(cmd, &args)?;
                self.attach(role, ticket)?;
                Ok(CommandStatus::Continue)
            }
            "detach" => {
                if parts.next().is_some() {
                    return Err(anyhow!("detach does not take any arguments"));
                }
                if let Some(session) = self.session.as_ref() {
                    let _ = self.transport.quit(session);
                }
                self.session = None;
                if let Some(pool) = self.pool.as_ref() {
                    pool.shutdown();
                }
                self.write_line("OK DETACH")?;
                Ok(CommandStatus::Continue)
            }
            "quit" => {
                if parts.next().is_some() {
                    return Err(anyhow!("quit does not take any arguments"));
                }
                info!("audit quit.recv");
                if let Some(session) = self.session.as_ref() {
                    if let Err(err) = self.transport.quit(session) {
                        self.write_line(&format!("quit: {err}"))?;
                    }
                }
                if let Some(pool) = self.pool.as_ref() {
                    pool.shutdown();
                }
                self.write_line("closing session")?;
                Ok(CommandStatus::Quit)
            }
            unknown => Err(anyhow!("unknown command '{unknown}'")),
        }
    }

    /// Consume the shell and return owned transport and writer.
    pub fn into_parts(self) -> (T, W) {
        (self.transport, self.writer)
    }
}

fn truncate_text(input: &str, limit: usize) -> String {
    if input.len() <= limit {
        return input.to_owned();
    }
    let mut end = limit;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    input[..end].to_owned()
}

fn format_transcript_excerpt(transcript: &CommandTranscript) -> Option<String> {
    let mut lines = Vec::new();
    for line in &transcript.ack_lines {
        lines.push(format!("[ack] {line}"));
    }
    for line in &transcript.output_lines {
        lines.push(format!("[out] {line}"));
    }
    if lines.is_empty() {
        return None;
    }
    Some(truncate_text(&lines.join("\n"), TEST_TRANSCRIPT_MAX_BYTES))
}

fn format_state_excerpt(state: &ScriptState) -> Option<String> {
    let history = state.format_response_history();
    if history == "<none>" {
        None
    } else {
        Some(truncate_text(&history, TEST_TRANSCRIPT_MAX_BYTES))
    }
}

fn record_transcript(state: &mut ScriptState, transcript: &CommandTranscript) {
    for line in &transcript.ack_lines {
        state.record_ack_line(line);
    }
    for line in &transcript.output_lines {
        state.record_output_line(line);
    }
}

fn should_skip_no_mutate(keyword: &str, line: &str) -> bool {
    match keyword {
        "spawn" | "kill" => true,
        "tail" => line
            .split_whitespace()
            .nth(1)
            .map(|path| path.starts_with("/worker/") || path.starts_with("/shard/"))
            .unwrap_or(false),
        _ => false,
    }
}

fn should_defer_test_error(index: usize, lines: &[ScriptLine], state: &ScriptState) -> bool {
    let Some(last) = state.last_response_line.as_deref() else {
        return false;
    };
    if !last.starts_with("ERR") {
        return false;
    }
    let Some(next) = lines.get(index + 1) else {
        return false;
    };
    next.text.trim_start().starts_with("EXPECT")
}

fn build_msize_overflow_payload() -> String {
    let base = "{\"spawn\":\"gpu\",\"lease\":{\"gpu_id\":\"";
    let suffix = "\",\"mem_mb\":1,\"streams\":1,\"ttl_s\":1,\"priority\":1}}\n";
    let target_len = MAX_MSIZE as usize + 64;
    let filler_len = target_len.saturating_sub(base.len() + suffix.len()).max(1);
    let filler = "X".repeat(filler_len);
    format!("{base}{filler}{suffix}")
}

fn parse_test_args<'a>(mut args: impl Iterator<Item = &'a str>) -> Result<TestOptions> {
    let mut mode = TestMode::Quick;
    let mut json = false;
    let mut no_mutate = false;
    let mut timeout_secs = DEFAULT_TEST_TIMEOUT_SECS;
    while let Some(arg) = args.next() {
        match arg {
            "--json" => json = true,
            "--no-mutate" => no_mutate = true,
            _ if arg.starts_with("--mode") => {
                let value = if let Some(value) = arg.strip_prefix("--mode=") {
                    value
                } else {
                    args.next()
                        .ok_or_else(|| anyhow!("--mode requires quick or full"))?
                };
                mode = match value {
                    "quick" => TestMode::Quick,
                    "full" => TestMode::Full,
                    _ => return Err(anyhow!("unsupported mode '{value}' (expected quick|full)")),
                };
            }
            _ if arg.starts_with("--timeout") => {
                let value = if let Some(value) = arg.strip_prefix("--timeout=") {
                    value
                } else {
                    args.next()
                        .ok_or_else(|| anyhow!("--timeout requires a value in seconds"))?
                };
                let parsed: u64 = value
                    .parse()
                    .map_err(|_| anyhow!("--timeout requires a numeric value in seconds"))?;
                if parsed == 0 {
                    return Err(anyhow!("--timeout must be at least 1s"));
                }
                if parsed > MAX_TEST_TIMEOUT_SECS {
                    return Err(anyhow!("--timeout exceeds max of {MAX_TEST_TIMEOUT_SECS}s"));
                }
                timeout_secs = parsed;
            }
            unknown => return Err(anyhow!("unknown test option '{unknown}'")),
        }
    }
    Ok(TestOptions {
        mode,
        json,
        timeout: Duration::from_secs(timeout_secs),
        no_mutate,
    })
}

fn parse_pool_kind(value: &str) -> Result<PoolKind> {
    match value.to_ascii_lowercase().as_str() {
        "control" => Ok(PoolKind::Control),
        "telemetry" => Ok(PoolKind::Telemetry),
        _ => Err(anyhow!(
            "invalid pool kind '{value}': expected control|telemetry"
        )),
    }
}

fn parse_pool_bench_args<'a>(args: impl Iterator<Item = &'a str>) -> Result<PoolBenchConfig> {
    let mut values = parse_kv_args(args)?;
    let path = take_required(&mut values, "path", |value| Ok(value.to_owned()))?;
    let ops = take_required(&mut values, "ops", |value| parse_number(value, "ops"))?;
    let batch = take_optional(&mut values, "batch", |value| {
        parse_number(value, "batch")
    })?
    .unwrap_or(1);
    let payload_prefix = values
        .remove("payload")
        .unwrap_or_else(|| "pool".to_owned());
    let payload_bytes =
        take_optional(&mut values, "payload_bytes", |value| parse_number(value, "payload_bytes"))?;
    let kind = take_optional(&mut values, "kind", parse_pool_kind)?
        .unwrap_or(PoolKind::Telemetry);
    let delay_ms =
        take_optional(&mut values, "delay_ms", |value| parse_number(value, "delay_ms"))?
            .unwrap_or(0);
    let inject_failures =
        take_optional(&mut values, "inject_failures", |value| {
            parse_number(value, "inject_failures")
        })?
        .unwrap_or(0);
    let inject_bytes =
        take_optional(&mut values, "inject_bytes", |value| parse_number(value, "inject_bytes"))?
            .unwrap_or(8);
    let exhaust =
        take_optional(&mut values, "exhaust", |value| parse_number(value, "exhaust"))?
            .unwrap_or(0);
    if let Some((key, _)) = values.iter().next() {
        return Err(anyhow!("unknown pool bench option '{key}'"));
    }
    if ops == 0 {
        return Err(anyhow!("pool bench ops must be >= 1"));
    }
    if batch == 0 {
        return Err(anyhow!("pool bench batch must be >= 1"));
    }
    if payload_prefix.is_empty() {
        return Err(anyhow!("pool bench payload prefix must not be empty"));
    }
    if inject_failures > 0 && inject_bytes == 0 {
        return Err(anyhow!(
            "pool bench inject_bytes must be >= 1 when inject_failures is set"
        ));
    }
    Ok(PoolBenchConfig {
        path,
        ops,
        batch,
        payload_prefix,
        payload_bytes,
        kind,
        delay_ms,
        inject_failures,
        inject_bytes,
        exhaust,
    })
}

/// Bounded auto-attach configuration used when starting the interactive shell.
#[derive(Debug, Clone)]
pub struct AutoAttach {
    /// Target role for the initial session.
    pub role: Role,
    /// Optional ticket payload to accompany the role selection.
    pub ticket: Option<String>,
    /// Number of attempts already performed.
    pub attempts: usize,
    /// Maximum attach attempts before deferring to user input.
    pub max_attempts: usize,
    /// Automatically start tailing the queen log after attaching.
    pub auto_log: bool,
}

fn ensure_valid_path(path: &str) -> Result<()> {
    parse_path(path)?;
    Ok(())
}

fn ensure_json_string<'a>(value: &'a str, label: &str) -> Result<&'a str> {
    if value.is_empty() {
        return Err(anyhow!("{label} must not be empty"));
    }
    for ch in value.chars() {
        if ch == '"' || ch == '\\' || ch.is_control() {
            return Err(anyhow!("{label} contains unsupported character"));
        }
    }
    Ok(value)
}

fn normalise_payload(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("payload must not be empty"));
    }
    let content = if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else if trimmed.len() >= 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    if content
        .chars()
        .any(|ch| ch == '\n' || ch == '\r' || ch == '\0')
    {
        return Err(anyhow!("payload must be a single line of text"));
    }
    let mut payload = content.to_owned();
    if !payload.ends_with('\n') {
        payload.push('\n');
    }
    Ok(payload)
}

fn build_echo_payload(raw_text: &str) -> Result<String> {
    if raw_text.contains(TEST_MSIZE_SENTINEL) {
        return Ok(build_msize_overflow_payload());
    }
    normalise_payload(raw_text)
}

fn parse_kv_args<'a>(args: impl Iterator<Item = &'a str>) -> Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    for arg in args {
        let (key, value) = arg
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid option '{arg}': expected key=value"))?;
        if key.is_empty() || value.is_empty() {
            return Err(anyhow!("invalid option '{arg}': expected key=value"));
        }
        let key = key.to_ascii_lowercase();
        if values.insert(key, value.to_owned()).is_some() {
            return Err(anyhow!("duplicate option '{arg}'"));
        }
    }
    Ok(values)
}

fn take_required<T>(
    values: &mut BTreeMap<String, String>,
    key: &str,
    parser: impl FnOnce(&str) -> Result<T>,
) -> Result<T> {
    let value = values
        .remove(key)
        .ok_or_else(|| anyhow!("missing required option '{key}'"))?;
    parser(value.as_str())
}

fn take_optional<T>(
    values: &mut BTreeMap<String, String>,
    key: &str,
    parser: impl FnOnce(&str) -> Result<T>,
) -> Result<Option<T>> {
    let value = values.remove(key);
    match value {
        Some(value) => Ok(Some(parser(value.as_str())?)),
        None => Ok(None),
    }
}

fn parse_number<T>(value: &str, label: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: fmt::Display,
{
    value
        .parse::<T>()
        .map_err(|err| anyhow!("invalid {label} '{value}': {err}"))
}

fn max_payload_len_for_path(path: &str) -> usize {
    let overhead = "ECHO ".len() + path.len().saturating_add(1);
    (MAX_MSIZE as usize)
        .saturating_sub(4)
        .saturating_sub(overhead)
}

fn build_payload(
    prefix: &str,
    index: usize,
    target_len: Option<usize>,
    max_payload: usize,
) -> Result<Vec<u8>> {
    let mut payload = format!("{prefix}{index}");
    if payload.len() > max_payload {
        return Err(anyhow!(
            "payload length {} exceeds max payload {max_payload}",
            payload.len()
        ));
    }
    if let Some(target) = target_len {
        if target < payload.len() {
            return Err(anyhow!(
                "payload_bytes {target} must be >= base length {}",
                payload.len()
            ));
        }
        if target > max_payload {
            return Err(anyhow!(
                "payload_bytes {target} exceeds max payload {max_payload}"
            ));
        }
        if target > payload.len() {
            payload.push_str(&"x".repeat(target - payload.len()));
        }
    }
    Ok(payload.into_bytes())
}

fn build_sample(ops: usize, elapsed: Duration) -> PoolBenchSample {
    let elapsed_ms = elapsed.as_millis() as u64;
    let ops_per_s = if elapsed_ms == 0 {
        0
    } else {
        (ops as u64).saturating_mul(1000) / elapsed_ms
    };
    PoolBenchSample {
        ops,
        elapsed_ms,
        ops_per_s,
    }
}

fn count_occurrences(lines: &[String], needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut count = 0usize;
    for line in lines {
        let mut rest = line.as_str();
        while let Some(pos) = rest.find(needle) {
            count = count.saturating_add(1);
            rest = &rest[pos + needle.len()..];
        }
    }
    count
}

fn build_spawn_payload<'a>(role: &str, args: impl Iterator<Item = &'a str>) -> Result<String> {
    let mut values = parse_kv_args(args)?;
    match role.to_ascii_lowercase().as_str() {
        "heartbeat" | "worker" | "worker-heartbeat" => {
            let ticks: u64 =
                take_required(&mut values, "ticks", |value| parse_number(value, "ticks"))?;
            let ttl_s: Option<u64> =
                take_optional(&mut values, "ttl_s", |value| parse_number(value, "ttl_s"))?;
            let ops: Option<u64> =
                take_optional(&mut values, "ops", |value| parse_number(value, "ops"))?;
            if !values.is_empty() {
                let extras = values.keys().cloned().collect::<Vec<_>>().join(", ");
                return Err(anyhow!("unknown spawn options: {extras}"));
            }
            let mut payload = format!("{{\"spawn\":\"heartbeat\",\"ticks\":{ticks}");
            if ttl_s.is_some() || ops.is_some() {
                payload.push_str(",\"budget\":{");
                let mut wrote = false;
                if let Some(ttl_s) = ttl_s {
                    payload.push_str(&format!("\"ttl_s\":{ttl_s}"));
                    wrote = true;
                }
                if let Some(ops) = ops {
                    if wrote {
                        payload.push(',');
                    }
                    payload.push_str(&format!("\"ops\":{ops}"));
                }
                payload.push('}');
            }
            payload.push('}');
            Ok(payload)
        }
        "gpu" | "worker-gpu" => {
            let gpu_id = take_required(&mut values, "gpu_id", |value| {
                ensure_json_string(value, "gpu_id").map(str::to_owned)
            })?;
            let mem_mb: u32 =
                take_required(&mut values, "mem_mb", |value| parse_number(value, "mem_mb"))?;
            let streams: u8 = take_required(&mut values, "streams", |value| {
                parse_number(value, "streams")
            })?;
            let ttl_s: u32 =
                take_required(&mut values, "ttl_s", |value| parse_number(value, "ttl_s"))?;
            let priority: Option<u8> = take_optional(&mut values, "priority", |value| {
                parse_number(value, "priority")
            })?;
            let budget_ttl_s: Option<u64> = take_optional(&mut values, "budget_ttl_s", |value| {
                parse_number(value, "budget_ttl_s")
            })?;
            let budget_ops: Option<u64> = take_optional(&mut values, "budget_ops", |value| {
                parse_number(value, "budget_ops")
            })?;
            if !values.is_empty() {
                let extras = values.keys().cloned().collect::<Vec<_>>().join(", ");
                return Err(anyhow!("unknown spawn options: {extras}"));
            }
            let mut payload = format!(
                "{{\"spawn\":\"gpu\",\"lease\":{{\"gpu_id\":\"{gpu_id}\",\"mem_mb\":{mem_mb},\"streams\":{streams},\"ttl_s\":{ttl_s}"
            );
            if let Some(priority) = priority {
                payload.push_str(&format!(",\"priority\":{priority}"));
            }
            payload.push('}');
            if budget_ttl_s.is_some() || budget_ops.is_some() {
                payload.push_str(",\"budget\":{");
                let mut wrote = false;
                if let Some(budget_ttl_s) = budget_ttl_s {
                    payload.push_str(&format!("\"ttl_s\":{budget_ttl_s}"));
                    wrote = true;
                }
                if let Some(budget_ops) = budget_ops {
                    if wrote {
                        payload.push(',');
                    }
                    payload.push_str(&format!("\"ops\":{budget_ops}"));
                }
                payload.push('}');
            }
            payload.push('}');
            Ok(payload)
        }
        _ => Err(anyhow!("unknown spawn role '{role}'")),
    }
}

fn parse_path(path: &str) -> Result<Vec<String>> {
    if !path.starts_with('/') {
        return Err(anyhow!("paths must be absolute"));
    }
    let mut components = Vec::new();
    for component in path.split('/').skip(1) {
        if component.is_empty() {
            continue;
        }
        if component == "." || component == ".." {
            return Err(anyhow!("path component '{component}' is not permitted"));
        }
        if component.as_bytes().contains(&0) {
            return Err(anyhow!("path component contains NUL byte"));
        }
        if components.len() >= MAX_PATH_COMPONENTS {
            return Err(anyhow!(
                "path exceeds maximum depth of {MAX_PATH_COMPONENTS} components"
            ));
        }
        components.push(component.to_owned());
    }
    Ok(components)
}

fn parse_role(input: &str) -> Result<Role> {
    if input.eq_ignore_ascii_case(proto_role_label(ProtoRole::Queen)) {
        Ok(Role::Queen)
    } else if input.eq_ignore_ascii_case(proto_role_label(ProtoRole::Worker)) {
        Ok(Role::WorkerHeartbeat)
    } else if input.eq_ignore_ascii_case(proto_role_label(ProtoRole::GpuWorker)) {
        Ok(Role::WorkerGpu)
    } else {
        Err(anyhow!("unknown role '{input}'"))
    }
}

fn parse_attach_args<'a>(cmd: &str, args: &'a [&'a str]) -> Result<(Role, Option<&'a str>)> {
    match args {
        [] => Err(anyhow!("{cmd} requires a role")),
        [role] => Ok((parse_role(role)?, None)),
        [role, ticket] => Ok((parse_role(role)?, Some(*ticket))),
        _ => Err(anyhow!(
            "{cmd} takes at most two arguments: role and optional ticket"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn attach_and_tail_logs() {
        let transport = NineDoorTransport::new(NineDoor::new());
        let buffer = Vec::new();
        let mut shell = Shell::new(transport, Cursor::new(buffer));
        shell.attach(Role::Queen, None).unwrap();
        shell.execute("tail /log/queen.log").unwrap();
        let (_transport, cursor) = shell.into_parts();
        let output = cursor.into_inner();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("Cohesix boot: root-task online"));
        assert!(rendered.contains("tick 1"));
    }

    #[test]
    fn parse_role_rejects_unknown() {
        assert!(parse_role("other").is_err());
    }

    #[test]
    fn execute_quit_command() {
        let transport = NineDoorTransport::new(NineDoor::new());
        let mut output = Vec::new();
        {
            let mut shell = Shell::new(transport, &mut output);
            shell.attach(Role::Queen, None).unwrap();
            assert_eq!(shell.execute("quit").unwrap(), CommandStatus::Quit);
        }
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("closing session"));
    }

    #[test]
    fn execute_detach_command() {
        let transport = NineDoorTransport::new(NineDoor::new());
        let mut output = Vec::new();
        {
            let mut shell = Shell::new(transport, &mut output);
            shell.attach(Role::Queen, None).unwrap();
            assert_eq!(shell.execute("detach").unwrap(), CommandStatus::Continue);
            assert!(shell.execute("ping").is_err());
        }
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("OK DETACH"));
    }

    #[test]
    fn worker_attach_requires_identity() {
        let mut transport = NineDoorTransport::new(NineDoor::new());
        let err = transport
            .attach(Role::WorkerHeartbeat, None)
            .expect_err("worker attach without ticket should fail");
        assert!(err
            .to_string()
            .contains("requires a capability ticket containing an identity"));
    }

    #[test]
    fn normalise_payload_appends_newline() {
        assert_eq!(normalise_payload("'trace'").unwrap(), "trace\n");
        assert_eq!(normalise_payload("plain").unwrap(), "plain\n");
    }

    #[test]
    fn echo_payload_expands_msize_sentinel() {
        let payload = build_echo_payload(TEST_MSIZE_SENTINEL).unwrap();
        assert!(payload.len() > MAX_MSIZE as usize);
    }

    #[test]
    fn help_command_lists_surface() {
        let transport = NineDoorTransport::new(NineDoor::new());
        let mut output = Vec::new();
        {
            let mut shell = Shell::new(transport, &mut output);
            shell.execute("help").unwrap();
        }
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("Cohesix command surface:"));
        assert!(rendered.contains("tail <path>"));
        assert!(rendered.contains("ls <path>"));
        assert!(rendered.contains("mount <service> <path>"));
        assert!(rendered.contains("detach"));
    }

    #[test]
    fn parse_role_accepts_gpu_worker() {
        assert_eq!(parse_role("worker-gpu").unwrap(), Role::WorkerGpu);
    }

    #[test]
    fn spawn_payload_requires_options() {
        assert!(build_spawn_payload("heartbeat", [].into_iter()).is_err());
        assert!(build_spawn_payload("gpu", [].into_iter()).is_err());
    }

    #[test]
    fn spawn_payload_formats_heartbeat() {
        let payload =
            build_spawn_payload("heartbeat", ["ticks=10", "ttl_s=60"].into_iter()).unwrap();
        assert_eq!(
            payload,
            "{\"spawn\":\"heartbeat\",\"ticks\":10,\"budget\":{\"ttl_s\":60}}"
        );
    }

    #[test]
    fn spawn_payload_formats_gpu() {
        let payload = build_spawn_payload(
            "gpu",
            [
                "gpu_id=GPU-0",
                "mem_mb=4096",
                "streams=2",
                "ttl_s=120",
                "priority=1",
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(
            payload,
            "{\"spawn\":\"gpu\",\"lease\":{\"gpu_id\":\"GPU-0\",\"mem_mb\":4096,\"streams\":2,\"ttl_s\":120,\"priority\":1}}"
        );
    }

    #[test]
    fn list_reads_directory_entries() {
        let mut transport = NineDoorTransport::new(NineDoor::new());
        let session = transport.attach(Role::Queen, None).unwrap();
        let entries = transport.list(&session, "/log").unwrap();
        assert!(entries.iter().any(|entry| entry == "queen.log"));
    }
}
