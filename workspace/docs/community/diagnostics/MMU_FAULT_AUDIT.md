// CLASSIFICATION: COMMUNITY
// Filename: MMU_FAULT_AUDIT.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-10-14

# MMU Fault Audit 2027-10-14

This document analyzes recent MMU fault logs located under `out/diag_mmu_fault_*`.


## Observed Fault

The QEMU monitor log shows a data abort when executing at `0xffffff8040012648`:
```
Invalid write at addr 0x7F000000, size 8, region '(null)', reason: rejected
Taking exception 4 [Data Abort] on CPU 0
...with ESR 0x25/0x96000050
...with FAR 0xffffff807f000000
...with ELR 0xffffff8040012648
```

The disassembly around `0xffffff8040012648` indicates the instruction resides within
`core::unicode::printable::is_printable`:
```
ffffff8040012638: f107995f        cmp     x10, #0x1e6
ffffff804001263c: 52000108        eor     w8, w8, #0x1
ffffff8040012640: 54ffff20        b.eq    ffffff8040012624
ffffff8040012644: 38ea696c        ldrsb   w12, [x11, x10]
ffffff8040012648: 9100054d        add     x13, x10, #0x1
```
This routine reads from a lookup table using `x11` as the base pointer.

The fault address (`FAR`) of `0xffffff807f000000` lies far outside the loaded ELF
segments. All program headers end below `0xffffff8040161000`.
Symbols confirm the heap spans `__heap_start` at 0xffffff8040051000 and
`__heap_end` at 0xffffff8040151000.

## Assessment

`is_printable` should only access a readonly lookup table within `.rodata`, yet
`x11` resolved to `0xffffff807f000000`. This points to register or stack
corruption prior to the lookup. No program headers cover that region, so the
faulted address is outside all loaded segments.

The stack is initialized in `_sel4_start`, but no memory barrier ensured the zero
initialisation completed before Rust executed. If the CPU speculatively cached
stale data, registers used for the lookup could be left uninitialised.

## Proposed Fix

Insert `dsb sy` and `isb` barriers after the BSS zeroing loop in `entry.S`. This
guarantees the zeroed memory and stack pointer are globally visible before calling
Rust `main`. The update is captured in CHANGELOG v0.454.
