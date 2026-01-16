// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Shared Cohesix console grammar and transport primitives.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![no_std]

//! Shared Cohesix console grammar and transport primitives used by the root-task
//! console, cohsh CLI, and host tooling.

extern crate alloc;

pub mod command;
pub mod docs;
pub mod help;
pub mod ticket;
pub mod verb;
pub mod wire;

#[cfg(feature = "tcp")]
pub mod tcp;

pub use command::{
    Command, CommandParser, ConsoleError, RateLimiter, MAX_ECHO_LEN, MAX_ID_LEN, MAX_JSON_LEN,
    MAX_LINE_LEN, MAX_PATH_LEN, MAX_ROLE_LEN, MAX_TICKET_LEN,
};
pub use ticket::{
    normalize_ticket, parse_role, proto_role_from_ticket, role_label, QueenTicketMode,
    RoleParseMode, TicketCheck, TicketError, TicketPolicy,
};
pub use verb::{ConsoleVerb, VerbSpec, ALL_VERBS, VERB_SPECS, VERB_SPEC_COUNT};
pub use wire::{
    parse_ack, parse_console_line, AckLine, AckStatus, ConsoleLine, ParsedAck, END_LINE,
};
