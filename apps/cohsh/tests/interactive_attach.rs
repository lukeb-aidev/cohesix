// Author: Lukas Bower

use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn interactive_mode_retains_prompt_after_failed_attach() {
    Command::new(assert_cmd::cargo::cargo_bin!("cohsh"))
        .arg("--transport")
        .arg("mock")
        .arg("--role")
        .arg("worker-heartbeat")
        .write_stdin("quit\n")
        .timeout(Duration::from_secs(5))
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "detached shell: run 'attach <role>' to connect",
        ))
        .stdout(predicate::str::contains("coh> "))
        .stderr(predicate::str::contains("requires an identity"));
}

#[test]
fn script_mode_propagates_attach_errors() {
    let script_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests/cli/tcp_basic.cohsh");
    Command::new(assert_cmd::cargo::cargo_bin!("cohsh"))
        .arg("--transport")
        .arg("mock")
        .arg("--role")
        .arg("worker-heartbeat")
        .arg("--script")
        .arg(&script_path)
        .timeout(Duration::from_secs(5))
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires an identity"));
}
