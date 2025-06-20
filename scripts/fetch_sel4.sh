// CLASSIFICATION: COMMUNITY
// Filename: fetch_sel4.sh v0.1
// Author: Lukas Bower
// Date Modified: 2025-12-25

#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

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
