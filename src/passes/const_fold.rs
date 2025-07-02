// CLASSIFICATION: COMMUNITY
// Filename: const_fold.rs v1.0
// Date Modified: 2025-05-26
// Author: Lukas Bower

/// A pass that performs constant folding on the IR.
use crate::ir::IRContext;
use crate::ir::{Instruction, Opcode};
use crate::pass_framework::traits::IRPass;
use crate::prelude::*;

/// Constant Folding pass implementation.
pub struct ConstFold;

impl ConstFold {
    /// Create a new constant folding pass.
    pub fn new() -> Self {
        ConstFold {}
    }

    /// Attempts to fold binary operations with constant operands.
    fn fold_instruction(instr: &Instruction) -> Option<Instruction> {
        match &instr.opcode {
            Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div => {
                if instr.operands.len() == 2 {
                    if let (Ok(lhs), Ok(rhs)) = (
                        instr.operands[0].parse::<i64>(),
                        instr.operands[1].parse::<i64>(),
                    ) {
                        use Opcode::*;
                        let result = match instr.opcode {
                            Add => lhs + rhs,
                            Sub => lhs - rhs,
                            Mul => lhs * rhs,
                            Div => {
                                if rhs == 0 {
                                    return None; // Avoid div-by-zero folding
                                }
                                lhs / rhs
                            }
                            _ => unreachable!(),
                        };
                        return Some(Instruction::new(
                            Opcode::Load,
                            vec![result.to_string(), instr.operands[0].clone()],
                        ));
                    }
                }
                None
            }
            _ => None,
        }
    }
}

impl IRPass for ConstFold {
    fn name(&self) -> &'static str {
        "ConstFold"
    }

    fn run(&self, context: &mut IRContext) {
        for module in &mut context.modules {
            for func in &mut module.functions {
                let mut new_body = vec![];
                for instr in &func.body {
                    if let Some(folded) = Self::fold_instruction(instr) {
                        new_body.push(folded);
                    } else {
                        new_body.push(instr.clone());
                    }
                }
                func.body = new_body;
            }
        }
    }
}

impl Default for ConstFold {
    fn default() -> Self {
        Self::new()
    }
}
