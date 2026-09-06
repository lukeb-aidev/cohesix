// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate Pi 4 U-Boot profile codegen against the active Milestone 26e contract.
// Author: Lukas Bower

use coh_rtc::{compile, CompileOptions};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("coh-rtc has tools parent")
        .parent()
        .expect("workspace root has parent")
        .join(path)
}

fn compile_options(manifest_path: PathBuf, temp_dir: &TempDir) -> CompileOptions {
    CompileOptions {
        manifest_path,
        out_dir: temp_dir.path().join("generated"),
        manifest_out: temp_dir.path().join("root_task_resolved.json"),
        cas_manifest_template_out: temp_dir.path().join("cas_manifest_template.json"),
        cli_script_out: temp_dir.path().join("boot_v0.coh"),
        doc_snippet_out: temp_dir.path().join("snippet.md"),
        gpu_breadcrumbs_snippet_out: temp_dir.path().join("gpu_breadcrumbs.md"),
        observability_interfaces_snippet_out: temp_dir.path().join("observability_interfaces.md"),
        observability_security_snippet_out: temp_dir.path().join("observability_security.md"),
        ticket_quotas_snippet_out: temp_dir.path().join("ticket_quotas.md"),
        trace_policy_snippet_out: temp_dir.path().join("trace_policy.md"),
        cas_interfaces_snippet_out: temp_dir.path().join("cas_interfaces.md"),
        cas_security_snippet_out: temp_dir.path().join("cas_security.md"),
        cbor_snippet_out: temp_dir.path().join("telemetry_cbor.md"),
        cohesix_py_defaults_out: temp_dir.path().join("cohesix_py_defaults.py"),
        cohesix_py_doc_out: temp_dir.path().join("cohesix_py_defaults.md"),
        coh_doctor_doc_out: temp_dir.path().join("coh_doctor_checks.md"),
        cohsh_policy_out: temp_dir.path().join("cohsh_policy.toml"),
        cohsh_policy_rust_out: temp_dir.path().join("cohsh_policy.rs"),
        cohsh_policy_doc_out: temp_dir.path().join("cohsh_policy.md"),
        cohsh_client_rust_out: temp_dir.path().join("cohsh_client.rs"),
        cohsh_client_doc_out: temp_dir.path().join("cohsh_client.md"),
        cohsh_grammar_doc_out: temp_dir.path().join("cohsh_grammar.md"),
        cohsh_ticket_policy_doc_out: temp_dir.path().join("cohsh_ticket_policy.md"),
        coh_policy_out: temp_dir.path().join("coh_policy.toml"),
        coh_policy_rust_out: temp_dir.path().join("coh_policy.rs"),
        coh_policy_doc_out: temp_dir.path().join("coh_policy.md"),
        swarmui_defaults_out: temp_dir.path().join("swarmui_defaults.toml"),
        swarmui_defaults_rust_out: temp_dir.path().join("swarmui_defaults.rs"),
        swarmui_defaults_doc_out: temp_dir.path().join("swarmui_defaults.md"),
    }
}

