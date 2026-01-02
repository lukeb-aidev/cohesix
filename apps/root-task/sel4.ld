/* Author: Lukas Bower */
/* Purpose: Linker script for the AArch64 root-task image layout (single PT_LOAD runs). */
/* Minimal AArch64 seL4 rootserver linker script (no jumps inside PT_LOAD) */
OUTPUT_FORMAT(elf64-littleaarch64)
OUTPUT_ARCH(aarch64)
ENTRY(_start)

HEAP_SIZE = 0x00040000;      /* 256 KiB bootstrap heap. */
__stack_guard = 0x00002000;  /* Guard page to catch stack overruns. */
__stack_size = 0x00020000;   /* 128 KiB stack for early boot logging/tracing. */

PHDRS {
  text PT_LOAD FLAGS(5); /* R+X */
  data PT_LOAD FLAGS(6); /* R+W */
}

SECTIONS
{
  /* Code / RO in one contiguous RX segment */
  . = ALIGN(0x1000);
  . = ALIGN(16);
  PROVIDE(_text = .);
  PROVIDE(__text_start = .);
  .text : {
    KEEP(*(.text._start))
    KEEP(*(.text.sel4_start))    /* entry */
    *(.text .text.*)
  } :text

  .rodata : {
    *(.rodata .rodata.*)
    *(.gnu.linkonce.r.*)
    *(.eh_frame*)
  } :text
  PROVIDE(__text_end = .);
  PROVIDE(__rodata_end = .);

  /* RW + BSS in one contiguous RW segment */
  . = ALIGN(0x1000);
  .data : {
    *(.data .data.*)
    *(.gnu.linkonce.d.*)
  } :data
  PROVIDE(__data_end = .);

  . = ALIGN(8);
  __bss_start__ = .;
  .bss (NOLOAD) : {
    *(.bss .bss.*)
    *(.sbss .sbss.*)
    *(COMMON)
  } :data
  __bss_end__ = .;
  PROVIDE(__bss_end = .);

  /* Dedicated bootstrap heap carved after .bss to avoid overlap with the stack. */
  . = ALIGN(0x1000);
  .heap (NOLOAD) : ALIGN(16) {
    __heap_start = .;
    . += HEAP_SIZE;
    __heap_end = .;
  } :data

  /* Statically reserved stack region; the runtime uses __stack_top as the initial SP. */
  . = ALIGN(0x1000);
  .stack (NOLOAD) : ALIGN(16) {
    __stack_bottom = .;
    __stack_start = .;
    . += __stack_size;
    __stack_top = .;
    __stack_end = .;
  } :data

  /*
  * Leave a guard gap below the IPC buffer to avoid stack overflows corrupting
  * the kernel-provided IPC buffer page (and the bootinfo frame that follows
  * it). The init thread stack must never run right up against the IPC buffer
  * mapping.
  */
  . = ALIGN(0x1000);
  .stack_guard (NOLOAD) : ALIGN(16) {
    __stack_guard_start = .;
    . += __stack_guard;
    __stack_guard_end = .;
    __ipcbuf_vaddr = .;
  } :data

  PROVIDE(_end = .);

  ASSERT(__heap_end <= __stack_bottom, "heap overlaps stack")
  ASSERT(__stack_top <= __stack_guard_start, "stack guard misplaced")

  /* Do NOT advance '.' to device/MMIO addresses here. Keep segments tight. */
}
