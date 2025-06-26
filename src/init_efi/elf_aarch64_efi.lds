// CLASSIFICATION: COMMUNITY
// Filename: elf_aarch64_efi.lds v0.2
// Author: Lukas Bower
// Date Modified: 2026-09-09
/* SPDX-License-Identifier: MIT */
OUTPUT_FORMAT("elf64-littleaarch64")
ENTRY(_start)
SECTIONS {
  . = 0;
  .text : { *(.text) }
  .data : { *(.data) }
  .bss  : { *(.bss COMMON) }
  /DISCARD/ : { *(.note.GNU-stack) }
}
