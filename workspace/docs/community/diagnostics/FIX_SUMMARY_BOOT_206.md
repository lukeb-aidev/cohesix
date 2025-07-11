// CLASSIFICATION: COMMUNITY
// Filename: FIX_SUMMARY_BOOT_206.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-31

# Fix Summary Boot 206

This note captures the changes applied to ensure Cohesix userland reaches `/bin/init`.

## Static ELF checks

```bash
readelf -l workspace/target/sel4-aarch64/release/cohesix_root | head -n 20
```

Elf file type is EXEC (Executable file)
Entry point 0xffffff8040000000
There are 2 program headers, starting at offset 64

Program Headers:
  Type           Offset             VirtAddr           PhysAddr
                 FileSiz            MemSiz              Flags  Align
  LOAD           0x0000000000001000 0xffffff8040000000 0xffffff8040000000
                 0x000000000004c4ac 0x000000000004c4ac  R E    0x1000
  LOAD           0x000000000004e000 0xffffff804004e000 0xffffff804004e000
                 0x0000000000000020 0x0000000000095000  RW     0x1000

 Section to Segment mapping:
  Segment Sections...
   00     .text .rodata .eh_frame_hdr .eh_frame 
   01     .data .bss .heap .stack 
\n```bash
ffffff8040001854 T main
```
\nCPIO listing could not be generated: cpio unavailable in container.

## Key Fixes
- Updated syscall stubs to return ENOENT/EBADF/ENOSYS instead of -1.
- Init script now prints `COHESIX_USERLAND_BOOT_OK`.
- `ensure_plan9_ns` stages `plan9.ns` into both staging and out directories with error checking.
