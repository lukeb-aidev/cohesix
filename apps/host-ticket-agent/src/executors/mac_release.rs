// Author: Lukas Bower
// Purpose: Dispatch compiler-selected macOS release attempts without accepting caller commands or credentials.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use super::{observation, ExecutorConfig};
use crate::HostTicketSpec;
use anyhow::{anyhow, ensure, Result};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

/// Retain only independently verified native postconditions under the admitted ticket identity.
pub fn execute(spec: &HostTicketSpec, config: &ExecutorConfig) -> Result<String> {
    ensure!(
        spec.schema == "host-ticket/v1",
        "not_supported macos-worker-receipt"
    );
    let id = spec.args["target_id"]
        .as_str()
        .ok_or_else(|| anyhow!("EPERM macos-target"))?;
    let target = cohesix_authority::mac_release::targets()?
        .into_iter()
        .find(|t| t.id == id && t.operation.action() == spec.action)
        .ok_or_else(|| anyhow!("not_enabled macos-target"))?;
    let root = observation::prepare(config)?.canonicalize()?;
    // Every attempt is fenced by the durable agent WAL. A fresh directory also
    // prevents accidental native replay if state is misconfigured after restart.
    let attempt = hex::encode(Sha256::digest(serde_json::to_vec(&(
        &spec.id,
        &spec.idempotency_key,
        spec.writer_epoch,
    ))?));
    let timeout = cohesix_authority::provider::action(&spec.action)?["timeout_ms"]
        .as_u64()
        .ok_or_else(|| anyhow!("EPERM macos-deadline"))?;
    let result = host_sidecar_bridge::mac_release::execute(
        &target,
        &root.join(format!("macos-{attempt}")),
        Instant::now() + Duration::from_millis(timeout),
    )?;
    let identity = format!(
        "macos:{}:{}",
        target.id,
        hex::encode(Sha256::digest(serde_json::to_vec(&result)?))
    );
    observation::retain_native(config, spec, &result, &identity)
}
