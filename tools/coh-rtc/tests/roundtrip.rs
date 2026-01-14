// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate coh-rtc determinism and validation behavior.
// Author: Lukas Bower

use coh_rtc::{compile, CompileOptions};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(path)
}

#[test]
fn manifest_codegen_is_deterministic() {
    let temp_dir = TempDir::new().expect("tempdir");
    let manifest_path = repo_path("configs/root_task.toml");
    let out_dir = temp_dir.path().join("generated");
    let manifest_out = temp_dir.path().join("root_task_resolved.json");
    let cli_script = temp_dir.path().join("boot_v0.coh");
    let doc_snippet = temp_dir.path().join("snippet.md");
    let cbor_snippet = temp_dir.path().join("telemetry_cbor.md");
    let cohsh_policy = temp_dir.path().join("cohsh_policy.toml");
    let cohsh_policy_rust = temp_dir.path().join("cohsh_policy.rs");
    let cohsh_policy_doc = temp_dir.path().join("cohsh_policy.md");

    let options = CompileOptions {
        manifest_path,
        out_dir: out_dir.clone(),
        manifest_out: manifest_out.clone(),
        cli_script_out: cli_script.clone(),
        doc_snippet_out: doc_snippet.clone(),
        cbor_snippet_out: cbor_snippet.clone(),
        cohsh_policy_out: cohsh_policy.clone(),
        cohsh_policy_rust_out: cohsh_policy_rust.clone(),
        cohsh_policy_doc_out: cohsh_policy_doc.clone(),
    };

    let first = compile(&options).expect("compile manifest");
    let baseline = snapshot_dir(&out_dir);
    let baseline_manifest = fs::read(&manifest_out).expect("manifest json");
    let baseline_cli = fs::read(&cli_script).expect("cli script");
    let baseline_docs = fs::read(&doc_snippet).expect("docs snippet");
    let baseline_cbor = fs::read(&cbor_snippet).expect("cbor snippet");
    let baseline_policy = fs::read(&cohsh_policy).expect("cohsh policy");
    let baseline_policy_rust = fs::read(&cohsh_policy_rust).expect("cohsh policy rust");
    let baseline_policy_doc = fs::read(&cohsh_policy_doc).expect("cohsh policy doc");

    let second = compile(&options).expect("compile manifest again");
    let second_snapshot = snapshot_dir(&out_dir);
    let second_manifest = fs::read(&manifest_out).expect("manifest json");
    let second_cli = fs::read(&cli_script).expect("cli script");
    let second_docs = fs::read(&doc_snippet).expect("docs snippet");
    let second_cbor = fs::read(&cbor_snippet).expect("cbor snippet");
    let second_policy = fs::read(&cohsh_policy).expect("cohsh policy");
    let second_policy_rust = fs::read(&cohsh_policy_rust).expect("cohsh policy rust");
    let second_policy_doc = fs::read(&cohsh_policy_doc).expect("cohsh policy doc");

    assert_eq!(baseline, second_snapshot);
    assert_eq!(baseline_manifest, second_manifest);
    assert_eq!(baseline_cli, second_cli);
    assert_eq!(baseline_docs, second_docs);
    assert_eq!(baseline_cbor, second_cbor);
    assert_eq!(baseline_policy, second_policy);
    assert_eq!(baseline_policy_rust, second_policy_rust);
    assert_eq!(baseline_policy_doc, second_policy_doc);
    assert_eq!(first.summary(), second.summary());
}

