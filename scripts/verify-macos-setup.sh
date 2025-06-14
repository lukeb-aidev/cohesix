# CLASSIFICATION: COMMUNITY
#!/usr/bin/env bash
# Filename: verify-macos-setup.sh v0.1
# Author: Lukas Bower
# Date Modified: 2025-06-05
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

ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT_DIR"

msg "Checking Homebrew …"
if ! command -v brew >/dev/null 2>&1; then
  msg "Installing Homebrew …"
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  eval "$(/opt/homebrew/bin/brew shellenv)" || true
fi

msg "Checking Xcode command line tools …"
if ! xcode-select -p >/dev/null 2>&1; then
  msg "Installing Xcode command line tools …"
  xcode-select --install || true
  read -p "📦 Press ENTER when installation is complete…" _
fi

msg "Checking Python version …"

# Try to install python@3.12 if not found
if ! command -v python3.12 >/dev/null 2>&1; then
  msg "Installing Python 3.12 via Homebrew …"
  brew install python@3.12 || fail "Homebrew Python install failed"
fi

PYTHON_BIN=""
for CANDIDATE in python3.12 python3.11 python3.10 python3; do
  if command -v "$CANDIDATE" >/dev/null 2>&1; then
    BIN_PATH="$(command -v "$CANDIDATE")"
    if [[ "$BIN_PATH" == "/usr/bin/python3" ]]; then
      continue  # Skip system python3 (3.9.x on macOS)
    fi
    PYTHON_BIN="$BIN_PATH"
    break
  fi
done

if [[ -z "$PYTHON_BIN" ]]; then
  fail "No usable python3 interpreter found"
fi

PY_VER="$($PYTHON_BIN -c 'import sys; print(".".join(map(str, sys.version_info[:3])))')"
PY_OK=$($PYTHON_BIN -c 'import sys; print(sys.version_info >= (3,10))')
if [[ "$PY_OK" != "True" ]]; then
  fail "Python 3.10+ required, found $PY_VER"
fi

msg "Using $PYTHON_BIN ($PY_VER)"
export PYTHON_BIN

msg "Checking git …"
command -v git >/dev/null 2>&1 || fail "git not found"

msg "Running metadata sync validation …"
"$PYTHON_BIN" scripts/validate_metadata_sync.py
# Future scripts can now access $PYTHON_BIN to ensure consistent interpreter usage

msg "✅ macOS setup verified."
