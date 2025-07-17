// CLASSIFICATION: COMMUNITY
// Filename: HOLISTIC_BOOT_FLOW_20250717.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-31

# Holistic Boot Flow Diagnostic – 2025-07-17

This document summarises the boot flow after recent fixes and diagnoses the remaining
fault preventing entry into userland.

## Loader Startup
The build script `cohesix_fetch_build.sh` packages the images in strict order:
`kernel.elf`, `kernel.dtb`, then `cohesix_root.elf`. The QEMU serial log confirms
that the loader discovers the DTB in the CPIO archive and loads both ELF images:

```
No DTB passed in from boot loader.
Looking for DTB in CPIO archive...found at 40b64288.
Loaded DTB from 40b64288.
ELF-loading image 'kernel' to 40000000
ELF-loading image 'cohesix_root.elf' to 4023f000
```

## Kernel Handoff
Once paging is enabled, the kernel reserves three virtual regions and drops to
user space. The log excerpt shows:

```
reserved virt address space regions: 3
  [ffffff8040000000..ffffff804023d000]
  [ffffff804023d000..ffffff804023ed15]
  [ffffff804023f000..ffffff80402e2000]
Booting all finished, dropped to user space
```

## Rootserver Fault
Immediately after user-space entry, the rootserver faults with an unknown syscall:

```
Caught cap fault in send phase at address 0
while trying to handle:
unknown syscall 0
in thread 0xffffff807ffc9400 "rootserver" at address 0x40217c
```

Disassembly around `0x40217c` shows a `svc #0` instruction meant to invoke
`seL4_DebugPutChar`. The program headers confirm `.text` is loaded at `0x400000`
with executable permissions. The root cause is the syscall identifier—positive
constants were used, yielding `0` after sign truncation. The kernel therefore
reports `unknown syscall 0`.

## Fix
`workspace/cohesix_root/src/sys.rs` now defines explicit negative syscall
constants and passes them as immediates to the inline assembly. With this change
`seL4_DebugPutChar` and related stubs invoke the correct kernel services.
Subsequent boots reach the rootserver main loop and print
`"✅ rootserver main loop entered"`.

