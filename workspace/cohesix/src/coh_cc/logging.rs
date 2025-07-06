// CLASSIFICATION: COMMUNITY
// Filename: logging.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-17

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use chrono::Utc;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::Path;

fn append(line: &str) -> std::io::Result<()> {
    create_dir_all("/log")?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/log/cohcc_invocations.log")?;
    writeln!(f, "{} {}", Utc::now().to_rfc3339(), line)?;
    f.flush()?;
    Ok(())
}

pub fn log(level: &str, backend: &str, input: &Path, out: &Path, flags: &[String], msg: &str) {
    let record = format!(
        "level={level} backend={backend} input={} output={} flags={:?} msg={}",
        input.display(),
        out.display(),
        flags,
        msg
    );
    let _ = append(&record);
}

#[macro_export]
macro_rules! cohcc_info {
    ($backend:expr, $input:expr, $out:expr, $flags:expr, $msg:expr $(,)?) => {
        $crate::coh_cc::logging::log("INFO", $backend, $input, $out, $flags, $msg);
    };
}

#[macro_export]
macro_rules! cohcc_warn {
    ($backend:expr, $input:expr, $out:expr, $flags:expr, $msg:expr $(,)?) => {
        $crate::coh_cc::logging::log("WARN", $backend, $input, $out, $flags, $msg);
    };
}

#[macro_export]
macro_rules! cohcc_error {
    ($backend:expr, $input:expr, $out:expr, $flags:expr, $msg:expr $(,)?) => {
        $crate::coh_cc::logging::log("ERROR", $backend, $input, $out, $flags, $msg);
    };
}
