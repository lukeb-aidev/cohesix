// CLASSIFICATION: COMMUNITY
// Filename: verify-macos-setup.sh v0.1
// Date Modified: 2025-06-16
// Author: Lukas Bower

#!/usr/bin/env bash
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
command -v brew >/dev/null 2>&1 || fail "Homebrew not found; install from https://brew.sh/"

msg "Checking Xcode command line tools …"
if ! xcode-select -p >/dev/null 2>&1; then
  fail "Xcode command line tools missing; run 'xcode-select --install'"
fi

msg "Checking Python version …"
if ! command -v python3 >/dev/null 2>&1; then
  fail "python3 not found"
fi
PY_VER="$(python3 -V | awk '{print $2}')"
IFS='.' read -r PY_MAJOR PY_MINOR _ <<< "$PY_VER"
if (( PY_MAJOR < 3 || (PY_MAJOR == 3 && PY_MINOR < 10) )); then
  fail "Python 3.10+ required, found $PY_VER"
fi

msg "Checking git …"
command -v git >/dev/null 2>&1 || fail "git not found"

msg "Running metadata sync validation …"
python3 scripts/validate_metadata_sync.py

msg "✅ macOS setup verified."
