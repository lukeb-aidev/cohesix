#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Ensure host runtime dependencies are installed for running Cohesix release bundles.
# Copyright 2026 Lukas Bower

set -euo pipefail

CHECK_ONLY=0
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUNDLE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
RELEASE_WHEEL=""

usage() {
  cat <<'EOF'
Usage: scripts/setup_environment.sh [--check]

Install the runtime dependencies for a Cohesix release bundle on macOS 26 or
later on Apple Silicon, or Ubuntu 22.04, 24.04, or 26.04 on ARM64. When the
bundle contains its Python wheel, setup also creates .venv and installs it.

Options:
  --check  Verify the host and dependencies without installing packages.
EOF
}

log() {
  printf "[setup] %s\n" "$*"
}

fail() {
  printf "[setup] error: %s\n" "$*" >&2
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --check)
      CHECK_ONLY=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

ensure_cmd() {
  local cmd="$1"
  command -v "$cmd" >/dev/null 2>&1
}

ensure_pkg_ubuntu() {
  local pkg="$1"
  dpkg -s "$pkg" >/dev/null 2>&1
}

detect_release_wheel() {
  local -a wheels=()
  shopt -s nullglob
  wheels=("${BUNDLE_ROOT}"/python/dist/*.whl)
  shopt -u nullglob
  if [[ "${#wheels[@]}" -gt 1 ]]; then
    fail "release bundle contains multiple Python wheels under python/dist"
  fi
  if [[ "${#wheels[@]}" -eq 1 ]]; then
    RELEASE_WHEEL="${wheels[0]}"
    if [[ ! -f "$RELEASE_WHEEL" || -L "$RELEASE_WHEEL" ]]; then
      fail "bundled Python wheel must be a regular, non-symlinked file"
    fi
  fi
}

python_is_supported() {
  local python_bin="$1"
  "$python_bin" -c \
    'import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)' \
    >/dev/null 2>&1
}

setup_release_venv() {
  local python_bin="$1"
  if [[ -z "$RELEASE_WHEEL" ]]; then
    log "No bundled Python wheel found; skipping release .venv setup."
    return 0
  fi
  if ! ensure_cmd "$python_bin" || ! python_is_supported "$python_bin"; then
    fail "the bundled Python client requires Python 3.11 or later: ${python_bin}"
  fi

  local venv_dir="${BUNDLE_ROOT}/.venv"
  if [[ -L "$venv_dir" ]]; then
    fail "refusing symlinked release Python environment: ${venv_dir}"
  fi
  if [[ -e "$venv_dir" && ! -x "$venv_dir/bin/python" ]]; then
    fail "${venv_dir} exists but is not a usable virtual environment"
  fi
  if [[ ! -d "$venv_dir" ]]; then
    if [[ "$CHECK_ONLY" -eq 1 ]]; then
      fail "release .venv is missing; rerun without --check to create it"
    fi
    log "Creating release Python environment at ${venv_dir}."
    "$python_bin" -m venv "$venv_dir"
  fi

  local venv_python="${venv_dir}/bin/python"
  if ! python_is_supported "$venv_python"; then
    fail "existing release .venv uses Python older than 3.11"
  fi
  if [[ "$CHECK_ONLY" -eq 0 ]]; then
    log "Installing bundled Cohesix Python client into .venv."
    "$venv_python" -m pip install \
      --disable-pip-version-check \
      --no-index \
      --no-deps \
      --force-reinstall \
      "$RELEASE_WHEEL"
  fi
  local wheel_filename="${RELEASE_WHEEL##*/}"
  local expected_version="${wheel_filename#cohesix-}"
  expected_version="${expected_version%%-*}"
  local actual_version
  actual_version=$(
    "$venv_python" -c \
      'import cohesix, importlib.metadata; print(importlib.metadata.version("cohesix"))'
  )
  if [[ "$actual_version" != "$expected_version" ]]; then
    fail "bundled Cohesix Python version mismatch: expected ${expected_version}, got ${actual_version}"
  fi
  log "Cohesix Python: ${actual_version}"
}

install_apt_packages() {
  local -a pkgs=("$@")
  if [[ "${#pkgs[@]}" -eq 0 ]]; then
    return 0
  fi
  local -a apt_prefix=(apt-get)
  if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
    if ensure_cmd sudo; then
      apt_prefix=(sudo apt-get)
    else
      fail "sudo is required to install packages: ${pkgs[*]}"
    fi
  fi
  log "Installing packages: ${pkgs[*]}"
  DEBIAN_FRONTEND=noninteractive "${apt_prefix[@]}" update -y
  DEBIAN_FRONTEND=noninteractive "${apt_prefix[@]}" install -y "${pkgs[@]}"
}

enable_ubuntu_universe() {
  if ! ensure_cmd add-apt-repository; then
    install_apt_packages software-properties-common
  fi

  local -a privilege_prefix=()
  if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
    if ensure_cmd sudo; then
      privilege_prefix=(sudo)
    else
      fail "sudo is required to enable the Ubuntu Universe repository"
    fi
  fi

  log "Ensuring the Ubuntu Universe repository is enabled."
  "${privilege_prefix[@]}" add-apt-repository -y universe
}

require_qemu_accel() {
  local accel="$1"
  local advertised
  advertised="$(qemu-system-aarch64 -accel help 2>/dev/null || true)"
  if [[ -z "$advertised" ]] || ! grep -Eq \
    "(^|[[:space:]])${accel}($|[[:space:]])" <<<"$advertised"
  then
    fail "qemu-system-aarch64 does not advertise the required ${accel} accelerator"
  fi
}

