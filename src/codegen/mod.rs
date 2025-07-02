// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Date Modified: 2025-05-27
// Author: Lukas Bower

/// Codegen module for the Coh_CC compiler. Exposes supported backends and dispatch functionality.
pub mod c;
pub mod debug;
pub mod dispatch;
pub mod wasm;

pub use c::generate_c;
pub use debug::generate_debug;
pub use dispatch::{dispatch, infer_backend_from_path, Backend};
pub use wasm::generate_wasm;
