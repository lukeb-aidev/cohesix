// CLASSIFICATION: COMMUNITY
// Filename: compile_success.rs v0.1
// Date Modified: 2025-06-01
// Author: Lukas Bower

//! Integration test for successful compile_from_file execution.

use std::fs;
use std::path::Path;

use cohesix::compile_from_file;

#[test]
fn compile_produces_output_file() {
    let input_path = "test_input.ir";
    let output_path = "test_output.c";

    // Write a tiny IR stub. Real parsing not yet implemented.
    fs::write(input_path, "dummy")
        .expect("failed to create temporary input");

    compile_from_file(input_path, output_path)
        .expect("compile_from_file should succeed");

    assert!(Path::new(output_path).exists(), "output file should be created");

    // Cleanup
    fs::remove_file(input_path).ok();
    fs::remove_file(output_path).ok();
}
