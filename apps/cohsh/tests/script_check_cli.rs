// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate cohsh --check mode parses regression scripts.
// Author: Lukas Bower

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::*;

fn script_paths() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cohsh");
    let mut paths = Vec::new();
    for entry in fs::read_dir(root).expect("read scripts/cohsh") {
        let entry = entry.expect("read_dir entry");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("coh") {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

#[test]
fn cli_check_parses_scripts() {
    let bin = assert_cmd::cargo::cargo_bin!("cohsh");
    for path in script_paths() {
        let mut cmd = Command::new(&bin);
        cmd.arg("--check").arg(&path);
        cmd.assert().success();
    }
}
