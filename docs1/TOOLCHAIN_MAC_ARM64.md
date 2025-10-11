# Toolchain — macOS 26 (Apple Silicon M4)

## Homebrew
```bash
brew update
brew install git cmake ninja llvm@17 python@3 qemu coreutils
```

## Rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
source "$HOME/.cargo/env"
rustc --version
```

## QEMU
```bash
qemu-system-aarch64 --version | head -n1
```

## seL4 (external build)
- Build upstream for **qemu-arm-virt (aarch64, GICv3)**.
- Produce: `out/elfloader`, `out/kernel.elf`, `out/rootfs.cpio`.
- Run: `./scripts/qemu-run.sh out/elfloader out/kernel.elf out/rootfs.cpio`
