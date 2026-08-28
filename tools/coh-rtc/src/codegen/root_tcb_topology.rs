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
        for (manifest, fixed, maximum) in [(qemu_manifest(), 9, 265), (pi4_manifest(), 16, 272)] {
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
    fn console_network_image_footprints_are_exactly_accounted() {
        for (
            manifest,
            fixed_frames,
            fixed_slots,
            maximum_frames,
            maximum_slots,
            admitted_frames,
            admitted_slots,
        ) in [
            (qemu_manifest(), 2_024, 4_378, 5_096, 12_570, 5_608, 14_618),
            (pi4_manifest(), 4_077, 9_267, 7_149, 17_459, 7_661, 19_507),
        ] {
            let qemu = manifest.profile.name == "virt-aarch64";
            assert_eq!(manifest.console_network_service.stack_pages, 32);
            assert_eq!(
                manifest.console_network_service.objects.frames,
                if qemu { 134 } else { 103 }
            );
            assert_eq!(
                manifest.console_network_service.objects.cspace_slots,
                if qemu { 162 } else { 160 }
            );
            assert_eq!(manifest.console_network_service.objects.fault_caps, 1);
            assert_eq!(
                manifest.console_network_service.objects.timeout_fault_caps,
                1
            );
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
            assert_eq!(
                manifest
                    .worker_resource_admission
                    .fixed_objects
                    .timeout_fault_caps,
                if qemu { 9 } else { 16 }
            );
            assert_eq!(maximum.timeout_fault_caps, if qemu { 265 } else { 272 });
            assert_eq!(
                maximum.timeout_fault_caps
                    + manifest
                        .worker_resource_admission
                        .post_construction_reserve
                        .timeout_fault_caps,
                if qemu { 273 } else { 280 }
            );
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
            let qemu = manifest.profile.name == "virt-aarch64";
            let root_control = manifest
                .temporal_authority
                .tasks
                .iter()
                .find(|task| task.id == "root-control")
                .expect("root-control temporal task");
            let (
                expected_root_budget,
                expected_root_wcet,
                expected_root_response,
                expected_serial_io_bound,
                expected_wcet_provenance,
                expected_console_core,
                expected_console_priority,
                expected_console_mcp,
                expected_console_response,
                expected_core_zero_demand,
                expected_core_two_demand,
                expected_timer_clock_hz,
            ) = if qemu {
                (
                    9_000,
                    8_500,
                    8_500,
                    64,
                    "m26e-qemu-root-dedicated-core-bounded-quantum-v1",
                    2,
                    180,
                    200,
                    3_000,
                    9_000,
                    8_000,
                    24_000_000,
                )
            } else {
                (
                    2_750,
                    2_500,
                    5_100,
                    0,
                    "m26e-pi4-root-adjacent-refill-natural-postpone-candidate-v24",
                    0,
                    180,
                    200,
                    8_100,
                    9_000,
                    7_400,
                    54_000_000,
                )
            };
            assert_eq!(root_control.core, 0);
            assert_eq!(root_control.sched_control_core, 0);
            assert_eq!(root_control.budget_us, expected_root_budget);
            assert_eq!(root_control.period_us, 10_000);
            assert_eq!(root_control.max_refills, 2);
            assert_eq!(
                root_control.timeout_policy,
                crate::temporal::TimeoutPolicy::NaturalPostpone
            );
            assert_eq!(root_control.wcet_us, expected_root_wcet);
            assert_eq!(root_control.response_time_us, expected_root_response);
            assert_eq!(
                root_control.virtio_operator_serial_io_bytes_per_turn,
                expected_serial_io_bound
            );
            assert_eq!(root_control.wcet_provenance, expected_wcet_provenance);
            assert_eq!(
                manifest.console_network_service.timer_clock_hz,
                expected_timer_clock_hz
            );

            assert!(manifest.temporal_authority.tasks.iter().all(|task| {
                task.id == "root-control" || task.virtio_operator_serial_io_bytes_per_turn == 0
            }));

            let console_network = manifest
                .temporal_authority
                .tasks
                .iter()
                .find(|task| task.id == "console-network-service")
                .expect("console-network-service temporal task");
            assert_eq!(console_network.core, expected_console_core);
            assert_eq!(console_network.sched_control_core, expected_console_core);
            assert_eq!(console_network.priority, expected_console_priority);
            assert_eq!(console_network.mcp, expected_console_mcp);
            assert_eq!(manifest.console_network_service.core, expected_console_core);
            assert_eq!(
                manifest.console_network_service.priority,
                expected_console_priority
            );
            assert_eq!(manifest.console_network_service.mcp, expected_console_mcp);
            assert_eq!(manifest.console_network_service.abi_version, 5);
            assert_eq!(console_network.budget_us, 3_000);
            assert_eq!(console_network.period_us, 10_000);
            assert_eq!(console_network.wcet_us, 3_000);
            assert_eq!(console_network.response_time_us, expected_console_response);
            assert_eq!(
                console_network.timeout_policy,
                crate::temporal::TimeoutPolicy::NaturalPostpone
            );
            assert_eq!(
                console_network.wcet_provenance,
                "m26e-qemu-console-received-progress-retention-candidate-v18"
            );

            let worker_expectations = if qemu {
                [
                    ("root-worker-executor-gpu", 2, 7_500),
                    ("root-worker-executor-lora", 3, 7_200),
                ]
            } else {
                [
                    ("root-worker-executor-gpu", 2, 7_100),
                    ("root-worker-executor-lora", 3, 7_400),
                ]
            };
            for (worker_id, expected_core, expected_response) in worker_expectations {
                let worker = manifest
                    .temporal_authority
                    .tasks
                    .iter()
                    .find(|task| task.id == worker_id)
                    .expect("Worker temporal task");
                assert_eq!(worker.core, expected_core);
                assert_eq!(worker.sched_control_core, expected_core);
                assert_eq!(worker.response_time_us, expected_response);
            }

            let core_zero_demand = manifest
                .temporal_authority
                .tasks
                .iter()
                .filter(|task| {
                    task.core == 0 && task.execution == crate::temporal::TemporalExecution::Active
                })
                .map(|task| task.budget_us)
                .sum::<u32>();
            assert_eq!(core_zero_demand, expected_core_zero_demand);
            let core_zero_admission = manifest
                .temporal_authority
                .core_admission
                .iter()
                .find(|admission| admission.core == 0)
                .expect("core-0 temporal admission");
            assert_eq!(
                core_zero_admission.capacity_us,
                if qemu { 20_000 } else { 10_000 }
            );
            assert_eq!(
                core_zero_admission.reserve_us,
                if qemu { 2_000 } else { 1_000 }
            );
            let core_zero_usable = core_zero_admission.capacity_us - core_zero_admission.reserve_us;
            assert!(core_zero_demand <= core_zero_usable);
            assert_eq!(
                core_zero_usable - core_zero_demand,
                if qemu { 9_000 } else { 0 }
            );

            let core_two_demand = manifest
                .temporal_authority
                .tasks
                .iter()
                .filter(|task| {
                    task.core == 2 && task.execution == crate::temporal::TemporalExecution::Active
                })
                .map(|task| task.budget_us)
                .sum::<u32>();
            assert_eq!(core_two_demand, expected_core_two_demand);
            let core_two_admission = manifest
                .temporal_authority
                .core_admission
                .iter()
                .find(|admission| admission.core == 2)
                .expect("core-2 temporal admission");
            assert_eq!(
                core_two_admission.capacity_us,
                if qemu { 20_000 } else { 10_000 }
            );
            assert_eq!(
                core_two_admission.reserve_us,
                if qemu { 2_000 } else { 1_000 }
            );
            assert!(
                core_two_demand <= core_two_admission.capacity_us - core_two_admission.reserve_us
            );
        }
    }

    #[test]
    fn pi4_console_priority_and_response_contract_fails_closed_on_temporal_drift() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

        let mut invalid_mcp = pi4_manifest();
        invalid_mcp
            .temporal_authority
            .tasks
            .iter_mut()
            .find(|task| task.id == "console-network-service")
            .expect("console-network temporal task")
            .mcp = 179;
        let error = invalid_mcp
            .validate_with_base(Some(repo_root.as_path()))
            .expect_err("priority 180 above MCP 179 must fail closed");
        assert!(
            error.to_string().contains("priority 180 exceeds MCP 179"),
            "unexpected error: {error}"
        );

        let mut service_drift = pi4_manifest();
        service_drift.console_network_service.priority = 200;
        let error = service_drift
            .validate_with_base(Some(repo_root.as_path()))
            .expect_err("duplicate service priority drift must fail closed");
        assert!(
            error
                .to_string()
                .contains("object/SC inventory disagrees with temporal task"),
            "unexpected error: {error}"
        );

        for (task_id, stale_response) in
            [("root-control", 8_100), ("console-network-service", 5_600)]
        {
            let mut response_drift = pi4_manifest();
            response_drift
                .temporal_authority
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap_or_else(|| panic!("temporal task {task_id}"))
                .response_time_us = stale_response;
            let error = response_drift
                .validate_with_base(Some(repo_root.as_path()))
                .expect_err("stale response bound must fail closed");
            assert!(
                error.to_string().contains("response-time result mismatch"),
                "unexpected error for {task_id}: {error}"
            );
        }
    }

    #[test]
    fn root_fault_turn_candidate_is_exactly_accounted() {
        for manifest in [qemu_manifest(), pi4_manifest()] {
            let root_fault = manifest
                .temporal_authority
                .tasks
                .iter()
                .find(|task| task.id == "root-fault")
                .expect("root-fault temporal task");
            assert_eq!(root_fault.budget_us, 3_000);
            assert_eq!(root_fault.period_us, 10_000);
            assert_eq!(root_fault.wcet_us, 2_400);
            assert_eq!(
                root_fault.response_time_us,
                if manifest.profile.name == "virt-aarch64" {
                    2_400
                } else {
                    2_600
                }
            );
            assert_eq!(
                root_fault.wcet_provenance,
                "m26e-qemu-root-fault-service-units-candidate-v6"
            );
        }
    }

    #[test]
    fn supervisor_cold_activation_candidates_are_profile_scoped() {
        let qemu = qemu_manifest();
        let qemu_worker = qemu
            .temporal_authority
            .tasks
            .iter()
            .find(|task| task.id == "root-worker-supervisor")
            .expect("QEMU root-worker-supervisor temporal task");
        assert_eq!(qemu_worker.budget_us, 3_000);
        assert_eq!(qemu_worker.period_us, 10_000);
        assert_eq!(qemu_worker.wcet_us, 2_400);
        assert_eq!(qemu_worker.response_time_us, 7_200);
        assert_eq!(
            qemu_worker.wcet_provenance,
            "m26e-qemu-root-worker-supervisor-cold-activation-candidate-v15"
        );

        let qemu_driver = qemu
            .temporal_authority
            .tasks
            .iter()
            .find(|task| task.id == "root-driver-supervisor")
            .expect("QEMU root-driver-supervisor temporal task");
        assert_eq!(qemu_driver.budget_us, 3_000);
        assert_eq!(qemu_driver.period_us, 10_000);
        assert_eq!(qemu_driver.wcet_us, 2_400);
        assert_eq!(qemu_driver.response_time_us, 4_800);
        assert_eq!(
            qemu_driver.wcet_provenance,
            "m26e-qemu-root-driver-supervisor-cold-activation-candidate-v15"
        );

        let qemu_supervisor_demand = qemu
            .temporal_authority
            .tasks
            .iter()
            .filter(|task| {
                matches!(
                    task.id.as_str(),
                    "root-worker-supervisor" | "root-driver-supervisor"
                )
            })
            .map(|task| task.budget_us)
            .sum::<u32>();
        assert_eq!(qemu_supervisor_demand, 6_000);
        assert_eq!(qemu.ninedoor_service.bootstrap_budget_us, 3_000);
        assert_eq!(qemu.ninedoor_service.bootstrap_period_us, 10_000);
        assert_eq!(qemu.ninedoor_service.priority, 128);
        assert!(qemu.ninedoor_service.priority < qemu_worker.priority);
        assert!(qemu.ninedoor_service.priority < qemu_driver.priority);
        let qemu_core_one_admission = qemu
            .temporal_authority
            .core_admission
            .iter()
            .find(|admission| admission.core == 1)
            .expect("QEMU core-1 temporal admission");
        assert_eq!(qemu_core_one_admission.capacity_us, 20_000);
        assert_eq!(qemu_core_one_admission.reserve_us, 2_000);
        assert_eq!(
            qemu_core_one_admission.capacity_us
                - qemu_core_one_admission.reserve_us
                - qemu_supervisor_demand,
            12_000
        );

        let pi4 = pi4_manifest();
        let pi4_worker = pi4
            .temporal_authority
            .tasks
            .iter()
            .find(|task| task.id == "root-worker-supervisor")
            .expect("Pi root-worker-supervisor temporal task");
        assert_eq!(pi4_worker.budget_us, 750);
        assert_eq!(pi4_worker.period_us, 10_000);
        assert_eq!(pi4_worker.wcet_us, 600);
        assert_eq!(pi4_worker.response_time_us, 1_400);
        assert_eq!(pi4_worker.wcet_provenance, "m26e-qemu-candidate-v1");

        let pi4_driver = pi4
            .temporal_authority
            .tasks
            .iter()
            .find(|task| task.id == "root-driver-supervisor")
            .expect("Pi root-driver-supervisor temporal task");
        assert_eq!(pi4_driver.budget_us, 1_000);
        assert_eq!(pi4_driver.period_us, 10_000);
        assert_eq!(pi4_driver.wcet_us, 800);
        assert_eq!(pi4_driver.response_time_us, 800);
        assert_eq!(pi4_driver.wcet_provenance, "m26e-qemu-candidate-v1");
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
        assert_eq!(record["inventory"]["tcbs"], 265);
        assert_eq!(record["inventory"]["scheduling_contexts"], 265);
        assert_eq!(record["inventory"]["frames"], 5096);
        assert_eq!(record["inventory"]["endpoints"], 271);
        assert_eq!(record["inventory"]["reply_objects"], 264);
        assert_eq!(record["inventory"]["cspace_slots"], 12570);

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
