// CLASSIFICATION: COMMUNITY
// Filename: entry.S v0.8
// Author: Lukas Bower
// Date Modified: 2027-10-13

    .global _sel4_start
    .extern __stack_end
    .extern __bss_start
    .extern __bss_end
_sel4_start:
    ldr x0, =__stack_end
    mov sp, x0
    mov x29, sp
    mov x0, sp
    bl boot_log_hex
    mov x0, x29
    bl boot_log_hex
    // Zero BSS
    ldr x0, =__bss_start
    ldr x1, =__bss_end
1:
    cmp x0, x1
    b.ge 2f
    str xzr, [x0], #8
    b   1b
2:
    bl main
    b   .
