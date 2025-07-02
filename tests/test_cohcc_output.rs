// CLASSIFICATION: COMMUNITY
// Filename: test_cohcc_output.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-12-08

use cohesix::CohError;
use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;

#[test]
fn compile_creates_binary() -> Result<(), CohError> {
    let dir = tempdir()?;
    let src = dir.path().join("example.coh");
    let out = dir.path().join("out.bin");
    fs::write(&src, "print('ok')")?;
    let bytes = cohesix::coh_cc::compile(src.to_str().expect("valid UTF-8 path"))?;
    fs::write(&out, &bytes)?;
    fs::set_permissions(&out, Permissions::from_mode(0o755))?;
    let meta = fs::metadata(&out)?;
    assert!(meta.len() > 0);
    Ok(())
}
