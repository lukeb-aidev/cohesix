#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build the supported digest-pinned batch edge workload for the selected Jetson CUDA host.
# Copyright 2026 Lukas Bower
set -euo pipefail

if [[ $# -ne 1 || "$(uname -s)" != Linux ]]; then
  echo "usage: scripts/build-gpu-batch-edges.sh FRESH_OUTPUT_DIRECTORY (Linux CUDA host)" >&2
  exit 2
fi

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$1"
cuda_root="${COH_CUDA_ROOT:-/usr/local/cuda}"
if [[ -e "$output_dir" || ! -x "$cuda_root/bin/nvcc" ]]; then
  echo "fresh output directory and selected CUDA compiler required" >&2
  exit 2
fi

profile="$(python3 - "$repo_root/configs/generated/provider_registry.json" "$cuda_root/version.json" <<'PY'
import json
import platform
import sys

registry = json.load(open(sys.argv[1], encoding="utf-8"))
profile = registry["contract"]["gpu_executor"]["profile"]
selected = next(row for row in registry["contract"]["profiles"] if row["id"] == profile)
version = json.load(open(sys.argv[2], encoding="utf-8"))["cuda"]["version"]
if (profile != "jetson-orin-nano-jp7" or platform.machine() != "aarch64"
        or version != selected["cuda_toolkit"]):
    raise SystemExit("profile_mismatch: Jetson Orin Nano CUDA toolkit required")
print(selected["compute_capability"].replace(".", ""))
PY
)"

umask 077
mkdir -p -- "$output_dir"
"$cuda_root/bin/nvcc" -std=c++17 -O2 -arch="sm_${profile}" \
  "$repo_root/apps/gpu-bridge-host/cuda/batch_edges.cu" \
  -o "$output_dir/batch-edges" >"$output_dir/build.log" 2>&1
python3 - "$repo_root" "$output_dir" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

root, output = map(Path, sys.argv[1:])
source = root / "apps/gpu-bridge-host/cuda/batch_edges.cu"
package = output / "batch-edges"
manifest = {
    "schema": "cohesix-registered-cuda-build/v1",
    "workload_id": "batch-edges",
    "profile": "jetson-orin-nano-jp7",
    "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
    "package_sha256": hashlib.sha256(package.read_bytes()).hexdigest(),
    "authoritative": False,
}
(output / "build.json").write_text(json.dumps(manifest, sort_keys=True, indent=2) + "\n", encoding="utf-8")
print(json.dumps(manifest, sort_keys=True))
PY
