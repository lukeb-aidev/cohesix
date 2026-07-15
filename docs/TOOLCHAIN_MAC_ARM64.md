<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define the reproducible macOS Apple Silicon host toolchain and external seL4 build contract. -->
<!-- Author: Lukas Bower -->

# Toolchain Setup — macOS 26 on Apple Silicon

macOS 26 on Apple Silicon is the primary Cohesix development host. This guide
installs host dependencies and explains how the current tree consumes, but does
not vendor, upstream seL4 build outputs.

## Supported toolchain

| Component | Current contract | Source of truth |
| --- | --- | --- |
| Rust | 1.93.1, `rustfmt`, `clippy`, `aarch64-unknown-none` | `rust-toolchain.toml` |
| Host packages | Git, CMake, Ninja, LLVM 17, Python 3, QEMU, coreutils, `jq` | `toolchain/setup_macos_arm64.sh` |
| Kernel baseline | Upstream seL4 15.0.0 at commit `881de507fe528490dc5e570c7810a149bad5880f` | `docs/audit/M26D_SEL4_15_PROVENANCE.md` |
| Target artifacts | Profile-selected seL4 generated headers, configuration, kernel, and elfloader outputs | `SEL4_BUILD_DIR` / `--sel4-build` |

The official kernel interface reference is the
[seL4 Reference Manual 15.0.0](https://sel4.systems/Info/Docs/seL4-manual-15.0.0.pdf).

## 1. Install host and Rust dependencies

The repository script is the canonical setup path:

```bash
./toolchain/setup_macos_arm64.sh
source "$HOME/.cargo/env"
```

It uses Homebrew to install missing packages, installs the pinned Rust
toolchain and components, and verifies `qemu-system-aarch64`. It does not build
seL4 or download target artifacts.

Verify the result:

```bash
rustc --version
rustup target list --installed | rg '^aarch64-unknown-none$'
qemu-system-aarch64 --version | head -n 1
cmake --version | head -n 1
ninja --version
python3 --version
```

Homebrew's LLVM 17 is available at `/opt/homebrew/opt/llvm/bin` on the standard
Apple Silicon prefix. Add it only for commands that need that toolchain:

```bash
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
```

Do not set model-specific flags such as `target-cpu=apple-m4` in shared scripts
or CI. Host binaries and caches must remain portable across supported Apple
Silicon machines.

## 2. Prepare the external seL4 source

seL4 source remains outside this repository. Parameterize its location rather
than embedding a developer home directory:

```bash
export COHESIX_SEL4_SOURCE="${COHESIX_SEL4_SOURCE:-$HOME/seL4_15}"
test -d "$COHESIX_SEL4_SOURCE"
git -C "$COHESIX_SEL4_SOURCE" rev-parse HEAD
```

The accepted Milestone 26d baseline is upstream seL4 15.0.0 at the commit in
the table above. The Pi 4 profile also uses the Cohesix VL805 high-BAR
device-untyped overlay recorded, hashed, and bounded in
[`docs/audit/M26D_SEL4_15_PROVENANCE.md`](audit/M26D_SEL4_15_PROVENANCE.md).
Do not silently apply a different kernel revision or local patch set.

Use the Python environment associated with that source tree when configuring
or rebuilding kernel profiles:

```bash
source "$COHESIX_SEL4_SOURCE/.venv_aarch64/bin/activate"
```

If that environment does not exist, follow the upstream seL4 15.0.0 setup
instructions and record the created environment in local build provenance.

## 3. Select generated kernel artifacts

The repository currently uses these profile-qualified output conventions:

| Directory | Target profile | Important boundary |
| --- | --- | --- |
| `seL4/build` | QEMU `aarch64/virt`, single core | Development/reference output |
| `seL4/SMP_build` | QEMU `aarch64/virt`, four-node SMP | Default current-source QEMU path |
| `seL4/build_UBOOT` | Raspberry Pi 4 `bcm2711` | Pi firmware → U-Boot → seL4 binary image |

These directories contain generated kernel truth, not vendored seL4 source.
The selected directory must match the intended target and root-task ABI:

```bash
export SEL4_BUILD_DIR="$PWD/seL4/SMP_build"
test -f "$SEL4_BUILD_DIR/kernel/autoconf/autoconf.h"
test -d "$SEL4_BUILD_DIR/libsel4/include"
```

Root-task compilation reads generated headers and configuration from this
directory. Do not combine a kernel or elfloader from one profile with headers,
slot layouts, or root-task artifacts from another.

QEMU launchers inspect the selected seL4 configuration and choose matching
machine details, including the GIC revision. An explicit override is valid only
when it agrees with that generated configuration. Pi 4 builds require the
generated virtual-counter export and `TIMER_CLOCK_HZ=54000000`; target timeout
logic must not substitute CPU-speed loops or physical-counter access.

## 4. Build the current QEMU profile

The integrated build stages the selected elfloader and kernel, regenerates
manifest outputs, builds the Rust target and host tools, assembles the rootfs,
and launches QEMU:

```bash
./scripts/cohesix-build-run.sh \
  --sel4-build "$SEL4_BUILD_DIR" \
  --out-dir out/cohesix \
  --profile release \
  --root-task-features cohesix-dev \
  --cargo-target aarch64-unknown-none \
  --transport tcp
```

Use `--no-run` to stage artifacts without claiming a boot. Use `--transport
qemu` when `cohsh` should own QEMU without exposing the guest TCP listener.
See [Quickstart](QUICKSTART.md) for the verified connection flow.

The workspace disables incremental compilation and routes Rust invocations
through `scripts/rustc-wrapper.sh` to avoid APFS temporary-directory races.
Those settings live in `.cargo/config.toml`; do not duplicate them in local or
CI commands.

## 5. Validate toolchain and generated alignment

Before attributing a failure to the target, record:

```bash
git rev-parse HEAD
rustc --version --verbose
qemu-system-aarch64 --version | head -n 1
git -C "$COHESIX_SEL4_SOURCE" rev-parse HEAD
shasum -a 256 "$SEL4_BUILD_DIR/kernel/autoconf/autoconf.h"
```

Run repository guards from the workspace root:

```bash
scripts/check-generated.sh
scripts/ci/check_test_plan.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace
```

Target-specific acceptance remains in the staged Test Plan. A host build proves
neither a QEMU boot nor Pi 4 behavior; retain the command, selected profile,
manifest fingerprint, and evidence directory for each claim.

## 6. Rebuilding seL4 profiles

Kernel-profile regeneration is scoped work, not a routine cleanup step. Follow
the active task in [Build plan](BUILD_PLAN.md) and the accepted values in the
provenance ledger. In particular:

- keep `ElfloaderRootserversLast=ON` for the accepted seL4 15 profiles;
- keep Pi 4 `IMAGE_START_ADDR=0x10000000` for the U-Boot `bootm` XIP handoff;
- generate QEMU DTB/PSCI settings from the selected profile rather than copying
  values between QEMU and Pi builds;
- store transient outputs under `out/` and replace tracked reference outputs
  only after profile guards pass;
- use `scripts/lib/strip_elfloader_modules.py` through the integrated harness so
  staged elfloaders do not retain an upstream test rootserver.

The detailed image, flash, boot, and current-image evidence procedure is in
[Hardware bring-up](HARDWARE_BRINGUP.md).
