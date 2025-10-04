// CLASSIFICATION: COMMUNITY
// Filename: cli_tools_test.rs v0.3
// Author: Lukas Bower
// Date Modified: 2029-09-21

use cohesix::plan9::syscalls;
use std::fs::{self, File};
use std::path::Path;

#[test]
fn cli_tools_exist() {
    let t = Path::new("workspace/tools/cli/src/bin/cohtrace.rs");
    assert!(t.exists(), "cohtrace.rs missing");
    let mut file = File::open(t).expect("open cohtrace.rs");
    let meta = syscalls::fstat(&file).expect("stat cohtrace.rs");
    assert!(
        !meta.permissions().readonly(),
        "cohtrace.rs should be accessible"
    );
    let run_src = Path::new("workspace/tools/cli/src/bin/cohrun.rs");
    assert!(run_src.exists(), "cohrun binary source missing");
}
