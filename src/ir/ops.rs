// CLASSIFICATION: COMMUNITY
// Filename: ops.rs v1.1
// Date Modified: 2025-07-24
// Author: Lukas Bower

/// Utility functions and constants for IR opcodes in the Cohesix compiler.
use crate::ir::Opcode;
use crate::prelude::*;

/// A slice containing every supported Opcode.
pub const ALL_OPCODES: &[Opcode] = &[
    Opcode::Nop,
    Opcode::Add,
    Opcode::Sub,
    Opcode::Mul,
    Opcode::Div,
    Opcode::Load,
    Opcode::Store,
    Opcode::Jump,
    Opcode::Branch {
        condition: String::new(),
    },
    Opcode::Call {
        function: String::new(),
    },
    Opcode::Ret,
];

/// Attempts to parse a string into an Opcode.
///
/// Matches case-insensitive names: "add", "Sub", "CALL", etc.
pub fn parse_opcode(name: &str) -> Option<Opcode> {
    match name.to_lowercase().as_str() {
        "nop" => Some(Opcode::Nop),
        "add" => Some(Opcode::Add),
        "sub" => Some(Opcode::Sub),
        "mul" => Some(Opcode::Mul),
        "div" => Some(Opcode::Div),
        "load" => Some(Opcode::Load),
        "store" => Some(Opcode::Store),
        "jump" => Some(Opcode::Jump),
        // For parametric opcodes, parsing must be handled upstream.
        _ => None,
    }
}

/// Returns a display‐friendly string for an Opcode.
pub fn opcode_to_string(op: &Opcode) -> String {
    format!("{:?}", op)
}

/// Represents a high-level category for grouping opcodes.
pub enum OpcodeCategory {
    Arithmetic,
    Memory,
    ControlFlow,
    Meta,
}

/// Returns the category of a given opcode.
pub fn categorize(op: &Opcode) -> OpcodeCategory {
    match op {
        Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div => OpcodeCategory::Arithmetic,
        Opcode::Load | Opcode::Store => OpcodeCategory::Memory,
        Opcode::Jump | Opcode::Branch { .. } | Opcode::Ret => OpcodeCategory::ControlFlow,
        Opcode::Nop | Opcode::Call { .. } => OpcodeCategory::Meta,
    }
}

/// Validate opcode correctness. Currently returns `true` until full
/// validation rules are implemented.
/// FIXME: Add opcode validation rules (e.g., operand arity, SSA form compatibility)
pub fn validate_opcode(_op: &Opcode) -> bool {
    true
}
