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
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/generated/policy.rs"
    ));
}

/// Coh host policy derived from the root-task manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohPolicy {
    /// Mount policy for host-side Secure9P views.
    pub mount: CohMountPolicy,
    /// Telemetry pull bounds and paths.
    pub telemetry: CohTelemetryPolicy,
    /// Runtime wrapper policy for coh run.
    pub run: CohRunPolicy,
    /// PEFT/LoRA import/export policy.
    pub peft: CohPeftPolicy,
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
            run: CohRunPolicy {
                lease: CohLeasePolicy {
                    schema: generated::COH_RUN_LEASE_SCHEMA.to_owned(),
                    active_state: generated::COH_RUN_LEASE_ACTIVE_STATE.to_owned(),
                    max_bytes: generated::COH_RUN_LEASE_MAX_BYTES,
                },
                breadcrumb: CohBreadcrumbPolicy {
                    schema: generated::COH_RUN_BREADCRUMB_SCHEMA.to_owned(),
                    max_line_bytes: generated::COH_RUN_BREADCRUMB_MAX_LINE_BYTES,
                    max_command_bytes: generated::COH_RUN_BREADCRUMB_MAX_COMMAND_BYTES,
                },
            },
            peft: CohPeftPolicy {
                export: CohPeftExportPolicy {
                    root: generated::COH_PEFT_EXPORT_ROOT.to_owned(),
                    max_telemetry_bytes: generated::COH_PEFT_EXPORT_MAX_TELEMETRY_BYTES,
                    max_policy_bytes: generated::COH_PEFT_EXPORT_MAX_POLICY_BYTES,
                    max_base_model_bytes: generated::COH_PEFT_EXPORT_MAX_BASE_MODEL_BYTES,
                },
                import: CohPeftImportPolicy {
                    registry_root: generated::COH_PEFT_IMPORT_REGISTRY_ROOT.to_owned(),
                    max_adapter_bytes: generated::COH_PEFT_IMPORT_MAX_ADAPTER_BYTES,
                    max_lora_bytes: generated::COH_PEFT_IMPORT_MAX_LORA_BYTES,
                    max_metrics_bytes: generated::COH_PEFT_IMPORT_MAX_METRICS_BYTES,
                    max_manifest_bytes: generated::COH_PEFT_IMPORT_MAX_MANIFEST_BYTES,
                },
                activate: CohPeftActivatePolicy {
                    max_model_id_bytes: generated::COH_PEFT_ACTIVATE_MAX_MODEL_ID_BYTES,
                    max_state_bytes: generated::COH_PEFT_ACTIVATE_MAX_STATE_BYTES,
                },
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

/// Runtime wrapper policy for coh run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohRunPolicy {
    /// Lease validation defaults.
    pub lease: CohLeasePolicy,
    /// Breadcrumb emission limits.
    pub breadcrumb: CohBreadcrumbPolicy,
}

/// PEFT/LoRA policy for export/import/activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohPeftPolicy {
    /// Export policy.
    pub export: CohPeftExportPolicy,
    /// Import policy.
    pub import: CohPeftImportPolicy,
    /// Activation policy.
    pub activate: CohPeftActivatePolicy,
}

/// PEFT export bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohPeftExportPolicy {
    /// Root path containing LoRA export jobs.
    pub root: String,
    /// Maximum bytes allowed for telemetry payloads.
    pub max_telemetry_bytes: u32,
    /// Maximum bytes allowed for policy payloads.
    pub max_policy_bytes: u32,
    /// Maximum bytes allowed for base model refs.
    pub max_base_model_bytes: u32,
}

/// PEFT import bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohPeftImportPolicy {
    /// Host-side registry root for imported adapters.
    pub registry_root: String,
    /// Maximum adapter payload bytes.
    pub max_adapter_bytes: u64,
    /// Maximum bytes for lora.json metadata.
    pub max_lora_bytes: u32,
    /// Maximum bytes for metrics metadata.
    pub max_metrics_bytes: u32,
    /// Maximum bytes for generated manifest.
    pub max_manifest_bytes: u32,
}

