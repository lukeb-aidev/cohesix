// CLASSIFICATION: COMMUNITY
// Filename: sel4_start.S v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-20

    .global _sel4_start
_sel4_start:
    call main
    jmp .
