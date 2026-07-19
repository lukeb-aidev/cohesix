// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate Pi 4 U-Boot profile codegen against the active Milestone 26b contract.
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
fn pi4_uboot_profile_emits_network_policy() {
    let temp_dir = TempDir::new().expect("tempdir");
    let options = compile_options(
        repo_path("configs/root_task_pi4_uboot_aarch64.toml"),
        &temp_dir,
    );
    compile(&options).expect("compile pi4 profile");

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
            256,
            "runtime image {} must cover the measured multi-segment linked ELF",
            image["id"]
        );
    }
    let irqs = manifest["root_task"]["driver_images"]["irqs"]
        .as_array()
        .expect("driver runtime IRQ topology");
    assert_eq!(irqs.len(), 1);
    assert_eq!(irqs[0]["hot-path"], "sdio-host");
    assert_eq!(irqs[0]["irq"], 158);
    assert_eq!(irqs[0]["badge"], 159);
    assert_eq!(irqs[0]["handler-slot"], 4);
    assert_eq!(irqs[0]["notification-slot"], 3);
    assert_eq!(irqs[0]["trigger"], "level");

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
    assert_eq!(cyw43_sdio["shared-len"], 8192);
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
    assert_eq!(runtime_pages(images, "sdio-host", "mmio-pages"), 2);
    assert_eq!(runtime_pages(images, "sdio-host", "dma-pages"), 1);
    assert_eq!(
        runtime_pages(images, "sdio-host", "shared-buffer-pages"),
        32
    );
    assert!(runtime_bool(images, "sdio-host", "hardware-state-migrated"));
    assert_eq!(
        runtime_pages(images, "pcie-root", "shared-buffer-pages"),
        16
    );
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
