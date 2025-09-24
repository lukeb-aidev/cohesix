// CLASSIFICATION: COMMUNITY
// Filename: MAC_M4_QEMU.md v0.1
// Author: Lukas Bower
// Date Modified: 2029-02-20

# Mac M4 End-to-End Build + QEMU Validation

Apple's M4 systems-on-chip are ARM64 (armv8.6+) devices. Cohesix already targets
aarch64 for seL4, so Mac M4 developers need to ensure the host toolchain mirrors
the Linux aarch64 flow while enabling QEMU's Hypervisor Framework (HVF)
acceleration. This guide documents the steps that now work on macOS 15 (Apple
Silicon) and mirrors the automation available on Ubuntu hosts.

## 1. Prerequisites

1. **Homebrew packages**
   ```bash
   brew update
   brew install qemu aarch64-unknown-linux-gnu llvm cmake ninja
   ```
   The Homebrew `aarch64-unknown-linux-gnu` formula provides the cross GCC that
   `cohesix_fetch_build.sh` and Rust expect. LLVM is used for `ld.lld`.
2. **Rust toolchains**
   ```bash
   rustup toolchain install stable nightly
   rustup component add rust-src --toolchain nightly
   rustup target add aarch64-unknown-none aarch64-unknown-linux-musl
   ```
3. **Python virtualenv support** (bundled with macOS Python 3.12 via Homebrew)
   ```bash
   brew install python@3.12
   ```
4. **Fresh metadata**
   ```bash
   scripts/verify-macos-setup.sh
   scripts/check-qemu-deps.sh
   ```
   The updated scripts normalize the `arm64` host architecture to `aarch64` and
   confirm QEMU firmware in `/opt/homebrew/share/qemu/`.

## 2. Workspace Preparation

1. Initialize Cohesix and normalize architecture metadata (writes
   `~/.cohesix_config` with `aarch64`):
   ```bash
   ./scripts/setup_build_env.sh
   ```
2. Confirm the loader picks up the normalized architecture:
   ```bash
   ./scripts/load_arch_config.sh --prompt   # optional confirmation
   ```
3. Hydrate dependencies:
   ```bash
   ./cohesix_fetch_build.sh --phase=1
   ./cohesix_fetch_build.sh --phase=2
   ./cohesix_fetch_build.sh --phase=3
   ```
   On macOS, the script now:
   - Accepts Homebrew's `aarch64-unknown-linux-gnu-gcc` cross compiler.
   - Skips the mandatory CUDA header check (CUDA is remote-only on Apple
     Silicon) while keeping the diagnostics visible in the build log.
   - Leaves `LD_LIBRARY_PATH` untouched if no local CUDA runtime exists.

## 3. QEMU Boot Validation

With the workspace populated, the HVF-aware launch scripts can boot the
assembled image quickly:

```bash
ci/qemu_boot_check.sh            # uses -accel hvf + -cpu host on Mac M4
scripts/boot_qemu.sh             # developer-facing boot helper with the same defaults
```

Both scripts automatically fall back to TCG if HVF is unavailable, but when QEMU
was installed via Homebrew on Apple Silicon, `-accel hvf` is detected and used.
Serial output is still captured in `log/qemu_debug_*.log` for audit trails.

## 4. Troubleshooting

| Symptom | Resolution |
|---------|------------|
| `aarch64 cross GCC missing` | Run `brew install aarch64-unknown-linux-gnu`. Confirm `which aarch64-unknown-linux-gnu-gcc`. |
| `ld.lld not found` | Use the Homebrew `llvm` package and ensure `/opt/homebrew/opt/llvm/bin` is on `PATH`. |
| QEMU reports `hvf` unavailable | Run `qemu-system-aarch64 -accel help` to verify HVF support. If not present, reinstall QEMU (`brew reinstall qemu`) and reboot to reload hypervisor entitlements. |
| CUDA warning in build logs | Expected: CUDA workloads stay remote via Secure9P. Ensure `/srv/cuda` points at the remote enclave before executing GPU jobs. |
| Boot scripts still see `arm64` | Delete `~/.cohesix_config` and rerun `scripts/setup_build_env.sh` so the normalized `aarch64` value is written. |

Following these steps ensures the Mac M4 developer flow produces the same
artifacts as the Ubuntu CI pipeline, while the new HVF-aware QEMU paths keep the
boot loop fast enough for iterative debugging.
