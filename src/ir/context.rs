// CLASSIFICATION: COMMUNITY
// Filename: context.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

/// IRContext provides stateful context for IR construction and analysis.
use crate::ir::module::Module;
use crate::prelude::*;

#[derive(Default)]
pub struct IRContext {
    pub modules: Vec<Module>,
}
