// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Worker heartbeat agent scaffold outlined in `docs/ARCHITECTURE.md` §2-§3.
//!
//! Heartbeat workers emit telemetry through NineDoor namespaces under the
//! supervision of root-task budget enforcement. This skeleton exposes a simple
//! API so integration points can compile while the real seL4 primitives are
//! brought online.

use anyhow::Result;
use cohesix_ticket::{BudgetSpec, Role, TicketTemplate};
use secure9p_wire::SessionId;

/// Builder for configuring heartbeat workers before seL4 integration lands.
#[derive(Debug, Clone)]
pub struct HeartbeatWorker {
    ticket: TicketTemplate,
    session: SessionId,
}

impl HeartbeatWorker {
    /// Create a heartbeat worker descriptor bound to the worker role and supplied session.
    #[must_use]
    pub fn new(session: SessionId) -> Self {
        Self {
            ticket: TicketTemplate::new(Role::WorkerHeartbeat, BudgetSpec::default_heartbeat()),
            session,
        }
    }

    /// Return the associated capability ticket.
    #[must_use]
    pub fn ticket(&self) -> &TicketTemplate {
        &self.ticket
    }

    /// Retrieve the session identifier bound to the worker.
    #[must_use]
    pub fn session(&self) -> SessionId {
        self.session
    }

    /// Emit a synthetic telemetry payload.
    pub fn emit(&self, tick: u64) -> Result<String> {
        Ok(format!("heartbeat {tick}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_payload_includes_tick() {
        let worker = HeartbeatWorker::new(SessionId::from_raw(7));
        let payload = worker.emit(3).unwrap();
        assert_eq!(payload, "heartbeat 3");
        assert_eq!(worker.session(), SessionId::from_raw(7));
    }
}
