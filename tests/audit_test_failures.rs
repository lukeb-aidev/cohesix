// CLASSIFICATION: COMMUNITY
// Filename: audit_test_failures.rs v0.2
// Date Modified: 2025-07-03
// Author: Cohesix Codex

use std::fs::{self, OpenOptions};
use std::io::Write;

#[test]
fn audit_test_failures() {
    if let Ok(data) = fs::read_to_string("tests/test_failures.log") {
        fs::create_dir_all("/srv").ok();
        let mut out = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/srv/testfailures.log")
            .unwrap();
        for line in data.lines() {
            let msg = if let Some((name, reason)) = line.split_once(':') {
                format!("{name} -- {reason}")
            } else {
                line.to_string()
            };
            let _ = writeln!(out, "{}", msg);
        }
    }
}
