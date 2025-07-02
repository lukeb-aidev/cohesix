// CLASSIFICATION: COMMUNITY
// Filename: guard.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-17

use crate::prelude::*;
use crate::{coh_bail, CohError};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use chrono::Utc;

fn append_log(line: &str) -> std::io::Result<()> {
    fs::create_dir_all("/log")?;
    let mut f = OpenOptions::new().create(true).append(true).open("/log/cohcc_builds.log")?;
    writeln!(f, "{} {}", Utc::now().to_rfc3339(), line)?;
    f.flush()?;
    Ok(())
}

fn append_fail_log(line: &str) -> std::io::Result<()> {
    fs::create_dir_all("/log")?;
    let mut f = OpenOptions::new().create(true).append(true).open("/log/cohcc_fail.log")?;
    writeln!(f, "{} {}", Utc::now().to_rfc3339(), line)?;
    f.flush()?;
    Ok(())
}

pub fn check_static_flags(args: &[String]) -> Result<(), CohError> {
    let disallowed = ["-shared", "-fPIC", "-rdynamic"];
    if args.iter().any(|f| disallowed.contains(&f.as_str())) {
        coh_bail!("dynamic linking flags are disallowed");
    }
    Ok(())
}

pub fn hash_output(path: &Path) -> Result<String, CohError> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn validate_output_path(path: &Path) -> Result<(), CohError> {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let forbidden = ["/tmp", "/srv", "/home"];
    for f in &forbidden {
        if canon.starts_with(f) {
            append_fail_log(&format!("forbidden output path {}", canon.display()))?;
            coh_bail!("output path not allowed");
        }
    }
    if !canon.starts_with("/mnt/data") {
        append_fail_log(&format!("output outside /mnt/data {}", canon.display()))?;
        coh_bail!("output path must be under /mnt/data");
    }
    Ok(())
}

pub fn verify_static_binary(output: &Path) -> Result<(), CohError> {
    use std::process::Command;
    let out = Command::new("readelf").arg("-d").arg(output).output()?;
    if !out.status.success() {
        append_fail_log(&format!("readelf failed for {}", output.display()))?;
        coh_bail!("readelf failed");
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.contains("(NEEDED)") {
        append_fail_log(&format!("dynamic binary {}", output.display()))?;
        coh_bail!("binary is dynamically linked");
    }
    Ok(())
}

pub fn log_build(hash: &str, backend: &str, input: &Path, output: &Path, flags: &[String]) -> std::io::Result<()> {
    append_log(&format!("hash={} backend={} input={} output={} flags={:?}", hash, backend, input.display(), output.display(), flags))
}
