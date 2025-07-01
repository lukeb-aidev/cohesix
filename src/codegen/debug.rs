// CLASSIFICATION: COMMUNITY
// Filename: debug.rs v1.0
// Date Modified: 2025-05-26
// Author: Lukas Bower

use crate::prelude::*;
/// Debug backend for the Coh_CC compiler. Emits human-readable IR dumps.


use crate::ir::Module;

/// Generates a debug string representation of the entire IR `Module`.
pub fn generate_debug(module: &Module) -> String {
    let mut output = String::new();
    output.push_str(&format!("Debug IR Dump: Module '{}'\n", module.name));
    for func in &module.functions {
        output.push_str(&format!("Function '{}':\n", func.name));
        for instr in &func.body {
            output.push_str(&format!("  {:?} {:?}\n", instr.opcode, instr.operands));
        }
    }
    output
}
