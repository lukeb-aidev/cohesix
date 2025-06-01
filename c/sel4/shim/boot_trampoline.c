// CLASSIFICATION: PRIVATE
// Filename: boot_trampoline.c v0.2
// Date Modified: 2025-06-01
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix · seL4 Boot Trampoline (C)
// 
// This minimal C function is invoked by the verified seL4 assembly
// start‑up code.  It performs **no** dynamic allocation, assumes an
// already‑valid stack, and immediately tail‑calls the Rust
// `rust_early_init()` symbol exported by the second‑stage loader.
//
// Any register or MMU manipulation must be done in assembly prior to
// calling this trampoline; doing it here would break the seL4 proof
// assumptions.
//
// Compile with  `-ffreestanding -fno-builtin` and link early in the
// boot image so the symbol is easily discoverable.
// ─────────────────────────────────────────────────────────────

#include "boot_trampoline.h"

void boot_trampoline(void)
{
    extern void rust_early_init(void);
    rust_early_init();
    /* No return expected. If Rust returned, spin forever. */
    for (;;)
        ;
}