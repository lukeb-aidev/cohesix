// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Date Modified: 2025-05-26
// Author: Lukas Bower

//! Central module for the IR pass framework in the Cohesix compiler.

pub mod ir_pass_framework;
pub mod manager;
pub mod traits;

pub use manager::PassManager;
pub use traits::IRPass;
