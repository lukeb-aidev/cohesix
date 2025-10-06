/* CLASSIFICATION: COMMUNITY */
/* Filename: link.ld v0.24 */
/* Author: Lukas Bower */
/* Date Modified: 2028-08-30 */
/* SPDX-License-Identifier: BSD-2-Clause */

ENTRY(_start)

PHDRS {
    text PT_LOAD FLAGS(5);
    data PT_LOAD FLAGS(6);
    dev  PT_LOAD FLAGS(6);
}

SECTIONS {
    . = 0x400000;

    .vectors : ALIGN(0x1000) {
        KEEP(*(.vectors))
    } :text

    .text : ALIGN(0x1000) {
        KEEP(*(.text*))
    } :text
    __text_end = .;

    .rodata : ALIGN(0x1000) {
        KEEP(*(.rodata*))
    } :text
    __rodata_end = .;

    __image_end = ALIGN(0x1000);

    .data : ALIGN(0x1000) {
        KEEP(*(.data*))
    } :data
    __data_end = .;

    .bss (NOLOAD) : ALIGN(0x1000) {
        __bss_start = .;
        KEEP(*(.bss*))
        *(COMMON)
        __bss_end = .;
    } :data

    .heap (NOLOAD) : ALIGN(0x1000) {
        __heap_start = .;
        . = . + 0x80000;
        __heap_end = .;
    } :data

    .stack (NOLOAD) : ALIGN(0x1000) {
        __stack_start = .;
        . = . + 0x10000;
        __stack_end = .;
    } :data

    .uart (NOLOAD) : ALIGN(0x1000) {
        __uart_start = .;
        . = . + 0x1000;
        __uart_end = .;
    } :dev
}
