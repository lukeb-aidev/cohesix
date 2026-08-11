// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Emit one target-qualified Cohesix Python SDK profile contract.
// Author: Lukas Bower

use anyhow::{bail, Context, Result};
use clap::Parser;
use coh_rtc::codegen::{cohesix_py, hash_bytes};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Args {
    /// Selected root-task source manifest.
    manifest: PathBuf,
    /// Compiler-owned seL4 profile catalog.
    #[arg(long, default_value = "configs/sel4/profiles.toml")]
    sel4_profiles: PathBuf,
    /// Exact release-eligible seL4 profile id.
    #[arg(long)]
    profile: String,
    /// Output path for the generated JSON contract.
    #[arg(long)]
    out: PathBuf,
    /// Optional output path for the target-neutral wheel defaults module.
    #[arg(long)]
    defaults_out: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct ProfileCatalog {
    profiles: BTreeMap<String, Sel4Profile>,
}

#[derive(Debug, Deserialize)]
struct Sel4Profile {
    target: String,
    release_eligible: bool,
    runtime_eligible: bool,
    #[serde(default)]
    qemu_gic_version: Option<u8>,
    cmake: BTreeMap<String, toml::Value>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let target = load_target(&args.sel4_profiles, &args.profile)?;
    let manifest = coh_rtc::ir::load_manifest(&args.manifest)?;
    manifest.validate_with_base(args.manifest.parent())?;
    let resolved = coh_rtc::ir::serialize_manifest(&manifest)?;
    let manifest_hash = hash_bytes(&resolved);
    let bytes =
        cohesix_py::render_profile_contract(&manifest, &manifest_hash, &args.profile, &target)?;
    cohesix_py::emit_profile_contract(&bytes, &args.out)?;
    if let Some(defaults_out) = &args.defaults_out {
        let defaults = cohesix_py::render_defaults(&manifest, &manifest_hash);
        cohesix_py::emit_defaults(&defaults, defaults_out)?;
    }
    println!("coh-rtc-python-profile: wrote {}", args.out.display());
    Ok(())
}

fn load_target(path: &Path, profile_id: &str) -> Result<String> {
    let source =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let catalog: ProfileCatalog =
        toml::from_str(&source).with_context(|| format!("failed to parse {}", path.display()))?;
    let profile = catalog
        .profiles
        .get(profile_id)
        .with_context(|| format!("unknown seL4 profile {profile_id}"))?;
    if !profile.release_eligible || !profile.runtime_eligible {
        bail!("Python target contract requires a release- and runtime-eligible profile");
    }
    for (name, expected) in [("MCS", "ON"), ("SMP", "ON"), ("NUM_NODES", "4")] {
        let actual = profile
            .cmake
            .get(name)
            .and_then(toml::Value::as_str)
            .unwrap_or_default();
        if actual != expected {
            bail!("seL4 profile {profile_id} requires {name}={expected}, got {actual}");
        }
    }
    match profile.target.as_str() {
        "qemu" if profile.qemu_gic_version == Some(3) => Ok("qemu".to_owned()),
        "qemu" => bail!("QEMU Python contract requires GICv3"),
        "pi4" => Ok("pi4".to_owned()),
        other => bail!("unsupported seL4 profile target {other}"),
    }
}
