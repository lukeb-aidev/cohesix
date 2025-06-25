// CLASSIFICATION: COMMUNITY
// Filename: pass.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-11

//! Simple pass framework for transforming IR modules.

use std::{boxed::Box, vec::Vec};

use crate::ir::{Module, Opcode};

/// Trait implemented by all passes over [`Module`].
pub trait Pass {
    /// Name of the pass for logging.
    fn name(&self) -> &'static str;
    /// Execute the pass on the provided module.
    fn run(&mut self, module: &mut Module);
}

/// Manages and runs a set of passes sequentially.
pub struct PassManager {
    passes: Vec<Box<dyn Pass>>,
}

impl PassManager {
    /// Create an empty manager.
    pub fn new() -> Self { Self { passes: Vec::new() } }

    /// Register a pass to run later.
    pub fn add_pass<P: Pass + 'static>(&mut self, pass: P) {
        self.passes.push(Box::new(pass));
    }

    /// Run all passes over the module.
    pub fn run_all(&mut self, module: &mut Module) {
        for pass in &mut self.passes {
            pass.run(module);
        }
    }
}

impl Default for PassManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Example pass that removes `Nop` instructions.
pub struct DeadCodeEliminationPass;

impl DeadCodeEliminationPass {
    /// Create a new dead code elimination pass.
    pub fn new() -> Self { Self }
}

impl Default for DeadCodeEliminationPass {
    fn default() -> Self {
        Self::new()
    }
}

impl Pass for DeadCodeEliminationPass {
    fn name(&self) -> &'static str { "DeadCodeElimination" }

    fn run(&mut self, module: &mut Module) {
        for func in &mut module.functions {
            func.body.retain(|i| i.opcode != Opcode::Nop);
        }
    }
}
