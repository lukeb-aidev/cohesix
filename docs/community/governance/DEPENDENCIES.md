// CLASSIFICATION: COMMUNITY
// Filename: DEPENDENCIES.md v0.5
// Author: Lukas Bower
// Date Modified: 2025-06-15

# System Dependencies

> For SPDX-style licenses and consolidation rationale, see: LICENSES_AND_REUSE.md

| Dependency            | Version      | Source / License                       | Notes                                               |
|-----------------------|--------------|----------------------------------------|-----------------------------------------------------|
| seL4 L4 microkernel   | 2025.05      | <https://sel4.systems> (MIT)           | Kernel foundation with Cohesix patches              |
| Plan 9 userland       | 9front 2025.05 | <https://9front.org> (MIT/BSD)         | 9P filesystem, `rc` shell, minimal POSIX subset     |
| BusyBox               | **1.36.1**   | <https://busybox.net> (GPL-2)          | Core UNIX tools + shell for lightweight utilities   |
| musl libc             | 1.2.3        | <https://musl.libc.org> (MIT)          | POSIX-compat layer for Plan 9 ports & BusyBox       |
| Go                    | **1.22**     | <https://go.dev> (BSD-style)           | CSP-based 9P services & auxiliary tooling           |
| Python                | 3.10+        | <https://python.org> (PSF)             | DSL, testing harnesses, runtime validators. Ensemble agents and tests now respect TMPDIR/COHESIX_ENS_TMP for writable paths Snapshot and validator traces now explicitly support TMPDIR and override guards. |
| C++17 & CUDA Toolkit  | 11.8 / 11.8  | <https://developer.nvidia.com> (NVIDIA EULA) | Torch/TensorRT GPU deploy; Rapier physics in Rust |

# Rust Crate Dependencies

| Crate            | Version  | Source / License  | Purpose                                   |
|------------------|----------|-------------------|-------------------------------------------|
| anyhow           | **1.0.82** | crates.io (MIT)   | Ergonomic error handling                  |
| clap             | **4.5.4**  | crates.io (MIT)   | Command-line argument parsing             |
| log              | 0.4       | crates.io (MIT)   | Structured logging facade                 |
| sha2             | 0.10      | crates.io (MIT)   | SHA-2 hashing (boot measure)              |
| serde            | 1.0       | crates.io (MIT)   | Serialize / deserialize                   |
| serde_json       | 1.0       | crates.io (MIT)   | JSON support                              |
| tokio            | 1.28      | crates.io (MIT)   | Async runtime                             |
| rapier3d         | 0.14      | crates.io (MIT)   | Physics simulation engine                 |
| ureq             | 2.9       | crates.io (MIT)   | HTTP client for cloud init                |
| regex-automata   | 0.4       | crates.io (MIT)   | Deterministic regex engine (utils)        |
| bytes            | 1.5       | crates.io (MIT)   | Zero‑copy byte buffers (async 9P helper) |
| p9               | 0.3.2     | crates.io (BSD-3-Clause) | 9P protocol server implementation |
| tempdir / tempfile | latest  | crates.io (MIT)              | Writable test directories for agent validation & boot tests |

# Go Dependencies

| Module | Version | Source / License | Purpose |
|--------|---------|------------------|---------|
| github.com/spf13/cobra | v1.8.0 | <https://github.com/spf13/cobra> (Apache-2.0) | CLI framework for `cohcli` |

# Tooling Dependencies

| Tool            | Version | Source / License            | Purpose                           |
|-----------------|---------|-----------------------------|-----------------------------------|
| OpenSSH         | 9.4p1   | <https://openssh.com> (BSD) | Secure remote access              |
| mandoc / man-db | 2.0.10  | BSD                         | Manual page rendering             |
| BusyBox (CLI)   | 1.36.1  | GPL-2                       | Coreutils and shell support       |
| curl            | 8.8.0   | curl License (MIT)          | HTTP fetches in build scripts     |
| zip             | 3.0     | Info-ZIP License            | Artefact packaging (deploy-ci)    |
| QEMU             | 8.1.2   | <https://www.qemu.org> (GPL-2)       | UEFI boot test; used in CI to validate firmware and kernel traces     |
| OpenSSL / libssl | 3.3       | <https://www.openssl.org> (Apache‑2.0/SSLeay) | Hash‑parity tests in boot measurement |
| clang / LLVM    | 17.0    | Apache-2.0 / UIUC           | Compiling C shims for seL4        |
