// CLASSIFICATION: COMMUNITY
// Filename: zig.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-18

use std::path::Path;
use std::process::Command;
use crate::coh_cc::{logging, guard, toolchain::Toolchain};

pub fn compile_and_link(source: &str, out: &str, flags: &[String], tc: &Toolchain) -> anyhow::Result<()> {
    let zig = tc.get_tool_path("zig")?;
    guard::check_static_flags(flags)?;
    let mut cmd = Command::new(zig);
    cmd.arg("cc");
    cmd.arg("-static");
    cmd.arg("-o").arg(out);
    for f in flags { cmd.arg(f); }
    cmd.arg(source);
    if !flags.iter().any(|f| f == "--no-strip") {
        cmd.arg("-s");
    }
    logging::log("INFO", "zig", Path::new(source), Path::new(out), flags, "compile");
    let status = cmd.status()?;
    if !status.success() {
        logging::log("ERROR", "zig", Path::new(source), Path::new(out), flags, "zig failed");
        anyhow::bail!("zig cc failed");
    }
    Ok(())
}

