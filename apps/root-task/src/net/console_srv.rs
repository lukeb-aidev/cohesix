// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: TCP console session management shared between kernel and host stacks, including buffering and drop policy.
// Author: Lukas Bower

//! TCP console session management shared between kernel and host stacks.

use core::{fmt::Write, ops::Range};
use heapless::{Deque, String as HeaplessString, Vec as HeaplessVec};
use log::{debug, info, warn};
use portable_atomic::{AtomicBool, Ordering};
use secure9p_codec::MAX_MSIZE;

use super::{ConsoleLine, AUTH_TIMEOUT_MS, CONSOLE_QUEUE_DEPTH};
use crate::console::proto::{render_ack, AckStatus, LineFormatError};
use crate::observe::{IngestMetrics, IngestSnapshot};
use crate::serial::DEFAULT_LINE_CAPACITY;
use cohesix_proto::{REASON_EXPECTED_TOKEN, REASON_INVALID_LENGTH, REASON_INVALID_TOKEN};
use console_ack_wire::AckLine;

// Transport-level guard to prevent unauthenticated TCP sessions from issuing console verbs.
// Application-layer ticket and role checks are enforced by the console/event pump.
const AUTH_PREFIX: &str = "AUTH ";
const DETAIL_REASON_EXPECTED_TOKEN: &str = "reason=expected-token";
const DETAIL_REASON_INVALID_LENGTH: &str = "reason=invalid-length";
const DETAIL_REASON_INVALID_TOKEN: &str = "reason=invalid-token";
const FRAME_ERROR_VERB: &str = "FRAME";
const PREAUTH_FIRST_CAPACITY: usize = 4;
const PREAUTH_LAST_CAPACITY: usize = 4;
const PREAUTH_INFO_ALLOWLIST: &[&str] = &["[net-console]", "[cohsh-net]", "[console]", "[event]"];

