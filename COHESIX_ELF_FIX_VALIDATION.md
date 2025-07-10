// CLASSIFICATION: COMMUNITY
// Filename: COHESIX_ELF_FIX_VALIDATION.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-30

# Cohesix ELF Fix Validation

This report documents the validation of `cohesix_root.elf` after applying fixes in **FixAndValidateCohesixELF-074**.

## Build Summary
- `cohesix_fetch_build.sh` now aborts when `third_party/seL4/lib/libsel4.a` is missing.
- `third_party/seL4/fetch_sel4.sh` uses `repo` with the `sel4test-manifest` for deterministic workspace setup.

## ELF Checks
readelf: Error: 'out/cohesix_root.elf': No such file
nm: 'out/cohesix_root.elf': No such file
objdump: 'out/cohesix_root.elf': No such file
