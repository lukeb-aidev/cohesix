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
e_shoff = header[5]
e_phentsize = header[8]
e_phnum = header[9]
e_shentsize = header[10]
e_shnum = header[11]
e_shstrndx = header[12]

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

sh_fmt = endian + "IIQQQQIIQQ"
sections = []

for index in range(e_shnum):
    offset = e_shoff + index * e_shentsize
    try:
        section = struct.unpack_from(sh_fmt, data, offset)
    except struct.error:
        print("section header truncated", file=sys.stderr)
        sys.exit(1)
    sections.append(section)

if not sections:
    print("rootserver ELF is missing section headers", file=sys.stderr)
    sys.exit(1)

if not (0 <= e_shstrndx < len(sections)):
    print("section string table index out of range", file=sys.stderr)
    sys.exit(1)

strtab_header = sections[e_shstrndx]
_, _, _, _, strtab_offset, strtab_size, _, _, _, _ = strtab_header
shstr = data[strtab_offset : strtab_offset + strtab_size]

def section_name(offset):
    end = shstr.find(b"\x00", offset)
    if end == -1:
        return ""
    return shstr[offset:end].decode(errors="ignore")

bss_bytes = 0
for header in sections:
    sh_name, sh_type, _, _, sh_offset, sh_size, _, _, _, _ = header
    name = section_name(sh_name)
    if name.startswith(".bss"):
        bss_bytes += sh_size

print(f"{min_start} {max_end} {span} {max_segment} {bss_bytes}")
PY
); then
    echo "failed to inspect rootserver ELF program headers" >&2
    exit 70
fi

read -r min_start max_end span_bytes max_segment_bytes bss_bytes <<<"$metrics"

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

max_segment_limit=$((32 * 1024 * 1024))
if (( max_segment_bytes > max_segment_limit )); then
    printf 'rootserver PT_LOAD MemSiz %s bytes (0x%X) exceeds %s bytes (0x%X)\n' \
        "$max_segment_bytes" "$max_segment_bytes" "$max_segment_limit" "$max_segment_limit" >&2
    exit 1
fi

bss_limit=$((8 * 1024 * 1024))
if (( bss_bytes > bss_limit )); then
    printf '.bss sections total %s bytes (0x%X) exceeds %s bytes (0x%X)\n' \
        "$bss_bytes" "$bss_bytes" "$bss_limit" "$bss_limit" >&2
    exit 1
fi

file_size_limit=$((8 * 1024 * 1024))
file_size=$(python3 - "$rootserver" <<'PY'
import os
import sys
print(os.path.getsize(sys.argv[1]))
PY
)

if (( file_size > file_size_limit )); then
    printf 'rootserver file size %s bytes (0x%X) exceeds %s bytes (0x%X)\n' \
        "$file_size" "$file_size" "$file_size_limit" "$file_size_limit" >&2
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

echo "top 10 largest symbols:"
nm -S --size-sort "$rootserver" | tail -n 10

echo "root-task guard checks passed"
