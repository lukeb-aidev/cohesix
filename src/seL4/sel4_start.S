// CLASSIFICATION: COMMUNITY
// Filename: sel4_start.S v0.2
// Author: Lukas Bower
// Date Modified: 2026-11-20

    .global _sel4_start
_sel4_start:
    call main
    jmp .

    .global switch_to_user
switch_to_user:
#ifdef __aarch64__
    // x0 = entry point, x1 = stack pointer
    msr    SP_EL0, x1
    msr    ELR_EL1, x0
    mov    x2, #0
    msr    SPSR_EL1, x2
    eret
#else
    // rdi = entry point, rsi = stack pointer
    mov    $0x2b, %ax
    mov    %ax, %ds
    mov    %ax, %es
    mov    %ax, %ss
    mov    %rsi, %rsp
    pushq  $0x2b
    pushq  %rsi
    pushfq
    pushq  $0x33
    pushq  %rdi
    iretq
#endif
