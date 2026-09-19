// Author: Lukas Bower
// Purpose: Validate native NVML MIG observations and exact enrolled instance selection before CUDA dispatch.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]
#![allow(missing_docs)]

use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub start: u32,
    pub size: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub uuid: String,
    pub gpu_instance_id: u32,
    pub compute_instance_id: u32,
    pub gpu_profile_id: u32,
    pub compute_profile_id: u32,
    pub memory_bytes: u64,
    pub gpu_placement: Placement,
    pub compute_placement: Placement,
}

/// Publicly observed immutable identity; this does not grant a lease or reserve memory.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub parent_ordinal: u32,
    pub parent_uuid: String,
    pub instance: Instance,
    pub topology_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Topology {
    pub schema: String,
    pub source: String,
    pub availability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_error: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_ordinal: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    pub instances: Vec<Instance>,
}

fn uuid(value: &str, prefix: &str) -> bool {
    let Some(value) = value.strip_prefix(prefix) else {
        return false;
    };
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
        && value.bytes().any(|byte| byte != b'0' && byte != b'-')
}

impl Instance {
    fn validate(&self) -> Result<()> {
        ensure!(
            uuid(&self.uuid, "MIG-") && self.memory_bytes > 0 && self.memory_bytes <= 16_u64 << 40,
            "invalid_mig_identity"
        );
        for placement in [&self.gpu_placement, &self.compute_placement] {
            ensure!(
                placement.start < 64
                    && (1..=64).contains(&placement.size)
                    && placement.start + placement.size <= 64,
                "invalid_mig_placement"
            );
        }
        ensure!(
            self.compute_placement.start + self.compute_placement.size <= self.gpu_placement.size,
            "invalid_compute_instance_placement"
        );
        Ok(())
    }
}

impl Selection {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.parent_ordinal < 32 && uuid(&self.parent_uuid, "GPU-"),
            "invalid_mig_parent"
        );
        ensure!(
            self.topology_sha256.len() == 64
                && self
                    .topology_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "invalid_mig_generation"
        );
        self.instance.validate()
    }

    pub fn cuda_uuid(&self) -> Result<String> {
        self.validate()?;
        Ok(self.instance.uuid[4..].replace('-', ""))
    }

    /// Mutable labels or a reused GI/CI integer cannot replace the enrolled native UUID and placement.
    pub fn require_observed(&self, topology: &Topology) -> Result<()> {
        self.validate()?;
        topology.validate()?;
        ensure!(
            topology.availability == "observed",
            "not_supported_or_unavailable MIG"
        );
        ensure!(
            topology.parent_ordinal == Some(self.parent_ordinal)
                && topology.parent_uuid.as_deref() == Some(self.parent_uuid.as_str()),
            "mig_parent_changed"
        );
        ensure!(
            topology.generation()? == self.topology_sha256,
            "stale_mig_generation"
        );
        let selected = topology
            .instances
            .iter()
            .find(|instance| instance.uuid == self.instance.uuid);
        ensure!(selected == Some(&self.instance), "stale_mig_instance");
        Ok(())
    }
}

impl Topology {
    /// Hash sorted native identities; observation time cannot renew a stale placement.
    pub fn generation(&self) -> Result<String> {
        self.validate()?;
        let mut canonical = self.clone();
        canonical.instances.sort_by(|a, b| a.uuid.cmp(&b.uuid));
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(
            &serde_json::to_value(canonical)?,
        )?)))
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == "cohesix-nvml-mig-topology/v1"
                && self.source == "nvml"
                && self.instances.len() <= 64,
            "invalid_mig_topology"
        );
        if self.availability != "observed" {
            ensure!(
                matches!(
                    self.availability.as_str(),
                    "not_enabled" | "not_supported" | "unavailable"
                ) && self.instances.is_empty()
                    && self.parent_uuid.is_none()
                    && self.parent_ordinal.is_none()
                    && self.mode.is_none()
                    && self.reason.as_ref().is_some_and(|reason| !reason.is_empty()
                        && reason.len() <= 64
                        && reason.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')),
                "invalid_mig_unavailable_state"
            );
            return Ok(());
        }
        ensure!(
            self.parent_ordinal.is_some_and(|ordinal| ordinal < 32)
                && self.parent_uuid.as_ref().is_some_and(|id| uuid(id, "GPU-"))
                && self.mode.as_deref() == Some("enabled")
                && self.reason.is_none()
                && self.native_error.is_none(),
            "invalid_mig_parent_state"
        );
        let mut uuids = BTreeSet::new();
        let mut identities = BTreeSet::new();
        for instance in &self.instances {
            instance.validate()?;
            ensure!(
                uuids.insert(&instance.uuid)
                    && identities.insert((instance.gpu_instance_id, instance.compute_instance_id)),
                "duplicate_mig_instance"
            );
        }
        Ok(())
    }
}

