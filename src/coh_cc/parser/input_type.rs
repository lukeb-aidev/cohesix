// CLASSIFICATION: COMMUNITY
// Filename: input_type.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-17

use crate::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputType {
    C,
    Rust,
    IR,
}

pub fn detect_input_type(path: &Path) -> InputType {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "rs" => InputType::Rust,
        "ir" => InputType::IR,
        _ => InputType::C,
    }
}

#[derive(Debug, Clone)]
pub struct CohInput {
    pub path: PathBuf,
    pub flags: Vec<String>,
    pub ty: InputType,
}

impl CohInput {
    pub fn new(path: PathBuf, flags: Vec<String>) -> Self {
        let ty = detect_input_type(&path);
        CohInput { path, flags, ty }
    }
}
