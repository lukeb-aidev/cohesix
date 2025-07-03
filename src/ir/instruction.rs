// CLASSIFICATION: COMMUNITY
// Filename: instruction.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Instruction representation for the Cohesix IR.
use std::fmt;

/// Low-level operation codes supported by the IR.
#[derive(Clone, Debug, PartialEq)]
pub enum Opcode {
    Nop,
    Add,
    Sub,
    Mul,
    Div,
    Load,
    Store,
    Jump,
    Branch { condition: String },
    Call { function: String },
    Ret,
}

/// Basic instruction consisting of an opcode and optional operands.
#[derive(Clone, Debug)]
pub struct Instruction {
    pub opcode: Opcode,
    pub operands: Vec<String>,
}

impl Instruction {
    /// Creates a new instruction with the given opcode and operands.
    pub fn new(opcode: Opcode, operands: Vec<String>) -> Self {
        Instruction { opcode, operands }
    }
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} {:?}", self.opcode, self.operands)
    }
}
