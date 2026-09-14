// Author: Lukas Bower
// Purpose: Reject unsupported authority claims and insecure production policy at compilation.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use crate::ir::Manifest;
use anyhow::{bail, Result};

pub fn validate(manifest: &Manifest) -> Result<()> {
    let policy = manifest.authority;
    if !policy.delegated_rest || policy.vm_verified_delegation {
        bail!("REST mutations require gateway-enforced delegation; VM-verified delegation is unavailable");
    }
    if !(1..=4096).contains(&policy.delegated_ticket_entries)
        || !(1..=86400).contains(&policy.delegated_ticket_max_ttl_s)
        || !(1..=256).contains(&policy.queen_dedupe_entries)
        || !(256..=2048).contains(&policy.queen_intent_max_bytes)
        || !(256..=8192).contains(&policy.gpu_frame_max_bytes)
        || policy.writer_epoch == 0
    {
        bail!("authority bounds exceed the supported finite contract");
    }
    if policy.production_worker_ledger
        || policy.production_driver_ledger
        || policy.structured_quarantine
    {
        bail!("production Worker/driver ledger and structured quarantine require Milestone 28b evidence");
    }
    if policy.host_ai {
        bail!("host AI requires accepted downstream admission/provider milestones");
    }
    if policy.production_failover {
        bail!("production failover requires accepted durable cutover and external fence evidence");
    }
    if policy.production {
        if !policy.strict_queen_intents
            || policy.legacy_queen_ctl
            || !policy.writer_epoch_required
            || !policy.execution_wal_required
            || policy.debug_memory
            || !manifest.ecosystem.audit.enable
            || !manifest.ecosystem.audit.replay_enable
        {
            bail!("production authority requires strict intents, fencing, execution WAL, audit/replay, and disabled raw debug/legacy control");
        }
        if manifest.ecosystem.host.federation.enable {
            bail!(
                "Release A production federation requires separately accepted promotion evidence"
            );
        }
        for ticket in &manifest.tickets {
            if !ticket.secret.is_empty() || ticket.secret_ref.is_none() {
                bail!("production tickets require secret_ref and forbid literal secrets");
            }
        }
        if let Some(signing) = &manifest.cas.signing {
            if signing
                .verification_key_path
                .as_ref()
                .is_some_and(|path| path.contains("fixtures"))
            {
                bail!("production CAS verification cannot trust fixture signing material");
            }
        }
    }
    for ticket in &manifest.tickets {
        if let Some(reference) = &ticket.secret_ref {
            if !ticket.secret.is_empty() {
                bail!("ticket secret and secret_ref are mutually exclusive");
            }
            cohesix_authority::secret::validate_reference(reference)?;
        } else if ticket.secret.is_empty() {
            bail!("ticket requires a secret reference or explicit development literal");
        }
    }
    Ok(())
}

/// Materialize the supported single-writer Release A profile without importing
/// private key bytes into source manifests or generated host artifacts.
pub fn release_a(
    mut manifest: Manifest,
    verification_key: &std::path::Path,
    writer_epoch: u64,
) -> Result<Manifest> {
    if writer_epoch == 0 {
        bail!("writer epoch must be nonzero");
    }
    manifest.authority.production = true;
    manifest.authority.legacy_queen_ctl = false;
    manifest.authority.strict_queen_intents = true;
    manifest.authority.writer_epoch_required = true;
    manifest.authority.writer_epoch = writer_epoch;
    manifest.authority.execution_wal_required = true;
    manifest.authority.debug_memory = false;
    manifest.ecosystem.host.federation.enable = false;
    manifest.ecosystem.audit.enable = true;
    manifest.ecosystem.audit.replay_enable = true;
    for ticket in &mut manifest.tickets {
        ticket.secret.clear();
        ticket.secret_ref = Some(format!(
            "env:COH_TICKET_{}_KEY",
            ticket.role.as_str().replace('-', "_").to_ascii_uppercase()
        ));
    }
    let signing = manifest
        .cas
        .signing
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("Release A requires CAS signing"))?;
    signing.required = true;
    signing.verification_key_path = Some(
        verification_key
            .canonicalize()?
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("verification key path must be UTF-8"))?
            .to_owned(),
    );
    validate(&manifest)?;
    Ok(manifest)
}
