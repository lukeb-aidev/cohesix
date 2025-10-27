// Author: Lukas Bower

#![cfg(feature = "kernel")]
#![allow(dead_code)]

use crate::console::Command;
use crate::event::AuditSink;
use core::fmt::Write;
use heapless::String as HeaplessString;

/// Function pointer used to forward console commands to NineDoor.
pub type NineDoorHandler = fn(&Command, &mut dyn AuditSink);

/// Retrieve the NineDoor bridge handler used during bring-up.
#[must_use]
pub fn bridge_handler() -> NineDoorHandler {
    handle_command
}

/// Minimal NineDoor bridge used by the seL4 build until the full Secure9P server is ported.
pub fn handle_command(command: &Command, audit: &mut dyn AuditSink) {
    match command {
        Command::Attach { role, ticket } => {
            let ticket_repr = ticket
                .as_ref()
                .map(|value| value.as_str())
                .unwrap_or("<none>");
            let mut message = HeaplessString::<128>::new();
            let _ = write!(
                message,
                "nine-door: attach role={} ticket={ticket_repr}",
                role.as_str()
            );
            audit.info(message.as_str());
        }
        Command::Tail { path } => {
            let mut message = HeaplessString::<128>::new();
            let _ = write!(message, "nine-door: tail {}", path.as_str());
            audit.info(message.as_str());
        }
        Command::Log => {
            audit.info("nine-door: log stream requested");
        }
        Command::Spawn(payload) => {
            let mut message = HeaplessString::<128>::new();
            let _ = write!(
                message,
                "nine-door: spawn payload={}...",
                truncate(payload.as_str(), 64)
            );
            audit.info(message.as_str());
        }
        Command::Kill(identifier) => {
            let mut message = HeaplessString::<128>::new();
            let _ = write!(message, "nine-door: kill {}", identifier.as_str());
            audit.info(message.as_str());
        }
        Command::Help | Command::Quit => {}
    }
}

fn truncate(input: &str, limit: usize) -> &str {
    if input.len() <= limit {
        input
    } else {
        &input[..limit]
    }
}
