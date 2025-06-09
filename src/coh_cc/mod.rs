// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-16

use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::process::Command;

const LOG_PATH: &str = "/log/cohcc_invocations.log";

/// Run the underlying compiler with sandbox enforcement.
pub fn run(mut args: Vec<String>) -> anyhow::Result<()> {
    let mut trace = false;
    let mut dry_run = false;
    let mut filtered = Vec::new();

    for arg in args.into_iter() {
        match arg.as_str() {
            "--trace" => trace = true,
            "--dry-run" => dry_run = true,
            _ => filtered.push(arg),
        }
    }

    let unsupported = ["-shared", "-fPIC", "-fpic", "-dynamic"];
    let mut final_args = Vec::new();
    let mut output_ok = false;
    let mut iter = filtered.iter().peekable();

    while let Some(arg) = iter.next() {
        if arg == "-o" {
            if let Some(path) = iter.peek() {
                if !path.starts_with("/mnt/data") {
                    return Err(anyhow::anyhow!("output path must be inside /mnt/data"));
                }
                output_ok = true;
                final_args.push(arg.clone());
                final_args.push(iter.next().unwrap().clone());
                continue;
            }
        }
        if unsupported.contains(&arg.as_str()) {
            continue;
        }
        final_args.push(arg.clone());
    }

    if !output_ok {
        return Err(anyhow::anyhow!(
            "-o <path> is required and must be in /mnt/data"
        ));
    }

    if !final_args.iter().any(|a| a == "-static") {
        final_args.push("-static".to_string());
    }
    if !final_args.iter().any(|a| a == "-nostdlib") {
        final_args.push("-nostdlib".to_string());
    }

    let compiler = if final_args.iter().any(|a| a.ends_with(".rs")) {
        std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into())
    } else {
        std::env::var("CC").unwrap_or_else(|_| "clang".into())
    };

    create_dir_all("/log")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_PATH)?;
    writeln!(
        file,
        "{compiler} {:?} arch={} sandbox={}",
        final_args,
        std::env::consts::ARCH,
        std::env::var("COH_SANDBOX").unwrap_or_else(|_| "none".into())
    )?;

    if dry_run {
        if trace {
            eprintln!("[dry-run] {} {:?}", compiler, final_args);
        }
        return Ok(());
    }

    if trace {
        eprintln!("{} {:?}", compiler, final_args);
    }

    let status = Command::new(&compiler).args(&final_args).status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("compiler failed"));
    }
    Ok(())
}
