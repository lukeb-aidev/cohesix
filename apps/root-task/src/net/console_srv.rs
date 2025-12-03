// Author: Lukas Bower

//! TCP console session management shared between kernel and host stacks.

use heapless::{Deque, String as HeaplessString};
use log::{debug, info, warn};

use super::{AUTH_TIMEOUT_MS, CONSOLE_QUEUE_DEPTH};
use crate::console::proto::{render_ack, AckLine, AckStatus, LineFormatError};
use crate::serial::DEFAULT_LINE_CAPACITY;

// Transport-level guard to prevent unauthenticated TCP sessions from issuing console verbs.
// Application-layer ticket and role checks are enforced by the console/event pump.
const AUTH_PREFIX: &str = "AUTH ";

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

/// State machine that validates authentication tokens and buffers console lines.
pub struct TcpConsoleServer {
    auth_token: &'static str,
    idle_timeout_ms: u64,
    state: SessionState,
    line_buffer: HeaplessString<DEFAULT_LINE_CAPACITY>,
    inbound: Deque<HeaplessString<DEFAULT_LINE_CAPACITY>, CONSOLE_QUEUE_DEPTH>,
    outbound: Deque<HeaplessString<DEFAULT_LINE_CAPACITY>, CONSOLE_QUEUE_DEPTH>,
    last_activity_ms: u64,
    auth_deadline_ms: Option<u64>,
    conn_id: Option<u64>,
}