/// Outcome of processing newly received bytes from the TCP stream.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// No state change occurred.
    None,
    /// The client successfully authenticated.
    Authenticated,
    /// Authentication failed and the connection should be terminated.
    AuthFailed(&'static str),
    /// The server should close the connection.
    Close,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum SessionState {
    WaitingAuth,
    Authenticated,
    Inactive,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
struct PreauthFlushStats {
    flushed: u64,
    dropped: u64,
}

/// State machine that validates authentication tokens and buffers console lines.
///
/// Flow:
/// root console emit -> `NetStack::send_console_line` -> `TcpConsoleServer::enqueue_outbound`
/// -> (pre-auth ring if unauthenticated, otherwise priority/outbound queues)
/// -> `NetStack::flush_outbound` -> `OutboundCoalescer` -> TCP socket send.
pub struct TcpConsoleServer {
    auth_token: &'static str,
    idle_timeout_ms: u64,
    state: SessionState,
    line_buffer: HeaplessString<DEFAULT_LINE_CAPACITY>,
    frame_buffer: HeaplessVec<u8, DEFAULT_LINE_CAPACITY>,
    frame_len_buf: [u8; 4],
    frame_len_pos: usize,
    frame_payload_len: Option<usize>,
    drop_remaining: usize,
    inbound: Deque<ConsoleLine, CONSOLE_QUEUE_DEPTH>,
    priority_outbound: Deque<HeaplessString<DEFAULT_LINE_CAPACITY>, { CONSOLE_QUEUE_DEPTH * 4 }>,
    outbound: Deque<HeaplessString<DEFAULT_LINE_CAPACITY>, CONSOLE_QUEUE_DEPTH>,
    // Retains the oldest and newest console lines while no authenticated client is
    // attached to avoid saturating the live outbound queue with boot-time bursts.
    preauth_first: Deque<HeaplessString<DEFAULT_LINE_CAPACITY>, PREAUTH_FIRST_CAPACITY>,
    preauth_last: Deque<HeaplessString<DEFAULT_LINE_CAPACITY>, PREAUTH_LAST_CAPACITY>,
    last_activity_ms: u64,
    auth_deadline_ms: Option<u64>,
    conn_id: Option<u64>,
    outbound_drops: u64,
    preauth_drop_count: u64,
    preauth_drop_warned: bool,
    ingest_metrics: IngestMetrics,
}

impl TcpConsoleServer {
    fn snapshot_console_queue_ptr<const N: usize>(
        queue: &mut Deque<ConsoleLine, N>,
    ) -> Option<usize> {
        if queue.is_full() {
            return None;
        }
        let sample = ConsoleLine::new(HeaplessString::new(), 0);
        queue.push_back(sample).ok()?;
        let ptr = queue
            .back()
            .map(|line| line.text.as_bytes().as_ptr() as usize);
        let _ = queue.pop_back();
        ptr
    }

    fn snapshot_string_queue_ptr<const N: usize>(
        queue: &mut Deque<HeaplessString<DEFAULT_LINE_CAPACITY>, N>,
    ) -> Option<usize> {
        if queue.is_full() {
            return None;
        }
        let sample = HeaplessString::new();
        queue.push_back(sample).ok()?;
        let ptr = queue.back().map(|line| line.as_bytes().as_ptr() as usize);
        let _ = queue.pop_back();
        ptr
    }

    pub fn log_buffer_addresses_once(&mut self, marker: &'static str) {
        static LOGGED: AtomicBool = AtomicBool::new(false);
        if LOGGED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let line_ptr = self.line_buffer.as_bytes().as_ptr() as usize;
        info!(
            target: "net-console",
            "[net-console] addr marker={marker} label=line-buffer ptr=0x{line_ptr:016x} len=0x{len:04x}",
            len = DEFAULT_LINE_CAPACITY,
        );
        if let Some(ptr) = Self::snapshot_console_queue_ptr(&mut self.inbound) {
            info!(
                target: "net-console",
                "[net-console] addr marker={marker} label=inbound-queue ptr=0x{ptr:016x} len=0x{len:04x}",
                len = DEFAULT_LINE_CAPACITY,
            );
        }
        if let Some(ptr) = Self::snapshot_string_queue_ptr(&mut self.priority_outbound) {
            info!(
                target: "net-console",
                "[net-console] addr marker={marker} label=priority-outbound ptr=0x{ptr:016x} len=0x{len:04x}",
                len = DEFAULT_LINE_CAPACITY,
            );
        }
        if let Some(ptr) = Self::snapshot_string_queue_ptr(&mut self.outbound) {
            info!(
                target: "net-console",
                "[net-console] addr marker={marker} label=outbound ptr=0x{ptr:016x} len=0x{len:04x}",
                len = DEFAULT_LINE_CAPACITY,
            );
        }
    }

    pub fn line_buffer_range(&self) -> Range<usize> {
        let ptr = self.line_buffer.as_bytes().as_ptr() as usize;
        ptr..ptr + DEFAULT_LINE_CAPACITY
    }

    fn set_state(&mut self, next: SessionState) {
        if self.state != next {
            info!("[cohsh-net][auth] state: {:?} -> {:?}", self.state, next);
            self.state = next;
        }
    }

    fn log_expected_auth(&self, expected_len: usize) {
        debug!(
            "[cohsh-net][auth] expected prefix=\"{}\" version=1 token_len={} total_len={} bytes",
            AUTH_PREFIX.trim_end(),
            self.auth_token.len(),
            expected_len
        );
    }

    fn log_reject(&self, reason: &str, line: &str) {
        warn!(
            "[cohsh-net][auth] reject: conn id={} reason={} raw_len={} raw_bytes={:02x?}",
            self.conn_label(),
            reason,
            line.len(),
            line.as_bytes()
        );
    }

    fn expected_frame_len(&self) -> usize {
        AUTH_PREFIX.len().saturating_add(self.auth_token.len())
    }

    /// Construct a new server that validates the provided authentication token.
    pub fn new(auth_token: &'static str, idle_timeout_ms: u64) -> Self {
        Self {
            auth_token,
            idle_timeout_ms,
            state: SessionState::Inactive,
            line_buffer: HeaplessString::new(),
            frame_buffer: HeaplessVec::new(),
            frame_len_buf: [0u8; 4],
            frame_len_pos: 0,
            frame_payload_len: None,
            drop_remaining: 0,
            inbound: Deque::new(),
            priority_outbound: Deque::new(),
            outbound: Deque::new(),
            preauth_first: Deque::new(),
            preauth_last: Deque::new(),
            last_activity_ms: 0,
            auth_deadline_ms: None,
            conn_id: None,
            outbound_drops: 0,
            preauth_drop_count: 0,
            preauth_drop_warned: false,
            ingest_metrics: IngestMetrics::default(),
        }
    }

    pub(crate) fn auth_token(&self) -> &'static str {
        self.auth_token
    }

    /// Reset the session state in preparation for a new client connection.
    pub fn begin_session(&mut self, now_ms: u64, conn_id: Option<u64>) {
        self.set_state(SessionState::WaitingAuth);
        self.line_buffer.clear();
        self.frame_buffer.clear();
        self.frame_len_pos = 0;
        self.frame_payload_len = None;
        self.drop_remaining = 0;
        self.inbound.clear();
        self.priority_outbound.clear();
        self.outbound.clear();
        self.reset_preauth_buffers();
        self.last_activity_ms = now_ms;
        self.auth_deadline_ms = None;
        self.conn_id = conn_id;
        let expected_len = self.expected_frame_len();
        self.log_expected_auth(expected_len);
        info!(
            "[net-console] handshake: expecting client hello len={} magic=\"{}\" version=1",
            expected_len,
            AUTH_PREFIX.trim_end()
        );
        #[cfg(feature = "net-trace-31337")]
        info!(
            "[cohsh-net] conn id={} auth: waiting for client hello (expected_len={} magic=\"{}\" token_len={})",
            self.conn_label(),
            expected_len,
            AUTH_PREFIX.trim_end(),
            self.auth_token.len()
        );
        self.auth_deadline_ms = Some(now_ms.saturating_add(AUTH_TIMEOUT_MS));
        info!("[net-console] auth begin (challenge staged)");
        info!("[net-console] auth: waiting for handshake payload");
    }

    /// Tear down any per-connection state.
    pub fn end_session(&mut self) {
        let preauth_stats = self.snapshot_preauth_for_teardown();
        self.enqueue_preauth_summary(preauth_stats, "session-end");
        self.set_state(SessionState::Inactive);
        self.line_buffer.clear();
        self.frame_buffer.clear();
        self.frame_len_pos = 0;
        self.frame_payload_len = None;
        self.drop_remaining = 0;
        self.inbound.clear();
        self.outbound.clear();
        self.reset_preauth_buffers();
        self.last_activity_ms = 0;
        self.auth_deadline_ms = None;
        self.conn_id = None;
        self.outbound_drops = 0;
    }

    fn reset_preauth_buffers(&mut self) {
        self.preauth_first.clear();
        self.preauth_last.clear();
        self.clear_preauth_epoch();
    }

    fn clear_preauth_epoch(&mut self) {
        self.preauth_drop_count = 0;
        self.preauth_drop_warned = false;
    }

    fn snapshot_preauth_for_teardown(&mut self) -> PreauthFlushStats {
        let flushed =
            (self.preauth_first.len() as u64).saturating_add(self.preauth_last.len() as u64);
        let dropped = self.preauth_drop_count;
        self.preauth_first.clear();
        self.preauth_last.clear();
        self.clear_preauth_epoch();
        PreauthFlushStats { flushed, dropped }
    }

    /// Consume bytes received from the client, returning any resulting session event.
    pub fn ingest(&mut self, payload: &[u8], now_ms: u64) -> SessionEvent {
        if matches!(self.state, SessionState::Inactive) {
            // Connection was closed before authentication; drop stray bytes.
            return SessionEvent::Close;
        }

        let mut event = SessionEvent::None;
        for &byte in payload {
            if self.drop_remaining > 0 {
                self.drop_remaining = self.drop_remaining.saturating_sub(1);
                if self.drop_remaining == 0 {
                    self.frame_len_pos = 0;
                    self.frame_payload_len = None;
                    self.frame_buffer.clear();
                }
                continue;
            }
            if let Some(expected_len) = self.frame_payload_len {
                if self.frame_buffer.push(byte).is_err() {
                    self.log_reject(REASON_INVALID_LENGTH, "<frame overflow>");
                    return SessionEvent::AuthFailed(REASON_INVALID_LENGTH);
                }
                if self.frame_buffer.len() == expected_len {
                    let line = match core::str::from_utf8(self.frame_buffer.as_slice()) {
                        Ok(line) => line,
                        Err(_) => {
                            self.log_reject(REASON_INVALID_LENGTH, "<frame utf8>");
                            return SessionEvent::AuthFailed(REASON_INVALID_LENGTH);
                        }
                    };
                    #[cfg(feature = "net-trace-31337")]
                    info!(
                        "[cohsh-net] conn id={} auth: received len={} bytes={:02x?}",
                        self.conn_label(),
                        line.len(),
                        line.as_bytes()
                    );
                    self.line_buffer.clear();
                    if self.line_buffer.push_str(line).is_err() {
                        self.log_reject(REASON_INVALID_LENGTH, "<frame copy>");
                        return SessionEvent::AuthFailed(REASON_INVALID_LENGTH);
                    }
                    let line = self.line_buffer.clone();
                    self.frame_payload_len = None;
                    self.frame_buffer.clear();
                    self.last_activity_ms = now_ms;
                    log::debug!(
                        target: "net-console",
                        "[tcp-console] line received: len={} first_bytes={:02x?}",
                        line.len(),
                        &line.as_bytes()[..core::cmp::min(line.len(), 32)],
                    );
                    event = self.handle_line(line, now_ms);
                    if matches!(event, SessionEvent::Close) {
                        break;
                    }
                }
            } else {
                self.frame_len_buf[self.frame_len_pos] = byte;
                self.frame_len_pos = self.frame_len_pos.saturating_add(1);
                if self.frame_len_pos == self.frame_len_buf.len() {
                    self.frame_len_pos = 0;
                    let declared = u32::from_le_bytes(self.frame_len_buf) as usize;
                    if declared < 4 || declared > MAX_MSIZE as usize {
                        self.log_reject(REASON_INVALID_LENGTH, "<frame length>");
                        if matches!(self.state, SessionState::Authenticated) && declared >= 4 {
                            self.enqueue_frame_length_error();
                            self.begin_frame_drop(declared.saturating_sub(4));
                            continue;
                        }
                        return SessionEvent::AuthFailed(REASON_INVALID_LENGTH);
                    }
                    let payload_len = declared.saturating_sub(4);
                    if payload_len > DEFAULT_LINE_CAPACITY {
                        self.log_reject(REASON_INVALID_LENGTH, "<frame payload>");
                        if matches!(self.state, SessionState::Authenticated) {
                            self.enqueue_frame_length_error();
                            self.begin_frame_drop(payload_len);
                            continue;
                        }
                        return SessionEvent::AuthFailed(REASON_INVALID_LENGTH);
                    }
                    self.frame_payload_len = Some(payload_len);
                    self.frame_buffer.clear();
                    if payload_len == 0 {
                        let line = HeaplessString::new();
                        event = self.handle_line(line, now_ms);
                        self.frame_payload_len = None;
                        if matches!(event, SessionEvent::Close) {
                            break;
                        }
                    }
                }
            }
        }

        event
    }

    fn enqueue_frame_length_error(&mut self) {
        let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
        let ack = AckLine {
            status: AckStatus::Err,
            verb: FRAME_ERROR_VERB,
            detail: Some(DETAIL_REASON_INVALID_LENGTH),
        };
        if render_ack(&mut line, &ack).is_ok() {
            let _ = self.enqueue_outbound(line.as_str());
        }
    }

    fn begin_frame_drop(&mut self, payload_len: usize) {
        self.drop_remaining = payload_len;
        self.frame_len_pos = 0;
        self.frame_payload_len = None;
        self.frame_buffer.clear();
        self.line_buffer.clear();
    }

    fn handle_line(
        &mut self,
        line: HeaplessString<DEFAULT_LINE_CAPACITY>,
        now_ms: u64,
    ) -> SessionEvent {
        match self.state {
            SessionState::WaitingAuth => self.process_auth(line),
            SessionState::Authenticated => {
                let entry = ConsoleLine::new(line, now_ms);
                if let Err(entry) = self.inbound.push_back(entry) {
                    // Drop oldest to make space for high-priority lines.
                    let _ = self.inbound.pop_front();
                    let _ = self.inbound.push_back(entry);
                    self.ingest_metrics.record_backpressure();
                    self.ingest_metrics.record_drop();
                }
                SessionEvent::None
            }
            SessionState::Inactive => SessionEvent::Close,
        }
    }

    fn process_auth(&mut self, line: HeaplessString<DEFAULT_LINE_CAPACITY>) -> SessionEvent {
        // Expected client hello: ASCII "AUTH " prefix and token payload.
        let raw_bytes = line.as_bytes();
        log::info!(
            "[cohsh-net][auth] parsing auth frame ({} bytes): {:02x?}",
            raw_bytes.len(),
            &raw_bytes[..core::cmp::min(raw_bytes.len(), 32)]
        );
        let expected_len = self.expected_frame_len();
        let observed_len = raw_bytes.len();
        info!("[cohsh-net] auth: hello received (len={})", observed_len);
        #[cfg(feature = "net-trace-31337")]
        log::info!(
            "[cohsh-net] conn id={} auth: parsing frame observed_len={} bytes={:02x?}",
            self.conn_label(),
            observed_len,
            &raw_bytes[..core::cmp::min(raw_bytes.len(), 32)]
        );
        if observed_len != expected_len {
            warn!(
                "[cohsh-net][auth] conn id={} invalid frame length: expected={}, got={}",
                self.conn_label(),
                expected_len,
                observed_len
            );
            self.log_reject(REASON_INVALID_LENGTH, line.as_str());
            let _ = self.enqueue_auth_ack(AckStatus::Err, Some(DETAIL_REASON_INVALID_LENGTH));
            self.set_state(SessionState::Inactive);
            warn!("[cohsh-net][auth] closing session: reason=invalid-length");
            warn!("[net-console] auth failed reason=invalid-length");
            return SessionEvent::AuthFailed("invalid-length");
        }

        let Some(stripped) = line.strip_prefix(AUTH_PREFIX) else {
            warn!(
                "[cohsh-net][auth] conn id={} reject: missing AUTH prefix raw_len={} raw_bytes={:02x?}",
                self.conn_label(),
                raw_bytes.len(),
                &raw_bytes[..core::cmp::min(raw_bytes.len(), AUTH_PREFIX.len())]
            );
            self.log_reject(REASON_EXPECTED_TOKEN, line.as_str());
            let _ = self.enqueue_auth_ack(AckStatus::Err, Some(DETAIL_REASON_EXPECTED_TOKEN));
            self.set_state(SessionState::Inactive);
            warn!("[cohsh-net][auth] closing session: reason=expected-token");
            warn!("[net-console] auth failed reason=expected-token");
            return SessionEvent::AuthFailed("expected-token");
        };

        let token = stripped.trim();
        if token.is_empty() {
            warn!(
                "[cohsh-net][auth] conn id={} reject: empty token raw_len={} raw_bytes={:02x?}",
                self.conn_label(),
                raw_bytes.len(),
                &raw_bytes[..core::cmp::min(raw_bytes.len(), AUTH_PREFIX.len())]
            );
            self.log_reject(REASON_EXPECTED_TOKEN, line.as_str());
            let _ = self.enqueue_auth_ack(AckStatus::Err, Some(DETAIL_REASON_EXPECTED_TOKEN));
            self.set_state(SessionState::Inactive);
            warn!("[cohsh-net][auth] closing session: reason=expected-token");
            warn!("[net-console] auth failed reason=expected-token");
            return SessionEvent::AuthFailed("expected-token");
        }

        let mut token_parts = token.split_whitespace();
        let role_str = token_parts.next().unwrap_or("");
        let role_ok = !role_str.is_empty();
        let version_ok = true;
        if !(version_ok && role_ok) {
            warn!(
                "[cohsh-net][auth] conn id={} invalid magic/version/role: version_ok={} role_ok={}",
                self.conn_label(),
                version_ok,
                role_ok
            );
        }

        info!(
            "[cohsh-net] parsed handshake: conn_id={} role='{}' token_len={}",
            self.conn_label(),
            role_str,
            token.len()
        );
        info!(
            "[net-console] handshake: got auth token len={} state={:?}",
            token.len(),
            self.state
        );

        if token != self.auth_token {
            warn!(
                "[cohsh-net][auth] reject: conn id={} invalid token (got_len={}, expected_len={})",
                self.conn_label(),
                token.len(),
                self.auth_token.len()
            );
            self.log_reject(REASON_INVALID_TOKEN, token);
            let _ = self.enqueue_auth_ack(AckStatus::Err, Some(DETAIL_REASON_INVALID_TOKEN));
            self.set_state(SessionState::Inactive);
            warn!("[cohsh-net][auth] closing session: reason=invalid-token");
            warn!("[net-console] auth failed reason=invalid-token");
            return SessionEvent::AuthFailed("invalid-token");
        }

        self.set_state(SessionState::Authenticated);
        self.auth_deadline_ms = None;
        let _ = self.enqueue_auth_ack(AckStatus::Ok, None);
        let preauth_stats = self.flush_preauth_buffer();
        self.enqueue_preauth_summary(preauth_stats, "auth-flush");
        info!(
            "[cohsh-net][auth] accepted client: conn_id={} role={:?}, version={:?}",
            self.conn_label(),
            role_str,
            1u8
        );
        info!("[net-console] auth ok");
        SessionEvent::Authenticated
    }

    fn conn_label(&self) -> u64 {
        self.conn_id.unwrap_or(0)
    }

    /// Return true if the authenticated client has been idle beyond the configured timeout.
    pub fn should_timeout(&self, now_ms: u64) -> bool {
        matches!(self.state, SessionState::Authenticated)
            && now_ms.saturating_sub(self.last_activity_ms) >= self.idle_timeout_ms
    }

    /// Return true if an unauthenticated client failed to present credentials in time.
    pub fn auth_timed_out(&self, now_ms: u64) -> bool {
        matches!(self.state, SessionState::WaitingAuth)
            && self
                .auth_deadline_ms
                .map(|deadline| now_ms >= deadline)
                .unwrap_or(false)
    }

    /// Forward buffered console lines to the provided visitor.
    pub fn drain_console_lines(
        &mut self,
        now_ms: u64,
        visitor: &mut dyn FnMut(ConsoleLine),
    ) {
        while let Some(line) = self.inbound.pop_front() {
            let latency = now_ms.saturating_sub(line.ingest_ms);
            self.ingest_metrics.record_latency_ms(latency);
            visitor(line);
        }
    }

    /// Snapshot ingest metrics for observability providers.
    pub fn ingest_snapshot(&self) -> IngestSnapshot {
        self.ingest_metrics.snapshot(self.inbound.len())
    }

    /// Queue a console response for transmission to the authenticated client.
    pub fn enqueue_outbound(&mut self, line: &str) -> Result<(), ()> {
        if line.trim().is_empty() {
            return Ok(());
        }
        let mut buf: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
        if buf.push_str(line).is_err() {
            return Err(());
        }
        let is_priority = Self::is_priority_line(buf.as_str());
        let is_sideband = Self::is_preauth_sideband(buf.as_str());
        if !self.is_authenticated() && !is_priority && !is_sideband {
            if !Self::is_preauth_allowed(buf.as_str()) {
                return Ok(());
            }
            self.enqueue_preauth(buf);
            return Ok(());
        }
        if is_sideband && !self.is_authenticated() {
            return self.enqueue_live(buf, true);
        }
        self.enqueue_live(buf, is_priority)
    }

    /// Return true when outbound data is buffered for transmission.
    pub fn has_outbound(&self) -> bool {
        !self.priority_outbound.is_empty() || !self.outbound.is_empty()
    }

    /// Peek at the next outbound line without removing it.
    pub fn peek_outbound(&self) -> Option<&HeaplessString<DEFAULT_LINE_CAPACITY>> {
        self.priority_outbound
            .front()
            .or_else(|| self.outbound.front())
    }

    /// Pop the next outbound console line, if any.
    pub fn pop_outbound(&mut self) -> Option<HeaplessString<DEFAULT_LINE_CAPACITY>> {
        self.priority_outbound
            .pop_front()
            .or_else(|| self.outbound.pop_front())
    }

    /// Requeue an outbound line at the front of the queue.
    pub fn push_outbound_front(&mut self, line: HeaplessString<DEFAULT_LINE_CAPACITY>) {
        if Self::is_priority_line(line.as_str()) {
            self.make_space_for_priority();
            if self.priority_outbound.push_front(line.clone()).is_err() {
                let _ = self
                    .priority_outbound
                    .pop_back()
                    .map(|line| self.insert_priority_into_outbound_front(line));
                let _ = self.priority_outbound.push_front(line);
            }
        } else if self.outbound.push_front(line).is_err() {
            let _ = self.outbound.pop_back();
        }
    }

    /// Returns `true` when a client is authenticated and actively connected.
    pub fn is_authenticated(&self) -> bool {
        matches!(self.state, SessionState::Authenticated)
    }

    /// Refresh the idle timer in response to transmitted data.
    pub fn mark_activity(&mut self, now_ms: u64) {
        if matches!(self.state, SessionState::Authenticated) {
            self.last_activity_ms = now_ms;
        }
    }

    fn enqueue_auth_ack(&mut self, status: AckStatus, detail: Option<&str>) -> Result<(), ()> {
        let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
        let ack = AckLine {
            status,
            verb: "AUTH",
            detail,
        };
        log::debug!(
            target: "net-console",
            "[tcp-console] enqueue ACK: status={:?} verb={:?} detail={:?}",
            ack.status,
            ack.verb,
            ack.detail,
        );
        let result = match render_ack(&mut line, &ack) {
            Ok(()) => {
                let len = line.len();
                if len > 0 {
                    self.push_outbound_front(line);
                }
                Ok(len)
            }
            Err(LineFormatError::Truncated) => {
                // Transport-level guard; fall back to a simple error string to avoid panics.
                let mut fallback: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
                let _ = fallback.push_str("ERR AUTH");
                let len = fallback.len();
                self.push_outbound_front(fallback);
                Ok(len)
            }
        };
        result.map(|len| {
            info!(
                "[cohsh-net] send: auth response len={} status={:?}",
                len, status
            );
        })
    }

    /// Buffer a console line while no authenticated client is attached, preserving
    /// the first observed messages and the most recent tail while aggregating
    /// drops to prevent log spam.
    fn enqueue_preauth(&mut self, line: HeaplessString<DEFAULT_LINE_CAPACITY>) {
        if self.preauth_first.len() < PREAUTH_FIRST_CAPACITY {
            if self.preauth_first.push_back(line).is_err() {
                self.note_preauth_drop();
            }
            return;
        }

        if self.preauth_last.is_full() {
            self.note_preauth_drop();
            let _ = self.preauth_last.pop_front();
        }
        if self.preauth_last.push_back(line).is_err() {
            self.note_preauth_drop();
        }
    }

    fn enqueue_live(
        &mut self,
        buf: HeaplessString<DEFAULT_LINE_CAPACITY>,
        is_priority: bool,
    ) -> Result<(), ()> {
        if is_priority {
            self.make_space_for_priority();
            if self.priority_outbound.push_back(buf.clone()).is_err() {
                warn!(
                    "[cohsh-net] priority outbound queue unexpectedly full; preserving latest critical line"
                );
                let _ = self
                    .priority_outbound
                    .pop_front()
                    .map(|line| self.insert_priority_into_outbound_front(line));
                self.priority_outbound.push_back(buf).map_err(|_| ())
            } else {
                Ok(())
            }
        } else {
            if self.outbound.is_full() {
                if let Some(dropped) = self.evict_oldest_non_priority() {
                    self.log_outbound_drop(dropped.as_str());
                }
            }
            let line_for_log = buf.clone();
            if self.outbound.push_back(buf).is_err() {
                self.log_outbound_drop(line_for_log.as_str());
                return Err(());
            }
            Ok(())
        }
    }

    fn log_outbound_drop(&mut self, line: &str) {
        self.outbound_drops = self.outbound_drops.saturating_add(1);
        if self.outbound_drops == 1 || self.outbound_drops.is_power_of_two() {
            warn!(
                "[cohsh-net] outbound queue saturated (drops={}) line='{}'",
                self.outbound_drops, line
            );
        }
    }

    /// Move any pre-auth lines into the live queues and emit a single summary when
    /// earlier lines were dropped.
    fn flush_preauth_buffer(&mut self) -> PreauthFlushStats {
        let mut stats = PreauthFlushStats {
            flushed: 0,
            dropped: self.preauth_drop_count,
        };

        while let Some(line) = self.preauth_first.pop_front() {
            let is_priority = Self::is_priority_line(line.as_str());
            let _ = self.enqueue_live(line, is_priority);
            stats.flushed = stats.flushed.saturating_add(1);
        }

        while let Some(line) = self.preauth_last.pop_front() {
            let is_priority = Self::is_priority_line(line.as_str());
            let _ = self.enqueue_live(line, is_priority);
            stats.flushed = stats.flushed.saturating_add(1);
        }

        self.clear_preauth_epoch();
        stats
    }

    fn enqueue_preauth_summary(&mut self, stats: PreauthFlushStats, reason: &'static str) {
        if stats.dropped == 0 && stats.flushed == 0 {
            return;
        }
        let mut summary: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
        if write!(
            summary,
            "[net-console] pre-auth summary reason={} flushed={} dropped={}",
            reason, stats.flushed, stats.dropped
        )
        .is_err()
        {
            let _ = summary.push_str("[net-console] pre-auth summary");
        }
        info!(
            "[cohsh-net] pre-auth summary: reason={} flushed={} dropped={}",
            reason, stats.flushed, stats.dropped
        );
        if self.is_authenticated() {
            let _ = self.enqueue_live(summary, true);
        }
    }

    fn note_preauth_drop(&mut self) {
        self.preauth_drop_count = self.preauth_drop_count.saturating_add(1);
        if !self.preauth_drop_warned {
            warn!(
                "[cohsh-net] pre-auth buffer saturated (drops={})",
                self.preauth_drop_count
            );
            self.preauth_drop_warned = true;
        }
    }

    pub(crate) fn is_priority_line(line: &str) -> bool {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        trimmed.starts_with("OK ") || trimmed.starts_with("ERR ") || trimmed == "END"
    }

    fn is_preauth_allowed(line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return false;
        }
        if Self::is_priority_line(trimmed)
            || trimmed.starts_with("WARN")
            || trimmed.starts_with("ERR")
            || trimmed.starts_with("ERROR")
        {
            return true;
        }
        if trimmed.starts_with("INFO") {
            return PREAUTH_INFO_ALLOWLIST
                .iter()
                .any(|prefix| trimmed.contains(prefix) || trimmed.starts_with(prefix));
        }
        PREAUTH_INFO_ALLOWLIST
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
    }

    pub(crate) fn is_preauth_sideband(line: &str) -> bool {
        let trimmed = line.trim_start();
        trimmed.starts_with("[net-console] pre-auth")
            || trimmed.starts_with("[net-console] authenticate")
            || trimmed.starts_with("[net-console] preauth")
    }

    pub(crate) fn is_preauth_transmit_allowed(line: &str) -> bool {
        line.starts_with("OK AUTH")
            || line.starts_with("ERR AUTH")
            || Self::is_preauth_sideband(line)
    }

    fn evict_oldest_non_priority(&mut self) -> Option<HeaplessString<DEFAULT_LINE_CAPACITY>> {
        let mut scratch: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, CONSOLE_QUEUE_DEPTH> =
            HeaplessVec::new();
        let mut dropped: Option<HeaplessString<DEFAULT_LINE_CAPACITY>> = None;
        while let Some(line) = self.outbound.pop_front() {
            if dropped.is_none() && !Self::is_priority_line(line.as_str()) {
                dropped = Some(line);
                continue;
            }
            let _ = scratch.push(line);
        }
        for line in scratch {
            let _ = self.outbound.push_back(line);
        }
        dropped
    }

    fn make_space_for_priority(&mut self) {
        if !self.priority_outbound.is_full() {
            return;
        }

        if let Some(dropped) = self.evict_oldest_non_priority() {
            self.log_outbound_drop(dropped.as_str());
        }

        if self.priority_outbound.is_full() {
            if let Some(line) = self.priority_outbound.pop_front() {
                self.insert_priority_into_outbound_front(line);
            }
        }
    }

    fn insert_priority_into_outbound_front(&mut self, line: HeaplessString<DEFAULT_LINE_CAPACITY>) {
        if self.outbound.is_full() {
            if let Some(dropped) = self.evict_oldest_non_priority() {
                self.log_outbound_drop(dropped.as_str());
            }
        }
        if self.outbound.push_front(line.clone()).is_err() {
            let _ = self.outbound.pop_back();
            let _ = self.outbound.push_front(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "changeme";

    fn frame_line<const N: usize>(line: &str) -> HeaplessVec<u8, N> {
        let mut buf = HeaplessVec::new();
        let total_len = line.len().saturating_add(4);
        let len: u32 = total_len.try_into().unwrap_or(u32::MAX);
        let _ = buf.extend_from_slice(&len.to_le_bytes());
        let _ = buf.extend_from_slice(line.as_bytes());
        buf
    }

    #[test]
    fn auth_success_emits_ack() {
        let mut server = TcpConsoleServer::new(TOKEN, 10_000);
        server.begin_session(0, Some(1));

        let payload = frame_line::<{ DEFAULT_LINE_CAPACITY + 8 }>("AUTH changeme");
        let event = server.ingest(payload.as_slice(), 1);

        assert_eq!(event, SessionEvent::Authenticated);
        assert!(server.is_authenticated());

        let mut outbound = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        let ack = server.pop_outbound().expect("auth ack missing");
        outbound.push_str(ack.as_str()).unwrap();
        assert_eq!(outbound.as_str(), "OK AUTH");
    }

    #[test]
    fn auth_failure_rejects_bad_token() {
        let mut server = TcpConsoleServer::new(TOKEN, 10_000);
        server.begin_session(0, Some(2));

        let payload = frame_line::<{ DEFAULT_LINE_CAPACITY + 8 }>("AUTH wrongtok");
        let event = server.ingest(payload.as_slice(), 1);

        assert_eq!(event, SessionEvent::AuthFailed("invalid-token"));
        assert!(!server.is_authenticated());

        let ack = server.pop_outbound().expect("error ack missing");
        assert!(ack.starts_with("ERR AUTH"));
    }

    #[test]
    fn oversized_frame_after_auth_emits_err_frame() {
        let mut server = TcpConsoleServer::new(TOKEN, 10_000);
        server.begin_session(0, Some(4));

        let auth_payload = frame_line::<{ DEFAULT_LINE_CAPACITY + 8 }>("AUTH changeme");
        let event = server.ingest(auth_payload.as_slice(), 1);
        assert_eq!(event, SessionEvent::Authenticated);
        let _ = server.pop_outbound();

        let declared = (MAX_MSIZE as usize + 1) as u32;
        let mut payload: HeaplessVec<u8, 8> = HeaplessVec::new();
        payload.extend_from_slice(&declared.to_le_bytes()).unwrap();
        payload.push(0).unwrap();

        let event = server.ingest(payload.as_slice(), 2);
        assert_eq!(event, SessionEvent::None);
        assert!(server.is_authenticated());

        let ack = server.pop_outbound().expect("frame error ack missing");
        assert_eq!(ack.as_str(), "ERR FRAME reason=invalid-length");
    }
    #[test]
    fn authenticates_and_tracks_activity() {
        let mut server = TcpConsoleServer::new("token", 1000);
        server.begin_session(10, Some(1));

        let payload = frame_line::<{ DEFAULT_LINE_CAPACITY + 8 }>("AUTH token");
        let event = server.ingest(payload.as_slice(), 11);
        assert_eq!(event, SessionEvent::Authenticated);
        assert!(server.is_authenticated());

        let ack = server.pop_outbound().expect("auth ack present");
        assert!(ack.starts_with("OK AUTH"));

        server.mark_activity(20);
        assert!(!server.should_timeout(1000));
    }

    #[test]
    fn auth_timeout_triggers_and_resets() {
        let mut server = TcpConsoleServer::new("token", 1000);
        server.begin_session(0, Some(1));

        assert!(!server.auth_timed_out(1));
        assert!(server.auth_timed_out(AUTH_TIMEOUT_MS + 1));

        server.end_session();
        assert!(!server.auth_timed_out(AUTH_TIMEOUT_MS + 2));
    }

    #[test]
    fn rejects_invalid_auth_and_marks_session_inactive() {
        let mut server = TcpConsoleServer::new("token", 1000);
        server.begin_session(0, Some(1));

        let payload = frame_line::<{ DEFAULT_LINE_CAPACITY + 8 }>("AUTH wrong");
        let event = server.ingest(payload.as_slice(), 1);
        assert_eq!(event, SessionEvent::AuthFailed("invalid-token"));
        assert!(!server.is_authenticated());

        let ack = server.pop_outbound().expect("ack present");
        assert!(ack.starts_with("ERR AUTH"));

        server.end_session();
        assert!(!server.should_timeout(2000));
    }

    #[test]
    fn drops_whitespace_only_outbound_lines() {
        let mut server = TcpConsoleServer::new(TOKEN, 10_000);
        server.begin_session(0, Some(3));

        assert!(server.enqueue_outbound("   \t").is_ok());
        assert!(!server.has_outbound());
    }

    #[test]
    fn buffers_preauth_lines_and_flushes_on_auth() {
        let mut server = TcpConsoleServer::new(TOKEN, 10_000);
        for idx in 0..(PREAUTH_FIRST_CAPACITY + PREAUTH_LAST_CAPACITY + 2) {
            let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
            write!(&mut line, "line-{idx}").unwrap();
            assert!(server.enqueue_outbound(line.as_str()).is_ok());
        }

        assert!(
            !server.has_outbound(),
            "pre-auth lines must not enter live queue"
        );

        server.begin_session(0, Some(4));
        let auth_payload = frame_line::<{ DEFAULT_LINE_CAPACITY + 8 }>("AUTH changeme");
        let event = server.ingest(auth_payload.as_slice(), 1);
        assert_eq!(event, SessionEvent::Authenticated);

        let mut drained: HeaplessVec<
            HeaplessString<DEFAULT_LINE_CAPACITY>,
            { PREAUTH_FIRST_CAPACITY + PREAUTH_LAST_CAPACITY + 4 },
        > = HeaplessVec::new();
        while let Some(line) = server.pop_outbound() {
            drained.push(line).unwrap();
        }

        assert!(
            drained.iter().any(|line| line.starts_with("OK AUTH")),
            "auth acknowledgement missing ({:?})",
            drained
        );
        assert!(
            drained.iter().any(|line| line.starts_with(
                "[net-console] pre-auth summary reason=auth-flush flushed=8 dropped=2"
            )),
            "pre-auth drop summary missing: {:?}",
            drained
        );
        let mut payloads = drained
            .iter()
            .filter(|line| line.as_str().starts_with("line-"))
            .map(|line| line.as_str())
            .collect::<HeaplessVec<&str, { PREAUTH_FIRST_CAPACITY + PREAUTH_LAST_CAPACITY + 2 }>>();
        payloads.sort_unstable();
        let mut expected: HeaplessVec<
            HeaplessString<DEFAULT_LINE_CAPACITY>,
            { PREAUTH_FIRST_CAPACITY + PREAUTH_LAST_CAPACITY },
        > = HeaplessVec::new();
        for idx in 0..PREAUTH_FIRST_CAPACITY {
            let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
            write!(&mut line, "line-{idx}").unwrap();
            expected.push(line).unwrap();
        }
        for idx in
            (PREAUTH_FIRST_CAPACITY + 2)..(PREAUTH_FIRST_CAPACITY + PREAUTH_LAST_CAPACITY + 2)
        {
            let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
            write!(&mut line, "line-{idx}").unwrap();
            expected.push(line).unwrap();
        }
        for expected_line in expected {
            assert!(
                payloads.contains(&expected_line.as_str()),
                "missing buffered line {} ({:?})",
                expected_line,
                payloads
            );
        }
    }

    #[test]
    fn drop_warning_edges_and_resets_after_flush() {
        let mut server = TcpConsoleServer::new(TOKEN, 10_000);
        server.begin_session(0, Some(7));
        for idx in 0..(PREAUTH_FIRST_CAPACITY + PREAUTH_LAST_CAPACITY + 4) {
            let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
            write!(&mut line, "line-{idx}").unwrap();
            server.enqueue_outbound(line.as_str()).unwrap();
        }
        assert!(server.preauth_drop_warned);
        let stats = server.flush_preauth_buffer();
        assert_eq!(
            stats.dropped, 4,
            "unexpected drop count with two-tier buffer"
        );
        assert!(!server.preauth_drop_warned, "drop warning did not reset");
    }

    #[test]
    fn preauth_clamping_filters_unwanted_noise() {
        let mut server = TcpConsoleServer::new(TOKEN, 10_000);
        server.begin_session(0, Some(8));
        server.enqueue_outbound("INFO noisy").unwrap();
        server.enqueue_outbound("DEBUG skip").unwrap();
        server
            .enqueue_outbound("[net-console] allowed info")
            .unwrap();
        server.enqueue_outbound("WARN keep").unwrap();

        assert_eq!(server.preauth_first.len(), 2);
        let mut lines: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, 4> = HeaplessVec::new();
        while let Some(line) = server.preauth_first.pop_front() {
            lines.push(line).unwrap();
        }
        let contents: HeaplessVec<HeaplessString<DEFAULT_LINE_CAPACITY>, 4> =
            lines.iter().cloned().collect();
        assert!(contents
            .iter()
            .any(|l| l.as_str() == "[net-console] allowed info"));
        assert!(contents.iter().any(|l| l.as_str() == "WARN keep"));
    }
}
