# CLASSIFICATION: COMMUNITY
# Filename: bootstrap_sel4_tools.sh v0.7
# Author: Lukas Bower
# Date Modified: 2026-02-27
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
SEL4="$ROOT/third_party/sel4"
TOOLS="$ROOT/third_party/sel4_tools"
SETTINGS="$TOOLS/cmake-tool/settings.cmake"

msg(){ printf "\e[32m==>\e[0m %s\n" "$*"; }
die(){ printf "\e[31m[ERR]\e[0m %s\n" "$*" >&2; exit 1; }

clone_repo(){
    local dir="$1" url="$2" branch="$3"
    if [ ! -d "$dir/.git" ]; then
        [ -d "$dir" ] && rm -rf "$dir"
        git clone --depth 1 --branch "$branch" "$url" "$dir"
        msg "Cloned $url at $branch"
    else
        local current
        current=$(git -C "$dir" rev-parse --abbrev-ref HEAD)
        if [ "$current" != "$branch" ]; then
            git -C "$dir" fetch origin "$branch"
            git -C "$dir" reset --hard "origin/$branch"
            msg "Reset $dir to origin/$branch"
        else
            msg "Using existing $dir on $current"
        fi
    fi
}

clone_repo "$SEL4" https://github.com/seL4/seL4.git seL4-12.1.0
clone_repo "$TOOLS" https://github.com/seL4/seL4_tools.git master

find "$TOOLS" -type f -name '*.sh' -exec chmod +x {} +

if [ ! -f "$SETTINGS" ]; then
    mkdir -p "$(dirname "$SETTINGS")"
    echo "# Generated" > "$SETTINGS"
fi
[ -w "$SETTINGS" ] || die "Cannot write to $SETTINGS"

pip_args=()
if [ -z "${VIRTUAL_ENV:-}" ]; then
    pip_args+=(--user)
fi
python3 -m pip install "${pip_args[@]}" jinja2 pyyaml >/dev/null
