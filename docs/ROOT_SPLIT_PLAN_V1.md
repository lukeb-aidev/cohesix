// CLASSIFICATION: COMMUNITY
// Filename: ROOT_SPLIT_PLAN_V1.md v1.0
// Author: Lukas Bower
// Date Modified: 2027-08-14

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

## 🔗 Future References

This document is **ROOT_SPLIT_PLAN_V1** and will be referenced in all future pull requests and Codex tasks. Example citation:

> Task WriteRootSplitDoc-001 implements ROOT_SPLIT_PLAN_V1

---

## 📝 Example Contributions

All future contributions referencing this plan must cite it explicitly, ensuring they align with the architecture and build steps defined here.

