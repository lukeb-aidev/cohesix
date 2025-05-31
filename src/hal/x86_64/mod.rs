// CLASSIFICATION: PRIVATE
// Filename: mod.rs · HAL x86_64
// Date Modified: 2025-05-31
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix Hardware Abstraction Layer – x86‑64 implementation
//
// Compilable stubs for early‑boot paging and interrupt setup on
// 64‑bit Intel/AMD platforms.  Real mode/long mode transitions
// and APIC configuration will be added during a future hydration
// pass once the final PC-class target is confirmed.
//
// ## Public API
// * [`init_paging`]     – map minimal identity & higher‑half pages.
// * [`init_interrupts`] – initialise Local APIC / IO‑APIC.
//
// All functions currently log a debug message and return `Ok(())`.
// ─────────────────────────────────────────────────────────────

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use log::debug;

/// Set up basic page tables for long mode.
///
/// Returns `Ok(())` so higher layers can link successfully.
pub fn init_paging() -> Result<(), &'static str> {
    debug!("HAL/x86_64: init_paging() stub – no‑op");
    Ok(())
}

/// Configure LAPIC / IO‑APIC for early boot.
///
/// Returns `Ok(())` so higher layers can link successfully.
pub fn init_interrupts() -> Result<(), &'static str> {
    debug!("HAL/x86_64: init_interrupts() stub – no‑op");
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