/// PEFT activation bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohPeftActivatePolicy {
    /// Maximum bytes for model identifiers.
    pub max_model_id_bytes: u32,
    /// Maximum bytes for persisted activation state.
    pub max_state_bytes: u32,
}

/// Lease validation settings for coh run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohLeasePolicy {
    /// Lease schema identifier.
    pub schema: String,
    /// Active state string required in lease entries.
    pub active_state: String,
    /// Maximum bytes read from the lease file.
    pub max_bytes: u32,
}

/// Breadcrumb emission settings for coh run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohBreadcrumbPolicy {
    /// Breadcrumb schema identifier.
    pub schema: String,
    /// Maximum bytes per breadcrumb line.
    pub max_line_bytes: u32,
    /// Maximum bytes of the command string.
    pub max_command_bytes: u32,
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
    run: RunTomlSection,
    peft: PeftTomlSection,
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
struct RunTomlSection {
    lease: LeaseTomlSection,
    breadcrumb: BreadcrumbTomlSection,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PeftTomlSection {
    export: PeftExportTomlSection,
    import: PeftImportTomlSection,
    activate: PeftActivateTomlSection,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PeftExportTomlSection {
    root: String,
    max_telemetry_bytes: u32,
    max_policy_bytes: u32,
    max_base_model_bytes: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PeftImportTomlSection {
    registry_root: String,
    max_adapter_bytes: u64,
    max_lora_bytes: u32,
    max_metrics_bytes: u32,
    max_manifest_bytes: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PeftActivateTomlSection {
    max_model_id_bytes: u32,
    max_state_bytes: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaseTomlSection {
    schema: String,
    active_state: String,
    max_bytes: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BreadcrumbTomlSection {
    schema: String,
    max_line_bytes: u32,
    max_command_bytes: u32,
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
    let contents =
        fs::read(path).with_context(|| format!("failed to read coh policy {}", path.display()))?;
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
        run: CohRunPolicy {
            lease: CohLeasePolicy {
                schema: parsed.coh.run.lease.schema,
                active_state: parsed.coh.run.lease.active_state,
                max_bytes: parsed.coh.run.lease.max_bytes,
            },
            breadcrumb: CohBreadcrumbPolicy {
                schema: parsed.coh.run.breadcrumb.schema,
                max_line_bytes: parsed.coh.run.breadcrumb.max_line_bytes,
                max_command_bytes: parsed.coh.run.breadcrumb.max_command_bytes,
            },
        },
        peft: CohPeftPolicy {
            export: CohPeftExportPolicy {
                root: parsed.coh.peft.export.root,
                max_telemetry_bytes: parsed.coh.peft.export.max_telemetry_bytes,
                max_policy_bytes: parsed.coh.peft.export.max_policy_bytes,
                max_base_model_bytes: parsed.coh.peft.export.max_base_model_bytes,
            },
            import: CohPeftImportPolicy {
                registry_root: parsed.coh.peft.import.registry_root,
                max_adapter_bytes: parsed.coh.peft.import.max_adapter_bytes,
                max_lora_bytes: parsed.coh.peft.import.max_lora_bytes,
                max_metrics_bytes: parsed.coh.peft.import.max_metrics_bytes,
                max_manifest_bytes: parsed.coh.peft.import.max_manifest_bytes,
            },
            activate: CohPeftActivatePolicy {
                max_model_id_bytes: parsed.coh.peft.activate.max_model_id_bytes,
                max_state_bytes: parsed.coh.peft.activate.max_state_bytes,
            },
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
        return Err(anyhow!("coh.telemetry.max_bytes_per_segment must be >= 1"));
    }
    if policy.telemetry.max_total_bytes_per_device == 0 {
        return Err(anyhow!(
            "coh.telemetry.max_total_bytes_per_device must be >= 1"
        ));
    }
    if policy.run.lease.schema.trim().is_empty() {
        return Err(anyhow!("coh.run.lease.schema must not be empty"));
    }
    if policy.run.lease.active_state.trim().is_empty() {
        return Err(anyhow!("coh.run.lease.active_state must not be empty"));
    }
    if policy.run.lease.max_bytes == 0 {
        return Err(anyhow!("coh.run.lease.max_bytes must be >= 1"));
    }
    if policy.run.breadcrumb.schema.trim().is_empty() {
        return Err(anyhow!("coh.run.breadcrumb.schema must not be empty"));
    }
    if policy.run.breadcrumb.max_line_bytes == 0 {
        return Err(anyhow!("coh.run.breadcrumb.max_line_bytes must be >= 1"));
    }
    if policy.run.breadcrumb.max_command_bytes == 0 {
        return Err(anyhow!("coh.run.breadcrumb.max_command_bytes must be >= 1"));
    }
    if policy.run.breadcrumb.max_command_bytes > policy.run.breadcrumb.max_line_bytes {
        return Err(anyhow!(
            "coh.run.breadcrumb.max_command_bytes {} exceeds max_line_bytes {}",
            policy.run.breadcrumb.max_command_bytes,
            policy.run.breadcrumb.max_line_bytes
        ));
    }
    validate_path("coh.peft.export.root", &policy.peft.export.root, false)?;
    if policy.peft.export.max_telemetry_bytes == 0 {
        return Err(anyhow!("coh.peft.export.max_telemetry_bytes must be >= 1"));
    }
    if policy.peft.export.max_telemetry_bytes > policy.telemetry.max_total_bytes_per_device {
        return Err(anyhow!(
            "coh.peft.export.max_telemetry_bytes {} exceeds coh.telemetry.max_total_bytes_per_device {}",
            policy.peft.export.max_telemetry_bytes,
            policy.telemetry.max_total_bytes_per_device
        ));
    }
    if policy.peft.export.max_policy_bytes == 0 {
        return Err(anyhow!("coh.peft.export.max_policy_bytes must be >= 1"));
    }
    if policy.peft.export.max_base_model_bytes == 0 {
        return Err(anyhow!("coh.peft.export.max_base_model_bytes must be >= 1"));
    }
    validate_host_path(
        "coh.peft.import.registry_root",
        policy.peft.import.registry_root.as_str(),
    )?;
    if policy.peft.import.max_adapter_bytes == 0 {
        return Err(anyhow!("coh.peft.import.max_adapter_bytes must be >= 1"));
    }
    if policy.peft.import.max_lora_bytes == 0 {
        return Err(anyhow!("coh.peft.import.max_lora_bytes must be >= 1"));
    }
    if policy.peft.import.max_metrics_bytes == 0 {
        return Err(anyhow!("coh.peft.import.max_metrics_bytes must be >= 1"));
    }
    if policy.peft.import.max_manifest_bytes == 0 {
        return Err(anyhow!("coh.peft.import.max_manifest_bytes must be >= 1"));
    }
    if policy.peft.activate.max_model_id_bytes == 0 {
        return Err(anyhow!("coh.peft.activate.max_model_id_bytes must be >= 1"));
    }
    if policy.peft.activate.max_state_bytes == 0 {
        return Err(anyhow!("coh.peft.activate.max_state_bytes must be >= 1"));
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

fn validate_host_path(label: &str, value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("{label} must not be empty"));
    }
    if trimmed.as_bytes().iter().any(|byte| *byte == 0) {
        return Err(anyhow!("{label} contains NUL byte"));
    }
    let mut depth = 0usize;
    for component in trimmed.split('/').filter(|seg| !seg.is_empty()) {
        if component == "." || component == ".." {
            return Err(anyhow!("{label} contains invalid component '{component}'"));
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
