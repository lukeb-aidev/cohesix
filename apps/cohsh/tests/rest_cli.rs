// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate Cohsh REST response-envelope CLI and environment handling.
// Author: Lukas Bower

#![cfg(feature = "rest")]

use std::path::PathBuf;
use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;

fn check_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cohsh/boot_v0.coh")
}

#[test]
fn rest_response_timeout_cli_overrides_a_malformed_environment_value() {
    let bin = assert_cmd::cargo::cargo_bin!("cohsh");
    let mut cmd = Command::new(bin);
    cmd.env("COHSH_REST_RESPONSE_TIMEOUT_MS", "invalid")
        .arg("--rest-response-timeout-ms")
        .arg("190000")
        .arg("--check")
        .arg(check_script());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("check ok:"));
}

#[test]
fn malformed_rest_response_timeout_environment_value_is_rejected() {
    let bin = assert_cmd::cargo::cargo_bin!("cohsh");
    let mut cmd = Command::new(bin);
    cmd.env("COHSH_REST_RESPONSE_TIMEOUT_MS", "invalid")
        .arg("--check")
        .arg(check_script());

    cmd.assert().failure().stderr(predicate::str::contains(
        "invalid COHSH_REST_RESPONSE_TIMEOUT_MS value 'invalid'",
    ));
}

#[test]
fn undersized_rest_response_timeout_is_rejected_before_transport_io() {
    let bin = assert_cmd::cargo::cargo_bin!("cohsh");
    let mut cmd = Command::new(bin);
    cmd.arg("--transport")
        .arg("rest")
        .arg("--rest-url")
        .arg("http://127.0.0.1:1")
        .arg("--rest-response-timeout-ms")
        .arg("14999");

    cmd.assert().failure().stderr(predicate::str::contains(
        "gateway operation response timeout must be at least 15000ms",
    ));
}
