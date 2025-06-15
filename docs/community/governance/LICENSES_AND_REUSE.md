// CLASSIFICATION: COMMUNITY
// Filename: LICENSES_AND_REUSE.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-06-15

# Licensing and OSS Reuse

This document merges `LICENSE_MATRIX.md`, `OSS_REUSE.md`, and `OPEN_SOURCE_DEPENDENCIES.md` into a single reference for Cohesix licensing and third-party software management.

## 1 · SPDX Summary

All Cohesix code and dependencies must be licensed under **Apache-2.0**, **MIT**, or **BSD** variants. Each file includes an SPDX header matching its license, and all external packages appear in the SBOM files `sbom_spdx_2.3.json` and `sbom_cyclonedx_1.5.json`.

## 2 · Reuse Policy

Only the licenses above are permitted. GPL or LGPL code is prohibited unless fully isolated. Every dependency is recorded in this document and audited during CI runs. Contributions must retain upstream copyright notices and provide public access to source code when required. CI checks will reject any non-approved license or missing SPDX header.

## 3 · Open Source Dependencies

The table below lists current third-party packages used by Cohesix.

|------|---------|--------------|---------|---------|
| sha2 | 0.10 | MIT OR Apache-2.0 | Hashing for integrity checks | https://crates.io/crates/sha2 |
| anyhow | 1.0 | MIT OR Apache-2.0 | Error handling in Rust | https://crates.io/crates/anyhow |
| env_logger | 0.11.7 | MIT OR Apache-2.0 | Structured logging | https://crates.io/crates/env_logger |
| log | 0.4 | MIT OR Apache-2.0 | Logging facade | https://crates.io/crates/log |
| clap | 4.5.4 | MIT OR Apache-2.0 | CLI argument parsing | https://crates.io/crates/clap |
| sysinfo | 0.30 | MIT | System information | https://crates.io/crates/sysinfo |
| libloading | 0.7 | MIT OR Apache-2.0 | Dynamic library loading | https://crates.io/crates/libloading |
| inotify | 0.10 | MIT OR Apache-2.0 | Device hotplug events | https://crates.io/crates/inotify |
| rapier3d | 0.17.2 | Apache-2.0 | Physics engine | https://crates.io/crates/rapier3d |
| tokio | 1 | MIT | Async runtime | https://crates.io/crates/tokio |
| ureq | 2.9 | MIT OR Apache-2.0 | HTTP client | https://crates.io/crates/ureq |
| serde | 1 | MIT OR Apache-2.0 | Serialization | https://crates.io/crates/serde |
| serde_json | 1 | MIT OR Apache-2.0 | JSON serialization | https://crates.io/crates/serde_json |
| sdl2 | 0.37 | MIT | Windowing & input | https://crates.io/crates/sdl2 |
| cobra | 1.8.0 | Apache-2.0 | Go CLI framework | https://github.com/spf13/cobra |
| chi | v5 | MIT | HTTP router for GUI orchestrator | https://github.com/go-chi/chi |

| tempfile | 3.10 | MIT OR Apache-2.0 | Temporary directory and file handling in tests | https://crates.io/crates/tempfile |
| qemu | system | GPL-2.0-only | Emulator used for UEFI boot testing | https://www.qemu.org/ |
| cuda-sys | 0.3 | Apache-2.0 | Rust FFI bindings for NVIDIA CUDA | https://crates.io/crates/cuda-sys |
\n## 4 · License Matrix

| Name | Version | SPDX | License File | CVEs |
|------|---------|------|-------------|------|
| sha2 | 0.10 | MIT OR Apache-2.0 | LICENSES/sha2-0.10.txt |  |
| anyhow | 1.0 | MIT OR Apache-2.0 | LICENSES/anyhow-1.0.txt |  |
| tokio | 1 | MIT | N/A |  |

## 5 · Audit References

- `sbom_spdx_2.3.json` – SPDX formatted software bill of materials
- `sbom_cyclonedx_1.5.json` – CycloneDX formatted SBOM
- Continuous license scanning via `cargo deny` and GitHub dependency graph
- SPDX documents and license scan output are included in every milestone archive
