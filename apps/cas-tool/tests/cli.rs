// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate cas-tool command-line capacity preflight behavior.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::process::Command;
use tempfile::TempDir;

#[test]
fn pack_rejects_chunk_size_override_before_creating_bundle() {
    let temp_dir = TempDir::new().expect("tempdir");
    let input = temp_dir.path().join("payload.bin");
    let template = temp_dir.path().join("template.json");
    let out_dir = temp_dir.path().join("bundle");
    std::fs::write(&input, [0x5au8; 128]).expect("write payload");
    std::fs::write(
        &template,
        r#"{
            "chunk_bytes": 128,
            "delta": null,
            "limits": {"max_chunks": 8, "max_payload_bytes": 1024},
            "signature": null
        }"#,
    )
    .expect("write template");

    let binary = std::env::var_os("CARGO_BIN_EXE_cas-tool")
        .expect("Cargo must provide the cas-tool integration-test binary");
    let output = Command::new(binary)
        .args([
            "pack",
            "--epoch",
            "override",
            "--input",
            input.to_str().expect("input path"),
            "--out-dir",
            out_dir.to_str().expect("output path"),
            "--template",
            template.to_str().expect("template path"),
            "--chunk-bytes",
            "64",
        ])
        .output()
        .expect("run cas-tool");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(stderr.contains("--chunk-bytes 64 does not match selected template chunk_bytes 128"));
    assert!(!out_dir.exists(), "rejected pack created an output bundle");
}
