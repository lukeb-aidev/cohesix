// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Author: Lukas Bower
// Purpose: Verify offline CLI manuals precede transport and credential initialization.

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn offline_manual_and_index_need_no_transport() {
    Command::new(assert_cmd::cargo::cargo_bin!("cohsh"))
        .args(["--man", "spawn"])
        .assert()
        .success()
        .stdout(contains("budget_ttl_s=<u64>"));
    Command::new(assert_cmd::cargo::cargo_bin!("cohsh"))
        .arg("--man")
        .assert()
        .success()
        .stdout(contains("  spawn\n"));
}

#[test]
fn invalid_manual_requests_fail_without_connecting() {
    for args in [vec!["--man", "unknown"], vec!["--man", "spawn", "extra"]] {
        Command::new(assert_cmd::cargo::cargo_bin!("cohsh"))
            .args(args)
            .assert()
            .failure();
    }
}
