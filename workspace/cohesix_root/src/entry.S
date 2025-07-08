// CLASSIFICATION: COMMUNITY
// Filename: entry.S v0.12
// Author: Lukas Bower
// Date Modified: 2027-10-20

    .global _sel4_start
    .extern __stack_end
    .extern __bss_start
    .extern __bss_end
_sel4_start:
    // zero all registers on entry
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
    mov sp, x0
    // establish stack after wipe
    ldr x0, =__stack_end
    mov sp, x0
    mov x29, sp
    // DEBUG: log registers after stack setup
    mov x0, sp
    bl boot_log_hex
    mov x0, x29
    bl boot_log_hex
    mov x0, x1
    bl boot_log_hex
    mov x0, x2
    bl boot_log_hex
    mov x0, x3
    bl boot_log_hex
    mov x0, x4
    bl boot_log_hex
    mov x0, x5
    bl boot_log_hex
    mov x0, x6
    bl boot_log_hex
    mov x0, x7
    bl boot_log_hex
    mov x0, x8
    bl boot_log_hex
    mov x0, x9
    bl boot_log_hex
    mov x0, x10
    bl boot_log_hex
    mov x0, x11
    bl boot_log_hex
    mov x0, x12
    bl boot_log_hex
    mov x0, x13
    bl boot_log_hex
    mov x0, x14
    bl boot_log_hex
    mov x0, x15
    bl boot_log_hex
    mov x0, x30
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
    dsb ish
    isb
    // clear general-purpose registers before jumping to Rust
    mov x0, #0
    mov x1, #0
    mov x2, #0
    mov x3, #0
    mov x4, #0
    mov x5, #0
    mov x6, #0
    mov x7, #0
    mov x8, #0
    mov x9, #0
    mov x10, #0
    mov x11, #0
    mov x12, #0
    mov x13, #0
    mov x14, #0
    mov x15, #0
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
    mov x30, xzr
    // Re-establish stack pointer after register wipe
    ldr x0, =__stack_end
    mov sp, x0
    mov x29, sp
    // DEBUG: manual read of .rodata string to validate mapping
    ldr x1, =rodata_check_str
    bl boot_log_hex       // log address
    ldr x0, [x1]
    bl boot_log_hex       // log first 8 bytes
    mov x0, #0
    bl boot_log_hex       // ensure zero is loggable
    bl main
    b   .

    // DEBUG: known .rodata string for MMU audit
    .section .rodata
rodata_check_str:
    .asciz "RODATA_OK"
    .align 8
