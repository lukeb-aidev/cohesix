// CLASSIFICATION: COMMUNITY
// Filename: bootstrap_sel4_tools.sh v0.3
// Author: Lukas Bower
// Date Modified: 2026-02-12
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
SEL4="$ROOT/third_party/sel4"
TOOLS="$ROOT/third_party/sel4_tools"
SETTINGS="$TOOLS/cmake-tool/settings.cmake"

msg(){ printf "\e[32m==>\e[0m %s\n" "$*"; }
die(){ printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

clone_or_update(){
    local dir="$1" url="$2"
    if [ ! -d "$dir/.git" ]; then
        git clone "$url" "$dir"
    else
        git -C "$dir" fetch --all
        git -C "$dir" pull --ff-only
    fi
}

clone_or_update "$SEL4" https://github.com/seL4/seL4.git
clone_or_update "$TOOLS" https://github.com/seL4/seL4_tools.git

find "$TOOLS" -type f -name '*.sh' -exec chmod +x {} +

if [ ! -f "$SETTINGS" ]; then
    mkdir -p "$(dirname "$SETTINGS")"
    echo "# Generated" > "$SETTINGS"
fi
[ -w "$SETTINGS" ] || die "Cannot write to $SETTINGS"
