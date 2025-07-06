// CLASSIFICATION: COMMUNITY
// Filename: ROOT_SPLIT_PLAN_V1.md v1.1
// Author: Lukas Bower
// Date Modified: 2027-08-15

# Cohesix Root Split — Version 1

This document serves as the canonical design reference for isolating `cohesix_root` from the rest of the Cohesix project. All future tasks referencing *ROOT_SPLIT_PLAN_V1* must align with this architecture.

---

## 🎯 Objectives

- **Isolate `cohesix_root`** as a dedicated `no_std`, minimal trusted root server, built only for seL4.
- **Retain all `std` capabilities** in the primary Cohesix library and binaries (CLI tools, CUDA, orchestrators, validator, agents).
- **Use a Rust workspace** to coordinate builds, while keeping separate build targets, dependencies, and compilation units.

---

## 🏗️ Directory Layout

```text
workspace/
├── cohesix_root/
│   ├── Cargo.toml       # no_std, builds only on sel4
│   └── src/
│       ├── main.rs
│       ├── plan9.rs
│       └── coherr.rs
├── cohesix/
│   ├── Cargo.toml       # std, CUDA, CLI, orchestration, agents
│   └── src/
│       ├── agent/
│       ├── validator/
│       └── …
├── cohesix-9p/
│   ├── Cargo.toml
│   └── src/
├── Cargo.toml           # workspace
├── cohesix_fetch_build.sh
└── docs/
    └── ROOT_SPLIT_PLAN_V1.md
```

---

## 🔍 Build Commands

To build the seL4 `no_std` root server:

```bash
cargo +nightly build -Z build-std=core,alloc --release --no-default-features \
    --target sel4-aarch64.json --package cohesix_root
```

To build all standard binaries (CLI, CUDA, agents):

```bash
cargo build --release
```

---

## ✅ Testing

- `cargo test` runs for the main `cohesix` package (`std`).
- `cargo check --package cohesix_root` validates `no_std` compilation on the seL4 target.
- `readelf` is used on `cohesix_root.elf` to check segment alignment and size.
- QEMU plus seL4 boots `cohesix_root.elf` as the root server.

---

## 🔧 Rationale

- Separates the trusted `no_std` TCB from the larger `std` ecosystem code.
- Allows the validator, orchestrators, and CUDA services to run unchanged on the host, while the root server remains minimal.
- Creates a clean, maintainable workspace with clear ownership of `std` vs `no_std` code.

---

## ✅ Viability Review

The approach is viable. seL4 officially supports `no_std` Rust components and
the `elfloader` can boot our root server on both x86_64 and aarch64. The split
keeps the minimal trusted computing base small while letting the rest of the
system use standard libraries.  Alternatives such as merging everything under a
single Cargo target complicate dependency management and would risk linking
unwanted `std` code into the root server.

---

## 🚀 Implementation Steps

1. **Prepare the seL4 build environment** using the upstream
   `sel4-sys` tooling. Install `rustup`, `cargo` and the seL4 targets
   `aarch64-unknown-none` and `x86_64-unknown-none`.
2. **Build the root server** with `cargo +nightly build -Z build-std=core,alloc
   --release --no-default-features --target sel4-aarch64.json` (repeat for the
   x86 target). The `cohesix_root.elf` produced will be loaded by the seL4
   `elfloader`.
3. **Compile userland binaries** via `cargo build --release` and stage them into
   the Plan9 filesystem image used by QEMU.
4. **Create a UEFI image** that bundles the seL4 `elfloader`, kernel image, and
   `cohesix_root.elf`. Scripts in `tools/` handle this process and place the
   result under `out/efi-image/`.
5. **Boot in QEMU** using `qemu-system-x86_64` or `qemu-system-aarch64` with the
   UEFI image. Verify that the validator and shell start correctly.

Testing after each step should run `cargo test`, `go test`, and the
integration scripts in `tests/`. Boot logs captured from QEMU must be stored in
`/log/` for review.  If QEMU is unavailable the boot tests may be skipped as
described in `VALIDATION_AND_TESTING.md`.

---

## 📚 Recommended References

- [seL4 Boot Flow](https://docs.sel4.systems/General/BootFlow.html) explains how
  the `elfloader` transfers control to the kernel and root task.
- [9front documentation](https://9p.io/plan9/) provides background on the Plan9
  userland principles we follow.
- [seL4 Rust Quickstart](https://github.com/sel4/sel4-rust) shows minimal Rust
  root servers and is a useful template for our build scripts.

---

## 🔗 Future References

This document is **ROOT_SPLIT_PLAN_V1** and will be referenced in all future pull requests and Codex tasks. Example citation:

> Task WriteRootSplitDoc-001 implements ROOT_SPLIT_PLAN_V1

---

## 📝 Example Contributions

All future contributions referencing this plan must cite it explicitly, ensuring they align with the architecture and build steps defined here.

