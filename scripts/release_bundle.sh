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

usage() {
  cat <<'USAGE'
Usage: scripts/release_bundle.sh [--name <release-name>] [--version <version>] [--force]

Assembles a release bundle from out/cohesix into releases/<release-name> and
creates releases/<release-name>.tar.gz.
Also refreshes the legacy flat layout under releases/.

Env overrides:
  RELEASE_NAME, RELEASE_VERSION
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
BUNDLE_DIR="${RELEASES_DIR}/${RELEASE_NAME}"
TARBALL="${RELEASES_DIR}/${RELEASE_NAME}.tar.gz"

require_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    echo "Missing required file: $path" >&2
    exit 1
  fi
}

require_dir() {
  local path="$1"
  if [[ ! -d "$path" ]]; then
    echo "Missing required directory: $path" >&2
    exit 1
  fi
}

require_dir "$OUT_DIR"
require_dir "${OUT_DIR}/host-tools"
require_file "${STAGING_DIR}/elfloader"
require_file "${STAGING_DIR}/kernel.elf"
require_file "${STAGING_DIR}/rootserver"
require_file "${OUT_DIR}/cohesix-system.cpio"
require_file "${STAGING_DIR}/cohesix/manifest.json"
require_file "${ROOT_DIR}/docs/QUICKSTART.md"
require_file "${ROOT_DIR}/README.md"
require_file "${ROOT_DIR}/LICENSE.txt"
require_file "${ROOT_DIR}/tests/fixtures/traces/trace_v0.trace"
require_dir "${ROOT_DIR}/apps/swarmui/frontend"
require_dir "${ROOT_DIR}/docs"
require_dir "${ROOT_DIR}/scripts/cohsh"

if [[ -e "$BUNDLE_DIR" || -e "$TARBALL" ]]; then
  if [[ "$FORCE" -eq 1 ]]; then
    rm -rf "$BUNDLE_DIR"
    rm -f "$TARBALL"
  else
    echo "Release path already exists: $BUNDLE_DIR or $TARBALL" >&2
    echo "Use --force to overwrite." >&2
    exit 1
  fi
fi

mkdir -p \
  "${BUNDLE_DIR}/bin" \
  "${BUNDLE_DIR}/image" \
  "${BUNDLE_DIR}/qemu" \
  "${BUNDLE_DIR}/scripts" \
  "${BUNDLE_DIR}/traces" \
  "${BUNDLE_DIR}/ui/swarmui" \
  "${BUNDLE_DIR}/docs"

cp -p "${OUT_DIR}/host-tools/"* "${BUNDLE_DIR}/bin/"
cp -p "${STAGING_DIR}/elfloader" "${BUNDLE_DIR}/image/elfloader"
cp -p "${STAGING_DIR}/kernel.elf" "${BUNDLE_DIR}/image/kernel.elf"
cp -p "${STAGING_DIR}/rootserver" "${BUNDLE_DIR}/image/rootserver"
cp -p "${OUT_DIR}/cohesix-system.cpio" "${BUNDLE_DIR}/image/cohesix-system.cpio"
cp -p "${STAGING_DIR}/cohesix/manifest.json" "${BUNDLE_DIR}/image/manifest.json"

if [[ -x "${ROOT_DIR}/scripts/lib/detect_gic_version.py" ]]; then
  GIC_CFG="${HOME}/seL4/build/kernel/gen_config/kernel/gen_config.h"
  if [[ -f "$GIC_CFG" ]]; then
    GIC_VER="$("${ROOT_DIR}/scripts/lib/detect_gic_version.py" "$GIC_CFG" || true)"
    if [[ -n "$GIC_VER" ]]; then
      printf "%s\n" "$GIC_VER" > "${BUNDLE_DIR}/image/gic-version.txt"
    fi
  fi
fi

cat <<'EOF' > "${BUNDLE_DIR}/qemu/run.sh"
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
chmod +x "${BUNDLE_DIR}/qemu/run.sh"

cp -R "${ROOT_DIR}/scripts/cohsh" "${BUNDLE_DIR}/scripts/"

cp -p "${ROOT_DIR}/tests/fixtures/traces/trace_v0.trace" "${BUNDLE_DIR}/traces/trace_v0.trace"
RELEASE_NAME="$RELEASE_NAME" python3 - <<'PY'
import hashlib
import os
from pathlib import Path

release = os.environ["RELEASE_NAME"]
trace = Path("releases") / release / "traces" / "trace_v0.trace"
digest = hashlib.sha256(trace.read_bytes()).hexdigest()
(trace.parent / "trace_v0.trace.sha256").write_text(digest + "\n", encoding="utf-8")
PY

cp -R "${ROOT_DIR}/apps/swarmui/frontend/." "${BUNDLE_DIR}/ui/swarmui/"

DOCS_LIST=(
  "ARCHITECTURE.md"
  "BOOT_REFERENCE.md"
  "GPU_NODES.md"
  "HOST_TOOLS.md"
  "INTERFACES.md"
  "NETWORK_CONFIG.md"
  "QUICKSTART.md"
  "ROLES_AND_SCHEDULING.md"
  "SECURE9P.md"
  "SECURITY.md"
  "USERLAND_AND_CLI.md"
  "USE_CASES.md"
  "WORKER_TICKETS.md"
)
for doc in "${DOCS_LIST[@]}"; do
  require_file "${ROOT_DIR}/docs/${doc}"
  cp -p "${ROOT_DIR}/docs/${doc}" "${BUNDLE_DIR}/docs/"
done

cp -p "${ROOT_DIR}/README.md" "${BUNDLE_DIR}/README.md"
cp -p "${ROOT_DIR}/LICENSE.txt" "${BUNDLE_DIR}/LICENSE.txt"
printf "%s\n" "${RELEASE_VERSION}" > "${BUNDLE_DIR}/VERSION.txt"

BUNDLE_DIR="${BUNDLE_DIR}" python3 - <<'PY'
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

if [[ "${BUNDLE_DIR}" == "${RELEASES_DIR}" ]]; then
  echo "Refusing to refresh flat layout: bundle dir equals releases root." >&2
  exit 1
fi

FLAT_DIRS=(bin docs image qemu scripts traces ui)
FLAT_FILES=(LICENSE.txt VERSION.txt README.md)

for dir in "${FLAT_DIRS[@]}"; do
  rm -rf "${RELEASES_DIR}/${dir}"
  cp -R "${BUNDLE_DIR}/${dir}" "${RELEASES_DIR}/${dir}"
done

for file in "${FLAT_FILES[@]}"; do
  rm -f "${RELEASES_DIR}/${file}"
  cp -p "${BUNDLE_DIR}/${file}" "${RELEASES_DIR}/${file}"
done

tar -C "${RELEASES_DIR}" -czf "${TARBALL}" "${RELEASE_NAME}"

echo "Release bundle ready: ${BUNDLE_DIR}"
echo "Tarball: ${TARBALL}"
