// CLASSIFICATION: COMMUNITY
// Filename: builder.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-11

/// IRBuilder for constructing Modules and Functions.
use crate::ir::{Function, Instruction, Module, Opcode};
use crate::prelude::*;

/// Helper for incrementally building IR modules.
pub struct IRBuilder {
    module: Module,
    current: Option<Function>,
}

impl IRBuilder {
    /// Create a new builder for a module with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        IRBuilder {
            module: Module::new(name),
            current: None,
        }
    }

    /// Begin emitting a new function. Any existing function will be finalized.
    pub fn start_function(&mut self, name: impl Into<String>) {
        if let Some(func) = self.current.take() {
            self.module.add_function(func);
        }
        self.current = Some(Function::new(name));
    }

    /// Emit an instruction into the current function.
    pub fn emit(&mut self, instr: Instruction) {
        if let Some(func) = &mut self.current {
            func.body.push(instr);
        }
    }

    /// Convenience to emit an Add instruction.
    pub fn build_add(&mut self, lhs: impl Into<String>, rhs: impl Into<String>) {
        self.emit(Instruction::new(Opcode::Add, vec![lhs.into(), rhs.into()]));
    }

    /// Finish the current function and add it to the module.
    pub fn finish_function(&mut self) {
        if let Some(func) = self.current.take() {
            self.module.add_function(func);
        }
    }

    /// Finalize and return the built module.
    pub fn finalize(mut self) -> Module {
        self.finish_function();
        self.module
    }
}
