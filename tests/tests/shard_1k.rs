// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify shard-aware cohsh script flows with and without legacy aliases.
// Author: Lukas Bower

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use cohsh::{NineDoorTransport, Shell};
use nine_door::{NineDoor, ShardLayout};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tests crate has a parent")
        .to_path_buf()
}

fn shard_script_path() -> PathBuf {
    repo_root().join("scripts").join("cohsh").join("shard_1k.coh")
}

fn run_script(server: NineDoor) -> anyhow::Result<()> {
    let transport = NineDoorTransport::new(server);
    let mut shell = Shell::new(transport, Vec::new());
    let file = File::open(shard_script_path())?;
    shell.run_script(BufReader::new(file))
}

#[test]
fn shard_script_passes_with_alias_enabled() -> anyhow::Result<()> {
    run_script(NineDoor::new())
}

#[test]
fn shard_script_rejects_legacy_alias_when_disabled() {
    let server = NineDoor::new_with_shard_layout(ShardLayout::enabled(8, false));
    let err = run_script(server).expect_err("legacy alias should fail when disabled");
    let message = err.to_string();
    assert!(
        message.contains("/worker/worker-1/telemetry"),
        "{message}"
    );
    assert!(message.contains("ERR"), "{message}");
}
