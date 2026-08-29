#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Assemble exact Cohesix runtime bundles, including QEMU and Raspberry Pi 4 artifacts.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
RELEASES_DIR="${ROOT_DIR}/releases"

RELEASE_NAME=""
RELEASE_VERSION=""
FORCE=0
LINUX_BUNDLE=0
LINUX_ONLY=0
CHECK_MANIFEST=0
VERIFY_WORKER_ACCEPTANCE=0
WORKER_QEMU_EVIDENCE=""
WORKER_PI4_EVIDENCE=""
WORKER_ROOT_QEMU_EVIDENCE=""
WORKER_ROOT_PI4_EVIDENCE=""
WORKER_SYSTEM_QEMU_EVIDENCE=""
WORKER_SYSTEM_PI4_EVIDENCE=""
LINUX_HOST_TOOLS_DIR=""
LINUX_HOST_TOOLS_MANIFEST=""
LINUX_BUILDER_HOST=""
LINUX_BUILDER_USER=""
LINUX_BUILDER_KEY=""
LINUX_BUILDER_BUILD_DIR=""
LINUX_BUILDER_RELEASE_DIR=""
LINUX_BUILDER_CARGO=""
LINUX_BUILDER_CARGO_HOME=""
LINUX_BUILDER_MAX_GLIBC=""
PI4_STAGE_DIR=""
SEL4_BUILD_DIR="${SEL4_BUILD_DIR:-${ROOT_DIR}/out/sel4/profile-v2/qemu-smp-production}"
IMPLEMENTATION_SURFACE_INVENTORY="${IMPLEMENTATION_SURFACE_INVENTORY:-${ROOT_DIR}/configs/generated/implementation_surface_inventory.json}"
PYTHON_WHEEL_DIR="${PYTHON_WHEEL_DIR:-${ROOT_DIR}/out/python-wheels}"
PYTHON_PACKAGE_MANIFEST="${PYTHON_PACKAGE_MANIFEST:-${ROOT_DIR}/out/python-compat/m26e-python-package.json}"

