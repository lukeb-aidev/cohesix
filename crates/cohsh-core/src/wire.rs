// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Shared ACK/ERR/END console wire helpers.
// Author: Lukas Bower

//! Shared ACK/ERR/END console wire helpers.

use core::fmt::Write;

pub use console_ack_wire::{AckLine, AckStatus, ParsedAck};

/// Terminal marker emitted at the end of a streaming response.
pub const END_LINE: &str = "END";

/// Existing C1 frames keep the fixed console response-line bound.
pub const CAT_CHUNK_MAX_WIRE_BYTES: usize = 256;
/// A logical record must fit the existing bounded response frame inventory.
pub const CAT_CHUNK_MAX_COUNT: usize = 64;
/// Schema 1.21 permits a complete AuditFS record up to the Secure9P byte ceiling.
/// This does not enlarge commands, ECHO payloads or the response frame inventory.
pub const CAT_CHUNK_REASSEMBLED_MAX_BYTES: usize = 8192;

/// Parsed console line classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleLine<'a> {
    /// Parsed acknowledgement line.
    Ack(ParsedAck<'a>),
    /// End-of-stream marker.
    End,
    /// Plain text payload.
    Text(&'a str),
}

/// Render an acknowledgement line into the provided writer.
pub fn render_ack<W: Write>(w: &mut W, ack: &AckLine<'_>) -> core::fmt::Result {
    console_ack_wire::render_ack(w, ack)
}

/// Parse an acknowledgement line following the `OK VERB ...` grammar.
#[must_use]
pub fn parse_ack(line: &str) -> Option<ParsedAck<'_>> {
    console_ack_wire::parse_ack(line)
}

/// Parse a console line into an acknowledgement, stream end, or payload line.
#[must_use]
pub fn parse_console_line(line: &str) -> ConsoleLine<'_> {
    let trimmed = line.trim();
    if trimmed == END_LINE {
        return ConsoleLine::End;
    }
    if let Some(ack) = parse_ack(trimmed) {
        return ConsoleLine::Ack(ack);
    }
    ConsoleLine::Text(trimmed)
}
