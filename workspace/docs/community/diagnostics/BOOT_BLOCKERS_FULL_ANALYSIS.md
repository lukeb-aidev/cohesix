// CLASSIFICATION: COMMUNITY
// Filename: BOOT_BLOCKERS_FULL_ANALYSIS.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-31

# Boot Failure Deep Dive

This document records the end-to-end investigation into why the
Cohesix AArch64 image fails to reach userland.

## Build and Staging Trace
`cohesix_fetch_build.sh` stages the rootserver ELF and kernel and
packages them into a CPIO archive. Key commands include:
```
cp "$ROOT/workspace/target/sel4-aarch64/release/cohesix_root" "$ROOT/out/cohesix_root.elf"
readelf -l "$ROOT/out/cohesix_root.elf"
find kernel.elf cohesix_root.elf | cpio -o -H newc > "$ROOT/out/boot/image.cpio"
```
The namespace file is copied via `ensure_plan9_ns()` to
`$STAGE_DIR/etc/plan9.ns`.

## Rust Boot Sequence
`src/main.rs` parses boot arguments, applies the namespace and executes
`/bin/init`:
```
write_role(role);
apply_namespace();
if !check_init_exists() { coherr!("fatal_missing_init"); }
exec_init();
```

### Memory Layout
Program headers from `cohesix_root.elf` show two LOAD segments starting
at `0xffffff8040000000` with the writable region at
`0xffffff8040051000`. Symbols define:
```
__heap_start = 0xffffff8040055000
__heap_end   = 0xffffff80400d5000
__stack_start = 0xffffff80400d6000
__stack_end   = 0xffffff80400e6000
```

## Observed Faults
`MMU_FAULT_AUDIT.md` captured a data abort at
`0xffffff8040012648` while accessing FAR `0xffffff807f000000`. The
instruction lies inside `core::unicode::printable::is_printable` and the
address falls outside all mapped segments.

## File System State
`config/plan9.ns` binds `/usr/coh/bin`, `/bin`, and `/usr` into the
runtime namespace. The miniroot supplies a minimal `/bin/init` script
that prints `COHESIX_USERLAND_BOOT_OK` and launches `rc` if present.

## Analysis Summary
The rootserver successfully loads and begins executing but triggers a
data abort before launching `/bin/init`. The abort occurs in a Rust
unicode helper due to a pointer far outside the ELF region. Possible
contributors include:
- early stack or register corruption during namespace application
- missing syscall implementations returning `-1` for file access
- a mis-staged `/bin/init` or plan9.ns causing invalid paths
Further work will focus on verifying BSS clearing, allocator behaviour,
and ensuring the init binary is present in the final CPIO image.
