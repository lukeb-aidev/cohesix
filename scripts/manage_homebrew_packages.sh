#!/usr/bin/env bash
# CLASSIFICATION: COMMUNITY
# Filename: manage_homebrew_packages.sh v0.1
# Author: Lukas Bower
# Date Modified: 2030-03-22
set -euo pipefail

STATE_DIR="${HOME}/.cohesix"
STATE_FILE="$STATE_DIR/homebrew_managed_packages.txt"
TMP_ROOT="${TMPDIR:-$STATE_DIR/tmp}"

msg() { printf "\e[32m[brew]\e[0m %s\n" "$*"; }
err() { printf "\e[31m[brew]\e[0m %s\n" "$*" >&2; exit 1; }

usage() {
    cat <<'USAGE'
Usage: manage_homebrew_packages.sh <command> [packages...]

Commands:
  install <pkg...>         Install Homebrew formulae and record them for Cohesix use.
  uninstall <pkg...>       Uninstall recorded formulae and remove them from the record.
  list-recorded            Show the set of formulae Cohesix has installed via this tool.
  prune-recorded           Uninstall every recorded formula.
USAGE
    exit 1
}

ensure_state() {
    mkdir -p "$STATE_DIR"
    mkdir -p "$TMP_ROOT"
    if [ ! -f "$STATE_FILE" ]; then
        : > "$STATE_FILE"
    fi
}

require_brew() {
    if ! command -v brew >/dev/null 2>&1; then
        err "Homebrew is not available on PATH. Run scripts/setup_build_env.sh first."
    fi
}

package_recorded() {
    local pkg="$1"
    if [ ! -f "$STATE_FILE" ]; then
        return 1
    fi
    grep -Fxq "$pkg" "$STATE_FILE"
}

record_package() {
    local pkg="$1"
    package_recorded "$pkg" && return
    ensure_state
    printf '%s\n' "$pkg" >> "$STATE_FILE"
}

remove_record() {
    local pkg="$1"
    [ -f "$STATE_FILE" ] || return
    ensure_state
    local tmp_file
    tmp_file="$TMP_ROOT/homebrew.$(date +%s).$$.$RANDOM"
    : > "$tmp_file"
    if [ -s "$STATE_FILE" ]; then
        grep -Fxv "$pkg" "$STATE_FILE" > "$tmp_file" || true
    fi
    mv "$tmp_file" "$STATE_FILE"
}

install_pkg() {
    local pkg="$1"
    if brew list --formula "$pkg" >/dev/null 2>&1; then
        msg "Formula $pkg already installed."
        record_package "$pkg"
        return
    fi
    msg "Installing $pkg via Homebrew …"
    brew install "$pkg"
    record_package "$pkg"
}

uninstall_pkg() {
    local pkg="$1"
    if ! package_recorded "$pkg"; then
        msg "Skipping $pkg – not recorded as Cohesix-managed."
        return
    fi
    if ! brew list --formula "$pkg" >/dev/null 2>&1; then
        msg "Formula $pkg not currently installed. Updating records."
        remove_record "$pkg"
        return
    fi
    msg "Uninstalling $pkg via Homebrew …"
    brew uninstall "$pkg"
    remove_record "$pkg"
}

list_recorded() {
    if [ ! -f "$STATE_FILE" ] || ! [ -s "$STATE_FILE" ]; then
        msg "No Cohesix-managed Homebrew formulae recorded."
        return
    fi
    msg "Cohesix-managed Homebrew formulae:"
    cat "$STATE_FILE"
}

prune_recorded() {
    if [ ! -f "$STATE_FILE" ] || ! [ -s "$STATE_FILE" ]; then
        msg "Nothing to prune."
        return
    fi
    while IFS= read -r pkg; do
        [ -n "$pkg" ] || continue
        uninstall_pkg "$pkg"
    done < "$STATE_FILE"
}

main() {
    ensure_state
    require_brew

    if [ $# -lt 1 ]; then
        usage
    fi

    local cmd="$1"
    shift || true

    case "$cmd" in
        install)
            [ $# -ge 1 ] || usage
            for pkg in "$@"; do
                install_pkg "$pkg"
            done
            ;;
        uninstall)
            [ $# -ge 1 ] || usage
            for pkg in "$@"; do
                uninstall_pkg "$pkg"
            done
            ;;
        list-recorded)
            list_recorded
            ;;
        prune-recorded)
            prune_recorded
            ;;
        *)
            usage
            ;;
    esac
}

main "$@"
