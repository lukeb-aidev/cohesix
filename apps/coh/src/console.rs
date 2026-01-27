// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide console-backed CohAccess helpers for coh.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{anyhow, Context, Result};
use cohesix_ticket::Role;
use cohsh::{CohshRetryPolicy, Session, TcpTransport, Transport};

use crate::policy::CohRetryPolicy;
use crate::CohAccess;

/// Coh access wrapper backed by the TCP console transport.
pub struct ConsoleSession {
    transport: TcpTransport,
    session: Session,
}

impl ConsoleSession {
    /// Connect to the TCP console and attach to the supplied role.
    pub fn connect(
        host: &str,
        port: u16,
        auth_token: &str,
        role: Role,
        ticket: Option<&str>,
        retry: CohRetryPolicy,
    ) -> Result<Self> {
        let retry_policy = CohshRetryPolicy {
            max_attempts: retry.max_attempts,
            backoff_ms: retry.backoff_ms,
            ceiling_ms: retry.ceiling_ms,
            timeout_ms: retry.timeout_ms,
        };
        let mut transport = TcpTransport::new(host, port)
            .with_retry_policy(retry_policy)
            .with_auth_token(auth_token);
        let session = transport
            .attach(role, ticket)
            .context("attach to TCP console")?;
        Ok(Self { transport, session })
    }

    fn join_lines(lines: &[String]) -> Vec<u8> {
        if lines.is_empty() {
            return Vec::new();
        }
        let mut out = String::new();
        for (idx, line) in lines.iter().enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            out.push_str(line);
        }
        out.into_bytes()
    }

    fn drain_acks(&mut self) {
        let _ = self.transport.drain_acknowledgements();
    }
}

impl CohAccess for ConsoleSession {
    fn list_dir(&mut self, path: &str, max_bytes: usize) -> Result<Vec<String>> {
        let entries = self.transport.list(&self.session, path);
        self.drain_acks();
        let entries = entries?;
        let bytes = entries.iter().map(|entry| entry.len()).sum::<usize>();
        if bytes > max_bytes {
            return Err(anyhow!("read {path} exceeds max bytes {max_bytes}"));
        }
        Ok(entries)
    }

    fn read_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        let lines = self.transport.read(&self.session, path);
        self.drain_acks();
        let lines = lines?;
        let payload = Self::join_lines(&lines);
        if payload.len() > max_bytes {
            return Err(anyhow!("read {path} exceeds max bytes {max_bytes}"));
        }
        Ok(payload)
    }

    fn write_append(&mut self, path: &str, payload: &[u8]) -> Result<usize> {
        let result = self.transport.write(&self.session, path, payload);
        self.drain_acks();
        result?;
        Ok(payload.len())
    }
}
