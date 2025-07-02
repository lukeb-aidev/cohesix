// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Date Modified: 2025-05-26
// Author: Lukas Bower

/// Entry point for all IR passes in the Cohesix compiler.
pub mod const_fold;
pub mod deadcode;
pub mod nop;
pub mod ssa;

pub use const_fold::ConstFold;
pub use deadcode::DeadCode;
pub use nop::NopPass;
pub use ssa::SsaPass;
