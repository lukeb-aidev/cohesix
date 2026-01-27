// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define Secure9P core session and access policy primitives.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![no_std]

//! Secure9P core helpers used by Cohesix protocol implementations.

extern crate alloc;

use alloc::string::String;

use cohesix_ticket::TicketClaims;
use secure9p_codec::OpenMode;
use thiserror::Error;

mod session;

pub use session::{
    FidError, FidTable, QueueDepth, QueueError, SessionLimits, ShardedFidTable, ShortWritePolicy,
    TagError, TagWindow, DEFAULT_FID_SHARDS, DEFAULT_SHORT_WRITE_BACKOFF_MS,
    DEFAULT_SHORT_WRITE_RETRIES,
};

/// Errors surfaced by Secure9P access policy checks.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AccessError {
    /// The requested action is not permitted.
    #[error("permission denied: {0}")]
    Permission(String),
}

/// Offset errors raised while enforcing append-only semantics.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AppendOnlyOffsetError {
    /// The requested offset is behind the retained append-only window.
    #[error("append-only offset {requested} is stale; oldest available {available_start}")]
    Stale {
        /// Offset supplied by the caller.
        requested: u64,
        /// Oldest retained offset.
        available_start: u64,
    },
    /// The requested offset does not match the expected append position.
    #[error("append-only offset {provided} does not match expected {expected}")]
    Invalid {
        /// Offset supplied by the caller.
        provided: u64,
        /// Expected append-only offset.
        expected: u64,
    },
}

/// Bounds describing an append-only read request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendOnlyReadBounds {
    /// Offset to read from.
    pub offset: u64,
    /// Number of bytes to return.
    pub len: usize,
    /// Indicates that the read was shorter than requested.
    pub short: bool,
}

/// Bounds describing an append-only write request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendOnlyWriteBounds {
    /// Number of bytes that can be written.
    pub len: usize,
    /// Indicates that the write would be short.
    pub short: bool,
}

/// Enforce append-only offset semantics for reads and compute short-read status.
pub fn append_only_read_bounds(
    offset: u64,
    available_start: u64,
    available_end: u64,
    count: u32,
) -> Result<AppendOnlyReadBounds, AppendOnlyOffsetError> {
    if offset < available_start {
        return Err(AppendOnlyOffsetError::Stale {
            requested: offset,
            available_start,
        });
    }
    let available = available_end.saturating_sub(offset) as usize;
    let requested = count as usize;
    let len = requested.min(available);
    Ok(AppendOnlyReadBounds {
        offset,
        len,
        short: len < requested,
    })
}

/// Enforce append-only offset semantics for writes and compute short-write status.
pub fn append_only_write_bounds(
    expected_offset: u64,
    provided_offset: u64,
    max_len: usize,
    requested_len: usize,
) -> Result<AppendOnlyWriteBounds, AppendOnlyOffsetError> {
    if provided_offset != expected_offset {
        return Err(AppendOnlyOffsetError::Invalid {
            provided: provided_offset,
            expected: expected_offset,
        });
    }
    let len = requested_len.min(max_len);
    Ok(AppendOnlyWriteBounds {
        len,
        short: len < requested_len,
    })
}

/// Access policy hook for Secure9P servers.
pub trait AccessPolicy {
    /// Validate a requested attach using the provided ticket claims.
    fn can_attach(&self, ticket: &TicketClaims) -> Result<(), AccessError>;
    /// Validate a requested open call.
    fn can_open(
        &self,
        ticket: &TicketClaims,
        path: &str,
        mode: OpenMode,
    ) -> Result<(), AccessError>;
    /// Validate a requested create call.
    fn can_create(&self, ticket: &TicketClaims, path: &str) -> Result<(), AccessError>;
}
