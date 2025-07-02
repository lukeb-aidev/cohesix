// CLASSIFICATION: COMMUNITY
// Filename: nop.rs v1.0
// Date Modified: 2025-05-26
// Author: Lukas Bower

/// A pass that removes all NOP instructions from the IR.
use crate::ir::IRContext;
use crate::ir::Instruction;
use crate::pass_framework::traits::IRPass;
use crate::prelude::*;

/// NOP elimination pass implementation.
pub struct NopPass;

impl NopPass {
    /// Create a new NOP elimination pass.
    pub fn new() -> Self {
        NopPass {}
    }

    /// Checks whether an instruction is a NOP.
    fn is_nop(instr: &Instruction) -> bool {
        matches!(instr.opcode, crate::ir::Opcode::Nop)
    }
}

impl IRPass for NopPass {
    fn name(&self) -> &'static str {
        "NopPass"
    }

    fn run(&self, context: &mut IRContext) {
        for module in &mut context.modules {
            for func in &mut module.functions {
                // Retain only non-NOP instructions
                func.body.retain(|instr| !Self::is_nop(instr));
            }
        }
    }
}

impl Default for NopPass {
    fn default() -> Self {
        Self::new()
    }
}
