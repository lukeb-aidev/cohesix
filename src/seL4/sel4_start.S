// CLASSIFICATION: COMMUNITY
// Filename: sel4_start.S v0.3
// Author: Lukas Bower
// Date Modified: 2026-11-21

    .global _sel4_start
_sel4_start:
    call main
    jmp .

    .global switch_to_user
switch_to_user:
#ifdef __aarch64__
    // x0 = entry point, x1 = user stack top
    msr    SP_EL0, x1
    msr    ELR_EL1, x0
    mov    x2, #0
    msr    SPSR_EL1, x2
    eret
#else
    // rdi = entry point, rsi = user stack top
    mov    $0x23, %ax
    mov    %ax, %ds
    mov    %ax, %es
    mov    %ax, %ss
    mov    %rsi, %rsp
    pushq  $0x23
    pushq  %rsi
    pushfq
    pushq  $0x1b
    pushq  %rdi
    iretq
#endif
