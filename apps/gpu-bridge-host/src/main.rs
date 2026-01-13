// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for the host-side GPU bridge; prints mirrored namespace metadata.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! CLI entry point for the host-side GPU bridge. The binary prints discovered
//! GPU information as JSON, enabling integration tests to synchronise the
//! NineDoor namespace with host state.

use anyhow::Result;
use clap::{ArgAction, Parser};

use gpu_bridge_host::{auto_bridge, namespace_to_json_pretty, GpuNamespaceSnapshot};

/// CLI arguments for the GPU bridge host tool.
#[derive(Debug, Parser)]
#[command(author, version, about = "Cohesix GPU bridge host utilities")]
struct Args {
    /// Use the deterministic mock backend instead of NVML.
    #[arg(long, action = ArgAction::SetTrue)]
    mock: bool,
    /// Print GPU namespace JSON to stdout.
    #[arg(long, action = ArgAction::SetTrue)]
    list: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let bridge = auto_bridge(args.mock)?;
    let namespace: GpuNamespaceSnapshot = bridge.serialise_namespace()?;
    if args.list {
        println!("{}", namespace_to_json_pretty(&namespace));
    }
    Ok(())
}
