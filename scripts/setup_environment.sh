#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Ensure host runtime dependencies are installed for running Cohesix release bundles.

set -euo pipefail

log() {
  printf "[setup] %s\n" "$*"
}

warn() {
  printf "[setup] warning: %s\n" "$*" >&2
}

fail() {
  printf "[setup] error: %s\n" "$*" >&2
  exit 1
}

ensure_cmd() {
  local cmd="$1"
  command -v "$cmd" >/dev/null 2>&1
}

ensure_pkg_ubuntu() {
  local pkg="$1"
  dpkg -s "$pkg" >/dev/null 2>&1
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

setup_macos() {
  local version
  version="$(sw_vers -productVersion 2>/dev/null || true)"
  if [[ -z "$version" ]]; then
    fail "sw_vers not available; unable to detect macOS version"
  fi
  if [[ "$version" != 26.* ]]; then
    warn "expected macOS 26.x, detected ${version}"
  fi

  if ! ensure_cmd brew; then
    fail "Homebrew not found. Install from https://brew.sh and re-run."
  fi

  if ensure_cmd qemu-system-aarch64; then
    log "qemu-system-aarch64 already available."
    return 0
  fi

  log "qemu-system-aarch64 not found; installing qemu via Homebrew."
  brew install qemu
}

setup_ubuntu() {
  if [[ ! -f /etc/os-release ]]; then
    fail "/etc/os-release not found; unable to detect Linux distribution"
  fi
  # shellcheck disable=SC1091
  . /etc/os-release
  if [[ "${ID:-}" != "ubuntu" ]]; then
    fail "unsupported Linux distribution: ${ID:-unknown} (expected ubuntu)"
  fi
  local ubuntu_version="${VERSION_ID:-unknown}"
  local allow_unsupported="${COHESIX_ALLOW_UNSUPPORTED_UBUNTU:-}"

  local -a missing=()

  if ! ensure_cmd qemu-system-aarch64; then
    missing+=("qemu-system-aarch64")
  fi

  local -a runtime_pkgs=()
  if [[ "$ubuntu_version" == 24.* ]]; then
    runtime_pkgs+=(
      "libwebkit2gtk-4.1-0"
      "libjavascriptcoregtk-4.1-0"
      "libayatana-appindicator3-1"
      "librsvg2-2"
    )
  elif [[ "$ubuntu_version" == 22.04 ]]; then
    if ! apt-cache show libwebkit2gtk-4.1-0 >/dev/null 2>&1 || ! apt-cache show libjavascriptcoregtk-4.1-0 >/dev/null 2>&1; then
      fail "Ubuntu 22.04 must expose libwebkit2gtk-4.1-0 and libjavascriptcoregtk-4.1-0 for SwarmUI (Tauri 2). Enable jammy-updates/universe or use Ubuntu 24.x."
    fi
    runtime_pkgs+=(
      "libwebkit2gtk-4.1-0"
      "libjavascriptcoregtk-4.1-0"
      "libayatana-appindicator3-1"
      "librsvg2-2"
    )
  else
    if [[ -z "$allow_unsupported" ]]; then
      fail "unsupported Ubuntu version: ${ubuntu_version} (expected 24.x or 22.04). Set COHESIX_ALLOW_UNSUPPORTED_UBUNTU=1 to proceed."
    fi
    warn "unsupported Ubuntu version: ${ubuntu_version}; proceeding with best-effort package selection"
    runtime_pkgs+=(
      "libayatana-appindicator3-1"
      "librsvg2-2"
    )
    local found_webkit=0
    local -a webkit_candidates=(
      "libwebkit2gtk-4.1-0"
      "libjavascriptcoregtk-4.1-0"
    )
    for pkg in "${webkit_candidates[@]}"; do
      if apt-cache show "$pkg" >/dev/null 2>&1; then
        runtime_pkgs+=("$pkg")
        found_webkit=1
      fi
    done
    if [[ "$found_webkit" -eq 0 ]]; then
      warn "webkit 4.1 runtime packages not found in apt metadata; SwarmUI may not run until installed"
    fi
  fi
  for pkg in "${runtime_pkgs[@]}"; do
    if ! ensure_pkg_ubuntu "$pkg"; then
      missing+=("$pkg")
    fi
  done

  if ! ensure_pkg_ubuntu "libgtk-3-0" && ! ensure_pkg_ubuntu "libgtk-3-0t64"; then
    missing+=("libgtk-3-0")
  fi

  if [[ "${#missing[@]}" -eq 0 ]]; then
    log "All runtime packages already installed."
    return 0
  fi

  install_apt_packages "${missing[@]}"
}

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
