// CLASSIFICATION: COMMUNITY
// Filename: BUILD_PLAN.md v0.3
// Date Modified: 2025-06-07
// Author: Lukas Bower

# Build Plan

This document outlines the step-by-step build strategy for Cohesix, covering native builds, cross-compilation, container images, and reproducible artifacts.

## 1. Prerequisites
- **Rust Toolchain**: Install via `rustup` (stable channel, version ≥ 1.76 for the 2024 edition) with targets:
  - `x86_64-unknown-linux-gnu`
  - `aarch64-unknown-linux-gnu`
- **Go**: Version ≥ 1.21 for Plan 9 services
- **Python**: Version ≥ 3.10 (for CLI and Codex scripts)
- **Docker**: Version ≥ 24.0 (for containerized builds)
- **C Compiler**: `gcc` or `clang` for BusyBox and C backend testing

## 2. Native Build (x86_64)

1. Fetch dependencies:
   ```bash
   cargo fetch
   go mod download
   pip install -r requirements.txt  # if any
   ```
2. Build the compiler and CLI:
   ```bash
   cargo build --release
   ```
3. Run tests:
   ```bash
   ./test_all_arch.sh
   ```
4. Generate cross-arch artifacts:
   ```bash
   cargo build --release --target aarch64-unknown-linux-gnu
   ```

## 3. Dockerized Multi-Arch Builds

We use Docker Buildx to create reproducible, multi-arch images.

1. **Set up Buildx builder**:
   ```bash
   docker buildx create --name cohesix-builder --use
   docker buildx inspect --bootstrap
   ```
2. **Build and push images**:
   ```bash
   docker buildx build \
     --platform linux/amd64,linux/arm64 \
     --tag lukeb-aidev/cohesix:latest \
     --push \
     -f Dockerfile .
   ```
3. **Dockerfile Outline**:
   ```dockerfile
   FROM rust:1.76-buster AS builder
   WORKDIR /workspace
   COPY . .
   RUN rustup target add aarch64-unknown-linux-gnu && \
       cargo build --release --target aarch64-unknown-linux-gnu

   FROM debian:bookworm-slim
   COPY --from=builder /workspace/target/aarch64-unknown-linux-gnu/release/cohcc /usr/local/bin/cohcc
   ENTRYPOINT ["/usr/local/bin/cohcc"]
   ```

## 4. BusyBox & Tooling Build

1. Clone BusyBox:
   ```bash
   git clone https://www.busybox.net/git/busybox.git
   cd busybox
   ```
2. Configure and build for aarch64:
   ```bash
   make defconfig
   make CROSS_COMPILE=aarch64-linux-gnu- -j$(nproc)
   ```
3. Install to staging:
   ```bash
   make CONFIG_PREFIX=/usr/local/busybox install
   ```

## 5. Reproducible Builds
- Pin all crate and module versions in `Cargo.lock` and `DEPENDENCIES.md`.
- Use `SOURCE_DATE_EPOCH` environment variable for deterministic timestamps:
  ```bash
  export SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)
  ```
- Validate reproducibility:
  ```bash
  docker run --rm \
    -e SOURCE_DATE_EPOCH \
    -v $(pwd):/workspace \
    buildpack/docker \
    /workspace/scripts/build-reproducible.sh
  ```

## 6. Cross-Compilation for OS Images

1. **seL4 Bootloader**:
   - Use `cargo build --release` targeting `riscv64imac-unknown-none-elf` or `aarch64-none-elf`.
2. **Plan 9 Services**:
   - Compile with Go’s `GOOS=plan9 GOARCH=amd64` for Plan 9 userland.
   - Cross-compile BusyBox as above for service bundling.
3. **Image Assembly**:
   ```bash
   scripts/assemble_image.sh \
     --bootloader target/aarch64-none-elf/release/bootloader.bin \
     --kernel target/aarch64-unknown-linux-gnu/release/cohesix.img \
     --services /usr/local/busybox/bin
   ```

## 7. CI Integration
- Integrate steps into `.github/workflows/ci.yml`:
  - Matrix job for x86_64 & aarch64 builds
  - Docker multi-arch build and push
  - BusyBox build and test
  - Reproducibility check

---

*This build plan ensures Cohesix artifacts are consistent, multi-arch, and production-ready.*
