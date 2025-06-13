// CLASSIFICATION: COMMUNITY
// Filename: linker.ld v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-22
/* SPDX-License-Identifier: BSD-2-Clause */
OUTPUT_FORMAT("pei-x86-64")
ENTRY(efi_main)
SECTIONS {
  .text : { *(.text*) }
  .data : { *(.data*) }
  .reloc : { *(.reloc) }
}