#[test]
fn pi4_refill_capacity_is_bound_to_the_tracked_sel4_16_build_identity() {
    const KERNEL_ID: &str = "sel4-16.0.0-aarch64-smp-mcs";
    const KERNEL_COMMIT: &str = "6e7c3b733d296cfd88d5fbf635c96e447a882374";

    let manifest_path = repo_path("configs/root_task_pi4_uboot_aarch64.toml");
    let mut manifest = coh_rtc::ir::load_manifest(&manifest_path).expect("load Pi manifest");
    assert_eq!(
        manifest.worker_resource_admission.selected_kernel,
        KERNEL_ID
    );

    let profiles: toml::Value = toml::from_str(
        &fs::read_to_string(repo_path("configs/sel4/profiles.toml"))
            .expect("read seL4 profile contract"),
    )
    .expect("parse seL4 profile contract");
    assert_eq!(
        profiles["source"]["manifest_ref"].as_str(),
        Some("refs/tags/16.0.0")
    );
    assert_eq!(
        profiles["source"]["repositories"]["kernel"].as_str(),
        Some(KERNEL_COMMIT)
    );
    for profile_id in ["pi4_production", "pi4_diagnostic"] {
        let profile = &profiles["profiles"][profile_id];
        assert_eq!(profile["target"].as_str(), Some("pi4"));
        assert_eq!(profile["cmake"]["KernelSel4Arch"].as_str(), Some("aarch64"));
        assert_eq!(profile["cmake"]["KernelIsMCS"].as_str(), Some("ON"));
        assert_eq!(profile["cmake"]["MCS"].as_str(), Some("ON"));
        assert_eq!(profile["cmake"]["SMP"].as_str(), Some("ON"));
    }

    let build_stamp: Value = serde_json::from_slice(
        &fs::read(repo_path(
            "seL4/build_UBOOT/cohesix-profile-build-inputs.json",
        ))
        .expect("read selected Pi build stamp"),
    )
    .expect("parse selected Pi build stamp");
    assert_eq!(build_stamp["profile"], "pi4_diagnostic");
    assert_eq!(
        build_stamp["source"]["repositories"]["kernel"]["expected_commit"],
        KERNEL_COMMIT
    );
    assert_eq!(
        build_stamp["source"]["repositories"]["kernel"]["actual_commit"],
        KERNEL_COMMIT
    );

    let cyw43 = manifest
        .temporal_authority
        .tasks
        .iter_mut()
        .find(|task| task.id == "driver-cyw43")
        .expect("driver-cyw43 temporal task");
    assert_eq!(cyw43.scheduling_context_bits, 8);
    assert_eq!(cyw43.max_refills, 8);
    cyw43.max_refills = 11;
    let error = manifest
        .validate_with_base(manifest_path.parent())
        .expect_err("selected eight-bit Pi SC cannot hold eleven refills");
    assert_eq!(
        error.to_string(),
        "active temporal task driver-cyw43 max_refills 11 exceeds selected kernel sel4-16.0.0-aarch64-smp-mcs SC bits 8 capacity 10"
    );
}

