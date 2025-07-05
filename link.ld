// CLASSIFICATION: COMMUNITY
// Filename: link.ld v0.2
// Author: Lukas Bower
// Date Modified: 2027-01-15
/* SPDX-License-Identifier: BSD-2-Clause */
SECTIONS {
    . = 0xffffff8040000000;

    .text : ALIGN(0x1000) {
        KEEP(*(.text*))
    }

    .rodata : ALIGN(0x1000) {
        KEEP(*(.rodata*))
    }

    .data : ALIGN(0x1000) {
        KEEP(*(.data*))
    }

    .bss : ALIGN(0x1000) {
        KEEP(*(.bss*))
        *(COMMON)
    }
}
