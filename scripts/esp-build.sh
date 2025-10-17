#!/usr/bin/env bash
# Author: Lukas Bower
set -euo pipefail

if ! command -v hdiutil >/dev/null 2>&1; then
    echo "hdiutil is required to attach the ESP image (macOS only)." >&2
    exit 1
fi

if ! command -v newfs_msdos >/dev/null 2>&1; then
    echo "newfs_msdos not found; install macOS command line tools." >&2
    exit 1
fi

ELFLOADER_EFI="${ELFLOADER_EFI:-out/cohesix/staging/elfloader.efi}"
KERNEL_ELF="${KERNEL_ELF:-out/cohesix/staging/kernel.elf}"
ROOTSERVER="${ROOTSERVER:-out/cohesix/staging/rootserver}"
INITRD="${INITRD:-out/cohesix/cohesix-system.cpio}"
ESP_IMG="${ESP_IMG:-out/cohesix/esp.img}"
ESP_MB="${ESP_MB:-64}"

mkdir -p "$(dirname "${ESP_IMG}")"
rm -f "${ESP_IMG}"

create_raw_image() {
    local size_mb="$1"
    local path="$2"
    if command -v mkfile >/dev/null 2>&1; then
        mkfile "${size_mb}m" "${path}"
    else
        dd if=/dev/zero of="${path}" bs=1m count="${size_mb}" status=none
    fi
}

create_raw_image "${ESP_MB}" "${ESP_IMG}"
newfs_msdos -F 32 -v COHESIXESP "${ESP_IMG}"

MNT="$(mktemp -d)"
trap 'hdiutil detach "${MNT}" -quiet || true; rmdir "${MNT}" || true' EXIT

hdiutil attach "${ESP_IMG}" -imagekey diskimage-class=CRawDiskImage -mountpoint "${MNT}" -quiet

mkdir -p "${MNT}/EFI/BOOT" "${MNT}/cohesix"

cp "${ELFLOADER_EFI}" "${MNT}/EFI/BOOT/BOOTAA64.EFI"
cp "${KERNEL_ELF}" "${MNT}/cohesix/kernel.elf"
cp "${ROOTSERVER}" "${MNT}/cohesix/rootserver"
if [ -f "${INITRD}" ]; then
    cp "${INITRD}" "${MNT}/cohesix/initrd.cpio"
fi

cat > "${MNT}/startup.nsh" <<'NSH'
\EFI\BOOT\BOOTAA64.EFI
NSH

sync
hdiutil detach "${MNT}" -quiet
rmdir "${MNT}"

echo "[esp-build] Created RAW ESP at ${ESP_IMG}"
