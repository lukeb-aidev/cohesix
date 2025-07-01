// CLASSIFICATION: COMMUNITY
// Filename: block.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

use crate::prelude::*;
/// Block: sequence of instructions with single entry and terminator.

pub struct Block {
    pub label: String,
    pub instructions: Vec<crate::ir::instruction::Instruction>,
    pub terminator: Option<crate::ir::instruction::Instruction>,
    pub successors: Vec<String>,
}
