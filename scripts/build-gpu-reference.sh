#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build the bounded gpu-bridge-host CUDA reference child on the selected native Linux CUDA installation.
# Copyright 2026 Lukas Bower
set -euo pipefail
if [[ $# -ne 1 ]]; then
  echo "usage: scripts/build-gpu-reference.sh OUTPUT_DIRECTORY" >&2
  exit 2
fi
if [[ "$(uname -s)" != Linux ]]; then
  echo "not_supported: CUDA reference build requires Linux" >&2
  exit 2
fi
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$1"
if [[ -e "$output_dir" ]]; then
  echo "output directory must be fresh" >&2
  exit 2
fi
cuda_root="${COH_CUDA_ROOT:-/usr/local/cuda}"
if [[ ! -x "$cuda_root/bin/nvcc" ]]; then
  echo "not_enabled: selected CUDA compiler unavailable" >&2
  exit 2
fi
umask 077
mkdir -p -- "$output_dir"
python3 - "$repo_root/configs/generated/provider_registry.json" "$cuda_root/version.json" >"$output_dir/architecture" <<'PY'
import json
import platform
import sys
registry = json.load(open(sys.argv[1]))
profile = next(row for row in registry["contract"]["profiles"] if row["id"] == "jetson-orin-nano-jp7")
selected = registry["contract"]["gpu_executor"]["profile"]
if selected == "jetson-orin-nano-jp7":
    if platform.machine() != profile["architecture"]:
        raise SystemExit("profile_mismatch: exact native host architecture required")
elif selected == "nvidia-mig-cuda13":
    if platform.machine() not in {"aarch64", "x86_64"}:
        raise SystemExit("not_supported: MIG helper requires native Linux AArch64 or x86_64")
else:
    raise SystemExit("not_supported: generated CUDA executor profile")
version = json.load(open(sys.argv[2]))["cuda"]["version"]
if version != profile["cuda_toolkit"]:
    raise SystemExit("profile_mismatch: exact CUDA toolkit version required")
capability = profile["compute_capability"].replace(".", "")
if not capability.isdigit():
    raise SystemExit("invalid generated architecture")
print("sm_" + capability if selected == "jetson-orin-nano-jp7" else "compute_80")
PY
architecture="$(cat -- "$output_dir/architecture")"
if [[ "$architecture" == compute_80 ]]; then
  architecture_args=(-gencode=arch=compute_80,code=compute_80 -DCOH_REFERENCE_MIG=1)
else
  architecture_args=(-arch="$architecture" -DCOH_REFERENCE_MIG=0)
fi
"$cuda_root/bin/nvcc" -std=c++17 -O2 -lineinfo "${architecture_args[@]}" \
  "$repo_root/apps/gpu-bridge-host/cuda/reference.cu" -o "$output_dir/cohesix-cuda-reference" \
  -L"$cuda_root/lib64/stubs" -lcuda -ldl \
  >"$output_dir/build.log" 2>&1
python3 - "$repo_root" "$output_dir" <<'PY'
import hashlib
import json
from pathlib import Path
import sys
root, output = map(Path, sys.argv[1:])
registry = json.loads((root / "configs/generated/provider_registry.json").read_text())
manifest = {"schema": "cohesix-cuda-reference-build/v1", "authoritative": False,
            "profile_id": registry["contract"]["gpu_executor"]["profile"], "provider_graph_sha256": registry["graph_sha256"],
            "source_sha256": hashlib.sha256((root / "apps/gpu-bridge-host/cuda/reference.cu").read_bytes()).hexdigest(),
            "support_source_sha256": {"mig_inventory.hpp": hashlib.sha256((root / "apps/gpu-bridge-host/cuda/mig_inventory.hpp").read_bytes()).hexdigest()},
            "binary_sha256": hashlib.sha256((output / "cohesix-cuda-reference").read_bytes()).hexdigest()}
(output / "build.json").write_text(json.dumps(manifest, sort_keys=True, indent=2) + "\n")
print(json.dumps(manifest))
PY
