/* CLASSIFICATION: COMMUNITY */
/* Filename: link.ld v0.13 */
/* Author: Lukas Bower */
/* Date Modified: 2027-11-23 */
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

    . = ALIGN(0x1000);
    .text : {
        KEEP(*(.text*))
    } :text

    . = ALIGN(0x1000);
    .rodata : {
        KEEP(*(.rodata*))
    } :text

    . = ALIGN(0x1000);
    .data : {
        KEEP(*(.data*))
    } :data

    . = ALIGN(0x1000);
    .bss (NOLOAD) : {
        __bss_start = .;
        KEEP(*(.bss*))
        *(COMMON)
        __bss_end = .;
    } :data

    /* record end of loaded image for runtime */
    __image_end = .;

    /* dedicated heap region mapped inside the ELF image */
    /*
     * Heap is reserved only at runtime, not stored in the image. Mark as
     * NOLOAD so the section occupies virtual memory without consuming file
     * bytes, ensuring no overlap when max-page-size is enforced.
     */
    . = ALIGN(0x1000);
    .heap (NOLOAD) : {
        __heap_start = .;
        . = . + 0x80000; /* 512KB heap */
        __heap_end = .;
    } :data

    /* reserved stack for root task, mapped in RW segment */
    /*
     * Similar to the heap, the stack region is reserved in memory only. Using
     * NOLOAD keeps the ELF image compact and prevents file offset overlap with
     * debug sections when page alignment is strictly enforced.
     */
    . = ALIGN(0x1000);
    .stack (NOLOAD) : {
        __stack_start = .;
        . = . + 0x10000; /* 64KB stack */
        __stack_end = .;
    } :data
}
