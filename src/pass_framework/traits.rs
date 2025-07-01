
use crate::prelude::*;

// CLASSIFICATION: COMMUNITY
// Filename: traits.rs v1.0
// Date Modified: 2025-05-31
// Author: Lukas Bower

/// Defines the core traits for IR passes in the Cohesix compiler.

use crate::ir::IRContext;

/// Trait for any IR pass that transforms or analyzes the IR.
pub trait IRPass {
    /// Returns the unique name of the pass, used for logging and identification.
    fn name(&self) -> &'static str;

    /// Executes the pass against the provided IR context, mutating it in place.
    fn run(&self, context: &mut IRContext);

    /// Returns a short description of what the pass does.
    fn description(&self) -> &'static str {
        "(undocumented IR pass)"
    }
}