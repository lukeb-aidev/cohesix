// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Capture redacted console projections without changing transport requests or responses.
// Author: Lukas Bower
#![forbid(unsafe_code)]

//! Version-2 canonical traces retain observed output, not a synthetic 9P exchange.
//! Recording is passive: reaching a bound stops capture, never retries a command.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{ensure, Context, Result};
use cohsh_core::trace::{CaptureMetadata, TraceLog, TraceLogBuilder, TracePolicy};
use sha2::{Digest, Sha256};

/// Shared passive recording state used by a shell output adapter.
pub struct Capture {
    builder: TraceLogBuilder,
    policy: TracePolicy,
    pending: Vec<u8>,
    secrets: Vec<String>,
    started: Instant,
    duration: Duration,
    completion: u8,
}

/// Shared recording state for the shell and its existing bounded session pool.
pub type CaptureRef = Arc<Mutex<Capture>>;

impl Capture {
    /// Create an identity-bound recorder using an explicit finite capture window.
    pub fn new(
        policy: TracePolicy,
        metadata: CaptureMetadata,
        secrets: Vec<String>,
    ) -> Result<CaptureRef> {
        ensure!(metadata.max_duration_ms > 0, "trace-duration-bound");
        let duration = Duration::from_millis(u64::from(metadata.max_duration_ms));
        Ok(Arc::new(Mutex::new(Self {
            builder: TraceLogBuilder::capture(policy, metadata)?,
            policy,
            pending: Vec::new(),
            secrets: secrets.into_iter().filter(|s| !s.is_empty()).collect(),
            started: Instant::now(),
            duration,
            completion: 0,
        })))
    }

    fn observe(&mut self, bytes: &[u8]) {
        if self.completion != 0 {
            return;
        }
        if self.started.elapsed() > self.duration {
            self.completion = 3;
            return;
        }
        for &byte in bytes {
            if self.completion != 0 {
                break;
            }
            if byte == b'\n' {
                let pending = std::mem::take(&mut self.pending);
                match std::str::from_utf8(&pending) {
                    Ok(line) => self.line(line),
                    Err(_) => self.completion = 1,
                }
            } else if self.pending.len() < self.policy.max_frame_bytes as usize {
                self.pending.push(byte);
            } else {
                self.pending.clear();
                self.completion = 2;
            }
        }
    }

    fn line(&mut self, line: &str) {
        let mut line = line.to_owned();
        for secret in &self.secrets {
            line = line.replace(secret, "<redacted>");
        }
        let Some(line) = redact_line(&line) else {
            return;
        };
        if self
            .builder
            .record_console_line(&line, is_ack(&line))
            .is_err()
        {
            self.completion = 2;
        }
    }

