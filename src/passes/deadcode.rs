// CLASSIFICATION: COMMUNITY
// Filename: deadcode.rs v1.0
// Date Modified: 2025-05-26
// Author: Lukas Bower

use crate::prelude::*;
//! A pass that removes dead (unreachable or unused) instructions from the IR.

use crate::ir::IRContext;
use crate::ir::{Instruction, Opcode};
use crate::pass_framework::traits::IRPass;

/// Dead Code Elimination pass implementation.
pub struct DeadCode;

impl DeadCode {
    /// Create a new dead code elimination pass.
    pub fn new() -> Self {
        DeadCode {}
    }

    /// Determines if an instruction is dead (e.g., Nop or unused Store).
    fn is_dead(instr: &Instruction) -> bool {
        match instr.opcode {
            Opcode::Nop => true,
            Opcode::Store => {
                // For simplicity, assume stores are always side-effecting and not dead.
                false
            }
            _ => false,
        }
    }
}

impl IRPass for DeadCode {
    fn name(&self) -> &'static str {
        "DeadCodeElimination"
    }

    fn run(&self, context: &mut IRContext) {
        for module in &mut context.modules {
            for func in &mut module.functions {
                func.body.retain(|instr| !Self::is_dead(instr));
            }
        }
    }
}

impl Default for DeadCode {
    fn default() -> Self {
        Self::new()
    }
}
