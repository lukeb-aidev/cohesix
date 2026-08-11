// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Emit compiler-owned root-TCB topology and exact object inventory evidence.
// Author: Lukas Bower

use crate::ir::Manifest;
use crate::resource_admission::KernelObjectBudget;
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// Schema consumed by the Milestone 26e root-TCB evidence promoter.
pub const GENERATED_ROOT_TCB_INVENTORY_SCHEMA: &str = "cohesix-root-tcb-generated-inventory/v1";
/// Stable filename emitted beside the selected resolved manifest.
pub const ROOT_TCB_TOPOLOGY_FILENAME: &str = "root_task_topology.json";

#[derive(Serialize)]
struct RootTcbTopologyProjection<'a> {
    profile: &'a crate::ir::Profile,
    root_task: &'a crate::ir::RootTaskSection,
    worker_runtime: &'a crate::ir::WorkerRuntimeConfig,
    temporal_authority: &'a crate::temporal::TemporalAuthorityConfig,
    worker_resource_admission: &'a crate::resource_admission::WorkerResourceAdmissionConfig,
    ninedoor_service: &'a crate::ir::NineDoorServiceConfig,
    console_network_service: &'a crate::ir::ConsoleNetworkServiceConfig,
}

#[derive(Serialize)]
struct GeneratedRootTcbInventory<'a> {
    schema: &'static str,
    profile: &'a str,
    manifest_sha256: &'a str,
    topology_sha256: String,
    topology: Value,
    inventory: KernelObjectBudget,
}

/// Return the topology artifact path derived from the resolved-manifest path.
#[must_use]
pub fn output_path(manifest_out: &Path) -> PathBuf {
    manifest_out
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(ROOT_TCB_TOPOLOGY_FILENAME)
}

/// Render the canonical compiler-owned topology and maximum admitted inventory.
pub fn render(manifest: &Manifest, manifest_hash: &str) -> Result<Vec<u8>> {
    let topology = serde_json::to_value(RootTcbTopologyProjection {
        profile: &manifest.profile,
        root_task: &manifest.root_task,
        worker_runtime: &manifest.worker_runtime,
        temporal_authority: &manifest.temporal_authority,
        worker_resource_admission: &manifest.worker_resource_admission,
        ninedoor_service: &manifest.ninedoor_service,
        console_network_service: &manifest.console_network_service,
    })
    .context("failed to serialize root-TCB topology projection")?;
    let topology_sha256 = super::hash_bytes(&canonical_json(&topology)?);
    let record = GeneratedRootTcbInventory {
        schema: GENERATED_ROOT_TCB_INVENTORY_SCHEMA,
        profile: &manifest.profile.name,
        manifest_sha256: manifest_hash,
        topology_sha256,
        topology,
        inventory: manifest.worker_resource_admission.maximum_inventory()?,
    };
    let mut rendered = serde_json::to_vec_pretty(&record)
        .context("failed to serialize generated root-TCB inventory")?;
    rendered.push(b'\n');
    Ok(rendered)
}

/// Emit the canonical topology beside the selected resolved manifest.
pub fn emit(manifest: &Manifest, manifest_hash: &str, manifest_out: &Path) -> Result<PathBuf> {
    let path = output_path(manifest_out);
    let rendered = render(manifest, manifest_hash)?;
    fs::write(&path, rendered)
        .with_context(|| format!("failed to write root-TCB topology {}", path.display()))?;
    Ok(path)
}

