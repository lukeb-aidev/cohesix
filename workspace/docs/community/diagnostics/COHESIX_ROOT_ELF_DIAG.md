// CLASSIFICATION: COMMUNITY
// Filename: COHESIX_ROOT_ELF_DIAG.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-27

# Cohesix Root ELF Investigation

During aarch64 builds, `cohesix_root.elf` produced by `cohesix_fetch_build.sh` reported an entry point of `0x0` and lacked standard seL4 syscall symbols.

## Findings

- `workspace/cohesix_root/link.ld` specified `ENTRY(_sel4_start)` yet the assembly entry file only defined `_start`.
- The missing symbol caused LLD to set the ELF header entry point to zero.
- `src/sys.rs` exposed only `seL4_DebugPutChar`, leaving `seL4_Send`, `seL4_Recv`, and `seL4_Yield` undefined.

## Fixes Implemented

- Updated both linker scripts to use `ENTRY(_start)`.
- Added minimal AArch64 syscall stubs for `seL4_Send`, `seL4_Recv`, and `seL4_Yield`.
- Bumped versions in `METADATA.md` accordingly.

After rebuilding, `readelf -h` now shows a non-zero entry and `nm` lists the core seL4 symbols.
