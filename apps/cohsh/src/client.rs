// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide a reusable Secure9P client API for Cohesix host tooling.
// Author: Lukas Bower

//! Cohesix Secure9P client helpers layered on cohsh-core transport primitives.

use std::collections::VecDeque;

use anyhow::{anyhow, Context, Result};
use cohesix_ticket::Role;
use cohsh_core::{
    normalize_ticket, Secure9pClient, Secure9pError, Secure9pTransport, TicketPolicy,
};
use secure9p_codec::{OpenMode, Qid};

const ROOT_FID: u32 = 1;

/// Events emitted by a tail stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailEvent {
    /// Payload line read from the tail target.
    Line(String),
    /// End-of-stream marker matching console semantics.
    End,
}

/// Streaming helper returned by `CohClient::tail`.
pub struct TailStream<'a, T: Secure9pTransport> {
    client: &'a mut CohClient<T>,
    fid: u32,
    offset: u64,
    buffer: Vec<u8>,
    pending: VecDeque<String>,
    finished: bool,
    closed: bool,
}

/// Secure9P client exposing Cohesix file operations.
pub struct CohClient<T: Secure9pTransport> {
    core: Secure9pClient<T>,
    next_fid: u32,
    walk_depth: usize,
    root_qid: Qid,
}

impl<T: Secure9pTransport> CohClient<T> {
    /// Attach to the Secure9P server using the provided role and ticket.
    pub fn connect(transport: T, role: Role, ticket: Option<&str>) -> Result<Self> {
        let mut core = Secure9pClient::new(transport);
        core.version(crate::generated_client::SECURE9P_MSIZE)
            .map_err(|err| anyhow!("version negotiation failed: {err}"))?;
        let ticket_check =
            normalize_ticket(role, ticket, TicketPolicy::ninedoor()).map_err(|err| match err {
                cohsh_core::TicketError::Missing => anyhow!(
                    "role {:?} requires a capability ticket containing an identity",
                    role
                ),
                cohsh_core::TicketError::TooLong(max) => {
                    anyhow!("ticket payload exceeds {max} bytes")
                }
                cohsh_core::TicketError::Invalid(inner) => anyhow!("invalid ticket: {inner}"),
                cohsh_core::TicketError::RoleMismatch { expected, found } => anyhow!(
                    "ticket role {:?} does not match requested role {:?}",
                    found,
                    expected
                ),
                cohsh_core::TicketError::MissingSubject => anyhow!(
                    "ticket is missing required subject identity for role {:?}",
                    role
                ),
            })?;
        let identity = ticket_check
            .claims
            .as_ref()
            .and_then(|claims| claims.subject.as_deref());
        let root_qid = core
            .attach(ROOT_FID, role, identity, ticket_check.ticket)
            .map_err(|err| anyhow!("attach request failed: {err}"))?;
        Ok(Self {
            core,
            next_fid: ROOT_FID + 1,
            walk_depth: crate::generated_client::SECURE9P_WALK_DEPTH as usize,
            root_qid,
        })
    }

    /// Walk and open the supplied path, returning a live fid.
    pub fn open(&mut self, path: &str, mode: OpenMode) -> Result<u32> {
        let components = self.parse_path(path)?;
        let fid = self.allocate_fid();
        self.core
            .walk(ROOT_FID, fid, &components)
            .map_err(|err| anyhow!("failed to walk to {path}: {err}"))?;
        let open_result = self
            .core
            .open(fid, mode)
            .map_err(|err| anyhow!("failed to open {path}: {err}"));
        if open_result.is_err() {
            let _ = self.core.clunk(fid);
        }
        open_result.map(|_| fid)
    }

    /// Walk to the supplied path, returning a fid and Qid without opening.
    pub fn walk_qid(&mut self, path: &str) -> Result<(u32, Qid)> {
        let components = self.parse_path(path)?;
        let fid = self.allocate_fid();
        let qid = if components.is_empty() {
            self.core
                .walk(ROOT_FID, fid, &components)
                .map_err(|err| anyhow!("failed to walk to {path}: {err}"))?;
            self.root_qid
        } else {
            let qids = self
                .core
                .walk(ROOT_FID, fid, &components)
                .map_err(|err| anyhow!("failed to walk to {path}: {err}"))?;
            qids.last()
                .copied()
                .ok_or_else(|| anyhow!("walk to {path} returned no qids"))?
        };
        Ok((fid, qid))
    }

    /// Walk and open the supplied path, returning the fid and Qid.
    pub fn open_with_qid(&mut self, path: &str, mode: OpenMode) -> Result<(u32, Qid)> {
        let (fid, _) = self.walk_qid(path)?;
        let open_result = self
            .core
            .open(fid, mode)
            .map_err(|err| anyhow!("failed to open {path}: {err}"));
        if open_result.is_err() {
            let _ = self.core.clunk(fid);
        }
        open_result.map(|(qid, _)| (fid, qid))
    }