#[test]
fn pi4_uboot_profile_emits_network_policy() {
    let temp_dir = TempDir::new().expect("tempdir");
    let options = compile_options(
        repo_path("configs/root_task_pi4_uboot_aarch64.toml"),
        &temp_dir,
    );
    compile(&options).expect("compile pi4 profile");

    let generated_module = fs::read_to_string(temp_dir.path().join("generated/mod.rs"))
        .expect("read generated module");
    let generated_bootstrap = fs::read_to_string(temp_dir.path().join("generated/bootstrap.rs"))
        .expect("read generated bootstrap");
    assert!(generated_module.contains("pub direct_genet: bool"));
    assert!(generated_bootstrap.contains("direct_virtio: false,"));
    assert!(generated_bootstrap.contains("direct_genet: true,"));

    let manifest: Value = serde_json::from_slice(
        &fs::read(temp_dir.path().join("root_task_resolved.json")).expect("read manifest"),
    )
    .expect("parse manifest");
    let network = &manifest["hw"]["network"];

    assert_eq!(manifest["profile"]["name"], "pi4-uboot-aarch64");
    assert_eq!(network["backend"], "bcmgenet-v5");
    assert_eq!(network["mode"], "dhcp");
    assert_eq!(network["interface"], "auto");
    assert_eq!(network["dhcp"]["discover_timeout_ms"], 1500);
    assert_eq!(network["dhcp"]["request_timeout_ms"], 1500);
    assert_eq!(network["dhcp"]["max_retries"], 6);
    let local_seat = &manifest["hw"]["local_seat"];
    assert_eq!(local_seat["enabled"], true);
    assert_eq!(local_seat["required"], true);
    assert_eq!(local_seat["keyboard_device"], "usb-kbd0");
    assert_eq!(local_seat["display_device"], "hdmi0");
    assert_eq!(local_seat["line_bytes"], 160);
    assert_eq!(local_seat["buffer_lines"], 128);
    assert_eq!(
        manifest["root_task"]["affinity"]["drivers"]["bcmgenet-v5"],
        1
    );
    assert_eq!(manifest["root_task"]["affinity"]["drivers"]["cyw43455"], 3);

    let temporal_tasks = manifest["temporal_authority"]["tasks"]
        .as_array()
        .expect("temporal tasks");
    let worker_classes = manifest["temporal_authority"]["worker_classes"]
        .as_array()
        .expect("worker classes");
    for (role, slots) in [
        ("worker-heartbeat", 1),
        ("worker-gpu", 127),
        ("worker-lora", 128),
    ] {
        let class = worker_classes
            .iter()
            .find(|class| class["role"] == role)
            .unwrap_or_else(|| panic!("worker class {role}"));
        assert_eq!(class["slots"], slots);
    }

    let admission = &manifest["worker_resource_admission"];
    assert_eq!(admission["capacity"]["cspace_slots"], 65_536);
    assert_eq!(admission["handoff"]["worker_control_queue_capacity"], 256);
    assert_eq!(admission["handoff"]["worker_fault_mailboxes"], 256);
    assert_eq!(admission["fault_registry"]["worker_tcbs"], 256);
    assert_eq!(admission["fault_registry"]["driver_tcbs"], 7);
    assert_eq!(admission["fault_registry"]["capacity"], 272);
    let executable_roles = admission["executable_roles"]
        .as_array()
        .expect("executable roles");
    for (role, slots) in [
        ("worker-heartbeat", 1),
        ("worker-gpu", 127),
        ("worker-lora", 128),
    ] {
        let executable = executable_roles
            .iter()
            .find(|entry| entry["role"] == role)
            .unwrap_or_else(|| panic!("executable role {role}"));
        assert_eq!(executable["namespace_capacity"], 256);
        assert_eq!(executable["executable_slots"], slots);
    }
    let admitted = |resource: &str| {
        admission["fixed_objects"][resource]
            .as_u64()
            .unwrap_or_else(|| panic!("fixed {resource}"))
            + executable_roles
                .iter()
                .map(|role| {
                    role["executable_slots"].as_u64().expect("executable slots")
                        * role["per_slot"][resource]
                            .as_u64()
                            .unwrap_or_else(|| panic!("per-slot {resource}"))
                })
                .sum::<u64>()
            + admission["post_construction_reserve"][resource]
                .as_u64()
                .unwrap_or_else(|| panic!("reserved {resource}"))
    };
    for (resource, used, capacity, headroom) in [
        ("tcbs", 280, 512, 232),
        ("cnodes", 280, 512, 232),
        ("vspaces", 280, 512, 232),
        ("page_tables", 2_640, 4_096, 1_456),
        ("asids", 280, 512, 232),
        ("frames", 7_663, 8_192, 529),
        ("endpoints", 303, 512, 209),
        ("notifications", 50, 128, 78),
        ("fault_caps", 280, 512, 232),
        ("timeout_fault_caps", 280, 512, 232),
        ("reply_objects", 279, 512, 233),
        ("scheduling_contexts", 280, 512, 232),
        ("cspace_slots", 19_513, 65_536, 46_023),
        ("untyped_bytes", 167_772_160, 268_435_456, 100_663_296),
    ] {
        assert_eq!(admitted(resource), used, "admitted {resource}");
        assert_eq!(
            admission["capacity"][resource]
                .as_u64()
                .unwrap_or_else(|| panic!("capacity {resource}")),
            capacity,
            "capacity {resource}"
        );
        assert_eq!(capacity - used, headroom, "headroom {resource}");
    }

    for task_id in [
        "driver-serial",
        "driver-usb",
        "driver-hdmi",
        "driver-genet",
        "driver-cyw43",
        "driver-sdio",
    ] {
        assert_eq!(
            temporal_timeout_policy(temporal_tasks, task_id),
            "natural-postpone",
            "resumable Pi driver {task_id} must cross ordinary MCS refill boundaries without a false terminal fault"
        );
    }
    assert_eq!(
        temporal_timeout_policy(temporal_tasks, "driver-pcie"),
        "terminal",
        "terminal Pi driver driver-pcie must retain its existing timeout policy"
    );
    let pcie = temporal_tasks
        .iter()
        .find(|task| task["id"] == "driver-pcie")
        .expect("selected Pi profile declares the sole PCIe/timer owner");
    // Two 5-ms timer duties in a 10-ms period need a head plus two
    // unexpired usage fragments. The fourth slot covers enable/call carry-in;
    // this changes refill storage, not admitted CPU time or fault policy.
    assert_eq!(pcie["max_refills"], 4);
    assert_eq!(pcie["scheduling_context_bits"], 8);
    assert_eq!(pcie["budget_us"], 400);
    assert_eq!(pcie["period_us"], 10_000);
    assert_eq!(pcie["priority"], 112);
    for task_id in ["driver-cyw43", "driver-sdio"] {
        let task = temporal_tasks
            .iter()
            .find(|task| task["id"] == task_id)
            .unwrap_or_else(|| panic!("temporal task {task_id}"));
        assert_eq!(task["scheduling_context_bits"], 8);
        assert_eq!(task["max_refills"], 8);
        assert_eq!(task["budget_us"], 1_500);
        assert_eq!(task["period_us"], 10_000);
        assert_eq!(task["priority"], 184);
    }

    let temporal_task = |task_id: &str| {
        temporal_tasks
            .iter()
            .find(|task| task["id"] == task_id)
            .unwrap_or_else(|| panic!("temporal task {task_id}"))
    };
    let root = temporal_task("root-control");
    assert_eq!(root["scheduling_context_bits"], 8);
    assert_eq!(root["max_refills"], 8);
    let root_fault = temporal_task("root-fault");
    let console = temporal_task("console-network-service");
    let console_objects = &manifest["console_network_service"]["objects"];
    assert_eq!(root["core"], 0);
    assert_eq!(root["priority"], 200);
    assert_eq!(root["mcp"], 200);
    assert_eq!(root["budget_us"], 5_500);
    assert_eq!(root["period_us"], 10_000);
    assert_eq!(root["wcet_us"], 2_500);
    assert_eq!(root["response_time_us"], 5_100);
    assert_eq!(
        root["wcet_provenance"],
        "m26e-pi4-root-fragmented-refill-candidate-v28"
    );
    assert_eq!(root_fault["core"], 0);
    assert_eq!(root_fault["sched_control_core"], 0);
    assert_eq!(root_fault["response_time_us"], 2_600);
    assert_eq!(console["core"], 2);
    assert_eq!(console["sched_control_core"], 2);
    assert_eq!(console["priority"], 200);
    assert_eq!(console["mcp"], 200);
    assert_eq!(console["priority"], root["priority"]);
    assert!(
        root["mcp"].as_u64().expect("root MCP")
            >= console["priority"].as_u64().expect("console priority")
    );
    assert_eq!(console["budget_us"], 3_000);
    assert_eq!(console["period_us"], 10_000);
    assert_eq!(console["max_refills"], 8);
    assert_eq!(console["wcet_us"], 3_000);
    assert_eq!(console["response_time_us"], 3_000);
    assert_eq!(
        console["wcet_provenance"],
        "m26e-pi4-console-cross-core-causal-publication-candidate-v21"
    );
    assert_eq!(manifest["console_network_service"]["abi_version"], 6);
    assert_eq!(manifest["console_network_service"]["priority"], 200);
    assert_eq!(manifest["console_network_service"]["mcp"], 200);
    assert_eq!(manifest["console_network_service"]["max_refills"], 8);
    assert_eq!(manifest["console_network_service"]["core"], 2);
    assert_eq!(console_objects["frames"], 104);
    assert_eq!(console_objects["cspace_slots"], 161);
    assert_eq!(admission["fixed_objects"]["frames"], 4_079);
    assert_eq!(admission["fixed_objects"]["cspace_slots"], 9_273);
    let genet = temporal_task("driver-genet");
    assert_eq!(genet["kind"], "driver");
    assert_eq!(genet["execution"], "active");
    assert_eq!(genet["core"], 1);
    assert_eq!(genet["sched_control_core"], 1);
    assert_eq!(genet["budget_us"], 3_000);
    assert_eq!(genet["period_us"], 10_000);
    assert_eq!(genet["max_refills"], 8);
    assert_eq!(genet["consumed_time_evidence"], true);
    assert_eq!(genet["timeout_policy"], "natural-postpone");
    assert_eq!(genet["priority"], 160);
    assert_eq!(genet["wcet_us"], 800);
    assert_eq!(genet["response_time_us"], 3_400);
    let core_one_demand: u64 = temporal_tasks
        .iter()
        .filter(|task| task["core"] == 1)
        .map(|task| task["budget_us"].as_u64().expect("core-1 budget"))
        .sum();
    let core_three_demand: u64 = temporal_tasks
        .iter()
        .filter(|task| task["core"] == 3)
        .map(|task| task["budget_us"].as_u64().expect("core-3 budget"))
        .sum();
    assert_eq!(core_one_demand, 8_250);
    assert_eq!(core_three_demand, 8_000);
    let core_zero_demand: u64 = temporal_tasks
        .iter()
        .filter(|task| task["core"] == 0)
        .map(|task| task["budget_us"].as_u64().expect("core-0 budget"))
        .sum();
    let core_zero_admission = manifest["temporal_authority"]["core_admission"]
        .as_array()
        .expect("core admission")
        .iter()
        .find(|entry| entry["core"] == 0)
        .expect("core-0 admission");
    assert_eq!(core_zero_demand, 8_750);
    assert_eq!(
        core_zero_admission["capacity_us"]
            .as_u64()
            .expect("core-0 capacity")
            - core_zero_admission["reserve_us"]
                .as_u64()
                .expect("core-0 reserve"),
        9_000
    );
    let hdmi = temporal_task("driver-hdmi");
    let gpu_executor = temporal_task("root-worker-executor-gpu");
    assert_eq!(hdmi["budget_us"], 2_000);
    assert_eq!(hdmi["period_us"], 10_000);
    assert_eq!(hdmi["wcet_us"], 1_800);
    assert_eq!(hdmi["core"], 1);
    assert_eq!(hdmi["sched_control_core"], 1);
    assert_eq!(hdmi["response_time_us"], 5_200);
    assert_eq!(
        hdmi["wcet_provenance"],
        "m26e-pi4-hdmi-write-only-candidate-v1"
    );
    assert_eq!(gpu_executor["budget_us"], 5_000);
    assert_eq!(gpu_executor["response_time_us"], 8_300);
    let pcie = temporal_task("driver-pcie");
    assert_eq!(pcie["budget_us"], 400);
    assert_eq!(pcie["period_us"], 10_000);
    assert_eq!(pcie["wcet_us"], 300);
    assert_eq!(pcie["priority"], 112);
    assert_eq!(pcie["core"], 2);
    assert_eq!(pcie["sched_control_core"], 2);
    assert_eq!(pcie["response_time_us"], 3_300);
    let core_two_demand: u64 = temporal_tasks
        .iter()
        .filter(|task| task["core"] == 2)
        .map(|task| task["budget_us"].as_u64().expect("core-2 budget"))
        .sum();
    let core_two_admission = manifest["temporal_authority"]["core_admission"]
        .as_array()
        .expect("core admission")
        .iter()
        .find(|entry| entry["core"] == 2)
        .expect("core-2 admission");
    assert_eq!(core_two_demand, 8_400);
    assert_eq!(
        core_two_admission["capacity_us"]
            .as_u64()
            .expect("core-2 capacity")
            - core_two_admission["reserve_us"]
                .as_u64()
                .expect("core-2 reserve"),
        9_000
    );

    let devices = manifest["hw"]["devices"].as_array().expect("devices array");
    assert!(devices
        .iter()
        .any(|device| device["kind"] == "wifi" && device["id"] == "cyw43xx0"));
    assert!(device_required(devices, "keyboard", "usb-kbd0"));
    assert!(device_required(devices, "display", "hdmi0"));

    let images = manifest["root_task"]["driver_images"]["images"]
        .as_array()
        .expect("driver runtime images");
    assert_eq!(images.len(), 7);
    for image in images {
        assert_eq!(
            image["code-pages"].as_u64().expect("code pages"),
            320,
            "runtime image {} must cover the measured multi-segment linked ELF",
            image["id"]
        );
    }
    let irqs = manifest["root_task"]["driver_images"]["irqs"]
        .as_array()
        .expect("driver runtime IRQ topology");
    assert_eq!(irqs.len(), 5);
    let serial_irq = irqs
        .iter()
        .find(|irq| irq["hot-path"] == "serial-console")
        .expect("serial-console IRQ topology");
    assert_eq!(serial_irq["irq"], 125);
    assert_eq!(serial_irq["badge"], 126);
    assert_eq!(serial_irq["handler-slot"], 4);
    assert_eq!(serial_irq["notification-slot"], 3);
    assert_eq!(serial_irq["trigger"], "level");
    let genet_irq = irqs
        .iter()
        .find(|irq| irq["hot-path"] == "genet-nic")
        .expect("GENET default-queue IRQ topology");
    assert_eq!(genet_irq["irq"], 189);
    assert_eq!(genet_irq["badge"], 1024);
    assert_eq!(genet_irq["handler-slot"], 4);
    assert_eq!(genet_irq["notification-slot"], 3);
    assert_eq!(genet_irq["trigger"], "level");
    let sdio_irq = irqs
        .iter()
        .find(|irq| irq["hot-path"] == "sdio-host" && irq["irq"] == 158)
        .expect("SDIO IRQ topology");
    assert_eq!(sdio_irq["irq"], 158);
    assert_eq!(sdio_irq["badge"], 159);
    assert_eq!(sdio_irq["handler-slot"], 4);
    assert_eq!(sdio_irq["notification-slot"], 3);
    assert_eq!(sdio_irq["trigger"], "level");
    let sdio_dma_irq = irqs
        .iter()
        .find(|irq| irq["hot-path"] == "sdio-host" && irq["irq"] == 116)
        .expect("SDIO DMA IRQ topology");
    assert_eq!(sdio_dma_irq["irq"], 32 + 0x50 + 4);
    assert_eq!(sdio_dma_irq["badge"], 512);
    assert_eq!(sdio_dma_irq["handler-slot"], 5);
    assert_eq!(sdio_dma_irq["notification-slot"], 3);
    assert_eq!(sdio_dma_irq["trigger"], "level");
    let pcie_timer_irq = irqs
        .iter()
        .find(|irq| irq["hot-path"] == "pcie-root")
        .expect("PCIe-owned system-timer IRQ topology");
    assert_eq!(pcie_timer_irq["irq"], 99);
    assert_eq!(pcie_timer_irq["badge"], 2048);
    assert_eq!(pcie_timer_irq["handler-slot"], 4);
    assert_eq!(pcie_timer_irq["notification-slot"], 3);
    assert_eq!(pcie_timer_irq["trigger"], "level");
    assert_eq!(irqs[0]["irq"], 125);
    assert_eq!(irqs[1]["irq"], 189);
    assert_eq!(irqs[2]["irq"], 158);
    assert_eq!(irqs[3]["irq"], 116);
    assert_eq!(irqs[4]["irq"], 99);

    // Bind the already-translated manifest IRQ identity to the exact selected
    // Pi kernel profile. The first DTS cell is the default-queue/misc line;
    // the second is the unused priority-queue line. Runtime code must not
    // reconstruct either seL4 IRQ by applying a hard-coded GIC offset.
    let selected_dts = fs::read_to_string(repo_path("seL4/build_UBOOT/kernel/kernel.dts"))
        .expect("selected Pi kernel DTS");
    let normalized_dts = selected_dts
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let genet_node = normalized_dts
        .split_once("ethernet@7d580000 {")
        .map(|(_, suffix)| suffix)
        .expect("selected GENET DTS node");
    assert!(genet_node.starts_with(" compatible = \"brcm,bcm2711-genet-v5\";"));
    assert!(genet_node.contains("interrupts = <0x00 0x9d 0x04 0x00 0x9e 0x04>;"));
    assert!(!irqs.iter().any(|irq| irq["irq"] == 190));

    let bus_links = manifest["root_task"]["driver_images"]["bus_links"]
        .as_array()
        .expect("driver runtime bus-link topology");
    assert_eq!(bus_links.len(), 1);
    let cyw43_sdio = &bus_links[0];
    assert_eq!(cyw43_sdio["channel"], "cyw43-sdio");
    assert_eq!(cyw43_sdio["client-hot-path"], "cyw43-wifi");
    assert_eq!(cyw43_sdio["owner-hot-path"], "sdio-host");
    assert_eq!(cyw43_sdio["client-notification-slot"], 3);
    assert_eq!(cyw43_sdio["owner-notification-slot"], 3);
    assert_eq!(cyw43_sdio["client-to-owner-slot"], 8);
    assert_eq!(cyw43_sdio["owner-to-client-slot"], 10);
    assert_eq!(cyw43_sdio["shared-offset"], 4096);
    assert_eq!(cyw43_sdio["shared-len"], 32 * 1024);
    assert_eq!(cyw43_sdio["link-epoch"], 0x4359_5301u32);
    assert_eq!(cyw43_sdio["event-offset"], 160);
    assert_eq!(cyw43_sdio["event-len"], 96);
    assert_eq!(cyw43_sdio["event-depth"], 4);
    let usb = images
        .iter()
        .find(|image| image["hot-path"] == "usb-keyboard")
        .expect("usb runtime image");
    assert!(
        usb["mmio-pages"].as_u64().expect("usb mmio pages") >= 16,
        "USB runtime must cover the xHCI minimum operational aperture"
    );
    assert_eq!(
        runtime_pages(images, "serial-console", "shared-buffer-pages"),
        4
    );
    assert_eq!(runtime_pages(images, "usb-keyboard", "dma-pages"), 128);
    assert_eq!(
        runtime_pages(images, "usb-keyboard", "shared-buffer-pages"),
        32
    );
    assert_eq!(runtime_pages(images, "hdmi-text", "dma-pages"), 0);
    assert_eq!(
        runtime_pages(images, "hdmi-text", "shared-buffer-pages"),
        16
    );
    assert_eq!(runtime_pages(images, "genet-nic", "dma-pages"), 64);
    assert_eq!(
        runtime_pages(images, "genet-nic", "shared-buffer-pages"),
        32
    );
    assert_eq!(runtime_pages(images, "cyw43-wifi", "dma-pages"), 0);
    assert_eq!(
        runtime_pages(images, "cyw43-wifi", "shared-buffer-pages"),
        64
    );
    assert_eq!(runtime_pages(images, "cyw43-wifi", "mmio-pages"), 0);
    assert!(runtime_bool(
        images,
        "cyw43-wifi",
        "hardware-state-migrated"
    ));
    assert_eq!(runtime_pages(images, "sdio-host", "mmio-pages"), 3);
    assert_eq!(runtime_pages(images, "sdio-host", "dma-pages"), 10);
    assert_eq!(
        runtime_pages(images, "sdio-host", "shared-buffer-pages"),
        32
    );
    assert!(runtime_bool(images, "sdio-host", "hardware-state-migrated"));
    assert_eq!(
        runtime_pages(images, "pcie-root", "shared-buffer-pages"),
        16
    );
    assert_eq!(runtime_pages(images, "pcie-root", "mmio-pages"), 11);
}

fn runtime_bool(images: &[Value], hot_path: &str, field: &str) -> bool {
    images
        .iter()
        .find(|image| image["hot-path"] == hot_path)
        .unwrap_or_else(|| panic!("runtime image for {hot_path}"))
        .get(field)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("{field} for {hot_path}"))
}

fn temporal_timeout_policy<'a>(tasks: &'a [Value], task_id: &str) -> &'a str {
    tasks
        .iter()
        .find(|task| task["id"] == task_id)
        .unwrap_or_else(|| panic!("temporal task {task_id}"))
        .get("timeout_policy")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("timeout policy for {task_id}"))
}

fn runtime_pages(images: &[Value], hot_path: &str, field: &str) -> u64 {
    images
        .iter()
        .find(|image| image["hot-path"] == hot_path)
        .unwrap_or_else(|| panic!("runtime image for {hot_path}"))
        .get(field)
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("{field} for {hot_path}"))
}

fn device_required(devices: &[Value], kind: &str, id: &str) -> bool {
    devices
        .iter()
        .find(|device| device["kind"] == kind && device["id"] == id)
        .and_then(|device| device["required"].as_bool())
        .unwrap_or(false)
}
