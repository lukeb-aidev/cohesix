// CLASSIFICATION: COMMUNITY
// Filename: ir_pass_trait.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! Trait definition for IR passes in Coh_CC
//! Each IR pass must implement a name and run method.
//! Additional metadata or diagnostics may be added in future versions.

use crate::ir::context::IRContext;

pub trait IRPass {
    fn name(&self) -> &'static str;

    /// A short description of what this pass does.
    fn description(&self) -> &'static str {
        "(undocumented IR pass)"
    }

    /// Run the pass on the given IR context.
    fn run(&self, context: &mut IRContext);
}
