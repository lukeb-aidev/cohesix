// CLASSIFICATION: COMMUNITY
// Filename: entry.S v0.22
// Author: Lukas Bower
// Date Modified: 2028-08-30

    .global _start
    .extern __stack_end
    .extern __bss_start
    .extern __bss_end
    .extern copy_bootinfo
_start:
    // Load stack pointer and clear BSS
    mov  x2, x0
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

    mov  x0, x2
    bl   copy_bootinfo

    bl   rust_entry
    b    .

    .section .rodata
rodata_check_str:
    .asciz "RODATA_OK"
    .align 8
