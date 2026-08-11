#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Assemble a Cohesix alpha release bundle under releases/ and emit a tarball.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
RELEASES_DIR="${ROOT_DIR}/releases"

RELEASE_NAME="${RELEASE_NAME:-Cohesix-0.1-Alpha}"
RELEASE_VERSION="${RELEASE_VERSION:-0.1.0-alpha1}"
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
LINUX_HOST_TARGET="${LINUX_HOST_TARGET:-aarch64-unknown-linux-gnu}"
LINUX_HOST_TOOLS_DIR="${LINUX_HOST_TOOLS_DIR:-}"
LINUX_SYNC_HOST="${LINUX_SYNC_HOST:-${COHESIX_SYNC_HOST:-}}"
LINUX_SYNC_USER="${LINUX_SYNC_USER:-${COHESIX_SYNC_USER:-ubuntu}}"
LINUX_SYNC_KEY="${LINUX_SYNC_KEY:-${COHESIX_SYNC_KEY:-}}"
LINUX_SYNC_REMOTE_DIR="${LINUX_SYNC_REMOTE_DIR:-${COHESIX_SYNC_REMOTE_DIR:-}}"
LINUX_SYNC_LOCAL_OUT="${LINUX_SYNC_LOCAL_OUT:-${COHESIX_SYNC_LOCAL_OUT:-}}"
HOST_TOOLS_PROFILE="${HOST_TOOLS_PROFILE:-release}"
SEL4_BUILD_DIR="${SEL4_BUILD_DIR:-${ROOT_DIR}/out/sel4/profile-v2/qemu-smp-production}"
IMPLEMENTATION_SURFACE_INVENTORY="${IMPLEMENTATION_SURFACE_INVENTORY:-${ROOT_DIR}/configs/generated/implementation_surface_inventory.json}"
PYTHON_WHEEL_DIR="${PYTHON_WHEEL_DIR:-${ROOT_DIR}/out/python-wheels}"
PYTHON_PACKAGE_MANIFEST="${PYTHON_PACKAGE_MANIFEST:-${ROOT_DIR}/out/python-compat/m26e-python-package.json}"