    fn read_result(&mut self, verb: &str, path: &str, result: &Result<Vec<String>>) {
        if self.completion != 0 {
            return;
        }
        if self.started.elapsed() > self.duration {
            self.completion = 3;
            return;
        }
        let request = format!("{verb} {path}");
        if validate_read_request(&request).is_err() {
            self.completion = 1;
            return;
        }
        let lines = match result {
            Ok(lines) => lines,
            Err(_) => {
                // Replace any earlier observation at replay time. Never expose
                // peer error strings or leave a stale successful read current.
                if self
                    .builder
                    .record_frame(request.as_bytes(), br#"{"error":"read-unavailable"}"#)
                    .is_err()
                {
                    self.completion = 2;
                }
                return;
            }
        };
        let lines: Vec<String> = lines
            .iter()
            .map(|line| {
                let mut line = line.clone();
                for secret in &self.secrets {
                    line = line.replace(secret, "<redacted>");
                }
                if verb == "LS" && retained_list_name(&line) {
                    line
                } else {
                    redact_line(&line).unwrap_or_else(|| "<redacted>".to_owned())
                }
            })
            .collect();
        match serde_json::to_vec(&lines) {
            Ok(response) => {
                if self
                    .builder
                    .record_frame(request.as_bytes(), &response)
                    .is_err()
                {
                    self.completion = 2;
                }
            }
            Err(_) => self.completion = 1,
        }
    }

    /// Atomically finalize a complete or explicitly partial capture, including errors.
    pub fn finish(&mut self, path: &Path, session_succeeded: bool) -> Result<u8> {
        if self.completion == 0 && (!session_succeeded || !self.pending.is_empty()) {
            self.completion = 1;
        }
        if self.completion == 0 && self.started.elapsed() > self.duration {
            self.completion = 3;
        }
        let mut trace = self.builder.snapshot();
        trace
            .capture
            .as_mut()
            .context("trace-capture-metadata")?
            .completion = self.completion;
        let bytes = trace.encode(self.policy)?;
        atomic_write(path, &bytes)?;
        Ok(self.completion)
    }
}

/// Pass-through transport wrapper recording bounded namespace read outcomes.
/// Writes, authentication, retries, batched operations and acknowledgements retain
/// the original transport behavior; request payloads are never persisted.
pub struct CaptureTransport<T> {
    inner: T,
    capture: CaptureRef,
}

impl<T> CaptureTransport<T> {
    /// Wrap a primary or pooled transport using the same canonical recorder.
    pub fn new(inner: T, capture: CaptureRef) -> Self {
        Self { inner, capture }
    }
    fn record(&self, verb: &str, path: &str, result: &Result<Vec<String>>) {
        if let Ok(mut capture) = self.capture.lock() {
            capture.read_result(verb, path, result);
        }
    }
}

impl<T: crate::Transport> crate::Transport for CaptureTransport<T> {
    fn attach(
        &mut self,
        role: cohesix_ticket::Role,
        ticket: Option<&str>,
    ) -> Result<crate::Session> {
        self.inner.attach(role, ticket)
    }
    fn kind(&self) -> &'static str {
        self.inner.kind()
    }
    fn ping(&mut self, session: &crate::Session) -> Result<String> {
        self.inner.ping(session)
    }
    fn read(&mut self, session: &crate::Session, path: &str) -> Result<Vec<String>> {
        let result = self.inner.read(session, path);
        self.record("CAT", path, &result);
        result
    }
    fn list(&mut self, session: &crate::Session, path: &str) -> Result<Vec<String>> {
        let result = self.inner.list(session, path);
        self.record("LS", path, &result);
        result
    }
    fn tail(
        &mut self,
        session: &crate::Session,
        path: &str,
        lines: Option<u16>,
    ) -> Result<Vec<String>> {
        let result = self.inner.tail(session, path, lines);
        self.record("TAIL", path, &result);
        result
    }
    fn read_batch(
        &mut self,
        session: &crate::Session,
        requests: &[crate::ReadBatchRequest],
    ) -> Result<Vec<crate::ReadBatchOutcome>> {
        let result = self.inner.read_batch(session, requests);
        for (index, request) in requests.iter().enumerate() {
            let observed = result
                .as_ref()
                .ok()
                .and_then(|items| items.get(index))
                .and_then(|outcome| outcome.as_ref().ok())
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("read-unavailable"));
            match request {
                crate::ReadBatchRequest::Read { path } => self.record("CAT", path, &observed),
                crate::ReadBatchRequest::List { path } => self.record("LS", path, &observed),
            }
        }
        result
    }
    fn command_batch(
        &mut self,
        session: &crate::Session,
        requests: &[crate::TransportBatchRequest],
    ) -> Result<Vec<crate::TransportBatchOutcome>> {
        let result = self.inner.command_batch(session, requests);
        for (index, request) in requests.iter().enumerate() {
            let observed = result
                .as_ref()
                .ok()
                .and_then(|items| items.get(index))
                .and_then(|outcome| outcome.as_ref().ok())
                .and_then(|response| match response {
                    crate::TransportBatchResponse::Lines(lines) => Some(lines.clone()),
                    _ => None,
                })
                .ok_or_else(|| anyhow::anyhow!("read-unavailable"));
            match request {
                crate::TransportBatchRequest::Read { path } => self.record("CAT", path, &observed),
                crate::TransportBatchRequest::List { path } => self.record("LS", path, &observed),
                _ => {}
            }
        }
        result
    }
    fn write(&mut self, session: &crate::Session, path: &str, payload: &[u8]) -> Result<()> {
        self.inner.write(session, path, payload)
    }
    fn write_batch(
        &mut self,
        session: &crate::Session,
        path: &str,
        payloads: &[Vec<u8>],
    ) -> Result<usize> {
        self.inner.write_batch(session, path, payloads)
    }
    fn console_command(
        &mut self,
        session: &crate::Session,
        command: &str,
        expected_ack: &str,
    ) -> Result<Vec<String>> {
        self.inner.console_command(session, command, expected_ack)
    }
    fn quit(&mut self, session: &crate::Session) -> Result<()> {
        self.inner.quit(session)
    }
    fn drain_acknowledgements(&mut self) -> Vec<String> {
        self.inner.drain_acknowledgements()
    }
    fn metrics(&self) -> crate::TransportMetrics {
        self.inner.metrics()
    }
    fn inject_short_write(&mut self, bytes: usize) -> bool {
        self.inner.inject_short_write(bytes)
    }
    fn tcp_endpoint(&self) -> Option<(String, u16)> {
        self.inner.tcp_endpoint()
    }
    fn tcp_connection_info(&self) -> Option<crate::TcpConnectionInfo> {
        self.inner.tcp_connection_info()
    }
}

