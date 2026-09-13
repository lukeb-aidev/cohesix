// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Cohesix status tool crate surface.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Cohesix status tool crate surface.

/// Canonical signed-device verifier, also available to SwarmUI consumers.
pub use cohesix_attestation as attestation;

use anyhow::{Context, Result};
use cohesix_ticket::Role;
use cohesix_worker_evidence::{parse_evidence, ValidatedEvidence};
use cohsh::client::CohClient;
use cohsh::policy::CohshPolicy;
use cohsh::SECURE9P_MSIZE;
use cohsh_core::command::MAX_LINE_LEN;
use cohsh_core::trace::{TraceLog, TracePolicy, TraceReplayTransport};

/// Trace replay client wrapper for coh-status field diagnostics.
pub struct TraceReplay {
    client: CohClient<TraceReplayTransport>,
}

impl TraceReplay {
    /// Construct a trace replay client from a trace payload.
    pub fn from_bytes(payload: &[u8], role: Role, ticket: Option<&str>) -> Result<Self> {
        let policy = trace_policy();
        let trace = TraceLog::decode(payload, policy).context("trace decode failed")?;
        anyhow::ensure!(
            trace.capture.is_none(),
            "console captures require captured_namespace"
        );
        let transport = TraceReplayTransport::new(trace.frames);
        let client = CohClient::connect(transport, role, ticket)?;
        Ok(Self { client })
    }

    /// Borrow the underlying Secure9P client for replay reads.
    pub fn client(&mut self) -> &mut CohClient<TraceReplayTransport> {
        &mut self.client
    }
}

/// Present retained console observations through the shared read-only replay core.
/// Source metadata and completeness remain available in the decoded canonical log.
pub fn captured_namespace(payload: &[u8]) -> Result<cohsh::trace_capture::CapturedNamespace> {
    let policy = trace_policy();
    let trace = TraceLog::decode(payload, policy)?;
    cohsh::trace_capture::CapturedNamespace::new(
        &trace,
        &cohsh::trace_capture::policy_digest(
            policy,
            CohshPolicy::from_generated().trace.max_duration_ms,
        ),
    )
}

/// Return the manifest-derived trace policy defaults.
#[must_use]
pub fn trace_policy() -> TracePolicy {
    let policy = CohshPolicy::from_generated();
    TracePolicy::new(policy.trace.max_bytes, SECURE9P_MSIZE, MAX_LINE_LEN as u32)
}

/// Parse and validate one bounded Worker evidence record without deriving a
/// weaker proof class from malformed or incomplete input.
///
/// `coh-status` is intentionally a read-only replay/library surface in
/// Milestone 26e. Callers receive the shared validator's exact typed record;
/// this function never turns namespace reachability or a generic `ready`
/// string into Worker execution proof.
pub fn validate_worker_evidence(payload: &[u8]) -> Result<ValidatedEvidence> {
    parse_evidence(payload).context("Worker evidence validation failed")
}

#[cfg(test)]
mod tests {
    use super::validate_worker_evidence;

    #[test]
    fn malformed_or_untyped_worker_evidence_is_not_promoted() {
        for payload in [
            br#"{}"#.as_slice(),
            br#"{"state":"ready"}"#.as_slice(),
            br#"{"schema":"cohesix-worker-task-evidence/v1","record_kind":"target-component","verdict":"pass"}"#.as_slice(),
        ] {
            assert!(validate_worker_evidence(payload).is_err());
        }
    }
}
