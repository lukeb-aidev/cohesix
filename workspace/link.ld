/* CLASSIFICATION: COMMUNITY */
/* Filename: link.ld v0.11 */
/* Author: Lukas Bower */
/* Date Modified: 2027-11-05 */
/* SPDX-License-Identifier: BSD-2-Clause */

ENTRY(_sel4_start)

/*
 * Explicit program header mapping with clear flags:
 * text: R+X (5), data: R+W (6)
 * BSS marked NOLOAD since it's zero-initialized at runtime.
 */

PHDRS {
    text PT_LOAD FLAGS(5); /* R+X */
    data PT_LOAD FLAGS(6); /* R+W */
}

SECTIONS {
    . = 0xffffff8040000000;

    .text : ALIGN(0x1000) {
        KEEP(*(.text*))
    } :text

    .rodata : ALIGN(0x1000) {
        KEEP(*(.rodata*))
    } :text

    .data : ALIGN(0x1000) {
        KEEP(*(.data*))
    } :data

    .bss : ALIGN(0x1000) {
        __bss_start = .;
        KEEP(*(.bss*))
        *(COMMON)
        __bss_end = .;
    } :data :NONE

    /* dedicated heap region mapped inside the ELF image */
    .heap : ALIGN(0x1000) {
        __heap_start = .;
        . = . + 0x80000; /* 512KB heap */
        __heap_end = .;
    } :data

    /* reserved stack for root task, mapped in RW segment */
    .stack : ALIGN(0x1000) {
        __stack_start = .;
        . = . + 0x10000; /* 64KB stack */
        __stack_end = .;
    } :data
}
