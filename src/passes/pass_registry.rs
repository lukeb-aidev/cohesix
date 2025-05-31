//! IR pass registry for Coh_CC
// CLASSIFICATION: COMMUNITY
// Filename: pass_registry.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! IR pass registry for Coh_CC
//! Manages registration and dispatch of IR passes across the compiler pipeline.

use super::ir_pass_trait::IRPass;

pub struct PassRegistry {
    passes: Vec<Box<dyn IRPass>>,
}

impl PassRegistry {
    pub fn new() -> Self {
        PassRegistry {
            passes: Vec::new(),
        }
    }

    /// Register a new IR pass into the registry.
    pub fn register(&mut self, pass: Box<dyn IRPass>) {
        println!("[PassRegistry] Registered pass: {}", pass.name());
        self.passes.push(pass);
    }

    /// Execute all registered passes in order.
    pub fn run_all_passes(&mut self, context: &mut crate::ir::context::IRContext) {
        for pass in &self.passes {
            println!("[PassRegistry] Running pass: {}", pass.name());
            pass.run(context);
        }
    }
}
