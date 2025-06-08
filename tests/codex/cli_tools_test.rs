// CLASSIFICATION: COMMUNITY
// Filename: cli_tools_test.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2025-07-11

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[test]
fn cli_tools_exist() {
    let t = Path::new("cli/cohtrace.py");
    assert!(t.exists(), "cohtrace.py missing");
    let meta = fs::metadata(t).unwrap();
    assert!(meta.permissions().mode() & 0o111 != 0, "cohtrace.py should be executable");
    let run_src = Path::new("src/bin/cohrun.rs");
    assert!(run_src.exists(), "cohrun binary source missing");
}
