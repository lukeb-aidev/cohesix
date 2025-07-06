// CLASSIFICATION: COMMUNITY
// Filename: emit.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-18

use crate::CohError;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::coh_cc::ir::schema::IRFunction;

/// Serialize `IRFunction` to the given path in JSON format.
/// Existing files are overwritten atomically via a temporary file.
pub fn emit_ir_to_file(ir: &IRFunction, path: &Path) -> Result<(), CohError> {
    let data = serde_json::to_vec(ir)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(&data)?;
        f.flush()?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}
