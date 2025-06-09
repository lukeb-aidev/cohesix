// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-17

use std::str::FromStr;

pub mod zig;
pub mod tcc;
pub mod cranelift;

#[derive(Debug, Clone, Copy)]
pub enum Backend { Tcc, Zig, Cranelift }

impl FromStr for Backend {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tcc" => Ok(Backend::Tcc),
            "zig" => Ok(Backend::Zig),
            "cranelift" => Ok(Backend::Cranelift),
            other => Err(anyhow::anyhow!("unknown backend {other}")),
        }
    }
}

pub fn compile_backend(source: &str, out: &str, flags: &[String], backend: Backend) -> anyhow::Result<()> {
    match backend {
        Backend::Tcc => tcc::compile_and_link(source, out, flags),
        Backend::Zig => zig::compile_and_link(source, out, flags),
        Backend::Cranelift => cranelift::compile_and_link(source, out, flags),
    }
}

