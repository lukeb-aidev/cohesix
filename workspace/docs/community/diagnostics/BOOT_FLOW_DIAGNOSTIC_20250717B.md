// CLASSIFICATION: COMMUNITY
// Filename: BOOT_FLOW_DIAGNOSTIC_20250717B.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-31

# Boot Flow Diagnostic – Unknown Syscall Fault 2025‑07‑17

This report examines the latest QEMU logs in `out/diag_mmu_fault_20250717_111838/` to trace the boot sequence from elfloader through `cohesix_root` entry.

## Loader and DTB Handling

`cohesix_fetch_build.sh` embeds `kernel.elf`, `kernel.dtb`, and `cohesix_root.elf` into `cohesix.cpio`. The log shows the loader finding the DTB inside the archive and loading both images:

```
No DTB passed in from boot loader.
Looking for DTB in CPIO archive...found at 40b64288.
Loaded DTB from 40b64288.
ELF-loading image 'kernel' to 40000000
ELF-loading image 'cohesix_root.elf' to 4023f000
```

## Kernel Handoff

After enabling paging, the kernel reserves the expected virtual regions and drops to user space:

```
reserved virt address space regions: 3
  [ffffff8040000000..ffffff804023d000]
  [ffffff804023d000..ffffff804023ed15]
  [ffffff804023f000..ffffff80402e2000]
Booting all finished, dropped to user space
```

## Rootserver Fault

Immediately on entry the rootserver triggers a capability fault due to an unrecognised syscall number:

```
Caught cap fault in send phase at address 0
while trying to handle:
unknown syscall 0
in thread 0xffffff807ffc9400 "rootserver" at address 0x40217c
```

Disassembly at `0x40217c` shows a `svc #0` with `x16` set to `#7`. The seL4 headers define `seL4_SysDebugPutChar` as `-9`; therefore the syscall register must hold a **negative** value. The current implementation uses positive IDs, leading to `unknown syscall` errors.

## Outstanding Issues

1. **Syscall numbers:** `sys.rs` uses positive values in `mov x16,#<id>`; seL4 expects the negative enum values (e.g., `-9` for `DebugPutChar`).
2. **CPIO order:** Verified correct (`kernel.elf`, `kernel.dtb`, `cohesix_root.elf`), no issue.
3. **DTB address range:** Loader finds DTB and maps it; ordering is consistent with kernel expectations.
4. **Rootserver memory layout:** Program headers show `.heap` and `.stack` segments aligned with linker script.

## Prioritized Fix List

1. **Correct syscall constants in `sys.rs`** so the rootserver uses negative IDs.
2. Audit other inline assembly to ensure sign-extended values are used where required.
3. Verify bootargs and namespace parsing once syscall stubs succeed.

## Implemented Fix

`sys.rs` now loads the negative syscall numbers (`-9`, `-3`, `-5`, `-7`, `-11`) before invoking `svc #0`. This aligns with the seL4 ABI and resolves the immediate boot fault.

