// CLASSIFICATION: COMMUNITY
// Filename: context.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! Context container for IR-level transformations and optimizations.
//! Holds all modules and serves as the root object for pass management.

use crate::ir::module::Module;

/// The central container for intermediate representation (IR) data.
/// Owns all loaded modules and provides interfaces for IR-wide operations.
#[derive(Clone, Debug, Default)]
pub struct IRContext {
    /// The set of all modules under management.
    pub modules: Vec<Module>,
}

impl IRContext {
    /// Create a new, empty IR context.
    pub fn new() -> Self {
        IRContext {
            modules: Vec::new(),
        }
    }

    /// Add a module to the context.
    pub fn add_module(&mut self, module: Module) {
        self.modules.push(module);
    }

    /// Get a reference to all modules.
    pub fn get_modules(&self) -> &[Module] {
        &self.modules
    }

    /// Get a mutable reference to all modules.
    pub fn get_modules_mut(&mut self) -> &mut [Module] {
        &mut self.modules
    }

    /// Clear all modules from the context.
    pub fn clear(&mut self) {
        self.modules.clear();
    }
}
