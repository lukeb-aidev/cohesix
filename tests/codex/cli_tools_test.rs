// CLASSIFICATION: COMMUNITY
// Filename: cli_tools_test.rs v0.2
// Author: Cohesix Codex
// Date Modified: 2026-12-31

use cohesix::plan9::syscalls;
use std::fs::{self, File};
use std::path::Path;

#[test]
fn cli_tools_exist() {
    let t = Path::new("cli/cohtrace.py");
    assert!(t.exists(), "cohtrace.py missing");
    let mut file = File::open(t).expect("open cohtrace.py");
    let meta = syscalls::fstat(&file).expect("stat cohtrace.py");
    assert!(!meta.permissions().readonly(), "cohtrace.py should be accessible");
    let run_src = Path::new("src/bin/cohrun.rs");
    assert!(run_src.exists(), "cohrun binary source missing");
}
