// CLASSIFICATION: COMMUNITY
// Filename: manager.rs v1.0
// Date Modified: 2025-05-26
// Author: Lukas Bower

//! PassManager orchestrates the registration and execution of IR passes in Coh_CC.

use crate::ir::IRContext;
use crate::pass_framework::traits::IRPass;

/// Manages a sequence of IR passes to run on a module context.
pub struct PassManager {
    /// Ordered list of boxed IRPass implementations.
    passes: Vec<Box<dyn IRPass>>,
}

impl PassManager {
    /// Creates a new, empty PassManager.
    pub fn new() -> Self {
        PassManager { passes: Vec::new() }
    }

    /// Registers a new pass for later execution.
    pub fn add_pass<P: IRPass + 'static>(&mut self, pass: P) {
        self.passes.push(Box::new(pass));
    }

    /// Executes all registered passes in order against the provided IRContext.
    pub fn run_all(&self, context: &mut IRContext) {
        for pass in &self.passes {
            log::info!("Running pass: {}", pass.name());
            pass.run(context);
        }
    }

    /// Returns the number of registered passes.
    pub fn count(&self) -> usize {
        self.passes.len()
    }
}
