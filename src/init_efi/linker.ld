/*
 * CLASSIFICATION: COMMUNITY
 * Filename: linker.ld v0.3
 * Author: Lukas Bower
 * Date Modified: 2026-08-29
 * SPDX-License-Identifier: MIT
 */
OUTPUT_FORMAT("elf64-littleaarch64")
OUTPUT_ARCH(aarch64)
EFI_SUBSYSTEM = 0x0a;
ENTRY(_start)
SECTIONS {
  . = 0x100000;
  .text : {
    *(.text*)
  }
  .data : {
    *(.data*)
    *(.dynamic)
  }
  PROVIDE(_DYNAMIC = .);
  .bss : {
    *(.bss*)
  }
}
