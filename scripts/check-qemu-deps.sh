# CLASSIFICATION: COMMUNITY
# Filename: check-qemu-deps.sh v0.2
# Author: Lukas Bower
# Date Modified: 2029-02-14
#!/usr/bin/env bash
set -euo pipefail

missing=0
os_name="$(uname -s)"

homebrew_roots=()
if [ "$os_name" = "Darwin" ]; then
    if [ -n "${HOMEBREW_PREFIX:-}" ]; then
        homebrew_roots+=("$HOMEBREW_PREFIX")
    fi
    if command -v brew >/dev/null 2>&1; then
        brew_prefix="$(brew --prefix 2>/dev/null || true)"
        if [ -n "$brew_prefix" ]; then
            homebrew_roots+=("$brew_prefix")
        fi
        brew_qemu_prefix="$(brew --prefix qemu 2>/dev/null || true)"
        if [ -n "$brew_qemu_prefix" ]; then
            homebrew_roots+=("$brew_qemu_prefix")
        fi
    fi
    homebrew_roots+=("/opt/homebrew" "/usr/local")
fi

if ! command -v qemu-system-x86_64 >/dev/null 2>&1; then
    echo "ERROR: qemu-system-x86_64 not found." >&2
    if [ "$os_name" = "Darwin" ]; then
        echo "Install QEMU via Homebrew (e.g., 'brew install qemu')." >&2
    else
        echo "Install QEMU via your package manager (e.g., 'sudo apt install qemu-system-x86')." >&2
    fi
    missing=1
fi

firmware=""
declare -a firmware_candidates=(
    /usr/share/OVMF/OVMF.fd
    /usr/share/qemu/OVMF.fd
    /usr/share/edk2/ovmf/OVMF_CODE.fd
    /usr/share/OVMF/OVMF_CODE.fd
    /usr/share/OVMF/OVMF.fd
    /usr/share/AAVMF/AAVMF_CODE.fd
)

for root in "${homebrew_roots[@]}"; do
    firmware_candidates+=(
        "$root/share/OVMF/OVMF.fd"
        "$root/share/OVMF/OVMF_CODE.fd"
        "$root/share/qemu/OVMF.fd"
        "$root/share/qemu/OVMF_CODE.fd"
        "$root/share/qemu/edk2-x86_64-code.fd"
        "$root/share/qemu/edk2-ovmf/OVMF_CODE.fd"
        "$root/share/edk2-ovmf/OVMF_CODE.fd"
    )
done

for f in "${firmware_candidates[@]}"; do
    if [ -f "$f" ]; then
        firmware="$f"
        break
    fi
done

if [ -z "$firmware" ]; then
    echo "ERROR: UEFI firmware OVMF.fd not found." >&2
    if [ "$os_name" = "Darwin" ]; then
        echo "Install QEMU via Homebrew (e.g., 'brew install qemu') to provide OVMF firmware." >&2
    else
        echo "Install the 'ovmf' or 'edk2-ovmf' package for your distribution." >&2
    fi
    missing=1
fi

gnu_efi_header=""
declare -a gnu_efi_candidates=(
    /usr/include/efi/efi.h
    /usr/include/efi.h
    /usr/local/include/efi/efi.h
    /usr/local/include/efi.h
)

if [ -n "${GNU_EFI_PREFIX:-}" ]; then
    gnu_efi_candidates+=("$GNU_EFI_PREFIX/include/efi/efi.h" "$GNU_EFI_PREFIX/include/efi.h")
fi

for root in "${homebrew_roots[@]}"; do
    gnu_efi_candidates+=(
        "$root/include/efi/efi.h"
        "$root/include/efi.h"
        "$root/opt/gnu-efi/include/efi/efi.h"
        "$root/opt/gnu-efi/include/efi.h"
        "$root/share/gnu-efi/inc/efi/efi.h"
    )
done

if command -v brew >/dev/null 2>&1; then
    brew_gnu_efi_prefix="$(brew --prefix gnu-efi 2>/dev/null || true)"
    if [ -n "$brew_gnu_efi_prefix" ]; then
        gnu_efi_candidates+=(
            "$brew_gnu_efi_prefix/include/efi/efi.h"
            "$brew_gnu_efi_prefix/include/efi.h"
        )
    fi
fi

for header in "${gnu_efi_candidates[@]}"; do
    if [ -f "$header" ]; then
        gnu_efi_header="$header"
        break
    fi
done

if [ -z "$gnu_efi_header" ]; then
    echo "ERROR: gnu-efi headers not found." >&2
    if [ "$os_name" = "Darwin" ]; then
        echo "Install gnu-efi headers via Homebrew if available (e.g., 'brew install gnu-efi') or set GNU_EFI_PREFIX to their location." >&2
    else
        echo "Install gnu-efi development headers (e.g., 'sudo apt install gnu-efi')." >&2
    fi
    missing=1
fi

if [ "$missing" -eq 1 ]; then
    exit 1
else
    echo "QEMU and EFI dependencies are present." >&2
    exit 0
fi
