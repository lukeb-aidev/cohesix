// CLASSIFICATION: COMMUNITY
// Filename: STATIC_LINK_FIX_REPORT.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-31

# Static Link Fix Report

This report documents the deterministic static linkage corrections for the Cohesix seL4 root build.

- `libsel4.a` is linked via build script and target specification.
- All headers under `third_party/seL4/include` are referenced by compiling `src/sel4_dummy.c`.
- `link.ld` now exposes precise heap and stack symbols with version bump.
- `.cargo/config.toml` enforces `-static` and `-no-pie` flags for `sel4-aarch64`.

The build and validation logs are captured in `STATIC_LINK_FIX_VALIDATION.md`.
