// CLASSIFICATION: COMMUNITY
// Filename: holistic_analysis.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-12

# Cohesix Root MMU Fault Investigation

This document consolidates diagnostics from `cohesix_root.elf` boot failures. The kernel loads and the elfloader succeeds, but execution falls back to `sel4test` due to early memory errors:

- frequent messages `Failed to allocate object of size ...`
- `vspace == NULL` warnings during capability reservation

Below is a holistic review of the captured artifacts and recommended next steps.

## Observations

### ELF Program Headers
- Two loadable segments with 4KiB alignment. Code is mapped read/execute, data mapped read/write. See lines 2–17 of the program header dump for details:
```
LOAD 0x1000 -> 0xffffff8040000000 filesz 0x51e0c memsz 0x51e0c R E
LOAD 0x53000 -> 0xffffff8040053000 filesz 0x80 memsz 0x95000 RW
```
Source: `cohesix_root_program_headers.txt` lines 2–17.

### Section Layout
- `.text` begins at `0xffffff8040000000` with size `0x3c61c`.
- `.bss`, `.heap`, and `.stack` share the second segment. Heap size is `0x80000` and ends only one page before the stack at `0xffffff80400d8000`.
- Sections dump excerpt:
```
[ 5] .data  0xffffff8040053000 80 bytes
[ 6] .bss   0xffffff8040055000 0x20 bytes
[ 7] .heap  0xffffff8040057000 0x80000 bytes
[ 8] .stack 0xffffff80400d8000 0x10000 bytes
```
Source: `cohesix_root_sections.txt` lines 16–23.

### Symbols
- Valid `_start` entry at `0xffffff8040000000`.
- Heap and stack symbols align with section dump.
- Only one libc symbol detected (`memcpy`), indicating a mostly freestanding binary.
```
1: 167: ffffff8040001854     4 FUNC    LOCAL  HIDDEN     1 memcpy
```
Source: `cohesix_root_libc_symbols.txt` line 1.

### QEMU Log
- The provided `qemu_debug_20250712_001455.log` is empty, so runtime messages are unavailable.

## Hypotheses (Ranked)

1. **Heap/Stack Overlap or Small Gap** – The heap ends one page before the stack starts. Under heavy allocation this may bleed into the stack, corrupting page tables and causing allocation failures.
2. **Insufficient Untyped Reservation** – The root task may request more objects than its initial untyped memory can supply, leading to repeated "Failed to allocate object" messages.
3. **Misaligned or Missing Section** – No glaring misalignments appear, but the minimal 4KiB alignment could still conflict with expected 64KiB reservations on some platforms.
4. **Unexpected libc Call** – Presence of `memcpy` might pull in additional runtime that relies on unavailable pages, though this seems less likely.
5. **QEMU Memory Layout** – Device tree or RAM size mismatch could undercut available untyped memory.

## Recommended Experiments

1. **Increase Heap Gap** – Pad an additional 0x10000 between `.heap` and `.stack` in the linker script to rule out overlap.
2. **Force 64KiB Alignment** – Align all loadable sections and program headers to 64KiB, matching large-page expectations.
3. **Verify Untyped List** – Add debug prints in the root server to dump initial untyped capability sizes.
4. **Simplify Root Allocations** – Temporarily reduce early object allocations (e.g., fewer paging structures) to see if failures stop.
5. **Check QEMU RAM & DTB** – Boot with explicit `-m 1024` and confirm device tree reserves expected memory regions.
6. **Audit libc Usage** – Replace or inline `memcpy`, ensuring no additional libc pulls creep in via compiler-builtins.

## Conclusion

The ELF headers themselves appear structurally sound, but the tight packing of heap and stack combined with possible untyped shortages is the most plausible cause of the MMU/vspace failures. Expanding alignment and verifying early allocations should provide clarity. Further instrumentation and larger safety margins are advised before broader refactoring.

