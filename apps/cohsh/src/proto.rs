// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the proto module for cohsh.
// Author: Lukas Bower

//! Console acknowledgement parsing helpers shared by Cohsh transports.

pub use console_ack_wire::{AckStatus, ParsedAck as Ack};

/// Parse an acknowledgement line following the `OK VERB ...` grammar.
pub fn parse_ack(line: &str) -> Option<Ack<'_>> {
    console_ack_wire::parse_ack(line)
}
