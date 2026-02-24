#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build and stage (optionally flash) a Raspberry Pi 4 U-Boot + seL4 Cohesix SD payload.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

MANIFEST_PATH="${ROOT_DIR}/configs/root_task_uefi_aarch64.toml"
SEL4_BUILD_DIR="${HOME}/seL4/build_UBOOT"
SEL4_VENV_DIR="${HOME}/seL4/.venv_aarch64"
U_BOOT_BIN="${ROOT_DIR}/third_party/u-boot/u-boot.bin"
FIRMWARE_DIR="${ROOT_DIR}/out/uefi/pi4-followup/firmware/v1.50"
STAGE_DIR="${ROOT_DIR}/out/pi4-sd"
FLASH_DISK=""
DISK_LABEL="COHESIX"
ROOT_TASK_FEATURES="kernel,bootstrap-trace,serial-console"
SKIP_BUILD=0

usage() {
    cat <<'USAGE'
Usage: scripts/pi4-image-build.sh [options]

Builds and stages a Pi 4 SD payload with:
  - Raspberry Pi firmware files (start4.elf, fixup4.dat, DTB + overlays)
  - U-Boot (u-boot.bin)
  - seL4 image (sel4test-driver-image-arm-bcm2711)
  - Cohesix autoboot script (boot.scr.uimg)

By default this script only builds/stages files under out/pi4-sd.
To erase and flash an SD card, pass --flash-disk /dev/diskN explicitly.

Options:
  --manifest <path>         Manifest TOML used for coh-rtc/root-task build
                            (default: configs/root_task_uefi_aarch64.toml)
  --sel4-build-dir <dir>    seL4 Pi4 build directory (default: ~/seL4/build_UBOOT)
  --venv <dir>              Python venv containing build tooling (default: ~/seL4/.venv_aarch64)
  --u-boot-bin <path>       U-Boot binary (default: third_party/u-boot/u-boot.bin)
  --firmware-dir <dir>      Pi firmware directory (default: out/uefi/pi4-followup/firmware/v1.50)
  --stage-dir <dir>         Output staging directory (default: out/pi4-sd)
  --root-task-features <f>  Comma-separated root-task feature list
                            (default: kernel,bootstrap-trace,serial-console)
  --skip-build              Skip rebuild and reuse existing seL4 image in sel4 build dir
  --flash-disk <device>     Erase + flash SD card (example: /dev/disk16)
  --disk-label <name>       FAT32 label when flashing (default: COHESIX)
  -h, --help                Show this help
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

resolve_mkimage() {
    if command -v mkimage >/dev/null 2>&1; then
        command -v mkimage
        return 0
    fi

    local fallback="${ROOT_DIR}/third_party/u-boot/tools/mkimage"
    if [[ -x "$fallback" ]]; then
        printf "%s\n" "$fallback"
        return 0
    fi

    fail "mkimage not found (install u-boot-tools or build third_party/u-boot/tools/mkimage)"
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
            --root-task-features)
                [[ $# -ge 2 ]] || fail "--root-task-features requires a list"
                ROOT_TASK_FEATURES="$2"
                shift 2
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

activate_venv() {
    if [[ ! -d "$SEL4_VENV_DIR" ]]; then
        fail "venv directory not found: ${SEL4_VENV_DIR}"
    fi
    # shellcheck disable=SC1090
    source "${SEL4_VENV_DIR}/bin/activate"
}

run_coh_rtc_codegen() {
    local manifest_json="${ROOT_DIR}/out/manifests/root_task_resolved.json"
    mkdir -p "${ROOT_DIR}/out/manifests"

    cargo run -p coh-rtc -- \
      "$MANIFEST_PATH" \
      --out "${ROOT_DIR}/apps/root-task/src/generated" \
      --manifest "$manifest_json" \
      --cas-manifest-template "${ROOT_DIR}/out/cas_manifest_template.json" \
      --cli-script "${ROOT_DIR}/scripts/cohsh/boot_v0.coh" \
      --doc-snippet "${ROOT_DIR}/docs/snippets/root_task_manifest.md" \
      --gpu-breadcrumbs-snippet "${ROOT_DIR}/docs/snippets/gpu_breadcrumbs.md" \
      --observability-interfaces-snippet "${ROOT_DIR}/docs/snippets/observability_interfaces.md" \
      --observability-security-snippet "${ROOT_DIR}/docs/snippets/observability_security.md" \
      --ticket-quotas-snippet "${ROOT_DIR}/docs/snippets/ticket_quotas.md" \
      --trace-policy-snippet "${ROOT_DIR}/docs/snippets/trace_policy.md" \
      --cas-interfaces-snippet "${ROOT_DIR}/docs/snippets/cas_interfaces.md" \
      --cas-security-snippet "${ROOT_DIR}/docs/snippets/cas_security.md" \
      --cohsh-grammar-doc "${ROOT_DIR}/docs/snippets/cohsh_grammar.md" \
      --cohsh-ticket-policy-doc "${ROOT_DIR}/docs/snippets/cohsh_ticket_policy.md"
}

build_pi4_image() {
    local root_task_elf="${ROOT_DIR}/target/aarch64-unknown-none/release/root-task"
    local embedded_rootserver="${SEL4_BUILD_DIR}/elfloader/rootserver"

    export SEL4_BUILD_DIR
    export SEL4_BUILD="$SEL4_BUILD_DIR"
    export SEL4_LD="${ROOT_DIR}/apps/root-task/sel4.ld"

    log "Regenerating manifest artifacts via coh-rtc"
    run_coh_rtc_codegen

    log "Building root-task (${ROOT_TASK_FEATURES})"
    cargo build \
      --target aarch64-unknown-none \
      --release \
      -p root-task \
      --no-default-features \
      --features "$ROOT_TASK_FEATURES"

    require_file "$root_task_elf"
    require_file "$embedded_rootserver"

    cp -f "$root_task_elf" "$embedded_rootserver"
    log "Synced root-task into ${embedded_rootserver}"

    local jobs
    jobs="$(sysctl -n hw.ncpu)"
    log "Rebuilding Pi4 seL4 image in ${SEL4_BUILD_DIR}"
    cmake --build "$SEL4_BUILD_DIR" \
      --target images/sel4test-driver-image-arm-bcm2711 \
      -j"$jobs"
}

write_boot_cmd() {
    local out="$1"
    cat >"$out" <<'EOF'
echo "[cohesix] pi4 autoboot script"
setenv coh_image sel4test-driver-image-arm-bcm2711
setenv coh_addr 0x10000000
setenv coh_state_addr 0x13000000
setenv coh_state_size 0
setenv cohesix_boot_bytes 0
setenv coh_write_state 'if env export -t ${coh_state_addr} cohesix_boot_stage cohesix_boot_bytes; then setexpr coh_state_size ${filesize} - 1; fatwrite mmc 0:1 ${coh_state_addr} cohesix_boot_state.txt ${coh_state_size}; fi'

setenv cohesix_boot_stage pre_load
setenv cohesix_boot_bytes 0
run coh_write_state

if fatload mmc 0:1 ${coh_addr} ${coh_image}; then
    setenv cohesix_boot_stage loaded
    setenv cohesix_boot_bytes ${filesize}
    run coh_write_state

    echo "[cohesix] loaded ${coh_image} to ${coh_addr}; jumping"
    setenv cohesix_boot_stage before_go
    run coh_write_state

    go ${coh_addr}

    setenv cohesix_boot_stage returned_from_go
    run coh_write_state
    echo "[cohesix] returned from image"
else
    setenv cohesix_boot_stage load_failed
    setenv cohesix_boot_bytes 0
    run coh_write_state

    echo "[cohesix] ERROR: failed to load ${coh_image} from mmc 0:1"
    echo "[cohesix] manual: fatls mmc 0:1"
    echo "[cohesix] manual: fatload mmc 0:1 0x10000000 ${coh_image}"
    echo "[cohesix] manual: go 0x10000000"
fi
EOF
}

stage_sd_payload() {
    local mkimage_bin="$1"
    local sel4_image="${SEL4_BUILD_DIR}/images/sel4test-driver-image-arm-bcm2711"
    local stage_overlays="${STAGE_DIR}/overlays"

    require_file "$sel4_image"
    require_file "$U_BOOT_BIN"
    require_dir "$FIRMWARE_DIR"

    rm -rf "$STAGE_DIR"
    mkdir -p "$stage_overlays"

    cp -f "${FIRMWARE_DIR}/start4.elf" "${STAGE_DIR}/start4.elf"
    cp -f "${FIRMWARE_DIR}/fixup4.dat" "${STAGE_DIR}/fixup4.dat"
    cp -f "${FIRMWARE_DIR}/bcm2711-rpi-4-b.dtb" "${STAGE_DIR}/bcm2711-rpi-4-b.dtb"
    cp -f "${FIRMWARE_DIR}/overlays/miniuart-bt.dtbo" "${stage_overlays}/miniuart-bt.dtbo"
    cp -f "${FIRMWARE_DIR}/overlays/upstream-pi4.dtbo" "${stage_overlays}/upstream-pi4.dtbo"
    cp -f "$U_BOOT_BIN" "${STAGE_DIR}/u-boot.bin"
    cp -f "$sel4_image" "${STAGE_DIR}/sel4test-driver-image-arm-bcm2711"

    cat > "${STAGE_DIR}/config.txt" <<'EOF'
arm_64bit=1
arm_boost=1
enable_uart=1
uart_2ndstage=1
enable_gic=1
kernel=u-boot.bin
dtoverlay=miniuart-bt
dtoverlay=upstream-pi4
total_mem=1024
EOF

    write_boot_cmd "${STAGE_DIR}/boot.cmd"
    "$mkimage_bin" \
      -A arm64 \
      -T script \
      -C none \
      -n "Cohesix Pi4 autoboot" \
      -d "${STAGE_DIR}/boot.cmd" \
      "${STAGE_DIR}/boot.scr.uimg" \
      >/dev/null

    cat > "${STAGE_DIR}/cohesix_boot_state.txt" <<'EOF'
cohesix_boot_stage=prepared
cohesix_boot_bytes=0
EOF

    require_file "${STAGE_DIR}/boot.scr.uimg"
    log "Staged Pi4 payload at ${STAGE_DIR}"
}

flash_sd_card() {
    local disk="$1"

    command -v diskutil >/dev/null 2>&1 || fail "diskutil not found"
    command -v rsync >/dev/null 2>&1 || fail "rsync not found"

    [[ "$disk" == /dev/disk* ]] || fail "--flash-disk must look like /dev/diskN"
    diskutil info "$disk" >/dev/null 2>&1 || fail "disk not found: ${disk}"

    log "Flashing ${disk} (this erases the target disk)"
    diskutil unmountDisk force "$disk" >/dev/null 2>&1 || true
    diskutil eraseDisk FAT32 "$DISK_LABEL" MBRFormat "$disk" >/dev/null

    local part="${disk}s1"
    local volume="/Volumes/${DISK_LABEL}"
    if [[ ! -d "$volume" ]]; then
        diskutil mount "$part" >/dev/null
    fi

    rsync -a --delete \
      --exclude=".Spotlight-V100" \
      --exclude=".fseventsd" \
      --exclude=".Trashes" \
      --exclude="._*" \
      "${STAGE_DIR}/" "${volume}/"

    sync

    local stage_hash sd_hash
    stage_hash="$(shasum -a 256 "${STAGE_DIR}/sel4test-driver-image-arm-bcm2711" | awk '{print $1}')"
    sd_hash="$(shasum -a 256 "${volume}/sel4test-driver-image-arm-bcm2711" | awk '{print $1}')"
    [[ "$stage_hash" == "$sd_hash" ]] || fail "rootserver image hash mismatch after flash"

    diskutil unmount "$volume" >/dev/null
    log "Flash complete and unmounted: ${disk}"
}

main() {
    parse_args "$@"

    cd "$ROOT_DIR"

    require_file "$MANIFEST_PATH"
    require_file "$U_BOOT_BIN"
    require_dir "$FIRMWARE_DIR"
    require_dir "$SEL4_BUILD_DIR"

    local mkimage_bin
    mkimage_bin="$(resolve_mkimage)"
    log "Using mkimage: ${mkimage_bin}"

    activate_venv

    if [[ "$SKIP_BUILD" -eq 0 ]]; then
        build_pi4_image
    else
        log "Skipping build (--skip-build)"
    fi

    stage_sd_payload "$mkimage_bin"

    if [[ -n "$FLASH_DISK" ]]; then
        flash_sd_card "$FLASH_DISK"
    else
        log "Stage-only run complete (no flash requested)"
    fi
}

main "$@"
