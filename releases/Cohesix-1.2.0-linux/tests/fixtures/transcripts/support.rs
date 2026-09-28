// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Shared transcript fixture helpers for convergence tests.
// Author: Lukas Bower

use std::fs;
use std::path::{Path, PathBuf};

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

pub fn fixtures_root() -> PathBuf {
    repo_root()
        .join("tests")
        .join("fixtures")
        .join("transcripts")
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

#[allow(dead_code)]
pub fn timing_path(frontend: &str, scenario: &str, label: &str) -> PathBuf {
    output_root()
        .join(frontend)
        .join(scenario)
        .join(format!("timing-{label}.txt"))
}

#[allow(dead_code)]
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
    let expected_text = fs::read_to_string(expected)
        .map_err(|err| format!("read expected transcript {}: {err}", expected.display()))?;
    let actual_text = fs::read_to_string(actual)
        .map_err(|err| format!("read actual transcript {}: {err}", actual.display()))?;
    if expected_text == actual_text {
        return Ok(());
    }
    Err(render_diff(expected, &expected_text, actual, &actual_text))
}

fn render_diff(expected: &Path, expected_text: &str, actual: &Path, actual_text: &str) -> String {
    let expected_lines: Vec<&str> = expected_text.lines().collect();
    let actual_lines: Vec<&str> = actual_text.lines().collect();
    let first_diff = expected_lines
        .iter()
        .zip(actual_lines.iter())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| expected_lines.len().min(actual_lines.len()));
    let start = first_diff.saturating_sub(3);
    let end = expected_lines
        .len()
        .max(actual_lines.len())
        .min(first_diff.saturating_add(4));

    let mut out = format!("--- {}\n+++ {}\n", expected.display(), actual.display());
    for idx in start..end {
        match (expected_lines.get(idx), actual_lines.get(idx)) {
            (Some(left), Some(right)) if left == right => {
                out.push_str(&format!(" {:>4} {left}\n", idx + 1));
            }
            (Some(left), Some(right)) => {
                out.push_str(&format!("-{:>4} {left}\n", idx + 1));
                out.push_str(&format!("+{:>4} {right}\n", idx + 1));
            }
            (Some(left), None) => {
                out.push_str(&format!("-{:>4} {left}\n", idx + 1));
            }
            (None, Some(right)) => {
                out.push_str(&format!("+{:>4} {right}\n", idx + 1));
            }
            (None, None) => {}
        }
    }
    out
}

pub fn compare_transcript(frontend: &str, scenario: &str, name: &str, lines: &[String]) -> PathBuf {
    let normalized = normalize_lines(lines);
    let output_path = output_path(frontend, scenario, name);
    write_transcript(&output_path, &normalized);
    let expected = fixture_path(scenario, name);
    diff_files(&expected, &output_path)
        .unwrap_or_else(|diff| panic!("{frontend} transcript drift:\n{diff}"));
    output_path
}
