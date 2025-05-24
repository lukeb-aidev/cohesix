// CLASSIFICATION: COMMUNITY
// Filename: dummy_test.rs v0.2
// Date Modified: 2025-05-27
// Author: Lukas Bower

//! Integration tests for the Cohesix compile_from_file function and basic IR functionality.

use std::fs;
use std::path::Path;
use std::process::Command;

use cohesix::{compile_from_file, ir::{Instruction, Opcode}};

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensure compile_from_file returns an error for a non-existent input file.
    #[test]
    fn test_compile_missing_file_fails() {
        let result = compile_from_file("nonexistent.ir", "out.c");
        assert!(result.is_err(), "Expected compile_from_file to error on missing input");
    }

    /// Ensure Instruction constructor and to_string still function in integration context.
    #[test]
    fn test_instruction_integration() {
        let instr = Instruction::new(Opcode::Mul, vec!["2".into(), "3".into()]);
        assert_eq!(instr.opcode, Opcode::Mul);
        let s = instr.to_string();
        assert!(s.contains("Mul"));
        assert!(s.contains("2"));
        assert!(s.contains("3"));
    }

    /// Ensure the CLI binary responds to `--help` without panicking.
    #[test]
    fn test_cli_help() {
        // Use `cargo run -- --help` as a simple smoke test.
        let output = Command::new(env!("CARGO_BIN_EXE_cohesix"))
            .arg("--help")
            .output()
            .expect("Failed to spawn CLI help");
        assert!(output.status.success(), "CLI --help should exit successfully");
        let help_text = String::from_utf8_lossy(&output.stdout);
        assert!(help_text.contains("Cohesix Compiler CLI"), "Help text should mention the CLI description");
    }
}
