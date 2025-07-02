// CLASSIFICATION: COMMUNITY
// Filename: loader.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-11-21

use crate::prelude::*;
use log::info;
use thiserror::Error;
use xmas_elf::{program::Type, ElfFile};

/// Execution context for a loaded ELF binary.
pub struct ProcessContext {
    pub entry_point: usize,
    pub stack_top: usize,
    pub page_table: Option<usize>,
    pub segments: Vec<MappedSegment>,
}

/// Information about a loaded memory segment.
pub struct MappedSegment {
    pub vaddr: usize,
    pub paddr: usize,
    pub size: usize,
    #[allow(dead_code)]
    data: Box<[u8]>,
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
    let mut segments = Vec::new();
    for ph in elf.program_iter() {
        if let Ok(Type::Load) = ph.get_type() {
            let mem_size = ph.mem_size() as usize;
            let file_size = ph.file_size() as usize;
            let vaddr = ph.virtual_addr() as usize;
            let offset = ph.offset() as usize;
            let mut mem = vec![0u8; mem_size];
            mem[..file_size].copy_from_slice(&data[offset..offset + file_size]);
            let paddr = mem.as_ptr() as usize;
            log_segment_map(vaddr, paddr, mem_size);
            segments.push(MappedSegment {
                vaddr,
                paddr,
                size: mem_size,
                data: mem.into_boxed_slice(),
            });
        }
    }
    let entry = elf.header.pt2.entry_point() as usize;
    let stack = vec![0u8; STACK_SIZE];
    let stack_top = stack.as_ptr() as usize + stack.len();
    std::mem::forget(stack);
    Ok(ProcessContext {
        entry_point: entry,
        stack_top,
        page_table: Some(segments.as_ptr() as usize),
        segments,
    })
}

fn log_segment_map(vaddr: usize, paddr: usize, size: usize) {
    info!("[Loader] Loaded segment virt=0x{vaddr:x} phys=0x{paddr:x} size=0x{size:x}");
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
