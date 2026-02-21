#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build a deterministic UEFI ESP tree containing Cohesix boot artifacts.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"

MANIFEST_JSON=""
OUT_DIR="${ROOT_DIR}/out/uefi"
ELFLOADER_EFI="${ROOT_DIR}/seL4/build/elfloader.efi"
KERNEL_ELF="${ROOT_DIR}/seL4/SMP_build/kernel.elf"
ROOTSERVER_ELF="${ROOT_DIR}/out/cohesix/staging/rootserver"
INITRD_CPIO=""
DTB_DIR=""
FORCE=0
ALLOW_NON_UEFI_PROFILE=0

usage() {
    cat <<'USAGE'
Usage: scripts/uefi/esp-build.sh --manifest <resolved-manifest.json> [options]

Builds a deterministic ESP tree at out/uefi/esp with:
  EFI/BOOT/BOOTAA64.EFI
  cohesix/kernel.elf
  cohesix/rootserver
  cohesix/manifest.json
  cohesix/manifest.sha256
  cohesix/initrd.cpio (optional)
  dtb/* (optional)

Options:
  --manifest <file>          Resolved manifest JSON (required)
  --out-dir <dir>            Output directory root (default: out/uefi)
  --elfloader <file>         EFI elfloader binary (default: seL4/build/elfloader.efi)
  --kernel <file>            seL4 kernel ELF (default: seL4/SMP_build/kernel.elf)
  --rootserver <file>        Root-task ELF (default: out/cohesix/staging/rootserver)
  --initrd <file>            Optional initrd CPIO to include as cohesix/initrd.cpio
  --dtb-dir <dir>            Optional DTB directory copied to esp/dtb/
  --allow-non-uefi-profile   Do not fail when manifest.profile.name != uefi-aarch64
  --force                    Remove existing output directory before building
  -h, --help                 Show this help
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --manifest)
            MANIFEST_JSON="$2"
            shift 2
            ;;
        --out-dir)
            OUT_DIR="$2"
            shift 2
            ;;
        --elfloader)
            ELFLOADER_EFI="$2"
            shift 2
            ;;
        --kernel)
            KERNEL_ELF="$2"
            shift 2
            ;;
        --rootserver)
            ROOTSERVER_ELF="$2"
            shift 2
            ;;
        --initrd)
            INITRD_CPIO="$2"
            shift 2
            ;;
        --dtb-dir)
            DTB_DIR="$2"
            shift 2
            ;;
        --allow-non-uefi-profile)
            ALLOW_NON_UEFI_PROFILE=1
            shift
            ;;
        --force)
            FORCE=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "[uefi-esp] unknown argument: $1" >&2
            usage
            exit 1
            ;;
    esac
done

if [[ -z "$MANIFEST_JSON" ]]; then
    echo "[uefi-esp] --manifest is required" >&2
    usage
    exit 1
fi

ESP_DIR="${OUT_DIR}/esp"
LOG_FILE="${OUT_DIR}/esp-build.log"
META_FILE="${OUT_DIR}/esp-meta.json"
HASH_LIST_FILE="${OUT_DIR}/esp.sha256"
TAR_FILE="${OUT_DIR}/esp.tar"

mkdir -p "${OUT_DIR}"
: > "${LOG_FILE}"

log() {
    echo "[uefi-esp] $*" | tee -a "${LOG_FILE}"
}

fail() {
    log "error: $*"
    exit 1
}

require_file() {
    local path="$1"
    [[ -f "$path" ]] || fail "required file missing: ${path}"
}

copy_file() {
    local src="$1"
    local dst="$2"
    mkdir -p "$(dirname "$dst")"
    cp -f "$src" "$dst"
}

require_file "$MANIFEST_JSON"
require_file "$ELFLOADER_EFI"
require_file "$KERNEL_ELF"
require_file "$ROOTSERVER_ELF"
if [[ -n "$INITRD_CPIO" ]]; then
    require_file "$INITRD_CPIO"
fi
if [[ -n "$DTB_DIR" && ! -d "$DTB_DIR" ]]; then
    fail "dtb directory does not exist: ${DTB_DIR}"
fi

if [[ "$FORCE" -eq 1 ]]; then
    rm -rf "${ESP_DIR}"
fi
mkdir -p "${ESP_DIR}"

profile_name="$(python3 - "$MANIFEST_JSON" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
data = json.loads(path.read_text(encoding="utf-8"))
print(data.get("profile", {}).get("name", ""))
PY
)"

if [[ "$ALLOW_NON_UEFI_PROFILE" -ne 1 && "$profile_name" != "uefi-aarch64" ]]; then
    fail "manifest.profile.name must be 'uefi-aarch64' (found '${profile_name}')"
fi

log "building deterministic ESP tree at ${ESP_DIR}"
log "manifest profile=${profile_name}"

copy_file "$ELFLOADER_EFI" "${ESP_DIR}/EFI/BOOT/BOOTAA64.EFI"
copy_file "$KERNEL_ELF" "${ESP_DIR}/cohesix/kernel.elf"
copy_file "$ROOTSERVER_ELF" "${ESP_DIR}/cohesix/rootserver"
copy_file "$MANIFEST_JSON" "${ESP_DIR}/cohesix/manifest.json"

if [[ -n "$INITRD_CPIO" ]]; then
    copy_file "$INITRD_CPIO" "${ESP_DIR}/cohesix/initrd.cpio"
fi

if [[ -n "$DTB_DIR" ]]; then
    mkdir -p "${ESP_DIR}/dtb"
    while IFS= read -r -d '' dtb; do
        copy_file "$dtb" "${ESP_DIR}/dtb/$(basename "$dtb")"
    done < <(find "$DTB_DIR" -maxdepth 1 -type f -print0 | sort -z)
fi

python3 - "${ESP_DIR}/cohesix/manifest.json" "${ESP_DIR}/cohesix/manifest.sha256" <<'PY'
import hashlib
import pathlib
import sys

manifest = pathlib.Path(sys.argv[1])
digest_out = pathlib.Path(sys.argv[2])
digest = hashlib.sha256(manifest.read_bytes()).hexdigest()
digest_out.write_text(f"{digest}  manifest.json\n", encoding="utf-8")
PY

python3 - "${ESP_DIR}" "${HASH_LIST_FILE}" "${META_FILE}" "${TAR_FILE}" <<'PY'
import hashlib
import json
import pathlib
import tarfile
import sys

esp = pathlib.Path(sys.argv[1])
hash_list = pathlib.Path(sys.argv[2])
meta_out = pathlib.Path(sys.argv[3])
tar_out = pathlib.Path(sys.argv[4])

records = []
for path in sorted(p for p in esp.rglob("*") if p.is_file()):
    rel = path.relative_to(esp).as_posix()
    data = path.read_bytes()
    records.append({
        "path": rel,
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    })

hash_list.write_text(
    "".join(f"{entry['sha256']}  {entry['path']}\n" for entry in records),
    encoding="utf-8",
)

meta = {
    "file_count": len(records),
    "files": records,
}
meta_out.write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")

with tarfile.open(tar_out, "w") as tf:
    for entry in records:
        full = esp / entry["path"]
        info = tf.gettarinfo(str(full), arcname=entry["path"])
        info.uid = 0
        info.gid = 0
        info.uname = "root"
        info.gname = "root"
        info.mtime = 0
        with full.open("rb") as fh:
            tf.addfile(info, fh)
PY

log "wrote ${ESP_DIR}"
log "wrote ${HASH_LIST_FILE}"
log "wrote ${META_FILE}"
log "wrote ${TAR_FILE}"
log "done"
