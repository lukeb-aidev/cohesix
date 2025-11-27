// Author: Lukas Bower

//! TCP console session management shared between kernel and host stacks.

use heapless::{Deque, String as HeaplessString};

use super::CONSOLE_QUEUE_DEPTH;
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
}

impl TcpConsoleServer {
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
        }
    }

    /// Reset the session state in preparation for a new client connection.
    pub fn begin_session(&mut self, now_ms: u64) {
        self.state = SessionState::WaitingAuth;
        self.line_buffer.clear();
        self.inbound.clear();
        self.outbound.clear();
        self.last_activity_ms = now_ms;
    }

    /// Tear down any per-connection state.
    pub fn end_session(&mut self) {
        self.state = SessionState::Inactive;
        self.line_buffer.clear();
        self.inbound.clear();
        self.outbound.clear();
        self.last_activity_ms = 0;
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
        let trimmed = line.trim();
        if !trimmed.starts_with(AUTH_PREFIX) {
            let _ = self.enqueue_auth_ack(AckStatus::Err, Some("reason=expected-token"));
            return SessionEvent::Close;
        }
        let token = trimmed.split_at(AUTH_PREFIX.len()).1.trim();
        if token != self.auth_token {
            let _ = self.enqueue_auth_ack(AckStatus::Err, Some("reason=invalid-token"));
            return SessionEvent::Close;
        }
        self.state = SessionState::Authenticated;
        let _ = self.enqueue_auth_ack(AckStatus::Ok, None);
        SessionEvent::Authenticated
    }

    /// Return true if the authenticated client has been idle beyond the configured timeout.
    pub fn should_timeout(&self, now_ms: u64) -> bool {
        matches!(self.state, SessionState::Authenticated)
            && now_ms.saturating_sub(self.last_activity_ms) >= self.idle_timeout_ms
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
    }
}
