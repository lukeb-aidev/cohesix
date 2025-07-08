// CLASSIFICATION: COMMUNITY
// Filename: sel4_start.S v0.5
// Author: Lukas Bower
// Date Modified: 2027-10-07

    .global _sel4_start
    .extern __stack_end
    .extern __bss_start
    .extern __bss_end
_sel4_start:
    ldr x0, =__stack_end
    mov sp, x0
    // Zero BSS before jumping into Rust
    ldr x0, =__bss_start
    ldr x1, =__bss_end
1:
    cmp x0, x1
    b.ge 2f
    str xzr, [x0], #8
    b   1b
2:
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
