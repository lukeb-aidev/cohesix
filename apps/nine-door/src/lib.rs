// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! NineDoor Secure9P server skeleton as defined in `docs/ARCHITECTURE.md` §2-§3.
//!
//! The final implementation will host the Secure9P codec, role-aware access
//! control, and namespace providers. The current milestone establishes the crate
//! boundary and shared abstractions consumed by the workspace crates.

use cohesix_ticket::{BudgetSpec, Role, TicketTemplate};
use secure9p_wire::{FrameHeader, SessionId};
use thiserror::Error;

/// Errors surfaced by the NineDoor skeleton while the full 9P pipeline is built out.
#[derive(Debug, Error)]
pub enum NineDoorError {
    /// Indicates that no session state exists for the supplied identifier.
    #[error("unknown session {0:?}")]
    UnknownSession(SessionId),
}

/// Placeholder NineDoor 9P server state.
#[derive(Debug)]
pub struct NineDoor {
    /// Tracks a sample bootstrap ticket so downstream crates can depend on the API.
    bootstrap_ticket: TicketTemplate,
}

impl NineDoor {
    /// Construct a new NineDoor skeleton populated with a queen bootstrap ticket.
    #[must_use]
    pub fn new() -> Self {
        Self {
            bootstrap_ticket: TicketTemplate::new(Role::Queen, BudgetSpec::unbounded()),
        }
    }

    /// Retrieve the negotiated frame header for the bootstrap session.
    pub fn describe_bootstrap_session(&self) -> Result<FrameHeader, NineDoorError> {
        let header = FrameHeader::new(SessionId::BOOTSTRAP, 0);
        Ok(header)
    }

    /// Borrow the bootstrap ticket template used for queen sessions.
    #[must_use]
    pub fn bootstrap_ticket(&self) -> &TicketTemplate {
        &self.bootstrap_ticket
    }
}

impl Default for NineDoor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_session_is_available() {
        let nine_door = NineDoor::new();
        let header = nine_door.describe_bootstrap_session().unwrap();
        assert_eq!(header.session(), SessionId::BOOTSTRAP);
    }
}
