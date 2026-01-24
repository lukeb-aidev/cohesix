// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Load and validate manifest-derived coh host policies.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[allow(clippy::all)]
mod generated {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/generated/policy.rs"));
}

/// Coh host policy derived from the root-task manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohPolicy {
    /// Mount policy for host-side Secure9P views.
    pub mount: CohMountPolicy,
    /// Telemetry pull bounds and paths.
    pub telemetry: CohTelemetryPolicy,
    /// Retry scheduling policy for transports.
    pub retry: CohRetryPolicy,
}

impl CohPolicy {
    /// Return the manifest hash embedded in the generated defaults.
    #[must_use]
    pub fn manifest_hash() -> &'static str {
        generated::MANIFEST_SHA256
    }

    /// Return the policy hash embedded in the generated defaults.
    #[must_use]
    pub fn policy_hash() -> &'static str {
        generated::POLICY_SHA256
    }

    /// Construct a policy from the generated defaults.
    #[must_use]
    pub fn from_generated() -> Self {
        Self {
            mount: CohMountPolicy {
                root: generated::COH_MOUNT_ROOT.to_owned(),
                allowlist: generated::COH_MOUNT_ALLOWLIST
                    .iter()
                    .map(|value| (*value).to_owned())
                    .collect(),
            },
            telemetry: CohTelemetryPolicy {
                root: generated::COH_TELEMETRY_ROOT.to_owned(),
                max_devices: generated::COH_TELEMETRY_MAX_DEVICES,
                max_segments_per_device: generated::COH_TELEMETRY_MAX_SEGMENTS_PER_DEVICE,
                max_bytes_per_segment: generated::COH_TELEMETRY_MAX_BYTES_PER_SEGMENT,
                max_total_bytes_per_device: generated::COH_TELEMETRY_MAX_TOTAL_BYTES_PER_DEVICE,
            },
            retry: CohRetryPolicy {
                max_attempts: generated::COH_RETRY_MAX_ATTEMPTS,
                backoff_ms: generated::COH_RETRY_BACKOFF_MS,
                ceiling_ms: generated::COH_RETRY_CEILING_MS,
                timeout_ms: generated::COH_RETRY_TIMEOUT_MS,
            },
        }
    }
}

/// Mount policy for host-side Secure9P views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohMountPolicy {
    /// Remote root path exposed by the mount.
    pub root: String,
    /// Allowlisted path prefixes.
    pub allowlist: Vec<String>,
}

/// Telemetry pull policy bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohTelemetryPolicy {
    /// Root path used for telemetry pulls.
    pub root: String,
    /// Maximum devices pulled per invocation.
    pub max_devices: u32,
    /// Maximum segments pulled per device.
    pub max_segments_per_device: u32,
    /// Maximum bytes per segment.
    pub max_bytes_per_segment: u32,
    /// Maximum total bytes per device.
    pub max_total_bytes_per_device: u32,
}

