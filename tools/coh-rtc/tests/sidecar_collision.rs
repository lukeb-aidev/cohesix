// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate sidecar mount collision hashing in coh-rtc output.
// Author: Lukas Bower

use coh_rtc::{codegen::hash_bytes, compile, CompileOptions};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn options_for_manifest(temp_dir: &TempDir, manifest_path: PathBuf) -> CompileOptions {
    let out_dir = temp_dir.path().join("generated");
    CompileOptions {
        manifest_path,
        out_dir: out_dir.clone(),
        manifest_out: temp_dir.path().join("root_task_resolved.json"),
        cas_manifest_template_out: temp_dir.path().join("cas_manifest_template.json"),
        cli_script_out: temp_dir.path().join("boot_v0.coh"),
        doc_snippet_out: temp_dir.path().join("snippet.md"),
        observability_interfaces_snippet_out: temp_dir.path().join("observability_interfaces.md"),
        observability_security_snippet_out: temp_dir.path().join("observability_security.md"),
        cas_interfaces_snippet_out: temp_dir.path().join("cas_interfaces.md"),
        cas_security_snippet_out: temp_dir.path().join("cas_security.md"),
        cbor_snippet_out: temp_dir.path().join("telemetry_cbor.md"),
        cohsh_policy_out: temp_dir.path().join("cohsh_policy.toml"),
        cohsh_policy_rust_out: temp_dir.path().join("cohsh_policy.rs"),
        cohsh_policy_doc_out: temp_dir.path().join("cohsh_policy.md"),
    }
}

#[test]
fn sidecar_mounts_hash_prefix_on_collision() {
    let temp_dir = TempDir::new().expect("tempdir");
    let manifest_path = temp_dir.path().join("manifest.toml");
    let manifest = r#"
# Author: Lukas Bower
# Purpose: Test manifest for sidecar collision hashing.
[root_task]
schema = "1.5"

[profile]
name = "virt-aarch64"
kernel = true

[event_pump]
tick_ms = 5

[secure9p]
msize = 8192
walk_depth = 8
tags_per_session = 16
batch_frames = 1

[secure9p.short_write]
policy = "reject"

[features]
net_console = true
serial_console = true
std_console = false
std_host_tools = false

[[tickets]]
role = "queen"
secret = "bootstrap"

[sidecars.modbus]
enable = true
mount_at = "/bus"

[[sidecars.modbus.adapters]]
id = "a"
mount = "log"
scope = "scope-a"
link = "serial"
baud = 9600
spool = { max_entries = 1, max_bytes = 128 }

[[sidecars.modbus.adapters]]
id = "b"
mount = "log"
scope = "scope-b"
link = "serial"
baud = 9600
spool = { max_entries = 1, max_bytes = 128 }
"#;
    fs::write(&manifest_path, manifest).expect("write manifest");

    let options = options_for_manifest(&temp_dir, manifest_path);
    compile(&options).expect("compile manifest");

    let bootstrap = fs::read_to_string(options.out_dir.join("bootstrap.rs"))
        .expect("read bootstrap.rs");
    let prefix_a = &hash_bytes("modbus:a:log".as_bytes())[0..8];
    let prefix_b = &hash_bytes("modbus:b:log".as_bytes())[0..8];
    let expected_a = format!("{prefix_a}-log");
    let expected_b = format!("{prefix_b}-log");
    assert!(bootstrap.contains(&format!("mount: \"{expected_a}\"")));
    assert!(bootstrap.contains(&format!("mount: \"{expected_b}\"")));
}