fn validate_read_request(request: &str) -> Result<(&str, &str)> {
    let (verb, path) = request.split_once(' ').context("trace-read-request")?;
    ensure!(
        matches!(verb, "CAT" | "LS" | "TAIL"),
        "trace-mutating-request"
    );
    ensure!(
        path.starts_with('/')
            && path.len() <= cohsh_core::MAX_PATH_LEN
            && !path.chars().any(char::is_whitespace),
        "trace-read-path"
    );
    if path != "/" {
        ensure!(
            path[1..].split('/').count() <= usize::from(crate::SECURE9P_WALK_DEPTH),
            "trace-read-depth"
        );
        for part in path[1..].split('/') {
            validate_component(part)?;
        }
    }
    Ok((verb, path))
}

fn validate_component(part: &str) -> Result<()> {
    ensure!(
        !part.is_empty()
            && part != "."
            && part != ".."
            && part.len() <= cohsh_core::MAX_PATH_LEN
            && !part
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || c == '/'),
        "trace-path-component"
    );
    Ok(())
}

fn retained_list_name(name: &str) -> bool {
    // A listing can be supplied by an untrusted projection. Do not let its
    // filename fast path bypass field redaction for credential-shaped text.
    validate_component(name).is_ok()
        && !name.contains(['=', '"', '\'', '{', '}'])
        && !name
            .split_once(':')
            .is_some_and(|(key, _)| sensitive_key(key))
}

type ReadObservation = (String, String, Option<Vec<String>>);

fn validate_read_frame(frame: &cohsh_core::trace::TraceFrame) -> Result<ReadObservation> {
    let request = std::str::from_utf8(&frame.request)?;
    let (verb, path) = validate_read_request(request)?;
    if frame.response == br#"{"error":"read-unavailable"}"# {
        return Ok((verb.to_owned(), path.to_owned(), None));
    }
    let lines: Vec<String> = serde_json::from_slice(&frame.response).context("trace-read-lines")?;
    for line in &lines {
        if verb == "LS" && retained_list_name(line) {
            continue;
        }
        ensure!(
            redact_line(line).as_deref() == Some(line.as_str()),
            "trace-unsanitized-read"
        );
    }
    Ok((verb.to_owned(), path.to_owned(), Some(lines)))
}

/// Offline view of the latest retained observation for each read verb/path.
/// Missing reads are unavailable; no replay operation opens sockets or writes files.
pub struct CapturedNamespace {
    observations: std::collections::BTreeMap<(String, String), Vec<String>>,
    transcript: Vec<String>,
}

impl CapturedNamespace {
    /// Validate the shared canonical trace and policy before exposing any observations.
    pub fn new(trace: &TraceLog, expected_policy: &[u8; 32]) -> Result<Self> {
        let transcript = replay_lines(trace, expected_policy)?;
        ensure!(
            trace
                .capture
                .as_ref()
                .is_some_and(|metadata| metadata.completion == 0),
            "trace-capture-incomplete"
        );
        let mut observations = std::collections::BTreeMap::new();
        for frame in &trace.frames {
            if !frame.request.is_empty() {
                let (verb, path, lines) = validate_read_frame(frame)?;
                if let Some(lines) = lines {
                    observations.insert((verb, path), lines);
                } else {
                    observations.remove(&(verb, path));
                }
            }
        }
        Ok(Self {
            observations,
            transcript,
        })
    }
    /// Retained post-redaction display order, including exact ACK/ERR/END lines.
    pub fn transcript(&self) -> &[String] {
        &self.transcript
    }
    fn observed(&self, verb: &str, path: &str) -> Result<Vec<String>> {
        self.observations
            .get(&(verb.to_owned(), path.to_owned()))
            .cloned()
            .context("trace-observation-unavailable")
    }
}

