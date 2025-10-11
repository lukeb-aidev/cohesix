# Toolchain — macOS 26 (Apple Silicon M4)
brew install git cmake ninja llvm@17 python@3 qemu coreutils
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
qemu-system-aarch64 --version | head -n1
