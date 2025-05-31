// CLASSIFICATION: COMMUNITY
// Filename: function.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! Defines the Function struct, representing a named sequence of IR instructions.
//! Functions serve as units of computation within a module.

use crate::ir::instruction::Instruction;

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub body: Vec<Instruction>,
}

impl std::fmt::Display for Function {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl Function {
    /// Create a new function with a given name and an empty body.
    pub fn new(name: String) -> Self {
        Function {
            name,
            body: Vec::new(),
        }
    }

    /// TODO: Implement control flow analysis or instruction validation
}