impl crate::Transport for CapturedNamespace {
    fn attach(&mut self, role: cohesix_ticket::Role, _: Option<&str>) -> Result<crate::Session> {
        Ok(crate::Session::new(
            secure9p_codec::SessionId::from_raw(1),
            role,
        ))
    }
    fn kind(&self) -> &'static str {
        "offline-captured-console"
    }
    fn ping(&mut self, _: &crate::Session) -> Result<String> {
        Ok("offline-captured-console; proof=none".to_owned())
    }
    fn read(&mut self, _: &crate::Session, path: &str) -> Result<Vec<String>> {
        self.observed("CAT", path)
    }
    fn list(&mut self, _: &crate::Session, path: &str) -> Result<Vec<String>> {
        self.observed("LS", path)
    }
    fn tail(&mut self, _: &crate::Session, path: &str, lines: Option<u16>) -> Result<Vec<String>> {
        Ok(crate::apply_tail_line_limit(
            self.observed("TAIL", path)?,
            crate::ensure_valid_tail_lines(lines)?,
        ))
    }
    fn write(&mut self, _: &crate::Session, _: &str, _: &[u8]) -> Result<()> {
        anyhow::bail!("trace-replay-readonly")
    }
}

/// Write-through adapter: the observed stdout bytes and transport behavior are unchanged.
pub struct CaptureWriter<W> {
    inner: W,
    capture: Option<CaptureRef>,
}

impl<W> CaptureWriter<W> {
    /// Optionally capture a normal writer without changing its write contract.
    pub fn new(inner: W, capture: Option<CaptureRef>) -> Self {
        Self { inner, capture }
    }
}

impl<W: Write> Write for CaptureWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(bytes)?;
        if let Some(capture) = &self.capture {
            if let Ok(mut capture) = capture.lock() {
                capture.observe(&bytes[..written]);
            }
        }
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Read a bounded regular canonical trace file; reject special files and symlinks.
pub fn read_trace(path: &Path, policy: TracePolicy) -> Result<TraceLog> {
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() <= u64::from(policy.max_bytes),
        "trace-file-bound"
    );
    let mut bytes = Vec::new();
    File::open(path)?
        .take(u64::from(policy.max_bytes) + 1)
        .read_to_end(&mut bytes)?;
    Ok(TraceLog::decode(&bytes, policy)?)
}

/// Validate a live capture before offline presentation. Legacy fixtures use the
/// existing 9P replayer; console captures cannot be fed to a live transport.
pub fn replay_lines(trace: &TraceLog, expected_policy: &[u8; 32]) -> Result<Vec<String>> {
    let metadata = trace.capture.as_ref().context("trace-is-legacy-9p")?;
    ensure!(
        &metadata.policy_sha256 == expected_policy,
        "trace-policy-mismatch"
    );
    let mut lines = Vec::new();
    let mut acks = Vec::new();
    for frame in &trace.frames {
        if !frame.request.is_empty() {
            validate_read_frame(frame)?;
            continue;
        }
        let line = std::str::from_utf8(&frame.response).context("trace-line-utf8")?;
        ensure!(
            redact_line(line).as_deref() == Some(line),
            "trace-unsanitized-line"
        );
        ensure!(!line.contains(['\n', '\r']), "trace-line-boundary");
        if is_ack(line) {
            acks.push(line.to_owned());
        }
        lines.push(line.to_owned());
    }
    ensure!(acks == trace.ack_lines, "trace-ack-order");
    Ok(lines)
}

