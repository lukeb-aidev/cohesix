#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build a compact portable Raspberry Pi 4 MBR/FAT32 release image from an exact staged payload.
# Copyright 2026 Lukas Bower

set -euo pipefail

STAGE_DIR=""
OUTPUT_IMAGE=""
OUTPUT_METADATA=""
OUTPUT_SHA256=""
FORCE=0
VOLUME_LABEL="COHESIX"
SECTOR_SIZE=512
PARTITION_START_LBA=2048
ATTACHED_DEVICE=""

usage() {
  cat <<'USAGE'
Usage: scripts/pi4_release_image.sh [options]

Required options:
  --stage-dir <path>       Exact Pi 4 FAT payload staging directory
  --output-image <path>    Portable raw MBR/FAT32 disk image output
  --output-metadata <path> Image layout and embedded-file provenance JSON
  --output-sha256 <path>   SHA-256 sidecar output

Optional:
  --force                  Replace existing selected output files

The disk size is derived from the staged payload with bounded FAT32 headroom.
It does not inherit the capacity of any physical SD card. The resulting raw
image can be written to any SD card whose byte capacity is at least the
minimum_target_bytes value in the generated metadata.
USAGE
}

fail() {
  printf '[pi4-release-image] error: %s\n' "$*" >&2
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --stage-dir)
      [[ $# -ge 2 ]] || fail "--stage-dir requires a path"
      STAGE_DIR="$2"
      shift 2
      ;;
    --output-image)
      [[ $# -ge 2 ]] || fail "--output-image requires a path"
      OUTPUT_IMAGE="$2"
      shift 2
      ;;
    --output-metadata)
      [[ $# -ge 2 ]] || fail "--output-metadata requires a path"
      OUTPUT_METADATA="$2"
      shift 2
      ;;
    --output-sha256)
      [[ $# -ge 2 ]] || fail "--output-sha256 requires a path"
      OUTPUT_SHA256="$2"
      shift 2
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
      fail "unknown argument: $1"
      ;;
  esac
done

[[ -n "$STAGE_DIR" ]] || fail "--stage-dir is required"
[[ -n "$OUTPUT_IMAGE" ]] || fail "--output-image is required"
[[ -n "$OUTPUT_METADATA" ]] || fail "--output-metadata is required"
[[ -n "$OUTPUT_SHA256" ]] || fail "--output-sha256 is required"
[[ -d "$STAGE_DIR" && ! -L "$STAGE_DIR" ]] || \
  fail "--stage-dir must select a regular directory"

for command in dd dot_clean hdiutil newfs_msdos python3 sync truncate; do
  command -v "$command" >/dev/null 2>&1 || fail "required command is missing: $command"
done
[[ "$(uname -s)" == "Darwin" ]] || \
  fail "portable Pi 4 release-image creation currently requires macOS hdiutil"

python3 - "$STAGE_DIR" "$OUTPUT_IMAGE" "$OUTPUT_METADATA" "$OUTPUT_SHA256" <<'PY'
from pathlib import Path
import sys

stage = Path(sys.argv[1]).resolve()
outputs = [Path(value).expanduser().resolve(strict=False) for value in sys.argv[2:]]
if len(set(outputs)) != len(outputs):
    raise SystemExit("Pi 4 release-image output paths must be distinct")
for output in outputs:
    if output == Path("/") or output == Path.home().resolve() or output == stage:
        raise SystemExit(f"unsafe Pi 4 release-image output path: {output}")
    if output.exists() and output.is_symlink():
        raise SystemExit(f"Pi 4 release-image output must not be a symlink: {output}")
entries = list(stage.rglob("*"))
if not entries or any(path.is_symlink() for path in entries):
    raise SystemExit("Pi 4 stage must be non-empty and contain no symlinks")
if any(not path.is_file() and not path.is_dir() for path in entries):
    raise SystemExit("Pi 4 stage contains a non-regular entry")
PY

if [[ "$FORCE" -eq 0 ]]; then
  for output in "$OUTPUT_IMAGE" "$OUTPUT_METADATA" "$OUTPUT_SHA256"; do
    [[ ! -e "$output" ]] || fail "output already exists: $output (use --force)"
  done
fi

work_dir="$(mktemp -d)"
cleanup() {
  if [[ -n "$ATTACHED_DEVICE" ]]; then
    hdiutil detach "$ATTACHED_DEVICE" >/dev/null 2>&1 || true
    ATTACHED_DEVICE=""
  fi
  rm -rf "$work_dir"
}
trap cleanup EXIT INT TERM

read -r disk_mib payload_bytes payload_files < <(
  python3 - "$STAGE_DIR" <<'PY'
from pathlib import Path
import sys

stage = Path(sys.argv[1])
files = [path for path in stage.rglob("*") if path.is_file()]
payload_bytes = sum(path.stat().st_size for path in files)
allocated_bytes = sum((path.stat().st_size + 4095) // 4096 * 4096 for path in files)
minimum_partition_bytes = allocated_bytes * 2 + 16 * 1024 * 1024
minimum_disk_bytes = max(64 * 1024 * 1024, minimum_partition_bytes + 1024 * 1024)
alignment = 4 * 1024 * 1024
disk_bytes = (minimum_disk_bytes + alignment - 1) // alignment * alignment
print(disk_bytes // (1024 * 1024), payload_bytes, len(files))
PY
)

total_sectors=$((disk_mib * 1024 * 1024 / SECTOR_SIZE))
partition_sectors=$((total_sectors - PARTITION_START_LBA))
[[ "$partition_sectors" -gt 0 && "$partition_sectors" -le 4294967295 ]] || \
  fail "derived Pi 4 partition size is outside MBR bounds"

partition_image="${work_dir}/partition.img"
raw_image="${work_dir}/cohesix-pi4-sd.img"
attach_plist="${work_dir}/attach.plist"
partition_bytes=$((partition_sectors * SECTOR_SIZE))

truncate -s "$partition_bytes" "$partition_image"
ATTACHED_DEVICE="$(hdiutil attach -nomount "$partition_image" | awk 'NR == 1 {print $1}')"
[[ "$ATTACHED_DEVICE" =~ ^/dev/disk[0-9]+$ ]] || \
  fail "unexpected temporary FAT device: $ATTACHED_DEVICE"
newfs_msdos -F 32 -v "$VOLUME_LABEL" "$ATTACHED_DEVICE" >/dev/null
hdiutil detach "$ATTACHED_DEVICE" >/dev/null
ATTACHED_DEVICE=""

hdiutil attach -plist -nobrowse -owners off "$partition_image" >"$attach_plist"
IFS=$'\t' read -r ATTACHED_DEVICE mount_point < <(
  python3 - "$attach_plist" <<'PY'
import plistlib
import sys

with open(sys.argv[1], "rb") as handle:
    payload = plistlib.load(handle)
matches = [
    entry
    for entry in payload.get("system-entities", [])
    if entry.get("mount-point") and entry.get("dev-entry")
]
if len(matches) != 1:
    raise SystemExit("temporary FAT image did not expose exactly one mounted volume")
print(matches[0]["dev-entry"], matches[0]["mount-point"], sep="\t")
PY
)
[[ "$ATTACHED_DEVICE" =~ ^/dev/disk[0-9]+$ ]] || \
  fail "unexpected mounted FAT device: $ATTACHED_DEVICE"
[[ -n "$mount_point" && -d "$mount_point" ]] || fail "temporary FAT volume is not mounted"

python3 - "$STAGE_DIR" "$mount_point" <<'PY'
from pathlib import Path
import shutil
import sys

source = Path(sys.argv[1])
mounted = Path(sys.argv[2])
for path in sorted(source.rglob("*"), key=lambda item: item.as_posix()):
    destination = mounted / path.relative_to(source)
    if path.is_dir():
        destination.mkdir(exist_ok=True)
        continue
    destination.parent.mkdir(parents=True, exist_ok=True)
    with path.open("rb") as input_handle, destination.open("wb") as output_handle:
        shutil.copyfileobj(input_handle, output_handle, length=1024 * 1024)
PY
sync
dot_clean -m "$mount_point"
sync
python3 - "$STAGE_DIR" "$mount_point" <<'PY'
from pathlib import Path
import hashlib
import sys

source = Path(sys.argv[1])
mounted = Path(sys.argv[2])

def records(root: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    for path in root.rglob("*"):
        if path.is_symlink() or not path.is_file():
            continue
        relative = path.relative_to(root).as_posix()
        result[relative] = hashlib.sha256(path.read_bytes()).hexdigest()
    return result

expected = records(source)
actual = records(mounted)
if actual != expected:
    raise SystemExit(
        "temporary FAT payload drift: "
        f"missing={sorted(expected.keys() - actual.keys())} "
        f"unexpected={sorted(actual.keys() - expected.keys())}"
    )
PY
hdiutil detach "$ATTACHED_DEVICE" >/dev/null
ATTACHED_DEVICE=""

truncate -s $((total_sectors * SECTOR_SIZE)) "$raw_image"
python3 - "$raw_image" "$PARTITION_START_LBA" "$partition_sectors" <<'PY'
from pathlib import Path
import struct
import sys

image = Path(sys.argv[1])
start_lba = int(sys.argv[2])
sector_count = int(sys.argv[3])
mbr = bytearray(512)
mbr[446:462] = struct.pack(
    "<B3sB3sII",
    0,
    b"\x20\x21\x00",
    0x0C,
    b"\xfe\xff\xff",
    start_lba,
    sector_count,
)
mbr[510:512] = b"\x55\xaa"
with image.open("r+b") as handle:
    handle.write(mbr)
PY
dd if="$partition_image" of="$raw_image" bs="$SECTOR_SIZE" \
  seek="$PARTITION_START_LBA" conv=notrunc status=none

hdiutil attach -readonly -plist -nobrowse -owners off "$raw_image" >"$attach_plist"
IFS=$'\t' read -r ATTACHED_DEVICE mount_point < <(
  python3 - "$attach_plist" <<'PY'
import plistlib
import re
import sys

with open(sys.argv[1], "rb") as handle:
    payload = plistlib.load(handle)
matches = [
    entry
    for entry in payload.get("system-entities", [])
    if entry.get("mount-point") and entry.get("dev-entry")
]
if len(matches) != 1:
    raise SystemExit("portable raw image did not expose exactly one mounted volume")
partition = matches[0]["dev-entry"]
disk = re.sub(r"s[0-9]+$", "", partition)
print(disk, matches[0]["mount-point"], sep="\t")
PY
)
[[ "$ATTACHED_DEVICE" =~ ^/dev/disk[0-9]+$ ]] || \
  fail "unexpected portable image device: $ATTACHED_DEVICE"
python3 - "$STAGE_DIR" "$mount_point" <<'PY'
from pathlib import Path
import hashlib
import sys

source = Path(sys.argv[1])
mounted = Path(sys.argv[2])

def records(root: Path) -> dict[str, str]:
    return {
        path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in root.rglob("*")
        if path.is_file() and not path.is_symlink()
    }

if records(source) != records(mounted):
    raise SystemExit("portable raw image payload differs from the exact Pi 4 stage")
PY
hdiutil detach "$ATTACHED_DEVICE" >/dev/null
ATTACHED_DEVICE=""

mkdir -p \
  "$(dirname "$OUTPUT_IMAGE")" \
  "$(dirname "$OUTPUT_METADATA")" \
  "$(dirname "$OUTPUT_SHA256")"
metadata_temp="${work_dir}/cohesix-pi4-sd.json"
sha256_temp="${work_dir}/cohesix-pi4-sd.img.sha256"
image_sha256="$(python3 - "$raw_image" <<'PY'
from pathlib import Path
import hashlib
import sys

digest = hashlib.sha256()
with Path(sys.argv[1]).open("rb") as handle:
    for chunk in iter(lambda: handle.read(1024 * 1024), b""):
        digest.update(chunk)
print(digest.hexdigest())
PY
)"
python3 - \
  "$STAGE_DIR" \
  "$raw_image" \
  "$metadata_temp" \
  "$image_sha256" \
  "$SECTOR_SIZE" \
  "$PARTITION_START_LBA" \
  "$partition_sectors" \
  "$payload_bytes" \
  "$payload_files" <<'PY'
from pathlib import Path
import hashlib
import json
import sys

(
    stage_value,
    image_value,
    metadata_value,
    image_sha256,
    sector_size,
    partition_start,
    partition_sectors,
    payload_bytes,
    payload_files,
) = sys.argv[1:]
stage = Path(stage_value)
image = Path(image_value)
records = []
for path in sorted(stage.rglob("*"), key=lambda item: item.as_posix()):
    if not path.is_file() or path.is_symlink():
        continue
    records.append(
        {
            "path": path.relative_to(stage).as_posix(),
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "size_bytes": path.stat().st_size,
        }
    )
payload = {
    "schema": "cohesix-pi4-portable-sd-image/v1",
    "filesystem": "FAT32",
    "image_filename": Path(sys.argv[2]).name,
    "image_sha256": image_sha256,
    "image_size_bytes": image.stat().st_size,
    "minimum_target_bytes": image.stat().st_size,
    "partition_scheme": "MBR",
    "partition_start_lba": int(partition_start),
    "partition_sector_count": int(partition_sectors),
    "payload_file_count": int(payload_files),
    "payload_size_bytes": int(payload_bytes),
    "sector_size_bytes": int(sector_size),
    "volume_label": "COHESIX",
    "files": records,
}
Path(metadata_value).write_text(
    json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
)
PY
printf '%s  %s\n' "$image_sha256" "$(basename "$OUTPUT_IMAGE")" >"$sha256_temp"
mv -f "$raw_image" "$OUTPUT_IMAGE"
mv -f "$metadata_temp" "$OUTPUT_METADATA"
mv -f "$sha256_temp" "$OUTPUT_SHA256"

printf '[pi4-release-image] Image: %s\n' "$OUTPUT_IMAGE"
printf '[pi4-release-image] Minimum target bytes: %s\n' $((total_sectors * SECTOR_SIZE))
printf '[pi4-release-image] Embedded payload: %s files, %s bytes\n' \
  "$payload_files" "$payload_bytes"
