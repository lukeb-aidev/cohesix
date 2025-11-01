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
- Incremental builds are forcibly disabled (`CARGO_INCREMENTAL=0`) for the
  entire workspace via `.cargo/config.toml`. APFS on macOS 26 occasionally drops
  the temporary directories that Rust's incremental engine relies on, which was
  manifesting as `No such file or directory` errors when crates like `zerocopy`
  or `serde` attempted to emit their rmeta artefacts under `target/debug/deps`.
  The global toggle keeps both seL4-targeted crates (`root-task`, `nine-door`,
  `worker-heart`, `worker-gpu`) and host-side tooling reliable, at the cost of
  slightly longer recompiles. The workspace also routes every `rustc`
  invocation through `scripts/rustc-wrapper.sh`, which pre-creates the dep-info
  and artefact directories so APFS/iCloud clean-ups cannot race the compiler.

## 3. QEMU Validation
```bash
qemu-system-aarch64 --version | head -n1
```
- Expect version ≥ 9.0 with `--machine virt,gic-version=3` support.
- `scripts/cohesix-build-run.sh` inspects the seL4 build `.config` to decide which
  GIC revision to request from QEMU. Ensure the kernel configuration enables
  GICv3 when following the architecture plan; the script will fall back to
  `gic-version=2` only when the build explicitly disables v3 support.

## 4. seL4 External Build (reference)
1. Clone upstream seL4 and tool repo at compatible tags.
2. Configure for `aarch64` + `qemu_arm_virt` platform with `CROSS_COMPILER_PREFIX=aarch64-none-elf-`.
3. Produce `elfloader`, `kernel.elf`, and a placeholder `rootfs.cpio` (may be empty initially).
4. Store artefacts under `out/` (not committed) and run the helper with
   explicit paths once the Rust root task has been compiled:
   ```bash
   scripts/qemu-run.sh \
     --elfloader out/elfloader \
     --kernel out/kernel.elf \
     --root-task target/aarch64-unknown-none/release/root-task \
     --out-dir out/qemu-direct
   ```
5. The Cohesix build harness copies `elfloader` into its staging directory and
   strips any baked-in kernel/root server payloads via
   `scripts/lib/strip_elfloader_modules.py`. This guarantees that the Rust
   `root-task` provided by the workspace becomes the first user task instead of
   the default `sel4test` module shipped with upstream builds.

## 5. Developer Quality-of-Life
- Install `just` (optional) for task orchestration.
- Use `cargo install cargo-nextest` for faster test runs.
- Configure VS Code or Neovim with Rust Analyzer pointing at the workspace root.

## 6. Continuous Integration Expectations
- CI runners must preinstall QEMU and set `RUSTFLAGS="-C target-cpu=apple-m4"` for performance parity.
- Provide a cached seL4 build or mock out seL4 dependencies when running unit tests.
