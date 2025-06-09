// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-17

pub mod config;
pub mod logging;
pub mod backend;
pub mod parser;

use anyhow::Context;
use config::Config;
use backend::{compile_backend, Backend};

/// Compile the given `source` file to `out_path` using the selected backend.
pub fn compile(source: &str, out_path: &str, flags: &[String], cfg: &Config) -> anyhow::Result<()> {
    if !cfg.valid_output(out_path) {
        anyhow::bail!("output path must be within project dir or /mnt/data");
    }
    compile_backend(source, out_path, flags, cfg.backend)
        .with_context(|| format!("compile failed using {:?}", cfg.backend))
}

