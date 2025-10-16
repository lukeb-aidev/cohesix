#!/usr/bin/env bash
# Author: Lukas Bower

set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "usage: $0 <rootserver-elf> [repo-root]" >&2
    exit 64
fi

rootserver="$1"
repo_root="${2:-$(git rev-parse --show-toplevel)}"

if [[ ! -f "$rootserver" ]]; then
    echo "rootserver ELF not found: $rootserver" >&2
    exit 66
fi

if ! command -v nm >/dev/null 2>&1; then
    echo "nm tool is required for symbol inspection" >&2
    exit 69
fi

if ! nm "$rootserver" | grep -q ' sel4_start$'; then
    echo "sel4_start missing from rootserver image" >&2
    exit 1
fi

if nm "$rootserver" | grep -q ' main$'; then
    echo "main symbol must not be present in rootserver" >&2
    exit 1
fi

required_paths=(
    "apps/root-task/src/console/mod.rs"
    "apps/root-task/src/event/mod.rs"
    "apps/root-task/src/serial/mod.rs"
    "apps/root-task/src/ninedoor.rs"
    "apps/root-task/src/net/mod.rs"
)

for path in "${required_paths[@]}"; do
    if [[ ! -f "$repo_root/$path" ]]; then
        echo "required module missing: $path" >&2
        exit 1
    fi
done

echo "root-task guard checks passed"
