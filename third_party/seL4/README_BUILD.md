// CLASSIFICATION: COMMUNITY
// Filename: README_BUILD.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-31

# Cohesix AArch64 seL4 Build Guide

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

The script builds `kernel.elf` and `cohesix_root.elf`, packages them
with a device tree into `out/boot/cohesix.cpio`, and validates the
ELF images using `readelf`, `nm`, and `objdump`.

If any required symbol is missing or the ELF headers are malformed,
the script aborts.

Troubleshooting logs are written to the current terminal.
