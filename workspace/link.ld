/* CLASSIFICATION: COMMUNITY */
/* Filename: link.ld v0.5 */
/* Author: Lukas Bower */
/* Date Modified: 2025-07-07 */
/* SPDX-License-Identifier: BSD-2-Clause */

ENTRY(_start)

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
        KEEP(*(.bss*))
        *(COMMON)
    } :data :NONE

    /* dedicated heap region mapped inside the ELF image */
    .heap : ALIGN(0x1000) {
        __heap_start = .;
        . = . + 0x100000; /* 1MB heap */
        __heap_end = .;
    } :data
}
