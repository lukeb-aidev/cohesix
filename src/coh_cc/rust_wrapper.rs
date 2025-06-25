// CLASSIFICATION: COMMUNITY
// Filename: rust_wrapper.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-18

use std::{string::String, vec::Vec, boxed::Box};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use chrono::Utc;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about)]
pub struct RustCli {
    #[command(subcommand)]
    pub command: RustCommand,
}

#[derive(Subcommand)]
pub enum RustCommand {
    Build {
        crate_dir: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        release: bool,
        #[arg(long)]
        sysroot: String,
    },
}

fn append_log(line: &str) -> std::io::Result<()> {
    fs::create_dir_all("/log")?;
    let mut f = OpenOptions::new().create(true).append(true).open("/log/cohcc_rust.log")?;
    writeln!(f, "{} {}", Utc::now().to_rfc3339(), line)?;
    f.flush()?;
    Ok(())
}

fn hash_output(path: &Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = std::io::Read::read(&mut f, &mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn build_crate(dir: &Path, target: &str, release: bool, sysroot: &Path) -> anyhow::Result<PathBuf> {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    cmd.arg("--offline");
    cmd.arg("--target").arg(target);
    if release {
        cmd.arg("--release");
    }
    cmd.current_dir(dir);
    let rustc_ver = String::from_utf8_lossy(&Command::new("rustc").arg("--version").output()?.stdout).trim().to_string();
    let mut rustflags = std::env::var("RUSTFLAGS").unwrap_or_default();
    if !rustflags.is_empty() { rustflags.push(' '); }
    rustflags.push_str("-C target-feature=+crt-static");
    if rustc_ver.contains("nightly") {
        rustflags.push_str(" -Zbuild-std=core,alloc");
    }
    cmd.env("RUSTFLAGS", rustflags);
    cmd.env("SYSROOT", sysroot);
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("cargo build failed");
    }
    let profile = if release { "release" } else { "debug" };
    let crate_name = dir.file_name().unwrap_or_else(|| std::ffi::OsStr::new("bin"));
    let out = dir.join("target").join(target).join(profile).join(crate_name);
    if out.with_extension("so").exists() || out.with_extension("dylib").exists() {
        anyhow::bail!("dynamic libraries detected");
    }
    Ok(out)
}

pub fn main_entry() -> anyhow::Result<()> {
    let cli = RustCli::parse();
    match cli.command {
        RustCommand::Build { crate_dir, target, release, sysroot } => {
            let dir = Path::new(&crate_dir);
            if !dir.join("Cargo.toml").exists() {
                anyhow::bail!("Cargo.toml not found in crate directory");
            }
            let sysroot_p = Path::new(&sysroot).canonicalize()?;
            if !sysroot_p.starts_with("/mnt/data") {
                anyhow::bail!("sysroot must be under /mnt/data");
            }
            let out = build_crate(dir, &target, release, &sysroot_p)?;
            let hash = hash_output(&out)?;
            let rustc_ver = String::from_utf8_lossy(&Command::new("rustc").arg("--version").output()?.stdout).trim().to_string();
            append_log(&format!("crate={} out={} rustc={} hash={}", dir.display(), out.display(), rustc_ver, hash))?;
        }
    }
    Ok(())
}

