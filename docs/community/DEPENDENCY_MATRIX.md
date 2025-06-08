// CLASSIFICATION: COMMUNITY
// Filename: DEPENDENCY_MATRIX.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-12

# Dependency Matrix

Cohesix relies on a small set of open source components. All imports use MIT, BSD, or Apache 2.0 licenses and are tracked with SPDX tags.

| Component | Version | License | Purpose |
|-----------|---------|---------|---------|
| seL4 | 2025.05 | MIT | Microkernel base |
| Plan 9 9front | 2025.05 | MIT/BSD | Userland services |
| BusyBox | 1.36.1 | GPL‑2 (isolated binary) | Core utilities |
| musl libc | 1.2.3 | MIT | POSIX layer |
| Go | 1.22 | BSD-style | Plan 9 services |
| Python | 3.10+ | PSF | CLI and tooling |
| rapier3d | 0.14 | MIT | Physics engine |
| clap | 4.5.4 | MIT | CLI argument parsing |
| ureq | 2.9 | MIT | HTTP client |
| spf13/cobra | v1.8.0 | Apache‑2.0 | Go CLI framework |

License files reside under `docs/community/LICENSES/` and are referenced in the SBOM (`sbom_spdx_2.3.json`). Use `cargo deny` and `reuse lint` in CI to ensure compliance.
