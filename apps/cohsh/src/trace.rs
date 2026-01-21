// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide trace-aware Secure9P transports for cohsh.
// Author: Lukas Bower

use std::collections::VecDeque;
use std::fmt;

use anyhow::{anyhow, Context, Result};
use cohesix_ticket::Role;
use cohsh_core::trace::{TraceLogBuilderRef, TraceError};
use cohsh_core::wire::{render_ack, AckLine, AckStatus};
use cohsh_core::{role_label, ConsoleVerb, Secure9pTransport};
use secure9p_codec::OpenMode;

use crate::client::{CohClient, TailEvent};
use crate::{Session, Transport, TransportMetrics, QUEEN_CTL_PATH};

/// Ack handling mode for trace record/replay.
pub enum TraceAckMode {
    /// Do not record or verify acknowledgements.
    None,
    /// Record acknowledgements into the shared trace builder.
    Record(TraceLogBuilderRef),
    /// Verify acknowledgements against recorded trace lines.
    Verify {
        /// Expected acknowledgement lines in replay order.
        expected: Vec<String>,
        /// Index of the next acknowledgement to verify.
        index: usize,
    },
}

impl TraceAckMode {
    fn record(&mut self, line: &str) -> Result<()> {
        match self {
            TraceAckMode::None => Ok(()),
            TraceAckMode::Record(builder) => builder
                .borrow_mut()
                .record_ack(line)
                .map_err(map_trace_error),
            TraceAckMode::Verify { expected, index } => {
                if *index >= expected.len() {
                    return Err(anyhow!("trace ack mismatch: missing expected ack"));
                }
                let expected_line = &expected[*index];
                if expected_line != line {
                    return Err(anyhow!(
                        "trace ack mismatch: expected '{}' got '{}'",
                        expected_line,
                        line
                    ));
                }
                *index = index.saturating_add(1);
                Ok(())
            }
        }
    }

    /// Ensure all expected acknowledgements were consumed.
    pub fn finish(&self) -> Result<()> {
        if let TraceAckMode::Verify { expected, index } = self {
            if *index != expected.len() {
                return Err(anyhow!(
                    "trace ack mismatch: {} acks remaining",
                    expected.len().saturating_sub(*index)
                ));
            }
        }
        Ok(())
    }
}

/// Secure9P transport wrapper for cohsh that emits console-style acknowledgements.
pub struct TraceShellTransport<T: Secure9pTransport> {
    factory: Box<dyn FnMut() -> Result<T>>,
    client: Option<CohClient<T>>,
    ack_lines: VecDeque<String>,
    ack_mode: TraceAckMode,
    next_session_id: u64,
    label: &'static str,
}

impl<T: Secure9pTransport> fmt::Debug for TraceShellTransport<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TraceShellTransport")
            .field("label", &self.label)
            .field("session_id", &self.next_session_id)
            .finish()
    }
}

impl<T: Secure9pTransport> TraceShellTransport<T> {
    /// Construct a trace-aware transport with a transport factory and ack mode.
    pub fn new(
        factory: Box<dyn FnMut() -> Result<T>>,
        ack_mode: TraceAckMode,
        label: &'static str,
    ) -> Self {
        Self {
            factory,
            client: None,
            ack_lines: VecDeque::new(),
            ack_mode,
            next_session_id: 1,
            label,
        }
    }

    /// Drain recorded acknowledgements and verify expected trace consumption.
    pub fn finish(&self) -> Result<()> {
        self.ack_mode.finish()
    }

    fn push_ack(&mut self, status: AckStatus, verb: &str, detail: Option<&str>) -> Result<()> {
        let mut line = String::new();
        let ack = AckLine { status, verb, detail };
        render_ack(&mut line, &ack).map_err(|err| anyhow!("ack render failed: {err}"))?;
        self.ack_mode.record(&line)?;
        self.ack_lines.push_back(line);
        Ok(())
    }

    fn read_lines(&mut self, path: &str) -> Result<Vec<String>> {
        let client = self
            .client
            .as_mut()
            .context("attach to a session before reading")?;
        let fid = client.open(path, OpenMode::read_only())?;
        let mut offset = 0u64;
        let mut buffer = Vec::new();
        loop {
            let chunk = client.read(fid, offset, client.negotiated_msize())?;
            if chunk.is_empty() {
                break;
            }
            offset = offset
                .checked_add(chunk.len() as u64)
                .context("offset overflow during read")?;
            buffer.extend_from_slice(&chunk);
            if chunk.len() < client.negotiated_msize() as usize {
                break;
            }
        }
        client.clunk(fid).ok();
        let text = String::from_utf8(buffer).context("log is not valid UTF-8")?;
        Ok(text.lines().map(|line| line.to_owned()).collect())
    }
}

