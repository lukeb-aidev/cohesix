// CLASSIFICATION: COMMUNITY
// Filename: loader.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-11-20

use log::info;
use thiserror::Error;
use xmas_elf::{
    program::{ProgramHeader, Type},
    ElfFile,
};

/// Execution context for a loaded ELF binary.
pub struct ProcessContext {
    pub entry_point: usize,
    pub stack_top: usize,
    pub page_table: Option<usize>,
}

#[derive(Error, Debug)]
pub enum LoaderError {
    #[error("failed to read {0}")]
    ReadError(String),
    #[error("ELF parse error")]
    ParseError,
}

const STACK_SIZE: usize = 0x4000;

/// Load a user ELF from disk and return a prepared `ProcessContext`.
pub fn load_user_elf(path: &str) -> Result<ProcessContext, LoaderError> {
    info!("Loading ELF from {path}");
    let data: Vec<u8> = {
        #[cfg(feature = "minimal_uefi")]
        {
            crate::kernel::fs::fat::open_bin(path)
                .map(|s| s.to_vec())
                .ok_or_else(|| LoaderError::ReadError(path.into()))?
        }
        #[cfg(not(feature = "minimal_uefi"))]
        {
            std::fs::read(path).map_err(|_| LoaderError::ReadError(path.into()))?
        }
    };

    let elf = ElfFile::new(&data).map_err(|_| LoaderError::ParseError)?;
    for ph in elf.program_iter() {
        if let Ok(Type::Load) = ph.get_type() {
            log_segment(&ph);
        }
    }
    let entry = elf.header.pt2.entry_point() as usize;
    let stack = vec![0u8; STACK_SIZE];
    let stack_top = stack.as_ptr() as usize + stack.len();
    std::mem::forget(stack);
    Ok(ProcessContext {
        entry_point: entry,
        stack_top,
        page_table: None,
    })
}

fn log_segment(ph: &ProgramHeader) {
    let vaddr = ph.virtual_addr();
    let mem_size = ph.mem_size();
    let flags = ph.flags();
    let mut flag_str = String::new();
    if flags.is_read() {
        flag_str.push('R');
    }
    if flags.is_write() {
        flag_str.push('W');
    }
    if flags.is_execute() {
        flag_str.push('X');
    }
    info!("Mapping segment at 0x{vaddr:x} size 0x{mem_size:x} flags {flag_str}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn load_minimal_elf() {
        let mut file = NamedTempFile::new().unwrap();
        let header = [
            0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        file.write_all(&header).unwrap();
        let ctx = load_user_elf(file.path().to_str().unwrap());
        assert!(ctx.is_ok());
    }
}
