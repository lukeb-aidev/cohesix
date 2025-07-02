// CLASSIFICATION: COMMUNITY
// Filename: ssa.rs v1.0
// Date Modified: 2025-05-26
// Author: Lukas Bower

/// A pass that converts IR into Static Single Assignment (SSA) form by renaming variables.
use crate::ir::IRContext;
use crate::pass_framework::traits::IRPass;
use crate::prelude::*;
use std::collections::HashMap;

/// SSA renaming pass implementation.
pub struct SsaPass;

impl SsaPass {
    /// Create a new SSA pass.
    pub fn new() -> Self {
        SsaPass {}
    }
}

impl IRPass for SsaPass {
    fn name(&self) -> &'static str {
        "SsaPass"
    }

    fn run(&self, context: &mut IRContext) {
        // Simple SSA renaming: for each function, rename operands to unique versions
        for module in &mut context.modules {
            for func in &mut module.functions {
                let mut counter: HashMap<String, usize> = HashMap::new();
                let mut new_body = Vec::with_capacity(func.body.len());
                for instr in &func.body {
                    // Clone instruction and rename operands
                    let mut renamed = instr.clone();
                    renamed.operands = renamed
                        .operands
                        .iter()
                        .map(|op| {
                            let count = counter.entry(op.clone()).or_insert(0);
                            *count += 1;
                            format!("{}_{}", op, count)
                        })
                        .collect();
                    new_body.push(renamed);
                }
                func.body = new_body;
            }
        }
    }
}

impl Default for SsaPass {
    fn default() -> Self {
        Self::new()
    }
}
