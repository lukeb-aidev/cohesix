

// CLASSIFICATION: COMMUNITY
// Filename: instruction.rs v1.0
// Date Modified: 2025-05-26
// Author: Lukas Bower

//! Defines the core IR instruction representation for the Cohesix compiler.

/// Represents an opcode in the intermediate representation.
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// A single IR instruction with its operands.
#[derive(Clone, Debug)]
pub struct Instruction {
    /// The operation code of this instruction.
    pub opcode: Opcode,
    /// String-encoded operand list. Interpretation depends on the opcode.
    pub operands: Vec<String>,
}

impl Default for Instruction {
    fn default() -> Self {
        Instruction {
            opcode: Opcode::Nop,
            operands: Vec::new(),
        }
    }
}

impl Instruction {
    /// Constructs a new instruction with the given opcode and operands.
    pub fn new(opcode: Opcode, operands: Vec<String>) -> Self {
        Instruction { opcode, operands }
    }

    /// Returns a human-readable representation of the instruction.
    pub fn to_string(&self) -> String {
        let ops = self.operands.join(", ");
        match &self.opcode {
            Opcode::Branch { condition } => format!("Branch {} if {}", ops, condition),
            Opcode::Call { function } => format!("Call {}({})", function, ops),
            other => format!("{:?} {}", other, ops),
        }
    }

    /// TODO: Add semantic validation, SSA checks, or side-effect tagging
}