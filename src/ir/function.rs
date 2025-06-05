// CLASSIFICATION: COMMUNITY
// Filename: function.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! Function IR node: owns instructions and metadata.

use crate::ir::instruction::Instruction;
use std::fmt;

#[derive(Clone, Debug, Default)]
pub struct Function {
    pub name: String,
    pub body: Vec<Instruction>,
}

impl Function {
    /// Create a new function with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Function {
            name: name.into(),
            body: Vec::new(),
        }
    }
}

impl fmt::Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "fn {}() {{", self.name)?;
        for instr in &self.body {
            writeln!(f, "  {}", instr)?;
        }
        write!(f, "}}")
    }
}
