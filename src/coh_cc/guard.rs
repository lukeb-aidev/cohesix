// CLASSIFICATION: COMMUNITY
// Filename: guard.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-17

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

pub fn check_static_flags(args: &[String]) -> anyhow::Result<()> {
    let disallowed = ["-shared", "-fPIC", "-rdynamic"];
    if args.iter().any(|f| disallowed.contains(&f.as_str())) {
        anyhow::bail!("dynamic linking flags are disallowed");
    }
    Ok(())
}

pub fn hash_output(path: &Path) -> anyhow::Result<String> {
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

pub fn log_build(hash: &str, backend: &str, input: &Path, output: &Path, flags: &[String]) -> std::io::Result<()> {
    append_log(&format!("hash={} backend={} input={} output={} flags={:?}", hash, backend, input.display(), output.display(), flags))
}
