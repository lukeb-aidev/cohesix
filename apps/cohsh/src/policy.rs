// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Load and validate manifest-derived cohsh client policies.
// Author: Lukas Bower

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[allow(clippy::all)]
mod generated {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/generated/policy.rs"
    ));
}

/// Cohsh client policy derived from the root-task manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CohshPolicy {
    /// Session pool sizing policy.
    pub pool: CohshPoolPolicy,
    /// Retry scheduling policy for transport operations.
    pub retry: CohshRetryPolicy,
    /// Heartbeat cadence policy for transport keepalives.
    pub heartbeat: CohshHeartbeatPolicy,
    /// Trace capture size limits.
    pub trace: CohshTracePolicy,
}

impl CohshPolicy {
    /// Return the manifest hash embedded in the generated defaults.
    pub fn manifest_hash() -> &'static str {
        generated::MANIFEST_SHA256
    }

    /// Return the policy hash embedded in the generated defaults.
    pub fn policy_hash() -> &'static str {
        generated::POLICY_SHA256
    }

    /// Construct a policy from the generated defaults.
    pub fn from_generated() -> Self {
        Self {
            pool: CohshPoolPolicy {
                control_sessions: generated::COHSH_POOL_CONTROL_SESSIONS,
                telemetry_sessions: generated::COHSH_POOL_TELEMETRY_SESSIONS,
            },
            retry: CohshRetryPolicy {
                max_attempts: generated::COHSH_RETRY_MAX_ATTEMPTS,
                backoff_ms: generated::COHSH_RETRY_BACKOFF_MS,
                ceiling_ms: generated::COHSH_RETRY_CEILING_MS,
                timeout_ms: generated::COHSH_RETRY_TIMEOUT_MS,
            },
            heartbeat: CohshHeartbeatPolicy {
                interval_ms: generated::COHSH_HEARTBEAT_INTERVAL_MS,
            },
            trace: CohshTracePolicy {
                max_bytes: generated::COHSH_TRACE_MAX_BYTES,
            },
        }
    }

    /// Apply overrides and return an updated policy.
    pub fn with_overrides(self, overrides: &PolicyOverrides) -> Result<Self> {
        let mut updated = self;
        if let Some(value) = overrides.pool_control_sessions {
            updated.pool.control_sessions = value;
        }
        if let Some(value) = overrides.pool_telemetry_sessions {
            updated.pool.telemetry_sessions = value;
        }
        if let Some(value) = overrides.retry_max_attempts {
            updated.retry.max_attempts = value;
        }
        if let Some(value) = overrides.retry_backoff_ms {
            updated.retry.backoff_ms = value;
        }
        if let Some(value) = overrides.retry_ceiling_ms {
            updated.retry.ceiling_ms = value;
        }
        if let Some(value) = overrides.retry_timeout_ms {
            updated.retry.timeout_ms = value;
        }
        if let Some(value) = overrides.heartbeat_interval_ms {
            updated.heartbeat.interval_ms = value;
        }
        validate_policy(&updated)?;
        Ok(updated)
    }
}

/// Cohsh session pool sizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CohshPoolPolicy {
    /// Number of pooled control sessions.
    pub control_sessions: u16,
    /// Number of pooled telemetry sessions.
    pub telemetry_sessions: u16,
}

/// Retry scheduling policy for cohsh transports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CohshRetryPolicy {
    /// Maximum retry attempts before failing.
    pub max_attempts: u8,
    /// Initial backoff delay in milliseconds.
    pub backoff_ms: u64,
    /// Maximum backoff delay in milliseconds.
    pub ceiling_ms: u64,
    /// Socket read/write timeout in milliseconds.
    pub timeout_ms: u64,
}

/// Heartbeat scheduling policy for cohsh transports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CohshHeartbeatPolicy {
    /// Heartbeat interval in milliseconds.
    pub interval_ms: u64,
}

/// Trace capture limits for cohsh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CohshTracePolicy {
    /// Maximum encoded trace size in bytes.
    pub max_bytes: u32,
}

