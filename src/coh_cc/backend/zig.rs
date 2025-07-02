// CLASSIFICATION: COMMUNITY
// Filename: zig.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-18

use crate::prelude::*;
use crate::{coh_bail, CohError};
use std::path::Path;
use std::process::Command;

use crate::coh_cc::backend::registry::CompilerBackend;
use crate::coh_cc::{guard, logging, parser::input_type::CohInput, toolchain::Toolchain};

/// Compiler backend using `zig cc` for portable static binaries.
pub struct ZigBackend;

impl ZigBackend {
    fn compile_and_link(
        &self,
        source: &Path,
        out: &Path,
        flags: &[String],
        tc: &Toolchain,
    ) -> Result<(), CohError> {
        let zig = tc.get_tool_path("zig")?;
        guard::check_static_flags(flags)?;
        let mut cmd = Command::new(zig);
        cmd.arg("cc");
        cmd.arg("-static");
        cmd.arg("-o").arg(out);
        for f in flags {
            cmd.arg(f);
        }
        cmd.arg(source);
        if !flags.iter().any(|f| f == "--no-strip") {
            cmd.arg("-s");
        }
        logging::log("INFO", "zig", source, out, flags, "compile");
        let status = cmd.status()?;
        if !status.success() {
            logging::log("ERROR", "zig", source, out, flags, "zig failed");
            coh_bail!("zig cc failed");
        }
        Ok(())
    }
}

impl CompilerBackend for ZigBackend {
    fn compile(
        &self,
        input: &CohInput,
        out_path: &Path,
        _target: &str,
        _sysroot: &Path,
        toolchain: &Toolchain,
    ) -> Result<(), CohError> {
        self.compile_and_link(&input.path, out_path, &input.flags, toolchain)
    }
}
