// CLASSIFICATION: PRIVATE
// Filename: mod.rs v0.7
// Date Modified: 2026-11-23
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix Hardware Abstraction Layer – x86‑64 implementation
//
// Provides early‑boot paging and interrupt setup on 64‑bit
// Intel/AMD platforms. The initial implementation maps the kernel
// identity and enables long mode paging. APIC configuration will be
// expanded in a future hydration pass.
//
// ## Public API
// * [`init_paging`]     – allocate page tables and enable paging.
// * [`init_interrupts`] – initialise Local APIC / IO‑APIC.
//
// Functions log actions and return `Ok(())` on success.
// ─────────────────────────────────────────────────────────────

#![allow(unsafe_code)]
#![warn(missing_docs)]

use log::{debug, info};

/// Allocate page tables and enable paging.
///
/// The kernel is identity-mapped so early boot code can run with paging enabled
/// without relocating pointers.
#[cfg(target_os = "none")]
pub fn init_paging() -> Result<(), &'static str> {
    use core::arch::asm;

    #[repr(align(4096))]
    struct Table([u64; 512]);

    static mut PML4: Table = Table([0; 512]);
    static mut PDPTE: Table = Table([0; 512]);
    static mut PDE: Table = Table([0; 512]);
    const EMPTY: Table = Table([0; 512]);
    static mut PT: [Table; 8] = [EMPTY; 8];

    unsafe {
        // Identity-map 0x0000_0000..0x0100_0000 (16 MiB)
        for tbl in 0..8 {
            for i in 0..512 {
                PT[tbl].0[i] = ((tbl as u64 * 0x200000) + (i as u64 * 0x1000)) | 0b11;
            }
            PDE.0[tbl] = (&PT[tbl] as *const _ as u64) | 0b11;
        }
        PDPTE.0[0] = (&PDE as *const _ as u64) | 0b11;
        PML4.0[0] = (&PDPTE as *const _ as u64) | 0b11;

        debug!("HAL/x86_64: page tables created");

        // Load the PML4 into CR3.
        asm!("mov cr3, {}", in(reg) &PML4 as *const _ as u64);

        // Enable PAE via CR4.PAE
        let mut cr4: u64;
        asm!("mov {}, cr4", out(reg) cr4);
        cr4 |= 1 << 5;
        asm!("mov cr4, {}", in(reg) cr4);

        // Enable paging via CR0.PG
        let mut cr0: u64;
        asm!("mov {}, cr0", out(reg) cr0);
        cr0 |= 1 << 31;
        asm!("mov cr0, {}", in(reg) cr0);
    }

    info!("[HAL] Mapping 0x0 - 0x1000000 (identity)");
    info!("[HAL] Paging enabled on x86_64");
    Ok(())
}

/// Compiles only on bare-metal (target_os = "none"), safe stub otherwise.
#[cfg(not(target_os = "none"))]
pub fn init_paging() -> Result<(), &'static str> {
    panic!("init_paging attempted on non-bare-metal target");
}

/// Configure LAPIC / IO‑APIC for early boot.
///
/// Returns `Ok(())` so higher layers can link successfully.
pub fn init_interrupts() -> Result<(), &'static str> {
    debug!("HAL/x86_64: init_interrupts() stub – no‑op");
    Ok(())
}

#[cfg(all(test, target_os = "none"))]
// Disabled under cargo test: hardware MMU instructions
mod tests {
    use super::*;

    #[test]
    fn stubs_return_ok() {
        init_paging().unwrap();
        init_interrupts().unwrap();
    }
}