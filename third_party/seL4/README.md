// CLASSIFICATION: COMMUNITY
// Filename: README.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-28

This directory pins the seL4 kernel and libsel4 to a known commit for deterministic builds.
The snapshot corresponds to commit `c8ee04268800a5b14dd565032dc969d7a2f621cc` from the official
https://github.com/seL4/sel4 repository. Use `fetch_sel4.sh` to clone this exact revision
into `~/sel4_workspace` if it does not already exist.

 Cohesix AArch64 seL4 Build Guide

This directory orchestrates fetching and building the seL4 kernel
alongside the `cohesix_root` userland. The process targets the QEMU
`virt` board using an AArch64 cross compiler.

## Prerequisites
- `cmake` ≥ 3.20
- `ninja`
- `aarch64-linux-gnu-gcc` and `aarch64-linux-gnu-g++`
- `rustup` with `aarch64-unknown-linux-musl` and nightly components
- `cargo`
- `dtc` for compiling the device tree

Run `fetch_sel4.sh` once to clone the pinned seL4 sources into
`~/sel4_workspace`. After that, execute:

```bash
third_party/seL4/build_sel4.sh
```