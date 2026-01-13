// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Ensure quit exits cleanly without hanging the interactive shell.
// Author: Lukas Bower

use std::time::Duration;

#[test]
fn quit_exits_cleanly() {
    let mut cmd = assert_cmd::cargo_bin_cmd!("cohsh");
    cmd.arg("--transport")
        .arg("mock")
        .env("RUST_LOG", "warn")
        .write_stdin("quit\n")
        .timeout(Duration::from_secs(1));
    let assert = cmd.assert().success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("Welcome to Cohesix"),
        "missing welcome banner: {stdout:?}"
    );
    assert!(
        stdout.contains("detached shell: run 'attach <role>' to connect"),
        "missing detached notice: {stdout:?}"
    );
    assert!(
        stdout.contains("closing session"),
        "missing closing session: {stdout:?}"
    );

    let welcome = stdout
        .find("Welcome to Cohesix")
        .expect("welcome missing");
    let detached = stdout
        .find("detached shell: run 'attach <role>' to connect")
        .expect("detached missing");
    let closing = stdout.find("closing session").expect("closing missing");
    assert!(
        welcome < detached && detached < closing,
        "unexpected output ordering: {stdout:?}"
    );
    assert!(
        stderr.trim().is_empty(),
        "unexpected stderr output: {stderr:?}"
    );
}

#[test]
fn blank_lines_reprint_prompt() {
    let mut cmd = assert_cmd::cargo_bin_cmd!("cohsh");
    cmd.arg("--transport")
        .arg("mock")
        .write_stdin("\nquit\n")
        .timeout(Duration::from_secs(1));
    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let prompt_count = stdout.matches("coh> ").count();
    assert!(
        prompt_count >= 2,
        "expected prompt to reprint after blank line, saw {prompt_count} prompts in {stdout:?}"
    );
}
