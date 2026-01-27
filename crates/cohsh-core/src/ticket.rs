// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Shared role parsing and ticket validation helpers.
// Author: Lukas Bower

//! Shared role parsing and ticket validation helpers.

use core::fmt;

use cohesix_proto::{role_label as proto_role_label, Role as ProtoRole};
use cohesix_ticket::{Role, TicketClaims, TicketError as ClaimsError, TicketToken};

use crate::command::MAX_TICKET_LEN;

/// Mode controlling how strictly role labels are parsed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoleParseMode {
    /// Only accept canonical labels from cohesix-proto.
    Strict,
    /// Accept the legacy `worker` alias in addition to canonical labels.
    AllowWorkerAlias,
}

/// Policy controlling ticket handling for queen sessions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueenTicketMode {
    /// Accept any non-empty ticket payload without parsing claims.
    Passthrough,
    /// Parse tickets (claims only) when provided, rejecting malformed payloads.
    Validate,
}

/// Normalisation rules applied to ticket payloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TicketPolicy {
    /// Maximum ticket length accepted by the console grammar.
    pub max_len: usize,
    /// Queen ticket handling behavior.
    pub queen_mode: QueenTicketMode,
}

impl TicketPolicy {
    /// Ticket policy for TCP console usage (validates queen tickets when present).
    pub const fn tcp() -> Self {
        Self {
            max_len: MAX_TICKET_LEN,
            queen_mode: QueenTicketMode::Validate,
        }
    }

    /// Ticket policy for NineDoor usage (passes through queen tickets).
    pub const fn ninedoor() -> Self {
        Self {
            max_len: MAX_TICKET_LEN,
            queen_mode: QueenTicketMode::Passthrough,
        }
    }
}

/// Output of ticket normalization.
#[derive(Debug)]
pub struct TicketCheck<'a> {
    /// Trimmed ticket payload, if present.
    pub ticket: Option<&'a str>,
    /// Parsed ticket claims, if decoded.
    pub claims: Option<TicketClaims>,
}

/// Errors raised when normalising a ticket payload.
#[derive(Debug, PartialEq, Eq)]
pub enum TicketError {
    /// Ticket payload missing when required.
    Missing,
    /// Ticket payload exceeds the maximum permitted length.
    TooLong(usize),
    /// Ticket payload failed claims decoding.
    Invalid(ClaimsError),
    /// Ticket role does not match the requested role.
    RoleMismatch {
        /// Role requested by the caller.
        expected: Role,
        /// Role encoded in the ticket claims.
        found: Role,
    },
    /// Ticket payload missing the required subject identity.
    MissingSubject,
}

impl fmt::Display for TicketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(f, "ticket payload is required"),
            Self::TooLong(max) => write!(f, "ticket payload exceeds {max} bytes"),
            Self::Invalid(err) => write!(f, "ticket is not a valid claims token: {err}"),
            Self::RoleMismatch { expected, found } => write!(
                f,
                "ticket role {:?} does not match requested role {:?}",
                found, expected
            ),
            Self::MissingSubject => write!(f, "ticket is missing required subject identity"),
        }
    }
}

/// Parse a role label into the corresponding ticket role.
#[must_use]
pub fn parse_role(input: &str, mode: RoleParseMode) -> Option<Role> {
    if input.eq_ignore_ascii_case(proto_role_label(ProtoRole::Queen)) {
        Some(Role::Queen)
    } else if matches!(mode, RoleParseMode::AllowWorkerAlias)
        && input.eq_ignore_ascii_case("worker")
    {
        Some(Role::WorkerHeartbeat)
    } else if input.eq_ignore_ascii_case(proto_role_label(ProtoRole::Worker)) {
        Some(Role::WorkerHeartbeat)
    } else if input.eq_ignore_ascii_case(proto_role_label(ProtoRole::GpuWorker)) {
        Some(Role::WorkerGpu)
    } else if input.eq_ignore_ascii_case(proto_role_label(ProtoRole::BusWorker)) {
        Some(Role::WorkerBus)
    } else if input.eq_ignore_ascii_case(proto_role_label(ProtoRole::LoraWorker)) {
        Some(Role::WorkerLora)
    } else {
        None
    }
}

/// Map a ticket role into the protocol role label.
#[must_use]
pub fn proto_role_from_ticket(role: Role) -> ProtoRole {
    match role {
        Role::Queen => ProtoRole::Queen,
        Role::WorkerHeartbeat => ProtoRole::Worker,
        Role::WorkerGpu => ProtoRole::GpuWorker,
        Role::WorkerBus => ProtoRole::BusWorker,
        Role::WorkerLora => ProtoRole::LoraWorker,
    }
}

/// Return the canonical label for a ticket role.
#[must_use]
pub fn role_label(role: Role) -> &'static str {
    proto_role_label(proto_role_from_ticket(role))
}

/// Normalise a ticket payload for the requested role.
pub fn normalize_ticket<'a>(
    role: Role,
    ticket: Option<&'a str>,
    policy: TicketPolicy,
) -> Result<TicketCheck<'a>, TicketError> {
    let trimmed = ticket.and_then(|value| {
        let candidate = value.trim();
        if candidate.is_empty() {
            None
        } else {
            Some(candidate)
        }
    });

    if let Some(value) = trimmed {
        if value.len() > policy.max_len {
            return Err(TicketError::TooLong(policy.max_len));
        }
    }

    match role {
        Role::Queen => match (trimmed, policy.queen_mode) {
            (Some(value), QueenTicketMode::Validate) => {
                let claims = TicketToken::decode_unverified(value).map_err(TicketError::Invalid)?;
                Ok(TicketCheck {
                    ticket: Some(value),
                    claims: Some(claims),
                })
            }
            (Some(value), QueenTicketMode::Passthrough) => Ok(TicketCheck {
                ticket: Some(value),
                claims: None,
            }),
            (None, _) => Ok(TicketCheck {
                ticket: None,
                claims: None,
            }),
        },
        Role::WorkerHeartbeat | Role::WorkerGpu | Role::WorkerBus | Role::WorkerLora => {
            let value = trimmed.ok_or(TicketError::Missing)?;
            let claims = TicketToken::decode_unverified(value).map_err(TicketError::Invalid)?;
            if claims.role != role {
                return Err(TicketError::RoleMismatch {
                    expected: role,
                    found: claims.role,
                });
            }
            if claims.subject.as_deref().is_none() {
                return Err(TicketError::MissingSubject);
            }
            Ok(TicketCheck {
                ticket: Some(value),
                claims: Some(claims),
            })
        }
    }
}
