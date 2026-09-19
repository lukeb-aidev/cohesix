// Author: Lukas Bower
// Purpose: Bound GPU workload ticket arguments identically before root admission and host dispatch.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use alloc::string::String;
use serde::{Deserialize, Serialize};

/// A submission names immutable host CAS input and an already admitted lease.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadSubmit {
    /// Exact active root lease, never an arbitrary path.
    pub lease_id: String,
    /// SHA-256 of the bounded workload request in the configured host CAS.
    pub request_sha256: String,
}

/// Observe or cancel an existing job on the same Worker and GPU binding.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadControl {
    /// The original submit ticket ID, used only after binding validation.
    pub job_id: String,
}

/// Validate the exact host-ticket/v2 argument ABI without host dependencies.
pub fn validate_args(action: &str, args: &serde_json::Value) -> bool {
    if action == "gpu.workload.submit" {
        serde_json::from_value::<WorkloadSubmit>(args.clone()).is_ok_and(|args| {
            crate::validate_id(&args.lease_id).is_ok()
                && args.lease_id.len() <= 32
                && args.request_sha256.len() == 64
                && args
                    .request_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    } else if matches!(action, "gpu.workload.cancel" | "gpu.workload.observe") {
        serde_json::from_value::<WorkloadControl>(args.clone())
            .is_ok_and(|args| crate::validate_id(&args.job_id).is_ok())
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mig_profile_preserves_exact_resource_and_transport_bounds() {
        let mut contract = ExecutorContract::default();
        assert!(contract.is_supported());
        contract.profile = "nvidia-mig-cuda13".into();
        assert!(contract.is_supported());
        contract.maximum_streams = 2;
        assert!(!contract.is_supported());
        contract.maximum_streams = 1;
        contract.profile = "arbitrary".into();
        assert!(!contract.is_supported());
    }

    #[test]
    fn workload_refs_reject_paths_missing_fields_and_extra_authority() {
        let args = serde_json::json!({"lease_id":"lease-1", "request_sha256":"a".repeat(64)});
        assert!(validate_args("gpu.workload.submit", &args));
        for bad in [
            serde_json::json!({"lease_id":"../lease", "request_sha256":"a".repeat(64)}),
            serde_json::json!({"lease_id":"lease-1"}),
            serde_json::json!({"lease_id":"lease-1", "request_sha256":"A".repeat(64)}),
            serde_json::json!({"lease_id":"lease-1", "request_sha256":"a".repeat(64), "approved":true}),
        ] {
            assert!(!validate_args("gpu.workload.submit", &bad));
        }
        assert!(validate_args(
            "gpu.workload.cancel",
            &serde_json::json!({"job_id":"job-1"})
        ));
        assert!(!validate_args(
            "gpu.workload.cancel",
            &serde_json::json!({"job_id":"/job"})
        ));
    }
}

/// Compiler-declared host-local transport; changing bounds requires regeneration.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutorContract {
    pub schema: String,
    pub transport: String,
    pub authentication: String,
    pub maximum_frame_bytes: usize,
    pub maximum_jobs: usize,
    pub maximum_wal_bytes: usize,
    pub lease_heartbeat_ttl_ms: u64,
    pub maximum_request_bytes: usize,
    pub maximum_device_allocation_bytes: u64,
    pub maximum_streams: u32,
    pub maximum_runtime_ms: u32,
    pub entrypoints: alloc::vec::Vec<String>,
    pub profile: String,
}
impl Default for ExecutorContract {
    fn default() -> Self {
        Self {
            schema: "cohesix-gpu-local/v1".into(),
            transport: "unix_socket".into(),
            authentication: "hmac_sha256".into(),
            maximum_frame_bytes: 16384,
            maximum_jobs: 64,
            maximum_wal_bytes: 4194304,
            lease_heartbeat_ttl_ms: 1000,
            maximum_request_bytes: 8192,
            maximum_device_allocation_bytes: 67108864,
            maximum_streams: 1,
            maximum_runtime_ms: 30000,
            entrypoints: alloc::vec!["vadd".into(), "matmul".into()],
            profile: "jetson-orin-nano-jp7".into(),
        }
    }
}

impl ExecutorContract {
    /// Both native profiles preserve the same finite transport and resource ABI.
    /// Selecting a MIG profile does not qualify its host or native execution.
    pub fn is_supported(&self) -> bool {
        if !matches!(
            self.profile.as_str(),
            "jetson-orin-nano-jp7" | "nvidia-mig-cuda13"
        ) {
            return false;
        }
        let expected = Self {
            profile: self.profile.clone(),
            ..Self::default()
        };
        self == &expected
    }
}

/// Stable device and publisher identity; snapshot refreshes do not change it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedDevice {
    /// Version of the device publication contract.
    pub schema: String,
    /// CUDA's immutable device UUID, lowercase hexadecimal.
    pub device_uuid: String,
    /// Exact CUDA ordinal associated with the UUID.
    pub device_ordinal: u32,
    /// Immutable attributes, native helper and host boot identity digest.
    pub topology_sha256: String,
    /// Exact native helper artifact digest.
    pub helper_sha256: String,
    /// Selected generated provider contract digest.
    pub provider_graph_sha256: String,
    /// Native publisher identity.
    pub source_id: String,
    /// Publisher lifetime, fenced on replacement.
    pub source_epoch: u64,
    /// Explicit production/fixture classification.
    pub source_mode: String,
}
