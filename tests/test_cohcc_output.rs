// CLASSIFICATION: COMMUNITY
// Filename: test_cohcc_output.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-12-31

use cohesix::CohError;
use std::fs::{self, File};
use tempfile::tempdir;

#[test]
fn compile_creates_binary() -> Result<(), CohError> {
    let dir = tempdir()?;
    let src = dir.path().join("example.coh");
    let out = dir.path().join("out.bin");
    fs::write(&src, "print('ok')")?;
    let bytes = cohesix::coh_cc::compile(src.to_str().expect("valid UTF-8 path"))?;
    fs::write(&out, &bytes)?;
    let mut f = File::open(&out)?;
    let meta = cohesix::plan9::syscalls::fstat(&f)?;
    assert!(meta.len() > 0);
    Ok(())
}
