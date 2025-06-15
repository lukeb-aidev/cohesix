// CLASSIFICATION: COMMUNITY
// Filename: c.rs v1.1
// Date Modified: 2025-07-24
// Author: Lukas Bower

//! C backend for the Coh_CC compiler. Translates IR into C code.

use crate::ir::{Module, Opcode};

/// Generates a C source file from an IR `Module`.
pub fn generate_c(module: &Module) -> String {
    let mut output = String::new();
    // Preamble
    output.push_str("#include <stdio.h>\n");
    output.push_str("#include <stdlib.h>\n\n");
    // Forward declarations
    for func in &module.functions {
        output.push_str(&format!("void {}();\n", func.name));
    }
    output.push_str("\n");

    // Function definitions
    for func in &module.functions {
        output.push_str(&format!("void {}() {{\n", func.name));
        for instr in &func.body {
            match &instr.opcode {
                Opcode::Add => output.push_str(&format!(
                    "    // ADD {:?}\n",
                    instr.operands
                )),
                Opcode::Sub => output.push_str(&format!(
                    "    // SUB {:?}\n",
                    instr.operands
                )),
                Opcode::Mul => output.push_str(&format!(
                    "    // MUL {:?}\n",
                    instr.operands
                )),
                Opcode::Div => output.push_str(&format!(
                    "    // DIV {:?}\n",
                    instr.operands
                )),
                Opcode::Call { function } => output.push_str(&format!("    {}();\n", function)),
                Opcode::Ret => output.push_str("    return;\n"),
                _ => output.push_str(&format!("    // Unhandled opcode: {:?}\n", instr.opcode)),
            }
        }
        output.push_str("}\n\n");
    }

    // Main function stub
    output.push_str("int main(int argc, char** argv) {\n");
    if let Some(first) = module.functions.first() {
        output.push_str(&format!("    {}();\n", first.name));
    }
    output.push_str("    return 0;\n}\n");

    output
}
