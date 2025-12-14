/* Author: Lukas Bower */
/* Minimal AArch64 seL4 rootserver linker script (no jumps inside PT_LOAD) */
OUTPUT_FORMAT(elf64-littleaarch64)
OUTPUT_ARCH(aarch64)
ENTRY(_start)

STACK_SIZE = 0x40000;        /* 256 KiB stack for early boot logging/tracing. */
STACK_GUARD_GAP = 0x2000;    /* Guard page below IPC buffer to catch stack overruns. */
IPCBUF_VADDR = 0x00106000;   /* Kernel-advertised init thread IPC buffer mapping. */

STACK_TOP = IPCBUF_VADDR - STACK_GUARD_GAP;
STACK_BOTTOM = STACK_TOP - STACK_SIZE;

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
    HEAP_SIZE = STACK_BOTTOM - __heap_start;
    ASSERT(HEAP_SIZE > 0, "heap exhausted by text/data/bss");
    . = . + HEAP_SIZE;
  } :data
  __heap_end = .;

  /* Statically reserved stack region; the runtime uses __stack_top as the initial SP. */
  . = STACK_BOTTOM;
  ASSERT((STACK_BOTTOM & 0xfff) == 0, "stack bottom must be page aligned");
  __stack_bottom = STACK_BOTTOM;
  .stack (NOLOAD) : {
    . = STACK_TOP;
  } :data
  __stack_top = .;

  /*
   * Leave a guard gap below the IPC buffer to avoid stack overflows corrupting
   * the kernel-provided IPC buffer page (and the bootinfo frame that follows
   * it). The init thread stack must never run right up against the IPC buffer
   * mapping.
   */
  . = __stack_top + STACK_GUARD_GAP;
  ASSERT(. == IPCBUF_VADDR, "IPC buffer virtual address must remain stable");

  PROVIDE(_end = IPCBUF_VADDR);

  /* Do NOT advance '.' to device/MMIO addresses here. Keep segments tight. */
}
