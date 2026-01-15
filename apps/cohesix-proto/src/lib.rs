// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the cohesix-proto library and public module surface.
// Author: Lukas Bower
#![no_std]

//! Shared protocol constants spanning console roles, ticket prefixes, and reason strings.

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
    /// LoRa worker role.
    LoraWorker,
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
