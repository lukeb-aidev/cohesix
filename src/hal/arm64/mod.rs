// CLASSIFICATION: PRIVATE
// Filename: mod.rs · HAL arm64
// Date Modified: 2025-05-31
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix Hardware Abstraction Layer – ARM64 implementation
//
// This module provides *compilable* stubs for early‐boot MMU and
// interrupt‑controller initialisation on 64‑bit ARM platforms.
// The real low‑level code will be added once the target SoC
// (e.g. Jetson Orin, Raspberry Pi 5) is finalised.
//
// ## Public API
// * [`init_paging`]       – set up basic page tables.
// * [`init_interrupts`]   – configure GICv3/LPI or SoC‑specific PIC.
//
// All functions currently log a debug message and return `Ok(())` so
// that higher layers can link successfully.
// ─────────────────────────────────────────────────────────────

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use log::debug;

/// Initialise MMU & basic paging tables.
///
/// Returned `Ok(())` indicates the stub executed; real hardware
/// changes will be introduced in a future hydration pass.
pub fn init_paging() -> Result<(), &'static str> {
    debug!("HAL/arm64: init_paging() stub – no‑op");
    Ok(())
}

/// Configure the interrupt controller (GIC/PIC) for early boot.
///
/// Returned `Ok(())` indicates the stub executed; real hardware
/// changes will be introduced in a future hydration pass.
pub fn init_interrupts() -> Result<(), &'static str> {
    debug!("HAL/arm64: init_interrupts() stub – no‑op");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stubs_return_ok() {
        init_paging().unwrap();
        init_interrupts().unwrap();
    }
}
