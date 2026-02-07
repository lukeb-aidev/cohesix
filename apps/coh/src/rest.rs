// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide REST-backed CohAccess helpers for coh.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{anyhow, Context, Result};
use cohesix_rest::GatewayClient;

use crate::CohAccess;

/// Coh access wrapper backed by the hive-gateway REST API.
pub struct RestSession {
    client: GatewayClient,
}

impl RestSession {
    /// Connect to the REST gateway.
    pub fn connect(base_url: impl Into<String>) -> Self {
        Self {
            client: GatewayClient::new(base_url),
        }
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

    fn normalise_payload(payload: &[u8]) -> Result<String> {
        let payload_str = std::str::from_utf8(payload).context("payload must be UTF-8")?;
        let trimmed = payload_str.strip_suffix('\n').unwrap_or(payload_str);
        if trimmed.contains('\n') || trimmed.contains('\r') {
            return Err(anyhow!("payload must be a single line"));
        }
        Ok(trimmed.to_owned())
    }
}

impl CohAccess for RestSession {
    fn list_dir(&mut self, path: &str, max_bytes: usize) -> Result<Vec<String>> {
        let entries = self.client.list(path)?;
        let bytes = entries.iter().map(|entry| entry.len()).sum::<usize>();
        if bytes > max_bytes {
            return Err(anyhow!("read {path} exceeds max bytes {max_bytes}"));
        }
        Ok(entries)
    }

    fn read_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        let max_bytes_u32 = u32::try_from(max_bytes)
            .map_err(|_| anyhow!("read {path} exceeds max bytes {max_bytes}"))?;
        let lines = self.client.read(path, max_bytes_u32)?;
        let payload = Self::join_lines(&lines);
        if payload.len() > max_bytes {
            return Err(anyhow!("read {path} exceeds max bytes {max_bytes}"));
        }
        Ok(payload)
    }

    fn write_append(&mut self, path: &str, payload: &[u8]) -> Result<usize> {
        let trimmed = Self::normalise_payload(payload)?;
        self.client.echo(path, trimmed.as_str())?;
        Ok(payload.len())
    }
}
