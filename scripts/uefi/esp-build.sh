#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build a deterministic UEFI ESP tree containing Cohesix boot artifacts.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"

MANIFEST_JSON=""
OUT_DIR="${ROOT_DIR}/out/uefi"
SEL4_BUILD_DIR="${ROOT_DIR}/seL4/build_UEFI"
SEL4_BUILD_DIR_REQUESTED="${SEL4_BUILD_DIR}"
ELFLOADER_EFI=""
KERNEL_ELF=""
ROOTSERVER_ELF="${ROOT_DIR}/out/cohesix/staging/rootserver"
INITRD_CPIO=""
DTB_DIR=""
FORCE=0
ALLOW_NON_UEFI_PROFILE=0
ELFLOADER_OVERRIDDEN=0
KERNEL_OVERRIDDEN=0
SYNC_EMBEDDED_ROOTSERVER=1
REBUILD_ELFLOADER=1

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
  --sel4-build-dir <dir>     seL4 UEFI build directory (default: seL4/build_UEFI)
  --elfloader <file>         EFI elfloader binary (default: <sel4-build-dir>/elfloader.efi)
  --kernel <file>            seL4 kernel ELF (default: <sel4-build-dir>/kernel/kernel.elf)
  --rootserver <file>        Root-task ELF (default: out/cohesix/staging/rootserver)
  --initrd <file>            Optional initrd CPIO to include as cohesix/initrd.cpio
  --dtb-dir <dir>            Optional DTB directory copied to esp/dtb/
  --no-sync-embedded-rootserver
                             Skip syncing Cohesix rootserver into <sel4-build-dir>/elfloader/rootserver
  --no-rebuild-elfloader     Skip rebuilding elfloader.efi after rootserver sync
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
        --sel4-build-dir)
            SEL4_BUILD_DIR="$2"
            SEL4_BUILD_DIR_REQUESTED="$2"
            shift 2
            ;;
        --elfloader)
            ELFLOADER_EFI="$2"
            ELFLOADER_OVERRIDDEN=1
            shift 2
            ;;
        --kernel)
            KERNEL_ELF="$2"
            KERNEL_OVERRIDDEN=1
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
        --no-sync-embedded-rootserver)
            SYNC_EMBEDDED_ROOTSERVER=0
            shift
            ;;
        --no-rebuild-elfloader)
            REBUILD_ELFLOADER=0
            shift
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

if [[ "$ELFLOADER_OVERRIDDEN" -ne 1 ]]; then
    ELFLOADER_EFI="${SEL4_BUILD_DIR}/elfloader.efi"
fi
if [[ "$KERNEL_OVERRIDDEN" -ne 1 ]]; then
    KERNEL_ELF="${SEL4_BUILD_DIR}/kernel/kernel.elf"
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

resolve_sel4_build_dir() {
    local cache_file="${SEL4_BUILD_DIR}/CMakeCache.txt"
    local cache_dir

    if [[ ! -f "$cache_file" ]]; then
        return 0
    fi

    cache_dir="$(awk -F= '/^CMAKE_CACHEFILE_DIR:INTERNAL=/{print $2; exit}' "$cache_file")"
    if [[ -z "$cache_dir" || "$cache_dir" == "$SEL4_BUILD_DIR" ]]; then
        return 0
    fi
    if [[ ! -d "$cache_dir" ]]; then
        return 0
    fi

    log "using canonical seL4 build directory from CMakeCache: ${cache_dir}"
    SEL4_BUILD_DIR="$cache_dir"
    if [[ "$ELFLOADER_OVERRIDDEN" -ne 1 ]]; then
        ELFLOADER_EFI="${SEL4_BUILD_DIR}/elfloader.efi"
    fi
    if [[ "$KERNEL_OVERRIDDEN" -ne 1 ]]; then
        KERNEL_ELF="${SEL4_BUILD_DIR}/kernel/kernel.elf"
    fi
}

detect_gic_version() {
    local detector="${ROOT_DIR}/scripts/lib/detect_gic_version.py"
    local kernel_cfg_guess
    local kernel_dir
    local config
    local version

    if [[ ! -f "$detector" ]]; then
        return 0
    fi

    kernel_dir="$(dirname "$KERNEL_ELF")"
    if [[ -d "$kernel_dir" ]]; then
        kernel_cfg_guess="$(cd "$kernel_dir" && pwd)/gen_config/kernel/gen_config.h"
    else
        kernel_cfg_guess=""
    fi
    local -a candidates=(
        "$kernel_cfg_guess"
        "${SEL4_BUILD_DIR}/kernel/gen_config/kernel/gen_config.h"
        "${ROOT_DIR}/seL4/build_UEFI/kernel/gen_config/kernel/gen_config.h"
        "${ROOT_DIR}/seL4/SMP_build/kernel/gen_config/kernel/gen_config.h"
        "${ROOT_DIR}/seL4/build/kernel/gen_config/kernel/gen_config.h"
    )

    for config in "${candidates[@]}"; do
        if [[ -f "$config" ]]; then
            version="$(python3 "$detector" "$config" 2>/dev/null || true)"
            if [[ "$version" == "2" || "$version" == "3" ]]; then
                printf "%s\n" "$version"
                return 0
            fi
        fi
    done
    return 0
}

