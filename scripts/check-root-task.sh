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

if ! command -v python3 >/dev/null 2>&1; then
    echo "python3 interpreter is required for ELF inspection" >&2
    exit 69
fi

if ! metrics=$(python3 - "$rootserver" <<'PY'
import struct
import sys
from pathlib import Path

path = Path(sys.argv[1])
try:
    data = path.read_bytes()
except OSError as exc:  # pragma: no cover - filesystem error surface
    print(f"failed to read {path}: {exc}", file=sys.stderr)
    sys.exit(1)

if len(data) < 16 or data[:4] != b"\x7fELF":
    print("rootserver is not an ELF image", file=sys.stderr)
    sys.exit(1)

ei_class = data[4]
ei_data = data[5]
if ei_class != 2:
    print("rootserver must be a 64-bit ELF image", file=sys.stderr)
    sys.exit(1)
if ei_data not in (1, 2):
    print("unsupported ELF data encoding", file=sys.stderr)
    sys.exit(1)

endian = "<" if ei_data == 1 else ">"
header_fmt = endian + "HHIQQQIHHHHHH"

try:
    header = struct.unpack_from(header_fmt, data, 16)
except struct.error:
    print("ELF header truncated", file=sys.stderr)
    sys.exit(1)

e_phoff = header[4]
e_phentsize = header[7]
e_phnum = header[8]

ph_fmt = endian + "IIQQQQQQ"
segments = []

for index in range(e_phnum):
    offset = e_phoff + index * e_phentsize
    try:
        p_type, _, _, _, p_paddr, _, p_memsz, _ = struct.unpack_from(ph_fmt, data, offset)
    except struct.error:
        print("program header truncated", file=sys.stderr)
        sys.exit(1)
    if p_type != 1 or p_memsz == 0:
        continue
    segments.append((p_paddr, p_memsz))

if not segments:
    print("rootserver ELF is missing PT_LOAD segments", file=sys.stderr)
    sys.exit(1)

min_start = min(start for start, _ in segments)
max_end = max(start + size for start, size in segments)
span = max_end - min_start
max_segment = max(size for _, size in segments)

print(f"{min_start} {max_end} {span} {max_segment}")
PY
); then
    echo "failed to inspect rootserver ELF program headers" >&2
    exit 70
fi

read -r min_start max_end span_bytes max_segment_bytes <<<"$metrics"

max_span_limit=$((8 * 1024 * 1024))
if (( span_bytes > max_span_limit )); then
    printf 'rootserver PT_LOAD span %s bytes (0x%X) exceeds %s bytes (0x%X); range [0x%X..0x%X)\n' \
        "$span_bytes" "$span_bytes" "$max_span_limit" "$max_span_limit" \
        "$min_start" "$max_end" >&2
    exit 1
fi

if (( max_end >= 0x1_0000_0000 )); then
    printf 'rootserver physical end address 0x%X exceeds 32-bit space\n' "$max_end" >&2
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