/// Bind the generated policy and fixed redaction rules without claiming a signature.
pub fn policy_digest(policy: TracePolicy, duration_ms: u32) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"cohesix-canonical-console-redaction/v1\0");
    hash.update(crate::CohshPolicy::policy_hash().as_bytes());
    for value in [
        policy.max_bytes,
        policy.max_frame_bytes,
        policy.max_ack_bytes,
        duration_ms,
    ] {
        hash.update(value.to_le_bytes());
    }
    hash.finalize().into()
}

/// Hash identity labels so credentials or private enrollment names cannot enter metadata.
pub fn identity_digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

/// Commit a canonical trace on the same filesystem with no shared temporary name.
pub fn atomic_write(path: &Path, payload: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(payload)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|_| anyhow::anyhow!("trace-commit"))?;
    Ok(())
}

fn is_ack(line: &str) -> bool {
    let line = line.strip_prefix("[console] ").unwrap_or(line);
    line.starts_with("OK ") || line.starts_with("ERR ") || line.starts_with("ACK ") || line == "END"
}

/// Redact one display line; authentication and echoed command payloads are omitted.
/// Unknown unstructured payloads are withheld because their fields have no schema.
pub fn redact_line(line: &str) -> Option<String> {
    if let Some(line) = line.strip_prefix("[console] ") {
        return redact_line(line).map(|line| format!("[console] {line}"));
    }
    let lower = line.to_ascii_lowercase();
    if lower.starts_with("auth ")
        || lower.starts_with("ok auth")
        || lower.starts_with("err auth")
        || lower.contains("attach queen ")
        || lower.contains("echo ")
    {
        return None;
    }
    if line == "<redacted>" {
        return Some(line.to_owned());
    }
    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(line) {
        redact_value(&mut value);
        return Some(value.to_string());
    }
    if line
        .split(|c: char| c.is_whitespace() || matches!(c, '=' | ':' | '"' | '\''))
        .any(sensitive_key)
    {
        // Preserve the ACK/ERR verb while withholding the whole unstructured detail.
        return Some(if is_ack(line) {
            format!(
                "{} {} <redacted>",
                line.split_whitespace().next().unwrap_or("ERR"),
                line.split_whitespace().nth(1).unwrap_or("UNKNOWN")
            )
        } else {
            "<redacted>".to_owned()
        });
    }
    if is_ack(line) {
        return Some(line.to_owned());
    }
    // Key/value diagnostic lines are file-shaped public observations. Free text
    // (including shell error echoes) has no field contract and is withheld.
    if line.split_whitespace().all(|part| {
        part.split_once('=')
            .is_some_and(|(key, value)| !key.is_empty() && !value.is_empty())
    }) && !line.is_empty()
    {
        Some(line.to_owned())
    } else {
        Some("<redacted>".to_owned())
    }
}

/// Shared evidence/trace secret-field classifier; ticket identity is handled separately.
pub fn sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "token",
        "authorization",
        "secret",
        "password",
        "signing_key",
        "api_key",
        "credential",
        "private_key",
    ]
    .iter()
    .any(|part| key.contains(part))
        || matches!(key.as_str(), "auth_ref" | "auth" | "ticket")
}

/// Redact nested objects and JSON embedded in string payloads before persistence.
pub fn redact_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, nested) in map {
                if key == "ticket" {
                    if let Some(ticket) = nested.as_str() {
                        if ticket != "none"
                            && !ticket.strip_prefix("sha256:").is_some_and(|hash| {
                                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
                            })
                        {
                            *nested = serde_json::Value::String(format!(
                                "sha256:{}",
                                hex::encode(identity_digest(ticket))
                            ));
                        }
                    } else {
                        *nested = serde_json::Value::String("<redacted>".to_owned());
                    }
                } else if sensitive_key(key) {
                    *nested = serde_json::Value::String("<redacted>".to_owned());
                } else {
                    redact_value(nested);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_value(value);
            }
        }
        serde_json::Value::String(text) => {
            if let Ok(mut embedded) = serde_json::from_str::<serde_json::Value>(text) {
                if embedded.is_object() || embedded.is_array() {
                    redact_value(&mut embedded);
                    *text = embedded.to_string();
                }
            } else if text
                .split(|c: char| c.is_whitespace() || matches!(c, '=' | ':' | '"' | '\''))
                .any(sensitive_key)
            {
                *text = "<redacted>".to_owned();
            }
        }
        _ => {}
    }
}
