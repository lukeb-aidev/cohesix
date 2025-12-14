/* Author: Lukas Bower */
/* Minimal AArch64 seL4 rootserver linker script (no jumps inside PT_LOAD) */
OUTPUT_FORMAT(elf64-littleaarch64)
OUTPUT_ARCH(aarch64)
ENTRY(_start)

__ipcbuf_vaddr = 0x00106000; /* Kernel-advertised init thread IPC buffer mapping. */
__stack_guard = 0x2000;      /* Guard page below IPC buffer to catch stack overruns. */
__stack_size = 0x40000;      /* 256 KiB stack for early boot logging/tracing. */

__stack_top = __ipcbuf_vaddr - __stack_guard;
__stack_bottom_unaligned = __stack_top - __stack_size;
__stack_bottom = __stack_bottom_unaligned & ~0xfff;

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
  __heap_start = .;
  .heap (NOLOAD) : {
    HEAP_SIZE = __stack_bottom - __heap_start;
    ASSERT(HEAP_SIZE > 0, "heap exhausted by text/data/bss");
    . = . + HEAP_SIZE;
  } :data
  __heap_end = __stack_bottom;

  /* Statically reserved stack region; the runtime uses __stack_top as the initial SP. */
  . = __stack_bottom;
  ASSERT((__stack_bottom & 0xfff) == 0, "stack bottom must be page aligned");
  .stack (NOLOAD) : {
    . = __stack_top;
  } :data
  ASSERT(ADDR(.stack) == __stack_bottom, "stack bottom must match computed bound");
  __stack_top = .;

  /*
   * Leave a guard gap below the IPC buffer to avoid stack overflows corrupting
   * the kernel-provided IPC buffer page (and the bootinfo frame that follows
   * it). The init thread stack must never run right up against the IPC buffer
   * mapping.
   */
  . = __stack_top + __stack_guard;
  ASSERT(. == __ipcbuf_vaddr, "IPC buffer virtual address must remain stable");

  PROVIDE(_end = __ipcbuf_vaddr);

  /* Do NOT advance '.' to device/MMIO addresses here. Keep segments tight. */
}
