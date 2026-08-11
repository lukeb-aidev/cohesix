// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the cohesix-proto library and public module surface.
// Author: Lukas Bower
#![no_std]

//! Shared protocol constants spanning console roles, ticket prefixes, and reason strings.

use core::{fmt, str::FromStr};

/// Roles recognised by the Cohesix console and transport layers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    /// Console owner role.
    Queen,
    /// Heartbeat worker role.
    Worker,
    /// GPU worker role.
    GpuWorker,
    /// Field bus worker role.
    BusWorker,
    /// AI LoRA model-adapter control worker role.
    LoraWorker,
}

/// Complete stable protocol role vocabulary.
///
/// This catalog describes labels only. In particular, the presence of
/// [`Role::BusWorker`] does not make WorkerBus an executable target role.
pub const ALL_ROLES: [Role; 5] = [
    Role::Queen,
    Role::Worker,
    Role::GpuWorker,
    Role::BusWorker,
    Role::LoraWorker,
];

/// Error returned when a protocol role label is not canonical.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseRoleError;

impl fmt::Display for ParseRoleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown Cohesix role label")
    }
}

/// Return the canonical label for the provided role.
pub const fn role_label(role: Role) -> &'static str {
    match role {
        Role::Queen => "queen",
        Role::Worker => "worker-heartbeat",
        Role::GpuWorker => "worker-gpu",
        Role::BusWorker => "worker-bus",
        Role::LoraWorker => "worker-lora",
    }
}

/// Parse one exact canonical protocol role label.
///
/// Aliases belong to presentation-layer command parsers; keeping the shared
/// protocol parser strict prevents recorded identities from drifting.
pub fn parse_role(label: &str) -> Result<Role, ParseRoleError> {
    match label {
        "queen" => Ok(Role::Queen),
        "worker-heartbeat" => Ok(Role::Worker),
        "worker-gpu" => Ok(Role::GpuWorker),
        "worker-bus" => Ok(Role::BusWorker),
        "worker-lora" => Ok(Role::LoraWorker),
        _ => Err(ParseRoleError),
    }
}

impl FromStr for Role {
    type Err = ParseRoleError;

    fn from_str(label: &str) -> Result<Self, Self::Err> {
        parse_role(label)
    }
}

/// Prefix used when generating capability tickets.
pub const TICKET_PREFIX: &str = "cohesix-ticket-";

/// Reason emitted when an authentication token is missing.
pub const REASON_EXPECTED_TOKEN: &str = "expected-token";
/// Reason emitted when an authentication token is malformed.
pub const REASON_INVALID_LENGTH: &str = "invalid-length";
/// Reason emitted when an authentication token does not match the configured secret.
pub const REASON_INVALID_TOKEN: &str = "invalid-token";
/// Reason emitted when an authentication exchange times out.
pub const REASON_TIMEOUT: &str = "timeout";
/// Reason emitted when a console session is terminated due to inactivity.
pub const REASON_INACTIVITY_TIMEOUT: &str = "inactivity-timeout";
/// Reason emitted when a receive error terminates the console session.
pub const REASON_RECV_ERROR: &str = "recv-error";

#[cfg(test)]
mod tests {
    use super::{parse_role, role_label, Role, ALL_ROLES};

    #[test]
    fn canonical_role_catalog_round_trips() {
        for role in ALL_ROLES {
            assert_eq!(parse_role(role_label(role)), Ok(role));
        }
    }

    #[test]
    fn parser_rejects_presentation_aliases_and_unknown_roles() {
        for label in ["worker", "heartbeat", "gpu", "lora", "WorkerGpu", ""] {
            assert!(
                parse_role(label).is_err(),
                "accepted noncanonical {label:?}"
            );
        }
    }

    #[test]
    fn legacy_worker_variant_keeps_heartbeat_label() {
        assert_eq!(role_label(Role::Worker), "worker-heartbeat");
    }
}