setup_macos() {
  if [[ "$(uname -m)" != "arm64" ]]; then
    fail "the macOS release bundle requires Apple Silicon; detected $(uname -m)"
  fi

  local version
  version="$(sw_vers -productVersion 2>/dev/null || true)"
  if [[ -z "$version" ]]; then
    fail "sw_vers not available; unable to detect macOS version"
  fi
  local major="${version%%.*}"
  if [[ ! "$major" =~ ^[0-9]+$ ]] || (( major < 26 )); then
    fail "macOS 26 or later is required; detected ${version}"
  fi

  if ensure_cmd qemu-system-aarch64; then
    log "qemu-system-aarch64 already available."
  elif [[ "$CHECK_ONLY" -eq 1 ]]; then
    fail "qemu-system-aarch64 is missing; rerun without --check to install it"
  else
    if ! ensure_cmd brew; then
      fail "Homebrew not found. Install from https://brew.sh and re-run."
    fi
    log "qemu-system-aarch64 not found; installing qemu via Homebrew."
    brew install qemu
  fi

  ensure_cmd qemu-system-aarch64 || \
    fail "qemu-system-aarch64 is unavailable after setup"
  require_qemu_accel hvf

  if [[ -n "$RELEASE_WHEEL" ]]; then
    local release_python=""
    if ensure_cmd python3 && python_is_supported "$(command -v python3)"; then
      release_python="$(command -v python3)"
    elif ensure_cmd brew; then
      local brew_python
      brew_python="$(brew --prefix python@3.13 2>/dev/null)/bin/python3.13"
      if [[ -x "$brew_python" ]] && python_is_supported "$brew_python"; then
        release_python="$brew_python"
      fi
    fi
    if [[ -z "$release_python" ]]; then
      if [[ "$CHECK_ONLY" -eq 1 ]]; then
        fail "Python 3.11 or later is missing; rerun without --check to install it"
      fi
      ensure_cmd brew || \
        fail "Homebrew is required to install Python 3.13 for the release client"
      log "Installing Python 3.13 via Homebrew for the release client."
      brew install python@3.13
      release_python="$(brew --prefix python@3.13)/bin/python3.13"
    fi
    setup_release_venv "$release_python"
  fi
}

setup_ubuntu() {
  case "$(uname -m)" in
    aarch64|arm64) ;;
    *) fail "the Linux release bundle requires ARM64; detected $(uname -m)" ;;
  esac

  if [[ ! -f /etc/os-release ]]; then
    fail "/etc/os-release not found; unable to detect Linux distribution"
  fi
  # shellcheck disable=SC1091
  . /etc/os-release
  if [[ "${ID:-}" != "ubuntu" ]]; then
    fail "unsupported Linux distribution: ${ID:-unknown} (expected ubuntu)"
  fi
  local ubuntu_version="${VERSION_ID:-unknown}"

  case "$ubuntu_version" in
    22.04|24.04|26.04) ;;
    *)
      fail "unsupported Ubuntu version: ${ubuntu_version} (expected 22.04, 24.04, or 26.04)"
      ;;
  esac

  local -a missing=()

  if ! ensure_cmd qemu-system-aarch64; then
    missing+=("qemu-system-arm")
  fi

  local gtk_runtime
  if [[ "$ubuntu_version" == "22.04" ]]; then
    gtk_runtime="libgtk-3-0"
  else
    gtk_runtime="libgtk-3-0t64"
  fi

  local -a runtime_pkgs=(
    "libwebkit2gtk-4.1-0"
    "libjavascriptcoregtk-4.1-0"
    "libayatana-appindicator3-1"
    "librsvg2-2"
    "libfuse3-3"
    "libxdo3"
    "$gtk_runtime"
  )

  local release_python=""
  if [[ -n "$RELEASE_WHEEL" ]]; then
    if [[ "$ubuntu_version" == "22.04" ]]; then
      runtime_pkgs+=("python3.11" "python3.11-venv")
      release_python="python3.11"
    else
      runtime_pkgs+=("python3" "python3-venv")
      release_python="python3"
    fi
  fi
  for pkg in "${runtime_pkgs[@]}"; do
    if ! ensure_pkg_ubuntu "$pkg"; then
      missing+=("$pkg")
    fi
  done

  if [[ "${#missing[@]}" -eq 0 ]]; then
    log "All runtime packages already installed."
  elif [[ "$CHECK_ONLY" -eq 1 ]]; then
    fail "missing runtime packages: ${missing[*]}; rerun without --check to install them"
  else
    enable_ubuntu_universe
    install_apt_packages "${missing[@]}"
  fi

  ensure_cmd qemu-system-aarch64 || \
    fail "qemu-system-aarch64 is unavailable after setup"
  for pkg in "${runtime_pkgs[@]}"; do
    ensure_pkg_ubuntu "$pkg" || \
      fail "runtime package is unavailable after setup: ${pkg}"
  done
  require_qemu_accel tcg
  if [[ -n "$release_python" ]]; then
    setup_release_venv "$release_python"
  fi
}

detect_release_wheel

case "$(uname -s)" in
  Darwin)
    setup_macos
    ;;
  Linux)
    setup_ubuntu
    ;;
  *)
    fail "unsupported OS: $(uname -s)"
    ;;
esac

log "Environment setup complete."
