<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Documents macOS ARM64 toolchain requirements and setup steps. -->
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
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.93.1
source "$HOME/.cargo/env"
rustup toolchain install 1.93.1
rustup override set 1.93.1
rustup component add rustfmt clippy --toolchain 1.93.1
rustup target add aarch64-unknown-none --toolchain 1.93.1
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
- QEMU launchers auto-select `hvf` on macOS (fallback to `tcg` if unavailable).
  Override with `COHESIX_QEMU_ACCEL` or `QEMU_ACCEL` (for example, `COHESIX_QEMU_ACCEL=tcg`).

## 4. seL4 External Build (reference)
1. Use upstream seL4 15.0.0 as the accepted kernel baseline for Milestone 26d.
   The current local source is `/Users/lukasbower/seL4_15` at upstream commit
   `881de507fe528490dc5e570c7810a149bad5880f`, with the Cohesix Pi 4 VL805
   BAR0 device-untyped overlay patch recorded in `docs/audit/M26D_SEL4_15_PROVENANCE.md`.
2. Activate the matching seL4 15 Python environment before configuring kernel
   profiles:
   ```bash
   source "$HOME/seL4_15/.venv_aarch64/bin/activate"
   ```
3. The checked-in profile artifact trees are:
   - `seL4/build` — QEMU `aarch64/virt`, single-core.
   - `seL4/SMP_build` — QEMU `aarch64/virt`, four-node SMP.
   - `seL4/build_UBOOT` — Raspberry Pi 4 `bcm2711`, U-Boot image handoff,
     VCNT-only EL0 counter export, `TIMER_CLOCK_HZ=54000000`.
4. Configure refreshed trees with `KERNEL_PATH=$HOME/seL4_15` and the matching
   `KERNEL_HELPERS_PATH` / `KERNEL_CONFIG_PATH` values from that source tree.
   One-domain profiles must not retain stale `KernelDomainSchedule` cache
   entries after configure or build.
5. Configure both QEMU trees with `ElfloaderRootserversLast=ON` and regenerate
   their embedded QEMU DTB from
   `virt,secure=off,virtualization=on,gic-version=2`. The generated DTS must
   record PSCI `method = "smc"` so the elfloader and the Cohesix QEMU launcher
   agree on the seL4 15 SMP boot path.
6. Configure the Pi 4 U-Boot tree with `ElfloaderRootserversLast=ON` and
   `IMAGE_START_ADDR=0x10000000` as well. The QEMU PSCI/DTB setting is not used
   for Pi U-Boot, but the seL4 15 rootserver-placement guard is shared across
   the refreshed elfloader profiles. The fixed Pi image start preserves the
   known-working U-Boot `bootm` XIP handoff shape; a shoehorn-computed lower
   address makes U-Boot treat the payload as a relocatable Linux `Image` and
   reject the seL4 elfloader before seL4 starts.
7. Store transient rebuild outputs under `out/` and replace the checked-in
   profile artifact trees only after the generated caches point at the accepted
   seL4 15 source and pass the profile-specific guards.
8. For direct QEMU experiments, run the helper with explicit paths once the
   Rust root task has been compiled:
   ```bash
   scripts/qemu-run.sh \
     --elfloader out/elfloader \
     --kernel out/kernel.elf \
     --root-task target/aarch64-unknown-none/release/root-task \
     --out-dir out/qemu-direct
   ```
9. The Cohesix build harness copies `elfloader` into its staging directory and
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
