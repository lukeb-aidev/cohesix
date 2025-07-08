// CLASSIFICATION: COMMUNITY
// Filename: entry.S v0.18
// Author: Lukas Bower
// Date Modified: 2027-10-31

    .global _sel4_start
    .extern __stack_end
    .extern __bss_start
    .extern __bss_end
_sel4_start:
    // BootInfo arrives in x0 but is unused. Load stack end reliably.
    adrp x1, __stack_end
    add x1, x1, :lo12:__stack_end
    mov sp, x1
    mov x29, sp

    // Preserve callee-saved registers to ensure clean state
    stp x29, x30, [sp, #-16]!
    stp x27, x28, [sp, #-16]!
    stp x25, x26, [sp, #-16]!
    stp x23, x24, [sp, #-16]!
    stp x21, x22, [sp, #-16]!
    stp x19, x20, [sp, #-16]!

    // Zero BSS using x1-x2 so x0 stays intact
    adrp x1, __bss_start
    add x1, x1, :lo12:__bss_start
    adrp x2, __bss_end
    add x2, x2, :lo12:__bss_end
1:
    cmp x1, x2
    b.ge 2f
    str xzr, [x1], #8
    b 1b
2:
    dsb sy
    isb

    // Clear all general-purpose registers to avoid stale values
    mov x0, xzr
    mov x1, xzr
    mov x2, xzr
    mov x3, xzr
    mov x4, xzr
    mov x5, xzr
    mov x6, xzr
    mov x7, xzr
    mov x8, xzr
    mov x9, xzr
    mov x10, xzr
    mov x11, xzr
    mov x12, xzr
    mov x13, xzr
    mov x14, xzr
    mov x15, xzr
    mov x16, xzr
    mov x17, xzr
    mov x18, xzr
    mov x19, xzr
    mov x20, xzr
    mov x21, xzr
    mov x22, xzr
    mov x23, xzr
    mov x24, xzr
    mov x25, xzr
    mov x26, xzr
    mov x27, xzr
    mov x28, xzr
    mov x29, xzr
    mov x30, xzr

    // Reassert stack pointer after wiping registers
    adrp x1, __stack_end
    add x1, x1, :lo12:__stack_end
    mov sp, x1
    mov x29, sp

    // Clear potential boot arg residue from argument registers
    mov x0, xzr
    mov x1, xzr
    mov x2, xzr
    mov x3, xzr

    // Zero argument registers once more for safety
    mov x0, xzr
    mov x1, xzr
    mov x2, xzr
    mov x3, xzr

    bl main

    // Restore registers in case main ever returns (should not happen)
    ldp x19, x20, [sp], #16
    ldp x21, x22, [sp], #16
    ldp x23, x24, [sp], #16
    ldp x25, x26, [sp], #16
    ldp x27, x28, [sp], #16
    ldp x29, x30, [sp], #16
    b .

    .section .rodata
rodata_check_str:
    .asciz "RODATA_OK"
    .align 8