impl TcpConsoleServer {
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
        AUTH_PREFIX
            .len()
            .saturating_add(self.auth_token.len())
            .saturating_add(1)
    }

    /// Construct a new server that validates the provided authentication token.
    pub fn new(auth_token: &'static str, idle_timeout_ms: u64) -> Self {
        Self {
            auth_token,
            idle_timeout_ms,
            state: SessionState::Inactive,
            line_buffer: HeaplessString::new(),
            inbound: Deque::new(),
            outbound: Deque::new(),
            last_activity_ms: 0,
            auth_deadline_ms: None,
            conn_id: None,
        }
    }

    /// Reset the session state in preparation for a new client connection.
    pub fn begin_session(&mut self, now_ms: u64, conn_id: Option<u64>) {
        self.set_state(SessionState::WaitingAuth);
        self.line_buffer.clear();
        self.inbound.clear();
        self.outbound.clear();
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
        if self
            .enqueue_auth_ack(AckStatus::Ok, Some("detail=present-token"))
            .is_ok()
        {
            self.auth_deadline_ms = Some(now_ms.saturating_add(AUTH_TIMEOUT_MS));
        }
        info!("[net-console] auth begin (challenge sent)");
        info!("[net-console] auth: waiting for handshake payload");
    }

    /// Tear down any per-connection state.
    pub fn end_session(&mut self) {
        self.set_state(SessionState::Inactive);
        self.line_buffer.clear();
        self.inbound.clear();
        self.outbound.clear();
        self.last_activity_ms = 0;
        self.auth_deadline_ms = None;
        self.conn_id = None;
    }

    /// Consume bytes received from the client, returning any resulting session event.
    pub fn ingest(&mut self, payload: &[u8], now_ms: u64) -> SessionEvent {
        if matches!(self.state, SessionState::Inactive) {
            // Connection was closed before authentication; drop stray bytes.
            return SessionEvent::Close;
        }

        let mut event = SessionEvent::None;
        for &byte in payload {
            match byte {
                b'\r' => {}
                b'\n' => {
                    if self.line_buffer.is_empty() {
                        continue;
                    }
                    #[cfg(feature = "net-trace-31337")]
                    info!(
                        "[cohsh-net] conn id={} auth: received len={} bytes={:02x?}",
                        self.conn_label(),
                        self.line_buffer.len().saturating_add(1),
                        self.line_buffer.as_bytes()
                    );
                    let line = self.line_buffer.clone();
                    self.line_buffer.clear();
                    self.last_activity_ms = now_ms;
                    event = self.handle_line(line);
                    if matches!(event, SessionEvent::Close) {
                        break;
                    }
                }
                0x08 | 0x7f => {
                    let _ = self.line_buffer.pop();
                }
                byte if byte.is_ascii() && !byte.is_ascii_control() => {
                    let _ = self.line_buffer.push(byte as char);
                }
                _ => {}
            }
        }

        event
    }

    fn handle_line(&mut self, line: HeaplessString<DEFAULT_LINE_CAPACITY>) -> SessionEvent {
        match self.state {
            SessionState::WaitingAuth => self.process_auth(line),
            SessionState::Authenticated => {
                let line_clone = line.clone();
                if self.inbound.push_back(line).is_err() {
                    // Drop oldest to make space for high-priority lines.
                    let _ = self.inbound.pop_front();
                    let _ = self.inbound.push_back(line_clone);
                }
                SessionEvent::None
            }
            SessionState::Inactive => SessionEvent::Close,
        }
    }

    fn process_auth(&mut self, line: HeaplessString<DEFAULT_LINE_CAPACITY>) -> SessionEvent {
        // Expected client hello: ASCII "AUTH " prefix, role/token payload, trailing '\n'.
        // The TCP layer strips the newline before passing the line here.
        let raw_bytes = line.as_bytes();
        log::info!(
            "[cohsh-net][auth] parsing auth frame ({} bytes): {:02x?}",
            raw_bytes.len(),
            &raw_bytes[..core::cmp::min(raw_bytes.len(), 32)]
        );
        let expected_len = self.expected_frame_len();
        let observed_len = raw_bytes.len().saturating_add(1);
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
            self.log_reject("invalid-length", line.as_str());
            let _ = self.enqueue_auth_ack(AckStatus::Err, Some("reason=invalid-length"));
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
            self.log_reject("expected-token", line.as_str());
            let _ = self.enqueue_auth_ack(AckStatus::Err, Some("reason=expected-token"));
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
            self.log_reject("expected-token", line.as_str());
            let _ = self.enqueue_auth_ack(AckStatus::Err, Some("reason=expected-token"));
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
            self.log_reject("invalid-token", token);
            let _ = self.enqueue_auth_ack(AckStatus::Err, Some("reason=invalid-token"));
            self.set_state(SessionState::Inactive);
            warn!("[cohsh-net][auth] closing session: reason=invalid-token");
            warn!("[net-console] auth failed reason=invalid-token");
            return SessionEvent::AuthFailed("invalid-token");
        }

        self.set_state(SessionState::Authenticated);
        self.auth_deadline_ms = None;
        let _ = self.enqueue_auth_ack(AckStatus::Ok, None);
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
        visitor: &mut dyn FnMut(HeaplessString<DEFAULT_LINE_CAPACITY>),
    ) {
        while let Some(line) = self.inbound.pop_front() {
            visitor(line);
        }
    }

    /// Queue a console response for transmission to the authenticated client.
    pub fn enqueue_outbound(&mut self, line: &str) -> Result<(), ()> {
        let mut buf: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
        if buf.push_str(line).is_err() {
            return Err(());
        }
        if self.outbound.push_back(buf.clone()).is_err() {
            let _ = self.outbound.pop_front();
            self.outbound.push_back(buf).map_err(|_| ())
        } else {
            Ok(())
        }
    }

    /// Pop the next outbound console line, if any.
    pub fn pop_outbound(&mut self) -> Option<HeaplessString<DEFAULT_LINE_CAPACITY>> {
        self.outbound.pop_front()
    }

    /// Requeue an outbound line at the front of the queue.
    pub fn push_outbound_front(&mut self, line: HeaplessString<DEFAULT_LINE_CAPACITY>) {
        if self.outbound.push_front(line).is_err() {
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
        match render_ack(&mut line, &ack) {
            Ok(()) => self.enqueue_outbound(line.as_str()),
            Err(LineFormatError::Truncated) => {
                // Transport-level guard; fall back to a simple error string to avoid panics.
                self.enqueue_outbound("ERR AUTH")
            }
        }
        .map(|result| {
            info!(
                "[cohsh-net] send: auth response len={} status={:?}",
                line.len(),
                status
            );
            result
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticates_and_tracks_activity() {
        let mut server = TcpConsoleServer::new("token", 1000);
        server.begin_session(10, Some(1));

        let ready = server.pop_outbound().expect("ready ack present");
        assert!(ready.starts_with("OK AUTH"));

        let event = server.ingest(b"AUTH token\n", 11);
        assert_eq!(event, SessionEvent::Authenticated);
        assert!(server.is_authenticated());

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

        let event = server.ingest(b"AUTH wrong\n", 1);
        assert_eq!(event, SessionEvent::AuthFailed("invalid-token"));
        assert!(!server.is_authenticated());

        let ack = server.pop_outbound().expect("ack present");
        assert!(ack.starts_with("ERR AUTH"));

        server.end_session();
        assert!(!server.should_timeout(2000));
    }
}