    /// Read bytes from an open fid.
    pub fn read(&mut self, fid: u32, offset: u64, count: u32) -> Result<Vec<u8>> {
        self.core.read(fid, offset, count).map_err(map_core_error)
    }

    /// Write bytes to an open fid.
    pub fn write(&mut self, fid: u32, offset: u64, data: &[u8]) -> Result<u32> {
        self.core.write(fid, offset, data).map_err(map_core_error)
    }

    /// Clunk the supplied fid.
    pub fn clunk(&mut self, fid: u32) -> Result<()> {
        self.core.clunk(fid).map_err(map_core_error)
    }

    /// Stream a file line-by-line, emitting a trailing `End` marker.
    pub fn tail(&mut self, path: &str) -> Result<TailStream<'_, T>> {
        let fid = self.open(path, OpenMode::read_only())?;
        Ok(TailStream {
            client: self,
            fid,
            offset: 0,
            buffer: Vec::new(),
            pending: VecDeque::new(),
            finished: false,
            closed: false,
        })
    }

    fn allocate_fid(&mut self) -> u32 {
        let fid = self.next_fid;
        self.next_fid = self.next_fid.wrapping_add(1);
        fid
    }

    /// Return the negotiated Secure9P maximum message size.
    #[must_use]
    pub fn negotiated_msize(&self) -> u32 {
        self.core.negotiated_msize()
    }

    fn parse_path(&self, path: &str) -> Result<Vec<String>> {
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
            if components.len() >= self.walk_depth {
                return Err(anyhow!(
                    "path exceeds maximum depth of {} components",
                    self.walk_depth
                ));
            }
            components.push(component.to_owned());
        }
        Ok(components)
    }
}

impl<'a, T: Secure9pTransport> TailStream<'a, T> {
    /// Fetch the next tail event, returning `None` after the end marker.
    pub fn next_event(&mut self) -> Result<Option<TailEvent>> {
        loop {
            if let Some(line) = self.pending.pop_front() {
                return Ok(Some(TailEvent::Line(line)));
            }
            if self.finished {
                if !self.closed {
                    let _ = self.client.clunk(self.fid);
                    self.closed = true;
                    return Ok(Some(TailEvent::End));
                }
                return Ok(None);
            }

            let chunk = self
                .client
                .read(self.fid, self.offset, self.client.negotiated_msize())?;
            if chunk.is_empty() {
                self.finished = true;
                if !self.buffer.is_empty() {
                    let line = decode_line(&self.buffer)?;
                    self.pending.push_back(line);
                    self.buffer.clear();
                }
                continue;
            }
            self.offset = self
                .offset
                .checked_add(chunk.len() as u64)
                .context("tail offset overflow")?;
            self.buffer.extend_from_slice(&chunk);
            self.extract_lines()?;
        }
    }

    fn extract_lines(&mut self) -> Result<()> {
        while let Some(pos) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line_bytes: Vec<u8> = self.buffer.drain(..pos).collect();
            let _ = self.buffer.drain(..1);
            let line = decode_line(&line_bytes)?;
            self.pending.push_back(line);
        }
        Ok(())
    }
}

impl<'a, T: Secure9pTransport> Iterator for TailStream<'a, T> {
    type Item = Result<TailEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_event() {
            Ok(Some(event)) => Some(Ok(event)),
            Ok(None) => None,
            Err(err) => Some(Err(err)),
        }
    }
}

impl<'a, T: Secure9pTransport> Drop for TailStream<'a, T> {
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.client.clunk(self.fid);
            self.closed = true;
        }
    }
}

/// Wrapper transport for in-process NineDoor connections.
pub struct InProcessTransport {
    connection: nine_door::InProcessConnection,
}

impl InProcessTransport {
    /// Wrap a NineDoor in-process connection for use with CohClient.
    pub fn new(connection: nine_door::InProcessConnection) -> Self {
        Self { connection }
    }
}

impl Secure9pTransport for InProcessTransport {
    type Error = nine_door::NineDoorError;

    fn exchange(&mut self, batch: &[u8]) -> Result<Vec<u8>, Self::Error> {
        self.connection.exchange_batch(batch)
    }
}

fn decode_line(bytes: &[u8]) -> Result<String> {
    let mut line = String::from_utf8(bytes.to_vec()).context("log is not valid UTF-8")?;
    if line.ends_with('\r') {
        line.pop();
    }
    Ok(line)
}

fn map_core_error<E: std::fmt::Display>(err: Secure9pError<E>) -> anyhow::Error {
    anyhow!("{err}")
}
