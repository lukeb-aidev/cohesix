// CLASSIFICATION: COMMUNITY
// Filename: fetch_sel4.sh v0.2
// Author: Lukas Bower
// Date Modified: 2026-01-08

#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

# Validate host architecture and toolchain
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64|aarch64|arm64)
        ;;
    *)
        echo "Unsupported host architecture: $ARCH" >&2
        exit 1
        ;;
esac

if [ "$ARCH" = "aarch64" ]; then
    if ! command -v gcc >/dev/null 2>&1 && ! command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
        echo "No aarch64 gcc toolchain available" >&2
        exit 1
    fi
else
    command -v gcc >/dev/null 2>&1 || { echo "gcc not found" >&2; exit 1; }
fi

update_submodule() {
    local path="$1"
    local url="$2"
    if [ ! -d "$path/.git" ]; then
        git submodule add "$url" "$path" || true
    fi
    git submodule update --init --recursive "$path"
}

update_submodule "third_party/sel4" "https://github.com/seL4/seL4.git"
update_submodule "third_party/sel4_tools" "https://github.com/seL4/seL4_tools.git"

echo "seL4 repositories are up to date."
