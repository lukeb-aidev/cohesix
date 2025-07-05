// CLASSIFICATION: COMMUNITY
// Filename: link.ld v0.1
// Author: Lukas Bower
// Date Modified: 2027-01-15
/* SPDX-License-Identifier: BSD-2-Clause */
SECTIONS {
    . = 0xffffff8040000000;
    .text : { *(.text*) }
    .rodata : { *(.rodata*) }
    .data : { *(.data*) }
    .bss : { *(.bss*) }
}