/// Retry policy for transport operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CohRetryPolicy {
    /// Maximum retry attempts before failing.
    pub max_attempts: u8,
    /// Initial backoff delay in milliseconds.
    pub backoff_ms: u64,
    /// Maximum backoff delay in milliseconds.
    pub ceiling_ms: u64,
    /// Socket read/write timeout in milliseconds.
    pub timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyToml {
    meta: PolicyMeta,
    coh: CohTomlSection,
    retry: RetryTomlSection,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyMeta {
    manifest_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CohTomlSection {
    mount: MountTomlSection,
    telemetry: TelemetryTomlSection,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MountTomlSection {
    root: String,
    allowlist: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TelemetryTomlSection {
    root: String,
    max_devices: u32,
    max_segments_per_device: u32,
    max_bytes_per_segment: u32,
    max_total_bytes_per_device: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetryTomlSection {
    max_attempts: u8,
    backoff_ms: u64,
    ceiling_ms: u64,
    timeout_ms: u64,
}

/// Return the default policy path under the working directory or bundle root.
#[must_use]
pub fn default_policy_path() -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join("out/coh_policy.toml");
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            if let Some(root) = bin_dir.parent() {
                let candidate = root.join("out/coh_policy.toml");
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/coh_policy.toml")
}

/// Load and validate the coh policy from disk, enforcing hash alignment.
pub fn load_policy(path: &Path) -> Result<CohPolicy> {
    let contents = fs::read(path)
        .with_context(|| format!("failed to read coh policy {}", path.display()))?;
    let hash = hash_bytes(&contents);
    if hash != CohPolicy::policy_hash() {
        return Err(anyhow!(
            "coh policy hash mismatch: expected {} got {}",
            CohPolicy::policy_hash(),
            hash
        ));
    }
    let text = std::str::from_utf8(&contents)
        .with_context(|| format!("invalid UTF-8 in coh policy {}", path.display()))?;
    let parsed: PolicyToml = toml::from_str(text)
        .with_context(|| format!("invalid coh policy TOML in {}", path.display()))?;
    if parsed.meta.manifest_sha256 != CohPolicy::manifest_hash() {
        return Err(anyhow!(
            "coh policy manifest hash mismatch: expected {} got {}",
            CohPolicy::manifest_hash(),
            parsed.meta.manifest_sha256
        ));
    }
    let policy = CohPolicy {
        mount: CohMountPolicy {
            root: parsed.coh.mount.root,
            allowlist: parsed.coh.mount.allowlist,
        },
        telemetry: CohTelemetryPolicy {
            root: parsed.coh.telemetry.root,
            max_devices: parsed.coh.telemetry.max_devices,
            max_segments_per_device: parsed.coh.telemetry.max_segments_per_device,
            max_bytes_per_segment: parsed.coh.telemetry.max_bytes_per_segment,
            max_total_bytes_per_device: parsed.coh.telemetry.max_total_bytes_per_device,
        },
        retry: CohRetryPolicy {
            max_attempts: parsed.retry.max_attempts,
            backoff_ms: parsed.retry.backoff_ms,
            ceiling_ms: parsed.retry.ceiling_ms,
            timeout_ms: parsed.retry.timeout_ms,
        },
    };
    validate_policy(&policy)?;
    Ok(policy)
}

fn validate_policy(policy: &CohPolicy) -> Result<()> {
    validate_path("coh.mount.root", &policy.mount.root, true)?;
    if policy.mount.allowlist.is_empty() {
        return Err(anyhow!("coh.mount.allowlist must not be empty"));
    }
    for entry in &policy.mount.allowlist {
        validate_path("coh.mount.allowlist", entry, false)?;
    }
    validate_path("coh.telemetry.root", &policy.telemetry.root, false)?;
    if policy.telemetry.max_devices == 0 {
        return Err(anyhow!("coh.telemetry.max_devices must be >= 1"));
    }
    if policy.telemetry.max_segments_per_device == 0 {
        return Err(anyhow!(
            "coh.telemetry.max_segments_per_device must be >= 1"
        ));
    }
    if policy.telemetry.max_bytes_per_segment == 0 {
        return Err(anyhow!(
            "coh.telemetry.max_bytes_per_segment must be >= 1"
        ));
    }
    if policy.telemetry.max_total_bytes_per_device == 0 {
        return Err(anyhow!(
            "coh.telemetry.max_total_bytes_per_device must be >= 1"
        ));
    }
    if policy.retry.max_attempts == 0 {
        return Err(anyhow!("coh retry max_attempts must be >= 1"));
    }
    if policy.retry.backoff_ms == 0 {
        return Err(anyhow!("coh retry backoff_ms must be >= 1"));
    }
    if policy.retry.ceiling_ms < policy.retry.backoff_ms {
        return Err(anyhow!(
            "coh retry ceiling_ms {} must be >= backoff_ms {}",
            policy.retry.ceiling_ms,
            policy.retry.backoff_ms
        ));
    }
    if policy.retry.timeout_ms == 0 {
        return Err(anyhow!("coh retry timeout_ms must be >= 1"));
    }
    Ok(())
}

fn validate_path(label: &str, value: &str, allow_root: bool) -> Result<()> {
    if !value.starts_with('/') {
        return Err(anyhow!("{label} must be absolute"));
    }
    if value == "/" {
        if allow_root {
            return Ok(());
        }
        return Err(anyhow!("{label} must not be '/'"));
    }
    let mut depth = 0usize;
    for component in value.split('/').skip(1) {
        if component.is_empty() {
            continue;
        }
        if component == "." || component == ".." {
            return Err(anyhow!("{label} contains invalid component '{component}'"));
        }
        if component.as_bytes().iter().any(|byte| *byte == 0) {
            return Err(anyhow!("{label} contains NUL byte"));
        }
        depth += 1;
        if depth > crate::MAX_PATH_COMPONENTS {
            return Err(anyhow!(
                "{label} exceeds max depth {}",
                crate::MAX_PATH_COMPONENTS
            ));
        }
    }
    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
