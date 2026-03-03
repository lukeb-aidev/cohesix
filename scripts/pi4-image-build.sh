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
SEL4_UPSTREAM_IMAGE_NAME="sel4test-driver-image-arm-bcm2711"
COHESIX_IMAGE_NAME="cohesix-image-arm-bcm2711"
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
  - seL4 image (upstream output copied as cohesix-image-arm-bcm2711)
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
  --image-name <name>       Staged/boot image filename on FAT partition
                            (default: cohesix-image-arm-bcm2711)
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

resolve_sel4_source_dir() {
    if [[ -f "${SEL4_BUILD_DIR}/CMakeCache.txt" ]]; then
        local cached
        cached="$(awk -F= '/^CMAKE_HOME_DIRECTORY:INTERNAL=/{print $2}' "${SEL4_BUILD_DIR}/CMakeCache.txt" | tail -n 1)"
        if [[ -n "$cached" && -d "$cached" && -f "${cached}/CMakeLists.txt" ]]; then
            printf "%s\n" "$cached"
            return 0
        fi
    fi

    local inferred
    inferred="$(cd "${SEL4_BUILD_DIR}/.." && pwd)"
    [[ -f "${inferred}/CMakeLists.txt" ]] || fail "could not resolve seL4 source dir for ${SEL4_BUILD_DIR}"
    printf "%s\n" "$inferred"
}

configure_pi4_sel4_build() {
    local sel4_source_dir="$1"

    log "Configuring ${SEL4_BUILD_DIR} for Pi4 serial diagnostics"
    cmake -S "$sel4_source_dir" -B "$SEL4_BUILD_DIR" \
      -DAARCH64=TRUE \
      -DARM_HYP=OFF \
      -DPLATFORM=bcm2711 \
      -DRELEASE=OFF \
      -DVERIFICATION=OFF \
      -DSMP=ON \
      -DNUM_NODES=4 \
      -DSel4testAllowSettingsOverride=ON \
      -DKernelPlatform=bcm2711 \
      -DKernelSel4Arch=aarch64 \
      -DKernelDebugBuild=ON \
      -DKernelPrinting=ON \
      -DHardwareDebugAPI=OFF \
      -DKernelMaxNumNodes=4 \
      -DKernelRootCNodeSizeBits=13 \
      -DElfloaderImage=binary \
      -DElfloaderIncludeDtb=ON \
      -DSIMULATION=OFF \
      -DCMAKE_BUILD_TYPE=Debug

    local cache_file="${SEL4_BUILD_DIR}/CMakeCache.txt"
    require_file "$cache_file"
    grep -q "^KernelPlatform:STRING=bcm2711$" "$cache_file" || fail "KernelPlatform not set to bcm2711"
    grep -q "^RELEASE:BOOL=OFF$" "$cache_file" || fail "RELEASE mode unexpectedly enabled"
    grep -q "^SMP:BOOL=ON$" "$cache_file" || fail "SMP not enabled"
    grep -q "^NUM_NODES:STRING=4$" "$cache_file" || fail "NUM_NODES not set to 4"
    grep -q "^Sel4testAllowSettingsOverride:BOOL=ON$" "$cache_file" || fail "Sel4testAllowSettingsOverride not ON"
    grep -q "^KernelDebugBuild:BOOL=ON$" "$cache_file" || fail "KernelDebugBuild not ON"
    grep -q "^KernelPrinting:BOOL=ON$" "$cache_file" || fail "KernelPrinting not ON"
    grep -q "^HardwareDebugAPI:BOOL=OFF$" "$cache_file" || fail "HardwareDebugAPI must be OFF for current sel4-sys bindings"
    grep -q "^KernelMaxNumNodes:STRING=4$" "$cache_file" || fail "KernelMaxNumNodes not 4"
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
    case ":${PATH}:" in
        *":${cpio_dir}:"*) ;;
        *) export PATH="${cpio_dir}:${PATH}" ;;
    esac
    log "Using cpio: ${cpio_bin}"
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
    local sel4_source_dir
    local jobs
    local root_hash_expected
    local root_hash_actual

    export SEL4_BUILD_DIR
    export SEL4_BUILD="$SEL4_BUILD_DIR"
    export SEL4_LD="${ROOT_DIR}/apps/root-task/sel4.ld"

    sel4_source_dir="$(resolve_sel4_source_dir)"
    configure_pi4_sel4_build "$sel4_source_dir"

    log "Regenerating manifest artifacts via coh-rtc"
    run_coh_rtc_codegen

    log "Building root-task (${ROOT_TASK_FEATURES})"
    cargo build \
      --target aarch64-unknown-none \
      --release \
      -p root-task \
      --no-default-features \
      --features "$ROOT_TASK_FEATURES"

    jobs="$(sysctl -n hw.ncpu)"
    require_file "$root_task_elf"
    log "Rebuilding Pi4 seL4 image in ${SEL4_BUILD_DIR}"
    cmake --build "$SEL4_BUILD_DIR" \
      --target "images/${SEL4_UPSTREAM_IMAGE_NAME}" \
      -j"$jobs"

    require_file "$embedded_rootserver"
    cp -f "$root_task_elf" "$embedded_rootserver"
    log "Injected root-task into ${embedded_rootserver}"

    # Repack the image after injection. The second build should not regenerate
    # rootserver if sel4test-driver has not changed.
    cmake --build "$SEL4_BUILD_DIR" \
      --target "images/${SEL4_UPSTREAM_IMAGE_NAME}" \
      -j"$jobs"

    root_hash_expected="$(shasum -a 256 "$root_task_elf" | awk '{print $1}')"
    root_hash_actual="$(shasum -a 256 "$embedded_rootserver" | awk '{print $1}')"
    [[ "$root_hash_actual" == "$root_hash_expected" ]] || \
      fail "embedded rootserver was regenerated after root-task injection"
}

