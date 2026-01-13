// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the console-ack-wire library and public module surface.
// Author: Lukas Bower
#![no_std]

//! Console acknowledgement wire representations shared across Cohesix clients and servers.

use core::fmt::Write;

/// ACK/ERR status emitted by console commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckStatus {
    /// Successful command execution.
    Ok,
    /// Command failed to execute.
    Err,
}

/// Structured representation of an acknowledgement line.
pub struct AckLine<'a> {
    /// Outcome of the command.
    pub status: AckStatus,
    /// Command verb associated with the acknowledgement.
    pub verb: &'a str,
    /// Optional detail payload appended to the line.
    pub detail: Option<&'a str>,
}

/// Parsed acknowledgement line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedAck<'a> {
    /// Outcome of the command.
    pub status: AckStatus,
    /// Verb associated with the acknowledgement.
    pub verb: &'a str,
    /// Optional trailing detail payload.
    pub detail: Option<&'a str>,
}

/// Render an acknowledgement line into the provided buffer using the standard grammar.
pub fn render_ack<W: Write>(w: &mut W, ack: &AckLine<'_>) -> core::fmt::Result {
    let label = match ack.status {
        AckStatus::Ok => "OK",
        AckStatus::Err => "ERR",
    };

    w.write_str(label)?;
    w.write_char(' ')?;
    w.write_str(ack.verb)?;

    if let Some(detail) = ack.detail {
        if !detail.is_empty() {
            w.write_char(' ')?;
            w.write_str(detail)?;
        }
    }

    Ok(())
}

/// Parse an acknowledgement line following the `OK VERB ...` grammar.
pub fn parse_ack(line: &str) -> Option<ParsedAck<'_>> {
    let trimmed = line.trim();
    let mut parts = trimmed.splitn(3, ' ');
    let status = match parts.next()? {
        "OK" => AckStatus::Ok,
        "ERR" => AckStatus::Err,
        _ => return None,
    };
    let verb = parts.next()?;
    let detail = parts
        .next()
        .map(str::trim)
        .filter(|detail| !detail.is_empty());
    Some(ParsedAck {
        status,
        verb,
        detail,
    })
}
