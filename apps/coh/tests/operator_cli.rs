// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Keep operator command routing, offline exits and bundle alias artifacts compatible.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::Result;
use std::process::{Command, Output};

fn coh(args: &[&str]) -> Result<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_coh"))
        .args(args)
        .env_remove("COH_REST_URL")
        .env_remove("HIVE_GATEWAY_URL")
        .output()?)
}

#[test]
fn bundle_alias_and_offline_commands_share_canonical_artifacts() -> Result<()> {
    let left = tempfile::TempDir::new()?;
    let right = tempfile::TempDir::new()?;
    let l = left.path().to_str().unwrap();
    let r = right.path().to_str().unwrap();
    for args in [
        vec!["bundle", "--mock", "--out", l],
        vec!["evidence", "pack", "--mock", "--out", r],
    ] {
        let result = coh(&args)?;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    // Separate mock sessions have their own observation timestamps. Compare the
    // canonical inventory and validate each digest against that session's bytes.
    let mut keys = Vec::new();
    for root in [left.path(), right.path()] {
        let checksums = std::fs::read(root.join("checksums.json"))?;
        let hashes: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&checksums)?;
        for (path, hash) in &hashes {
            assert_eq!(
                *hash,
                coh::operator::digest(&std::fs::read(root.join(path))?),
                "{path}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(root.join("pack.sha256"))?.trim(),
            coh::operator::digest(&checksums)
        );
        keys.push(hashes.into_keys().collect::<Vec<_>>());
    }
    assert_eq!(keys[0], keys[1]);
    let inspect = coh(&["inspect", "--input", l, "--json"])?;
    assert!(
        inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    assert_eq!(
        inspect.stdout,
        coh(&["inspect", "--input", l, "--json"])?.stdout
    );
    let diff = coh(&["diff", "--left", l, "--right", l])?;
    assert!(diff.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&diff.stdout)?,
        serde_json::json!([])
    );
    let attest = coh(&["attest", "--input", l, "--json"])?;
    assert!(!attest.status.success());
    let value: serde_json::Value = serde_json::from_slice(&attest.stdout)?;
    assert_ne!(value["verdict"], "PASS");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/traces/trace_v0.trace");
    assert!(coh(&["trace", "--input", fixture.to_str().unwrap()])?
        .status
        .success());
    Ok(())
}
