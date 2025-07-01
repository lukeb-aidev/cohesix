// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

use crate::prelude::*;
/// Entry point for IR pass framework.

pub mod ir_pass_framework;
pub mod manager;
pub mod traits;

pub use manager::PassManager;
pub use traits::IRPass;
