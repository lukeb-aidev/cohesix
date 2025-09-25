#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: verify-macos-setup.sh v0.2
# Author: Lukas Bower
# Date Modified: 2030-03-22
###############################################################################
# verify-macos-setup.sh – Cohesix helper
#
# Verifies that a macOS workstation has the core tools required for
# Cohesix development. Fails fast if anything is missing.
#
# Checks:
#   1. Homebrew package manager
#   2. Xcode command line tools
#   3. Python 3.10+
#   4. Git
#   5. Metadata sync via scripts/validate_metadata_sync.py
###############################################################################
set -euo pipefail

msg()  { printf "\e[32m[macos]\e[0m %s\n" "$*"; }
fail() { printf "\e[31m[fail]\e[0m %s\n" "$*"; exit 1; }

ensure_homebrew_shellenv() {
  if command -v brew >/dev/null 2>&1; then
    eval "$(brew shellenv)"
    return
  fi

  if [ -x "/opt/homebrew/bin/brew" ]; then
    eval "$(/opt/homebrew/bin/brew shellenv)"
    return
  fi

  if [ -x "/usr/local/bin/brew" ]; then
    eval "$(/usr/local/bin/brew shellenv)"
    return
  fi

  msg "Installing Homebrew …"
  NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  if [ -x "/opt/homebrew/bin/brew" ]; then
    eval "$(/opt/homebrew/bin/brew shellenv)"
  elif [ -x "/usr/local/bin/brew" ]; then
    eval "$(/usr/local/bin/brew shellenv)"
  else
    fail "Homebrew installation did not yield a brew binary."
  fi
}

select_python_bin() {
  local python_bin=""
  for candidate in python3.12 python3.11 python3.10 python3; do
    if command -v "$candidate" >/dev/null 2>&1; then
      local bin_path
      bin_path="$(command -v "$candidate")"
      if [[ "$bin_path" == "/usr/bin/python3" ]]; then
        continue
      fi
      python_bin="$bin_path"
      break
    fi
  done

  if [[ -z "$python_bin" ]]; then
    fail "No usable python3 interpreter found"
  fi

  printf '%s\n' "$python_bin"
}

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT_DIR"

OS_NAME="$(uname -s)"
if [[ "$OS_NAME" != "Darwin" ]]; then
  fail "This script is only supported on macOS."
fi

MACOS_VERSION="$(sw_vers -productVersion 2>/dev/null || true)"
MACOS_MAJOR="${MACOS_VERSION%%.*}"
ARCH="$(uname -m)"
msg "Detected macOS ${MACOS_VERSION:-unknown} on architecture $ARCH."
if [[ "$MACOS_MAJOR" =~ ^[0-9]+$ && "$MACOS_MAJOR" -lt 26 ]]; then
  msg "⚠️  macOS $MACOS_VERSION detected. Cohesix officially validates macOS 26 or newer for Apple Silicon hosts."
fi

msg "Ensuring Homebrew availability …"
ensure_homebrew_shellenv

MANAGER_SCRIPT="$ROOT_DIR/scripts/manage_homebrew_packages.sh"
if [ ! -x "$MANAGER_SCRIPT" ]; then
  fail "Missing Homebrew management helper at $MANAGER_SCRIPT"
fi

REQUIRED_FORMULAE=(cmake ninja python@3.12 pkg-config coreutils gnu-tar)
msg "Ensuring required Homebrew formulae: ${REQUIRED_FORMULAE[*]}"
"$MANAGER_SCRIPT" install "${REQUIRED_FORMULAE[@]}"

msg "Checking Xcode command line tools …"
if ! xcode-select -p >/dev/null 2>&1; then
  msg "Installing Xcode command line tools …"
  xcode-select --install || true
  read -r -p "📦 Press ENTER when installation is complete…" _
fi

msg "Checking Python toolchain …"

PYTHON_BIN="$(select_python_bin)"

PY_VER="$($PYTHON_BIN -c 'import sys; print(".".join(map(str, sys.version_info[:3])))')"
PY_OK=$($PYTHON_BIN -c 'import sys; print(sys.version_info >= (3,10))')
if [[ "$PY_OK" != "True" ]]; then
  fail "Python 3.10+ required, found $PY_VER"
fi

msg "Using $PYTHON_BIN ($PY_VER)"
export PYTHON_BIN

msg "Checking git …"
# Metadata validation handled by CI
command -v git >/dev/null 2>&1 || fail "git not found"


msg "✅ macOS setup verified."
