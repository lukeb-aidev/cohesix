// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Parse replay control commands and maintain replay status snapshots.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};
use secure9p_codec::ErrorCode;
use secure9p_core::append_only_write_bounds;

use crate::NineDoorError;
use super::audit::{ReplayConfig, ReplaySummary};

/// Replay control command parsed from `/replay/ctl`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayCommand {
    /// Starting cursor for the replay window.
    pub from: u64,
}

/// State backing `/replay/ctl` and `/replay/status`.
#[derive(Debug)]
pub(crate) struct ReplayState {
    config: ReplayConfig,
    ctl_log: Vec<u8>,
    status: Vec<u8>,
}

impl ReplayState {
    pub fn new(config: ReplayConfig) -> Self {
        let status = if config.enabled_flag() {
            b"{\"state\":\"idle\"}\n".to_vec()
        } else {
            Vec::new()
        };
        Self {
            config,
            ctl_log: Vec::new(),
            status,
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled_flag()
    }

    pub fn ctl_log(&self) -> &[u8] {
        &self.ctl_log
    }

    pub fn status(&self) -> &[u8] {
        &self.status
    }

    pub fn append_ctl(
        &mut self,
        offset: u64,
        data: &[u8],
    ) -> Result<ReplayCommand, NineDoorError> {
        let text = std::str::from_utf8(data).map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("replay control must be utf-8: {err}"),
            )
        })?;
        let mut parsed = None;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if parsed.is_some() {
                return Err(NineDoorError::protocol(
                    ErrorCode::Invalid,
                    "replay control accepts one command per write",
                ));
            }
            parsed = Some(serde_json::from_str::<ReplayCommand>(trimmed).map_err(|err| {
                NineDoorError::protocol(
                    ErrorCode::Invalid,
                    format!("invalid replay command: {err}"),
                )
            })?);
        }
        let command = parsed.ok_or_else(|| {
            NineDoorError::protocol(ErrorCode::Invalid, "replay control missing command")
        })?;
        let payload = ensure_line_terminated(data);
        let bounds = apply_append(
            self.ctl_log.len(),
            offset,
            self.config.ctl_max_bytes(),
            payload.len(),
            "replay control",
        )?;
        if bounds.short {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                format!(
                    "replay control exceeds max bytes {}",
                    self.config.ctl_max_bytes()
                ),
            ));
        }
        self.ctl_log.extend_from_slice(&payload[..bounds.len]);
        Ok(command)
    }

    pub fn set_status_ok(&mut self, summary: &ReplaySummary) -> Result<(), NineDoorError> {
        let sequence_hash = format!("{:016x}", fnv1a64(summary.sequence.as_bytes()));
        let payload = format!(
            "{{\"state\":\"ok\",\"from\":{},\"to\":{},\"entries\":{},\"match\":true,\"sequence_fnv1a\":\"{}\"}}\n",
            summary.from,
            summary.to,
            summary.entries,
            sequence_hash
        );
        if payload.len() > self.config.status_max_bytes() {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                format!(
                    "replay status exceeds max bytes {}",
                    self.config.status_max_bytes()
                ),
            ));
        }
        self.status = payload.into_bytes();
        Ok(())
    }

    pub fn set_status_err(&mut self, message: &str) -> Result<(), NineDoorError> {
        #[derive(Serialize)]
        struct ReplayStatusError<'a> {
            state: &'static str,
            error: &'a str,
        }

        let payload = serde_json::to_string(&ReplayStatusError {
            state: "err",
            error: message,
        })
        .map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("replay status encode failed: {err}"),
            )
        })?;
        let payload = format!("{payload}\n");
        if payload.len() > self.config.status_max_bytes() {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                format!(
                    "replay status exceeds max bytes {}",
                    self.config.status_max_bytes()
                ),
            ));
        }
        self.status = payload.into_bytes();
        Ok(())
    }
}

struct AppendBounds {
    len: usize,
    short: bool,
}

fn apply_append(
    current_len: usize,
    offset: u64,
    max_len: usize,
    requested: usize,
    label: &str,
) -> Result<AppendBounds, NineDoorError> {
    let expected_offset = current_len as u64;
    let provided_offset = if offset == u64::MAX {
        expected_offset
    } else {
        offset
    };
    let bounds = append_only_write_bounds(expected_offset, provided_offset, max_len, requested)
        .map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("{label} append offset rejected: {err}"),
            )
        })?;
    Ok(AppendBounds {
        len: bounds.len,
        short: bounds.short,
    })
}

fn ensure_line_terminated(data: &[u8]) -> Vec<u8> {
    if data.ends_with(b"\n") {
        return data.to_vec();
    }
    let mut out = data.to_vec();
    out.push(b'\n');
    out
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut hash = OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}
