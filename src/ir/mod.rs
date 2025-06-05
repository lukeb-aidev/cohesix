// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! IR root module. Declares and organizes all IR submodules.

pub mod block;
pub mod context;
pub mod function;
pub mod instruction;
pub mod module;
pub mod operand;
pub mod ops;
pub mod ty;
pub mod value;

pub use context::IRContext;
pub use function::Function;
pub use instruction::{Instruction, Opcode};
pub use module::Module;
