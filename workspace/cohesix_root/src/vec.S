// CLASSIFICATION: COMMUNITY
// Filename: vec.S v0.1
// Author: Lukas Bower
// Date Modified: 2028-01-20

    .section .vectors, "ax"
    .align 11
    .global vectors_start
vectors_start:
    b handle_el1_sync
    .org 0x80
    b handle_el1_irq
    .org 0x100
    b handle_el1_fiq
    .org 0x180
    b handle_el1_serror
    .org 0x200
    b handle_el1_sync_sp0
    .org 0x280
    b handle_el1_irq_sp0
    .org 0x300
    b handle_el1_fiq_sp0
    .org 0x380
    b handle_el1_serror_sp0
    .org 0x400
    b handle_el0_sync
    .org 0x480
    b handle_el0_irq
    .org 0x500
    b handle_el0_fiq
    .org 0x580
    b handle_el0_serror
    .org 0x600
    b handle_el0_32_sync
    .org 0x680
    b handle_el0_32_irq
    .org 0x700
    b handle_el0_32_fiq
    .org 0x780
    b handle_el0_32_serror

