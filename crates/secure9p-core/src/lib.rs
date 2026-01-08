// Author: Lukas Bower
// Purpose: Define Secure9P core session and access policy primitives.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![no_std]

//! Secure9P core helpers used by Cohesix protocol implementations.

extern crate alloc;

use alloc::string::String;

use cohesix_ticket::TicketClaims;
use secure9p_codec::OpenMode;
use thiserror::Error;

/// Errors surfaced by Secure9P access policy checks.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AccessError {
    /// The requested action is not permitted.
    #[error("permission denied: {0}")]
    Permission(String),
}

/// Access policy hook for Secure9P servers.
pub trait AccessPolicy {
    /// Validate a requested attach using the provided ticket claims.
    fn can_attach(&self, ticket: &TicketClaims) -> Result<(), AccessError>;
    /// Validate a requested open call.
    fn can_open(&self, ticket: &TicketClaims, path: &str, mode: OpenMode)
        -> Result<(), AccessError>;
    /// Validate a requested create call.
    fn can_create(&self, ticket: &TicketClaims, path: &str) -> Result<(), AccessError>;
}
