// Author: Lukas Bower
#![allow(dead_code)]

/// Capability-space helpers extracted from the seL4 boot info structure.
pub mod cspace;
/// ABI guard helpers ensuring the seL4 FFI signatures remain pinned.
pub mod ffi;
/// Thin wrapper around `seL4_Untyped_Retype` tailored for the init CSpace policy.
pub mod retype;
/// Helpers for selecting RAM-backed untyped capabilities during bootstrap.
pub mod untyped_pick;

pub use untyped_pick::pick_untyped;
