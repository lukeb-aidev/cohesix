// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Date Modified: 2025-05-26
// Author: Lukas Bower

//! Intermediate Representation (IR) root module for the Cohesix compiler.

pub mod instruction;
pub mod ops;
pub mod module;

pub use instruction::{Instruction, Opcode};
pub use module::Module;
