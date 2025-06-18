// CLASSIFICATION: COMMUNITY
// Filename: loader.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-09-26

use anyhow::{Context, Result};
use std::fs::File;
use std::io::Read;

const MAGIC: &[u8; 4] = b"COHB";
const VERSION: u8 = 1;

/// Load a `cohcc` binary and simulate execution.
pub fn load_and_run(path: &str) -> Result<()> {
    let mut f = File::open(path).with_context(|| format!("open {path}"))?;
    let mut data = Vec::new();
    f.read_to_end(&mut data).context("read file")?;
    if data.len() < 5 {
        anyhow::bail!("file too small");
    }
    if &data[0..4] != MAGIC {
        anyhow::bail!("invalid magic header");
    }
    if data[4] != VERSION {
        anyhow::bail!("unsupported version {}", data[4]);
    }
    for opcode in &data[5..] {
        match opcode {
            0x01 => println!("[EXEC: ADD]"),
            0x02 => println!("[EXEC: PRINT]"),
            0xFF => {
                println!("[EXEC: END]");
                break;
            }
            other => println!("[EXEC: UNKNOWN 0x{other:02X}]"),
        }
    }
    Ok(())
}
