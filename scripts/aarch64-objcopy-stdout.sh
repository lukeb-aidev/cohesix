#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Wrap host AArch64 objcopy so seL4's uImage helper can safely target /dev/stdout on macOS.
# Copyright 2026 Lukas Bower

set -euo pipefail

find_real_objcopy() {
    local candidate=""
    for candidate in \
        /opt/homebrew/bin/aarch64-elf-objcopy \
        /opt/homebrew/bin/aarch64-linux-gnu-objcopy \
        "$(command -v aarch64-elf-objcopy 2>/dev/null || true)" \
        "$(command -v aarch64-linux-gnu-objcopy 2>/dev/null || true)" \
        "$(command -v objcopy 2>/dev/null || true)"; do
        [[ -n "${candidate}" && -x "${candidate}" ]] || continue
        if [[ "$(realpath "${candidate}")" != "$(realpath "$0")" ]]; then
            printf '%s\n' "${candidate}"
            return 0
        fi
    done
    return 1
}

REAL_OBJCOPY="$(find_real_objcopy || true)"
if [[ -z "${REAL_OBJCOPY}" ]]; then
    echo "aarch64-objcopy-stdout.sh: no usable objcopy found" >&2
    exit 1
fi

if [[ "$#" -gt 0 && "${!#}" == "/dev/stdout" ]]; then
    temp_out="$(mktemp "${TMPDIR:-/tmp}/coh-objcopy.XXXXXX.bin")"
    trap 'rm -f "${temp_out}"' EXIT
    args=("$@")
    args[$(($# - 1))]="${temp_out}"
    "${REAL_OBJCOPY}" "${args[@]}"
    cat "${temp_out}"
else
    "${REAL_OBJCOPY}" "$@"
fi
