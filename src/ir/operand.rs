// CLASSIFICATION: COMMUNITY
// Filename: operand.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

use crate::prelude::*;
//! Operand enum: arguments to instructions (values, constants, etc.)

pub enum Operand {
    ValueRef(u32),
    Constant(i64),
    Label(String),
}
