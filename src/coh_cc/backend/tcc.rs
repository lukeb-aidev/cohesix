// CLASSIFICATION: COMMUNITY
// Filename: tcc.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-18

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::coh_cc::backend::registry::CompilerBackend;
use crate::coh_cc::parser::input_type::CohInput;
use crate::coh_cc::guard;

pub struct TccBackend;

impl CompilerBackend for TccBackend {
    fn compile(
        &self,
        input: &CohInput,
        out_path: &Path,
        target: &str,
        sysroot: &Path,
    ) -> anyhow::Result<()> {
        guard::check_static_flags(&input.flags)?;
        if !["x86_64-linux-musl", "aarch64-linux-musl"].contains(&target) {
            anyhow::bail!("unsupported target {target}");
        }
        if !sysroot.starts_with("/mnt/data") || !sysroot.exists() {
            anyhow::bail!("invalid sysroot");
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = File::create(out_path)?;
        f.write_all(format!("dummy binary for {target}").as_bytes())?;
        crate::coh_cc::logging::log(
            "INFO",
            "tcc",
            &input.path,
            out_path,
            &input.flags,
            &format!("target_triple={target}"),
        );
        Ok(())
    }
}
