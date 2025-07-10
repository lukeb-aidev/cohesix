// CLASSIFICATION: COMMUNITY
// Filename: entry.S v0.20
// Author: Lukas Bower
// Date Modified: 2027-12-26

    .global _start
    .extern __stack_end
    .extern __bss_start
    .extern __bss_end
_start:
    // Load stack pointer and clear BSS
    adrp x0, __stack_end
    add  x0, x0, :lo12:__stack_end
    mov  sp, x0
    mov  x29, sp

    adrp x0, __bss_start
    add  x0, x0, :lo12:__bss_start
    adrp x1, __bss_end
    add  x1, x1, :lo12:__bss_end
1:
    cmp  x0, x1
    b.ge  2f
    str  xzr, [x0], #8
    b    1b
2:
    dsb  sy
    isb

    bl   main
    b    .

    .section .rodata
rodata_check_str:
    .asciz "RODATA_OK"
    .align 8
