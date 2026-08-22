#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build and stage (optionally flash) a Raspberry Pi 4 U-Boot + seL4 Cohesix SD payload.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

MANIFEST_PATH="${ROOT_DIR}/configs/root_task_pi4_uboot_aarch64.toml"
CANONICAL_MANIFEST_PATH="${ROOT_DIR}/configs/root_task.toml"
DEFAULT_REPO_SEL4_BUILD_DIR="${ROOT_DIR}/seL4/build_UBOOT"
SEL4_BUILD_DIR="${DEFAULT_REPO_SEL4_BUILD_DIR}"
SEL4_VENV_DIR="${ROOT_DIR}/.venv"
PI4_SEL4_PROFILE="pi4_diagnostic"
U_BOOT_BIN="${ROOT_DIR}/third_party/u-boot/u-boot.bin"
GENERATED_CONFIG_DIR="${ROOT_DIR}/configs/generated"
FIRMWARE_DIR="${ROOT_DIR}/third_party/raspberry-pi-firmware/v1.50"
PI4_WIFI_FIRMWARE_DIR=""
STAGE_DIR="${ROOT_DIR}/out/pi4-sd"
SEL4_UPSTREAM_IMAGE_NAME="sel4test-driver-image-arm-bcm2711"
COHESIX_IMAGE_NAME="cohesix-image-arm-bcm2711"
COHESIX_LOGO_SOURCE="${ROOT_DIR}/docs/COHESIX_LOGO_SQ.png"
COHESIX_LOGO_STAGE_NAME="cohesix-logo.bmp"
BOOTSTD_LOGO_STAGE_NAME="boot.bmp"
BRCMFMAC_CMDLINE_STAGE_NAME="brcmfmac-dyndbg.cmdline"
BRCMFMAC_DYNAMIC_DEBUG_STAGE_NAME="brcmfmac-dyndbg.sh"
DRIVER_RUNTIME_CPIO_STAGE_NAME="cohesix-driver-runtimes.cpio.uimg"
PI4_IMAGE_IDENTITY_STAGE_NAME="pi4-image-identity.json"
SEL4_IMAGE_PROVENANCE_SUFFIX=".cohesix-provenance.json"
DRIVER_RUNTIME_EMBED_DIR="${ROOT_DIR}/out/pi4-driver-runtime-embed"
DRIVER_RUNTIME_EMBED_CPIO_NAME="cohesix-driver-runtimes.cpio"
ROOT_TASK_STRIP_DIR="${ROOT_DIR}/out/pi4-root-task-stripped"
PI4_ASSEMBLY_DIR="${ROOT_DIR}/out/pi4-image-assembly"
FLASH_DISK=""
INITIALIZE_DISK=0
DISK_LABEL="COHESIX"
ROOT_TASK_FEATURES="release-pi4,bootstrap-trace"
SKIP_BUILD=0
CLEAN_BUILD=0
PI4_TOTAL_MEM_MB=2048
PI4_UBOOT_IMAGE_START_ADDR=0x10000000
RESTORE_CANONICAL_CODEGEN=0
PRESERVED_POLICY_TEMP=""
POLICY_RECOVERY_FILE=""
POLICY_RECOVERY_CONSUMED_FILE=""
FLASH_MEDIA_MUTATION_STARTED=0
FLASH_CAFFEINATE_PID=""
PI4_DTB_PADDED_SIZE=$((128 * 1024))
U_BOOT_CROSS_COMPILE="aarch64-linux-gnu-"
U_BOOT_MENU_INPUT="usb"
U_BOOT_MENU_INPUT_SOURCE="default"
EXACT_GIT_COMMIT=""
EXACT_GIT_SHORT=""
EXACT_BUILD_TIMESTAMP=""
BUILD_REPOSITORY_STATE_DIGEST=""
EXACT_BUILD_ID=""
CANONICAL_SEL4_STATE_DIGEST=""
EXACT_PI4_IMAGE=""
EXACT_ROOT_ELF=""
EXACT_ROOT_CPIO=""
EXACT_CANONICAL_PROFILE_STAMP=""
EXACT_COMPOSITION_RECORD=""
EXACT_COMPOSITION_CACHE=""
EXACT_COMPOSITION_TIMER_HEADER=""

usage() {
    cat <<'USAGE'
Usage: scripts/pi4-image-build.sh [options]

Builds and stages a Pi 4 SD payload with:
  - Raspberry Pi firmware files (start4.elf, fixup4.dat, DTB + overlays)
  - U-Boot (u-boot.bin)
  - seL4 image (upstream output copied as cohesix-image-arm-bcm2711)
  - Embedded Pi 4 driver-runtime CPIO used by physical driver-task boots
  - Cohesix autoboot script (boot.scr.uimg)
  - Optional Cohesix HDMI logo (cohesix-logo.bmp for U-Boot video)
  - Linux brcmfmac dynamic-debug helpers for known-good Wi-Fi trace capture

By default this script only builds/stages files under out/pi4-sd.
To refresh an existing canonical Cohesix SD card, pass
--flash-disk /dev/diskN explicitly. Whole-disk initialization additionally
requires --initialize-disk.

Options:
  --manifest <path>         Manifest input for root-task build:
                            TOML (coh-rtc source) or resolved JSON
                            (default: configs/root_task_pi4_uboot_aarch64.toml)
  --sel4-build-dir <dir>    Canonical immutable seL4 Pi4 build directory
                            (must resolve to seL4/build_UBOOT)
  --venv <dir>              Python venv containing build tooling (default: <repo>/.venv)
  --u-boot-bin <path>       U-Boot binary (default: third_party/u-boot/u-boot.bin)
  --firmware-dir <dir>      Pi firmware directory (default: third_party/raspberry-pi-firmware/v1.50)
  --stage-dir <dir>         Output staging directory (default: out/pi4-sd)
  --image-name <name>       Staged/boot image filename on FAT partition
                            (default: cohesix-image-arm-bcm2711)
  --root-task-features <f>  Comma-separated root-task feature list
                            (default: release-pi4,bootstrap-trace)
  --uboot-menu-input <m>    U-Boot setup menu input mode: usb or serial
                            (default: usb; serial is an explicit lab opt-out)
  --clean                   Clean and rebuild root-task and Pi 4 U-Boot outputs;
                            never rebuild or mutate seL4/build_UBOOT
  --skip-build              Reuse the provenance-bound exact-image assembly
  --flash-disk <device>     Refresh an existing canonical COHESIX card
                            (example: /dev/disk16)
  --initialize-disk         Explicitly create the one-partition MBR/FAT32
                            layout before flashing; never selected implicitly
  --policy-recovery-file <path>
                            Explicit private cohesix.env copy retained by a
                            prior interrupted flash
  --disk-label <name>       FAT32 label when flashing (default: COHESIX)
  -h, --help                Show this help

Environment:
  USB is always staged as Cohesix-owned cold boot. U-Boot xHCI handoff export is disabled.
  COHESIX_AARCH64_BINUTILS_PREFIX may select one absolute complete binutils
  family prefix (default: /opt/homebrew/bin/aarch64-linux-gnu-).
  seL4/build_UBOOT is the sole immutable pi4_diagnostic kernel/elfloader input.
  Cohesix rootserver composition publishes only replaceable outputs under out/.
USAGE
}

log() {
    echo "[pi4-image] $*"
}

fail() {
    echo "[pi4-image] error: $*" >&2
    exit 1
}

require_file() {
    local path="$1"
    [[ -f "$path" ]] || fail "required file missing: ${path}"
}

require_dir() {
    local path="$1"
    [[ -d "$path" ]] || fail "required directory missing: ${path}"
}

repository_state_digest() {
    python3 - "$ROOT_DIR" <<'PY'
import hashlib
import os
import stat
import subprocess
import sys

root = sys.argv[1]


def git(*args: str) -> bytes:
    return subprocess.check_output(("git", "-C", root, *args))


digest = hashlib.sha256()
digest.update(b"cohesix-exact-repository-state/v2\0")
for label, payload in (
    (b"head", git("rev-parse", "--verify", "HEAD")),
    (b"index", git("diff", "--binary", "--cached", "HEAD", "--")),
    (b"worktree", git("diff", "--binary", "HEAD", "--")),
):
    digest.update(label + b"\0")
    digest.update(len(payload).to_bytes(8, "big"))
    digest.update(payload)

untracked = sorted(
    entry
    for entry in git("ls-files", "--others", "--exclude-standard", "-z").split(b"\0")
    if entry
)
for encoded_path in untracked:
    path = os.path.join(root, os.fsdecode(encoded_path))
    observed = os.lstat(path)
    if not stat.S_ISREG(observed.st_mode):
        raise SystemExit(f"untracked repository entry is not a regular file: {path}")
    digest.update(b"untracked\0" + len(encoded_path).to_bytes(8, "big"))
    digest.update(encoded_path)
    with open(path, "rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
print(digest.hexdigest())
PY
}

verify_only_codegen_repository_changes() {
    python3 - "$ROOT_DIR" <<'PY'
import os
import subprocess
import sys

root = sys.argv[1]
allowed_exact = {
    "apps/coh/src/generated/policy.rs",
    "apps/cohsh/src/generated/client.rs",
    "apps/cohsh/src/generated/policy.rs",
    "apps/swarmui/src/generated.rs",
    "configs/generated/cas_manifest_template.json",
    "configs/generated/coh_policy.toml",
    "configs/generated/coh_policy.toml.sha256",
    "configs/generated/cohsh_policy.toml",
    "configs/generated/cohsh_policy.toml.sha256",
    "configs/generated/host_integration_dependency.json",
    "configs/generated/root_task_resolved.json",
    "configs/generated/root_task_resolved.json.sha256",
    "configs/generated/root_task_topology.json",
    "configs/generated/swarmui_defaults.toml",
    "configs/generated/swarmui_defaults.toml.sha256",
    "docs/snippets/cas_interfaces.md",
    "docs/snippets/cas_security.md",
    "docs/snippets/coh_doctor_checks.md",
    "docs/snippets/coh_policy.md",
    "docs/snippets/cohesix_py_defaults.md",
    "docs/snippets/cohsh_client.md",
    "docs/snippets/cohsh_grammar.md",
    "docs/snippets/cohsh_policy.md",
    "docs/snippets/cohsh_ticket_policy.md",
    "docs/snippets/gpu_breadcrumbs.md",
    "docs/snippets/observability_interfaces.md",
    "docs/snippets/observability_security.md",
    "docs/snippets/root_task_manifest.md",
    "docs/snippets/swarmui_defaults.md",
    "docs/snippets/ticket_quotas.md",
    "docs/snippets/trace_policy.md",
    "scripts/cohsh/boot_v0.coh",
    "tools/cohesix-py/cohesix/generated.py",
}


def git_paths(*args: str) -> set[str]:
    payload = subprocess.check_output(("git", "-C", root, *args))
    return {
        os.fsdecode(path)
        for path in payload.split(b"\0")
        if path
    }


changed = git_paths("diff", "--name-only", "-z", "HEAD", "--")
changed |= git_paths("diff", "--cached", "--name-only", "-z", "HEAD", "--")
changed |= git_paths("ls-files", "--others", "--exclude-standard", "-z")
unexpected = sorted(
    path
    for path in changed
    if path not in allowed_exact
    and not path.startswith("apps/root-task/src/generated/")
)
if unexpected:
    raise SystemExit(
        "repository changed outside coh-rtc outputs during exact build: "
        + ", ".join(unexpected)
    )
PY
}

capture_exact_source_identity() {
    local status
    EXACT_GIT_COMMIT="$(git -C "$ROOT_DIR" rev-parse --verify HEAD)"
    [[ "$EXACT_GIT_COMMIT" =~ ^[0-9a-f]{40,64}$ ]] || \
      fail "repository HEAD is not a full lowercase hexadecimal Git object ID"
    EXACT_GIT_SHORT="$(git -C "$ROOT_DIR" rev-parse --short=12 HEAD)"
    [[ "$EXACT_GIT_COMMIT" == "$EXACT_GIT_SHORT"* ]] || \
      fail "short Git identity is not a prefix of repository HEAD"
    status="$(git -C "$ROOT_DIR" status --porcelain=v1 --untracked-files=all)"
    [[ -z "$status" ]] || \
      fail "exact Pi image builds require a clean checkout including untracked files"
    EXACT_BUILD_TIMESTAMP="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    log "Exact source identity: commit=${EXACT_GIT_COMMIT} timestamp=${EXACT_BUILD_TIMESTAMP}"
}

capture_build_repository_state() {
    verify_only_codegen_repository_changes
    BUILD_REPOSITORY_STATE_DIGEST="$(repository_state_digest)"
    [[ -n "$BUILD_REPOSITORY_STATE_DIGEST" ]] || \
      fail "failed to fingerprint repository state before root-task build"
}

verify_final_clean_repository_state() {
    local current_head
    local status
    current_head="$(git -C "$ROOT_DIR" rev-parse --verify HEAD)"
    [[ "$current_head" == "$EXACT_GIT_COMMIT" ]] || \
      fail "repository HEAD changed before exact image build cleanup completed"
    status="$(git -C "$ROOT_DIR" status --porcelain=v1 --untracked-files=all)"
    [[ -z "$status" ]] || \
      fail "repository did not return to its exact clean checkout after target codegen"
}

sel4_tree_state_digest() {
    local tree="$1"
    python3 - "$tree" <<'PY'
import hashlib
import os
import stat
import sys

root = os.path.realpath(sys.argv[1])
digest = hashlib.sha256()
digest.update(b"cohesix-sel4-tree-state/v1\0")
for current, directories, files in os.walk(root, topdown=True, followlinks=False):
    directories.sort()
    files.sort()
    for name in (*directories, *files):
        path = os.path.join(current, name)
        relative = os.path.relpath(path, root).encode("utf-8", "surrogateescape")
        observed = os.lstat(path)
        mode = stat.S_IFMT(observed.st_mode) | stat.S_IMODE(observed.st_mode)
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        digest.update(mode.to_bytes(8, "big"))
        if stat.S_ISLNK(observed.st_mode):
            target = os.readlink(path).encode("utf-8", "surrogateescape")
            digest.update(b"link\0" + len(target).to_bytes(8, "big") + target)
        elif stat.S_ISREG(observed.st_mode):
            digest.update(b"file\0" + observed.st_size.to_bytes(8, "big"))
            with open(path, "rb") as handle:
                while chunk := handle.read(1024 * 1024):
                    digest.update(chunk)
        elif stat.S_ISDIR(observed.st_mode):
            digest.update(b"dir\0")
        else:
            raise SystemExit(f"unsupported entry in seL4 build tree: {path}")
print(digest.hexdigest())
PY
}

capture_canonical_sel4_state() {
    CANONICAL_SEL4_STATE_DIGEST="$(sel4_tree_state_digest "$SEL4_BUILD_DIR")"
    [[ "$CANONICAL_SEL4_STATE_DIGEST" =~ ^[0-9a-f]{64}$ ]] || \
      fail "failed to fingerprint the canonical seL4 profile tree"
    log "Bound immutable canonical seL4 tree: ${CANONICAL_SEL4_STATE_DIGEST}"
}

verify_canonical_sel4_state() {
    local phase="$1"
    local observed
    [[ -n "$CANONICAL_SEL4_STATE_DIGEST" ]] || \
      fail "canonical seL4 fingerprint is unavailable at ${phase}"
    observed="$(sel4_tree_state_digest "$SEL4_BUILD_DIR")"
    [[ "$observed" == "$CANONICAL_SEL4_STATE_DIGEST" ]] || \
      fail "canonical seL4 profile tree changed during exact-image work at ${phase}"
}

verify_build_repository_state() {
    local phase="$1"
    local current_head
    local current_digest
    [[ -n "$BUILD_REPOSITORY_STATE_DIGEST" ]] || \
      fail "repository build-state fingerprint is unavailable at ${phase}"
    current_head="$(git -C "$ROOT_DIR" rev-parse --verify HEAD)"
    [[ "$current_head" == "$EXACT_GIT_COMMIT" ]] || \
      fail "repository HEAD changed during exact image build at ${phase}"
    current_digest="$(repository_state_digest)"
    [[ "$current_digest" == "$BUILD_REPOSITORY_STATE_DIGEST" ]] || \
      fail "repository files changed during exact image build at ${phase}"
}

verify_unsealed_pi4_build_marker() {
    local artifact="$1"
    local require_elf_section="${2:-0}"
    local expected_root_elf="${3:-}"
    local expected_root_cpio="${4:-}"
    local -a args=(
        verify-unsealed-marker
        --artifact "$artifact"
    )
    require_file "$artifact"
    if [[ "$require_elf_section" -eq 1 ]]; then
        args+=(--require-elf-load-section)
    fi
    if [[ -n "$expected_root_elf" || -n "$expected_root_cpio" ]]; then
        [[ -n "$expected_root_elf" && -n "$expected_root_cpio" ]] || \
          fail "root archive verification requires both ELF and CPIO inputs"
        args+=(
          --expected-root-elf "$expected_root_elf"
          --expected-root-cpio "$expected_root_cpio"
        )
    fi
    python3 "${ROOT_DIR}/scripts/pi4_image_identity.py" "${args[@]}" >/dev/null || \
      fail "Pi image build marker is absent, ambiguous, or not runtime-loaded: ${artifact}"
}

seal_staged_pi4_image() {
    local unsealed_image="$1"
    local staged_image="$2"
    local fallback_image="$3"
    local identity_metadata="${STAGE_DIR}/${PI4_IMAGE_IDENTITY_STAGE_NAME}"
    local expected_root_elf="$EXACT_ROOT_ELF"
    local expected_root_cpio="$EXACT_ROOT_CPIO"

    require_file "$unsealed_image"
    require_file "$expected_root_elf"
    require_file "$expected_root_cpio"
    verify_build_repository_state "before identity metadata publication"
    python3 "${ROOT_DIR}/scripts/pi4_image_identity.py" seal \
      --image "$unsealed_image" \
      --expected-root-elf "$expected_root_elf" \
      --expected-root-cpio "$expected_root_cpio" >/dev/null || \
      fail "failed to seal the staged Pi image identity"
    mv -f "$unsealed_image" "$staged_image"
    python3 "${ROOT_DIR}/scripts/pi4_image_identity.py" verify \
      --image "$staged_image" \
      --metadata "$identity_metadata" \
      --git-commit "$EXACT_GIT_COMMIT" \
      --source-tree-clean \
      --expected-root-elf "$expected_root_elf" \
      --expected-root-cpio "$expected_root_cpio" >/dev/null || \
      fail "failed to publish final-path Pi image identity metadata"
    require_file "$identity_metadata"
    EXACT_BUILD_ID="$(python3 - "$ROOT_DIR" "$identity_metadata" "$EXACT_GIT_COMMIT" "$EXACT_BUILD_TIMESTAMP" <<'PY'
import sys
from pathlib import Path

sys.path.insert(0, str(Path(sys.argv[1]) / "scripts"))
import pi4_image_identity as identity

metadata = identity.read_metadata(Path(sys.argv[2]))
if metadata.git_commit != sys.argv[3]:
    raise SystemExit("identity metadata Git commit differs from the exact build")
if metadata.build_timestamp != sys.argv[4]:
    raise SystemExit("identity metadata timestamp differs from the exact build")
expected = identity.canonical_build_id(
    sys.argv[3], sys.argv[4], metadata.image_id
)
if metadata.build_id != expected:
    raise SystemExit("identity metadata build ID is not canonical")
print(expected)
PY
)"
    [[ "$EXACT_BUILD_ID" =~ ^[0-9a-f]{64}$ ]] || \
      fail "identity metadata omitted the canonical build ID"
    python3 "${ROOT_DIR}/scripts/pi4_image_identity.py" verify-metadata \
      --image "$staged_image" \
      --metadata "$identity_metadata" \
      --expected-git-commit "$EXACT_GIT_COMMIT" \
      --expected-build-id "$EXACT_BUILD_ID" \
      --expected-root-elf "$expected_root_elf" \
      --expected-root-cpio "$expected_root_cpio" >/dev/null || \
      fail "sealed staged Pi image identity did not verify"

    cp -f "$staged_image" "$fallback_image"
    python3 "${ROOT_DIR}/scripts/pi4_image_identity.py" verify \
      --image "$fallback_image" \
      --expected-root-elf "$expected_root_elf" \
      --expected-root-cpio "$expected_root_cpio" >/dev/null || \
      fail "sealed fallback Pi image identity did not verify"
    cmp -s "$staged_image" "$fallback_image" || \
      fail "primary and fallback Pi images differ after identity sealing"
    # Reinspect the primary image after metadata publication and fallback copy.
    python3 "${ROOT_DIR}/scripts/pi4_image_identity.py" verify-metadata \
      --image "$staged_image" \
      --metadata "$identity_metadata" \
      --expected-git-commit "$EXACT_GIT_COMMIT" \
      --expected-build-id "$EXACT_BUILD_ID" \
      --expected-root-elf "$expected_root_elf" \
      --expected-root-cpio "$expected_root_cpio" >/dev/null || \
      fail "sealed staged image changed after metadata publication"
    verify_build_repository_state "after identity metadata publication"
    log "Sealed complete Pi image identity: ${identity_metadata}"
}

