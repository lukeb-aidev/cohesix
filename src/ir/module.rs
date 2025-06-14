// CLASSIFICATION: COMMUNITY
// Filename: module.rs v1.1
// Date Modified: 2025-07-24
// Author: Lukas Bower

//! Defines the IR Module and associated utilities for the Cohesix compiler.

use crate::ir::Function;

/// A compilation unit containing multiple functions.
#[derive(Clone, Debug)]
pub struct Module {
    /// The name of this module (e.g., filename or identifier).
    pub name: String,
    /// Ordered list of functions within this module.
    pub functions: Vec<Function>,
    /// Optional metadata associated with this module.
    pub metadata: Option<String>, // FIXME: replace with structured metadata when ready
}

impl Module {
    /// Creates a new empty Module with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Module {
            name: name.into(),
            functions: Vec::new(),
            metadata: None,
        }
    }

    /// Adds a function to the module.
    pub fn add_function(&mut self, func: Function) {
        self.functions.push(func);
    }

    /// Finds a function by name, returning a mutable reference if found.
    pub fn get_function_mut(&mut self, name: &str) -> Option<&mut Function> {
        self.functions.iter_mut().find(|f| f.name == name)
    }

    /// Returns an iterator over function references.
    pub fn functions(&self) -> impl Iterator<Item = &Function> {
        self.functions.iter()
    }

    /// Pretty-prints the module and its functions.
    pub fn to_string(&self) -> String {
        let mut out = format!("Module: {}\n", self.name);
        for func in &self.functions {
            out.push_str(&format!("{}", func)); // assumes Function has Display impl
        }
        out
    }

    /// FIXME: Validate structural integrity, uniqueness of function names, etc.
    pub fn validate(&self) -> bool {
        true // stub: always valid for now
    }

    /// Return a textual representation of the module using the IR printer.
    pub fn print_ir(&self) -> String {
        crate::ir::printer::print_module(self)
    }
}
