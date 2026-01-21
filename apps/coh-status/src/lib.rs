// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Cohesix status tool crate surface.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Cohesix status tool crate surface.

use anyhow::{Context, Result};
use cohsh::client::CohClient;
use cohsh::policy::CohshPolicy;
use cohsh::SECURE9P_MSIZE;
use cohsh_core::command::MAX_LINE_LEN;
use cohsh_core::trace::{TraceLog, TraceReplayTransport, TracePolicy};
use cohesix_ticket::Role;

/// Trace replay client wrapper for coh-status field diagnostics.
pub struct TraceReplay {
    client: CohClient<TraceReplayTransport>,
}

impl TraceReplay {
    /// Construct a trace replay client from a trace payload.
    pub fn from_bytes(payload: &[u8], role: Role, ticket: Option<&str>) -> Result<Self> {
        let policy = trace_policy();
        let trace = TraceLog::decode(payload, policy).context("trace decode failed")?;
        let transport = TraceReplayTransport::new(trace.frames);
        let client = CohClient::connect(transport, role, ticket)?;
        Ok(Self { client })
    }

    /// Borrow the underlying Secure9P client for replay reads.
    pub fn client(&mut self) -> &mut CohClient<TraceReplayTransport> {
        &mut self.client
    }
}

/// Return the manifest-derived trace policy defaults.
#[must_use]
pub fn trace_policy() -> TracePolicy {
    let policy = CohshPolicy::from_generated();
    TracePolicy::new(
        policy.trace.max_bytes,
        SECURE9P_MSIZE,
        MAX_LINE_LEN as u32,
    )
}