#[test]
fn invalid_manifest_rejected() {
    let temp_dir = TempDir::new().expect("tempdir");
    let manifest = r#"
# Author: Lukas Bower
# Purpose: Invalid manifest sample for coh-rtc tests.
[root_task]
schema = "1.3"

[profile]
name = "virt-aarch64"
kernel = true

[event_pump]
tick_ms = 5

[secure9p]
msize = 9000
walk_depth = 8
tags_per_session = 4
batch_frames = 1

[secure9p.short_write]
policy = "reject"

[features]
net_console = false
serial_console = true
std_console = false
std_host_tools = false

[[tickets]]
role = "queen"
secret = "bootstrap"
"#;

    let manifest_path = temp_dir.path().join("bad.toml");
    fs::write(&manifest_path, manifest).expect("write manifest");

    let options = CompileOptions {
        manifest_path,
        out_dir: temp_dir.path().join("out"),
        manifest_out: temp_dir.path().join("resolved.json"),
        cli_script_out: temp_dir.path().join("boot_v0.coh"),
        doc_snippet_out: temp_dir.path().join("snippet.md"),
        cbor_snippet_out: temp_dir.path().join("telemetry_cbor.md"),
        cohsh_policy_out: temp_dir.path().join("cohsh_policy.toml"),
        cohsh_policy_rust_out: temp_dir.path().join("cohsh_policy.rs"),
        cohsh_policy_doc_out: temp_dir.path().join("cohsh_policy.md"),
    };

    let err = compile(&options).expect_err("manifest should be rejected");
    assert!(err.to_string().contains("secure9p.msize"));
}

