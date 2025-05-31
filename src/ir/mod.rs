// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! Intermediate Representation (IR) root module for the Cohesix compiler.
//! Re-exports all IR-related components including instructions, functions, and context management.

pub mod instruction;
pub mod ops;
pub mod module;
pub mod function;
pub mod context;

pub use instruction::{Instruction, Opcode};
pub use module::Module;
pub use function::Function;
pub use context::IRContext;
