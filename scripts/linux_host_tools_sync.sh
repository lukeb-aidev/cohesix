#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Refresh Linux host tools on a remote Ubuntu builder and sync them locally.
# Copyright 2026 Lukas Bower

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

remote_os_info() {
  run_ssh "source /etc/os-release && echo \${VERSION_ID:-} \${VERSION_CODENAME:-}"
}

SRC_TARBALL="/tmp/cohesix-host-tools-src.tar.gz"
REMOTE_TARBALL="/home/${USER}/cohesix-src.tar.gz"
REMOTE_TOOLS_TARBALL="/home/${USER}/host-tools-linux.tar.gz"
REMOTE_JAMMY_ROOT="${REMOTE_DIR}/.jammy-rootfs"
MAX_GLIBC_VERSION="${MAX_GLIBC_VERSION:-2.35}"

printf "[sync] Packaging host-tool sources...\n"
rm -f "$SRC_TARBALL"
{
  printf '%s\0' Cargo.toml Cargo.lock .cargo/config.toml scripts/rustc-wrapper.sh
  git ls-files -z --cached --others --exclude-standard apps crates tools tests resources
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

read -r REMOTE_VERSION_ID REMOTE_CODENAME <<<"$(remote_os_info)"
USE_JAMMY_CHROOT=0
if [[ "${REMOTE_VERSION_ID}" == 24.* || "${REMOTE_CODENAME}" == "noble" ]]; then
  USE_JAMMY_CHROOT=1
fi

if [[ "$USE_JAMMY_CHROOT" -eq 1 ]]; then
  printf "[sync] Remote host is Ubuntu %s (%s); building in jammy chroot for glibc 2.35 compatibility...\n" \
    "$REMOTE_VERSION_ID" "${REMOTE_CODENAME:-unknown}"
  run_ssh "sudo apt-get update -y && sudo apt-get install -y debootstrap"
  run_ssh "if [[ ! -d '${REMOTE_JAMMY_ROOT}' ]]; then \
    sudo debootstrap --arch=arm64 jammy '${REMOTE_JAMMY_ROOT}' http://ports.ubuntu.com/ubuntu-ports; \
    sudo cp /etc/resolv.conf '${REMOTE_JAMMY_ROOT}/etc/resolv.conf'; \
  fi"
  run_ssh "sudo tee '${REMOTE_JAMMY_ROOT}/etc/apt/sources.list' >/dev/null <<'EOF'
deb http://ports.ubuntu.com/ubuntu-ports jammy main universe multiverse
deb http://ports.ubuntu.com/ubuntu-ports jammy-updates main universe multiverse
deb http://ports.ubuntu.com/ubuntu-ports jammy-security main universe multiverse
EOF"
  run_ssh "sudo mkdir -p '${REMOTE_JAMMY_ROOT}/work' '${REMOTE_JAMMY_ROOT}/proc' '${REMOTE_JAMMY_ROOT}/sys' '${REMOTE_JAMMY_ROOT}/dev'"
  run_ssh "sudo mountpoint -q '${REMOTE_JAMMY_ROOT}/proc' || sudo mount -t proc /proc '${REMOTE_JAMMY_ROOT}/proc'"
  run_ssh "sudo mountpoint -q '${REMOTE_JAMMY_ROOT}/sys' || sudo mount --bind /sys '${REMOTE_JAMMY_ROOT}/sys'"
  run_ssh "sudo mountpoint -q '${REMOTE_JAMMY_ROOT}/dev' || sudo mount --bind /dev '${REMOTE_JAMMY_ROOT}/dev'"
  run_ssh "sudo mountpoint -q '${REMOTE_JAMMY_ROOT}/work' || sudo mount --bind '${REMOTE_DIR}' '${REMOTE_JAMMY_ROOT}/work'"
fi

printf "[sync] Installing build dependencies...\n"
if [[ "$USE_JAMMY_CHROOT" -eq 1 ]]; then
  run_ssh "sudo chroot '${REMOTE_JAMMY_ROOT}' /bin/bash -lc \"set -euo pipefail
    if ! dpkg -s libwebkit2gtk-4.0-dev libjavascriptcoregtk-4.0-dev >/dev/null 2>&1; then
      apt-get update -y
      apt-get install -y libwebkit2gtk-4.0-dev libjavascriptcoregtk-4.0-dev
    fi
    if ! dpkg -s build-essential pkg-config libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev curl libfuse3-dev libnvidia-ml-dev binutils >/dev/null 2>&1; then
      apt-get update -y
      apt-get install -y build-essential pkg-config libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev curl libfuse3-dev libnvidia-ml-dev binutils
    fi\""
else
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
    if ! dpkg -s build-essential pkg-config libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev curl libfuse3-dev libnvidia-ml-dev binutils >/dev/null 2>&1; then
      sudo apt-get update -y
      sudo apt-get install -y build-essential pkg-config libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev curl libfuse3-dev libnvidia-ml-dev binutils
    fi"
fi

printf "[sync] Ensuring Rust toolchain...\n"
if [[ "$USE_JAMMY_CHROOT" -eq 1 ]]; then
  run_ssh "sudo chroot '${REMOTE_JAMMY_ROOT}' /bin/bash -lc \"command -v cargo >/dev/null 2>&1 || curl https://sh.rustup.rs -sSf | sh -s -- -y\""
else
  run_ssh "command -v cargo >/dev/null 2>&1 || curl https://sh.rustup.rs -sSf | sh -s -- -y"
fi

printf "[sync] Building Linux host tools...\n"
if [[ "$USE_JAMMY_CHROOT" -eq 1 ]]; then
  run_ssh "sudo chroot '${REMOTE_JAMMY_ROOT}' /bin/bash -lc \"source /root/.cargo/env && cd '/work' && \
    export CARGO_BUILD_JOBS=1; \
    cargo build --release -p gpu-bridge-host && \
    cargo build --release -p cas-tool && \
    cargo build --release -p hive-gateway && \
    cargo build --release -p host-ticket-agent && \
    cargo build --release -p host-sidecar-bridge --features tcp && \
    cargo build --release -p cohsh --features tcp && \
    cargo build --release -p coh --features fuse,nvml && \
    RUSTFLAGS='-C debuginfo=0' cargo build --release -p swarmui\""
  run_ssh "sudo umount -l '${REMOTE_JAMMY_ROOT}/work' || true"
  run_ssh "sudo umount -l '${REMOTE_JAMMY_ROOT}/proc' || true"
  run_ssh "sudo umount -l '${REMOTE_JAMMY_ROOT}/sys' || true"
  run_ssh "sudo umount -l '${REMOTE_JAMMY_ROOT}/dev' || true"
else
  run_ssh "source \$HOME/.cargo/env && cd '${REMOTE_DIR}' && \
    export CARGO_BUILD_JOBS=1; \
    cargo build --release -p gpu-bridge-host && \
    cargo build --release -p cas-tool && \
    cargo build --release -p hive-gateway && \
    cargo build --release -p host-ticket-agent && \
    cargo build --release -p host-sidecar-bridge --features tcp && \
    cargo build --release -p cohsh --features tcp && \
    cargo build --release -p coh --features fuse,nvml && \
    RUSTFLAGS='-C debuginfo=0' cargo build --release -p swarmui"
fi

printf "[sync] Verifying GLIBC compatibility (<= %s)...\n" "$MAX_GLIBC_VERSION"
remote_glibc_cmd=$(cat <<EOF
set -euo pipefail
max_glibc="${MAX_GLIBC_VERSION}"
bins=(cohsh coh gpu-bridge-host host-sidecar-bridge cas-tool swarmui hive-gateway host-ticket-agent)
for bin in "\${bins[@]}"; do
  path="${REMOTE_DIR}/target/release/\${bin}"
  if [[ ! -x "\${path}" ]]; then
    echo "[sync] ERROR: missing expected binary \${path}" >&2
    exit 1
  fi
  ver=\$(strings "\${path}" | grep -o 'GLIBC_[0-9\\.]*' | sed 's/GLIBC_//' | sort -V | tail -n 1 || true)
  if [[ -n "\${ver}" ]]; then
    worst=\$(printf '%s\\n%s\\n' "\${max_glibc}" "\${ver}" | sort -V | tail -n 1)
    if [[ "\${worst}" != "\${max_glibc}" ]]; then
      echo "[sync] ERROR: \${bin} requires GLIBC_\${ver} (max allowed GLIBC_\${max_glibc})" >&2
      exit 1
    fi
  fi
  echo "[sync] \${bin} max GLIBC_\${ver:-unknown}"
done
EOF
)
run_ssh "bash -lc $(printf %q "${remote_glibc_cmd}")"

printf "[sync] Staging host tool binaries...\n"
run_ssh "mkdir -p '${REMOTE_DIR}/out/host-tools-linux' && \
  install -m 0755 '${REMOTE_DIR}/target/release/cohsh' '${REMOTE_DIR}/out/host-tools-linux/' && \
  install -m 0755 '${REMOTE_DIR}/target/release/coh' '${REMOTE_DIR}/out/host-tools-linux/' && \
  install -m 0755 '${REMOTE_DIR}/target/release/gpu-bridge-host' '${REMOTE_DIR}/out/host-tools-linux/' && \
  install -m 0755 '${REMOTE_DIR}/target/release/host-sidecar-bridge' '${REMOTE_DIR}/out/host-tools-linux/' && \
  install -m 0755 '${REMOTE_DIR}/target/release/cas-tool' '${REMOTE_DIR}/out/host-tools-linux/' && \
  install -m 0755 '${REMOTE_DIR}/target/release/swarmui' '${REMOTE_DIR}/out/host-tools-linux/' && \
  install -m 0755 '${REMOTE_DIR}/target/release/hive-gateway' '${REMOTE_DIR}/out/host-tools-linux/' && \
  install -m 0755 '${REMOTE_DIR}/target/release/host-ticket-agent' '${REMOTE_DIR}/out/host-tools-linux/'"

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