embedded_rootserver_size() {
    local image="$1"
    python3 - "$image" <<'PY'
import pathlib
import sys

image_path = pathlib.Path(sys.argv[1])
blob = image_path.read_bytes()
magic = b"070701"

def parse_entry(offset: int):
    if offset + 110 > len(blob) or blob[offset : offset + 6] != magic:
        return None
    try:
        fields = [int(blob[offset + 6 + i : offset + 14 + i], 16) for i in range(0, 13 * 8, 8)]
    except ValueError:
        return None

    filesize = fields[6]
    namesize = fields[11]
    if namesize <= 0:
        return None

    name_start = offset + 110
    name_end = name_start + namesize
    if name_end > len(blob):
        return None

    raw_name = blob[name_start:name_end].rstrip(b"\x00")
    try:
        name = raw_name.decode("utf-8")
    except UnicodeDecodeError:
        return None

    data_start = (name_end + 3) & ~3
    data_end = data_start + filesize
    if data_end > len(blob):
        return None

    next_offset = (data_end + 3) & ~3
    return name, filesize, next_offset

for start in range(0, len(blob) - 110):
    if blob[start : start + 6] != magic:
        continue
    offset = start
    rootserver_size = None
    seen = 0
    while True:
        parsed = parse_entry(offset)
        if parsed is None:
            break
        name, filesize, offset = parsed
        seen += 1
        if name == "rootserver":
            rootserver_size = filesize
        if name == "TRAILER!!!":
            if rootserver_size is not None:
                print(rootserver_size)
                raise SystemExit(0)
            break
        if seen > 10000:
            break

raise SystemExit(1)
PY
}

sync_embedded_rootserver() {
    local embedded_rootserver="${SEL4_BUILD_DIR}/elfloader/rootserver"
    local generated_image=""

    if [[ "$SYNC_EMBEDDED_ROOTSERVER" -ne 1 ]]; then
        log "skipping embedded rootserver sync (--no-sync-embedded-rootserver)"
        return 0
    fi

    [[ -d "$SEL4_BUILD_DIR" ]] || fail "seL4 build directory not found: ${SEL4_BUILD_DIR}"
    require_file "$ROOTSERVER_ELF"
    require_file "$embedded_rootserver"

    if cmp -s "$ROOTSERVER_ELF" "$embedded_rootserver"; then
        log "embedded rootserver source already matches ${ROOTSERVER_ELF}"
    else
        copy_file "$ROOTSERVER_ELF" "$embedded_rootserver"
        log "synced Cohesix rootserver to ${embedded_rootserver}"
    fi

    if [[ "$REBUILD_ELFLOADER" -ne 1 ]]; then
        log "skipping elfloader rebuild (--no-rebuild-elfloader)"
        return 0
    fi

    command -v cmake >/dev/null 2>&1 || fail "cmake is required to rebuild elfloader.efi"
    log "rebuilding elfloader.efi in ${SEL4_BUILD_DIR}"
    if ! cmake --build "$SEL4_BUILD_DIR" --target rootserver_image >>"${LOG_FILE}" 2>&1; then
        fail "elfloader.efi rebuild failed (see ${LOG_FILE})"
    fi

    generated_image="$(find "${SEL4_BUILD_DIR}/images" -maxdepth 1 -type f -name "*-image-*" | sort | head -n 1 || true)"
    if [[ -n "$generated_image" ]]; then
        copy_file "$generated_image" "$ELFLOADER_EFI"
        log "updated ${ELFLOADER_EFI} from ${generated_image}"
    fi
}

require_file "$MANIFEST_JSON"
require_file "$ROOTSERVER_ELF"
if [[ -n "$INITRD_CPIO" ]]; then
    require_file "$INITRD_CPIO"
fi
if [[ -n "$DTB_DIR" && ! -d "$DTB_DIR" ]]; then
    fail "dtb directory does not exist: ${DTB_DIR}"
fi

resolve_sel4_build_dir
sync_embedded_rootserver
require_file "$ELFLOADER_EFI"
require_file "$KERNEL_ELF"

embedded_size="$(embedded_rootserver_size "$ELFLOADER_EFI" || true)"
expected_size="$(wc -c < "$ROOTSERVER_ELF" | tr -d '[:space:]')"
if [[ -z "$embedded_size" ]]; then
    fail "could not locate embedded rootserver entry inside ${ELFLOADER_EFI}"
fi
if [[ "$embedded_size" != "$expected_size" ]]; then
    fail "embedded rootserver size mismatch in ${ELFLOADER_EFI} (expected ${expected_size}, found ${embedded_size})"
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

gic_version="$(detect_gic_version)"
if [[ "$gic_version" == "2" || "$gic_version" == "3" ]]; then
    printf "%s\n" "$gic_version" > "${ESP_DIR}/cohesix/gic-version.txt"
    log "detected kernel GIC version: ${gic_version}"
fi

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
