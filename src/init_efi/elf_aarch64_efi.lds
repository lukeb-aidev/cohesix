// CLASSIFICATION: COMMUNITY
// Filename: elf_aarch64_efi.lds v0.5
// Author: Lukas Bower
// Date Modified: 2026-09-12
/* SPDX-License-Identifier: MIT */
/* Do not copy/paste from web sources—tabs are required for makefiles and scripts. */
ENTRY(_start)
OUTPUT_FORMAT("elf64-littleaarch64")
SECTIONS {
  . = 0;
  ImageBase = .;
  .text : { *(.text) }
  .data : { *(.data) }
  .bss  : {
    *(.bss*)
    *(COMMON)
  }
  /DISCARD/ : { *(.note.GNU-stack) }
}
