// CLASSIFICATION: COMMUNITY
// Filename: logging.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-17

use chrono::Utc;
use std::fs::{OpenOptions, create_dir_all};
use std::io::Write;

fn append(path: &str, line: &str) -> std::io::Result<()> {
    create_dir_all("/log")?;
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{} {}", Utc::now().to_rfc3339(), line)?;
    Ok(())
}

pub fn log_invocation(line: &str) {
    let _ = append("/log/cohcc_invocations.log", line);
}

pub fn log_failure(line: &str) {
    let _ = append("/log/cohcc_fail.log", line);
}

