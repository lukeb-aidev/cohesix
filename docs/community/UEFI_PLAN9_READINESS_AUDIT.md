// CLASSIFICATION: COMMUNITY
// Filename: UEFI_PLAN9_READINESS_AUDIT.md v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

# UEFI + Plan9 Readiness Audit

The following report summarizes an audit of the Cohesix repository to verify readiness to boot into a seL4 + Plan9 userland via ISO under QEMU. The review focused on code dependencies, userland tooling, Go helpers, and binary formats.

## 1. Linux/POSIX Dependencies

- `src/runtime/loader.rs` uses `std::os::unix::fs::PermissionsExt` for setting execute permissions, implying a Unix‑style filesystem dependency【F:src/runtime/loader.rs†L36-L42】.
- `src/telemetry/router.rs` and `src/physical/sensors.rs` reference `/sys` paths for temperature and sensor access【F:src/telemetry/router.rs†L48-L48】【F:src/physical/sensors.rs†L121-L140】.
- `config/plan9.ns` mounts `/dev` and `/proc` directly, which are Linux‑specific and not present in a pure UEFI Plan9 environment【F:config/plan9.ns†L6-L8】.
- C source `src/seL4/root_task.c` uses standard C library calls (`fopen`, `mkdir`, `getenv`) and POSIX headers (`unistd.h`, `fcntl.h`)【F:src/seL4/root_task.c†L10-L32】.
- Build script `cohesix_fetch_build.sh` installs `aarch64-linux-musl-gcc` via `apt` and relies on typical Linux package locations【F:cohesix_fetch_build.sh†L8-L36】.

## 2. Userland Tooling

- `tools/make_iso.sh` stages BusyBox, CLI wrappers (`cohcli`, `cohtrace`, `cohcap`, etc.), demos, Python modules, and man pages under `/usr/bin` and `/usr/cli`【F:tools/make_iso.sh†L53-L121】.
- Validation at the end of `make_iso.sh` checks for binaries and man pages, but the script logs a warning if BusyBox is missing; ensure BusyBox build completes before ISO creation【F:tools/make_iso.sh†L131-L171】.
- Plan9 namespace file `plan9.ns` binds `/usr/coh/bin` into `/bin` and mounts services such as `/srv/cuda` and `/telemetry` but does not mount a home directory for root; confirm expected behaviour【F:plan9.ns†L6-L18】.
- Additional namespace configuration in `config/plan9.ns` still references `/dev` and `/proc`, indicating legacy support not aligned with pure Plan9【F:config/plan9.ns†L6-L8】.

## 3. Go Helper Binaries

- `tools/make_iso.sh` copies compiled Go binaries from `go/bin` into `/usr/cli/` and places `coh-9p-helper` under `/srv/9p` when present. Ensure these helpers are built during the standard build pipeline【F:tools/make_iso.sh†L89-L101】.
- Go helper `coh-9p-helper` implements a TCP to Unix socket proxy and compiles with minimal dependencies; no POSIX‐only calls were detected in the Go source【F:go/cmd/coh-9p-helper/main.go†L12-L58】.

## 4. CUDA and Rapier Integration

- `src/cuda/runtime.rs` dynamically loads `libcuda.so` when not targeting UEFI. Under UEFI it exposes a stub interface at `/srv/cuda` with graceful fallback logging【F:src/cuda/runtime.rs†L8-L30】【F:src/cuda/runtime.rs†L80-L107】.
- The CUDA executor falls back to writing `cuda disabled` if the library is absent and logs the reason to `/srv/cuda_result`【F:src/cuda/runtime.rs†L188-L216】.
- Rapier integration under `/sim/` was not explicitly referenced in this audit, though tests in `src/demos` rely on physics features.

## 5. Binary Format and Staging

- Repository `bin/` contains shell wrappers rather than prebuilt ELF binaries; final ISO creation expects compiled binaries under `out/` which were not present in this repository checkout. Binary verification could not be completed.
- `tools/make_iso.sh` copies kernel and userland ELF binaries from `out/boot` into the ISO, renaming them appropriately for the EFI boot path【F:tools/make_iso.sh†L23-L41】【F:tools/make_iso.sh†L169-L171】.

## 6. Testing & Build Status

- Running `cargo test --workspace --no-run` failed due to missing vendored crates, indicating the build requires a full vendor directory or network access for dependencies【d5852f†L1-L22】.
- The repository includes `tests/` for kernel and userland functionality; however, these were not executed in this environment.

## Summary

- **Linux/POSIX Dependencies:** Several source files still reference `/sys`, `/dev`, or use `std::os::unix` features. These should be replaced with Plan9‑compatible abstractions or removed.
- **Userland Completeness:** `make_iso.sh` stages the expected CLI tools and validates their presence. Ensure BusyBox and mandoc are always built before ISO creation.
- **Go Helpers:** `coh-9p-helper` and other helpers are copied into `/usr/cli`; confirm cross‑compilation for UEFI if needed.
- **Plan9 Namespace:** The primary `plan9.ns` looks minimal, but `config/plan9.ns` includes Linux mounts that may not function under pure UEFI. Consolidate to a single Plan9‑only namespace definition.
- **Binaries:** Build outputs were unavailable, so ELF/PE validation could not be done. Confirm all binaries are statically linked and placed under `/usr/bin`.
- **CUDA/Rapier:** CUDA gracefully falls back if not present, exposing a stub via `/srv/cuda`. No explicit Rapier gaps noted, but ensure `/sim/` is mounted for worker nodes.

This audit indicates progress toward a UEFI + Plan9 boot but highlights lingering POSIX assumptions, missing build artifacts, and potential namespace inconsistencies. Address these gaps before declaring full readiness for ISO boot on seL4. 