usage() {
  cat <<'USAGE'
Usage: scripts/release_bundle.sh [release options] [--check-manifest]
       scripts/release_bundle.sh --verify-worker-acceptance <six evidence paths>

Assembles a release bundle from out/cohesix into releases/<release-name> and
creates releases/<release-name>.tar.gz.

With --linux, also builds (or uses) Linux host tools and emits
releases/<release-name>-linux.tar.gz. Use --linux-only to emit only the Linux bundle.

Env overrides:
  RELEASE_NAME, RELEASE_VERSION
  SEL4_BUILD_DIR (defaults to $REPO/out/sel4/profile-v2/qemu-smp-production;
                  the selected tree must pass qemu_smp_production release validation)
  LINUX_HOST_TARGET (default: aarch64-unknown-linux-gnu)
  LINUX_HOST_TOOLS_DIR (prebuilt host tools dir; if empty, build from source)
  LINUX_SYNC_HOST (if set, run scripts/linux_host_tools_sync.sh before bundling)
  LINUX_SYNC_USER (default: ubuntu)
  LINUX_SYNC_KEY (required when LINUX_SYNC_HOST is set; optional SSH key path)
  LINUX_SYNC_REMOTE_DIR (optional remote work dir)
  LINUX_SYNC_LOCAL_OUT (optional local host-tools dir)
  COHESIX_SYNC_HOST/USER/KEY/REMOTE_DIR/LOCAL_OUT (aliases for LINUX_SYNC_*; use these to avoid hardcoded host/key names)
  HOST_TOOLS_PROFILE (default: release)
  IMPLEMENTATION_SURFACE_INVENTORY (defaults to the canonical generated inventory;
                                    intended only for non-mutating pre-regeneration validation)
  PYTHON_WHEEL_DIR (defaults to out/python-wheels; must contain one target-neutral wheel)
  PYTHON_PACKAGE_MANIFEST (defaults to out/python-compat/m26e-python-package.json)
  ALLOW_CROSS_LINUX_HOST_TOOLS=1 (override host-target guard for cross builds)

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
LINUX_HOST_TOOLS_DIR="${LINUX_HOST_TOOLS_DIR:-${OUT_DIR}/host-tools-linux}"
MACOS_BUNDLE_NAME="${RELEASE_NAME}-MacOS"
LINUX_BUNDLE_NAME="${RELEASE_NAME}-linux"

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
    "${ROOT_DIR}/configs/generated/cohesix_python_qemu_smp_production.json" \
    "${ROOT_DIR}/configs/generated/cohesix_python_pi4_production.json" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

manifest_path, wheel, qemu, pi4 = map(Path, sys.argv[1:])

def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
if manifest.get("schema") != "cohesix-python-package/v1":
    raise SystemExit("release Python package manifest schema is invalid")
wheel_record = manifest.get("wheel", {})
if wheel_record.get("filename") != wheel.name or wheel_record.get("sha256") != digest(wheel):
    raise SystemExit("release Python wheel differs from its package manifest")
if wheel.name != "cohesix-0.2.0a2-py3-none-any.whl":
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
  HOST_TOOLS_DIR="$DEFAULT_HOST_TOOLS_DIR" \
  STAGING_DIR="$STAGING_DIR" \
  OUT_DIR="$OUT_DIR" \
  python3 - <<'PY'
import json
import os
from pathlib import Path
import subprocess

inventory = json.loads(Path(os.environ["INVENTORY_PATH"]).read_text(encoding="utf-8"))
release = inventory["release"]
root = Path(os.environ["ROOT_DIR"])
host_root = Path(os.environ["HOST_TOOLS_DIR"])
staging = Path(os.environ["STAGING_DIR"])
out = Path(os.environ["OUT_DIR"])

expected_host = {Path(path).name for path in release["host_tools"]}
if not host_root.is_dir():
    raise SystemExit(f"host tools directory missing: {host_root}")
actual_host = {path.name for path in host_root.iterdir() if path.is_file()}
if actual_host != expected_host:
    raise SystemExit(
        "host tool set drift: "
        f"missing={sorted(expected_host - actual_host)} "
        f"unexpected={sorted(actual_host - expected_host)}"
    )

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
    if source is None or not source.is_file():
        raise SystemExit(f"target image source missing for {destination}: {source}")

for relative in release["forbidden_paths"]:
    if relative in release["host_tools"] or relative in release["target_images"]:
        raise SystemExit(f"forbidden release path is selected: {relative}")

for binary in sorted(host_root / name for name in expected_host):
    description = subprocess.run(
        ["file", "-b", str(binary)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.lower()
    if "arm64" not in description and "aarch64" not in description:
        raise SystemExit(f"wrong host-tool architecture for {binary}: {description.strip()}")
PY

  local gic_config="${SEL4_BUILD_DIR}/kernel/gen_config/kernel/gen_config.h"
  require_file "$gic_config"
  local gic_version
  gic_version="$("${ROOT_DIR}/scripts/lib/detect_gic_version.py" "$gic_config")"
  [[ "$gic_version" == "3" ]] || fail \
    "runtime release requires QEMU GICv3; selected kernel reports GIC${gic_version}"
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

purge_release_paths() {
  local mac_dir="${RELEASES_DIR}/${MACOS_BUNDLE_NAME}"
  local linux_dir="${RELEASES_DIR}/${LINUX_BUNDLE_NAME}"
  local mac_tar="${RELEASES_DIR}/${MACOS_BUNDLE_NAME}.tar.gz"
  local linux_tar="${RELEASES_DIR}/${LINUX_BUNDLE_NAME}.tar.gz"

  rm -rf "$mac_dir" "$linux_dir"
  rm -f "$mac_tar" "$linux_tar"
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

build_linux_host_tools() {
  local target="$1"
  local out_dir="$2"
  local profile="$3"

  command -v cargo >/dev/null 2>&1 || fail "cargo is required to build Linux host tools"
  command -v rustc >/dev/null 2>&1 || fail "rustc is required to build Linux host tools"

  local host_triple
  host_triple="$(rustc -vV | awk '/host:/ {print $2}')"
  if [[ "$host_triple" != "$target" && "${ALLOW_CROSS_LINUX_HOST_TOOLS:-0}" -ne 1 ]]; then
    fail "Host target ${host_triple} does not match ${target}; build on Linux ${target} or set ALLOW_CROSS_LINUX_HOST_TOOLS=1"
  fi

  local profile_args=()
  local profile_dir="$profile"
  case "$profile" in
    release)
      profile_args=(--release)
      profile_dir="release"
      ;;
    dev|debug)
      profile_dir="debug"
      ;;
    *)
      profile_args=(--profile "$profile")
      ;;
  esac

  local host_packages=(gpu-bridge-host cas-tool swarmui hive-gateway host-ticket-agent)
  local host_bins=(cohsh coh "${host_packages[@]}" host-sidecar-bridge)
  local build_args=(build)
  if (( ${#profile_args[@]} > 0 )); then
    build_args+=("${profile_args[@]}")
  fi
  build_args+=(--target "$target")
  for pkg in "${host_packages[@]}"; do
    build_args+=(-p "$pkg")
  done

  echo "[release] Building Linux host tools via: cargo ${build_args[*]}"
  cargo "${build_args[@]}"

  local sidecar_args=(build)
  if (( ${#profile_args[@]} > 0 )); then
    sidecar_args+=("${profile_args[@]}")
  fi
  sidecar_args+=(--target "$target" -p host-sidecar-bridge --features tcp)

  echo "[release] Building Linux host-sidecar-bridge with TCP support via: cargo ${sidecar_args[*]}"
  cargo "${sidecar_args[@]}"

  local cohsh_args=(build)
  if (( ${#profile_args[@]} > 0 )); then
    cohsh_args+=("${profile_args[@]}")
  fi
  cohsh_args+=(--target "$target" -p cohsh --features tcp)

  echo "[release] Building Linux cohsh via: cargo ${cohsh_args[*]}"
  cargo "${cohsh_args[@]}"

  local coh_args=(build)
  if (( ${#profile_args[@]} > 0 )); then
    coh_args+=("${profile_args[@]}")
  fi
  coh_args+=(--target "$target" -p coh --features "fuse,nvml")

  echo "[release] Building Linux coh via: cargo ${coh_args[*]}"
  cargo "${coh_args[@]}"

  local artifact_dir="target/$target/$profile_dir"
  [[ -d "$artifact_dir" ]] || fail "Cargo artefact directory not found: $artifact_dir"

  rm -rf "$out_dir"
  mkdir -p "$out_dir"
  for bin in "${host_bins[@]}"; do
    local src="$artifact_dir/$bin"
    [[ -f "$src" ]] || fail "Expected host tool not found: $src"
    install -m 0755 "$src" "$out_dir/$bin"
  done
}

bundle_release() {
  local bundle_name="$1"
  local host_tools_dir="$2"
  local tarball_name="${3:-$bundle_name}"
  local bundle_dir="${RELEASES_DIR}/${bundle_name}"
  local tarball="${RELEASES_DIR}/${tarball_name}.tar.gz"

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
  cp -p "${STAGING_DIR}/elfloader" "${bundle_dir}/image/elfloader"
  cp -p "${STAGING_DIR}/kernel.elf" "${bundle_dir}/image/kernel.elf"
  cp -p "${STAGING_DIR}/rootserver" "${bundle_dir}/image/rootserver"
  cp -p "${OUT_DIR}/cohesix-system.cpio" "${bundle_dir}/image/cohesix-system.cpio"
  cp -p "${STAGING_DIR}/cohesix/manifest.json" "${bundle_dir}/image/manifest.json"
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
DEFAULT_QEMU_VIRT="on"
DEFAULT_QEMU_ACCEL=""
DEFAULT_QEMU_MACHINE_EXTRA=""
if [[ "$HOST_OS" == "Darwin" ]]; then
  DEFAULT_QEMU_ACCEL="tcg"
  DEFAULT_QEMU_VIRT="on"
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
    echo "[qemu] Requested QEMU accelerator '$accel' not supported by ${QEMU_BIN}; falling back to tcg" >&2
    accel="tcg"
  fi
  echo "$accel"
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

  case "$arg" in
    on|off)
      return
      ;;
    *)
      echo "[qemu] Invalid QEMU virtualization setting (use on|off): $arg" >&2
      exit 1
      ;;
  esac
}

format_qemu_machine_arg() {
  local virt="$1"
  local machine="virt,gic-version=${GIC_VER}"
  if [[ "$HOST_OS" != "Darwin" ]] || [[ -n "$QEMU_VIRT_RAW" ]]; then
    machine="${machine},virtualization=${virt}"
  fi
  if [[ -n "$QEMU_MACHINE_EXTRA_RAW" ]]; then
    machine="${machine},${QEMU_MACHINE_EXTRA_RAW}"
  fi
  echo "$machine"
}

QEMU_ACCEL="$(resolve_qemu_accel)"
echo "[qemu] Using QEMU accel: ${QEMU_ACCEL}"
QEMU_SMP_ARG="$(resolve_qemu_smp_arg)"
validate_qemu_smp_arg "$QEMU_SMP_ARG"
echo "[qemu] Using QEMU SMP: ${QEMU_SMP_ARG}"
QEMU_VIRT_ARG="$(resolve_qemu_virt_arg)"
validate_qemu_virt_arg "$QEMU_VIRT_ARG"
QEMU_MACHINE_ARG="$(format_qemu_machine_arg "$QEMU_VIRT_ARG")"
echo "[qemu] Using QEMU machine: ${QEMU_MACHINE_ARG}"

"${QEMU_BIN}" \
  -accel "${QEMU_ACCEL}" \
  -machine "${QEMU_MACHINE_ARG}" \
  -cpu cortex-a57 \
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
  RELEASE_NAME="$bundle_name" python3 - <<'PY'
import hashlib
import os
from pathlib import Path

release = os.environ["RELEASE_NAME"]
trace = Path("releases") / release / "traces" / "trace_v0.trace"
digest = hashlib.sha256(trace.read_bytes()).hexdigest()
(trace.parent / "trace_v0.trace.sha256").write_text(digest + "\n", encoding="utf-8")
hive = Path("releases") / release / "traces" / "trace_v0.hive.cbor"
hive_digest = hashlib.sha256(hive.read_bytes()).hexdigest()
(hive.parent / "trace_v0.hive.cbor.sha256").write_text(hive_digest + "\n", encoding="utf-8")
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

  BUNDLE_DIR="${bundle_dir}" \
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
expected = set(release["expected_bundle_files"])
manifest_relative = "MANIFEST.sha256"
if manifest_relative not in expected:
    raise SystemExit("compiler release inventory omits MANIFEST.sha256")

actual_without_manifest = {
    path.relative_to(bundle).as_posix()
    for path in bundle.rglob("*")
    if path.is_file() and path.relative_to(bundle).as_posix() != manifest_relative
}
expected_without_manifest = expected - {manifest_relative}
if actual_without_manifest != expected_without_manifest:
    raise SystemExit(
        "release bundle file-set drift before manifest: "
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
        "release bundle exact file-set drift: "
        f"missing={sorted(expected - actual)} "
        f"unexpected={sorted(actual - expected)}"
    )

for forbidden in release["forbidden_paths"]:
    if forbidden in actual:
        raise SystemExit(f"forbidden release path present: {forbidden}")
PY

  tar -C "${RELEASES_DIR}" -czf "${tarball}" "${bundle_name}"

  echo "Release bundle ready: ${bundle_dir}"
  echo "Tarball: ${tarball}"
}

if [[ "$VERIFY_WORKER_ACCEPTANCE" -eq 1 ]]; then
  [[ "$CHECK_MANIFEST" -eq 0 && "$LINUX_BUNDLE" -eq 0 && "$FORCE" -eq 0 ]] || fail \
    "--verify-worker-acceptance is verification-only and cannot be combined with bundle mutation options"
  verify_worker_acceptance
  exit 0
fi

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
require_file "${ROOT_DIR}/scripts/setup_environment.sh"
require_dir "${ROOT_DIR}/apps/swarmui/frontend"
require_dir "${ROOT_DIR}/docs"
require_dir "${ROOT_DIR}/scripts/cohsh"

validate_release_sel4_profile
validate_release_inventory_inputs

if [[ "$CHECK_MANIFEST" -eq 1 ]]; then
  echo "[release] Exact compiler-generated release manifest and inputs: PASS"
  exit 0
fi

if [[ "$FORCE" -eq 1 ]]; then
  purge_release_paths
fi

if [[ "$LINUX_BUNDLE" -eq 1 ]]; then
  if [[ -n "$LINUX_SYNC_HOST" ]]; then
    if [[ -z "$LINUX_SYNC_KEY" ]]; then
      fail "LINUX_SYNC_KEY (or COHESIX_SYNC_KEY) is required when LINUX_SYNC_HOST is set"
    fi
    echo "[release] Syncing Linux host tools via scripts/linux_host_tools_sync.sh"
    sync_args=(--host "$LINUX_SYNC_HOST" --no-bundle)
    if [[ -n "$LINUX_SYNC_USER" ]]; then
      sync_args+=(--user "$LINUX_SYNC_USER")
    fi
    if [[ -n "$LINUX_SYNC_KEY" ]]; then
      sync_args+=(--key "$LINUX_SYNC_KEY")
    fi
    if [[ -n "$LINUX_SYNC_REMOTE_DIR" ]]; then
      sync_args+=(--remote-dir "$LINUX_SYNC_REMOTE_DIR")
    fi
    if [[ -n "$LINUX_SYNC_LOCAL_OUT" ]]; then
      sync_args+=(--local-out "$LINUX_SYNC_LOCAL_OUT")
      LINUX_HOST_TOOLS_DIR="$LINUX_SYNC_LOCAL_OUT"
    fi
    "${ROOT_DIR}/scripts/linux_host_tools_sync.sh" "${sync_args[@]}"
  fi
  if [[ ! -d "$LINUX_HOST_TOOLS_DIR" || -z "$(ls -A "$LINUX_HOST_TOOLS_DIR" 2>/dev/null)" ]]; then
    build_linux_host_tools "$LINUX_HOST_TARGET" "$LINUX_HOST_TOOLS_DIR" "$HOST_TOOLS_PROFILE"
  fi
fi

if [[ "$LINUX_ONLY" -ne 1 ]]; then
  bundle_release "${MACOS_BUNDLE_NAME}" "$DEFAULT_HOST_TOOLS_DIR"
fi

if [[ "$LINUX_BUNDLE" -eq 1 ]]; then
  bundle_release "${LINUX_BUNDLE_NAME}" "$LINUX_HOST_TOOLS_DIR"
fi
