// Author: Lukas Bower
// Purpose: Static manifest hooks for ticket inventory and namespace mounts.

#![cfg(feature = "kernel")]

use cohesix_ticket::Role;

/// Ticket entry describing a role/token pair available during bootstrap.
#[derive(Debug, Clone, Copy)]
pub struct TicketSpec {
    /// Role granted by the ticket.
    pub role: Role,
    /// Shared secret used to validate ticket claims.
    pub secret: &'static str,
}

/// Service mount entry describing a canonical target to bind.
#[derive(Debug, Clone, Copy)]
pub struct NamespaceMount {
    /// Service identifier registered with the namespace provider.
    pub service: &'static str,
    /// Canonical path segments describing the service root.
    pub target: &'static [&'static str],
}

/// Return the static ticket inventory used during bootstrap authentication.
#[must_use]
pub const fn ticket_inventory() -> &'static [TicketSpec] {
    &TICKET_INVENTORY
}

/// Return the static namespace mount table used for service bindings.
#[must_use]
pub const fn namespace_mounts() -> &'static [NamespaceMount] {
    &NAMESPACE_MOUNTS
}

const TICKET_INVENTORY: [TicketSpec; 3] = [
    TicketSpec {
        role: Role::Queen,
        secret: "bootstrap",
    },
    TicketSpec {
        role: Role::WorkerHeartbeat,
        secret: "worker",
    },
    TicketSpec {
        role: Role::WorkerGpu,
        secret: "worker-gpu",
    },
];

const NAMESPACE_MOUNTS: [NamespaceMount; 0] = [];