write_boot_cmd() {
    local out="$1"
    local coh_image="$2"
    local fallback_image="$3"
    cat >"$out" <<'EOF'
echo "[cohesix] pi4 autoboot script"
setenv bootdelay 0
setenv coh_image __COH_IMAGE__
setenv coh_image_fallback __COH_IMAGE_FALLBACK__
setenv coh_addr 0x10000000

if fatload mmc 0:1 ${coh_addr} ${coh_image}; then
    echo "[cohesix] loaded ${coh_image} to ${coh_addr}; jumping"
    go ${coh_addr}
    echo "[cohesix] returned from image"
else
    echo "[cohesix] primary image load failed: ${coh_image}"
    if fatload mmc 0:1 ${coh_addr} ${coh_image_fallback}; then
        setenv coh_image ${coh_image_fallback}

        echo "[cohesix] loaded fallback ${coh_image} to ${coh_addr}; jumping"
        go ${coh_addr}
        echo "[cohesix] returned from image"
    else
        echo "[cohesix] ERROR: failed to load ${coh_image} or fallback ${coh_image_fallback} from mmc 0:1"
        echo "[cohesix] manual: fatls mmc 0:1"
        echo "[cohesix] manual: fatload mmc 0:1 0x10000000 ${coh_image}"
        echo "[cohesix] manual: go 0x10000000"
    fi
fi
EOF
    sed -i '' "s/__COH_IMAGE__/${coh_image}/g" "$out"
    sed -i '' "s/__COH_IMAGE_FALLBACK__/${fallback_image}/g" "$out"
}

stage_sd_payload() {
    local mkimage_bin="$1"
    local sel4_image="${SEL4_BUILD_DIR}/images/${SEL4_UPSTREAM_IMAGE_NAME}"
    local stage_overlays="${STAGE_DIR}/overlays"
    local fallback_image="${STAGE_DIR}/${SEL4_UPSTREAM_IMAGE_NAME}"

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
    cp -f "$sel4_image" "${STAGE_DIR}/${COHESIX_IMAGE_NAME}"
    # Keep legacy fallback filename in sync with the staged Cohesix image so a
    # fallback boot path cannot silently run stale bits.
    cp -f "${STAGE_DIR}/${COHESIX_IMAGE_NAME}" "$fallback_image"

    cat > "${STAGE_DIR}/config.txt" <<'EOF'
arm_64bit=1
arm_boost=1
enable_uart=1
uart_2ndstage=1
enable_gic=1
kernel=u-boot.bin
dtoverlay=upstream-pi4
# Keep mini-UART on GPIO14/15 to match seL4 bcm2711 serial1 console routing.
core_freq=250
total_mem=1024
EOF

    write_boot_cmd "${STAGE_DIR}/boot.cmd" "${COHESIX_IMAGE_NAME}" "${SEL4_UPSTREAM_IMAGE_NAME}"
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

    COPYFILE_DISABLE=1 rsync -a --delete \
      --exclude=".Spotlight-V100" \
      --exclude=".fseventsd" \
      --exclude=".Trashes" \
      --exclude="._*" \
      "${STAGE_DIR}/" "${volume}/"

    find "${volume}" -xdev -name '._*' -type f -delete 2>/dev/null || true

    sync

    local stage_hash sd_hash
    local stage_fallback_hash sd_fallback_hash
    stage_hash="$(shasum -a 256 "${STAGE_DIR}/${COHESIX_IMAGE_NAME}" | awk '{print $1}')"
    sd_hash="$(shasum -a 256 "${volume}/${COHESIX_IMAGE_NAME}" | awk '{print $1}')"
    [[ "$stage_hash" == "$sd_hash" ]] || fail "rootserver image hash mismatch after flash"
    stage_fallback_hash="$(shasum -a 256 "${STAGE_DIR}/${SEL4_UPSTREAM_IMAGE_NAME}" | awk '{print $1}')"
    sd_fallback_hash="$(shasum -a 256 "${volume}/${SEL4_UPSTREAM_IMAGE_NAME}" | awk '{print $1}')"
    [[ "$stage_fallback_hash" == "$sd_fallback_hash" ]] || fail "fallback image hash mismatch after flash"

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
    local cpio_bin
    mkimage_bin="$(resolve_mkimage)"
    log "Using mkimage: ${mkimage_bin}"

    activate_venv

    cpio_bin="$(resolve_cpio)"
    configure_cpio_path "$cpio_bin"

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
