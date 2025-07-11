// CLASSIFICATION: COMMUNITY
// Filename: WHY_COHESIX_ROOT_FAILS_TO_BOOT_20250711.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

# Why Cohesix Root Fails to Boot (MMU fault 20250711)

This document summarizes the findings from an audit of the `cohesix_root` ELF after a boot failure captured on **2025-07-11**. Diagnostics were collected in `out/diag_mmu_fault_20250711_231910/`.

## Main Audit Points

1. **Boot Flow in `main.rs`** – The root task logs memory regions, parses `/boot/bootargs.txt`, and writes the active role to `/srv/cohrole`. It reads `/etc/plan9.ns` and then verifies the presence of `/bin/init` before calling `exec_init()`.
2. **System Call Stubs in `sys.rs`** – All filesystem and process syscalls (`coh_open`, `coh_read`, `coh_exec`, etc.) currently return negative errno values (e.g. `ENOENT`, `EBADF`). Because `check_init_exists()` relies on these calls, it always reports `missing_init_bin` and the root task never actually spawns userland.
3. **Allocator Bounds** – The linker script reserves 512 KB for `.heap` and 64 KB for `.stack`. Program headers show these regions mapped in a single RW segment:
   ```
   9:  LOAD           0x0000000000001000 0xffffff8040000000 0xffffff8040000000
   11:  LOAD           0x0000000000051000 0xffffff8040051000 0xffffff8040051000
   ```
   The symbols `__heap_start` and `__heap_end` match the addresses reported in the ELF symbols file.
4. **Libc Symbols** – Diagnostics only reported one libc-like symbol (`memcpy`) which originates from our own `sys.rs` helpers and does not pull in an external libc.
5. **Suspicious Calls** – `cohesix_root_suspicious_calls.txt` is empty, indicating no unexpected external branches in the disassembly.
6. **QEMU Log** – `qemu_debug_20250711_230704.log` is empty, so the exact MMU fault could not be correlated with an address. However, the loaded segments show the stack and heap regions placed immediately after the `.data` section which is consistent with the linker script.

## Likely Failure Cause

Because all filesystem and exec syscalls in `sys.rs` are hard-coded stubs returning errors, the root server never locates `/bin/init` and cannot mount a namespace. The allocator and BSS checks complete successfully, but execution halts when `exec_init()` fails and the panic loop is entered.

## Minimal Fix Plan

1. Implement real seL4 bindings for file and process syscalls or proxy them to an initial ramfs service so `/bin/init` can be opened and executed.
2. Capture complete QEMU traces to correlate any MMU fault with the addresses found in `cohesix_root_program_headers.txt` and `cohesix_root_sections.txt`.
3. Re-run the ELF diagnostic script after implementing syscalls to ensure no new undefined symbols or suspicious calls appear.
