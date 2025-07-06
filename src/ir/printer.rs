// CLASSIFICATION: COMMUNITY
// Filename: printer.rs v1.1
// Author: Lukas Bower
// Date Modified: 2027-08-11

/// Textual IR printer producing a simple textual representation.
use crate::ir::{Function, Module};
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use core::fmt::Write;

/// Print a single function to a string.
pub fn print_function(func: &Function) -> String {
    func.to_string()
}

/// Print an entire module to a string.
pub fn print_module(module: &Module) -> String {
    let mut out = String::new();
    writeln!(&mut out, "Module {}", module.name).unwrap();
    for func in &module.functions {
        writeln!(&mut out, "{}", func).unwrap();
    }
    out
}
