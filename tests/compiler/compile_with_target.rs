// CLASSIFICATION: COMMUNITY
// Filename: compile_with_target.rs v0.1
// Date Modified: 2025-07-11
// Author: Lukas Bower

use cohesix::compile_from_file_with_target;
use std::fs;

#[test]
fn compile_with_target_runs() {
    fs::write("tiny.ir", "dummy").unwrap();
    let res = compile_from_file_with_target("tiny.ir", "tiny.c", "x86_64");
    fs::remove_file("tiny.ir").ok();
    fs::remove_file("tiny.c").ok();
    assert!(res.is_ok());
}
