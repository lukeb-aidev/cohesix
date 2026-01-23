#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Assemble a Cohesix alpha release bundle under releases/ and emit a tarball.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
RELEASES_DIR="${ROOT_DIR}/releases"

RELEASE_NAME="${RELEASE_NAME:-Cohesix-0.1-Alpha}"
RELEASE_VERSION="${RELEASE_VERSION:-0.1.0-alpha1}"
FORCE=0
LINUX_BUNDLE=0
LINUX_ONLY=0
LINUX_HOST_TARGET="${LINUX_HOST_TARGET:-aarch64-unknown-linux-gnu}"
LINUX_HOST_TOOLS_DIR="${LINUX_HOST_TOOLS_DIR:-}"
HOST_TOOLS_PROFILE="${HOST_TOOLS_PROFILE:-release}"

usage() {
  cat <<'USAGE'
Usage: scripts/release_bundle.sh [--name <release-name>] [--version <version>] [--force] [--linux] [--linux-only]

Assembles a release bundle from out/cohesix into releases/<release-name> and
creates releases/<release-name>.tar.gz.

With --linux, also builds (or uses) Linux host tools and emits
releases/<release-name>-linux.tar.gz. Use --linux-only to emit only the Linux bundle.

Env overrides:
  RELEASE_NAME, RELEASE_VERSION
  LINUX_HOST_TARGET (default: aarch64-unknown-linux-gnu)
  LINUX_HOST_TOOLS_DIR (prebuilt host tools dir; if empty, build from source)
  HOST_TOOLS_PROFILE (default: release)
  ALLOW_CROSS_LINUX_HOST_TOOLS=1 (override host-target guard for cross builds)
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

OUT_DIR="${ROOT_DIR}/out/cohesix"
STAGING_DIR="${OUT_DIR}/staging"
DEFAULT_HOST_TOOLS_DIR="${OUT_DIR}/host-tools"
LINUX_HOST_TOOLS_DIR="${LINUX_HOST_TOOLS_DIR:-${OUT_DIR}/host-tools-linux}"

fail() {
  echo "$1" >&2
  exit 1
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

  local host_packages=(gpu-bridge-host cas-tool swarmui)
  local host_bins=(cohsh "${host_packages[@]}" host-sidecar-bridge)
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
  if ! compgen -G "${host_tools_dir}/*" >/dev/null; then
    fail "Host tools directory is empty: $host_tools_dir"
  fi

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
    "${bundle_dir}/image" \
    "${bundle_dir}/out" \
    "${bundle_dir}/qemu" \
    "${bundle_dir}/resources/fixtures" \
    "${bundle_dir}/scripts" \
    "${bundle_dir}/traces" \
    "${bundle_dir}/ui/swarmui" \
    "${bundle_dir}/docs"

  cp -p "${host_tools_dir}/"* "${bundle_dir}/bin/"
  cp -p "${STAGING_DIR}/elfloader" "${bundle_dir}/image/elfloader"
  cp -p "${STAGING_DIR}/kernel.elf" "${bundle_dir}/image/kernel.elf"
  cp -p "${STAGING_DIR}/rootserver" "${bundle_dir}/image/rootserver"
  cp -p "${OUT_DIR}/cohesix-system.cpio" "${bundle_dir}/image/cohesix-system.cpio"
  cp -p "${STAGING_DIR}/cohesix/manifest.json" "${bundle_dir}/image/manifest.json"
  cp -p "${ROOT_DIR}/configs/root_task.toml" "${bundle_dir}/configs/root_task.toml"
  cp -p "${ROOT_DIR}/resources/fixtures/cas_signing_key.hex" "${bundle_dir}/resources/fixtures/cas_signing_key.hex"
  cp -p "${ROOT_DIR}/out/cas_manifest_template.json" "${bundle_dir}/out/cas_manifest_template.json"
  cp -p "${ROOT_DIR}/out/cohsh_policy.toml" "${bundle_dir}/out/cohsh_policy.toml"
  cp -p "${ROOT_DIR}/out/cohsh_policy.toml.sha256" "${bundle_dir}/out/cohsh_policy.toml.sha256"

  if [[ -x "${ROOT_DIR}/scripts/lib/detect_gic_version.py" ]]; then
    GIC_CFG="${HOME}/seL4/build/kernel/gen_config/kernel/gen_config.h"
    if [[ -f "$GIC_CFG" ]]; then
      GIC_VER="$("${ROOT_DIR}/scripts/lib/detect_gic_version.py" "$GIC_CFG" || true)"
      if [[ -n "$GIC_VER" ]]; then
        printf "%s\n" "$GIC_VER" > "${bundle_dir}/image/gic-version.txt"
      fi
    fi
  fi

  cat <<'EOF' > "${bundle_dir}/qemu/run.sh"
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
IMAGE_DIR="${ROOT_DIR}/image"

QEMU_BIN="${QEMU_BIN:-qemu-system-aarch64}"
TCP_PORT="${TCP_PORT:-31337}"
UDP_PORT="${UDP_PORT:-31338}"
SMOKE_PORT="${SMOKE_PORT:-31339}"
GIC_VER_FILE="${IMAGE_DIR}/gic-version.txt"
GIC_VER="2"
if [[ -f "${GIC_VER_FILE}" ]]; then
  GIC_VER="$(tr -d '\n' < "${GIC_VER_FILE}")"
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

"${QEMU_BIN}" \
  -machine "virt,gic-version=${GIC_VER}" \
  -cpu cortex-a57 \
  -m 1024 \
  -smp 1 \
  -serial mon:stdio \
  -display none \
  -kernel "${ELFLOADER}" \
  -initrd "${CPIO}" \
  -device loader,file="${KERNEL}",addr=0x70000000,force-raw=on \
  -device loader,file="${ROOTSERVER}",addr=0x80000000,force-raw=on \
  -global virtio-mmio.force-legacy=off \
  -netdev "user,id=net0,hostfwd=tcp:127.0.0.1:${TCP_PORT}-:31337,hostfwd=udp:127.0.0.1:${UDP_PORT}-:31338,hostfwd=tcp:127.0.0.1:${SMOKE_PORT}-:31339" \
  -device "virtio-net-device,netdev=net0,mac=52:55:00:d1:55:01,bus=virtio-mmio-bus.0"
EOF
  chmod +x "${bundle_dir}/qemu/run.sh"

  cp -R "${ROOT_DIR}/scripts/cohsh" "${bundle_dir}/scripts/"
  cp -p "${ROOT_DIR}/scripts/setup_environment.sh" "${bundle_dir}/scripts/setup_environment.sh"
  chmod +x "${bundle_dir}/scripts/setup_environment.sh"

  cp -p "${ROOT_DIR}/tests/fixtures/traces/trace_v0.trace" "${bundle_dir}/traces/trace_v0.trace"
  cp -p "${ROOT_DIR}/tests/fixtures/traces/trace_v0.hive.cbor" "${bundle_dir}/traces/trace_v0.hive.cbor"
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

  cp -R "${ROOT_DIR}/apps/swarmui/frontend/." "${bundle_dir}/ui/swarmui/"

  DOCS_LIST=(
    "ARCHITECTURE.md"
    "GPU_NODES.md"
    "HOST_TOOLS.md"
    "INTERFACES.md"
    "NETWORK_CONFIG.md"
    "ROLES_AND_SCHEDULING.md"
    "SECURE9P.md"
    "SECURITY.md"
    "USERLAND_AND_CLI.md"
    "USE_CASES.md"
    "WORKER_TICKETS.md"
  )
  for doc in "${DOCS_LIST[@]}"; do
    require_file "${ROOT_DIR}/docs/${doc}"
    cp -p "${ROOT_DIR}/docs/${doc}" "${bundle_dir}/docs/"
  done

  cp -p "${ROOT_DIR}/docs/QUICKSTART.md" "${bundle_dir}/QUICKSTART.md"
  cp -p "${ROOT_DIR}/README.md" "${bundle_dir}/README.md"
  cp -p "${ROOT_DIR}/releases/RELEASE_NOTES-${RELEASE_VERSION}.md" "${bundle_dir}/RELEASE_NOTES.md"
  cp -p "${ROOT_DIR}/LICENSE.txt" "${bundle_dir}/LICENSE.txt"
  printf "%s\n" "${RELEASE_VERSION}" > "${bundle_dir}/VERSION.txt"

  BUNDLE_DIR="${bundle_dir}" python3 - <<'PY'
from pathlib import Path
import os

bundle = Path(os.environ["BUNDLE_DIR"])

readme = bundle / "README.md"
if readme.exists():
    text = readme.read_text(encoding="utf-8")
    text = text.replace(
        "apps/swarmui/frontend/assets/icons/cohesix-header.svg",
        "ui/swarmui/assets/icons/cohesix-header.svg",
    )
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

  tar -C "${RELEASES_DIR}" -czf "${tarball}" "${bundle_name}"

  echo "Release bundle ready: ${bundle_dir}"
  echo "Tarball: ${tarball}"
}

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
require_file "${ROOT_DIR}/resources/fixtures/cas_signing_key.hex"
require_file "${ROOT_DIR}/tests/fixtures/traces/trace_v0.trace"
require_file "${ROOT_DIR}/tests/fixtures/traces/trace_v0.hive.cbor"
require_file "${ROOT_DIR}/scripts/setup_environment.sh"
require_dir "${ROOT_DIR}/apps/swarmui/frontend"
require_dir "${ROOT_DIR}/docs"
require_dir "${ROOT_DIR}/scripts/cohsh"

if [[ "$LINUX_BUNDLE" -eq 1 ]]; then
  if [[ ! -d "$LINUX_HOST_TOOLS_DIR" || -z "$(ls -A "$LINUX_HOST_TOOLS_DIR" 2>/dev/null)" ]]; then
    build_linux_host_tools "$LINUX_HOST_TARGET" "$LINUX_HOST_TOOLS_DIR" "$HOST_TOOLS_PROFILE"
  fi
fi

if [[ "$LINUX_ONLY" -ne 1 ]]; then
  bundle_release "${RELEASE_NAME}-MacOS" "$DEFAULT_HOST_TOOLS_DIR"
fi

if [[ "$LINUX_BUNDLE" -eq 1 ]]; then
  bundle_release "${RELEASE_NAME}-linux" "$LINUX_HOST_TOOLS_DIR"
fi
