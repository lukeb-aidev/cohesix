// CLASSIFICATION: COMMUNITY
// Filename: BUILD_PLAN.md v0.6
// Date Modified: 2026-12-31
// Author: Lukas Bower

# Build Plan

This document outlines the step-by-step build strategy for Cohesix, covering
native builds, cross-compilation, container images, and reproducible artifacts.
The preferred approach is to run `cohesix_fetch_build.sh`, which downloads
prerequisites and assembles `cohesix_root.elf` and the UEFI boot image in a
single step.

## 1. Prerequisites
- **Rust Toolchain**: install via `rustup` with targets:
  - `x86_64-unknown-uefi`
  - `aarch64-unknown-uefi`
- **Go**: version ≥ 1.21 for Plan9 services
- **Python**: version ≥ 3.10 (CLI helpers)
- **Docker**: version ≥ 24 for containerized builds
- **C Compiler**: `gcc` or `clang` for BusyBox and C shims

## 2. Native Build (x86_64)
1. Fetch dependencies:
   ```bash
   cargo fetch
   go mod download
   pip install -r requirements.txt
   ```
2. Build the compiler and CLI:
   ```bash
   cargo build --release --target x86_64-unknown-uefi
   ```
3. Run tests:
   ```bash
   ./test_all_arch.sh
   ```
4. Assemble the boot image:
   ```bash
   cohesix_fetch_build.sh --target x86_64
   cohtrace snapshot --tag local_build
   ```

## 3. Dockerized Multi-Arch Builds
Use Docker Buildx to produce reproducible images:
```bash
docker buildx create --name cohesix-builder --use
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  --tag cohesix:latest \
  -f Dockerfile .
```
The Dockerfile installs the Rust toolchain and outputs `cohesix_root.elf` and
`kernel.efi` for each architecture.

## 4. BusyBox & Tooling Build
```bash
git clone https://www.busybox.net/git/busybox.git
cd busybox
make defconfig
make CROSS_COMPILE=aarch64-none-elf- -j$(nproc)
make CONFIG_PREFIX=/usr/local/busybox install
```

## 5. Reproducible Builds
- Pin crate versions in `Cargo.lock` and `DEPENDENCIES.md`.
- Set `SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)`.
- Verify deterministic output via CI scripts.

## 6. Cross-Compilation for OS Images
```bash
scripts/assemble_image.sh \
  --bootloader target/aarch64-none-elf/release/bootloader.bin \
  --kernel target/aarch64-unknown-uefi/release/cohesix.img \
  --services /usr/local/busybox/bin
```

## 7. CI Integration
The `.github/workflows/ci.yml` file builds for both architectures, runs tests,
validates SBOMs, and ensures metadata sync via `validate_metadata_sync.py`.
