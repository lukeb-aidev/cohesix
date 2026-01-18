#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Compare convergence transcripts across transports and frontends.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
output_root="${target_dir}/convergence-transcripts"
scenario="converge_v0"
tolerance_ms="${CONVERGENCE_TOLERANCE_MS:-50}"

cargo test -p cohsh-core --test transcripts
cargo test -p cohsh --test transcripts
cargo test -p swarmui --test transcript
cargo test -p coh-status --test transcript

compare_pair() {
    local left="$1"
    local right="$2"
    if [[ ! -f "$left" ]]; then
        echo "missing transcript: $left" >&2
        return 2
    fi
    if [[ ! -f "$right" ]]; then
        echo "missing transcript: $right" >&2
        return 2
    fi
    diff -u "$left" "$right"
}

for case_name in boot_v0 abuse "${scenario}"; do
    serial="${output_root}/cohsh-core/${case_name}/serial.txt"
    core="${output_root}/cohsh-core/${case_name}/core.txt"
    tcp="${output_root}/cohsh-core/${case_name}/tcp.txt"
    compare_pair "$serial" "$core"
    compare_pair "$serial" "$tcp"
done

baseline="${output_root}/cohsh-core/${scenario}/serial.txt"
compare_pair "$baseline" "${output_root}/cohsh/${scenario}/cohsh.txt"
compare_pair "$baseline" "${output_root}/swarmui/${scenario}/swarmui.txt"
compare_pair "$baseline" "${output_root}/coh-status/${scenario}/coh-status.txt"

python3 - "$tolerance_ms" \
    "${output_root}/cohsh-core/${scenario}/timing-serial.txt" \
    "${output_root}/cohsh-core/${scenario}/timing-core.txt" \
    "${output_root}/cohsh-core/${scenario}/timing-tcp.txt" \
    "${output_root}/cohsh/${scenario}/timing-transcript.txt" \
    "${output_root}/swarmui/${scenario}/timing-transcript.txt" \
    "${output_root}/coh-status/${scenario}/timing-transcript.txt" <<'PY'
import pathlib
import sys

tolerance = int(sys.argv[1])
paths = [pathlib.Path(p) for p in sys.argv[2:]]
values = {}
for path in paths:
    if not path.is_file():
        print(f"missing timing file: {path}", file=sys.stderr)
        sys.exit(2)
    text = path.read_text().strip()
    if not text.startswith("elapsed_ms="):
        print(f"invalid timing format in {path}: {text}", file=sys.stderr)
        sys.exit(2)
    raw = text.split("=", 1)[1]
    values[path] = int(raw)

min_value = min(values.values())
max_value = max(values.values())
print("timing window ms:")
for path, value in values.items():
    print(f"  {path}: {value}")
print(f"delta={max_value - min_value} tolerance={tolerance}")
if max_value - min_value > tolerance:
    print("timing delta exceeds tolerance", file=sys.stderr)
    sys.exit(1)
PY

printf "transcript compare ok: zero-byte delta\n"
