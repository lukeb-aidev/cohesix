// Author: Lukas Bower
// Purpose: Durably retain bounded native provider observations before publishing compact content references in ticket results.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::ExecutorConfig;
use crate::HostTicketSpec;

const MAX_BYTES: usize = 65_536;
const MAX_OBJECTS: usize = 4096;

/// Refuse an unsafe/full evidence destination before a provider side effect.
pub fn prepare(config: &ExecutorConfig) -> Result<PathBuf> {
    let root = &config.provider_evidence_root;
    match fs::symlink_metadata(root) {
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            bail!("invalid_target provider-evidence-root")
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(root)?;
        }
        Err(error) => return Err(error.into()),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(root)?.permissions().mode() & 0o077 != 0 {
            bail!("EPERM provider-evidence-root requires private permissions");
        }
    }
    if fs::read_dir(root)?.take(MAX_OBJECTS).count() >= MAX_OBJECTS {
        bail!("ELIMIT provider-evidence-capacity");
    }
    Ok(root.clone())
}

/// Bind a native observation to the dispatched ticket without claiming Worker authority.
pub fn retain<T: Serialize>(
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
    observation: &T,
) -> Result<String> {
    retain_inner(
        config,
        spec,
        observation,
        None,
        None,
        cohesix_evidence::Outcome::Verified,
    )
}

/// Retain a provider-owned native object identity and extend the enrolled causal chain.
pub fn retain_native<T: Serialize>(
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
    observation: &T,
    native_identity: &str,
) -> Result<String> {
    retain_inner(
        config,
        spec,
        observation,
        Some(native_identity),
        None,
        cohesix_evidence::Outcome::Verified,
    )
}

/// A durable native ACK supplies its original timestamp on every reconciliation.
pub fn retain_native_at<T: Serialize>(
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
    observation: &T,
    native_identity: &str,
    observed_unix_ms: u64,
) -> Result<String> {
    retain_inner(
        config,
        spec,
        observation,
        Some(native_identity),
        Some(observed_unix_ms),
        cohesix_evidence::Outcome::Verified,
    )
}

/// Retain an actual terminal native outcome, including failure/cancel/expiry,
/// with its original durable time so reconciliation produces identical bytes.
pub fn retain_native_outcome_at<T: Serialize>(
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
    observation: &T,
    native_identity: &str,
    observed_unix_ms: u64,
    outcome: cohesix_evidence::Outcome,
) -> Result<String> {
    retain_inner(
        config,
        spec,
        observation,
        Some(native_identity),
        Some(observed_unix_ms),
        outcome,
    )
}

fn retain_inner<T: Serialize>(
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
    observation: &T,
    native_identity: Option<&str>,
    observed_unix_ms: Option<u64>,
    outcome: cohesix_evidence::Outcome,
) -> Result<String> {
    let root = prepare(config)?;
    let observed = match observed_unix_ms {
        Some(value) => value,
        None => u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?,
    };
    let value = serde_json::json!({
        "schema": "cohesix-native-provider-evidence/v1", "proof_class": "native_provider_operation",
        "authoritative": false, "worker_proof": false,
        "ticket_id": spec.id, "idempotency_key": spec.idempotency_key, "action": spec.action,
        "writer_epoch": spec.writer_epoch,
        "provider_graph_sha256": cohesix_authority::provider::registry()?["graph_sha256"],
        "observed_unix_ms": observed,
        "observation": observation,
    });
    let bytes = serde_json::to_vec(&value)?;
    let reference = store(&root, &bytes)?;
    if let Some(identity) = native_identity {
        crate::causal::observed_outcome(config, spec, &bytes, identity, outcome)?;
    }
    Ok(reference)
}

fn store(root: &Path, bytes: &[u8]) -> Result<String> {
    if bytes.len() > MAX_BYTES {
        bail!("ELIMIT provider-observation-bytes");
    }
    let digest = hex::encode(Sha256::digest(bytes));
    let path = root.join(format!("{digest}.json"));
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file()
                || metadata.len() != bytes.len() as u64
                || fs::read(&path)? != bytes
            {
                bail!("invalid_evidence provider-object-mismatch");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            crate::wal::durable_atomic_write(&path, bytes, MAX_BYTES, "provider observation")?;
        }
        Err(error) => return Err(anyhow!("provider object unavailable: {error}")),
    }
    Ok(format!("native_observation=sha256:{digest}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_reference_survives_readback_and_rejects_changed_bytes_and_oversize() {
        let directory = tempfile::tempdir().unwrap();
        let bytes = br#"{"native_id":"observed-id","terminal":true}"#;
        let reference = store(directory.path(), bytes).unwrap();
        let digest = reference
            .strip_prefix("native_observation=sha256:")
            .unwrap();
        let path = directory.path().join(format!("{digest}.json"));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(store(directory.path(), bytes).unwrap(), reference);
        fs::write(&path, b"corrupt").unwrap();
        assert!(store(directory.path(), bytes).is_err());
        assert!(store(directory.path(), &vec![0; MAX_BYTES + 1]).is_err());
    }
}