verify_final_staged_pi4_image() {
    local mkimage_bin="$1"
    local staged_image="${STAGE_DIR}/${COHESIX_IMAGE_NAME}"
    local fallback_image="${STAGE_DIR}/${SEL4_UPSTREAM_IMAGE_NAME}"
    local identity_metadata="${STAGE_DIR}/${PI4_IMAGE_IDENTITY_STAGE_NAME}"
    local expected_root_elf="$EXACT_ROOT_ELF"
    local expected_root_cpio="$EXACT_ROOT_CPIO"

    python3 "${ROOT_DIR}/scripts/pi4_image_identity.py" verify-metadata \
      --image "$staged_image" \
      --metadata "$identity_metadata" \
      --expected-git-commit "$EXACT_GIT_COMMIT" \
      --expected-build-id "$EXACT_BUILD_ID" \
      --expected-root-elf "$expected_root_elf" \
      --expected-root-cpio "$expected_root_cpio" >/dev/null || \
      fail "final primary image identity verification failed"
    python3 "${ROOT_DIR}/scripts/pi4_image_identity.py" verify \
      --image "$fallback_image" \
      --expected-root-elf "$expected_root_elf" \
      --expected-root-cpio "$expected_root_cpio" >/dev/null || \
      fail "final fallback image identity verification failed"
    cmp -s "$staged_image" "$fallback_image" || \
      fail "primary and fallback Pi images differ at final verification"
    "$mkimage_bin" -l "$staged_image" >/dev/null || \
      fail "mkimage rejected the final primary Pi image"
    "$mkimage_bin" -l "$fallback_image" >/dev/null || \
      fail "mkimage rejected the final fallback Pi image"
    verify_build_repository_state "after final staged-image verification"
}

