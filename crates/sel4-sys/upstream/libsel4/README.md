<!-- Author: Lukas Bower -->
<!-- Purpose: Record the provenance and scope of the curated libsel4 fallback snapshot. -->
<!-- Copyright 2026 Lukas Bower -->

# Curated libsel4 fallback snapshot

This directory mirrors the public header inputs from seL4 16.0.0 at kernel
commit `6e7c3b733d296cfd88d5fbf635c96e447a882374`.

The snapshot contains `arch_include`, `include`, `mode_include`,
`sel4_arch_include`, and the `bcm2711` and `qemu-arm-virt` platform subsets
from upstream `sel4_plat_include`. The platform subsets retain the historical
local `plat_include` directory name expected by `crates/sel4-sys/build.rs`.
Files below those mirrored directories are kept byte-for-byte identical to
upstream except that the trailing blank line in
`mode_include/32/sel4/mode/types.h` is removed to satisfy the repository's
whitespace guard.

Profile-selected generated headers remain authoritative for target builds.
This curated snapshot is only the fail-closed fallback used when an explicit
generated-header tree is not supplied.
