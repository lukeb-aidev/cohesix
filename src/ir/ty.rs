// CLASSIFICATION: COMMUNITY
// Filename: ty.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

use crate::prelude::*;
//! IR type system: integer, pointer, function types, etc.

pub enum IRType {
    Int32,
    Ptr(Box<IRType>),
    Function(Vec<IRType>, Box<IRType>),
}
