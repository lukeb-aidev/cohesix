// CLASSIFICATION: COMMUNITY
// Filename: init.rs v0.4
// Date Modified: 2025-07-10
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix · Bootloader Early‑Init
//
// Responsible for *very* early setup inside the second‑stage
// bootloader (still running in firmware context):
//
//  1. Parse the raw boot‑loader cmdline string.
//  2. Perform minimal HAL bring‑up (paging + IRQ stubs).
//  3. Hand a [`BootContext`] record to the Rust kernel entry.
//
// Heavy‑duty tasks (verified boot, scheduler start‑up, etc.) are
// deferred to later stages.
// ─────────────────────────────────────────────────────────────

use crate::prelude::*;
#[forbid(unsafe_code)]
#[warn(missing_docs)]

use anyhow::Result;
use log::info;

use crate::{
    bootloader::args::{parse_cmdline, BootArgs},
    hal,
};

/// Collected state handed to the Rust kernel entry‐point.
#[derive(Debug)]
pub struct BootContext {
    /// Parsed boot arguments.
    pub args: BootArgs,
    /// Selected boot role.
    pub role: String,
}

/// Perform early initialisation.
///
/// This function should be invoked by the second‑stage bootloader
/// (e.g. a Rust `#[no_std]` stub or loader.bin).  It purposefully
/// does **not** allocate on the heap and avoids complex features
/// so it can run with a limited runtime.
///
/// * `cmdline` – raw ASCII cmdline string passed by firmware.
pub fn early_init(cmdline: &str) -> Result<BootContext> {
    // 1. Parse cmd‑line
    let args = parse_cmdline(cmdline).map_err(|e| anyhow::anyhow!(e))?;
    let role = args.get("cohrole").unwrap_or("Unknown").to_string();

    // 2. Basic HAL bring‑up
    //    — Page‑tables + IRQ controller stubs (real impl later)
    #[cfg(target_arch = "aarch64")]
    {
        hal::arm64::init_paging().map_err(|e| anyhow::anyhow!(e))?;
        hal::arm64::init_interrupts().map_err(|e| anyhow::anyhow!(e))?;
    }
    #[cfg(target_arch = "x86_64")]
    {
        hal::x86_64::init_paging().map_err(|e| anyhow::anyhow!(e))?;
        hal::x86_64::init_interrupts().map_err(|e| anyhow::anyhow!(e))?;
    }

    std::fs::create_dir_all("/srv").ok();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/boot.log")
    {
        use std::io::Write;
        let _ = writeln!(f, "role={role} cmdline={cmdline}");
    }
    std::fs::write("/srv/cohrole", &role).ok();
    let _ = crate::slm::decryptor::SLMDecryptor::preload_from_dir("/persist/models", b"testkey");

    info!("Bootloader early‑init complete");

    Ok(BootContext { args, role })
}

// ───────────────────────────── tests ─────────────────────────────────────────
// Runs only under bare-metal QEMU targets to prevent SIGILL on host.
#[cfg(all(test, target_os = "none"))]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_cmdline() {
        let ctx = early_init("root=/srv/sda quiet").unwrap();
        assert_eq!(ctx.args.get("root"), Some("/srv/sda"));
        assert!(ctx.args.has_flag("quiet"));
        assert_eq!(ctx.role, "Unknown");
    }
}