usage() {
  cat <<'USAGE'
Usage: scripts/release_bundle.sh [release options] [--check-manifest]
       scripts/release_bundle.sh --verify-worker-acceptance <six evidence paths>

Assembles peer releases/<release-name>-MacOS and
releases/<release-name>-Pi4 bundles and archives. With --linux, it also creates
the peer releases/<release-name>-linux bundle and archive.

With --linux, builds Linux ARM64 host tools and the final Linux release tarball
on the explicitly selected remote builder. Use --linux-only to omit macOS.

Release options:
  --name <name>                       Required when creating bundles
  --version <version>                 Defaults to the compiler inventory version
  --force                             Replace the selected output bundle(s)
  --linux                             Also create the remote-built Linux bundle
  --linux-only                        Create only the remote-built Linux bundle
  --pi4-stage-dir <path>              Exact canonical Pi 4 SD staging directory

Remote Linux builder options (all required with --linux except --key):
  --linux-builder-host <host>         SSH hostname or address
  --linux-builder-user <user>         SSH username
  --linux-builder-key <path>          Optional SSH key; omit for SSH agent/config auth
  --linux-builder-build-dir <path>    Remote source/target/staging root
  --linux-builder-release-dir <path>  Remote directory retaining the release tarball
  --linux-builder-cargo <path>        Absolute remote cargo executable
  --linux-builder-cargo-home <path>   Remote Cargo registry/cache directory
  --linux-builder-max-glibc <x.y>     Maximum permitted GLIBC symbol version
  --linux-host-tools-dir <path>       Local destination for downloaded host tools
  --linux-host-tools-manifest <path>  Local destination for build provenance JSON

Env overrides:
  SEL4_BUILD_DIR (defaults to $REPO/out/sel4/profile-v2/qemu-smp-production;
                  the selected tree must pass qemu_smp_production release validation)
  IMPLEMENTATION_SURFACE_INVENTORY (defaults to the canonical generated inventory;
                                    intended only for non-mutating pre-regeneration validation)
  PYTHON_WHEEL_DIR (defaults to out/python-wheels; must contain one target-neutral wheel)
  PYTHON_PACKAGE_MANIFEST (defaults to out/python-compat/m26e-python-package.json)

Remote builder environment locations are never inferred from hostnames or users
and have no embedded Jetson/NVMe defaults; they must be supplied as arguments.

--check-manifest validates the exact compiler-generated release input set and
exits without creating, replacing, or deleting a release bundle.

--verify-worker-acceptance validates the immutable QEMU/Pi Worker-component,
root-TCB, and full-system graph without creating a release bundle. It requires:
  --worker-qemu-evidence <path>       --worker-pi4-evidence <path>
  --worker-root-qemu-evidence <path>  --worker-root-pi4-evidence <path>
  --worker-system-qemu-evidence <path> --worker-system-pi4-evidence <path>
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --name)
      [[ $# -ge 2 ]] || { echo "--name requires a value" >&2; exit 1; }
      RELEASE_NAME="$2"
      shift 2
      ;;
    --version)
      [[ $# -ge 2 ]] || { echo "--version requires a value" >&2; exit 1; }
      RELEASE_VERSION="$2"
      shift 2
      ;;
    --force)
      FORCE=1
      shift
      ;;
    --linux)
      LINUX_BUNDLE=1
      shift
      ;;
    --linux-only)
      LINUX_BUNDLE=1
      LINUX_ONLY=1
      shift
      ;;
    --pi4-stage-dir)
      [[ $# -ge 2 ]] || { echo "--pi4-stage-dir requires a path" >&2; exit 1; }
      PI4_STAGE_DIR="$2"
      shift 2
      ;;
    --linux-builder-host)
      [[ $# -ge 2 ]] || { echo "--linux-builder-host requires a value" >&2; exit 1; }
      LINUX_BUILDER_HOST="$2"
      shift 2
      ;;
    --linux-builder-user)
      [[ $# -ge 2 ]] || { echo "--linux-builder-user requires a value" >&2; exit 1; }
      LINUX_BUILDER_USER="$2"
      shift 2
      ;;
    --linux-builder-key)
      [[ $# -ge 2 ]] || { echo "--linux-builder-key requires a path" >&2; exit 1; }
      LINUX_BUILDER_KEY="$2"
      shift 2
      ;;
    --linux-builder-build-dir)
      [[ $# -ge 2 ]] || { echo "--linux-builder-build-dir requires a path" >&2; exit 1; }
      LINUX_BUILDER_BUILD_DIR="$2"
      shift 2
      ;;
    --linux-builder-release-dir)
      [[ $# -ge 2 ]] || { echo "--linux-builder-release-dir requires a path" >&2; exit 1; }
      LINUX_BUILDER_RELEASE_DIR="$2"
      shift 2
      ;;
    --linux-builder-cargo)
      [[ $# -ge 2 ]] || { echo "--linux-builder-cargo requires a path" >&2; exit 1; }
      LINUX_BUILDER_CARGO="$2"
      shift 2
      ;;
    --linux-builder-cargo-home)
      [[ $# -ge 2 ]] || { echo "--linux-builder-cargo-home requires a path" >&2; exit 1; }
      LINUX_BUILDER_CARGO_HOME="$2"
      shift 2
      ;;
    --linux-builder-max-glibc)
      [[ $# -ge 2 ]] || { echo "--linux-builder-max-glibc requires a value" >&2; exit 1; }
      LINUX_BUILDER_MAX_GLIBC="$2"
      shift 2
      ;;
    --linux-host-tools-dir)
      [[ $# -ge 2 ]] || { echo "--linux-host-tools-dir requires a path" >&2; exit 1; }
      LINUX_HOST_TOOLS_DIR="$2"
      shift 2
      ;;
    --linux-host-tools-manifest)
      [[ $# -ge 2 ]] || { echo "--linux-host-tools-manifest requires a path" >&2; exit 1; }
      LINUX_HOST_TOOLS_MANIFEST="$2"
      shift 2
      ;;
    --check-manifest)
      CHECK_MANIFEST=1
      shift
      ;;
    --verify-worker-acceptance)
      VERIFY_WORKER_ACCEPTANCE=1
      shift
      ;;
    --worker-qemu-evidence)
      [[ $# -ge 2 ]] || { echo "--worker-qemu-evidence requires a path" >&2; exit 1; }
      WORKER_QEMU_EVIDENCE="$2"
      shift 2
      ;;
    --worker-pi4-evidence)
      [[ $# -ge 2 ]] || { echo "--worker-pi4-evidence requires a path" >&2; exit 1; }
      WORKER_PI4_EVIDENCE="$2"
      shift 2
      ;;
    --worker-root-qemu-evidence)
      [[ $# -ge 2 ]] || { echo "--worker-root-qemu-evidence requires a path" >&2; exit 1; }
      WORKER_ROOT_QEMU_EVIDENCE="$2"
      shift 2
      ;;
    --worker-root-pi4-evidence)
      [[ $# -ge 2 ]] || { echo "--worker-root-pi4-evidence requires a path" >&2; exit 1; }
      WORKER_ROOT_PI4_EVIDENCE="$2"
      shift 2
      ;;
    --worker-system-qemu-evidence)
      [[ $# -ge 2 ]] || { echo "--worker-system-qemu-evidence requires a path" >&2; exit 1; }
      WORKER_SYSTEM_QEMU_EVIDENCE="$2"
      shift 2
      ;;
    --worker-system-pi4-evidence)
      [[ $# -ge 2 ]] || { echo "--worker-system-pi4-evidence requires a path" >&2; exit 1; }
      WORKER_SYSTEM_PI4_EVIDENCE="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

verify_worker_acceptance() {
  local validator="${ROOT_DIR}/scripts/worker_task_evidence.py"
  require_file "$validator"
  local path
  for path in \
    "$WORKER_QEMU_EVIDENCE" \
    "$WORKER_PI4_EVIDENCE" \
    "$WORKER_ROOT_QEMU_EVIDENCE" \
    "$WORKER_ROOT_PI4_EVIDENCE" \
    "$WORKER_SYSTEM_QEMU_EVIDENCE" \
    "$WORKER_SYSTEM_PI4_EVIDENCE"; do
    [[ -n "$path" ]] || fail \
      "--verify-worker-acceptance requires all six target/component/root/system evidence paths"
    require_file "$path"
  done

  local verify_dir
  verify_dir="$(mktemp -d)"
  trap 'rm -rf "${verify_dir}"' RETURN
  python3 "$validator" promote-release \
    --worker-qemu "$WORKER_QEMU_EVIDENCE" \
    --worker-pi4 "$WORKER_PI4_EVIDENCE" \
    --root-qemu "$WORKER_ROOT_QEMU_EVIDENCE" \
    --root-pi4 "$WORKER_ROOT_PI4_EVIDENCE" \
    --system-qemu "$WORKER_SYSTEM_QEMU_EVIDENCE" \
    --system-pi4 "$WORKER_SYSTEM_PI4_EVIDENCE" \
    --out "$verify_dir/worker-release-acceptance.json" || \
    fail "Worker-runtime acceptance graph validation failed"
  echo "[release] Worker-runtime six-record acceptance graph: PASS"
}

OUT_DIR="${ROOT_DIR}/out/cohesix"
STAGING_DIR="${OUT_DIR}/staging"
GENERATED_CONFIG_DIR="${ROOT_DIR}/configs/generated"
DEFAULT_HOST_TOOLS_DIR="${OUT_DIR}/host-tools"
MACOS_BUNDLE_NAME=""
LINUX_BUNDLE_NAME=""
PI4_BUNDLE_NAME=""

fail() {
  echo "$1" >&2
  exit 1
}

if [[ "$IMPLEMENTATION_SURFACE_INVENTORY" != "${ROOT_DIR}/configs/generated/implementation_surface_inventory.json" && "$CHECK_MANIFEST" -ne 1 ]]; then
  fail "non-canonical implementation-surface inventory is allowed only with --check-manifest"
fi

release_inventory_path() {
  printf '%s\n' "${IMPLEMENTATION_SURFACE_INVENTORY}"
}

release_inventory_values() {
  local key="$1"
  python3 - "$(release_inventory_path)" "$key" <<'PY'
import json
from pathlib import Path
import sys

inventory = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
values = inventory.get("release", {}).get(sys.argv[2])
if not isinstance(values, list) or (not values and sys.argv[2] != "versioned_migrations"):
    raise SystemExit(f"release manifest key is missing or empty: {sys.argv[2]}")
for value in values:
    if not isinstance(value, str) or not value or value.startswith("/") or ".." in Path(value).parts:
        raise SystemExit(f"invalid release path in {sys.argv[2]}: {value!r}")
    print(value)
PY
}

release_inventory_scalar() {
  local key="$1"
  python3 - "$(release_inventory_path)" "$key" <<'PY'
import json
from pathlib import Path
import sys

inventory = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
value = inventory.get("release", {}).get(sys.argv[2])
if not isinstance(value, str) or not value:
    raise SystemExit(f"release manifest key is missing or empty: {sys.argv[2]}")
print(value)
PY
}

validate_python_package_inputs() {
  require_dir "$PYTHON_WHEEL_DIR"
  require_file "$PYTHON_PACKAGE_MANIFEST"
  local wheel_candidates=()
  while IFS= read -r candidate; do
    wheel_candidates+=("$candidate")
  done < <(find "$PYTHON_WHEEL_DIR" -maxdepth 1 -type f -name 'cohesix-*.whl' -print)
  [[ ${#wheel_candidates[@]} -eq 1 ]] || fail \
    "Python release input requires exactly one target-neutral cohesix wheel"
  local wheel="${wheel_candidates[0]}"
  [[ ! -L "$wheel" && ! -L "$PYTHON_PACKAGE_MANIFEST" ]] || fail \
    "Python release inputs must be regular non-symlink files"

  python3 - \
    "$PYTHON_PACKAGE_MANIFEST" \
    "$wheel" \
    "$(release_inventory_path)" \
    "${ROOT_DIR}/configs/generated/cohesix_python_qemu_smp_production.json" \
    "${ROOT_DIR}/configs/generated/cohesix_python_pi4_production.json" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

manifest_path, wheel, inventory_path, qemu, pi4 = map(Path, sys.argv[1:])

def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
if manifest.get("schema") != "cohesix-python-package/v1":
    raise SystemExit("release Python package manifest schema is invalid")
wheel_record = manifest.get("wheel", {})
if wheel_record.get("filename") != wheel.name or wheel_record.get("sha256") != digest(wheel):
    raise SystemExit("release Python wheel differs from its package manifest")
inventory = json.loads(inventory_path.read_text(encoding="utf-8"))
wheel_destinations = [
    path
    for path in inventory.get("release", {}).get("generated_bundle_files", [])
    if path.startswith("python/dist/") and path.endswith(".whl")
]
if len(wheel_destinations) != 1:
    raise SystemExit("compiler inventory must select exactly one Python wheel")
if wheel.name != Path(wheel_destinations[0]).name:
    raise SystemExit(f"release Python wheel name is not inventory-selected: {wheel.name}")
contracts = manifest.get("profile_contracts", {})
for target, path in (("qemu", qemu), ("pi4", pi4)):
    value = json.loads(path.read_text(encoding="utf-8"))
    record = contracts.get(target, {})
    if (
        record.get("sha256") != digest(path)
        or record.get("manifest_sha256") != value.get("manifest_sha256")
    ):
        raise SystemExit(f"release Python {target} contract differs from package manifest")
proof = manifest.get("proof_boundary", {})
if any(
    proof.get(field) is not False
    for field in (
        "package_install_is_target_proof",
        "mock_is_target_proof",
        "python_projection_is_authority",
    )
):
    raise SystemExit("release Python package manifest widens proof authority")
PY
  PYTHON_RELEASE_WHEEL="$wheel"
}

validate_release_inventory_inputs() {
  local inventory
  inventory="$(release_inventory_path)"
  require_file "$inventory"
  local inventory_version
  inventory_version="$(release_inventory_scalar version)"
  [[ "$RELEASE_VERSION" == "$inventory_version" ]] || fail \
    "release version ${RELEASE_VERSION} does not match compiler inventory ${inventory_version}"
  python3 "${ROOT_DIR}/scripts/ci/check_implementation_surfaces.py" \
    --repo-root "$ROOT_DIR" \
    --inventory "$inventory"
  validate_python_package_inputs

  INVENTORY_PATH="$inventory" \
  ROOT_DIR="$ROOT_DIR" \
  STAGING_DIR="$STAGING_DIR" \
  OUT_DIR="$OUT_DIR" \
  PI4_STAGE_DIR="$PI4_STAGE_DIR" \
  python3 - <<'PY'
import json
import os
from pathlib import Path

inventory = json.loads(Path(os.environ["INVENTORY_PATH"]).read_text(encoding="utf-8"))
release = inventory["release"]
root = Path(os.environ["ROOT_DIR"])
staging = Path(os.environ["STAGING_DIR"])
out = Path(os.environ["OUT_DIR"])
pi4_stage = Path(os.environ["PI4_STAGE_DIR"])

generated_root = root / "configs/generated"
expected_generated = {Path(path).name for path in release["generated_configs"]}
actual_generated = {path.name for path in generated_root.iterdir() if path.is_file()}
if actual_generated != expected_generated:
    raise SystemExit(
        "generated config release set drift: "
        f"missing={sorted(expected_generated - actual_generated)} "
        f"unexpected={sorted(actual_generated - expected_generated)}"
    )

source_keys = (
    "public_documents",
    "host_assets",
    "operator_scripts",
    "python_artifacts",
    "cas_fixtures",
    "trace_fixtures",
    "transcript_fixtures",
    "ui_assets",
    "support_files",
    "versioned_migrations",
)
for relative in (
    path for key in source_keys for path in release[key]
):
    path = root / relative
    if not path.is_file():
        raise SystemExit(f"release source missing: {path}")

image_sources = {
    "image/elfloader": staging / "elfloader",
    "image/kernel.elf": staging / "kernel.elf",
    "image/rootserver": staging / "rootserver",
    "image/cohesix-system.cpio": out / "cohesix-system.cpio",
    "image/manifest.json": staging / "cohesix/manifest.json",
}
for destination in release["target_images"]:
    if destination == "image/gic-version.txt":
        continue
    source = image_sources.get(destination)
    if source is None or not source.is_file() or source.is_symlink():
        raise SystemExit(f"target image source missing for {destination}: {source}")

expected_pi4 = set(release["pi4_stage_files"])
actual_pi4 = {
    path.relative_to(pi4_stage).as_posix()
    for path in pi4_stage.rglob("*")
    if path.is_file() and not path.is_symlink()
}
if actual_pi4 != expected_pi4:
    raise SystemExit(
        "Pi 4 SD staging set drift: "
        f"missing={sorted(expected_pi4 - actual_pi4)} "
        f"unexpected={sorted(actual_pi4 - expected_pi4)}"
    )
if any(path.is_symlink() for path in pi4_stage.rglob("*")):
    raise SystemExit("Pi 4 SD staging set must not contain symlinks")

for relative in release["forbidden_paths"]:
    if relative in release["host_tools"] or relative in release["target_images"]:
        raise SystemExit(f"forbidden release path is selected: {relative}")
PY

  validate_pi4_stage_identity

  local gic_config="${SEL4_BUILD_DIR}/kernel/gen_config/kernel/gen_config.h"
  require_file "$gic_config"
  local gic_version
  gic_version="$("${ROOT_DIR}/scripts/lib/detect_gic_version.py" "$gic_config")"
  [[ "$gic_version" == "3" ]] || fail \
    "runtime release requires QEMU GICv3; selected kernel reports GIC${gic_version}"
}

validate_pi4_stage_identity() {
  require_dir "$PI4_STAGE_DIR"
  local primary="${PI4_STAGE_DIR}/cohesix-image-arm-bcm2711"
  local fallback="${PI4_STAGE_DIR}/sel4test-driver-image-arm-bcm2711"
  local metadata="${PI4_STAGE_DIR}/pi4-image-identity.json"
  require_file "$primary"
  require_file "$fallback"
  require_file "$metadata"
  cmp -s "$primary" "$fallback" || \
    fail "Pi 4 primary and fallback staged images differ"

  python3 - \
    "$ROOT_DIR" \
    "$primary" \
    "$metadata" \
    "${ROOT_DIR}/scripts/pi4_image_identity.py" <<'PY'
from pathlib import Path
import json
import subprocess
import sys

root, image, metadata_path, verifier = map(Path, sys.argv[1:])
head = subprocess.run(
    ["git", "-C", str(root), "rev-parse", "--verify", "HEAD"],
    check=True,
    capture_output=True,
    text=True,
).stdout.strip()
metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
if metadata.get("schema") != "cohesix-pi4-image-identity/v2":
    raise SystemExit("Pi 4 image identity schema is invalid")
if metadata.get("git_commit") != head or metadata.get("source_tree_clean") is not True:
    raise SystemExit("Pi 4 SD image is not bound to the current clean source commit")
observed = json.loads(
    subprocess.run(
        [sys.executable, str(verifier), "verify", "--image", str(image)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
)
for field in (
    "build_marker",
    "build_marker_sha256",
    "build_timestamp",
    "embedded_git_commit",
    "image_id",
    "image_sha256",
    "size_bytes",
    "uimage_data_crc32",
    "uimage_header_crc32",
):
    if metadata.get(field) != observed.get(field):
        raise SystemExit(f"Pi 4 image identity mismatch: {field}")
if not head.startswith(str(metadata.get("embedded_git_commit", ""))):
    raise SystemExit("Pi 4 image build marker does not identify the current commit")
PY
}

validate_host_tools_inputs() {
  local host_tools_dir="$1"
  local platform="$2"
  local provenance="${3:-}"
  require_dir "$host_tools_dir"

  INVENTORY_PATH="$(release_inventory_path)" \
  HOST_TOOLS_DIR="$host_tools_dir" \
  HOST_PLATFORM="$platform" \
  HOST_PROVENANCE="$provenance" \
  REPO_HEAD="$(git -C "$ROOT_DIR" rev-parse --verify HEAD)" \
  EXPECTED_MAX_GLIBC="$LINUX_BUILDER_MAX_GLIBC" \
  python3 - <<'PY'
from pathlib import Path
import hashlib
import json
import os
import subprocess

inventory = json.loads(Path(os.environ["INVENTORY_PATH"]).read_text(encoding="utf-8"))
expected = {Path(path).name for path in inventory["release"]["host_tools"]}
root = Path(os.environ["HOST_TOOLS_DIR"])
platform = os.environ["HOST_PLATFORM"]
actual = {path.name for path in root.iterdir() if path.is_file() and not path.is_symlink()}
if actual != expected:
    raise SystemExit(
        "host tool set drift: "
        f"missing={sorted(expected - actual)} unexpected={sorted(actual - expected)}"
    )
if any(not path.is_file() or path.is_symlink() for path in root.iterdir()):
    raise SystemExit("host tool directory contains non-regular entries")

descriptions: dict[str, str] = {}
for name in sorted(expected):
    binary = root / name
    description = subprocess.run(
        ["file", "-b", str(binary)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    descriptions[name] = description
    lowered = description.lower()
    if platform == "macos":
        if "mach-o" not in lowered or "arm64" not in lowered:
            raise SystemExit(f"wrong macOS ARM64 host tool: {binary}: {description}")
    elif platform == "linux":
        if "elf" not in lowered or not ("aarch64" in lowered or "arm aarch64" in lowered):
            raise SystemExit(f"wrong Linux ARM64 host tool: {binary}: {description}")
    else:
        raise SystemExit(f"unknown host-tool platform: {platform}")

if platform == "linux":
    provenance_path = Path(os.environ["HOST_PROVENANCE"])
    if not provenance_path.is_file() or provenance_path.is_symlink():
        raise SystemExit(f"Linux host-tool provenance missing: {provenance_path}")
    manifest = json.loads(provenance_path.read_text(encoding="utf-8"))
    if manifest.get("schema") != "cohesix-linux-host-tools-build/v1":
        raise SystemExit("Linux host-tool provenance schema is invalid")
    if manifest.get("source_tree_clean") is not True:
        raise SystemExit("Linux host tools were not built from a clean source tree")
    builder = manifest.get("builder", {})
    if builder.get("source_commit") != os.environ["REPO_HEAD"]:
        raise SystemExit("Linux host tools were built from a different source commit")
    if builder.get("architecture") not in {"aarch64", "arm64"}:
        raise SystemExit("Linux host-tool provenance has the wrong architecture")
    if builder.get("rustc_host") != "aarch64-unknown-linux-gnu":
        raise SystemExit("Linux host-tool provenance has the wrong Rust host")
    if builder.get("max_glibc_version") != os.environ["EXPECTED_MAX_GLIBC"]:
        raise SystemExit("Linux host-tool provenance has the wrong GLIBC ceiling")
    records = manifest.get("artifacts", [])
    by_name = {
        record.get("filename"): record
        for record in records
        if isinstance(record, dict) and isinstance(record.get("filename"), str)
    }
    if set(by_name) != expected or len(records) != len(expected):
        raise SystemExit("Linux host-tool provenance artifact set drift")
    for name in sorted(expected):
        path = root / name
        record = by_name[name]
        if record.get("sha256") != hashlib.sha256(path.read_bytes()).hexdigest():
            raise SystemExit(f"Linux host-tool provenance digest mismatch: {name}")
        if record.get("size_bytes") != path.stat().st_size:
            raise SystemExit(f"Linux host-tool provenance size mismatch: {name}")
        if record.get("file_description") != descriptions[name]:
            raise SystemExit(f"Linux host-tool provenance file kind mismatch: {name}")
PY
}

validate_release_sel4_profile() {
  local profile_python="${ROOT_DIR}/out/toolchain/sel4-profile-venv/bin/python"
  local profile_tool="${ROOT_DIR}/scripts/sel4_profile.py"

  [[ -x "$profile_python" ]] || fail \
    "canonical seL4 profile Python is missing: $profile_python (run toolchain/setup_macos_arm64.sh)"
  [[ -f "$profile_tool" ]] || fail "seL4 profile validator is missing: $profile_tool"
  "$profile_python" "$profile_tool" validate \
    --profile qemu_smp_production \
    --build-dir "$SEL4_BUILD_DIR" \
    --require-source \
    --require-artifacts \
    --for-release \
    || fail "release input does not satisfy qemu_smp_production"
}

require_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    fail "Missing required file: $path"
  fi
}

require_dir() {
  local path="$1"
  if [[ ! -d "$path" ]]; then
    fail "Missing required directory: $path"
  fi
}

write_bundle_manifest() {
  local bundle_dir="$1"
  local expected_key="$2"
  BUNDLE_DIR="$bundle_dir" \
  EXPECTED_KEY="$expected_key" \
  INVENTORY_PATH="$(release_inventory_path)" \
  python3 - <<'PY'
import hashlib
import json
import os
from pathlib import Path

bundle = Path(os.environ["BUNDLE_DIR"])
inventory = json.loads(
    Path(os.environ["INVENTORY_PATH"]).read_text(encoding="utf-8")
)
release = inventory["release"]
expected_key = os.environ["EXPECTED_KEY"]
expected = set(release[expected_key])
manifest_relative = "MANIFEST.sha256"
if manifest_relative not in expected:
    raise SystemExit(
        f"compiler release inventory {expected_key} omits MANIFEST.sha256"
    )

actual_without_manifest = {
    path.relative_to(bundle).as_posix()
    for path in bundle.rglob("*")
    if path.is_file() and path.relative_to(bundle).as_posix() != manifest_relative
}
expected_without_manifest = expected - {manifest_relative}
if actual_without_manifest != expected_without_manifest:
    raise SystemExit(
        f"release bundle {expected_key} file-set drift before manifest: "
        f"missing={sorted(expected_without_manifest - actual_without_manifest)} "
        f"unexpected={sorted(actual_without_manifest - expected_without_manifest)}"
    )

lines = []
for relative in sorted(actual_without_manifest):
    digest = hashlib.sha256((bundle / relative).read_bytes()).hexdigest()
    lines.append(f"{digest}  {relative}")
(bundle / manifest_relative).write_text("\n".join(lines) + "\n", encoding="utf-8")

actual = {
    path.relative_to(bundle).as_posix()
    for path in bundle.rglob("*")
    if path.is_file()
}
if actual != expected:
    raise SystemExit(
        f"release bundle {expected_key} exact file-set drift: "
        f"missing={sorted(expected - actual)} "
        f"unexpected={sorted(actual - expected)}"
    )
for forbidden in release["forbidden_paths"]:
    if forbidden in actual:
        raise SystemExit(f"forbidden release path present: {forbidden}")
PY
}

rewrite_pi4_quickstart() {
  local bundle_dir="$1"
  BUNDLE_DIR="$bundle_dir" python3 - <<'PY'
import os
from pathlib import Path

bundle = Path(os.environ["BUNDLE_DIR"])
readme = bundle / "README.md"
text = readme.read_text(encoding="utf-8")
text = text.replace("docs/QUICKSTART.md", "QUICKSTART.md")
readme.write_text(text, encoding="utf-8")

quickstart = bundle / "QUICKSTART.md"
quickstart.write_text(
    """<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Explain how to verify and flash the portable Cohesix Pi 4 release image. -->
<!-- Author: Lukas Bower -->

# Cohesix Pi 4 release quickstart

The `image/cohesix-pi4-sd.img` file is a complete raw MBR/FAT32 boot image.
Its compact size is derived from the release payload, not from the SD card used
while building it. Read `image/cohesix-pi4-sd.json` and use only a card whose
byte capacity is at least `minimum_target_bytes`. Any larger SD card works; the
remaining capacity is intentionally left unallocated and no expansion is
required for Cohesix to boot.

Verify the release manifest and image digest before writing media:

```bash
shasum -a 256 --check MANIFEST.sha256
cd image
shasum -a 256 --check cohesix-pi4-sd.img.sha256
```

Writing the image destroys the selected card. Resolve the exact removable
whole-disk device first and substitute it for `/dev/diskN` or `/dev/sdX`.
Never copy these commands with an unresolved placeholder.

On macOS:

```bash
diskutil list external physical
diskutil unmountDisk /dev/diskN
sudo dd if=image/cohesix-pi4-sd.img of=/dev/rdiskN bs=4m
sync
diskutil eject /dev/diskN
```

On Linux:

```bash
lsblk --bytes --output NAME,SIZE,TYPE,TRAN,MODEL
sudo umount /dev/sdX?*
sudo dd if=image/cohesix-pi4-sd.img of=/dev/sdX bs=4M conv=fsync status=progress
sudo eject /dev/sdX
```

This release image is packaging evidence. Flash/readback, a fresh boot, serial
capture, networking, and performance remain separate Pi 4 acceptance evidence.
""",
    encoding="utf-8",
)
PY
}

bundle_release() {
  local bundle_name="$1"
  local host_tools_dir="$2"
  local archive_mode="${3:-local}"
  local bundle_dir="${RELEASES_DIR}/${bundle_name}"
  local tarball="${RELEASES_DIR}/${bundle_name}.tar.gz"

  require_dir "$host_tools_dir"
  if [[ -e "$bundle_dir" || -e "$tarball" ]]; then
    if [[ "$FORCE" -eq 1 ]]; then
      rm -rf "$bundle_dir"
      rm -f "$tarball"
    else
      fail "Release path already exists: $bundle_dir or $tarball (use --force)"
    fi
  fi

  mkdir -p \
    "${bundle_dir}/bin" \
    "${bundle_dir}/configs" \
    "${bundle_dir}/configs/generated" \
    "${bundle_dir}/image" \
    "${bundle_dir}/out" \
    "${bundle_dir}/python" \
    "${bundle_dir}/qemu" \
    "${bundle_dir}/resources/keys" \
    "${bundle_dir}/resources/systemd" \
    "${bundle_dir}/scripts" \
    "${bundle_dir}/traces" \
    "${bundle_dir}/ui/swarmui" \
    "${bundle_dir}/docs"

  local selected_path
  while IFS= read -r selected_path; do
    cp -p "${host_tools_dir}/${selected_path#bin/}" "${bundle_dir}/${selected_path}"
  done < <(release_inventory_values host_tools)
  while IFS= read -r selected_path; do
    local source_path=""
    case "$selected_path" in
      image/gic-version.txt)
        continue
        ;;
      image/elfloader)
        source_path="${STAGING_DIR}/elfloader"
        ;;
      image/kernel.elf)
        source_path="${STAGING_DIR}/kernel.elf"
        ;;
      image/rootserver)
        source_path="${STAGING_DIR}/rootserver"
        ;;
      image/cohesix-system.cpio)
        source_path="${OUT_DIR}/cohesix-system.cpio"
        ;;
      image/manifest.json)
        source_path="${STAGING_DIR}/cohesix/manifest.json"
        ;;
      *)
        fail "No release source mapping for target image: ${selected_path}"
        ;;
    esac
    mkdir -p "$(dirname "${bundle_dir}/${selected_path}")"
    cp -p "$source_path" "${bundle_dir}/${selected_path}"
  done < <(release_inventory_values target_images)
  while IFS= read -r selected_path; do
    cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/${selected_path}"
  done < <(release_inventory_values generated_configs)
  while IFS= read -r selected_path; do
    cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/${selected_path}"
  done < <(release_inventory_values host_assets)

  while IFS= read -r selected_path; do
    local destination="$selected_path"
    if [[ "$selected_path" == "docs/QUICKSTART.md" ]]; then
      destination="QUICKSTART.md"
    fi
    mkdir -p "$(dirname "${bundle_dir}/${destination}")"
    cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/${destination}"
  done < <(release_inventory_values public_documents)

  while IFS= read -r selected_path; do
    mkdir -p "$(dirname "${bundle_dir}/${selected_path}")"
    cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/${selected_path}"
  done < <(release_inventory_values operator_scripts)

  while IFS= read -r selected_path; do
    local destination="python/cohesix-py/${selected_path#tools/cohesix-py/}"
    mkdir -p "$(dirname "${bundle_dir}/${destination}")"
    cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/${destination}"
  done < <(release_inventory_values python_artifacts)
  mkdir -p "${bundle_dir}/python/dist"
  cp -p "$PYTHON_RELEASE_WHEEL" \
    "${bundle_dir}/python/dist/$(basename "$PYTHON_RELEASE_WHEEL")"
  cp -p "$PYTHON_PACKAGE_MANIFEST" \
    "${bundle_dir}/python/m26e-python-package.json"

  while IFS= read -r selected_path; do
    local destination="cas/${selected_path#tests/fixtures/cas/}"
    mkdir -p "$(dirname "${bundle_dir}/${destination}")"
    cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/${destination}"
  done < <(release_inventory_values cas_fixtures)

  while IFS= read -r selected_path; do
    local destination="traces/${selected_path#tests/fixtures/traces/}"
    mkdir -p "$(dirname "${bundle_dir}/${destination}")"
    cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/${destination}"
  done < <(release_inventory_values trace_fixtures)

  while IFS= read -r selected_path; do
    local destination="tests/fixtures/transcripts/${selected_path#tests/fixtures/transcripts/}"
    mkdir -p "$(dirname "${bundle_dir}/${destination}")"
    cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/${destination}"
  done < <(release_inventory_values transcript_fixtures)

  while IFS= read -r selected_path; do
    local destination="ui/swarmui/${selected_path#apps/swarmui/frontend/}"
    mkdir -p "$(dirname "${bundle_dir}/${destination}")"
    cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/${destination}"
  done < <(release_inventory_values ui_assets)

  while IFS= read -r selected_path; do
    local destination="$selected_path"
    if [[ "$selected_path" == "releases/RELEASE_NOTES-${RELEASE_VERSION}.md" ]]; then
      destination="RELEASE_NOTES.md"
    fi
    mkdir -p "$(dirname "${bundle_dir}/${destination}")"
    cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/${destination}"
  done < <(release_inventory_values support_files)

  while IFS= read -r selected_path; do
    mkdir -p "$(dirname "${bundle_dir}/${selected_path}")"
    cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/${selected_path}"
  done < <(release_inventory_values versioned_migrations)

  GIC_CFG="${SEL4_BUILD_DIR}/kernel/gen_config/kernel/gen_config.h"
  GIC_VER="$("${ROOT_DIR}/scripts/lib/detect_gic_version.py" "$GIC_CFG")"
  [[ "$GIC_VER" == "3" ]] || fail "release runner requires GICv3"
  printf "%s\n" "$GIC_VER" > "${bundle_dir}/image/gic-version.txt"

  cat <<'EOF' > "${bundle_dir}/qemu/run.sh"
#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Launch Cohesix under QEMU from a release bundle.
# Copyright 2026 Lukas Bower
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
IMAGE_DIR="${ROOT_DIR}/image"

QEMU_BIN="${QEMU_BIN:-qemu-system-aarch64}"
HOST_OS="$(uname -s 2>/dev/null || true)"
QEMU_HOST_ADDR="${QEMU_HOST_ADDR:-127.0.0.1}"
TCP_PORT="${TCP_PORT:-31337}"
UDP_PORT="${UDP_PORT:-31338}"
SMOKE_PORT="${SMOKE_PORT:-31339}"
DEFAULT_QEMU_SMP_TOPO="4,cores=4,threads=1,sockets=1"
DEFAULT_QEMU_VIRT="off"
DEFAULT_QEMU_ACCEL=""
DEFAULT_QEMU_MACHINE_EXTRA=""
if [[ "$HOST_OS" == "Darwin" ]]; then
  DEFAULT_QEMU_ACCEL="hvf"
  DEFAULT_QEMU_VIRT="off"
  DEFAULT_QEMU_MACHINE_EXTRA="kernel-irqchip=off"
fi
QEMU_SMP_RAW="${COHESIX_QEMU_SMP:-${QEMU_SMP:-}}"
QEMU_SMP_TOPO_RAW="${COHESIX_QEMU_SMP_TOPO:-${QEMU_SMP_TOPO:-}}"
QEMU_VIRT_RAW="${COHESIX_QEMU_VIRT:-${QEMU_VIRT:-}}"
QEMU_MACHINE_EXTRA_RAW="${COHESIX_QEMU_MACHINE_EXTRA:-${QEMU_MACHINE_EXTRA:-}}"
if [[ -z "$QEMU_VIRT_RAW" ]]; then
  QEMU_VIRT_RAW="$DEFAULT_QEMU_VIRT"
fi
if [[ -z "$QEMU_MACHINE_EXTRA_RAW" && -n "$DEFAULT_QEMU_MACHINE_EXTRA" ]]; then
  QEMU_MACHINE_EXTRA_RAW="$DEFAULT_QEMU_MACHINE_EXTRA"
fi
if [[ -z "${COHESIX_QEMU_ACCEL:-}" && -z "${QEMU_ACCEL:-}" && -n "$DEFAULT_QEMU_ACCEL" ]]; then
  QEMU_ACCEL="$DEFAULT_QEMU_ACCEL"
fi
GIC_VER_FILE="${IMAGE_DIR}/gic-version.txt"
if [[ ! -f "${GIC_VER_FILE}" ]]; then
  echo "[qemu] missing compiler-selected GIC version: ${GIC_VER_FILE}" >&2
  exit 1
fi
GIC_VER="$(tr -d '\n' < "${GIC_VER_FILE}")"
if [[ "$GIC_VER" != "3" ]]; then
  echo "[qemu] release requires GICv3; selected GIC${GIC_VER}" >&2
  exit 1
fi

ELFLOADER="${IMAGE_DIR}/elfloader"
KERNEL="${IMAGE_DIR}/kernel.elf"
ROOTSERVER="${IMAGE_DIR}/rootserver"
CPIO="${IMAGE_DIR}/cohesix-system.cpio"

for path in "${ELFLOADER}" "${KERNEL}" "${ROOTSERVER}" "${CPIO}"; do
  if [[ ! -f "${path}" ]]; then
    echo "[qemu] missing: ${path}" >&2
    exit 1
  fi
done

detect_qemu_accel() {
  local accel="${COHESIX_QEMU_ACCEL:-${QEMU_ACCEL:-}}"
  if [[ -n "$accel" ]]; then
    echo "$accel"
    return
  fi

  local host_os
  host_os="$(uname -s 2>/dev/null || true)"
  case "$host_os" in
    Darwin)
      echo "hvf"
      ;;
    Linux)
      if [[ -c /dev/kvm && -r /dev/kvm && -w /dev/kvm ]]; then
        echo "kvm"
      else
        echo "tcg"
      fi
      ;;
    *)
      echo "tcg"
      ;;
  esac
}

has_kvm_device() {
  [[ -c /dev/kvm && -r /dev/kvm && -w /dev/kvm ]]
}

qemu_accel_supported() {
  local accel="$1"
  local help
  help="$("${QEMU_BIN}" -accel help 2>/dev/null || true)"
  if [[ -z "$help" ]]; then
    return 0
  fi
  echo "$help" | grep -Eiq "(^|[ ,])${accel}([ ,]|$)"
}

resolve_qemu_accel() {
  local accel
  accel="$(detect_qemu_accel)"
  if [[ -z "$accel" ]]; then
    accel="tcg"
  fi
  if [[ "$accel" == "kvm" && "$HOST_OS" == "Linux" ]]; then
    if ! has_kvm_device; then
      echo "[qemu] Requested QEMU accelerator 'kvm' but /dev/kvm is unavailable; falling back to tcg" >&2
      accel="tcg"
    fi
  fi
  if ! qemu_accel_supported "$accel"; then
    if [[ "$HOST_OS" == "Darwin" && "$accel" == "hvf" ]]; then
      echo "[qemu] canonical Darwin QEMU requires HVF, but ${QEMU_BIN} does not advertise it; set COHESIX_QEMU_ACCEL=tcg only for a claim-ineligible diagnostic run" >&2
      exit 1
    fi
    echo "[qemu] Requested QEMU accelerator '$accel' not supported by ${QEMU_BIN}; falling back to tcg" >&2
    accel="tcg"
  fi
  echo "$accel"
}

resolve_qemu_cpu_arg() {
  local accel="$1"
  local cpu_model="cortex-a57"
  if [[ "$HOST_OS" == "Linux" && "$accel" == "kvm" ]]; then
    cpu_model="host"
  fi
  if [[ "$accel" == "tcg" || ( "$HOST_OS" == "Linux" && "$accel" == "kvm" ) ]]; then
    cpu_model="${cpu_model},cntfrq=24000000"
  fi
  echo "$cpu_model"
}

resolve_qemu_smp_arg() {
  if [[ -n "$QEMU_SMP_TOPO_RAW" ]]; then
    echo "$QEMU_SMP_TOPO_RAW"
    return
  fi
  if [[ -n "$QEMU_SMP_RAW" ]]; then
    echo "$QEMU_SMP_RAW"
    return
  fi
  echo "$DEFAULT_QEMU_SMP_TOPO"
}

resolve_qemu_virt_arg() {
  if [[ -n "$QEMU_VIRT_RAW" ]]; then
    echo "$QEMU_VIRT_RAW"
    return
  fi
  echo "$DEFAULT_QEMU_VIRT"
}

validate_qemu_smp_arg() {
  local arg="$1"

  if [[ -z "$arg" ]]; then
    echo "[qemu] Invalid QEMU SMP setting: empty value" >&2
    exit 1
  fi

  if [[ "$arg" =~ ^[0-9]+$ ]]; then
    if [[ "$arg" -lt 1 ]]; then
      echo "[qemu] Invalid QEMU_SMP (must be >= 1): $arg" >&2
      exit 1
    fi
    return
  fi

  if [[ "$arg" == *" "* ]]; then
    echo "[qemu] Invalid QEMU SMP topology (contains spaces): $arg" >&2
    exit 1
  fi

  local token
  IFS=',' read -r -a tokens <<< "$arg"
  for token in "${tokens[@]}"; do
    if [[ "$token" =~ ^[0-9]+$ ]]; then
      if [[ "$token" -lt 1 ]]; then
        echo "[qemu] Invalid QEMU SMP topology token: $token" >&2
        exit 1
      fi
      continue
    fi
    if [[ "$token" =~ ^[A-Za-z][A-Za-z0-9_-]*=[0-9]+$ ]]; then
      local value="${token#*=}"
      if [[ "$value" -lt 1 ]]; then
        echo "[qemu] Invalid QEMU SMP topology token: $token" >&2
        exit 1
      fi
      continue
    fi
    echo "[qemu] Invalid QEMU SMP topology token: $token" >&2
    exit 1
  done
}

validate_qemu_virt_arg() {
  local arg="$1"

  if [[ "$arg" != "off" ]]; then
    echo "[qemu] selected release profile requires virtualization=off; got $arg" >&2
    exit 1
  fi
}

format_qemu_machine_arg() {
  local virt="$1"
  local machine="virt,gic-version=${GIC_VER},virtualization=${virt}"
  if [[ "$QEMU_MACHINE_EXTRA_RAW" == *"gic-version"* \
      || "$QEMU_MACHINE_EXTRA_RAW" == *"virtualization"* \
      || "$QEMU_MACHINE_EXTRA_RAW" == *"virt,"* \
      || "$QEMU_MACHINE_EXTRA_RAW" == *"machine="* \
      || "$QEMU_MACHINE_EXTRA_RAW" == *"type="* ]]; then
    echo "[qemu] machine extras must not override the profile-owned machine" >&2
    exit 1
  fi
  if [[ -n "$QEMU_MACHINE_EXTRA_RAW" ]]; then
    machine="${machine},${QEMU_MACHINE_EXTRA_RAW}"
  fi
  echo "$machine"
}

QEMU_ACCEL="$(resolve_qemu_accel)"
echo "[qemu] Using QEMU accel: ${QEMU_ACCEL}"
if [[ "$QEMU_ACCEL" == "tcg" ]]; then
  echo "[qemu] TCG is an explicit diagnostic envelope; this run is claim-ineligible"
elif [[ "$HOST_OS" == "Darwin" && "$QEMU_ACCEL" != "hvf" ]]; then
  echo "[qemu] non-HVF Darwin acceleration is outside the release envelope; this run is claim-ineligible"
fi
QEMU_SMP_ARG="$(resolve_qemu_smp_arg)"
validate_qemu_smp_arg "$QEMU_SMP_ARG"
echo "[qemu] Using QEMU SMP: ${QEMU_SMP_ARG}"
QEMU_VIRT_ARG="$(resolve_qemu_virt_arg)"
validate_qemu_virt_arg "$QEMU_VIRT_ARG"
QEMU_MACHINE_ARG="$(format_qemu_machine_arg "$QEMU_VIRT_ARG")"
echo "[qemu] Using QEMU machine: ${QEMU_MACHINE_ARG}"
QEMU_CPU_ARG="$(resolve_qemu_cpu_arg "$QEMU_ACCEL")"
echo "[qemu] Using QEMU CPU: ${QEMU_CPU_ARG}"

"${QEMU_BIN}" \
  -accel "${QEMU_ACCEL}" \
  -machine "${QEMU_MACHINE_ARG}" \
  -cpu "${QEMU_CPU_ARG}" \
  -m 1024 \
  -smp "${QEMU_SMP_ARG}" \
  -serial mon:stdio \
  -display none \
  -kernel "${ELFLOADER}" \
  -initrd "${CPIO}" \
  -device loader,file="${KERNEL}",addr=0x70000000,force-raw=on \
  -device loader,file="${ROOTSERVER}",addr=0x80000000,force-raw=on \
  -global virtio-mmio.force-legacy=off \
  -netdev "user,id=net0,hostfwd=tcp:${QEMU_HOST_ADDR}:${TCP_PORT}-:31337,hostfwd=udp:${QEMU_HOST_ADDR}:${UDP_PORT}-:31338,hostfwd=tcp:${QEMU_HOST_ADDR}:${SMOKE_PORT}-:31339" \
  -device "virtio-net-device,netdev=net0,mac=52:55:00:d1:55:01,bus=virtio-mmio-bus.0"
EOF
  chmod +x "${bundle_dir}/qemu/run.sh"
  chmod +x "${bundle_dir}/scripts/setup_environment.sh"
  BUNDLE_DIR="$bundle_dir" python3 - <<'PY'
import hashlib
import os
from pathlib import Path

bundle = Path(os.environ["BUNDLE_DIR"])
trace = bundle / "traces" / "trace_v0.trace"
digest = hashlib.sha256(trace.read_bytes()).hexdigest()
(trace.parent / "trace_v0.trace.sha256").write_text(digest + "\n", encoding="utf-8")
hive = bundle / "traces" / "trace_v0.hive.cbor"
hive_digest = hashlib.sha256(hive.read_bytes()).hexdigest()
(hive.parent / "trace_v0.hive.cbor.sha256").write_text(hive_digest + "\n", encoding="utf-8")
cas_fixture = bundle / "cas" / "max_chunks_v1.txt"
cas_fixture_digest = hashlib.sha256(cas_fixture.read_bytes()).hexdigest()
(cas_fixture.parent / "max_chunks_v1.txt.sha256").write_text(
    cas_fixture_digest + "\n", encoding="utf-8"
)
PY

  printf "%s\n" "${RELEASE_VERSION}" > "${bundle_dir}/VERSION.txt"

  BUNDLE_DIR="${bundle_dir}" python3 - <<'PY'
from pathlib import Path
import os

bundle = Path(os.environ["BUNDLE_DIR"])

readme = bundle / "README.md"
if readme.exists():
    text = readme.read_text(encoding="utf-8")
    text = text.replace(
        "## Status\n- [docs/BUILD_PLAN.md](docs/BUILD_PLAN.md) \n",
        "## Status\nSee `docs/QUICKSTART.md` for how to run this bundle.\n",
    )
    readme.write_text(text, encoding="utf-8")

    text = readme.read_text(encoding="utf-8")
    text = text.replace("docs/QUICKSTART.md", "QUICKSTART.md")
    readme.write_text(text, encoding="utf-8")

arch = bundle / "docs" / "ARCHITECTURE.md"
if arch.exists():
    text = arch.read_text(encoding="utf-8")
    text = text.replace(
        "UI clients or hardware/UEFI deployment details (UEFI boot is planned; see `docs/BUILD_PLAN.md`).",
        "UI clients or hardware/UEFI deployment details (UEFI boot is planned).",
    )
    text = text.replace("- `docs/BUILD_PLAN.md`\n", "")
    text = text.replace("- `docs/REPO_LAYOUT.md`\n", "")
    arch.write_text(text, encoding="utf-8")

interfaces = bundle / "docs" / "INTERFACES.md"
if interfaces.exists():
    text = interfaces.read_text(encoding="utf-8")
    text = text.replace(
        "and referenced from `ROLES_AND_SCHEDULING.md` and `BUILD_PLAN.md`",
        "and referenced from `ROLES_AND_SCHEDULING.md`",
    )
    interfaces.write_text(text, encoding="utf-8")

gpu_nodes = bundle / "docs" / "GPU_NODES.md"
if gpu_nodes.exists():
    text = gpu_nodes.read_text(encoding="utf-8")
    text = text.replace(
        "Future work (per `BUILD_PLAN.md` milestones):",
        "Future work includes",
    )
    gpu_nodes.write_text(text, encoding="utf-8")
PY

  write_bundle_manifest "$bundle_dir" expected_bundle_files

  case "$archive_mode" in
    local)
      COPYFILE_DISABLE=1 tar --no-xattrs -C "${RELEASES_DIR}" -czf "${tarball}" "${bundle_name}"
      ;;
    remote-linux)
      local archive_args=(
        archive-bundle
        --host "$LINUX_BUILDER_HOST"
        --user "$LINUX_BUILDER_USER"
        --remote-release-dir "$LINUX_BUILDER_RELEASE_DIR"
        --bundle-dir "$bundle_dir"
        --local-tarball "$tarball"
      )
      if [[ -n "$LINUX_BUILDER_KEY" ]]; then
        archive_args+=(--key "$LINUX_BUILDER_KEY")
      fi
      if [[ "$FORCE" -eq 1 ]]; then
        archive_args+=(--force)
      fi
      "${ROOT_DIR}/scripts/linux_host_tools_sync.sh" "${archive_args[@]}"
      ;;
    *)
      fail "unknown release archive mode: ${archive_mode}"
      ;;
  esac

  echo "Release bundle ready: ${bundle_dir}"
  echo "Tarball: ${tarball}"
}

bundle_pi4_release() {
  local bundle_name="$1"
  local bundle_dir="${RELEASES_DIR}/${bundle_name}"
  local tarball="${RELEASES_DIR}/${bundle_name}.tar.gz"

  if [[ -e "$bundle_dir" || -e "$tarball" ]]; then
    if [[ "$FORCE" -eq 1 ]]; then
      rm -rf "$bundle_dir"
      rm -f "$tarball"
    else
      fail "Release path already exists: $bundle_dir or $tarball (use --force)"
    fi
  fi

  mkdir -p "${bundle_dir}/docs" "${bundle_dir}/image"
  local selected_path
  while IFS= read -r selected_path; do
    local destination="$selected_path"
    if [[ "$selected_path" == "docs/QUICKSTART.md" ]]; then
      destination="QUICKSTART.md"
    fi
    mkdir -p "$(dirname "${bundle_dir}/${destination}")"
    cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/${destination}"
  done < <(release_inventory_values public_documents)

  while IFS= read -r selected_path; do
    case "$selected_path" in
      LICENSE.txt)
        cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/LICENSE.txt"
        ;;
      "releases/RELEASE_NOTES-${RELEASE_VERSION}.md")
        cp -p "${ROOT_DIR}/${selected_path}" "${bundle_dir}/RELEASE_NOTES.md"
        ;;
    esac
  done < <(release_inventory_values support_files)

  "${ROOT_DIR}/scripts/pi4_release_image.sh" \
    --stage-dir "$PI4_STAGE_DIR" \
    --output-image "${bundle_dir}/image/cohesix-pi4-sd.img" \
    --output-metadata "${bundle_dir}/image/cohesix-pi4-sd.json" \
    --output-sha256 "${bundle_dir}/image/cohesix-pi4-sd.img.sha256"
  printf '%s\n' "$RELEASE_VERSION" >"${bundle_dir}/VERSION.txt"
  rewrite_pi4_quickstart "$bundle_dir"
  write_bundle_manifest "$bundle_dir" expected_pi4_bundle_files

  COPYFILE_DISABLE=1 tar --no-xattrs \
    -C "$RELEASES_DIR" -czf "$tarball" "$bundle_name"
  echo "Pi 4 release bundle ready: ${bundle_dir}"
  echo "Tarball: ${tarball}"
}

if [[ "$VERIFY_WORKER_ACCEPTANCE" -eq 1 ]]; then
  [[ "$CHECK_MANIFEST" -eq 0 && "$LINUX_BUNDLE" -eq 0 && "$FORCE" -eq 0 ]] || fail \
    "--verify-worker-acceptance is verification-only and cannot be combined with bundle mutation options"
  verify_worker_acceptance
  exit 0
fi

require_file "$(release_inventory_path)"
if [[ -z "$RELEASE_VERSION" ]]; then
  RELEASE_VERSION="$(release_inventory_scalar version)"
fi
[[ -n "$PI4_STAGE_DIR" ]] || fail "--pi4-stage-dir is required"
if [[ "$CHECK_MANIFEST" -eq 1 && "$LINUX_BUNDLE" -eq 1 ]]; then
  fail "--check-manifest is read-only and cannot be combined with --linux"
fi
if [[ "$CHECK_MANIFEST" -eq 0 ]]; then
  [[ -n "$RELEASE_NAME" ]] || fail "--name is required when creating release bundles"
  [[ "$RELEASE_NAME" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || \
    fail "release name contains unsupported characters"
  local_status="$(git -C "$ROOT_DIR" status --porcelain=v1 --untracked-files=all)"
  [[ -z "$local_status" ]] || fail "release creation requires a clean source checkout"
fi
MACOS_BUNDLE_NAME="${RELEASE_NAME}-MacOS"
LINUX_BUNDLE_NAME="${RELEASE_NAME}-linux"
PI4_BUNDLE_NAME="${RELEASE_NAME}-Pi4"

require_dir "$OUT_DIR"
require_file "${STAGING_DIR}/elfloader"
require_file "${STAGING_DIR}/kernel.elf"
require_file "${STAGING_DIR}/rootserver"
require_file "${OUT_DIR}/cohesix-system.cpio"
require_file "${STAGING_DIR}/cohesix/manifest.json"
require_file "${ROOT_DIR}/docs/QUICKSTART.md"
require_file "${ROOT_DIR}/README.md"
require_file "${ROOT_DIR}/LICENSE.txt"
require_file "${ROOT_DIR}/releases/RELEASE_NOTES-${RELEASE_VERSION}.md"
require_file "${ROOT_DIR}/configs/root_task.toml"
require_file "${GENERATED_CONFIG_DIR}/coh_policy.toml"
require_file "${GENERATED_CONFIG_DIR}/coh_policy.toml.sha256"
require_file "${ROOT_DIR}/tests/fixtures/traces/trace_v0.trace"
require_file "${ROOT_DIR}/tests/fixtures/traces/trace_v0.hive.cbor"
require_file "${ROOT_DIR}/tests/fixtures/cas/max_chunks_v1.txt"
require_file "${ROOT_DIR}/scripts/setup_environment.sh"
require_dir "${ROOT_DIR}/apps/swarmui/frontend"
require_dir "${ROOT_DIR}/docs"
require_dir "${ROOT_DIR}/scripts/cohsh"

validate_release_sel4_profile
validate_release_inventory_inputs

if [[ "$CHECK_MANIFEST" -eq 1 ]]; then
  validate_host_tools_inputs "$DEFAULT_HOST_TOOLS_DIR" macos
  echo "[release] Exact compiler-generated release manifest and inputs: PASS"
  exit 0
fi

if [[ "$LINUX_ONLY" -ne 1 ]]; then
  validate_host_tools_inputs "$DEFAULT_HOST_TOOLS_DIR" macos
fi

if [[ "$LINUX_BUNDLE" -eq 1 ]]; then
  for required in \
    "$LINUX_BUILDER_HOST" \
    "$LINUX_BUILDER_USER" \
    "$LINUX_BUILDER_BUILD_DIR" \
    "$LINUX_BUILDER_RELEASE_DIR" \
    "$LINUX_BUILDER_CARGO" \
    "$LINUX_BUILDER_CARGO_HOME" \
    "$LINUX_BUILDER_MAX_GLIBC" \
    "$LINUX_HOST_TOOLS_DIR" \
    "$LINUX_HOST_TOOLS_MANIFEST"; do
    [[ -n "$required" ]] || \
      fail "--linux requires every documented remote builder and local output argument"
  done
  sync_args=(
    build-tools
    --host "$LINUX_BUILDER_HOST"
    --user "$LINUX_BUILDER_USER"
    --remote-build-dir "$LINUX_BUILDER_BUILD_DIR"
    --remote-cargo "$LINUX_BUILDER_CARGO"
    --remote-cargo-home "$LINUX_BUILDER_CARGO_HOME"
    --local-out "$LINUX_HOST_TOOLS_DIR"
    --manifest-out "$LINUX_HOST_TOOLS_MANIFEST"
    --max-glibc-version "$LINUX_BUILDER_MAX_GLIBC"
  )
  if [[ -n "$LINUX_BUILDER_KEY" ]]; then
    sync_args+=(--key "$LINUX_BUILDER_KEY")
  fi
  "${ROOT_DIR}/scripts/linux_host_tools_sync.sh" "${sync_args[@]}"
  validate_host_tools_inputs \
    "$LINUX_HOST_TOOLS_DIR" linux "$LINUX_HOST_TOOLS_MANIFEST"
fi

if [[ "$LINUX_ONLY" -ne 1 ]]; then
  bundle_release "${MACOS_BUNDLE_NAME}" "$DEFAULT_HOST_TOOLS_DIR"
  bundle_pi4_release "${PI4_BUNDLE_NAME}"
fi

if [[ "$LINUX_BUNDLE" -eq 1 ]]; then
  bundle_release \
    "${LINUX_BUNDLE_NAME}" "$LINUX_HOST_TOOLS_DIR" remote-linux
fi