fn snapshot_dir(path: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut entries = fs::read_dir(path)
        .expect("read dir")
        .map(|entry| {
            let entry = entry.expect("entry");
            let file_name = entry.file_name().to_string_lossy().to_string();
            let contents = fs::read(entry.path()).expect("read file");
            (file_name, contents)
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

#[test]
fn cache_kernel_ops_required_for_dma() {
    let temp_dir = TempDir::new().expect("tempdir");
    let manifest = r#"
# Author: Lukas Bower
# Purpose: Invalid cache manifest sample for coh-rtc tests.
[root_task]
schema = "1.3"

[profile]
name = "virt-aarch64"
kernel = true

[event_pump]
tick_ms = 5

[secure9p]
msize = 8192
walk_depth = 8
tags_per_session = 4
batch_frames = 1

[secure9p.short_write]
policy = "reject"

[cache]
kernel_ops = false
dma_clean = true
dma_invalidate = false
unify_instructions = false

[features]
net_console = false
serial_console = true
std_console = false
std_host_tools = false

[[tickets]]
role = "queen"
secret = "bootstrap"
"#;

    let manifest_path = temp_dir.path().join("bad-cache.toml");
    fs::write(&manifest_path, manifest).expect("write manifest");

    let options = CompileOptions {
        manifest_path,
        out_dir: temp_dir.path().join("out"),
        manifest_out: temp_dir.path().join("resolved.json"),
        cli_script_out: temp_dir.path().join("boot_v0.coh"),
        doc_snippet_out: temp_dir.path().join("snippet.md"),
        cbor_snippet_out: temp_dir.path().join("telemetry_cbor.md"),
        cohsh_policy_out: temp_dir.path().join("cohsh_policy.toml"),
        cohsh_policy_rust_out: temp_dir.path().join("cohsh_policy.rs"),
        cohsh_policy_doc_out: temp_dir.path().join("cohsh_policy.md"),
    };

    let err = compile(&options).expect_err("manifest should be rejected");
    assert!(err.to_string().contains("cache.kernel_ops"));
}

#[test]
fn sharding_shard_bits_over_max_rejected() {
    let temp_dir = TempDir::new().expect("tempdir");
    let manifest = r#"
# Author: Lukas Bower
# Purpose: Invalid sharding manifest sample for coh-rtc tests.
[root_task]
schema = "1.3"

[profile]
name = "virt-aarch64"
kernel = true

[event_pump]
tick_ms = 5

[secure9p]
msize = 8192
walk_depth = 8
tags_per_session = 4
batch_frames = 1

[secure9p.short_write]
policy = "reject"

[features]
net_console = false
serial_console = true
std_console = false
std_host_tools = false

[sharding]
enabled = true
shard_bits = 9
legacy_worker_alias = true

[[tickets]]
role = "queen"
secret = "bootstrap"
"#;

    let manifest_path = temp_dir.path().join("bad-sharding.toml");
    fs::write(&manifest_path, manifest).expect("write manifest");

    let options = CompileOptions {
        manifest_path,
        out_dir: temp_dir.path().join("out"),
        manifest_out: temp_dir.path().join("resolved.json"),
        cli_script_out: temp_dir.path().join("boot_v0.coh"),
        doc_snippet_out: temp_dir.path().join("snippet.md"),
        cbor_snippet_out: temp_dir.path().join("telemetry_cbor.md"),
        cohsh_policy_out: temp_dir.path().join("cohsh_policy.toml"),
        cohsh_policy_rust_out: temp_dir.path().join("cohsh_policy.rs"),
        cohsh_policy_doc_out: temp_dir.path().join("cohsh_policy.md"),
    };

    let err = compile(&options).expect_err("manifest should be rejected");
    assert!(err.to_string().contains("sharding.shard_bits"));
}

#[test]
fn legacy_worker_paths_rejected_when_alias_disabled() {
    let temp_dir = TempDir::new().expect("tempdir");
    let manifest = r#"
# Author: Lukas Bower
# Purpose: Invalid alias manifest sample for coh-rtc tests.
[root_task]
schema = "1.3"

[profile]
name = "virt-aarch64"
kernel = true

[event_pump]
tick_ms = 5

[secure9p]
msize = 8192
walk_depth = 8
tags_per_session = 4
batch_frames = 1

[secure9p.short_write]
policy = "reject"

[features]
net_console = false
serial_console = true
std_console = false
std_host_tools = false

[namespaces]
role_isolation = true

[[namespaces.mounts]]
service = "legacy-worker"
target = ["worker", "self"]

[sharding]
enabled = true
shard_bits = 8
legacy_worker_alias = false

[[tickets]]
role = "queen"
secret = "bootstrap"
"#;

    let manifest_path = temp_dir.path().join("bad-alias.toml");
    fs::write(&manifest_path, manifest).expect("write manifest");

    let options = CompileOptions {
        manifest_path,
        out_dir: temp_dir.path().join("out"),
        manifest_out: temp_dir.path().join("resolved.json"),
        cli_script_out: temp_dir.path().join("boot_v0.coh"),
        doc_snippet_out: temp_dir.path().join("snippet.md"),
        cbor_snippet_out: temp_dir.path().join("telemetry_cbor.md"),
        cohsh_policy_out: temp_dir.path().join("cohsh_policy.toml"),
        cohsh_policy_rust_out: temp_dir.path().join("cohsh_policy.rs"),
        cohsh_policy_doc_out: temp_dir.path().join("cohsh_policy.md"),
    };

    let err = compile(&options).expect_err("manifest should be rejected");
    assert!(err
        .to_string()
        .contains("references legacy /worker paths"));
}

#[test]
fn sharding_requires_walk_depth() {
    let temp_dir = TempDir::new().expect("tempdir");
    let manifest = r#"
# Author: Lukas Bower
# Purpose: Invalid walk depth manifest sample for coh-rtc tests.
[root_task]
schema = "1.3"

[profile]
name = "virt-aarch64"
kernel = true

[event_pump]
tick_ms = 5

[secure9p]
msize = 8192
walk_depth = 4
tags_per_session = 4
batch_frames = 1

[secure9p.short_write]
policy = "reject"

[features]
net_console = false
serial_console = true
std_console = false
std_host_tools = false

[sharding]
enabled = true
shard_bits = 8
legacy_worker_alias = true

[[tickets]]
role = "queen"
secret = "bootstrap"
"#;

    let manifest_path = temp_dir.path().join("bad-walk-depth.toml");
    fs::write(&manifest_path, manifest).expect("write manifest");

    let options = CompileOptions {
        manifest_path,
        out_dir: temp_dir.path().join("out"),
        manifest_out: temp_dir.path().join("resolved.json"),
        cli_script_out: temp_dir.path().join("boot_v0.coh"),
        doc_snippet_out: temp_dir.path().join("snippet.md"),
        cbor_snippet_out: temp_dir.path().join("telemetry_cbor.md"),
        cohsh_policy_out: temp_dir.path().join("cohsh_policy.toml"),
        cohsh_policy_rust_out: temp_dir.path().join("cohsh_policy.rs"),
        cohsh_policy_doc_out: temp_dir.path().join("cohsh_policy.md"),
    };

    let err = compile(&options).expect_err("manifest should be rejected");
    assert!(err
        .to_string()
        .contains("sharding.enabled requires secure9p.walk_depth >= 5"));
}
