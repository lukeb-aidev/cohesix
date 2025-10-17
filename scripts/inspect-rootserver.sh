#!/usr/bin/env bash
# Author: Lukas Bower

set -euo pipefail

rootserver=${1:-out/cohesix/staging/rootserver}
if [[ ! -f "$rootserver" ]]; then
    echo "rootserver ELF not found: $rootserver" >&2
    exit 66
fi

echo "== readelf -l $rootserver =="
readelf -l "$rootserver"

echo
nm_output=$(mktemp)
trap 'rm -f "$nm_output" 2>/dev/null || true' EXIT

echo "== size -A $rootserver =="
size -A "$rootserver"

echo
nm -S --size-sort "$rootserver" >"$nm_output"
echo "== nm -S --size-sort $rootserver | tail -n 50 =="
tail -n 50 "$nm_output"

python3 - "$rootserver" "$nm_output" <<'PY'
import struct
import sys
from pathlib import Path

path = Path(sys.argv[1])
nm_report = Path(sys.argv[2])

data = path.read_bytes()
if len(data) < 16 or data[:4] != b"\x7fELF":
    print("ERROR: image is not an ELF file", file=sys.stderr)
    sys.exit(1)

ei_class = data[4]
ei_data = data[5]
if ei_class != 2:
    print("ERROR: expected a 64-bit ELF image", file=sys.stderr)
    sys.exit(1)
if ei_data not in (1, 2):
    print("ERROR: unsupported ELF data encoding", file=sys.stderr)
    sys.exit(1)

endian = "<" if ei_data == 1 else ">"
header_fmt = endian + "HHIQQQIHHHHHH"

type_, machine, version, entry, phoff, shoff, flags, ehsize, phentsize, phnum, shentsize, shnum, shstrndx = struct.unpack_from(
    header_fmt, data, 16
)

ph_fmt = endian + "IIQQQQQQ"
violations = False
threshold_segment = 32 * 1024 * 1024
for index in range(phnum):
    offset = phoff + index * phentsize
    p_type, _, _, _, p_paddr, _, p_memsz, _ = struct.unpack_from(ph_fmt, data, offset)
    if p_type != 1 or p_memsz == 0:
        continue
    if p_memsz > threshold_segment:
        print(
            f"ERROR: PT_LOAD #{index} MemSiz {p_memsz} bytes (0x{p_memsz:X}) exceeds {threshold_segment} bytes",
            file=sys.stderr,
        )
        violations = True

sym_threshold = 4 * 1024 * 1024
with nm_report.open("r", encoding="utf-8", errors="ignore") as handle:
    for line in handle:
        parts = line.strip().split()
        if len(parts) < 4:
            continue
        try:
            size = int(parts[1], 16)
        except ValueError:
            continue
        if size > sym_threshold:
            name = parts[-1]
            print(
                f"ERROR: symbol {name} size {size} bytes (0x{size:X}) exceeds {sym_threshold} bytes",
                file=sys.stderr,
            )
            violations = True

if violations:
    sys.exit(1)
PY
