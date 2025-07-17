// CLASSIFICATION: COMMUNITY
// Filename: BOOT_FLOW_DIAGNOSTIC_20250717.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-31

# Boot Flow Diagnostic – MMU Fault 2025‑07‑17

This report correlates the QEMU boot logs under `out/diag_mmu_fault_20250717_105341/` with the current loader packaging and `cohesix_root` ELF state.

## Boot Loader and CPIO Packaging

`cohesix_fetch_build.sh` packages `kernel.elf`, `kernel.dtb`, and `cohesix_root.elf` into `cohesix.cpio`. The first three entries were verified during the build step and match the kernel’s expectations:

```
$(cpio -it < boot/cohesix.cpio | head -n 3)
```

The serial log confirms the ELF loader found the DTB in the archive and loaded the kernel and rootserver images successfully.

## Kernel Handoff

From `qemu_serial_20250717_100941.log`:

```
No DTB passed in from boot loader.
Looking for DTB in CPIO archive...found at 40b64288.
Loaded DTB from 40b64288.
... Booting all finished, dropped to user space
```

The kernel completes boot and transfers control to the rootserver.

## Rootserver Initialization and Fault

Immediately after user‑space entry the rootserver triggers a VM fault:

```
vm fault on data at address 0x9000000 with status 0x92000046
in thread "rootserver" at address 0x40217c
```

The disassembly around `0x402170` shows a write to the UART base address `0x09000000` prior to any mapping. Program headers list a `.uart` segment but no dynamic mapping occurs during start‑up:

```
LOAD           0x000c000 ... VirtAddr 0x00000000004a1000 ... RW
```

## Root Cause

`sys::init_uart()` writes directly to `UART_BASE` using `core::ptr::write_volatile`, assuming the device frame is mapped. In the current build the kernel does not map this region for the rootserver, so the store instruction at `0x40217c` faults.

## Fix

Avoid touching the UART MMIO region in `init_uart`. The debug console is already exposed via `seL4_DebugPutChar` which does not require additional mapping. The updated implementation simply references the symbol to keep the section but performs no access.