/// Observe at most 64 native instances in a deadline-bounded, parent-owned helper.
pub fn discover(
    helper: &Path,
    helper_sha256: &str,
    state: &Path,
    ordinal: u32,
) -> Result<Topology> {
    discover_until(
        helper,
        helper_sha256,
        state,
        ordinal,
        Instant::now() + Duration::from_secs(10),
        &AtomicBool::new(false),
    )
}

pub(crate) fn discover_until(
    helper: &Path,
    helper_sha256: &str,
    state: &Path,
    ordinal: u32,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Topology> {
    if ordinal >= 32 {
        bail!("invalid_mig_parent_ordinal");
    }
    let selected = crate::reference::prepare(helper, helper_sha256, state)?;
    let value = crate::reference::invoke(
        &selected,
        state,
        &["mig-inventory".into(), ordinal.to_string()],
        deadline.min(Instant::now() + Duration::from_secs(10)),
        cancel,
        None,
        65536,
    )?;
    let mut result: Topology = serde_json::from_value(value)?;
    result.validate()?;
    result.instances.sort_by(|a, b| a.uuid.cmp(&b.uuid));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(state.join("mig.json"))?;
    file.write_all(&serde_json::to_vec(&result)?)?;
    file.sync_all()?;
    std::fs::File::open(state)?.sync_all()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topology() -> Topology {
        serde_json::from_value(serde_json::json!({
            "schema":"cohesix-nvml-mig-topology/v1", "source":"nvml",
            "availability":"observed", "parent_ordinal":0,
            "parent_uuid":"GPU-12345678-1234-1234-1234-123456789abc", "mode":"enabled",
            "instances":[{"uuid":"MIG-23456789-2345-2345-2345-23456789abcd",
                "gpu_instance_id":3, "compute_instance_id":0,
                "gpu_profile_id":19, "compute_profile_id":0, "memory_bytes":5_368_709_120_u64,
                "gpu_placement":{"start":2,"size":1},
                "compute_placement":{"start":0,"size":1}}]
        }))
        .unwrap()
    }

    #[test]
    fn mig_enrollment_fences_native_uuid_profile_placement_and_whole_generation() {
        let original = topology();
        let selected = Selection {
            parent_ordinal: 0,
            parent_uuid: original.parent_uuid.clone().unwrap(),
            instance: original.instances[0].clone(),
            topology_sha256: original.generation().unwrap(),
        };
        selected.require_observed(&original).unwrap();
        assert_eq!(
            selected.cuda_uuid().unwrap(),
            "2345678923452345234523456789abcd"
        );
        let mut changed = original.clone();
        changed.instances[0].gpu_profile_id += 1;
        assert!(selected.require_observed(&changed).is_err());
        changed = original.clone();
        changed.instances[0].gpu_placement.start += 1;
        assert!(selected.require_observed(&changed).is_err());
        changed = original.clone();
        changed.instances[0].uuid.replace_range(4..5, "3");
        assert!(selected.require_observed(&changed).is_err());
        changed = original.clone();
        let mut second = changed.instances[0].clone();
        second.uuid.replace_range(4..5, "4");
        second.gpu_instance_id = 4;
        changed.instances.push(second);
        changed.validate().unwrap();
        assert!(selected
            .require_observed(&changed)
            .unwrap_err()
            .to_string()
            .contains("stale_mig_generation"));
        let digest = changed.generation().unwrap();
        changed.instances.reverse();
        assert_eq!(digest, changed.generation().unwrap());
    }

    #[test]
    fn mig_unsupported_legacy_duplicate_and_impossible_placement_fail_closed() {
        let mut observed = topology();
        observed.instances.push(observed.instances[0].clone());
        assert!(observed.validate().is_err());
        observed = topology();
        observed.instances[0].uuid = "MIG-GPU-12345678/1/0".into();
        assert!(observed.validate().is_err());
        observed = topology();
        observed.instances[0].compute_placement.start = 1;
        assert!(observed.validate().is_err());
        observed = topology();
        observed.availability = "not_supported".into();
        observed.reason = Some("nvml_observation".into());
        assert!(observed.validate().is_err());
        observed.instances.clear();
        observed.parent_uuid = None;
        observed.parent_ordinal = None;
        observed.mode = None;
        observed.validate().unwrap();
        let original = topology();
        let selected = Selection {
            parent_ordinal: 0,
            parent_uuid: original.parent_uuid.clone().unwrap(),
            instance: original.instances[0].clone(),
            topology_sha256: original.generation().unwrap(),
        };
        assert!(selected.require_observed(&observed).is_err());
    }
}