fn canonical_json(value: &Value) -> Result<Vec<u8>> {
    fn append(value: &Value, output: &mut Vec<u8>) -> Result<()> {
        match value {
            Value::Null => output.extend_from_slice(b"null"),
            Value::Bool(true) => output.extend_from_slice(b"true"),
            Value::Bool(false) => output.extend_from_slice(b"false"),
            Value::Number(number) => output.extend_from_slice(number.to_string().as_bytes()),
            Value::String(text) => output.extend_from_slice(
                &serde_json::to_vec(text).context("failed to canonicalize JSON string")?,
            ),
            Value::Array(items) => {
                output.push(b'[');
                for (index, item) in items.iter().enumerate() {
                    if index != 0 {
                        output.push(b',');
                    }
                    append(item, output)?;
                }
                output.push(b']');
            }
            Value::Object(fields) => {
                output.push(b'{');
                let mut names = fields.keys().collect::<Vec<_>>();
                names.sort_unstable();
                for (index, name) in names.into_iter().enumerate() {
                    if index != 0 {
                        output.push(b',');
                    }
                    output.extend_from_slice(
                        &serde_json::to_vec(name)
                            .context("failed to canonicalize JSON object name")?,
                    );
                    output.push(b':');
                    append(&fields[name], output)?;
                }
                output.push(b'}');
            }
        }
        Ok(())
    }

    let mut output = Vec::new();
    append(value, &mut output)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn qemu_manifest() -> Manifest {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("configs/root_task.toml");
        crate::ir::load_manifest(&path).expect("load QEMU manifest")
    }

    fn pi4_manifest() -> Manifest {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("configs/root_task_pi4_uboot_aarch64.toml");
        crate::ir::load_manifest(&path).expect("load Pi 4 manifest")
    }

    #[test]
    fn ninedoor_bootstrap_candidate_is_exactly_accounted() {
        for (manifest, fixed, maximum) in [(qemu_manifest(), 7, 10), (pi4_manifest(), 14, 17)] {
            assert_eq!(manifest.ninedoor_service.objects.scheduling_contexts, 1);
            assert_eq!(
                manifest.ninedoor_service.bootstrap_scheduling_context_bits,
                8
            );
            assert_eq!(manifest.ninedoor_service.bootstrap_budget_us, 3_000);
            assert_eq!(manifest.ninedoor_service.bootstrap_period_us, 10_000);
            assert_eq!(manifest.ninedoor_service.bootstrap_max_refills, 2);
            assert_eq!(
                manifest
                    .worker_resource_admission
                    .fixed_objects
                    .scheduling_contexts,
                fixed
            );
            assert_eq!(
                manifest
                    .worker_resource_admission
                    .maximum_inventory()
                    .expect("maximum inventory")
                    .scheduling_contexts,
                maximum
            );
        }
    }

    #[test]
    fn console_network_image_shrink_is_exactly_accounted() {
        for (
            manifest,
            fixed_frames,
            fixed_slots,
            maximum_frames,
            maximum_slots,
            admitted_frames,
            admitted_slots,
        ) in [
            (qemu_manifest(), 2_017, 4_056, 2_065, 4_248, 2_577, 6_296),
            (pi4_manifest(), 4_065, 8_960, 4_113, 9_152, 4_625, 11_200),
        ] {
            assert_eq!(manifest.console_network_service.stack_pages, 32);
            assert_eq!(manifest.console_network_service.objects.frames, 97);
            assert_eq!(manifest.console_network_service.objects.cspace_slots, 121);
            assert_eq!(
                manifest.worker_resource_admission.fixed_objects.frames,
                fixed_frames
            );
            assert_eq!(
                manifest
                    .worker_resource_admission
                    .fixed_objects
                    .cspace_slots,
                fixed_slots
            );
            let maximum = manifest
                .worker_resource_admission
                .maximum_inventory()
                .expect("maximum inventory");
            assert_eq!(maximum.frames, maximum_frames);
            assert_eq!(maximum.cspace_slots, maximum_slots);
            assert_eq!(
                maximum.frames
                    + manifest
                        .worker_resource_admission
                        .post_construction_reserve
                        .frames,
                admitted_frames
            );
            assert_eq!(
                maximum.cspace_slots
                    + manifest
                        .worker_resource_admission
                        .post_construction_reserve
                        .cspace_slots,
                admitted_slots
            );
        }
    }

    #[test]
    fn root_control_turn_candidate_is_exactly_accounted() {
        for manifest in [qemu_manifest(), pi4_manifest()] {
            let root_control = manifest
                .temporal_authority
                .tasks
                .iter()
                .find(|task| task.id == "root-control")
                .expect("root-control temporal task");
            assert_eq!(root_control.budget_us, 2_750);
            assert_eq!(root_control.period_us, 10_000);
            assert_eq!(root_control.wcet_us, 2_500);
            assert_eq!(root_control.response_time_us, 5_100);
            assert_eq!(
                root_control.wcet_provenance,
                "m26e-qemu-root-phase-candidate-v3"
            );

            let console_network = manifest
                .temporal_authority
                .tasks
                .iter()
                .find(|task| task.id == "console-network-service")
                .expect("console-network-service temporal task");
            assert_eq!(console_network.response_time_us, 7_500);

            let core_zero_demand = manifest
                .temporal_authority
                .tasks
                .iter()
                .filter(|task| {
                    task.core == 0 && task.execution == crate::temporal::TemporalExecution::Active
                })
                .map(|task| task.budget_us)
                .sum::<u32>();
            assert_eq!(core_zero_demand, 9_000);
            let core_zero_admission = manifest
                .temporal_authority
                .core_admission
                .iter()
                .find(|admission| admission.core == 0)
                .expect("core-0 temporal admission");
            assert_eq!(core_zero_admission.capacity_us, 10_000);
            assert_eq!(core_zero_admission.reserve_us, 1_000);
            assert_eq!(
                core_zero_demand,
                core_zero_admission.capacity_us - core_zero_admission.reserve_us
            );
        }
    }

    #[test]
    fn generated_inventory_is_derived_from_the_maximum_role_mix() {
        let manifest = qemu_manifest();
        let rendered = render(&manifest, &"a".repeat(64)).expect("render topology");
        let record: Value = serde_json::from_slice(&rendered).expect("parse topology");
        let record_fields = record
            .as_object()
            .expect("topology envelope")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            record_fields,
            [
                "inventory",
                "manifest_sha256",
                "profile",
                "schema",
                "topology",
                "topology_sha256",
            ]
        );
        assert_eq!(record["schema"], GENERATED_ROOT_TCB_INVENTORY_SCHEMA);
        assert_eq!(record["profile"], "virt-aarch64");
        assert_eq!(
            record["inventory"]
                .as_object()
                .expect("object inventory")
                .len(),
            14
        );
        assert_eq!(record["inventory"]["tcbs"], 10);
        assert_eq!(record["inventory"]["scheduling_contexts"], 10);
        assert_eq!(record["inventory"]["frames"], 2065);
        assert_eq!(record["inventory"]["endpoints"], 15);
        assert_eq!(record["inventory"]["reply_objects"], 6);
        assert_eq!(record["inventory"]["cspace_slots"], 4248);

        let canonical = canonical_json(&record["topology"]).expect("canonical topology");
        assert_eq!(
            record["topology_sha256"],
            hex::encode(Sha256::digest(canonical))
        );
    }

    #[test]
    fn compiler_topology_hash_detects_temporal_drift() {
        let manifest = qemu_manifest();
        let baseline: Value = serde_json::from_slice(
            &render(&manifest, &"b".repeat(64)).expect("render baseline topology"),
        )
        .expect("parse baseline topology");
        let mut changed = manifest;
        changed.temporal_authority.tasks[0].budget_us += 1;
        let drifted: Value = serde_json::from_slice(
            &render(&changed, &"b".repeat(64)).expect("render drifted topology"),
        )
        .expect("parse drifted topology");
        assert_ne!(baseline["topology_sha256"], drifted["topology_sha256"]);
    }
}
