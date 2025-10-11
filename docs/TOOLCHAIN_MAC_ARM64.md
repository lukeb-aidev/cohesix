<!-- Author: Lukas Bower -->
# Toolchain Setup — macOS 26 (Apple Silicon M4)

## 1. Homebrew Prerequisites
```bash
brew update
brew install git cmake ninja llvm@17 python@3 qemu coreutils jq
```
- Use Homebrew-provided `llvm@17` for LLD; export `PATH="/opt/homebrew/opt/llvm/bin:$PATH"` when building seL4.

## 2. Rust Toolchain
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
source "$HOME/.cargo/env"
rustup component add rustfmt clippy
rustc --version
```

## 3. QEMU Validation
```bash
qemu-system-aarch64 --version | head -n1
```
- Expect version ≥ 9.0 with `--machine virt,gic-version=3` support.

## 4. seL4 External Build (reference)
1. Clone upstream seL4 and tool repo at compatible tags.
2. Configure for `aarch64` + `qemu_arm_virt` platform with `CROSS_COMPILER_PREFIX=aarch64-none-elf-`.
3. Produce `elfloader`, `kernel.elf`, and a placeholder `rootfs.cpio` (may be empty initially).
4. Store artefacts under `out/` (not committed) and run:
   ```bash
   ./scripts/qemu-run.sh out/elfloader out/kernel.elf out/rootfs.cpio
   ```

## 5. Developer Quality-of-Life
- Install `just` (optional) for task orchestration.
- Use `cargo install cargo-nextest` for faster test runs.
- Configure VS Code or Neovim with Rust Analyzer pointing at the workspace root.

## 6. Continuous Integration Expectations
- CI runners must preinstall QEMU and set `RUSTFLAGS="-C target-cpu=apple-m4"` for performance parity.
- Provide a cached seL4 build or mock out seL4 dependencies when running unit tests.
