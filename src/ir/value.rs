// CLASSIFICATION: COMMUNITY
// Filename: value.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Value represents a SSA result or named constant in the IR.
pub struct Value {
    pub id: usize,
    pub ty: crate::ir::ty::IRType,
    pub origin: Option<usize>, // Instruction ID that produced this value
    pub name: Option<String>,
}