impl<T: Secure9pTransport> Transport for TraceShellTransport<T> {
    fn attach(&mut self, role: Role, ticket: Option<&str>) -> Result<Session> {
        if self.client.is_some() {
            return Err(anyhow!(
                "already attached; run 'quit' to close the current session"
            ));
        }
        let transport = (self.factory)()?;
        let connect = CohClient::connect(transport, role, ticket);
        let client = match connect {
            Ok(client) => client,
            Err(err) => {
                let detail = format!("reason={err}");
                let _ = self.push_ack(
                    AckStatus::Err,
                    ConsoleVerb::Attach.ack_label(),
                    Some(detail.as_str()),
                );
                return Err(err);
            }
        };
        let session = Session::new(
            secure9p_codec::SessionId::from_raw(self.next_session_id),
            role,
        );
        self.next_session_id = self.next_session_id.saturating_add(1);
        self.client = Some(client);
        let detail = format!("role={}", role_label(role));
        self.push_ack(
            AckStatus::Ok,
            ConsoleVerb::Attach.ack_label(),
            Some(detail.as_str()),
        )?;
        Ok(session)
    }

    fn kind(&self) -> &'static str {
        self.label
    }

    fn ping(&mut self, session: &Session) -> Result<String> {
        let client = self
            .client
            .as_mut()
            .context("attach to a session before running ping")?;
        let fid = client.open("/", OpenMode::read_only())?;
        let _ = client.clunk(fid);
        Ok(format!("attached as {:?} via {}", session.role(), self.label))
    }

    fn tail(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        let result = (|| {
            let client = self
                .client
                .as_mut()
                .context("attach to a session before running tail")?;
            let mut stream = client.tail(path)?;
            let mut lines = Vec::new();
            while let Some(event) = stream.next() {
                match event? {
                    TailEvent::Line(line) => lines.push(line),
                    TailEvent::End => lines.push(cohsh_core::END_LINE.to_owned()),
                }
            }
            Ok(lines)
        })();
        match result {
            Ok(lines) => {
                let detail = format!("path={path}");
                self.push_ack(
                    AckStatus::Ok,
                    ConsoleVerb::Tail.ack_label(),
                    Some(detail.as_str()),
                )?;
                Ok(lines)
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                let _ = self.push_ack(
                    AckStatus::Err,
                    ConsoleVerb::Tail.ack_label(),
                    Some(detail.as_str()),
                );
                Err(err)
            }
        }
    }

    fn read(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        match self.read_lines(path) {
            Ok(lines) => {
                let detail = format!("path={path}");
                self.push_ack(
                    AckStatus::Ok,
                    ConsoleVerb::Cat.ack_label(),
                    Some(detail.as_str()),
                )?;
                Ok(lines)
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                let _ = self.push_ack(
                    AckStatus::Err,
                    ConsoleVerb::Cat.ack_label(),
                    Some(detail.as_str()),
                );
                Err(err)
            }
        }
    }

    fn list(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
        match self.read_lines(path) {
            Ok(lines) => {
                let detail = format!("path={path}");
                self.push_ack(
                    AckStatus::Ok,
                    ConsoleVerb::Ls.ack_label(),
                    Some(detail.as_str()),
                )?;
                Ok(lines)
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                let _ = self.push_ack(
                    AckStatus::Err,
                    ConsoleVerb::Ls.ack_label(),
                    Some(detail.as_str()),
                );
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
        let fid = {
            let client = self
                .client
                .as_mut()
                .context("attach to a session before running write")?;
            client.open(path, OpenMode::write_append())
        };
        let fid = match fid {
            Ok(fid) => fid,
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                let _ = self.push_ack(AckStatus::Err, verb, Some(detail.as_str()));
                return Err(err);
            }
        };
        let result = (|| {
            let client = self
                .client
                .as_mut()
                .context("attach to a session before running write")?;
            let written = client.write(fid, u64::MAX, payload)?;
            if written as usize != payload.len() {
                return Err(anyhow!(
                    "short write to {path}: expected {} bytes, wrote {written}",
                    payload.len()
                ));
            }
            client.clunk(fid).ok();
            Ok(())
        })();
        match result {
            Ok(()) => {
                let detail = format!("path={path} bytes={}", payload.len());
                self.push_ack(AckStatus::Ok, verb, Some(detail.as_str()))?;
                Ok(())
            }
            Err(err) => {
                let detail = format!("path={path} reason={err}");
                let _ = self.push_ack(AckStatus::Err, verb, Some(detail.as_str()));
                if let Some(client) = self.client.as_mut() {
                    let _ = client.clunk(fid);
                }
                Err(err)
            }
        }
    }

    fn quit(&mut self, _session: &Session) -> Result<()> {
        self.push_ack(AckStatus::Ok, ConsoleVerb::Quit.ack_label(), None)?;
        self.client = None;
        Ok(())
    }

    fn drain_acknowledgements(&mut self) -> Vec<String> {
        self.ack_lines.drain(..).collect()
    }

    fn metrics(&self) -> TransportMetrics {
        TransportMetrics::default()
    }
}

fn map_trace_error(err: TraceError) -> anyhow::Error {
    anyhow!("trace error: {err}")
}
