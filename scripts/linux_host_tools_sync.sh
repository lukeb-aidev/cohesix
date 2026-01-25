#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Refresh Linux host tools on a remote Ubuntu builder and sync them locally.

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/linux_host_tools_sync.sh --host <ip> [options]

Options:
  --host <ip>           Ubuntu builder IP or hostname (required)
  --user <name>         SSH username (default: ubuntu)
  --key <path>          SSH private key (default: ~/.ssh/cohesix-builder-key.pem)
  --remote-dir <path>   Remote work dir (default: /home/<user>/cohesix-host-tools)
  --local-out <path>    Local host-tools dir (default: out/cohesix/host-tools-linux)
  --no-clean            Skip remote cleanup before copy
  --full-clean          Remove the entire remote work dir (slow)
  --no-bundle           Skip running scripts/release_bundle.sh after sync
  -h, --help            Show this help
USAGE
}

HOST=""
USER="ubuntu"
KEY_PATH="${HOME}/.ssh/cohesix-builder-key.pem"
REMOTE_DIR=""
LOCAL_OUT="out/cohesix/host-tools-linux"
CLEAN=1
FULL_CLEAN=0
BUNDLE=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --host)
      HOST="$2"
      shift 2
      ;;
    --user)
      USER="$2"
      shift 2
      ;;
    --key)
      KEY_PATH="$2"
      shift 2
      ;;
    --remote-dir)
      REMOTE_DIR="$2"
      shift 2
      ;;
    --local-out)
      LOCAL_OUT="$2"
      shift 2
      ;;
    --no-clean)
      CLEAN=0
      shift
      ;;
    --full-clean)
      FULL_CLEAN=1
      shift
      ;;
    --no-bundle)
      BUNDLE=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ -z "$HOST" ]]; then
  echo "--host is required" >&2
  usage
  exit 1
fi

if [[ -z "$REMOTE_DIR" ]]; then
  REMOTE_DIR="/home/${USER}/cohesix-host-tools"
fi

if [[ ! -f "$KEY_PATH" ]]; then
  echo "SSH key not found: $KEY_PATH" >&2
  exit 1
fi

SSH_OPTS=(
  -i "$KEY_PATH"
  -o BatchMode=yes
  -o StrictHostKeyChecking=accept-new
)

run_ssh() {
  ssh "${SSH_OPTS[@]}" "${USER}@${HOST}" "$@"
}

SRC_TARBALL="/tmp/cohesix-host-tools-src.tar.gz"
REMOTE_TARBALL="/home/${USER}/cohesix-src.tar.gz"
REMOTE_TOOLS_TARBALL="/home/${USER}/host-tools-linux.tar.gz"

printf "[sync] Packaging host-tool sources...\n"
rm -f "$SRC_TARBALL"
{
  printf '%s\0' Cargo.toml Cargo.lock .cargo/config.toml scripts/rustc-wrapper.sh
  git ls-files -z apps crates tools tests resources
} | tar --null -T - -czf "$SRC_TARBALL"

if [[ "$CLEAN" -eq 1 ]]; then
  if [[ "$FULL_CLEAN" -eq 1 ]]; then
    printf "[sync] Cleaning remote workspace (full)...\n"
    run_ssh "rm -rf '${REMOTE_DIR}' && mkdir -p '${REMOTE_DIR}'"
  else
    printf "[sync] Cleaning remote workspace (fast)...\n"
    run_ssh "mkdir -p '${REMOTE_DIR}' && rm -rf '${REMOTE_DIR}/apps' '${REMOTE_DIR}/crates' '${REMOTE_DIR}/tools' '${REMOTE_DIR}/tests' '${REMOTE_DIR}/resources'"
  fi
else
  run_ssh "mkdir -p '${REMOTE_DIR}'"
fi

printf "[sync] Copying source tarball...\n"
scp "${SSH_OPTS[@]}" "$SRC_TARBALL" "${USER}@${HOST}:${REMOTE_TARBALL}"

printf "[sync] Extracting source on remote...\n"
run_ssh "tar -xzf '${REMOTE_TARBALL}' -C '${REMOTE_DIR}' && rm -f '${REMOTE_TARBALL}'"

