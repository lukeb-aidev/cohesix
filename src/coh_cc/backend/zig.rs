// CLASSIFICATION: COMMUNITY
// Filename: zig.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-17

use std::path::Path;
use std::process::Command;
use crate::coh_cc::logging::{log_invocation, log_failure};

pub fn zig_path() -> Option<String> {
    std::env::var("ZIG_PATH").ok().or_else(|| {
        let default = "/mnt/data/toolchains/zig-linux-x86_64-0.11.0/zig";
        if Path::new(default).exists() { Some(default.to_string()) } else { None }
    })
}

pub fn compile_and_link(source: &str, out: &str, flags: &[String]) -> anyhow::Result<()> {
    let zig = zig_path().ok_or_else(|| anyhow::anyhow!("zig compiler not found"))?;
    let disallowed = ["-shared", "-dynamic", "-fPIC", "-fpic", "--dynamic"];
    if flags.iter().any(|f| disallowed.contains(&f.as_str())) {
        let msg = "dynamic linking flags are disallowed";
        log_failure(msg);
        anyhow::bail!(msg);
    }
    let mut cmd = Command::new(zig);
    cmd.arg("cc");
    cmd.arg("-static");
    cmd.arg("-o").arg(out);
    for f in flags { cmd.arg(f); }
    cmd.arg(source);
    if !flags.iter().any(|f| f == "--no-strip") {
        cmd.arg("-s");
    }
    log_invocation(&format!("zig cc {:?}", cmd.get_args().collect::<Vec<_>>()));
    let status = cmd.status()?;
    if !status.success() {
        log_failure("zig failed");
        anyhow::bail!("zig cc failed");
    }
    Ok(())
}

