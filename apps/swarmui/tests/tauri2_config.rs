// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate SwarmUI Tauri 2 configuration and Cargo test-target alignment.
// Author: Lukas Bower

use std::fs;
use std::path::Path;

use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn tauri_binary_is_not_a_cargo_test_harness() {
    let manifest_path = manifest_dir().join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path).expect("read SwarmUI manifest");
    let manifest: TomlValue = toml::from_str(&manifest).expect("parse SwarmUI manifest");
    let bins = manifest["bin"].as_array().expect("manifest bin targets");
    let swarmui_bin = bins
        .iter()
        .find(|bin| bin["name"].as_str() == Some("swarmui"))
        .expect("swarmui bin target");

    assert_eq!(swarmui_bin["path"].as_str(), Some("src-tauri/main.rs"));
    assert_eq!(
        swarmui_bin["test"].as_bool(),
        Some(false),
        "Tauri binary must stay out of cargo test harness builds"
    );
    assert_eq!(
        swarmui_bin["bench"].as_bool(),
        Some(false),
        "Tauri binary must stay out of cargo bench harness builds"
    );
}

#[test]
fn tauri2_config_uses_v2_distribution_and_global_invoke() {
    let config_path = manifest_dir().join("tauri.conf.json");
    let config = fs::read_to_string(&config_path).expect("read Tauri config");
    let config: JsonValue = serde_json::from_str(&config).expect("parse Tauri config");

    assert_eq!(config["build"]["frontendDist"].as_str(), Some("frontend"));
    assert!(
        config["build"].get("distDir").is_none(),
        "Tauri 2 config must use frontendDist, not distDir"
    );
    assert!(
        config["build"].get("devPath").is_none(),
        "Tauri 2 config must use frontendDist/devUrl, not devPath"
    );
    assert_eq!(config["app"]["withGlobalTauri"].as_bool(), Some(true));
    assert_eq!(config["mainBinaryName"].as_str(), Some("swarmui"));
}

#[test]
fn tauri2_default_capability_is_core_only_for_main_window() {
    let capability_path = manifest_dir().join("src-tauri/capabilities/default.json");
    let capability = fs::read_to_string(&capability_path).expect("read default capability");
    let capability: JsonValue =
        serde_json::from_str(&capability).expect("parse default capability");

    assert_eq!(capability["identifier"].as_str(), Some("default"));
    assert_eq!(capability["windows"].as_array().expect("windows").len(), 1);
    assert_eq!(capability["windows"][0].as_str(), Some("main"));
    assert_eq!(
        capability["permissions"]
            .as_array()
            .expect("permissions")
            .len(),
        1
    );
    assert_eq!(capability["permissions"][0].as_str(), Some("core:default"));
}
