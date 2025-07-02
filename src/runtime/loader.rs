// CLASSIFICATION: COMMUNITY
// Filename: loader.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-12-31

use crate::prelude::*;
use crate::{coh_bail, coh_error, CohError};
use std::fs::File;
use std::io::Read;

const MAGIC: &[u8; 4] = b"COHB";
const VERSION: u8 = 1;

/// Load a `cohcc` binary and execute the embedded program.
pub fn load_and_run(path: &str) -> Result<(), CohError> {
    let mut f = File::open(path).map_err(|e| coh_error!("open {path}: {e}"))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data).map_err(|e| coh_error!("read file: {e}"))?;
    if data.len() < 5 {
        coh_bail!("file too small");
    }
    if &data[0..4] != MAGIC {
        coh_bail!("invalid magic header");
    }
    if data[4] != VERSION {
        coh_bail!("unsupported version {}", data[4]);
    }
    use std::fs;
    use std::process::Command;

    let exe_bytes = &data[5..];
    let tmp_path = "/srv/coh_exec.bin";
    fs::write(tmp_path, exe_bytes).context("write temp exe")?;

    let status = Command::new(tmp_path).status().map_err(|e| coh_error!("exec: {e}"))?;
    fs::remove_file(tmp_path).ok();
    if !status.success() {
        coh_bail!("program exited with {:?}", status.code());
    }
    Ok(())
}
