/*
 * CLASSIFICATION: COMMUNITY
 * Filename: linker.ld v0.2
 * Author: Lukas Bower
 * Date Modified: 2026-08-28
 * SPDX-License-Identifier: MIT
 */
ENTRY(_start)
SECTIONS {
  . = 0x100000;
  .text : {
    *(.text*)
  }
  .data : {
    *(.data*)
  }
  .bss : {
    *(.bss*)
  }
}
