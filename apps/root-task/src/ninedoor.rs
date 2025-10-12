#![cfg(target_os = "none")]
#![allow(dead_code)]

use crate::console::Command;
use crate::event::AuditSink;
use core::fmt::Write;
use heapless::String;

/// Trait used by the event pump to forward console commands to NineDoor.
pub trait NineDoorHandler {
    /// Handle a console command after it has passed validation.
    fn handle(&mut self, command: &Command, audit: &mut dyn AuditSink);
}

/// Minimal NineDoor bridge used by the seL4 build until the full Secure9P server is ported.
#[derive(Debug, Default)]
pub struct NineDoorBridge;

impl NineDoorBridge {
    /// Create a new bridge instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl NineDoorHandler for NineDoorBridge {
    fn handle(&mut self, command: &Command, audit: &mut dyn AuditSink) {
        match command {
            Command::Attach { role, ticket } => {
                let ticket_repr = ticket
                    .as_ref()
                    .map(HeaplessStringExt::as_str)
                    .unwrap_or("<none>");
                let mut message = String::<128>::new();
                let _ = write!(
                    message,
                    "nine-door: attach role={} ticket={ticket_repr}",
                    role.as_str()
                );
                audit.info(message.as_str());
            }
            Command::Tail { path } => {
                let mut message = String::<128>::new();
                let _ = write!(message, "nine-door: tail {}", path.as_str());
                audit.info(message.as_str());
            }
            Command::Log => {
                audit.info("nine-door: log stream requested");
            }
            Command::Spawn(payload) => {
                let mut message = String::<128>::new();
                let _ = write!(
                    message,
                    "nine-door: spawn payload={}...",
                    truncate(payload.as_str(), 64)
                );
                audit.info(message.as_str());
            }
            Command::Kill(identifier) => {
                let mut message = String::<128>::new();
                let _ = write!(message, "nine-door: kill {}", identifier.as_str());
                audit.info(message.as_str());
            }
            Command::Help | Command::Quit => {}
        }
    }
}

trait HeaplessStringExt {
    fn as_str(&self) -> &str;
}

impl<const N: usize> HeaplessStringExt for heapless::String<N> {
    fn as_str(&self) -> &str {
        heapless::String::as_str(self)
    }
}

fn truncate(input: &str, limit: usize) -> &str {
    if input.len() <= limit {
        input
    } else {
        &input[..limit]
    }
}