find_aarch64_strip() {
    local prefix="${COHESIX_AARCH64_BINUTILS_PREFIX:-/opt/homebrew/bin/aarch64-linux-gnu-}"
    local strip_tool="${prefix}strip"

    [[ "$prefix" == /* ]] || \
      fail "COHESIX_AARCH64_BINUTILS_PREFIX must be an absolute tool prefix"
    [[ -x "$strip_tool" ]] || \
      fail "selected AArch64 binutils strip tool is not executable: ${strip_tool}"
    printf '%s\n' "$strip_tool"
}

STRIPPED_ROOT_TASK_ELF=""
strip_root_task_for_pi_image() {
    local src="$1"
    local strip_tool
    local src_bytes
    local dst_bytes

    strip_tool="$(find_aarch64_strip)"

    mkdir -p "$ROOT_TASK_STRIP_DIR"
    STRIPPED_ROOT_TASK_ELF="${ROOT_TASK_STRIP_DIR}/root-task"
    cp -f "$src" "$STRIPPED_ROOT_TASK_ELF"
    "$strip_tool" --strip-all --remove-section=.comment "$STRIPPED_ROOT_TASK_ELF"
    require_file "$STRIPPED_ROOT_TASK_ELF"
    [[ -s "$STRIPPED_ROOT_TASK_ELF" ]] || fail "stripped root-task ELF is empty"

    src_bytes="$(stat -f '%z' "$src")"
    dst_bytes="$(stat -f '%z' "$STRIPPED_ROOT_TASK_ELF")"
    log "Using stripped root-task ELF: ${STRIPPED_ROOT_TASK_ELF} (${src_bytes} -> ${dst_bytes} bytes)"
}

verify_u_boot_pi4_target() {
    local u_boot_source_dir="${ROOT_DIR}/third_party/u-boot"
    local default_u_boot_bin="${u_boot_source_dir}/u-boot.bin"
    local config_file="${u_boot_source_dir}/.config"
    local u_boot_elf="${u_boot_source_dir}/u-boot"
    local device_tree
    local -a u_boot_inputs=(
        "${u_boot_source_dir}/configs/rpi_4_defconfig"
        "${u_boot_source_dir}/board/raspberrypi/rpi/rpi.env"
        "${u_boot_source_dir}/common/usb_hub.c"
        "${u_boot_source_dir}/drivers/usb/host/xhci-pci.c"
        "${u_boot_source_dir}/drivers/usb/host/xhci-ring.c"
    )
    local input=""

    if [[ "${U_BOOT_BIN}" != "${default_u_boot_bin}" ]]; then
        return 0
    fi

    if [[ ! -f "${config_file}" ]]; then
        log "Skipping U-Boot target check (missing ${config_file})"
        return 0
    fi

    if [[ ! -f "${default_u_boot_bin}" ]]; then
        fail "u-boot.bin is missing; run: gmake -C third_party/u-boot rpi_4_defconfig && gmake -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    fi
    if [[ "${config_file}" -nt "${default_u_boot_bin}" ]]; then
        fail "u-boot.bin is older than ${config_file}; rebuild U-Boot so the flashed binary matches the requested commands"
    fi
    if [[ -f "${u_boot_elf}" && "${config_file}" -nt "${u_boot_elf}" ]]; then
        fail "u-boot ELF is older than ${config_file}; rebuild U-Boot so the flashed binary matches the requested commands"
    fi
    for input in "${u_boot_inputs[@]}"; do
        [[ -f "${input}" ]] || continue
        if [[ "${input}" -nt "${default_u_boot_bin}" ]]; then
            fail "u-boot.bin is older than ${input}; rebuild U-Boot so the flashed binary matches the requested Pi 4 bring-up sources"
        fi
        if [[ -f "${u_boot_elf}" && "${input}" -nt "${u_boot_elf}" ]]; then
            fail "u-boot ELF is older than ${input}; rebuild U-Boot so the flashed binary matches the requested Pi 4 bring-up sources"
        fi
    done

    device_tree="$(awk -F= '/^CONFIG_DEFAULT_DEVICE_TREE=/{gsub(/"/, "", $2); print $2}' "${config_file}" | tail -n 1)"
    if [[ "${device_tree}" != "bcm2711-rpi-4-b" ]]; then
        fail "u-boot.bin is not configured for Pi 4 (CONFIG_DEFAULT_DEVICE_TREE=${device_tree:-unset}); run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    fi
    grep -q '^CONFIG_CMD_ASKENV=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_CMD_ASKENV; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    local required_menu_config=""
    for required_menu_config in \
        CONFIG_HUSH_PARSER \
        CONFIG_CMD_EXPORTENV \
        CONFIG_CMD_IMPORTENV \
        CONFIG_CMD_ITEST \
        CONFIG_CMD_MEMORY \
        CONFIG_CMD_SOURCE \
        CONFIG_CMD_SETEXPR \
        CONFIG_CMD_FAT \
        CONFIG_FAT_WRITE \
        CONFIG_REGEX \
        CONFIG_SYS_DEVICE_NULLDEV; do
        grep -q "^${required_menu_config}=y$" "${config_file}" || \
          fail "u-boot.bin is missing ${required_menu_config}; rebuild U-Boot from third_party/u-boot/configs/rpi_4_defconfig"
    done
    grep -q '^CONFIG_CMD_BMP=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_CMD_BMP; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    grep -q '^CONFIG_CMD_BOOTM=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_CMD_BOOTM; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    grep -q '^CONFIG_LEGACY_IMAGE_FORMAT=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_LEGACY_IMAGE_FORMAT; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    grep -Fq 'CONFIG_BOOTCOMMAND="if fatload mmc 0:1 ${scriptaddr} boot.scr.uimg; then source ${scriptaddr}; else echo [cohesix] ERROR: boot.scr.uimg missing on mmc 0:1; fi"' "${config_file}" || \
      fail "u-boot.bin does not boot the Cohesix script directly; rebuild U-Boot from third_party/u-boot/configs/rpi_4_defconfig"
    ! grep -q '^CONFIG_BOOTCOMMAND="bootflow scan' "${config_file}" || \
      fail "u-boot.bin still uses bootflow scan; rebuild U-Boot from third_party/u-boot/configs/rpi_4_defconfig"
    grep -q '^CONFIG_BOOTDELAY=2$' "${config_file}" || \
      fail "u-boot.bin must expose a 2-second serial autoboot abort window for remote Cohesix menu recovery"
    grep -q '^CONFIG_USE_PREBOOT=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_PREBOOT; rebuild U-Boot from third_party/u-boot/configs/rpi_4_defconfig"
    grep -Fq 'CONFIG_PREBOOT="setenv stdin serial; setenv stdout serial,vidconsole; setenv stderr serial,vidconsole"' "${config_file}" || \
      fail "u-boot.bin must start on the serial/video console before Cohesix owns USB input"
    grep -q '^CONFIG_ENV_IS_NOWHERE=y$' "${config_file}" || \
      fail "u-boot.bin must ignore generic persistent uboot.env; only Cohesix cohesix.env policy is permitted"
    ! grep -q '^CONFIG_ENV_IS_IN_FAT=y$' "${config_file}" || \
      fail "u-boot.bin must not import generic FAT uboot.env before the Cohesix script"
    if [[ "${U_BOOT_MENU_INPUT}" == "usb" ]]; then
        grep -q '^CONFIG_USB_KEYBOARD=y$' "${config_file}" || \
          fail "u-boot.bin is missing CONFIG_USB_KEYBOARD for --uboot-menu-input usb; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
        if ! grep -Eq '^CONFIG_SYS_USB_EVENT_POLL=y$|^CONFIG_SYS_USB_EVENT_POLL_VIA_CONTROL_EP=y$' "${config_file}"; then
          fail "u-boot.bin is missing a supported USB keyboard polling mode for --uboot-menu-input usb; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
        fi
    fi
    grep -q '^CONFIG_SYS_CONSOLE_IS_IN_ENV=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_SYS_CONSOLE_IS_IN_ENV; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
}

verify_skip_build_image_fresh() {
    local image="$EXACT_PI4_IMAGE"
    local input=""
    local stale=""
    local -a freshness_inputs=(
        "${MANIFEST_PATH}"
        "${ROOT_DIR}/apps/root-task/Cargo.toml"
        "${ROOT_DIR}/apps/root-task/build.rs"
        "${ROOT_DIR}/apps/root-task/build_support.rs"
        "${ROOT_DIR}/apps/root-task/src"
        "${ROOT_DIR}/apps/pi4-driver-runtime/Cargo.toml"
        "${ROOT_DIR}/apps/pi4-driver-runtime/src"
        "${ROOT_DIR}/crates/pi4-driver-abi/Cargo.toml"
        "${ROOT_DIR}/crates/pi4-driver-abi/src"
    )

    require_file "$image"
    for input in "${freshness_inputs[@]}"; do
        [[ -e "$input" ]] || continue
        if [[ -f "$input" && "$input" -nt "$image" ]]; then
            fail "--skip-build selected stale seL4 image ${image}; ${input} is newer. Re-run without --skip-build or pass --sel4-build-dir to the matching build tree."
        fi
        if [[ -d "$input" ]]; then
            stale="$(find "$input" \
                \( -path "${ROOT_DIR}/apps/root-task/src/generated" -o \
                   -path "${ROOT_DIR}/apps/root-task/src/generated/*" \) -prune -o \
                -type f \( -name '*.rs' -o -name '*.toml' \) -newer "$image" -print -quit)"
            if [[ -n "$stale" ]]; then
                fail "--skip-build selected stale seL4 image ${image}; ${stale} is newer. Re-run without --skip-build or pass --sel4-build-dir to the matching build tree."
            fi
        fi
    done
}

select_exact_assembly_inputs() {
    EXACT_PI4_IMAGE="${PI4_ASSEMBLY_DIR}/${SEL4_UPSTREAM_IMAGE_NAME}"
    EXACT_ROOT_ELF="${PI4_ASSEMBLY_DIR}/rootserver"
    EXACT_ROOT_CPIO="${PI4_ASSEMBLY_DIR}/archive.archive.o.cpio"
    EXACT_CANONICAL_PROFILE_STAMP="${SEL4_BUILD_DIR}/cohesix-profile-build-inputs.json"
    EXACT_COMPOSITION_RECORD="${PI4_ASSEMBLY_DIR}/composition-profile-build-inputs.json"
    EXACT_COMPOSITION_CACHE="${PI4_ASSEMBLY_DIR}/composition-CMakeCache.txt"
    EXACT_COMPOSITION_TIMER_HEADER="${PI4_ASSEMBLY_DIR}/composition-platform_gen.h"
}

adopt_skip_build_source_timestamp() {
    local image="$EXACT_PI4_IMAGE"
    local root_elf="$EXACT_ROOT_ELF"
    local root_cpio="$EXACT_ROOT_CPIO"
    local identity_json

    require_file "$image"
    require_file "$root_elf"
    require_file "$root_cpio"
    identity_json="$(python3 "${ROOT_DIR}/scripts/pi4_image_identity.py" \
      verify-unsealed-marker \
      --artifact "$image" \
      --expected-root-elf "$root_elf" \
      --expected-root-cpio "$root_cpio")" || \
      fail "--skip-build image lacks exact root/archive marker provenance"
    EXACT_BUILD_TIMESTAMP="$(printf '%s' "$identity_json" | \
      python3 -c 'import json,sys; record=json.load(sys.stdin); commit=sys.argv[1]; marker=record["build_marker"]; embedded=record["embedded_git_commit"]; assert commit.startswith(embedded) and not marker.split()[1].endswith("-dirty"); print(record["build_timestamp"])' \
      "$EXACT_GIT_COMMIT")" || \
      fail "--skip-build image marker does not belong to the exact clean commit"
    [[ -n "$EXACT_BUILD_TIMESTAMP" ]] || \
      fail "--skip-build image marker omitted its exact build timestamp"
    log "Reusing exact image build timestamp: ${EXACT_BUILD_TIMESTAMP}"
}

write_sel4_image_provenance() {
    local image="$EXACT_PI4_IMAGE"
    local root_elf="$EXACT_ROOT_ELF"
    local root_cpio="$EXACT_ROOT_CPIO"
    local cache="$EXACT_COMPOSITION_CACHE"
    local timer_header="$EXACT_COMPOSITION_TIMER_HEADER"
    local provenance="${image}${SEL4_IMAGE_PROVENANCE_SUFFIX}"

    python3 - \
      "$provenance" "$image" "$root_elf" "$root_cpio" "$MANIFEST_PATH" \
      "$cache" "$timer_header" "$EXACT_GIT_COMMIT" "$EXACT_BUILD_TIMESTAMP" \
      "$ROOT_TASK_FEATURES" "$EXACT_CANONICAL_PROFILE_STAMP" \
      "$EXACT_COMPOSITION_RECORD" "$CANONICAL_SEL4_STATE_DIGEST" <<'PY'
import hashlib
import json
import os
import sys
import tempfile
from pathlib import Path

(
    destination,
    image_path,
    root_path,
    cpio_path,
    manifest_path,
    cache_path,
    timer_header_path,
    commit,
    timestamp,
    features,
    canonical_profile_stamp,
    composition_record,
    canonical_profile_state,
) = sys.argv[1:]


def digest(path: str) -> str:
    value = hashlib.sha256()
    with open(path, "rb") as handle:
        while chunk := handle.read(1024 * 1024):
            value.update(chunk)
    return value.hexdigest()


record = {
    "schema": "cohesix-pi4-sel4-image-provenance/v3",
    "git_commit": commit,
    "source_tree_clean": True,
    "build_timestamp": timestamp,
    "root_task_features": features,
    "source_manifest_sha256": digest(manifest_path),
    "canonical_profile_stamp_sha256": digest(canonical_profile_stamp),
    "canonical_profile_state_sha256": canonical_profile_state,
    "composition_record_sha256": digest(composition_record),
    "composition_cmake_cache_sha256": digest(cache_path),
    "composition_timer_header_sha256": digest(timer_header_path),
    "wrapper_sha256": digest(image_path),
    "rootserver_sha256": digest(root_path),
    "rootserver_cpio_sha256": digest(cpio_path),
}
rendered = (json.dumps(record, indent=2, sort_keys=True) + "\n").encode()
target = Path(destination)
target.parent.mkdir(parents=True, exist_ok=True)
temporary_name = None
try:
    with tempfile.NamedTemporaryFile(
        mode="wb", dir=target.parent, prefix=f".{target.name}.", delete=False
    ) as temporary:
        temporary_name = temporary.name
        temporary.write(rendered)
        temporary.flush()
        os.fsync(temporary.fileno())
    os.replace(temporary_name, target)
    temporary_name = None
finally:
    if temporary_name is not None:
        try:
            os.unlink(temporary_name)
        except FileNotFoundError:
            pass
PY
    require_file "$provenance"
    log "Wrote exact seL4 wrapper provenance: ${provenance}"
}

verify_skip_build_provenance() {
    local image="$EXACT_PI4_IMAGE"
    local provenance="${image}${SEL4_IMAGE_PROVENANCE_SUFFIX}"

    require_file "$provenance"
    python3 - \
      "$provenance" "$image" "$EXACT_ROOT_ELF" "$EXACT_ROOT_CPIO" \
      "$MANIFEST_PATH" "$EXACT_COMPOSITION_CACHE" \
      "$EXACT_COMPOSITION_TIMER_HEADER" "$EXACT_GIT_COMMIT" \
      "$EXACT_BUILD_TIMESTAMP" "$ROOT_TASK_FEATURES" \
      "$EXACT_CANONICAL_PROFILE_STAMP" "$EXACT_COMPOSITION_RECORD" \
      "$CANONICAL_SEL4_STATE_DIGEST" <<'PY'
import hashlib
import json
import sys

(
    provenance_path,
    image_path,
    root_path,
    cpio_path,
    manifest_path,
    cache_path,
    timer_header_path,
    commit,
    timestamp,
    features,
    canonical_profile_stamp,
    composition_record,
    canonical_profile_state,
) = sys.argv[1:]


def digest(path: str) -> str:
    value = hashlib.sha256()
    with open(path, "rb") as handle:
        while chunk := handle.read(1024 * 1024):
            value.update(chunk)
    return value.hexdigest()


with open(provenance_path, "r", encoding="utf-8") as handle:
    record = json.load(handle)
expected = {
    "schema": "cohesix-pi4-sel4-image-provenance/v3",
    "git_commit": commit,
    "source_tree_clean": True,
    "build_timestamp": timestamp,
    "root_task_features": features,
    "source_manifest_sha256": digest(manifest_path),
    "canonical_profile_stamp_sha256": digest(canonical_profile_stamp),
    "canonical_profile_state_sha256": canonical_profile_state,
    "composition_record_sha256": digest(composition_record),
    "composition_cmake_cache_sha256": digest(cache_path),
    "composition_timer_header_sha256": digest(timer_header_path),
    "wrapper_sha256": digest(image_path),
    "rootserver_sha256": digest(root_path),
    "rootserver_cpio_sha256": digest(cpio_path),
}
if record != expected:
    missing = sorted(set(expected) - set(record)) if isinstance(record, dict) else []
    extra = sorted(set(record) - set(expected)) if isinstance(record, dict) else []
    raise SystemExit(
        "--skip-build provenance does not match the selected exact build "
        f"(missing={missing} extra={extra})"
    )
PY
    log "Verified --skip-build manifest, feature, profile, rootserver, and wrapper provenance"
}

verify_boot_cmd_handoff() {
    local path="$1"

    require_file "$path"

    grep -q "setenv coh_menu_input ${U_BOOT_MENU_INPUT}" "$path" || fail "boot.cmd menu input mode does not match ${U_BOOT_MENU_INPUT}"
    grep -q 'setenv coh_fastboot_rsts_addr 0xfe100020' "$path" || fail "boot.cmd is missing Cohesix fast-boot RSTS address"
    grep -q 'setenv coh_fastboot_rsts_mask 0x00ff0000' "$path" || fail "boot.cmd is missing Cohesix fast-boot RSTS mask"
    grep -q 'setenv coh_fastboot_rsts_magic 0x00430000' "$path" || fail "boot.cmd is missing Cohesix fast-boot RSTS marker"
    grep -q 'setenv coh_fastboot_rsts_reset_mask 0x00000400' "$path" || fail "boot.cmd is missing fast-boot reset-status diagnostic mask"
    grep -q 'setenv coh_fastboot_rsts_clear_mask 0xff00ffff' "$path" || fail "boot.cmd is missing Cohesix fast-boot RSTS clear mask"
    grep -q 'setexpr.l coh_fastboot_rsts \*${coh_fastboot_rsts_addr}' "$path" || fail "boot.cmd does not read Cohesix fast-boot RSTS state"
    grep -q 'setexpr.l coh_fastboot_rsts_marker ${coh_fastboot_rsts} "&" ${coh_fastboot_rsts_mask}' "$path" || fail "boot.cmd does not mask Cohesix high fast-boot RSTS state"
    grep -q 'setexpr.l coh_fastboot_rsts_reset ${coh_fastboot_rsts} "&" ${coh_fastboot_rsts_reset_mask}' "$path" || fail "boot.cmd does not mask reset-status diagnostics"
    ! grep -q 'coh_fastboot_source software-reset-saved-policy' "$path" || fail "boot.cmd must not auto boot saved policy from the software-reset bit"
    grep -q 'setexpr.l coh_fastboot_rsts_reset ${coh_fastboot_rsts} "&" ${coh_fastboot_rsts_reset_mask}' "$path" || fail "boot.cmd does not retain reset-status diagnostics"
    ! grep -q 'test "${coh_has_saved_config}" = "1"; then setenv coh_fastboot 1' "$path" || fail "boot.cmd must not gate an auto boot on saved Cohesix policy"
    grep -q 'mw.l ${coh_fastboot_rsts_addr} ${coh_fastboot_rsts_clear} 1' "$path" || fail "boot.cmd does not clear the Cohesix fast-boot marker"
    grep -q 'boot marker diagnostics: rsts=${coh_fastboot_rsts}' "$path" || fail "boot.cmd does not report bounded Cohesix boot marker diagnostics"
    grep -q "setenv coh_force_serial_preboot 'setenv stdin serial" "$path" || fail "boot.cmd does not force serial input before fast-boot detection"
    grep -q '^run coh_force_serial_preboot$' "$path" || fail "boot.cmd does not arm the serial-only preboot path before loading policy"
    grep -q '^run coh_detect_saved_config$' "$path" || fail "boot.cmd does not resolve saved policy before fast-boot detection"
    ! grep -q 'run coh_maybe_fastboot' "$path" || fail "boot.cmd must enter the interactive menu by default instead of fast-booting"
    grep -q 'run coh_report_fastboot_miss' "$path" || fail "boot.cmd must log reset marker diagnostics before the menu"
    ! grep -q 'coh_maybe_fastboot_or_recovery' "$path" || fail "boot.cmd must not retain the fast-boot-or-recovery bypass"
    ! grep -q '^setenv bootdelay 0$' "$path" || fail "boot.cmd must not erase the compiled U-Boot autoboot abort window"
    grep -q '^run coh_start_menu$' "$path" || fail "boot.cmd does not enter the bounded interactive Cohesix menu on cold boot"
    ! grep -q 'unattended boot: using saved or manifest settings' "$path" || fail "boot.cmd must not bypass the menu without a Cohesix fast-boot source"
    grep -q 'test "${coh_menu_input}" = "usb"' "$path" || fail "boot.cmd is missing guarded USB menu-input setup"
    grep -q 'run coh_quiesce_usb' "$path" || fail "boot.cmd is missing USB quiesce step"
    grep -q 'run coh_clear_xhci_handoff_live' "$path" || fail "boot.cmd is missing xHCI stale-token clearing before usb stop"
    grep -q 'setenv coh_xhci_mmio;' "$path" || fail "boot.cmd does not clear stale xHCI MMIO before usb stop"
    grep -q 'setenv coh_xhci_pci_cmd;' "$path" || fail "boot.cmd does not clear stale xHCI PCI command before usb stop"
    grep -q 'xHCI trust tokens cleared before Cohesix cold boot' "$path" || fail "boot.cmd does not clear U-Boot xHCI trust tokens before Cohesix cold boot"
    grep -q 'usb stop failed or was inactive before Cohesix boot' "$path" || fail "boot.cmd must tolerate unconditional U-Boot USB stop before Cohesix handoff"
    ! grep -q 'coh_export_xhci_stop_seed' "$path" || fail "boot.cmd still exports an xHCI stop-state seed"
    ! grep -q 'run coh_export_xhci_handoff' "$path" || fail "boot.cmd still contains obsolete xHCI handoff export"
    ! grep -q 'setenv coh_xhci_mmio 0x' "$path" || fail "boot.cmd still exports obsolete xHCI MMIO handoff"
    ! grep -q 'setenv coh_xhci_pci_cmd 0x' "$path" || fail "boot.cmd still exports obsolete xHCI PCI command handoff"
    ! grep -q 'cohesix,xhci-usbcmd' "$path" || fail "boot.cmd still mirrors obsolete xHCI USBCMD seed"
    ! grep -q 'cohesix,xhci-usbsts' "$path" || fail "boot.cmd still mirrors obsolete xHCI USBSTS seed"
    ! grep -q 'cohesix,xhci-iman0' "$path" || fail "boot.cmd still mirrors obsolete xHCI IMAN seed"
    ! grep -q 'setenv coh_xhci_handoff_ready 1' "$path" || fail "boot.cmd still exports obsolete xHCI handoff-ready token"
    ! grep -q '\[cohesix:usb-trace\]' "$path" || fail "boot.cmd still contains obsolete USB trace breadcrumbs"
    ! grep -q 'coh_force_xhci_handoff_reprobe' "$path" || fail "boot.cmd still contains obsolete forced xHCI reprobe logic"
    ! grep -q 'cohesix,xhci-cap-length' "$path" || fail "boot.cmd still mirrors obsolete xHCI capability snapshots"
    ! grep -q 'cohesix,xhci-mmio' "$path" || fail "boot.cmd still mirrors obsolete xHCI MMIO handoff diagnostics"
    ! grep -q 'cohesix,xhci-pci-cmd' "$path" || fail "boot.cmd still mirrors obsolete xHCI PCI command diagnostics"
    ! grep -q 'cohesix,xhci-handoff-ready' "$path" || fail "boot.cmd still mirrors obsolete xHCI handoff-ready diagnostics"
    ! grep -q 'cohesix,xhci-irq-quiesced' "$path" || fail "boot.cmd still mirrors obsolete xHCI IRQ handoff diagnostics"
    ! grep -q 'cohesix,xhci-handoff-halted' "$path" || fail "boot.cmd still mirrors obsolete xHCI halted handoff diagnostics"
    ! grep -q 'cohesix,xhci-handoff-safe' "$path" || fail "boot.cmd still mirrors obsolete xHCI handoff-safe diagnostics"
    ! grep -q 'cohesix,xhci-handoff-source' "$path" || fail "boot.cmd still mirrors obsolete xHCI handoff source diagnostics"
    grep -q 'setenv coh_begin_wifi_secret_input' "$path" || fail "boot.cmd is missing USB-only Wi-Fi secret-entry setup"
    grep -q 'setenv stdout vidconsole; setenv stderr vidconsole' "$path" || fail "boot.cmd does not suppress serial echo during Wi-Fi secret entry"
    grep -q 'setenv coh_end_wifi_secret_input' "$path" || fail "boot.cmd is missing Wi-Fi secret-entry console restore"
    grep -q 'setenv stdout serial,vidconsole; setenv stderr serial,vidconsole' "$path" || fail "boot.cmd does not restore serial output after Wi-Fi secret entry"
    grep -Fq 'Privacy notice: Wi-Fi network name and password are visible on this display; they are hidden from serial output' "$path" || fail "boot.cmd does not disclose local Wi-Fi input visibility"
    grep -Fq 'askenv coh_wifi_psk_new "Wi-Fi password (leave blank for an open network): " 64' "$path" || fail "boot.cmd does not collect replacement Wi-Fi passwords in the protected USB-only prompt"
    grep -Fq 'Wi-Fi password entry is unavailable over serial because U-Boot echoes typed input' "$path" || fail "boot.cmd does not explain serial-safe Wi-Fi policy staging"
    grep -Fq 'No Wi-Fi network is configured and local USB input is unavailable' "$path" || fail "boot.cmd does not emit the context-safe missing Wi-Fi policy marker"
    grep -q 'fatsize mmc 0:1 ${coh_policy_file}' "$path" || fail "boot.cmd does not size-bound Cohesix policy loads"
    grep -q 'env import -r -t ${coh_policy_addr} ${filesize}' "$path" || fail "boot.cmd does not import allowlisted CRLF-safe Cohesix policy"
    ! grep -q 'env import -d' "$path" || fail "boot.cmd must clear Cohesix policy before import instead of warning on omitted optional fields"
    grep -q 'setenv stdout nulldev; setenv stderr nulldev; if cmp.b ${coh_policy_addr} ${coh_policy_verify_addr} ${coh_policy_export_size}' "$path" || fail "boot.cmd does not privately read back and verify saved Cohesix policy"
    grep -q 'while test "${coh_menu_running}" = "1"' "$path" || fail "boot.cmd does not use the bounded Cohesix menu dispatcher"
    grep -Fq 'Change Wi-Fi network' "$path" || fail "boot.cmd does not permit replacement of existing Wi-Fi settings"
    grep -Fq 'Reset saved settings?' "$path" || fail "boot.cmd does not confirm saved-settings reset"
    grep -Fq 'askenv coh_choice "Select option [0]: " 4' "$path" || fail "boot.cmd reset confirmation does not default to cancel"
    grep -Fq 'run coh_read_cancel_choice' "$path" || fail "boot.cmd reset confirmation does not use the cancel-default reader"
    grep -Fq 'setenv coh_menu_page reset' "$path" || fail "boot.cmd root menu does not route reset through confirmation"
    grep -Fq 'elif test "${coh_menu_page}" = "reset"; then run coh_confirm_reset' "$path" || fail "boot.cmd dispatcher does not serve the saved-settings reset page"
    grep -Fq '  0. Back to boot menu' "$path" || fail "boot.cmd does not expose consistent back navigation"
    grep -Fq '  9. Advanced: Open U-Boot shell' "$path" || fail "boot.cmd does not expose the advanced U-Boot shell consistently"
    grep -Fq 'Boot once without saving' "$path" || fail "boot.cmd does not distinguish one-time boot from persistence"
    grep -Fq 'Boot logo: On (select to turn off)' "$path" || fail "boot.cmd does not display boot-logo state"
    ! grep -Fq 'DHCP ON' "$path" || fail "boot.cmd still exposes nonstandard DHCP ON terminology"
    ! grep -Fq 'DHCP OFF' "$path" || fail "boot.cmd still exposes nonstandard DHCP OFF terminology"
    ! grep -Fq 'Wired Ethernet (GENET)' "$path" || fail "boot.cmd still exposes hardware-specific Ethernet terminology"
    ! grep -Fq 'Wi-Fi (CYW43455)' "$path" || fail "boot.cmd still exposes hardware-specific Wi-Fi terminology"
    grep -q 'invalid coh_show_logo value' "$path" || fail "boot.cmd does not reject invalid saved logo policy"
    grep -Fq 'echo "[cohesix] Wi-Fi network: Configured (name hidden)"' "$path" || fail "boot.cmd does not redact Wi-Fi network names from menu and serial summaries"
    ! grep -Fq 'wifi-ssid=${coh_wifi_ssid}' "$path" || fail "boot.cmd must not print an untrusted Wi-Fi SSID"
}

verify_pi4_sel4_xhci_device_untyped() {
    local generated_dts="${SEL4_BUILD_DIR}/kernel/kernel.dts"

    require_file "$generated_dts"
    grep -q 'device-untypes@600000000' "$generated_dts" || \
      fail "repo-managed Pi4 kernel.dts is missing device-untypes@600000000 (${generated_dts})"
    log "Verified repo-managed Pi4 device-untyped artifact for VL805 BAR0"
}

require_cmake_bool() {
    local cache_file="$1"
    local name="$2"
    local expected="$3"

    grep -Eq "^${name}:[A-Z]+=${expected}$" "$cache_file" || \
      fail "${name} not ${expected} in ${cache_file}"
}

platform_timer_clock_hz() {
    local header="$1"

    awk '
      /^#define[[:space:]]+TIMER_CLOCK_HZ[[:space:]]+/ {
        line = $0
        gsub(/[^0-9]/, " ", line)
        split(line, parts, /[[:space:]]+/)
        for (idx in parts) {
          if (parts[idx] != "") {
            print parts[idx]
            exit
          }
        }
      }
    ' "$header"
}

verify_pi4_sel4_counter_config() {
    local build_dir="${1:-$SEL4_BUILD_DIR}"
    local cache_file="${build_dir}/CMakeCache.txt"
    local platform_header="${build_dir}/kernel/gen_headers/plat/platform_gen.h"
    local timer_clock_hz

    require_file "$cache_file"
    require_file "$platform_header"
    require_cmake_bool "$cache_file" "KernelArmExportVCNTUser" "ON"
    require_cmake_bool "$cache_file" "KernelArmExportPCNTUser" "OFF"
    require_cmake_bool "$cache_file" "KernelArmExportPTMRUser" "OFF"
    require_cmake_bool "$cache_file" "KernelArmExportVTMRUser" "OFF"

    timer_clock_hz="$(platform_timer_clock_hz "$platform_header")"
    [[ "$timer_clock_hz" == "54000000" ]] || \
      fail "Pi4 seL4 TIMER_CLOCK_HZ must be 54000000, got ${timer_clock_hz:-unset} in ${platform_header}"

    log "Verified Pi4 seL4 virtual counter export and TIMER_CLOCK_HZ=${timer_clock_hz}"
}

verify_pi4_uboot_image_start_addr() {
    local require_generated="${1:-cache}"
    local build_dir="${2:-$SEL4_BUILD_DIR}"
    local cache_file="${build_dir}/CMakeCache.txt"
    local image_start_header="${build_dir}/elfloader/gen_headers/image_start_addr.h"

    require_file "$cache_file"
    grep -Eq "^IMAGE_START_ADDR(:[A-Z]+)?=${PI4_UBOOT_IMAGE_START_ADDR}$" "$cache_file" || \
      fail "Pi4 U-Boot IMAGE_START_ADDR must be ${PI4_UBOOT_IMAGE_START_ADDR} for bootm/XIP handoff"

    if [[ "$require_generated" == "generated" ]]; then
        require_file "$image_start_header"
        grep -Eq "^#define IMAGE_START_ADDR ${PI4_UBOOT_IMAGE_START_ADDR}$" "$image_start_header" || \
          fail "Pi4 U-Boot image_start_addr.h drifted from ${PI4_UBOOT_IMAGE_START_ADDR}: ${image_start_header}"
    fi
}

verify_pi4_elfloader_platform_info() {
    local build_dir="${1:-$SEL4_BUILD_DIR}"
    local platform_info="${build_dir}/elfloader/gen_headers/platform_info.h"

    require_file "$platform_info"
    grep -q "memory_region" "$platform_info" || \
      fail "Pi4 elfloader platform_info.h is missing memory_region"
}

verify_one_domain_schedule_cache_absent() {
    local build_dir="${1:-$SEL4_BUILD_DIR}"
    local cache_file="${build_dir}/CMakeCache.txt"

    require_file "$cache_file"
    grep -q "^KernelNumDomains:STRING=1$" "$cache_file" || return 0
    ! grep -Eq "^KernelDomainSchedule(:|-)" "$cache_file" || \
      fail "canonical one-domain seL4 profile contains forbidden KernelDomainSchedule input"
}

require_sel4_lib_available() {
    require_file "${SEL4_BUILD_DIR}/libsel4/libsel4.a"
}

validate_pi4_sel4_build() {
    local profile_tool="${ROOT_DIR}/scripts/sel4_profile.py"
    local profile_python="${SEL4_VENV_DIR}/bin/python"

    require_file "$profile_tool"
    require_file "$profile_python"

    log "Validating immutable repo-managed ${PI4_SEL4_PROFILE}: ${SEL4_BUILD_DIR}"
    "$profile_python" "$profile_tool" validate \
      --repo-managed \
      --profile "$PI4_SEL4_PROFILE" \
      --build-dir "$SEL4_BUILD_DIR" \
      --require-artifacts \
      --for-runtime >/dev/null || \
      fail "repo-managed ${PI4_SEL4_PROFILE} validation failed for ${SEL4_BUILD_DIR}"

    local cache_file="${SEL4_BUILD_DIR}/CMakeCache.txt"
    require_file "$cache_file"
    grep -q "^KernelPlatform:STRING=bcm2711$" "$cache_file" || fail "KernelPlatform not set to bcm2711"
    grep -q "^RELEASE:BOOL=OFF$" "$cache_file" || fail "RELEASE mode unexpectedly enabled"
    grep -q "^SMP:BOOL=ON$" "$cache_file" || fail "SMP not enabled"
    grep -q "^NUM_NODES:STRING=4$" "$cache_file" || fail "NUM_NODES not set to 4"
    grep -Eq "^RPI4_MEMORY:[A-Z]+=${PI4_TOTAL_MEM_MB}$" "$cache_file" || fail "RPI4_MEMORY not set to ${PI4_TOTAL_MEM_MB}"
    grep -q "^Sel4testAllowSettingsOverride:BOOL=ON$" "$cache_file" || fail "Sel4testAllowSettingsOverride not ON"
    grep -q "^KernelDebugBuild:BOOL=ON$" "$cache_file" || fail "KernelDebugBuild not ON"
    grep -q "^KernelPrinting:BOOL=ON$" "$cache_file" || fail "KernelPrinting not ON"
    verify_pi4_sel4_counter_config
    grep -q "^HardwareDebugAPI:BOOL=OFF$" "$cache_file" || fail "HardwareDebugAPI must be OFF for current sel4-sys bindings"
    grep -q "^KernelMaxNumNodes:STRING=4$" "$cache_file" || fail "KernelMaxNumNodes not 4"
    grep -q "^ElfloaderRootserversLast:BOOL=ON$" "$cache_file" || fail "ElfloaderRootserversLast must be ON for seL4 16 Pi4 rootserver placement"
    grep -q "^ElfloaderImage:STRING=uimage$" "$cache_file" || fail "ElfloaderImage not set to uimage"
    grep -q "^ElfloaderIncludeDtb:BOOL=OFF$" "$cache_file" || fail "ElfloaderIncludeDtb must be OFF for Pi4 U-Boot DTB handoff"
    verify_one_domain_schedule_cache_absent
    verify_pi4_uboot_image_start_addr
    verify_pi4_elfloader_platform_info
}

resolve_mkimage() {
    local canonical="${ROOT_DIR}/third_party/u-boot/tools/mkimage"
    if [[ -x "$canonical" ]]; then
        printf "%s\n" "$canonical"
        return 0
    fi

    fail "repository U-Boot mkimage missing: ${canonical}; rebuild third_party/u-boot without --clean"
}

cpio_supports_reproducible() {
    local cpio_bin="$1"
    "$cpio_bin" --help 2>&1 | grep -q -- "--reproducible"
}

resolve_cpio() {
    local -a candidates=()
    local candidate=""

    if command -v cpio >/dev/null 2>&1; then
        candidates+=("$(command -v cpio)")
    fi
    if command -v gcpio >/dev/null 2>&1; then
        candidates+=("$(command -v gcpio)")
    fi

    candidates+=(
        "/opt/homebrew/opt/cpio/bin/cpio"
        "/usr/local/opt/cpio/bin/cpio"
    )

    for candidate in "${candidates[@]}"; do
        [[ -x "$candidate" ]] || continue
        if cpio_supports_reproducible "$candidate"; then
            printf "%s\n" "$candidate"
            return 0
        fi
    done

    fail "GNU cpio with --reproducible support not found (install Homebrew cpio or ensure gcpio is on PATH)"
}

configure_cpio_path() {
    local cpio_bin="$1"
    local cpio_dir
    cpio_dir="$(dirname "$cpio_bin")"
    # Driver-runtime packaging invokes "cpio" by name from a nested shell. Keep
    # the verified GNU cpio first even if its directory already appears later
    # in PATH behind macOS /usr/bin/cpio.
    export PATH="${cpio_dir}:${PATH}"
    log "Using cpio: ${cpio_bin}"
}

prepend_path_var() {
    local var_name="$1"
    local path="$2"
    local current="${!var_name:-}"

    case ":${current}:" in
        *":${path}:"*) ;;
        *)
            if [[ -n "${current}" ]]; then
                printf -v "${var_name}" '%s:%s' "${path}" "${current}"
            else
                printf -v "${var_name}" '%s' "${path}"
            fi
            export "${var_name}"
            ;;
    esac
}

append_env_flag() {
    local var_name="$1"
    local flag="$2"
    local current="${!var_name:-}"

    case " ${current} " in
        *" ${flag} "*) ;;
        *)
            if [[ -n "${current}" ]]; then
                printf -v "${var_name}" '%s %s' "${current}" "${flag}"
            else
                printf -v "${var_name}" '%s' "${flag}"
            fi
            export "${var_name}"
            ;;
    esac
}

resolve_gnu_make() {
    if command -v gmake >/dev/null 2>&1; then
        command -v gmake
        return 0
    fi

    if command -v make >/dev/null 2>&1 && make --version 2>/dev/null | grep -q 'GNU Make'; then
        command -v make
        return 0
    fi

    fail "GNU make is required to rebuild Pi 4 U-Boot (install gmake or provide GNU make as 'make')"
}

configure_u_boot_openssl_env() {
    local prefix=""
    local -a candidates=()
    local pkg_config_cflags=""

    if command -v brew >/dev/null 2>&1; then
        prefix="$(brew --prefix openssl@3 2>/dev/null || true)"
        [[ -n "${prefix}" ]] && candidates+=("${prefix}")
        prefix="$(brew --prefix openssl 2>/dev/null || true)"
        [[ -n "${prefix}" ]] && candidates+=("${prefix}")
    fi

    candidates+=(
        "/opt/homebrew/opt/openssl@3"
        "/usr/local/opt/openssl@3"
        "/opt/homebrew/opt/openssl"
        "/usr/local/opt/openssl"
    )

    for prefix in "${candidates[@]}"; do
        [[ -d "${prefix}" ]] || continue
        append_env_flag HOSTCFLAGS "-I${prefix}/include"
        append_env_flag HOSTLDFLAGS "-L${prefix}/lib"
        [[ -d "${prefix}/lib/pkgconfig" ]] && prepend_path_var PKG_CONFIG_PATH "${prefix}/lib/pkgconfig"
        [[ -d "${prefix}/lib64/pkgconfig" ]] && prepend_path_var PKG_CONFIG_PATH "${prefix}/lib64/pkgconfig"
        pkg_config_cflags="$(PKG_CONFIG_PATH="${PKG_CONFIG_PATH:-}" pkg-config --cflags libssl libcrypto 2>/dev/null || true)"
        [[ -n "${pkg_config_cflags}" ]] && append_env_flag HOSTCFLAGS "${pkg_config_cflags}"
        log "Using OpenSSL from ${prefix} for Pi4 U-Boot host tools"
        return 0
    done

    fail "could not resolve a Homebrew OpenSSL prefix for Pi4 U-Boot; install openssl@3 or use a prebuilt default u-boot.bin without --clean"
}

clean_root_task_build() {
    log "Cleaning root-task cargo artifacts"
    cargo clean -p root-task
}

resolve_build_jobs() {
    local jobs="${TP_HOST_JOBS:-${CARGO_BUILD_JOBS:-${CMAKE_BUILD_PARALLEL_LEVEL:-}}}"
    if [[ -z "${jobs}" ]]; then
        jobs="$(sysctl -n hw.ncpu)"
    fi
    [[ "${jobs}" =~ ^[1-9][0-9]*$ ]] || \
      fail "build parallelism must be a positive integer (TP_HOST_JOBS/CARGO_BUILD_JOBS/CMAKE_BUILD_PARALLEL_LEVEL), got: ${jobs}"
    printf "%s\n" "${jobs}"
}

rebuild_u_boot_pi4() {
    local u_boot_source_dir="${ROOT_DIR}/third_party/u-boot"
    local default_u_boot_bin="${u_boot_source_dir}/u-boot.bin"
    local gnu_make=""
    local jobs=""
    local rc=0

    [[ "${U_BOOT_BIN}" == "${default_u_boot_bin}" ]] || \
      fail "--clean currently requires the default Pi4 U-Boot output (${default_u_boot_bin})"

    gnu_make="$(resolve_gnu_make)"
    jobs="$(resolve_build_jobs)"

    configure_u_boot_openssl_env

    log "Cleaning Pi4 U-Boot build in ${u_boot_source_dir}"
    "${gnu_make}" -C "${u_boot_source_dir}" distclean
    log "Configuring Pi4 U-Boot (rpi_4_defconfig)"
    "${gnu_make}" -C "${u_boot_source_dir}" ARCH=arm CROSS_COMPILE="${U_BOOT_CROSS_COMPILE}" rpi_4_defconfig
    log "Accepting default answers for any new Pi4 U-Boot Kconfig symbols"
    set +o pipefail
    yes "" | "${gnu_make}" -C "${u_boot_source_dir}" ARCH=arm CROSS_COMPILE="${U_BOOT_CROSS_COMPILE}" oldconfig
    rc=$?
    set -o pipefail
    [[ "${rc}" -eq 0 ]] || fail "failed to refresh Pi4 U-Boot defaults with oldconfig"
    log "Building Pi4 U-Boot"
    "${gnu_make}" -C "${u_boot_source_dir}" ARCH=arm CROSS_COMPILE="${U_BOOT_CROSS_COMPILE}" -j"${jobs}"

    require_file "${default_u_boot_bin}"
    prepend_path_var PATH "${u_boot_source_dir}/tools"
}

clean_pi4_build() {
    clean_root_task_build
    rebuild_u_boot_pi4
    log "Canonical seL4/build_UBOOT remains immutable"
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --manifest)
                [[ $# -ge 2 ]] || fail "--manifest requires a path"
                MANIFEST_PATH="$2"
                shift 2
                ;;
            --sel4-build-dir)
                [[ $# -ge 2 ]] || fail "--sel4-build-dir requires a path"
                SEL4_BUILD_DIR="$2"
                shift 2
                ;;
            --venv)
                [[ $# -ge 2 ]] || fail "--venv requires a path"
                SEL4_VENV_DIR="$2"
                shift 2
                ;;
            --u-boot-bin)
                [[ $# -ge 2 ]] || fail "--u-boot-bin requires a path"
                U_BOOT_BIN="$2"
                shift 2
                ;;
            --firmware-dir)
                [[ $# -ge 2 ]] || fail "--firmware-dir requires a path"
                FIRMWARE_DIR="$2"
                shift 2
                ;;
            --stage-dir)
                [[ $# -ge 2 ]] || fail "--stage-dir requires a path"
                STAGE_DIR="$2"
                shift 2
                ;;
            --image-name)
                [[ $# -ge 2 ]] || fail "--image-name requires a filename"
                COHESIX_IMAGE_NAME="$2"
                shift 2
                ;;
            --root-task-features)
                [[ $# -ge 2 ]] || fail "--root-task-features requires a list"
                ROOT_TASK_FEATURES="$2"
                shift 2
                ;;
            --uboot-menu-input)
                [[ $# -ge 2 ]] || fail "--uboot-menu-input requires serial or usb"
                U_BOOT_MENU_INPUT="$2"
                U_BOOT_MENU_INPUT_SOURCE="cli"
                shift 2
                ;;
            --clean)
                CLEAN_BUILD=1
                shift
                ;;
            --skip-build)
                SKIP_BUILD=1
                shift
                ;;
            --flash-disk)
                [[ $# -ge 2 ]] || fail "--flash-disk requires a device path"
                FLASH_DISK="$2"
                shift 2
                ;;
            --initialize-disk)
                INITIALIZE_DISK=1
                shift
                ;;
            --policy-recovery-file)
                [[ $# -ge 2 ]] || fail "--policy-recovery-file requires a path"
                POLICY_RECOVERY_FILE="$2"
                shift 2
                ;;
            --disk-label)
                [[ $# -ge 2 ]] || fail "--disk-label requires a name"
                DISK_LABEL="$2"
                shift 2
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                fail "unknown argument: $1"
                ;;
        esac
    done
}

validate_menu_input_mode() {
    if [[ -n "${COHESIX_UBOOT_MENU_INPUT:-}" && "${U_BOOT_MENU_INPUT_SOURCE}" != "cli" ]]; then
        log "Ignoring COHESIX_UBOOT_MENU_INPUT=${COHESIX_UBOOT_MENU_INPUT}; use --uboot-menu-input for explicit serial lab captures"
    fi
    case "${U_BOOT_MENU_INPUT}" in
        serial|usb) ;;
        *) fail "--uboot-menu-input must be serial or usb (got ${U_BOOT_MENU_INPUT})" ;;
    esac
}

validate_output_paths() {
    local image_name_folded
    local protected
    local reserved
    local reserved_folded
    [[ -n "$COHESIX_IMAGE_NAME" ]] || fail "--image-name must not be empty"
    [[ "$COHESIX_IMAGE_NAME" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || \
      fail "--image-name must use a FAT-safe alphanumeric basename"
    [[ "$COHESIX_IMAGE_NAME" == "$(basename "$COHESIX_IMAGE_NAME")" ]] || \
      fail "--image-name must be one basename without directory components"
    [[ "$COHESIX_IMAGE_NAME" != "." && "$COHESIX_IMAGE_NAME" != ".." ]] || \
      fail "--image-name must not be . or .."
    [[ "$COHESIX_IMAGE_NAME" != *. ]] || \
      fail "--image-name must not end with a FAT-normalized trailing dot"
    image_name_folded="$(printf '%s' "$COHESIX_IMAGE_NAME" | LC_ALL=C tr '[:upper:]' '[:lower:]')"
    for reserved in \
        "$SEL4_UPSTREAM_IMAGE_NAME" \
        "$PI4_IMAGE_IDENTITY_STAGE_NAME" \
        "$COHESIX_LOGO_STAGE_NAME" \
        "$BOOTSTD_LOGO_STAGE_NAME" \
        "$BRCMFMAC_CMDLINE_STAGE_NAME" \
        "$BRCMFMAC_DYNAMIC_DEBUG_STAGE_NAME" \
        "$DRIVER_RUNTIME_CPIO_STAGE_NAME" \
        "cohesix-driver-runtimes.cpio" \
        "start4.elf" "fixup4.dat" "bcm2711-rpi-4-b.dtb" "u-boot.bin" \
        "config.txt" "boot.cmd" "boot.scr.uimg" "cohesix_boot_state.txt" \
        "pi4-runtime-dma-proof.env" "cohesix.env" "overlays"; do
        reserved_folded="$(printf '%s' "$reserved" | LC_ALL=C tr '[:upper:]' '[:lower:]')"
        [[ "$image_name_folded" != "$reserved_folded" ]] || \
          fail "--image-name collides with reserved staged artifact: ${reserved}"
    done
    [[ "$STAGE_DIR" != "/" && "$STAGE_DIR" != "$ROOT_DIR" && \
       "$STAGE_DIR" != "${ROOT_DIR}/out" ]] || \
      fail "--stage-dir must be a dedicated directory strictly below the repository out directory"
    case "${STAGE_DIR}/" in
        "${ROOT_DIR}/out/"*) ;;
        "${ROOT_DIR}/"*)
            fail "--stage-dir inside the checkout must be strictly under ${ROOT_DIR}/out"
            ;;
    esac
    if [[ -n "${POLICY_RECOVERY_FILE}" ]]; then
        [[ -n "${FLASH_DISK}" ]] || \
          fail "--policy-recovery-file requires --flash-disk"
        [[ -f "${POLICY_RECOVERY_FILE}" ]] || \
          fail "--policy-recovery-file must name a regular file"
        case "${POLICY_RECOVERY_FILE}/" in
            "${STAGE_DIR}/"*)
                fail "--policy-recovery-file must be outside the replaceable stage directory"
                ;;
        esac
    fi
    if [[ "${INITIALIZE_DISK}" -eq 1 ]]; then
        [[ -n "${FLASH_DISK}" ]] || fail "--initialize-disk requires --flash-disk"
    fi
    for protected in \
        "$SEL4_BUILD_DIR" \
        "$SEL4_VENV_DIR" \
        "$FIRMWARE_DIR" \
        "$PI4_ASSEMBLY_DIR" \
        "$(dirname "$MANIFEST_PATH")" \
        "$(dirname "$U_BOOT_BIN")"; do
        case "${protected}/" in
            "${STAGE_DIR}/"*)
                fail "--stage-dir must not contain a protected source/build path: ${protected}"
                ;;
        esac
        case "${STAGE_DIR}/" in
            "${protected}/"*)
                fail "--stage-dir must not be inside a protected source/build path: ${protected}"
                ;;
        esac
    done
}

activate_venv() {
    if [[ ! -d "$SEL4_VENV_DIR" ]]; then
        fail "venv directory not found: ${SEL4_VENV_DIR}"
    fi
    # shellcheck disable=SC1090
    source "${SEL4_VENV_DIR}/bin/activate"
}

realpath_py() {
    python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$1"
}

canonicalize_input_paths() {
    MANIFEST_PATH="$(realpath_py "${MANIFEST_PATH}")"
    SEL4_BUILD_DIR="$(realpath_py "${SEL4_BUILD_DIR}")"
    SEL4_VENV_DIR="$(realpath_py "${SEL4_VENV_DIR}")"
    U_BOOT_BIN="$(realpath_py "${U_BOOT_BIN}")"
    FIRMWARE_DIR="$(realpath_py "${FIRMWARE_DIR}")"
    PI4_WIFI_FIRMWARE_DIR="${COHESIX_PI4_WIFI_FIRMWARE_DIR:-${FIRMWARE_DIR}/firmware/cyw43455-linux-capture}"
    STAGE_DIR="$(realpath_py "${STAGE_DIR}")"
    if [[ -n "${POLICY_RECOVERY_FILE}" ]]; then
        POLICY_RECOVERY_FILE="$(realpath_py "${POLICY_RECOVERY_FILE}")"
    fi
}

validate_canonical_sel4_build_dir() {
    local canonical

    canonical="$(realpath_py "$DEFAULT_REPO_SEL4_BUILD_DIR")"
    [[ "$SEL4_BUILD_DIR" == "$canonical" ]] || \
      fail "--sel4-build-dir must resolve exactly to ${canonical}; alternate or out/ seL4 inputs are not supported"
}

root_task_target_dir() {
    local target_dir="${CARGO_TARGET_DIR:-${ROOT_DIR}/target}"

    case "$target_dir" in
        /*)
            printf "%s\n" "$target_dir"
            ;;
        *)
            printf "%s\n" "${ROOT_DIR}/${target_dir}"
            ;;
    esac
}

root_task_release_elf_path() {
    printf "%s/aarch64-unknown-none/release/root-task\n" "$(root_task_target_dir)"
}

run_coh_rtc_codegen_for_manifest() {
    local manifest_path="$1"
    local manifest_json="$2"
    mkdir -p "${GENERATED_CONFIG_DIR}"

    cargo run --locked -p coh-rtc -- \
      "$manifest_path" \
      --out "${ROOT_DIR}/apps/root-task/src/generated" \
      --manifest "$manifest_json" \
      --cas-manifest-template "${GENERATED_CONFIG_DIR}/cas_manifest_template.json" \
      --cli-script "${ROOT_DIR}/scripts/cohsh/boot_v0.coh" \
      --doc-snippet "${ROOT_DIR}/docs/snippets/root_task_manifest.md" \
      --gpu-breadcrumbs-snippet "${ROOT_DIR}/docs/snippets/gpu_breadcrumbs.md" \
      --observability-interfaces-snippet "${ROOT_DIR}/docs/snippets/observability_interfaces.md" \
      --observability-security-snippet "${ROOT_DIR}/docs/snippets/observability_security.md" \
      --ticket-quotas-snippet "${ROOT_DIR}/docs/snippets/ticket_quotas.md" \
      --trace-policy-snippet "${ROOT_DIR}/docs/snippets/trace_policy.md" \
      --cas-interfaces-snippet "${ROOT_DIR}/docs/snippets/cas_interfaces.md" \
      --cas-security-snippet "${ROOT_DIR}/docs/snippets/cas_security.md" \
      --cohesix-py-defaults "${ROOT_DIR}/tools/cohesix-py/cohesix/generated.py" \
      --cohesix-py-doc "${ROOT_DIR}/docs/snippets/cohesix_py_defaults.md" \
      --coh-doctor-doc "${ROOT_DIR}/docs/snippets/coh_doctor_checks.md" \
      --cohsh-policy "${GENERATED_CONFIG_DIR}/cohsh_policy.toml" \
      --cohsh-policy-rust "${ROOT_DIR}/apps/cohsh/src/generated/policy.rs" \
      --cohsh-policy-doc "${ROOT_DIR}/docs/snippets/cohsh_policy.md" \
      --cohsh-client-rust "${ROOT_DIR}/apps/cohsh/src/generated/client.rs" \
      --cohsh-client-doc "${ROOT_DIR}/docs/snippets/cohsh_client.md" \
      --cohsh-grammar-doc "${ROOT_DIR}/docs/snippets/cohsh_grammar.md" \
      --cohsh-ticket-policy-doc "${ROOT_DIR}/docs/snippets/cohsh_ticket_policy.md" \
      --coh-policy "${GENERATED_CONFIG_DIR}/coh_policy.toml" \
      --coh-policy-rust "${ROOT_DIR}/apps/coh/src/generated/policy.rs" \
      --coh-policy-doc "${ROOT_DIR}/docs/snippets/coh_policy.md" \
      --swarmui-defaults "${GENERATED_CONFIG_DIR}/swarmui_defaults.toml" \
      --swarmui-defaults-rust "${ROOT_DIR}/apps/swarmui/src/generated.rs" \
      --swarmui-defaults-doc "${ROOT_DIR}/docs/snippets/swarmui_defaults.md"
}

run_coh_rtc_codegen() {
    run_coh_rtc_codegen_for_manifest \
      "${MANIFEST_PATH}" \
      "${GENERATED_CONFIG_DIR}/root_task_resolved.json"
}

restore_canonical_codegen() {
    if [[ "${RESTORE_CANONICAL_CODEGEN}" -eq 0 ]]; then
        return 0
    fi
    log "Restoring canonical manifest artifacts via coh-rtc (${CANONICAL_MANIFEST_PATH})"
    run_coh_rtc_codegen_for_manifest \
      "${CANONICAL_MANIFEST_PATH}" \
      "${GENERATED_CONFIG_DIR}/root_task_resolved.json"
}

cleanup() {
    local status=$?
    trap - EXIT
    stop_flash_caffeinate
    if [[ -n "${PRESERVED_POLICY_TEMP:-}" ]]; then
        if [[ "$status" -ne 0 && "${FLASH_MEDIA_MUTATION_STARTED:-0}" -eq 1 && \
              -z "${POLICY_RECOVERY_CONSUMED_FILE:-}" ]]; then
            chmod 600 "$PRESERVED_POLICY_TEMP" 2>/dev/null || true
            log "Retained saved policy after interrupted media update: ${PRESERVED_POLICY_TEMP}"
            log "Retry with --policy-recovery-file ${PRESERVED_POLICY_TEMP}"
        else
            rm -f "$PRESERVED_POLICY_TEMP"
        fi
        PRESERVED_POLICY_TEMP=""
    fi
    if [[ "$status" -ne 0 && -n "${POLICY_RECOVERY_CONSUMED_FILE:-}" ]]; then
        log "Retained explicit policy recovery file after interrupted flash: ${POLICY_RECOVERY_CONSUMED_FILE}"
        log "Retry with --policy-recovery-file ${POLICY_RECOVERY_CONSUMED_FILE}"
    fi
    if ! restore_canonical_codegen; then
        status=1
    fi
    if [[ -n "$EXACT_GIT_COMMIT" ]] && ! verify_final_clean_repository_state; then
        status=1
    fi
    exit "$status"
}

sync_resolved_manifest_json() {
    local manifest_json="${GENERATED_CONFIG_DIR}/root_task_resolved.json"
    local src_real
    local dst_real
    mkdir -p "${GENERATED_CONFIG_DIR}"

    src_real="$(realpath_py "${MANIFEST_PATH}")"
    dst_real="$(realpath_py "${manifest_json}")"
    if [[ "${src_real}" != "${dst_real}" ]]; then
        cp -f "${MANIFEST_PATH}" "${manifest_json}"
    fi

    if [[ -f "${MANIFEST_PATH}.sha256" ]]; then
        src_real="$(realpath_py "${MANIFEST_PATH}.sha256")"
        dst_real="$(realpath_py "${manifest_json}.sha256")"
        if [[ "${src_real}" != "${dst_real}" ]]; then
            cp -f "${MANIFEST_PATH}.sha256" "${manifest_json}.sha256"
        fi
    fi
}

compose_pi4_assembly() {
    local composition_epoch
    local composition_tool="${ROOT_DIR}/scripts/pi4_prebuilt_composition.py"
    local profile_python="${SEL4_VENV_DIR}/bin/python"
    local root_hash_actual
    local root_hash_expected

    require_file "$composition_tool"
    require_file "$profile_python"
    composition_epoch="$(python3 - "$EXACT_BUILD_TIMESTAMP" <<'PY'
from datetime import datetime, timezone
import sys

parsed = datetime.strptime(sys.argv[1], "%Y-%m-%dT%H:%M:%SZ")
print(int(parsed.replace(tzinfo=timezone.utc).timestamp()))
PY
)" || fail "could not convert exact image timestamp to a Unix epoch"
    [[ "$composition_epoch" =~ ^[0-9]+$ ]] || \
      fail "exact image composition timestamp is not a Unix epoch"

    rm -rf "$PI4_ASSEMBLY_DIR"
    mkdir -p "$PI4_ASSEMBLY_DIR"
    log "Composing root-task with immutable seL4/build_UBOOT artifacts"
    "$profile_python" "$composition_tool" \
      --sel4-build-dir "$SEL4_BUILD_DIR" \
      --rootserver "$STRIPPED_ROOT_TASK_ELF" \
      --output-dir "$PI4_ASSEMBLY_DIR" \
      --timestamp "$composition_epoch" || \
      fail "repo-managed Pi4 prebuilt composition failed"

    select_exact_assembly_inputs
    cp -f "${SEL4_BUILD_DIR}/CMakeCache.txt" "$EXACT_COMPOSITION_CACHE"
    cp -f "${SEL4_BUILD_DIR}/kernel/gen_headers/plat/platform_gen.h" \
      "$EXACT_COMPOSITION_TIMER_HEADER"
    require_file "$EXACT_PI4_IMAGE"
    require_file "$EXACT_ROOT_ELF"
    require_file "$EXACT_ROOT_CPIO"
    require_file "$EXACT_CANONICAL_PROFILE_STAMP"
    require_file "$EXACT_COMPOSITION_RECORD"
    require_file "${PI4_ASSEMBLY_DIR}/elfloader"
    require_file "${PI4_ASSEMBLY_DIR}/payload.bin"

    root_hash_expected="$(shasum -a 256 "$STRIPPED_ROOT_TASK_ELF" | awk '{print $1}')"
    root_hash_actual="$(shasum -a 256 "$EXACT_ROOT_ELF" | awk '{print $1}')"
    [[ "$root_hash_actual" == "$root_hash_expected" ]] || \
      fail "composed rootserver differs from the exact stripped root-task"
    verify_unsealed_pi4_build_marker \
      "$EXACT_PI4_IMAGE" 0 "$EXACT_ROOT_ELF" "$EXACT_ROOT_CPIO"
    log "Published immutable-input Pi assembly at ${PI4_ASSEMBLY_DIR}"
}

build_pi4_image() {
    local root_task_elf

    export SEL4_BUILD_DIR
    export SEL4_BUILD="$SEL4_BUILD_DIR"
    export SEL4_LD="${ROOT_DIR}/apps/root-task/sel4.ld"

    root_task_elf="$(root_task_release_elf_path)"

    verify_pi4_sel4_xhci_device_untyped
    validate_pi4_sel4_build
    require_sel4_lib_available
    capture_canonical_sel4_state

    if [[ "${MANIFEST_PATH}" == *.toml ]]; then
        log "Regenerating manifest artifacts via coh-rtc"
        run_coh_rtc_codegen
    elif [[ "${MANIFEST_PATH}" == *.json ]]; then
        log "Using pre-resolved manifest JSON (${MANIFEST_PATH})"
        sync_resolved_manifest_json
    else
        fail "unsupported --manifest extension (expected .toml or .json): ${MANIFEST_PATH}"
    fi
    capture_build_repository_state

    local sel4_target="aarch64-unknown-none"
    local sel4_profile="release"
    local sel4_artifact_dir
    local worker_output_dir
    local worker_output_canonical_dir
    local worker_image_archive
    local worker_image_manifest
    local worker_manifest_tool
    local console_network_runtime_path
    local nine_door_runtime_path
    local sel4_target_package
    local -a sel4_runtime_packages=(
        nine-door-runtime
        console-network-runtime
        worker-heart
        worker-gpu
        worker-lora
        pi4-driver-runtime
    )
    sel4_artifact_dir="$(root_task_target_dir)/${sel4_target}/${sel4_profile}"
    local -a required_sel4_runtime_paths=(
        "${sel4_artifact_dir}/nine-door-runtime"
        "${sel4_artifact_dir}/console-network-runtime"
        "${sel4_artifact_dir}/worker-heart"
        "${sel4_artifact_dir}/worker-gpu"
        "${sel4_artifact_dir}/worker-lora"
    )
    console_network_runtime_path="${sel4_artifact_dir}/console-network-runtime"
    nine_door_runtime_path="${sel4_artifact_dir}/nine-door-runtime"
    worker_output_dir="${ROOT_DIR}/out/pi4-worker-images"
    worker_output_canonical_dir="${worker_output_dir}/canonical"
    worker_image_archive="${worker_output_dir}/cohesix-worker-images.cpio"
    worker_image_manifest="${worker_output_dir}/cohesix-worker-image-manifest.json"
    worker_manifest_tool="${SCRIPT_DIR}/worker_image_manifest.py"

    log "Building 26e child/runtime images for Pi4 root-task"
    local -a sel4_runtime_build_args=(build --locked --target "$sel4_target" --release)
    for sel4_target_package in "${sel4_runtime_packages[@]}"; do
        sel4_runtime_build_args+=( -p "$sel4_target_package" )
    done
    cargo "${sel4_runtime_build_args[@]}"

    for sel4_target_package in "${required_sel4_runtime_paths[@]}"; do
        require_file "$sel4_target_package"
    done

    [[ -f "$worker_manifest_tool" ]] || fail "Worker image manifest tool is missing: $worker_manifest_tool"
    mkdir -p "$worker_output_dir"
    python3 "$worker_manifest_tool" build \
      --image-dir "$sel4_artifact_dir" \
      --output-dir "$worker_output_canonical_dir" \
      --archive "$worker_image_archive" \
      --manifest "$worker_image_manifest" \
      --target "$sel4_target" \
      --profile "$sel4_profile"
    python3 "$worker_manifest_tool" verify \
      --archive "$worker_image_archive" \
      --manifest "$worker_image_manifest"
    require_file "$worker_image_archive"
    require_file "$worker_image_manifest"
    [[ -s "$worker_image_archive" ]] || \
      fail "Worker image archive is empty: ${worker_image_archive}"
    [[ -s "$worker_image_manifest" ]] || \
      fail "Worker image manifest is empty: ${worker_image_manifest}"
    worker_image_archive="$(realpath_py "$worker_image_archive")"
    worker_image_manifest="$(realpath_py "$worker_image_manifest")"
    log "Using worker image archive: ${worker_image_archive}"
    log "Using worker image manifest: ${worker_image_manifest}"

    local embedded_runtime_cpio="${DRIVER_RUNTIME_EMBED_DIR}/${DRIVER_RUNTIME_EMBED_CPIO_NAME}"
    mkdir -p "${DRIVER_RUNTIME_EMBED_DIR}"

    log "Packaging Pi4 isolated driver runtime images"
    package_driver_runtime_raw_cpio "$embedded_runtime_cpio" "$sel4_artifact_dir"


    log "Building root-task (${ROOT_TASK_FEATURES})"
    require_dir "${PI4_WIFI_FIRMWARE_DIR}"
    log "Using Pi4 WiFi firmware bundle: ${PI4_WIFI_FIRMWARE_DIR}"
      COHESIX_BUILD_STAMP="$EXACT_BUILD_TIMESTAMP" \
      COHESIX_EXACT_GIT_COMMIT="$EXACT_GIT_COMMIT" \
      COHESIX_EXACT_SOURCE_CLEAN=1 \
      COHESIX_CONSOLE_NETWORK_RUNTIME_IMAGE="$console_network_runtime_path" \
      COHESIX_NINEDOOR_RUNTIME_IMAGE="$nine_door_runtime_path" \
      COHESIX_WORKER_IMAGE_ARCHIVE="$worker_image_archive" \
      COHESIX_WORKER_IMAGE_MANIFEST="$worker_image_manifest" \
      COHESIX_PI4_DRIVER_RUNTIME_PAYLOAD="${embedded_runtime_cpio}" \
      COHESIX_PI4_WIFI_FIRMWARE_DIR="${PI4_WIFI_FIRMWARE_DIR}" \
      cargo build \
        --locked \
        --target aarch64-unknown-none \
        --release \
        -p root-task \
        --no-default-features \
        --features "$ROOT_TASK_FEATURES"
    verify_build_repository_state "after root-task build"

    require_file "$root_task_elf"
    verify_unsealed_pi4_build_marker "$root_task_elf" 1
    log "Built root-task ELF: ${root_task_elf}"
    strip_root_task_for_pi_image "$root_task_elf"
    verify_unsealed_pi4_build_marker "$STRIPPED_ROOT_TASK_ELF" 1
    compose_pi4_assembly
    verify_canonical_sel4_state "after rootserver composition"
    validate_pi4_sel4_build
    verify_canonical_sel4_state "after post-composition validation"
    verify_build_repository_state "after final seL4 wrapper build"
    write_sel4_image_provenance
}

stage_uboot_logo() {
    local out="$1"
    local temp_bmp
    local python_bin

    if [[ ! -f "${COHESIX_LOGO_SOURCE}" ]]; then
        log "Skipping Cohesix logo staging (missing ${COHESIX_LOGO_SOURCE})"
        return 0
    fi
    if ! command -v sips >/dev/null 2>&1; then
        log "Skipping Cohesix logo staging (sips not found)"
        return 0
    fi
    python_bin="$(command -v python3 || true)"
    if [[ -z "${python_bin}" ]]; then
        log "Skipping Cohesix logo staging (python3 not found)"
        return 0
    fi

    temp_bmp="$(mktemp "${TMPDIR:-/tmp}/cohesix-logo.XXXXXX.bmp")"
    trap 'rm -f "${temp_bmp}"' RETURN

    sips -Z 320 -s format bmp "${COHESIX_LOGO_SOURCE}" --out "${temp_bmp}" >/dev/null
    "${python_bin}" - "${temp_bmp}" "${out}" <<'PY'
import struct
import sys
from pathlib import Path

src = Path(sys.argv[1])
dst = Path(sys.argv[2])
data = bytearray(src.read_bytes())
if data[:2] != b"BM":
    raise SystemExit("not a BMP file")
pixel_offset = struct.unpack_from("<I", data, 10)[0]
width = struct.unpack_from("<i", data, 18)[0]
height = struct.unpack_from("<i", data, 22)[0]
bits_per_pixel = struct.unpack_from("<H", data, 28)[0]
compression = struct.unpack_from("<I", data, 30)[0]
if bits_per_pixel != 24 or compression != 0:
    raise SystemExit("unsupported BMP format")
if height < 0:
    row_bytes = ((abs(width) * (bits_per_pixel // 8) + 3) // 4) * 4
    rows = [
        data[pixel_offset + row_bytes * idx: pixel_offset + row_bytes * (idx + 1)]
        for idx in range(abs(height))
    ]
    rows.reverse()
    struct.pack_into("<i", data, 22, abs(height))
    data[pixel_offset:pixel_offset + row_bytes * len(rows)] = b"".join(rows)
dst.write_bytes(data)
PY
    trap - RETURN
    rm -f "${temp_bmp}"
    log "Staged U-Boot logo at ${out}"
}

stage_pi4_dtb() {
    local src="$1"
    local out="$2"

    require_file "$src"
    python3 - "$src" "$out" "$PI4_DTB_PADDED_SIZE" <<'PY'
import struct
import sys
from pathlib import Path

src = Path(sys.argv[1])
dst = Path(sys.argv[2])
target_size = int(sys.argv[3])
data = bytearray(src.read_bytes())
if len(data) < 40:
    raise SystemExit("dtb too small")
if data[:4] != b"\xd0\x0d\xfe\xed":
    raise SystemExit("invalid dtb magic")

totalsize = struct.unpack_from(">I", data, 4)[0]
blob_len = max(len(data), totalsize)
if blob_len > len(data):
    data.extend(b"\x00" * (blob_len - len(data)))
new_size = max(blob_len, target_size)
struct.pack_into(">I", data, 4, new_size)
if len(data) < new_size:
    data.extend(b"\x00" * (new_size - len(data)))
dst.write_bytes(data)
PY
    log "Staged padded Pi4 DTB at ${out} (${PI4_DTB_PADDED_SIZE} bytes target)"
}

write_boot_cmd() {
    local out="$1"
    local coh_image="$2"
    local fallback_image="$3"
    cat >"$out" <<'EOF'
echo "[cohesix] pi4 autoboot script"
setenv coh_image __COH_IMAGE__
setenv coh_image_fallback __COH_IMAGE_FALLBACK__
setenv coh_addr 0x10000000
setenv coh_dtb_addr 0x14000000
setenv coh_dtb_file bcm2711-rpi-4-b.dtb
setenv coh_policy_addr 0x02100000
setenv coh_policy_verify_addr 0x02101000
setenv coh_policy_file cohesix.env
setenv coh_policy_max_size 0x180
setenv coh_ipv4_text_regex "^[0-9][0-9]?[0-9]?[.][0-9][0-9]?[0-9]?[.][0-9][0-9]?[0-9]?[.][0-9][0-9]?[0-9]?\$"
setenv coh_prefix_text_regex "^[0-9][0-9]?\$"
setenv coh_psk_min_regex "^........"
setenv coh_runtime_cpio_addr 0x15000000
setenv coh_runtime_cpio_file __COH_RUNTIME_CPIO_FILE__
setenv coh_logo_addr 0x02000000
setenv coh_logo_file __COH_LOGO_FILE__
setenv coh_logo_bootstd_file __COH_BOOTSTD_LOGO_FILE__
setenv coh_logo_delay 1
setenv coh_logo_x 20
setenv coh_logo_y 20
setenv coh_menu_input __COH_MENU_INPUT__
setenv coh_pm_password 0x5a000000
setenv coh_fastboot_rsts_addr 0xfe100020
setenv coh_fastboot_rsts_mask 0x00ff0000
setenv coh_fastboot_rsts_magic 0x00430000
setenv coh_fastboot_rsts_reset_mask 0x00000400
setenv coh_fastboot_rsts_clear_mask 0xff00ffff
setenv coh_reset_policy 'setenv coh_net_mode ""; setenv coh_net_interface ""; setenv coh_static_ip ""; setenv coh_static_prefix_len ""; setenv coh_static_gateway ""; setenv coh_wifi_ssid ""; setenv coh_wifi_psk ""'
setenv coh_clear_saved_policy 'run coh_reset_policy; setenv coh_show_logo ""'
setenv coh_force_serial_preboot 'setenv stdin serial; setenv stdout serial,vidconsole; setenv stderr serial,vidconsole; setenv coh_usb_input_ready 0'
setenv coh_detect_fastboot 'setenv coh_fastboot 0; setenv coh_fastboot_source menu; setexpr.l coh_fastboot_rsts *${coh_fastboot_rsts_addr}; setexpr.l coh_fastboot_rsts_marker ${coh_fastboot_rsts} "&" ${coh_fastboot_rsts_mask}; setexpr.l coh_fastboot_rsts_reset ${coh_fastboot_rsts} "&" ${coh_fastboot_rsts_reset_mask}; if itest.l ${coh_fastboot_rsts_marker} == ${coh_fastboot_rsts_magic}; then setenv coh_fastboot_source marker-diagnostic; run coh_clear_fastboot_marker; fi'
setenv coh_clear_fastboot_marker 'setexpr.l coh_fastboot_rsts_clear *${coh_fastboot_rsts_addr} "&" ${coh_fastboot_rsts_clear_mask}; setexpr.l coh_fastboot_rsts_clear ${coh_fastboot_rsts_clear} "|" ${coh_pm_password}; mw.l ${coh_fastboot_rsts_addr} ${coh_fastboot_rsts_clear} 1'
setenv coh_report_fastboot_miss 'echo "[cohesix] boot marker diagnostics: rsts=${coh_fastboot_rsts} high=${coh_fastboot_rsts_marker} reset=${coh_fastboot_rsts_reset} saved=${coh_has_saved_config} source=${coh_fastboot_source}"'
setenv coh_normalize_choice 'if test -z "${coh_choice}"; then setenv coh_choice 1; elif test "${coh_choice}" = " 0"; then setenv coh_choice 0; elif test "${coh_choice}" = "  0"; then setenv coh_choice 0; elif test "${coh_choice}" = " 1"; then setenv coh_choice 1; elif test "${coh_choice}" = "  1"; then setenv coh_choice 1; elif test "${coh_choice}" = " 2"; then setenv coh_choice 2; elif test "${coh_choice}" = "  2"; then setenv coh_choice 2; elif test "${coh_choice}" = " 3"; then setenv coh_choice 3; elif test "${coh_choice}" = "  3"; then setenv coh_choice 3; elif test "${coh_choice}" = " 4"; then setenv coh_choice 4; elif test "${coh_choice}" = "  4"; then setenv coh_choice 4; elif test "${coh_choice}" = " 5"; then setenv coh_choice 5; elif test "${coh_choice}" = "  5"; then setenv coh_choice 5; elif test "${coh_choice}" = " 9"; then setenv coh_choice 9; elif test "${coh_choice}" = "  9"; then setenv coh_choice 9; fi'
setenv coh_read_choice 'setenv coh_choice; askenv coh_choice "Select option [1]: " 4; run coh_normalize_choice'
setenv coh_read_cancel_choice 'setenv coh_choice; askenv coh_choice "Select option [0]: " 4; if test -z "${coh_choice}"; then setenv coh_choice 0; else run coh_normalize_choice; fi'
setenv coh_bootstrap_usb_session 'if test "${coh_menu_input}" = "usb"; then if test "${coh_usb_input_ready}" != "1"; then echo "[cohesix] starting USB host session for menu/input"; pci enum; if usb start; then setenv coh_usb_input_ready 1; echo "[cohesix] USB host session active"; else setenv coh_usb_input_ready 0; echo "[cohesix] WARNING: usb start failed before menu/input"; fi; fi; else setenv coh_usb_input_ready 0; fi'
setenv coh_prepare_input 'run coh_bootstrap_usb_session; if test "${coh_usb_input_ready}" = "1"; then echo "[cohesix] USB keyboard input active"; setenv stdin usbkbd,serial; else echo "[cohesix] USB keyboard input unavailable; serial only"; setenv stdin serial; fi; setenv stdout serial,vidconsole; setenv stderr serial,vidconsole'
setenv coh_clear_xhci_handoff_live 'setenv coh_xhci_mmio; setenv coh_xhci_pci_cmd; setenv coh_xhci_handoff_ready; setenv coh_xhci_irq_quiesced; setenv coh_xhci_halted; setenv coh_xhci_handoff_safe; setenv coh_xhci_usbcmd; setenv coh_xhci_usbsts; setenv coh_xhci_iman0'
setenv coh_quiesce_usb 'setenv stdin serial; run coh_clear_xhci_handoff_live; if usb stop; then run coh_clear_xhci_handoff_live; echo "[cohesix] USB host stop requested; xHCI trust tokens cleared before Cohesix cold boot"; else run coh_clear_xhci_handoff_live; echo "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot"; fi'
setenv coh_toggle_logo 'if test "${coh_show_logo}" = "1"; then setenv coh_show_logo 0; echo "[cohesix] HDMI logo disabled"; else setenv coh_show_logo 1; echo "[cohesix] HDMI logo enabled"; fi'
setenv coh_detect_saved_config 'setenv coh_has_saved_config 0; setenv coh_policy_invalid 0; setenv coh_policy_invalid_reason; setenv coh_policy_network_fields 0; if test -n "${coh_net_mode}"; then setenv coh_policy_network_fields 1; fi; if test -n "${coh_net_interface}"; then setenv coh_policy_network_fields 1; fi; if test -n "${coh_static_ip}"; then setenv coh_policy_network_fields 1; fi; if test -n "${coh_static_prefix_len}"; then setenv coh_policy_network_fields 1; fi; if test -n "${coh_static_gateway}"; then setenv coh_policy_network_fields 1; fi; if test -n "${coh_wifi_ssid}"; then setenv coh_policy_network_fields 1; fi; if test -n "${coh_wifi_psk}"; then setenv coh_policy_network_fields 1; fi; if test "${coh_policy_network_fields}" = "1"; then if test "${coh_net_mode}" = "dhcp"; then setenv coh_policy_invalid 0; elif test "${coh_net_mode}" = "static"; then if test -z "${coh_static_ip}"; then setenv coh_policy_invalid 1; setenv coh_policy_invalid_reason static-ip-missing; elif test "${coh_static_ip}" =~ "${coh_ipv4_text_regex}"; then if test -z "${coh_static_prefix_len}"; then setenv coh_policy_invalid 1; setenv coh_policy_invalid_reason static-prefix-missing; elif test "${coh_static_prefix_len}" =~ "${coh_prefix_text_regex}"; then if itest ${coh_static_prefix_len} < 1; then setenv coh_policy_invalid 1; setenv coh_policy_invalid_reason static-prefix-invalid; elif itest ${coh_static_prefix_len} > 32; then setenv coh_policy_invalid 1; setenv coh_policy_invalid_reason static-prefix-invalid; fi; else setenv coh_policy_invalid 1; setenv coh_policy_invalid_reason static-prefix-invalid; fi; else setenv coh_policy_invalid 1; setenv coh_policy_invalid_reason static-ip-invalid; fi; if test "${coh_policy_invalid}" = "0"; then if test -n "${coh_static_gateway}"; then if test "${coh_static_gateway}" =~ "${coh_ipv4_text_regex}"; then setenv coh_policy_invalid 0; else setenv coh_policy_invalid 1; setenv coh_policy_invalid_reason static-gateway-invalid; fi; fi; fi; else setenv coh_policy_invalid 1; setenv coh_policy_invalid_reason net-mode-invalid; fi; if test "${coh_policy_invalid}" = "0"; then if test "${coh_net_interface}" = "wired"; then setenv coh_policy_invalid 0; elif test "${coh_net_interface}" = "wifi"; then if test -z "${coh_wifi_ssid}"; then setenv coh_policy_invalid 1; setenv coh_policy_invalid_reason wifi-ssid-missing; elif test -n "${coh_wifi_psk}"; then if test "${coh_wifi_psk}" =~ "${coh_psk_min_regex}"; then setenv coh_policy_invalid 0; else setenv coh_policy_invalid 1; setenv coh_policy_invalid_reason wifi-psk-too-short; fi; fi; else setenv coh_policy_invalid 1; setenv coh_policy_invalid_reason net-interface-invalid; fi; fi; if test "${coh_policy_invalid}" = "1"; then echo "[cohesix] WARNING: invalid saved network settings (${coh_policy_invalid_reason}); using default settings"; run coh_reset_policy; else if test "${coh_net_mode}" = "dhcp"; then setenv coh_static_ip ""; setenv coh_static_prefix_len ""; setenv coh_static_gateway ""; fi; if test "${coh_net_interface}" = "wired"; then setenv coh_wifi_ssid ""; setenv coh_wifi_psk ""; fi; setenv coh_has_saved_config 1; fi; fi'
setenv coh_load_saved_policy 'run coh_clear_saved_policy; setenv coh_policy_loaded 0; setenv coh_policy_load_state absent; if fatsize mmc 0:1 ${coh_policy_file}; then if itest ${filesize} > 0; then if itest ${filesize} <= ${coh_policy_max_size}; then if fatload mmc 0:1 ${coh_policy_addr} ${coh_policy_file} ${filesize}; then if env import -r -t ${coh_policy_addr} ${filesize} coh_net_mode coh_net_interface coh_static_ip coh_static_prefix_len coh_static_gateway coh_wifi_ssid coh_wifi_psk coh_show_logo; then if test -z "${coh_show_logo}"; then setenv coh_show_logo 1; fi; if test "${coh_show_logo}" = "0"; then setenv coh_policy_loaded 1; setenv coh_policy_load_state loaded; echo "[cohesix] loaded saved settings from ${coh_policy_file}"; elif test "${coh_show_logo}" = "1"; then setenv coh_policy_loaded 1; setenv coh_policy_load_state loaded; echo "[cohesix] loaded saved settings from ${coh_policy_file}"; else setenv coh_policy_load_state invalid; echo "[cohesix] WARNING: invalid coh_show_logo value in ${coh_policy_file}; using default settings"; run coh_clear_saved_policy; fi; else setenv coh_policy_load_state invalid; echo "[cohesix] WARNING: failed to import ${coh_policy_file}; ignoring saved settings"; run coh_clear_saved_policy; fi; else setenv coh_policy_load_state unreadable; echo "[cohesix] WARNING: failed to read ${coh_policy_file}; ignoring saved settings"; run coh_clear_saved_policy; fi; else setenv coh_policy_load_state oversized; echo "[cohesix] WARNING: ${coh_policy_file} exceeds ${coh_policy_max_size} bytes; ignoring saved settings"; fi; else setenv coh_policy_load_state empty; echo "[cohesix] WARNING: ${coh_policy_file} is empty; using default settings"; fi; fi; if test -z "${coh_show_logo}"; then setenv coh_show_logo 1; fi'
setenv coh_persist_policy 'setenv coh_policy_persisted 0; setenv coh_policy_export_size; if env export -t -s ${coh_policy_max_size} ${coh_policy_addr} coh_net_mode coh_net_interface coh_static_ip coh_static_prefix_len coh_static_gateway coh_wifi_ssid coh_wifi_psk coh_show_logo; then setenv coh_policy_export_size ${filesize}; if fatwrite mmc 0:1 ${coh_policy_addr} ${coh_policy_file} ${coh_policy_export_size}; then if fatsize mmc 0:1 ${coh_policy_file}; then if itest ${filesize} == ${coh_policy_export_size}; then if fatload mmc 0:1 ${coh_policy_verify_addr} ${coh_policy_file} ${coh_policy_export_size}; then setenv coh_policy_saved_stdout "${stdout}"; setenv coh_policy_saved_stderr "${stderr}"; setenv stdout nulldev; setenv stderr nulldev; if cmp.b ${coh_policy_addr} ${coh_policy_verify_addr} ${coh_policy_export_size}; then setenv coh_policy_compare_ok 1; else setenv coh_policy_compare_ok 0; fi; setenv stdout "${coh_policy_saved_stdout}"; setenv stderr "${coh_policy_saved_stderr}"; setenv coh_policy_saved_stdout; setenv coh_policy_saved_stderr; if test "${coh_policy_compare_ok}" = "1"; then setenv coh_policy_persisted 1; setenv coh_policy_loaded 1; echo "[cohesix] saved and verified settings in ${coh_policy_file}"; else echo "[cohesix] ERROR: ${coh_policy_file} readback differs; not restarting"; fi; setenv coh_policy_compare_ok; else echo "[cohesix] ERROR: failed to read back ${coh_policy_file}; not restarting"; fi; else echo "[cohesix] ERROR: ${coh_policy_file} size verification failed; not restarting"; fi; else echo "[cohesix] ERROR: failed to size ${coh_policy_file} after write; not restarting"; fi; else echo "[cohesix] ERROR: failed to write ${coh_policy_file}; not restarting"; fi; else echo "[cohesix] ERROR: failed to export bounded saved settings; not restarting"; fi'
setenv coh_show_logo_splash 'if test "${coh_show_logo}" = "1"; then if test "${coh_logo_shown}" != "1"; then cls; if fatload mmc 0:1 ${coh_logo_addr} ${coh_logo_bootstd_file}; then if bmp display ${coh_logo_addr} m m; then echo "[cohesix] loading boot options..."; if test "${coh_logo_delay}" != "0"; then sleep ${coh_logo_delay}; fi; setenv coh_logo_shown 1; else echo "[cohesix] logo draw failed: ${coh_logo_bootstd_file}"; fi; else echo "[cohesix] logo splash skipped: ${coh_logo_bootstd_file}"; fi; fi; fi'
setenv coh_load_runtime_dtb 'setenv coh_boot_error 0; if fatload mmc 0:1 ${coh_dtb_addr} ${coh_dtb_file}; then if fdt addr ${coh_dtb_addr}; then echo "[cohesix] loaded ${coh_dtb_file} to ${coh_dtb_addr}"; else echo "[cohesix] ERROR: failed to select ${coh_dtb_file}"; setenv coh_boot_error 1; fi; else echo "[cohesix] ERROR: failed to load ${coh_dtb_file}"; setenv coh_boot_error 1; fi'
setenv coh_apply_dtb_policy 'if test "${coh_boot_error}" != "1" && test -n "${coh_net_mode}"; then if fdt set /chosen cohesix,net-mode "${coh_net_mode}"; then echo "[cohesix] dtb chosen cohesix,net-mode=${coh_net_mode}"; else echo "[cohesix] ERROR: failed to set cohesix,net-mode"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_net_interface}"; then if fdt set /chosen cohesix,net-interface "${coh_net_interface}"; then echo "[cohesix] dtb chosen cohesix,net-interface=${coh_net_interface}"; else echo "[cohesix] ERROR: failed to set cohesix,net-interface"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_static_ip}"; then if fdt set /chosen cohesix,static-ipv4 "${coh_static_ip}"; then echo "[cohesix] dtb chosen cohesix,static-ipv4=${coh_static_ip}"; else echo "[cohesix] ERROR: failed to set cohesix,static-ipv4"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_static_prefix_len}"; then if fdt set /chosen cohesix,static-prefix-len "${coh_static_prefix_len}"; then echo "[cohesix] dtb chosen cohesix,static-prefix-len=${coh_static_prefix_len}"; else echo "[cohesix] ERROR: failed to set cohesix,static-prefix-len"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_static_gateway}"; then if fdt set /chosen cohesix,static-gateway "${coh_static_gateway}"; then echo "[cohesix] dtb chosen cohesix,static-gateway=${coh_static_gateway}"; else echo "[cohesix] ERROR: failed to set cohesix,static-gateway"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_wifi_ssid}"; then if fdt set /chosen cohesix,wifi-ssid "${coh_wifi_ssid}"; then echo "[cohesix] dtb chosen cohesix,wifi-ssid=<set>"; else echo "[cohesix] ERROR: failed to set cohesix,wifi-ssid"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_wifi_psk}"; then if fdt set /chosen cohesix,wifi-psk "${coh_wifi_psk}"; then echo "[cohesix] dtb chosen cohesix,wifi-psk=<set>"; else echo "[cohesix] ERROR: failed to set cohesix,wifi-psk"; setenv coh_boot_error 1; fi; fi'
setenv coh_emit_policy_summary 'if test "${coh_net_mode}" = "dhcp"; then echo "[cohesix] IPv4: Automatic (DHCP)"; elif test "${coh_net_mode}" = "static"; then echo "[cohesix] IPv4: Manual (static)"; else echo "[cohesix] IPv4: Default settings"; fi; if test "${coh_net_interface}" = "wired"; then echo "[cohesix] Network: Ethernet"; elif test "${coh_net_interface}" = "wifi"; then echo "[cohesix] Network: Wi-Fi"; else echo "[cohesix] Network: Default settings"; fi; if test -n "${coh_static_ip}"; then echo "[cohesix] Address: ${coh_static_ip}/${coh_static_prefix_len}"; fi; if test -n "${coh_static_gateway}"; then echo "[cohesix] Default gateway: ${coh_static_gateway}"; fi; if test -n "${coh_wifi_ssid}"; then echo "[cohesix] Wi-Fi network: Configured (name hidden)"; fi'
setenv coh_load_driver_runtimes 'if fatload mmc 0:1 ${coh_runtime_cpio_addr} ${coh_runtime_cpio_file}; then echo "[cohesix] loaded ${coh_runtime_cpio_file} to ${coh_runtime_cpio_addr}"; else echo "[cohesix] ERROR: failed to load ${coh_runtime_cpio_file}"; setenv coh_boot_error 1; fi'
setenv coh_boot_loaded_image 'run coh_load_runtime_dtb; if test "${coh_boot_error}" = "1"; then echo "[cohesix] ERROR: boot aborted before driver runtime load"; else run coh_load_driver_runtimes; if test "${coh_boot_error}" = "1"; then echo "[cohesix] ERROR: boot aborted before USB quiesce"; else run coh_quiesce_usb; run coh_apply_dtb_policy; if test "${coh_boot_error}" = "1"; then echo "[cohesix] ERROR: boot aborted before kernel handoff"; else echo "[cohesix] loaded ${coh_image} and ${coh_runtime_cpio_file}; bootm with ${coh_dtb_file}"; bootm ${coh_addr} ${coh_runtime_cpio_addr} ${coh_dtb_addr}; echo "[cohesix] returned from image"; fi; fi; fi'
setenv coh_boot_sequence 'run coh_emit_policy_summary; if fatload mmc 0:1 ${coh_addr} ${coh_image}; then run coh_boot_loaded_image; else echo "[cohesix] primary image load failed: ${coh_image}"; if fatload mmc 0:1 ${coh_addr} ${coh_image_fallback}; then setenv coh_image ${coh_image_fallback}; run coh_boot_loaded_image; else echo "[cohesix] ERROR: failed to load ${coh_image} or fallback ${coh_image_fallback} from mmc 0:1"; echo "[cohesix] manual: fatls mmc 0:1"; echo "[cohesix] manual: fatload mmc 0:1 0x10000000 ${coh_image}"; echo "[cohesix] manual: fatload mmc 0:1 0x14000000 ${coh_dtb_file}"; echo "[cohesix] manual: bootm 0x10000000 - 0x14000000"; fi; fi'
setenv coh_prompt_dhcp 'run coh_prepare_input; cls; echo "[cohesix] Network setup (step 1 of 3)"; echo "[cohesix] Choose IPv4 configuration"; echo "  1. Automatic (DHCP)"; echo "  2. Manual (static IPv4)"; echo "  0. Back to boot menu"; echo "  9. Advanced: Open U-Boot shell"; run coh_read_choice; if test "${coh_choice}" = "1"; then setenv coh_net_mode dhcp; setenv coh_static_ip ""; setenv coh_static_prefix_len ""; setenv coh_static_gateway ""; setenv coh_menu_page interface; elif test "${coh_choice}" = "2"; then setenv coh_net_mode static; setenv coh_menu_page interface; elif test "${coh_choice}" = "0"; then run coh_load_saved_policy; setenv coh_menu_page root; elif test "${coh_choice}" = "9"; then setenv coh_menu_running 0; else echo "[cohesix] Invalid selection; choose a listed number"; fi'
setenv coh_prompt_interface 'run coh_prepare_input; cls; echo "[cohesix] Network setup (step 2 of 3)"; echo "[cohesix] Choose network connection"; echo "  1. Ethernet (wired)"; echo "  2. Wi-Fi (wireless)"; echo "  0. Back"; echo "  9. Advanced: Open U-Boot shell"; run coh_read_choice; if test "${coh_choice}" = "1"; then setenv coh_net_interface wired; setenv coh_wifi_ssid ""; setenv coh_wifi_psk ""; if test "${coh_net_mode}" = "static"; then setenv coh_menu_page static; else setenv coh_menu_page confirm; fi; elif test "${coh_choice}" = "2"; then setenv coh_net_interface wifi; setenv coh_menu_page wifi; elif test "${coh_choice}" = "0"; then setenv coh_menu_page dhcp; elif test "${coh_choice}" = "9"; then setenv coh_menu_running 0; else echo "[cohesix] Invalid selection; choose a listed number"; fi'
setenv coh_begin_wifi_secret_input 'setenv stdin usbkbd; setenv stdout vidconsole; setenv stderr vidconsole'
setenv coh_end_wifi_secret_input 'if test "${coh_usb_input_ready}" = "1"; then setenv stdin usbkbd,serial; else setenv stdin serial; fi; setenv stdout serial,vidconsole; setenv stderr serial,vidconsole'
setenv coh_finish_wifi_setup 'if test "${coh_net_mode}" = "static"; then setenv coh_menu_page static; else setenv coh_menu_page confirm; fi'
setenv coh_capture_wifi_credentials 'run coh_prepare_input; if test "${coh_usb_input_ready}" = "1"; then echo "[cohesix] Privacy notice: Wi-Fi network name and password are visible on this display; they are hidden from serial output"; setenv coh_wifi_ssid_new; setenv coh_wifi_psk_new; run coh_begin_wifi_secret_input; askenv coh_wifi_ssid_new "Wi-Fi network name (SSID): " 32; if test -z "${coh_wifi_ssid_new}"; then run coh_end_wifi_secret_input; setenv coh_wifi_ssid_new; setenv coh_wifi_psk_new; echo "[cohesix] Wi-Fi network name is required; existing settings were not changed"; setenv coh_menu_page wifi; else askenv coh_wifi_psk_new "Wi-Fi password (leave blank for an open network): " 64; setenv coh_wifi_input_valid 1; if test -n "${coh_wifi_psk_new}"; then if test "${coh_wifi_psk_new}" =~ "${coh_psk_min_regex}"; then setenv coh_wifi_input_valid 1; else setenv coh_wifi_input_valid 0; fi; fi; run coh_end_wifi_secret_input; if test "${coh_wifi_input_valid}" = "1"; then setenv coh_wifi_ssid "${coh_wifi_ssid_new}"; setenv coh_wifi_psk "${coh_wifi_psk_new}"; echo "[cohesix] Wi-Fi settings captured from the local USB keyboard"; run coh_finish_wifi_setup; else echo "[cohesix] Wi-Fi password must be blank for an open network or at least 8 characters; existing settings were not changed"; setenv coh_menu_page wifi; fi; setenv coh_wifi_ssid_new; setenv coh_wifi_psk_new; setenv coh_wifi_input_valid; fi; else echo "[cohesix] Wi-Fi password entry is unavailable over serial because U-Boot echoes typed input"; if test -n "${coh_wifi_ssid}"; then echo "[cohesix] Existing Wi-Fi settings were not changed"; echo "[cohesix] Connect a USB keyboard or update ${coh_policy_file} on the SD boot partition, then restart"; setenv coh_menu_page wifi; else echo "[cohesix] No Wi-Fi network is configured and local USB input is unavailable"; echo "[cohesix] Connect a USB keyboard or create ${coh_policy_file} on the SD boot partition, then restart"; setenv coh_menu_page interface; fi; fi'
setenv coh_wifi_setup 'run coh_prepare_input; cls; echo "[cohesix] Network setup (step 3 of 3)"; echo "[cohesix] Choose Wi-Fi network"; if test -n "${coh_wifi_ssid}"; then echo "[cohesix] Current Wi-Fi network is configured"; echo "  1. Keep current Wi-Fi settings"; echo "  2. Change Wi-Fi network"; echo "  0. Back"; echo "  9. Advanced: Open U-Boot shell"; run coh_read_choice; if test "${coh_choice}" = "1"; then run coh_finish_wifi_setup; elif test "${coh_choice}" = "2"; then run coh_capture_wifi_credentials; elif test "${coh_choice}" = "0"; then setenv coh_menu_page interface; elif test "${coh_choice}" = "9"; then setenv coh_menu_running 0; else echo "[cohesix] Invalid selection; choose a listed number"; fi; else echo "[cohesix] No Wi-Fi network is configured"; echo "  1. Enter Wi-Fi network"; echo "  0. Back"; echo "  9. Advanced: Open U-Boot shell"; run coh_read_choice; if test "${coh_choice}" = "1"; then run coh_capture_wifi_credentials; elif test "${coh_choice}" = "0"; then setenv coh_menu_page interface; elif test "${coh_choice}" = "9"; then setenv coh_menu_running 0; else echo "[cohesix] Invalid selection; choose a listed number"; fi; fi'
setenv coh_static_setup 'run coh_prepare_input; cls; echo "[cohesix] Network setup: manual IPv4"; echo "  1. Enter manual IPv4 settings"; echo "  0. Back"; echo "  9. Advanced: Open U-Boot shell"; run coh_read_choice; if test "${coh_choice}" = "1"; then askenv coh_static_ip "IPv4 address (for example 192.168.1.50): " 15; if test -z "${coh_static_ip}"; then echo "[cohesix] IPv4 address is required"; elif test "${coh_static_ip}" =~ "${coh_ipv4_text_regex}"; then askenv coh_static_prefix_len "Subnet prefix length (for example 24): " 2; if test -z "${coh_static_prefix_len}"; then echo "[cohesix] Subnet prefix length is required"; elif test "${coh_static_prefix_len}" =~ "${coh_prefix_text_regex}"; then if itest ${coh_static_prefix_len} < 1; then echo "[cohesix] Subnet prefix length must be 1-32"; elif itest ${coh_static_prefix_len} > 32; then echo "[cohesix] Subnet prefix length must be 1-32"; else askenv coh_static_gateway "Default gateway (optional): " 15; if test -z "${coh_static_gateway}"; then setenv coh_menu_page confirm; elif test "${coh_static_gateway}" =~ "${coh_ipv4_text_regex}"; then setenv coh_menu_page confirm; else echo "[cohesix] Default gateway must use dotted-decimal IPv4 syntax"; fi; fi; else echo "[cohesix] Subnet prefix length must contain decimal digits"; fi; else echo "[cohesix] IPv4 address must use dotted-decimal syntax"; fi; elif test "${coh_choice}" = "0"; then setenv coh_menu_page interface; elif test "${coh_choice}" = "9"; then setenv coh_menu_running 0; else echo "[cohesix] Invalid selection; choose a listed number"; fi'
setenv coh_confirm_prompt 'run coh_prepare_input; cls; echo "[cohesix] Review network settings"; run coh_emit_policy_summary; echo "  1. Boot once without saving"; echo "  2. Save settings and restart"; echo "  3. Edit network settings"; echo "  0. Discard changes and return to boot menu"; echo "  9. Advanced: Open U-Boot shell"; run coh_read_choice; if test "${coh_choice}" = "1"; then run coh_boot_sequence; setenv coh_menu_page confirm; elif test "${coh_choice}" = "2"; then run coh_persist_policy; if test "${coh_policy_persisted}" = "1"; then echo "[cohesix] Saved settings verified; restarting"; reset; echo "[cohesix] ERROR: reset returned; settings remain saved"; else echo "[cohesix] Save failed; review settings and retry"; fi; setenv coh_menu_page confirm; elif test "${coh_choice}" = "3"; then setenv coh_menu_page dhcp; elif test "${coh_choice}" = "0"; then run coh_load_saved_policy; setenv coh_menu_page root; elif test "${coh_choice}" = "9"; then setenv coh_menu_running 0; else echo "[cohesix] Invalid selection; choose a listed number"; fi'
setenv coh_confirm_reset 'run coh_prepare_input; cls; echo "[cohesix] Reset saved settings?"; echo "[cohesix] This resets saved network and boot logo settings to defaults"; echo "  1. Confirm reset"; echo "  0. Cancel"; echo "  9. Advanced: Open U-Boot shell"; run coh_read_cancel_choice; if test "${coh_choice}" = "1"; then run coh_clear_saved_policy; setenv coh_show_logo 1; run coh_persist_policy; if test "${coh_policy_persisted}" = "1"; then echo "[cohesix] Saved settings reset to defaults"; else echo "[cohesix] ERROR: Could not reset saved settings; reloading settings from SD"; run coh_load_saved_policy; fi; setenv coh_menu_page root; elif test "${coh_choice}" = "0"; then setenv coh_menu_page root; elif test "${coh_choice}" = "9"; then setenv coh_menu_running 0; else echo "[cohesix] Invalid selection; choose a listed number"; fi'
setenv coh_prompt_root 'run coh_show_logo_splash; run coh_prepare_input; run coh_detect_saved_config; cls; echo "[cohesix] Cohesix boot menu"; if test "${coh_has_saved_config}" = "1"; then echo "[cohesix] Saved network settings loaded"; run coh_emit_policy_summary; echo "  1. Boot with saved settings"; else echo "[cohesix] Default network settings active"; echo "  1. Boot with default settings"; fi; echo "  2. Change network settings"; if test "${coh_show_logo}" = "1"; then echo "  3. Boot logo: On (select to turn off)"; else echo "  3. Boot logo: Off (select to turn on)"; fi; echo "  4. Reset saved settings to defaults"; echo "  5. Save settings and restart"; echo "  9. Advanced: Open U-Boot shell"; run coh_read_choice; if test "${coh_choice}" = "1"; then run coh_boot_sequence; setenv coh_menu_page root; elif test "${coh_choice}" = "2"; then setenv coh_menu_page dhcp; elif test "${coh_choice}" = "3"; then run coh_toggle_logo; elif test "${coh_choice}" = "4"; then setenv coh_menu_page reset; elif test "${coh_choice}" = "5"; then run coh_persist_policy; if test "${coh_policy_persisted}" = "1"; then echo "[cohesix] Saved settings verified; restarting"; reset; echo "[cohesix] ERROR: reset returned; settings remain saved"; else echo "[cohesix] Save failed; remaining at boot menu"; fi; setenv coh_menu_page root; elif test "${coh_choice}" = "9"; then setenv coh_menu_running 0; else echo "[cohesix] Invalid selection; choose a listed number"; fi'
setenv coh_menu_loop 'while test "${coh_menu_running}" = "1"; do if test "${coh_menu_page}" = "root"; then run coh_prompt_root; elif test "${coh_menu_page}" = "dhcp"; then run coh_prompt_dhcp; elif test "${coh_menu_page}" = "interface"; then run coh_prompt_interface; elif test "${coh_menu_page}" = "wifi"; then run coh_wifi_setup; elif test "${coh_menu_page}" = "static"; then run coh_static_setup; elif test "${coh_menu_page}" = "confirm"; then run coh_confirm_prompt; elif test "${coh_menu_page}" = "reset"; then run coh_confirm_reset; else echo "[cohesix] WARNING: invalid menu page ${coh_menu_page}; returning to boot menu"; setenv coh_menu_page root; fi; done'
setenv coh_start_menu 'setenv coh_menu_page root; setenv coh_menu_running 1; run coh_menu_loop'
run coh_force_serial_preboot
run coh_load_saved_policy
run coh_detect_saved_config
run coh_detect_fastboot
run coh_report_fastboot_miss
run coh_start_menu
EOF
    sed -i '' "s/__COH_IMAGE__/${coh_image}/g" "$out"
    sed -i '' "s/__COH_IMAGE_FALLBACK__/${fallback_image}/g" "$out"
    sed -i '' "s/__COH_MENU_INPUT__/${U_BOOT_MENU_INPUT}/g" "$out"
    sed -i '' "s/__COH_LOGO_FILE__/${COHESIX_LOGO_STAGE_NAME}/g" "$out"
    sed -i '' "s/__COH_BOOTSTD_LOGO_FILE__/${BOOTSTD_LOGO_STAGE_NAME}/g" "$out"
    sed -i '' "s/__COH_RUNTIME_CPIO_FILE__/${DRIVER_RUNTIME_CPIO_STAGE_NAME}/g" "$out"
}

write_linux_wifi_debug_helpers() {
    local cmdline_path="${STAGE_DIR}/${BRCMFMAC_CMDLINE_STAGE_NAME}"
    local script_path="${STAGE_DIR}/${BRCMFMAC_DYNAMIC_DEBUG_STAGE_NAME}"

    cat >"${cmdline_path}" <<'EOF'
ignore_loglevel loglevel=8 initcall_debug brcmfmac.debug=0x001fffff dyndbg="file drivers/net/wireless/broadcom/brcm80211/brcmfmac/* +p; file drivers/net/wireless/broadcom/brcm80211/brcmutil/* +p; file drivers/mmc/core/* +p; file drivers/mmc/host/sdhci* +p"
EOF

    cat >"${script_path}" <<'EOF'
#!/bin/sh
# Author: Lukas Bower
# Purpose: Enable Linux brcmfmac dynamic debug for Pi 4 known-good Wi-Fi boot captures.
# Copyright 2026 Lukas Bower

set -eu

mount -t debugfs none /sys/kernel/debug 2>/dev/null || true

control=/sys/kernel/debug/dynamic_debug/control
if [ ! -w "$control" ]; then
    echo "brcmfmac dynamic debug unavailable: $control is not writable" >&2
    exit 1
fi

printf '%s\n' 'file drivers/net/wireless/broadcom/brcm80211/brcmfmac/* +p' >"$control"
printf '%s\n' 'file drivers/net/wireless/broadcom/brcm80211/brcmutil/* +p' >"$control"
printf '%s\n' 'file drivers/mmc/core/* +p' >"$control"
printf '%s\n' 'file drivers/mmc/host/sdhci* +p' >"$control"

dmesg -n 8 2>/dev/null || true

if command -v modprobe >/dev/null 2>&1; then
    modprobe -r brcmfmac brcmutil 2>/dev/null || true
    modprobe brcmfmac debug=0x001fffff || modprobe brcmfmac
fi

echo "brcmfmac dynamic debug enabled; capture with: dmesg -w"
EOF
    chmod +x "${script_path}"

    grep -q 'brcmfmac.debug=0x001fffff' "${cmdline_path}" || fail "brcmfmac command line helper missing debug mask"
    grep -q 'dynamic_debug/control' "${script_path}" || fail "brcmfmac dynamic debug helper missing debugfs control path"
}

assert_driver_runtime_elf_budgets() {
    local runtime_artifact_dir="$1"
    local manifest_json="${GENERATED_CONFIG_DIR}/root_task_resolved.json"
    require_file "$manifest_json"
    python3 - "$manifest_json" "$runtime_artifact_dir" <<'PY'
import json
import os
import struct
import sys

PAGE_BYTES = 4096
PT_LOAD = 1


def load_span_pages(path: str) -> int:
    with open(path, "rb") as handle:
        image = handle.read()
    if len(image) < 64 or image[:4] != b"\x7fELF":
        raise ValueError(f"{path}: not an ELF image")
    if image[4] != 2 or image[5] != 1:
        raise ValueError(f"{path}: expected little-endian ELF64")
    phoff = struct.unpack_from("<Q", image, 32)[0]
    phentsize = struct.unpack_from("<H", image, 54)[0]
    phnum = struct.unpack_from("<H", image, 56)[0]
    if phentsize < 56 or phnum == 0:
        raise ValueError(f"{path}: invalid program header table")
    min_vaddr = None
    max_vaddr = 0
    for index in range(phnum):
        base = phoff + index * phentsize
        if base + 56 > len(image):
            raise ValueError(f"{path}: truncated program header table")
        p_type = struct.unpack_from("<I", image, base)[0]
        if p_type != PT_LOAD:
            continue
        p_vaddr = struct.unpack_from("<Q", image, base + 16)[0]
        p_memsz = struct.unpack_from("<Q", image, base + 40)[0]
        if p_memsz == 0:
            continue
        page_base = p_vaddr & ~(PAGE_BYTES - 1)
        page_end = (p_vaddr + p_memsz + PAGE_BYTES - 1) & ~(PAGE_BYTES - 1)
        min_vaddr = page_base if min_vaddr is None else min(min_vaddr, page_base)
        max_vaddr = max(max_vaddr, page_end)
    if min_vaddr is None or max_vaddr <= min_vaddr:
        raise ValueError(f"{path}: no loadable segment span")
    return (max_vaddr - min_vaddr) // PAGE_BYTES


manifest_path, runtime_dir = sys.argv[1], sys.argv[2]
with open(manifest_path, "r", encoding="utf-8") as handle:
    manifest = json.load(handle)

errors = []
for image in manifest["root_task"]["driver_images"]["images"]:
    artifact = os.path.basename(image["artifact"])
    path = os.path.join(runtime_dir, artifact)
    declared_pages = int(image["code-pages"])
    if not os.path.isfile(path):
        errors.append(f"{artifact}: runtime artifact is missing from {runtime_dir}")
        continue
    actual_pages = load_span_pages(path)
    if actual_pages > declared_pages:
        errors.append(
            f"{artifact}: ELF span requires {actual_pages} pages but manifest declares "
            f"{declared_pages} code-pages"
        )

if errors:
    for error in errors:
        print(error, file=sys.stderr)
    sys.exit(1)
PY
}

package_driver_runtime_raw_cpio() {
    local raw_cpio="$1"
    local artifact_dir="${2:-${ROOT_DIR}/target/aarch64-unknown-none/release}"
    local cpio_bin
    local raw_dir
    raw_dir="$(dirname "$raw_cpio")"
    local runtime_root="${raw_dir}/driver-runtime-root"
    local runtime_bin="${runtime_root}/cohesix/bin"
    local strip_tool
    local bin

    cpio_bin="$(resolve_cpio)"
    configure_cpio_path "$cpio_bin"

    assert_driver_runtime_elf_budgets "$artifact_dir"
    strip_tool="$(find_aarch64_strip)"
    mkdir -p "$raw_dir"
    rm -rf "$runtime_root"
    mkdir -p "$runtime_bin"
    for bin in \
        pi4-driver-serial \
        pi4-driver-usb \
        pi4-driver-hdmi \
        pi4-driver-genet \
        pi4-driver-cyw43 \
        pi4-driver-sdio \
        pi4-driver-pcie
    do
        require_file "${artifact_dir}/${bin}"
        install -m 0755 "${artifact_dir}/${bin}" "${runtime_bin}/${bin}"
        "$strip_tool" \
            --strip-all \
            --remove-section=.comment \
            --remove-section=.eh_frame \
            --remove-section=.eh_frame_hdr \
            "${runtime_bin}/${bin}"
        log "Staged isolated driver runtime: ${bin}"
    done
    log "Keeping per-role Pi4 driver runtime images for manifest artifact identity"

    (
        cd "$runtime_root"
        find cohesix -print | LC_ALL=C sort | cpio --reproducible -o -H newc > "$raw_cpio"
    )
    require_file "$raw_cpio"
    verify_driver_runtime_cpio_entries "$raw_cpio"
    log "Packaged Pi4 driver runtime raw CPIO at ${raw_cpio}"
}

verify_driver_runtime_cpio_entries() {
    local raw_cpio="$1"
    local entries
    local entry
    entries="$(LC_ALL=C cpio -it < "$raw_cpio" 2>/dev/null)"
    for entry in \
        cohesix/bin/pi4-driver-serial \
        cohesix/bin/pi4-driver-usb \
        cohesix/bin/pi4-driver-hdmi \
        cohesix/bin/pi4-driver-genet \
        cohesix/bin/pi4-driver-cyw43 \
        cohesix/bin/pi4-driver-sdio \
        cohesix/bin/pi4-driver-pcie
    do
        grep -Fxq "$entry" <<<"$entries" || fail "missing Pi4 driver runtime artifact in CPIO: ${entry}"
    done
    if grep -Fxq "cohesix/bin/pi4-driver-runtime" <<<"$entries"; then
        fail "generic Pi4 driver runtime artifact is not a manifest-declared per-role image"
    fi
}

stage_driver_runtime_payload() {
    local mkimage_bin="$1"
    mkdir -p "$STAGE_DIR"
    local stage_dir_abs
    stage_dir_abs="$(cd "$STAGE_DIR" && pwd)"
    local raw_cpio="${stage_dir_abs}/cohesix-driver-runtimes.cpio"

    package_driver_runtime_raw_cpio "$raw_cpio"
    "$mkimage_bin" \
      -A arm64 \
      -T ramdisk \
      -C none \
      -n "Cohesix Pi4 driver runtimes" \
      -d "$raw_cpio" \
      "${stage_dir_abs}/${DRIVER_RUNTIME_CPIO_STAGE_NAME}" \
      >/dev/null
    require_file "${stage_dir_abs}/${DRIVER_RUNTIME_CPIO_STAGE_NAME}"
    log "Staged Pi4 driver runtime payload at ${stage_dir_abs}/${DRIVER_RUNTIME_CPIO_STAGE_NAME}"
}

write_pi4_runtime_dma_build_proof() {
    local proof_path="${STAGE_DIR}/pi4-runtime-dma-proof.env"
    local manifest_json="${GENERATED_CONFIG_DIR}/root_task_resolved.json"
    local runtime_raw="${STAGE_DIR}/cohesix-driver-runtimes.cpio"
    local runtime_uimg="${STAGE_DIR}/${DRIVER_RUNTIME_CPIO_STAGE_NAME}"
    local staged_image="${STAGE_DIR}/${COHESIX_IMAGE_NAME}"
    local image_identity="${STAGE_DIR}/${PI4_IMAGE_IDENTITY_STAGE_NAME}"
    local expected_root_elf="$EXACT_ROOT_ELF"
    local timestamp

    require_file "$manifest_json"
    require_file "$runtime_raw"
    require_file "$runtime_uimg"
    require_file "$staged_image"
    require_file "$image_identity"
    require_file "$expected_root_elf"

    timestamp="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
    {
        printf "PI4_RUNTIME_DMA_PROOF=target-build\n"
        printf "PI4_RUNTIME_DMA_PROOF_REASON=stage-only-no-live-serial\n"
        printf "PI4_RUNTIME_DMA_PROFILE=bounded-no-iommu\n"
        printf "PI4_RUNTIME_DMA_COUNTER_PROOF=not-live\n"
        printf "PI4_RUNTIME_DMA_PROOF_CREATED_AT_UTC=%s\n" "$timestamp"
        printf "PI4_RUNTIME_DMA_MANIFEST=%s\n" "$manifest_json"
        printf "PI4_RUNTIME_DMA_MANIFEST_SHA256=%s\n" "$(shasum -a 256 "$manifest_json" | awk '{print $1}')"
        printf "PI4_RUNTIME_DMA_RUNTIME_CPIO=%s\n" "$runtime_raw"
        printf "PI4_RUNTIME_DMA_RUNTIME_CPIO_SHA256=%s\n" "$(shasum -a 256 "$runtime_raw" | awk '{print $1}')"
        printf "PI4_RUNTIME_DMA_RUNTIME_UIMAGE=%s\n" "$runtime_uimg"
        printf "PI4_RUNTIME_DMA_RUNTIME_UIMAGE_SHA256=%s\n" "$(shasum -a 256 "$runtime_uimg" | awk '{print $1}')"
        printf "PI4_RUNTIME_DMA_STAGED_IMAGE=%s\n" "$staged_image"
        printf "PI4_RUNTIME_DMA_STAGED_IMAGE_SHA256=%s\n" "$(shasum -a 256 "$staged_image" | awk '{print $1}')"
        printf "PI4_IMAGE_IDENTITY_SCHEME=cohesix-pi4-image-identity/v2\n"
        printf "PI4_IMAGE_IDENTITY_GIT_COMMIT=%s\n" "$EXACT_GIT_COMMIT"
        printf "PI4_IMAGE_IDENTITY_BUILD_TIMESTAMP=%s\n" "$EXACT_BUILD_TIMESTAMP"
        printf "PI4_IMAGE_IDENTITY_BUILD_ID=%s\n" "$EXACT_BUILD_ID"
        printf "PI4_IMAGE_IDENTITY_SOURCE_TREE_CLEAN=yes\n"
        printf "PI4_IMAGE_IDENTITY_METADATA=%s\n" "$image_identity"
        printf "PI4_IMAGE_IDENTITY_METADATA_SHA256=%s\n" "$(shasum -a 256 "$image_identity" | awk '{print $1}')"
        printf "PI4_IMAGE_IDENTITY_ROOT_ELF_SHA256=%s\n" "$(shasum -a 256 "$expected_root_elf" | awk '{print $1}')"
        printf "PI4_IMAGE_IDENTITY_ROOT_CPIO_SHA256=%s\n" "$(shasum -a 256 "$EXACT_ROOT_CPIO" | awk '{print $1}')"
    } >"$proof_path"
    require_file "$proof_path"
    log "Wrote Pi4 runtime/DMA stage-only proof at ${proof_path}"
}

stage_sd_payload() {
    local mkimage_bin="$1"
    local sel4_image="$EXACT_PI4_IMAGE"
    local stage_overlays="${STAGE_DIR}/overlays"
    local staged_image="${STAGE_DIR}/${COHESIX_IMAGE_NAME}"
    local fallback_image="${STAGE_DIR}/${SEL4_UPSTREAM_IMAGE_NAME}"
    local unsealed_image="${STAGE_DIR}/.${COHESIX_IMAGE_NAME}.unsealed"

    require_file "$sel4_image"
    require_file "$U_BOOT_BIN"
    require_dir "$FIRMWARE_DIR"

    rm -rf "$STAGE_DIR"
    mkdir -p "$stage_overlays"

    cp -f "${FIRMWARE_DIR}/start4.elf" "${STAGE_DIR}/start4.elf"
    cp -f "${FIRMWARE_DIR}/fixup4.dat" "${STAGE_DIR}/fixup4.dat"
    stage_pi4_dtb "${FIRMWARE_DIR}/bcm2711-rpi-4-b.dtb" "${STAGE_DIR}/bcm2711-rpi-4-b.dtb"
    cp -f "${FIRMWARE_DIR}/overlays/miniuart-bt.dtbo" "${stage_overlays}/miniuart-bt.dtbo"
    cp -f "${FIRMWARE_DIR}/overlays/upstream-pi4.dtbo" "${stage_overlays}/upstream-pi4.dtbo"
    cp -f "$U_BOOT_BIN" "${STAGE_DIR}/u-boot.bin"
    # Keep the unsealed wrapper hidden until the complete final image passes
    # marker-section, normalized-identity, and U-Boot CRC verification.
    cp -f "$sel4_image" "$unsealed_image"
    stage_uboot_logo "${STAGE_DIR}/${COHESIX_LOGO_STAGE_NAME}"
    if [[ -f "${STAGE_DIR}/${COHESIX_LOGO_STAGE_NAME}" ]]; then
        cp -f "${STAGE_DIR}/${COHESIX_LOGO_STAGE_NAME}" "${STAGE_DIR}/${BOOTSTD_LOGO_STAGE_NAME}"
    fi
    stage_driver_runtime_payload "$mkimage_bin"
    write_linux_wifi_debug_helpers

    cat > "${STAGE_DIR}/config.txt" <<EOF
arm_64bit=1
arm_boost=1
enable_uart=1
enable_gic=1
disable_overscan=1
kernel=u-boot.bin
dtoverlay=upstream-pi4
# Keep mini-UART on GPIO14/15 to match seL4 bcm2711 serial1 console routing.
core_freq=250
total_mem=${PI4_TOTAL_MEM_MB}
EOF
    ! grep -q '^uart_2ndstage=1$' "${STAGE_DIR}/config.txt" || fail "Pi firmware second-stage UART logging must stay disabled for clean U-Boot/HDMI evidence"

    write_boot_cmd "${STAGE_DIR}/boot.cmd" "${COHESIX_IMAGE_NAME}" "${SEL4_UPSTREAM_IMAGE_NAME}"
    verify_boot_cmd_handoff "${STAGE_DIR}/boot.cmd"
    "$mkimage_bin" \
      -A arm64 \
      -T script \
      -C none \
      -n "Cohesix Pi4 autoboot" \
      -d "${STAGE_DIR}/boot.cmd" \
      "${STAGE_DIR}/boot.scr.uimg" \
      >/dev/null

    cat > "${STAGE_DIR}/cohesix_boot_state.txt" <<EOF
cohesix_boot_stage=prepared
cohesix_boot_bytes=0
cohesix_boot_image=${COHESIX_IMAGE_NAME}
EOF

    # Identity sealing is the final transformation of the boot image. No
    # repack, padding, or mkimage operation may mutate it after this point.
    seal_staged_pi4_image "$unsealed_image" "$staged_image" "$fallback_image"
    "$mkimage_bin" -l "$staged_image" >/dev/null || \
      fail "mkimage rejected the sealed primary Pi image"
    "$mkimage_bin" -l "$fallback_image" >/dev/null || \
      fail "mkimage rejected the sealed fallback Pi image"
    write_pi4_runtime_dma_build_proof
    verify_final_staged_pi4_image "$mkimage_bin"
    require_file "${STAGE_DIR}/boot.scr.uimg"
    log "Staged Pi4 payload at ${STAGE_DIR}"
}

flash_sd_card() {
    local disk="$1"
    local wait_attempts=20
    local policy_file="cohesix.env"
    local preserved_policy=""
    local target_identity=""
    local current_identity=""
    local validation_layout="canonical"
    local part=""
    local exact_part=""
    local preflash_volume=""
    local volume=""
    local current_volume=""

    command -v diskutil >/dev/null 2>&1 || fail "diskutil not found"
    command -v rsync >/dev/null 2>&1 || fail "rsync not found"
    command -v cmp >/dev/null 2>&1 || fail "cmp not found"

    [[ "$disk" =~ ^/dev/disk[0-9]+$ ]] || \
      fail "--flash-disk must name one explicit whole disk such as /dev/disk16"
    require_flash_session_unlocked
    start_flash_caffeinate
    require_flash_session_unlocked
    if [[ "${INITIALIZE_DISK}" -eq 1 ]]; then
        validation_layout="initialize"
    fi
    target_identity="$(validated_flash_target_identity "$disk" "$validation_layout")" || \
      fail "refusing unsafe flash target: ${disk}"

    if part="$(canonical_flash_partition "$disk" "$DISK_LABEL" 2>/dev/null)"; then
        diskutil mount "$part" >/dev/null 2>&1 || true
        preflash_volume="$(validated_flash_partition_mount "$disk" "$part" "$DISK_LABEL")" || \
          fail "failed to validate the exact ${DISK_LABEL} child ${part}"
        [[ -n "$preflash_volume" && -d "$preflash_volume" ]] || \
          fail "failed to mount the exact ${DISK_LABEL} child ${part}"
    elif [[ "${INITIALIZE_DISK}" -eq 0 ]]; then
        fail "${disk} must already contain exactly one MBR FAT32 ${DISK_LABEL} partition; use --initialize-disk only for intentional first-time media setup"
    else
        part="${disk}s1"
    fi

    if [[ -n "${POLICY_RECOVERY_FILE}" ]]; then
        [[ -z "$preflash_volume" || ! -s "${preflash_volume}/${policy_file}" ]] || \
          fail "--policy-recovery-file refuses to replace an existing non-empty ${policy_file}"
        [[ -f "${POLICY_RECOVERY_FILE}" ]] || \
          fail "--policy-recovery-file must name a regular file"
        [[ -r "${POLICY_RECOVERY_FILE}" && -s "${POLICY_RECOVERY_FILE}" ]] || \
          fail "--policy-recovery-file must be readable and non-empty"
        local recovery_size
        recovery_size="$(stat -f '%z' "${POLICY_RECOVERY_FILE}")"
        [[ "$recovery_size" =~ ^[0-9]+$ && "$recovery_size" -le 384 ]] || \
          fail "--policy-recovery-file exceeds the 384-byte Cohesix policy bound"
        preserved_policy="$(mktemp "${TMPDIR:-/tmp}/cohesix-policy.XXXXXX")"
        chmod 600 "$preserved_policy"
        cp -f "${POLICY_RECOVERY_FILE}" "$preserved_policy"
        chmod 600 "$preserved_policy"
        PRESERVED_POLICY_TEMP="$preserved_policy"
        POLICY_RECOVERY_CONSUMED_FILE="${POLICY_RECOVERY_FILE}"
        log "Using explicit private Cohesix policy recovery file"
    elif [[ -n "$preflash_volume" && -f "${preflash_volume}/${policy_file}" && -s "${preflash_volume}/${policy_file}" ]]; then
        local existing_policy_size
        existing_policy_size="$(stat -f '%z' "${preflash_volume}/${policy_file}")"
        [[ "$existing_policy_size" =~ ^[0-9]+$ && "$existing_policy_size" -le 384 ]] || \
          fail "existing ${policy_file} exceeds the 384-byte Cohesix policy bound"
        preserved_policy="$(mktemp "${TMPDIR:-/tmp}/cohesix-policy.XXXXXX")"
        chmod 600 "$preserved_policy"
        cp -f "${preflash_volume}/${policy_file}" "$preserved_policy"
        chmod 600 "$preserved_policy"
        PRESERVED_POLICY_TEMP="$preserved_policy"
        log "Preserving existing Cohesix U-Boot policy file ${policy_file} across flash"
    fi

    current_identity="$(validated_flash_target_identity "$disk" "$validation_layout")" || \
      fail "flash target disappeared before the critical section: ${disk}"
    [[ "$current_identity" == "$target_identity" ]] || \
      fail "flash target identity changed before the critical section: ${disk}"

    if [[ "${INITIALIZE_DISK}" -eq 1 ]]; then
        require_flash_session_unlocked
        log "Initializing ${disk} as one MBR/FAT32 ${DISK_LABEL} partition (explicit opt-in)"
        FLASH_MEDIA_MUTATION_STARTED=1
        if ! diskutil eraseDisk FAT32 "$DISK_LABEL" MBRFormat "$disk" >/dev/null; then
            fail "explicit initialization failed for ${disk}; policy recovery was retained"
        fi
        if ! resolve_exact_flash_partition_after_initialize \
          "$disk" "$part" "$DISK_LABEL" "$target_identity" "$wait_attempts"; then
            fail "exact target ${disk}/${part} disappeared or changed after initialization; refusing to select another disk"
        fi
        part="$FLASH_PARTITION_DEVICE"
        volume="$FLASH_PARTITION_MOUNT"
    else
        exact_part="$(canonical_flash_partition "$disk" "$DISK_LABEL")" || \
          fail "canonical ${DISK_LABEL} partition disappeared before copy"
        [[ "$exact_part" == "$part" ]] || \
          fail "canonical ${DISK_LABEL} partition changed before copy"
        volume="$(validated_flash_partition_mount "$disk" "$part" "$DISK_LABEL")" || \
          fail "canonical ${DISK_LABEL} partition identity changed before copy"
        [[ "$volume" == "$preflash_volume" && -d "$volume" ]] || \
          fail "canonical ${DISK_LABEL} mount changed before copy"
        FLASH_PARTITION_DEVICE="$part"
        FLASH_PARTITION_MOUNT="$volume"
        require_flash_session_unlocked
        FLASH_MEDIA_MUTATION_STARTED=1
        log "Refreshing ${disk} in place through mounted exact child ${part}; partition map and FAT volume are retained"
    fi
    [[ -n "$part" && -d "$volume" ]] || fail "exact flash volume is unavailable"
    disable_spotlight_for_flash_volume "$volume"

    COPYFILE_DISABLE=1 rsync -a --delete \
      --exclude=".Spotlight-V100" \
      --exclude=".fseventsd" \
      --exclude=".Trashes" \
      --exclude=".metadata_never_index" \
      --exclude="._*" \
      --exclude="${policy_file}" \
      "${STAGE_DIR}/" "${volume}/"

    if [[ -n "$preserved_policy" && -f "$preserved_policy" ]]; then
        cp -f "$preserved_policy" "${volume}/${policy_file}"
        chmod 600 "${volume}/${policy_file}" 2>/dev/null || true
        log "Restored preserved Cohesix U-Boot policy file ${policy_file}"
    fi

    disable_spotlight_for_flash_volume "$volume"
    find "${volume}" -xdev -name '._*' -type f -delete 2>/dev/null || true

    sync

    verify_flashed_stage_files "$volume"
    if [[ -n "$preserved_policy" && -f "$preserved_policy" ]]; then
        cmp -s "$preserved_policy" "${volume}/${policy_file}" || \
          fail "preserved ${policy_file} mismatch after flash"
    fi

    current_identity="$(validated_flash_target_identity "$disk")" || \
      fail "flash target disappeared before readback completion: ${disk}"
    [[ "$current_identity" == "$target_identity" ]] || \
      fail "flash target identity changed before readback completion: ${disk}"
    exact_part="$(canonical_flash_partition "$disk" "$DISK_LABEL")" || \
      fail "canonical ${DISK_LABEL} partition disappeared before readback completion"
    [[ "$exact_part" == "$part" ]] || \
      fail "canonical ${DISK_LABEL} partition changed before readback completion"
    current_volume="$(validated_flash_partition_mount "$disk" "$part" "$DISK_LABEL")" || \
      fail "canonical ${DISK_LABEL} mount identity changed before readback completion"
    [[ "$current_volume" == "$volume" && -d "$current_volume" ]] || \
      fail "canonical ${DISK_LABEL} mount changed before readback completion"

    unmount_flashed_disk "$disk" "$volume"
    FLASH_MEDIA_MUTATION_STARTED=0
    stop_flash_caffeinate
    if [[ -n "$preserved_policy" && -f "$preserved_policy" ]]; then
        rm -f "$preserved_policy"
        preserved_policy=""
        PRESERVED_POLICY_TEMP=""
    fi
    if [[ -n "${POLICY_RECOVERY_CONSUMED_FILE}" ]]; then
        rm -f "${POLICY_RECOVERY_CONSUMED_FILE}"
        POLICY_RECOVERY_CONSUMED_FILE=""
        log "Removed consumed private policy recovery file"
    fi
    log "Flash complete and unmounted: ${disk}"
}

validated_flash_target_identity() {
    local disk="$1"
    local layout="${2:-canonical}"
    local disk_basename="${disk#/dev/}"

    diskutil info -plist "$disk" 2>/dev/null | python3 -c '
import hashlib
import json
import plistlib
import sys

disk = sys.argv[1]
identifier = sys.argv[2]
layout = sys.argv[3]
try:
    info = plistlib.loads(sys.stdin.buffer.read())
except Exception as error:
    print(f"cannot read disk identity for {disk}: {error}", file=sys.stderr)
    raise SystemExit(2)

def require(condition, message):
    if not condition:
        print(f"unsafe flash target {disk}: {message}", file=sys.stderr)
        raise SystemExit(2)

require(info.get("DeviceIdentifier") == identifier, "device identifier changed")
require(info.get("DeviceNode") == disk, "device node changed")
require(info.get("WholeDisk") is True, "target is not a whole disk")
require(info.get("ParentWholeDisk") == identifier, "whole-disk parent mismatch")
if layout == "canonical":
    require(info.get("Content") == "FDisk_partition_scheme", "partition map is not MBR")
elif layout != "initialize":
    require(False, f"unknown validation layout {layout!r}")
require(info.get("Writable") is True and info.get("WritableMedia") is True, "media is read-only")
require(
    info.get("RemovableMediaOrExternalDevice") is True
    and (info.get("Removable") is True or info.get("Ejectable") is True),
    "media is neither removable nor ejectable",
)
require(info.get("VirtualOrPhysical") == "Physical", "target is not physical media")
require(info.get("SystemImage") is not True, "target is a system image")
require(info.get("OSInternalMedia") is not True, "target is OS-internal media")
require(isinstance(info.get("TotalSize"), int) and info["TotalSize"] > 0, "media size is invalid")

stable = {
    key: info.get(key)
    for key in (
        "DeviceIdentifier",
        "DeviceNode",
        "ParentWholeDisk",
        "TotalSize",
        "IOKitSize",
        "DeviceBlockSize",
        "MediaName",
        "BusProtocol",
        "DeviceTreePath",
        "IORegistryEntryName",
    )
}
encoded = json.dumps(stable, sort_keys=True, separators=(",", ":")).encode()
print(hashlib.sha256(encoded).hexdigest())
' "$disk" "$disk_basename" "$layout"
}

canonical_flash_partition() {
    local disk="$1"
    local label="$2"
    local disk_basename="${disk#/dev/}"

    diskutil list -plist "$disk" 2>/dev/null | python3 -c '
import plistlib
import sys

identifier = sys.argv[1]
label = sys.argv[2]
try:
    listing = plistlib.loads(sys.stdin.buffer.read())
except Exception as error:
    print(f"cannot read partition list for {identifier}: {error}", file=sys.stderr)
    raise SystemExit(2)

matches = [
    disk
    for disk in listing.get("AllDisksAndPartitions", [])
    if disk.get("DeviceIdentifier") == identifier
]
if len(matches) != 1:
    print(f"expected one exact whole-disk record for {identifier}", file=sys.stderr)
    raise SystemExit(2)
partitions = matches[0].get("Partitions", [])
expected = f"{identifier}s1"
if len(partitions) != 1:
    print(f"{identifier} must contain exactly one partition", file=sys.stderr)
    raise SystemExit(2)
partition = partitions[0]
if (
    partition.get("DeviceIdentifier") != expected
    or partition.get("Content") != "DOS_FAT_32"
    or partition.get("VolumeName") != label
):
    print(f"{identifier} does not contain exact FAT32 {label} child {expected}", file=sys.stderr)
    raise SystemExit(2)
print(f"/dev/{expected}")
' "$disk_basename" "$label"
}

validated_flash_partition_mount() {
    local disk="$1"
    local part="$2"
    local label="$3"
    local disk_basename="${disk#/dev/}"
    local part_basename="${part#/dev/}"

    diskutil info -plist "$part" 2>/dev/null | python3 -c '
import plistlib
import sys

disk = sys.argv[1]
partition = sys.argv[2]
label = sys.argv[3]
try:
    info = plistlib.loads(sys.stdin.buffer.read())
except Exception as error:
    print(f"cannot read partition identity for {partition}: {error}", file=sys.stderr)
    raise SystemExit(2)

checks = (
    (info.get("DeviceIdentifier") == partition, "partition identifier changed"),
    (info.get("DeviceNode") == f"/dev/{partition}", "partition node changed"),
    (info.get("WholeDisk") is False, "child unexpectedly became a whole disk"),
    (info.get("ParentWholeDisk") == disk, "partition parent changed"),
    (info.get("Content") == "DOS_FAT_32", "partition is not FAT32"),
    (info.get("VolumeName") == label, "partition label changed"),
    (
        info.get("Writable") is True and info.get("WritableMedia") is True,
        "partition is read-only",
    ),
)
for valid, message in checks:
    if not valid:
        print(f"invalid flash partition /dev/{partition}: {message}", file=sys.stderr)
        raise SystemExit(2)
mount = info.get("MountPoint")
if isinstance(mount, str):
    print(mount)
' "$disk_basename" "$part_basename" "$label"
}

FLASH_PARTITION_DEVICE=""
FLASH_PARTITION_MOUNT=""
resolve_exact_flash_partition_after_initialize() {
    local disk="$1"
    local part="$2"
    local label="$3"
    local expected_identity="$4"
    local wait_attempts="$5"
    local attempt
    local identity
    local exact_part
    local volume

    FLASH_PARTITION_DEVICE=""
    FLASH_PARTITION_MOUNT=""
    for attempt in $(seq 1 "$wait_attempts"); do
        identity="$(validated_flash_target_identity "$disk" 2>/dev/null)" || identity=""
        if [[ -n "$identity" && "$identity" == "$expected_identity" ]]; then
            exact_part="$(canonical_flash_partition "$disk" "$label" 2>/dev/null)" || exact_part=""
            if [[ "$exact_part" == "$part" ]]; then
                diskutil mount "$part" >/dev/null 2>&1 || true
                volume="$(validated_flash_partition_mount "$disk" "$part" "$label" 2>/dev/null)" || volume=""
                if [[ -n "$volume" && -d "$volume" ]]; then
                    FLASH_PARTITION_DEVICE="$part"
                    FLASH_PARTITION_MOUNT="$volume"
                    return 0
                fi
            fi
        fi
        sleep 1
    done
    return 1
}

verify_flashed_stage_files() {
    local volume="$1"
    local staged
    local relative
    local target
    local verified=0

    while IFS= read -r -d '' staged; do
        relative="${staged#${STAGE_DIR}/}"
        target="${volume}/${relative}"
        [[ -f "$target" ]] || fail "staged file missing after flash: ${relative}"
        cmp -s "$staged" "$target" || fail "staged file mismatch after flash: ${relative}"
        verified=$((verified + 1))
    done < <(find "$STAGE_DIR" -type f -print0)
    [[ "$verified" -gt 0 ]] || fail "stage contains no regular files to verify"
    log "Verified all ${verified} staged regular files on the flash target"
}

require_flash_session_unlocked() {
    command -v ioreg >/dev/null 2>&1 || fail "ioreg not found"

    if ! ioreg -n Root -d1 -a 2>/dev/null | python3 -c '
import os
import plistlib
import sys

try:
    root = plistlib.loads(sys.stdin.buffer.read())
except Exception as error:
    print(f"cannot inspect console lock state: {error}", file=sys.stderr)
    raise SystemExit(2)

uid = os.getuid()
sessions = [
    session
    for session in root.get("IOConsoleUsers", [])
    if session.get("kCGSSessionOnConsoleKey") is True
    and session.get("kCGSessionLoginDoneKey") is True
    and session.get("kCGSSessionUserIDKey") == uid
]
if (
    len(sessions) != 1
    or root.get("IOConsoleLocked") is not False
    or sessions[0].get("CGSSessionScreenIsLocked") is True
):
    print(
        "the active macOS console must be unlocked before flashing; "
        "a locked loginwindow can eject removable media during mount",
        file=sys.stderr,
    )
    raise SystemExit(2)
'; then
        fail "refusing to flash while the active macOS console is locked"
    fi
}

start_flash_caffeinate() {
    command -v caffeinate >/dev/null 2>&1 || fail "caffeinate not found"
    [[ -z "${FLASH_CAFFEINATE_PID:-}" ]] || fail "flash caffeinate guard is already active"

    caffeinate -dimsu -t 3600 -w "$$" >/dev/null 2>&1 &
    FLASH_CAFFEINATE_PID=$!
    sleep 1
    kill -0 "$FLASH_CAFFEINATE_PID" 2>/dev/null || \
      fail "failed to establish the macOS flash caffeinate guard"
    log "Holding display, idle, disk, system, and user-active assertions during flash"
}

stop_flash_caffeinate() {
    if [[ -n "${FLASH_CAFFEINATE_PID:-}" ]]; then
        kill "$FLASH_CAFFEINATE_PID" 2>/dev/null || true
        wait "$FLASH_CAFFEINATE_PID" 2>/dev/null || true
        FLASH_CAFFEINATE_PID=""
    fi
}

disable_spotlight_for_flash_volume() {
    local volume="$1"

    # macOS can start Spotlight metadata sync on a FAT volume before the final
    # unmount, which makes diskutil report an mdsync dissenter.
    # The marker is the documented non-root opt-out for removable volumes.
    touch "${volume}/.metadata_never_index" 2>/dev/null || true
    mkdir -p "${volume}/.fseventsd" 2>/dev/null || true
    touch "${volume}/.fseventsd/no_log" 2>/dev/null || true
}

stop_spotlight_unmount_dissenter() {
    local unmount_output="$1"
    local pid

    pid="$(printf "%s\n" "$unmount_output" \
      | sed -n 's/.*dissented by PID \([0-9][0-9]*\).*mdsync.*/\1/p' \
      | head -n 1)"
    if [[ -n "$pid" ]]; then
        log "Stopping Spotlight metadata sync pid=${pid} before final unmount"
        kill "$pid" 2>/dev/null || true
    fi
}

unmount_flashed_disk() {
    local disk="$1"
    local volume="$2"
    local output=""
    local attempt

    for attempt in $(seq 1 5); do
        if output="$(diskutil unmount "$volume" 2>&1)"; then
            return 0
        fi
        stop_spotlight_unmount_dissenter "$output"
        sleep 1
    done

    log "Final volume unmount was blocked; forcing exact-child unmount for ${volume}"
    if ! output="$(diskutil unmount force "$volume" 2>&1)"; then
        fail "failed to unmount flashed volume ${volume} on ${disk}: ${output}"
    fi
}

main() {
    parse_args "$@"
    validate_menu_input_mode
    canonicalize_input_paths
    validate_canonical_sel4_build_dir
    validate_output_paths

    cd "$ROOT_DIR"
    trap cleanup EXIT

    if [[ "${CLEAN_BUILD}" -eq 1 && "${SKIP_BUILD}" -eq 1 ]]; then
        fail "--clean cannot be combined with --skip-build"
    fi
    capture_exact_source_identity
    local manifest_real
    manifest_real="$(realpath_py "${MANIFEST_PATH}")"
    if [[ "${manifest_real}" != "$(realpath_py "${CANONICAL_MANIFEST_PATH}")" ]]; then
        RESTORE_CANONICAL_CODEGEN=1
    fi

    require_file "$MANIFEST_PATH"
    require_dir "$FIRMWARE_DIR"
    require_dir "$SEL4_BUILD_DIR"

    activate_venv

    if [[ "${CLEAN_BUILD}" -eq 1 ]]; then
        clean_pi4_build
    fi

    require_file "$U_BOOT_BIN"
    verify_u_boot_pi4_target

    local mkimage_bin
    local cpio_bin
    mkimage_bin="$(resolve_mkimage)"
    prepend_path_var PATH "$(dirname "${mkimage_bin}")"
    log "Using mkimage: ${mkimage_bin}"

    cpio_bin="$(resolve_cpio)"
    configure_cpio_path "$cpio_bin"

    if [[ "$SKIP_BUILD" -eq 0 ]]; then
        build_pi4_image
    else
        verify_pi4_sel4_xhci_device_untyped
        validate_pi4_sel4_build
        capture_canonical_sel4_state
        select_exact_assembly_inputs
        verify_skip_build_image_fresh
        adopt_skip_build_source_timestamp
        verify_skip_build_provenance
        if [[ "${MANIFEST_PATH}" == *.toml ]]; then
            log "Regenerating selected manifest artifacts for --skip-build proof"
            run_coh_rtc_codegen
        elif [[ "${MANIFEST_PATH}" == *.json ]]; then
            log "Synchronizing selected resolved manifest for --skip-build proof"
            sync_resolved_manifest_json
        else
            fail "unsupported --manifest extension (expected .toml or .json): ${MANIFEST_PATH}"
        fi
        capture_build_repository_state
        log "Skipping build (--skip-build)"
    fi

    stage_sd_payload "$mkimage_bin"
    verify_canonical_sel4_state "after final staged-image publication"

    if [[ -n "$FLASH_DISK" ]]; then
        flash_sd_card "$FLASH_DISK"
    else
        log "Stage-only run complete (no flash requested)"
    fi
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
