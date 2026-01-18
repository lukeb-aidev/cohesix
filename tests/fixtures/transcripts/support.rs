// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Shared transcript fixture helpers for convergence tests.
// Author: Lukas Bower

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

pub fn fixtures_root() -> PathBuf {
    repo_root().join("tests").join("fixtures").join("transcripts")
}

pub fn target_root() -> PathBuf {
    std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root().join("target"))
}

pub fn output_root() -> PathBuf {
    target_root().join("convergence-transcripts")
}

pub fn fixture_path(scenario: &str, name: &str) -> PathBuf {
    fixtures_root().join(scenario).join(name)
}

pub fn output_path(frontend: &str, scenario: &str, name: &str) -> PathBuf {
    output_root().join(frontend).join(scenario).join(name)
}

pub fn timing_path(frontend: &str, scenario: &str, label: &str) -> PathBuf {
    output_root()
        .join(frontend)
        .join(scenario)
        .join(format!("timing-{label}.txt"))
}

pub fn write_timing(frontend: &str, scenario: &str, label: &str, elapsed_ms: u64) {
    let path = timing_path(frontend, scenario, label);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create timing dir");
    }
    let payload = format!("elapsed_ms={elapsed_ms}\n");
    fs::write(&path, payload).expect("write timing");
}

pub fn normalize_lines(lines: &[String]) -> Vec<String> {
    let mut filtered = Vec::new();
    for line in lines {
        if line == "END" {
            filtered.push(line.to_owned());
            continue;
        }
        if line.starts_with("OK AUTH") || line.starts_with("ERR AUTH") {
            continue;
        }
        if line.starts_with("OK ") || line.starts_with("ERR ") {
            filtered.push(line.to_owned());
        }
    }
    filtered
}

pub fn write_transcript(path: &Path, lines: &[String]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create transcript dir");
    }
    let mut payload = lines.join("\n");
    payload.push('\n');
    fs::write(path, payload).expect("write transcript");
}

pub fn diff_files(expected: &Path, actual: &Path) -> Result<(), String> {
    let output = ProcessCommand::new("diff")
        .args(["-u", expected.to_str().unwrap(), actual.to_str().unwrap()])
        .output()
        .map_err(|err| format!("diff failed: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("{stdout}{stderr}"))
}

pub fn compare_transcript(
    frontend: &str,
    scenario: &str,
    name: &str,
    lines: &[String],
) -> PathBuf {
    let normalized = normalize_lines(lines);
    let output_path = output_path(frontend, scenario, name);
    write_transcript(&output_path, &normalized);
    let expected = fixture_path(scenario, name);
    diff_files(&expected, &output_path)
        .unwrap_or_else(|diff| panic!("{frontend} transcript drift:\n{diff}"));
    output_path
}
