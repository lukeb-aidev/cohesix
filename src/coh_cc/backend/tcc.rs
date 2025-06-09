// CLASSIFICATION: COMMUNITY
// Filename: tcc.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-17

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::coh_cc::backend::registry::CompilerBackend;
use crate::coh_cc::parser::input_type::CohInput;
use crate::coh_cc::guard;

pub struct TccBackend;

impl CompilerBackend for TccBackend {
    fn compile(&self, input: &CohInput, out_path: &Path) -> anyhow::Result<()> {
        guard::check_static_flags(&input.flags)?;
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = File::create(out_path)?;
        f.write_all(b"dummy binary")?;
        Ok(())
    }
}