/// Optional overrides layered on top of the manifest-derived policy.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PolicyOverrides {
    /// Override pooled control session capacity.
    pub pool_control_sessions: Option<u16>,
    /// Override pooled telemetry session capacity.
    pub pool_telemetry_sessions: Option<u16>,
    /// Override retry max attempts.
    pub retry_max_attempts: Option<u8>,
    /// Override retry backoff in milliseconds.
    pub retry_backoff_ms: Option<u64>,
    /// Override retry ceiling in milliseconds.
    pub retry_ceiling_ms: Option<u64>,
    /// Override retry timeout in milliseconds.
    pub retry_timeout_ms: Option<u64>,
    /// Override heartbeat interval in milliseconds.
    pub heartbeat_interval_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyToml {
    meta: PolicyMeta,
    cohsh: CohshTomlSection,
    retry: RetryTomlSection,
    heartbeat: HeartbeatTomlSection,
    trace: TraceTomlSection,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyMeta {
    manifest_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CohshTomlSection {
    pool: PoolTomlSection,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PoolTomlSection {
    control_sessions: u16,
    telemetry_sessions: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetryTomlSection {
    max_attempts: u8,
    backoff_ms: u64,
    ceiling_ms: u64,
    timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatTomlSection {
    interval_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceTomlSection {
    max_bytes: u32,
}

/// Return the default policy path under the working directory or bundle root.
pub fn default_policy_path() -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join("out/cohsh_policy.toml");
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            if let Some(root) = bin_dir.parent() {
                let candidate = root.join("out/cohsh_policy.toml");
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/cohsh_policy.toml")
}

/// Load and validate the cohsh policy from disk, enforcing hash alignment.
pub fn load_policy(path: &Path) -> Result<CohshPolicy> {
    let contents = fs::read(path)
        .with_context(|| format!("failed to read cohsh policy {}", path.display()))?;
    let hash = hash_bytes(&contents);
    if hash != CohshPolicy::policy_hash() {
        return Err(anyhow!(
            "cohsh policy hash mismatch: expected {} got {}",
            CohshPolicy::policy_hash(),
            hash
        ));
    }
    let text = std::str::from_utf8(&contents)
        .with_context(|| format!("invalid UTF-8 in cohsh policy {}", path.display()))?;
    let parsed: PolicyToml = toml::from_str(text)
        .with_context(|| format!("invalid cohsh policy TOML in {}", path.display()))?;
    if parsed.meta.manifest_sha256 != CohshPolicy::manifest_hash() {
        return Err(anyhow!(
            "cohsh policy manifest hash mismatch: expected {} got {}",
            CohshPolicy::manifest_hash(),
            parsed.meta.manifest_sha256
        ));
    }
    let policy = CohshPolicy {
        pool: CohshPoolPolicy {
            control_sessions: parsed.cohsh.pool.control_sessions,
            telemetry_sessions: parsed.cohsh.pool.telemetry_sessions,
        },
        retry: CohshRetryPolicy {
            max_attempts: parsed.retry.max_attempts,
            backoff_ms: parsed.retry.backoff_ms,
            ceiling_ms: parsed.retry.ceiling_ms,
            timeout_ms: parsed.retry.timeout_ms,
        },
        heartbeat: CohshHeartbeatPolicy {
            interval_ms: parsed.heartbeat.interval_ms,
        },
        trace: CohshTracePolicy {
            max_bytes: parsed.trace.max_bytes,
        },
    };
    validate_policy(&policy)?;
    Ok(policy)
}

fn validate_policy(policy: &CohshPolicy) -> Result<()> {
    if policy.pool.control_sessions == 0 {
        return Err(anyhow!("cohsh pool control_sessions must be >= 1"));
    }
    if policy.pool.telemetry_sessions == 0 {
        return Err(anyhow!("cohsh pool telemetry_sessions must be >= 1"));
    }
    if policy.retry.max_attempts == 0 {
        return Err(anyhow!("cohsh retry max_attempts must be >= 1"));
    }
    if policy.retry.backoff_ms == 0 {
        return Err(anyhow!("cohsh retry backoff_ms must be >= 1"));
    }
    if policy.retry.ceiling_ms < policy.retry.backoff_ms {
        return Err(anyhow!(
            "cohsh retry ceiling_ms {} must be >= backoff_ms {}",
            policy.retry.ceiling_ms,
            policy.retry.backoff_ms
        ));
    }
    if policy.retry.timeout_ms == 0 {
        return Err(anyhow!("cohsh retry timeout_ms must be >= 1"));
    }
    if policy.heartbeat.interval_ms == 0 {
        return Err(anyhow!("cohsh heartbeat interval_ms must be >= 1"));
    }
    if policy.trace.max_bytes == 0 {
        return Err(anyhow!("cohsh trace max_bytes must be > 0"));
    }
    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