printf "[sync] Installing build dependencies...\n"
run_ssh "set -euo pipefail
  if ! dpkg -s libwebkit2gtk-4.0-dev libjavascriptcoregtk-4.0-dev >/dev/null 2>&1; then
    sudo tee /etc/apt/sources.list.d/cohesix-jammy.list >/dev/null <<'EOF'
deb http://ports.ubuntu.com/ubuntu-ports jammy main universe
deb http://ports.ubuntu.com/ubuntu-ports jammy-updates main universe
deb http://ports.ubuntu.com/ubuntu-ports jammy-security main universe
EOF
    sudo tee /etc/apt/preferences.d/cohesix-jammy >/dev/null <<'EOF'
Package: *
Pin: release n=jammy
Pin-Priority: 100

Package: libwebkit2gtk-4.0-*
Pin: release n=jammy
Pin-Priority: 990

Package: libjavascriptcoregtk-4.0-*
Pin: release n=jammy
Pin-Priority: 990

Package: gir1.2-javascriptcoregtk-4.0
Pin: release n=jammy
Pin-Priority: 990
EOF
    sudo apt-get update -y
    sudo apt-get install -y libwebkit2gtk-4.0-dev libjavascriptcoregtk-4.0-dev
  fi
  if ! dpkg -s build-essential pkg-config libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev curl libfuse3-dev libnvidia-ml-dev >/dev/null 2>&1; then
    sudo apt-get update -y
    sudo apt-get install -y build-essential pkg-config libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev curl libfuse3-dev libnvidia-ml-dev
  fi"

printf "[sync] Ensuring Rust toolchain...\n"
run_ssh "command -v cargo >/dev/null 2>&1 || curl https://sh.rustup.rs -sSf | sh -s -- -y"

printf "[sync] Building Linux host tools...\n"
run_ssh "source \$HOME/.cargo/env && cd '${REMOTE_DIR}' && \
  export CARGO_BUILD_JOBS=1; \
  cargo build --release -p gpu-bridge-host && \
  cargo build --release -p cas-tool && \
  cargo build --release -p host-sidecar-bridge --features tcp && \
  cargo build --release -p cohsh --features tcp && \
  cargo build --release -p coh --features fuse,nvml && \
  RUSTFLAGS='-C debuginfo=0' cargo build --release -p swarmui"

printf "[sync] Staging host tool binaries...\n"
run_ssh "mkdir -p '${REMOTE_DIR}/out/host-tools-linux' && \
  install -m 0755 '${REMOTE_DIR}/target/release/cohsh' '${REMOTE_DIR}/out/host-tools-linux/' && \
  install -m 0755 '${REMOTE_DIR}/target/release/coh' '${REMOTE_DIR}/out/host-tools-linux/' && \
  install -m 0755 '${REMOTE_DIR}/target/release/gpu-bridge-host' '${REMOTE_DIR}/out/host-tools-linux/' && \
  install -m 0755 '${REMOTE_DIR}/target/release/host-sidecar-bridge' '${REMOTE_DIR}/out/host-tools-linux/' && \
  install -m 0755 '${REMOTE_DIR}/target/release/cas-tool' '${REMOTE_DIR}/out/host-tools-linux/' && \
  install -m 0755 '${REMOTE_DIR}/target/release/swarmui' '${REMOTE_DIR}/out/host-tools-linux/'"

printf "[sync] Packing host tools for transfer...\n"
run_ssh "tar -C '${REMOTE_DIR}/out' -czf '${REMOTE_TOOLS_TARBALL}' host-tools-linux"

printf "[sync] Downloading host tools...\n"
mkdir -p "$(dirname "$LOCAL_OUT")"
scp "${SSH_OPTS[@]}" "${USER}@${HOST}:${REMOTE_TOOLS_TARBALL}" "/tmp/host-tools-linux.tar.gz"

tar -xzf "/tmp/host-tools-linux.tar.gz" -C "$(dirname "$LOCAL_OUT")"
rm -f "/tmp/host-tools-linux.tar.gz"
run_ssh "rm -f '${REMOTE_TOOLS_TARBALL}'"

printf "[sync] Linux host tools synced to %s\n" "$LOCAL_OUT"

if [[ "$BUNDLE" -eq 1 ]]; then
  printf "[sync] Refreshing release bundles...\n"
  LINUX_HOST_TOOLS_DIR="$LOCAL_OUT" ./scripts/release_bundle.sh --force --linux
fi

printf "[sync] Done.\n"
