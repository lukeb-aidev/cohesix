/* CLASSIFICATION: COMMUNITY */
/* Filename: link.ld v0.17 */
/* Author: Lukas Bower */
/* Date Modified: 2027-12-27 */
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

    . = ALIGN(0x1000);
    .text : {
        KEEP(*(.text*))
    } :text
    __text_end = .;

    /* WHY: ensure .rodata begins on a new 4KiB page after .text */
    . = ALIGN(0x1000);
    . += 0x1000;
    .rodata : {
        KEEP(*(.rodata*))
    } :text
    __rodata_end = .;

    /* WHY: compute end of loaded image before starting the heap */
    . = ALIGN(0x1000);
    __image_end = .;

    /* WHY: keep .data on its own aligned page following __image_end */
    . = ALIGN(0x1000);
    . += 0x1000;
    .data : {
        KEEP(*(.data*))
    } :data
    __data_end = .;

    /* WHY: bss follows data on a new aligned page */
    . = ALIGN(0x1000);
    . += 0x1000;
    .bss (NOLOAD) : {
        __bss_start = .;
        KEEP(*(.bss*))
        *(COMMON)
        __bss_end = .;
    } :data


    /* dedicated heap region mapped inside the ELF image */
    /*
     * Heap is reserved only at runtime, not stored in the image. Mark as
     * NOLOAD so the section occupies virtual memory without consuming file
     * bytes. We bump the counter after alignment to guarantee separation from
     * preceding sections when they are empty.
     */
    . = ALIGN(0x1000);
    . += 0x1000; /* WHY: avoid overlap with loaded image */
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
    . += 0x1000; /* WHY: keep stack on a separate page */
    .stack (NOLOAD) : {
        __stack_start = .;
        . = . + 0x10000; /* 64KB stack */
        __stack_end = .;
    } :data
}
