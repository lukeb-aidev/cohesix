#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Build and stage (optionally flash) a Raspberry Pi 4 U-Boot + seL4 Cohesix SD payload.
# Copyright 2026 Lukas Bower

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

MANIFEST_PATH="${ROOT_DIR}/configs/root_task_pi4_uboot_aarch64.toml"
CANONICAL_MANIFEST_PATH="${ROOT_DIR}/configs/root_task.toml"
SEL4_BUILD_DIR="${HOME}/seL4/build_UBOOT"
SEL4_VENV_DIR="${HOME}/seL4/.venv_aarch64"
U_BOOT_BIN="${ROOT_DIR}/third_party/u-boot/u-boot.bin"
OBJCOPY_WRAPPER="${ROOT_DIR}/scripts/aarch64-objcopy-stdout.sh"
FIRMWARE_DIR="${ROOT_DIR}/out/uefi/pi4-followup/firmware/v1.50"
STAGE_DIR="${ROOT_DIR}/out/pi4-sd"
SEL4_UPSTREAM_IMAGE_NAME="sel4test-driver-image-arm-bcm2711"
COHESIX_IMAGE_NAME="cohesix-image-arm-bcm2711"
COHESIX_LOGO_SOURCE="${ROOT_DIR}/docs/COHESIX_LOGO.png"
COHESIX_LOGO_STAGE_NAME="cohesix-logo.bmp"
BOOTSTD_LOGO_STAGE_NAME="boot.bmp"
FLASH_DISK=""
DISK_LABEL="COHESIX"
ROOT_TASK_FEATURES="kernel,bootstrap-trace,serial-console,net-console"
SKIP_BUILD=0
PI4_TOTAL_MEM_MB=2048
RESTORE_CANONICAL_CODEGEN=0
PI4_DTB_PADDED_SIZE=$((128 * 1024))

usage() {
    cat <<'USAGE'
Usage: scripts/pi4-image-build.sh [options]

Builds and stages a Pi 4 SD payload with:
  - Raspberry Pi firmware files (start4.elf, fixup4.dat, DTB + overlays)
  - U-Boot (u-boot.bin)
  - seL4 image (upstream output copied as cohesix-image-arm-bcm2711)
  - Cohesix autoboot script (boot.scr.uimg)
  - Optional Cohesix HDMI logo (cohesix-logo.bmp for U-Boot video)

By default this script only builds/stages files under out/pi4-sd.
To erase and flash an SD card, pass --flash-disk /dev/diskN explicitly.

Options:
  --manifest <path>         Manifest input for root-task build:
                            TOML (coh-rtc source) or resolved JSON
                            (default: configs/root_task_pi4_uboot_aarch64.toml)
  --sel4-build-dir <dir>    seL4 Pi4 build directory (default: ~/seL4/build_UBOOT)
  --venv <dir>              Python venv containing build tooling (default: ~/seL4/.venv_aarch64)
  --u-boot-bin <path>       U-Boot binary (default: third_party/u-boot/u-boot.bin)
  --firmware-dir <dir>      Pi firmware directory (default: out/uefi/pi4-followup/firmware/v1.50)
  --stage-dir <dir>         Output staging directory (default: out/pi4-sd)
  --image-name <name>       Staged/boot image filename on FAT partition
                            (default: cohesix-image-arm-bcm2711)
  --root-task-features <f>  Comma-separated root-task feature list
                            (default: kernel,bootstrap-trace,serial-console,net-console)
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
    grep -q '^CONFIG_CMD_BMP=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_CMD_BMP; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    grep -q '^CONFIG_CMD_BOOTM=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_CMD_BOOTM; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    grep -q '^CONFIG_LEGACY_IMAGE_FORMAT=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_LEGACY_IMAGE_FORMAT; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    grep -q '^CONFIG_USB_KEYBOARD=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_USB_KEYBOARD; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    grep -q '^CONFIG_CMD_CONITRACE=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_CMD_CONITRACE; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    if ! grep -Eq '^CONFIG_SYS_USB_EVENT_POLL=y$|^CONFIG_SYS_USB_EVENT_POLL_VIA_CONTROL_EP=y$' "${config_file}"; then
      fail "u-boot.bin is missing a supported USB keyboard polling mode; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    fi
    grep -q '^CONFIG_SYS_CONSOLE_IS_IN_ENV=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_SYS_CONSOLE_IS_IN_ENV; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
}

verify_boot_cmd_diagnostics() {
    local path="$1"

    require_file "$path"

    grep -q 'coh_usb_capture_diag' "$path" || fail "boot.cmd is missing USB diagnostic capture"
    grep -q 'coh_usb_trace_diag' "$path" || fail "boot.cmd is missing USB keyboard trace diagnostics"
    grep -q 'coh_usb_cold_diag' "$path" || fail "boot.cmd is missing USB cold re-enumeration diagnostics"
    grep -q 'conitrace' "$path" || fail "boot.cmd is missing conitrace USB test path"
    grep -q 'usb tree' "$path" || fail "boot.cmd is missing usb tree diagnostics"
    grep -q 'usb info' "$path" || fail "boot.cmd is missing usb info diagnostics"
    grep -q 'env exists usb_pgood_delay' "$path" || fail "boot.cmd is missing usb_pgood_delay diagnostics"
    grep -q 'printenv stdin' "$path" || fail "boot.cmd is missing stdin diagnostics"
    grep -q 'cohesix,xhci-mmio' "$path" || fail "boot.cmd is missing xHCI MMIO DT handoff"
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
      -DRPI4_MEMORY="${PI4_TOTAL_MEM_MB}" \
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
      -DElfloaderImage=uimage \
      -DElfloaderIncludeDtb=OFF \
      -DCMAKE_OBJCOPY="${OBJCOPY_WRAPPER}" \
      -DSIMULATION=OFF \
      -DCMAKE_BUILD_TYPE=Debug

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
    grep -q "^HardwareDebugAPI:BOOL=OFF$" "$cache_file" || fail "HardwareDebugAPI must be OFF for current sel4-sys bindings"
    grep -q "^KernelMaxNumNodes:STRING=4$" "$cache_file" || fail "KernelMaxNumNodes not 4"
    grep -q "^ElfloaderImage:STRING=uimage$" "$cache_file" || fail "ElfloaderImage not set to uimage"
    grep -q "^ElfloaderIncludeDtb:BOOL=OFF$" "$cache_file" || fail "ElfloaderIncludeDtb must be OFF for Pi4 U-Boot DTB handoff"
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

realpath_py() {
    python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$1"
}

run_coh_rtc_codegen_for_manifest() {
    local manifest_path="$1"
    local manifest_json="$2"
    mkdir -p "${ROOT_DIR}/out/manifests"

    cargo run -p coh-rtc -- \
      "$manifest_path" \
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
      --cohesix-py-defaults "${ROOT_DIR}/tools/cohesix-py/cohesix/generated.py" \
      --cohesix-py-doc "${ROOT_DIR}/docs/snippets/cohesix_py_defaults.md" \
      --coh-doctor-doc "${ROOT_DIR}/docs/snippets/coh_doctor_checks.md" \
      --cohsh-policy "${ROOT_DIR}/out/cohsh_policy.toml" \
      --cohsh-policy-rust "${ROOT_DIR}/apps/cohsh/src/generated/policy.rs" \
      --cohsh-policy-doc "${ROOT_DIR}/docs/snippets/cohsh_policy.md" \
      --cohsh-client-rust "${ROOT_DIR}/apps/cohsh/src/generated/client.rs" \
      --cohsh-client-doc "${ROOT_DIR}/docs/snippets/cohsh_client.md" \
      --cohsh-grammar-doc "${ROOT_DIR}/docs/snippets/cohsh_grammar.md" \
      --cohsh-ticket-policy-doc "${ROOT_DIR}/docs/snippets/cohsh_ticket_policy.md" \
      --coh-policy "${ROOT_DIR}/out/coh_policy.toml" \
      --coh-policy-rust "${ROOT_DIR}/apps/coh/src/generated/policy.rs" \
      --coh-policy-doc "${ROOT_DIR}/docs/snippets/coh_policy.md" \
      --swarmui-defaults "${ROOT_DIR}/out/swarmui_defaults.toml" \
      --swarmui-defaults-rust "${ROOT_DIR}/apps/swarmui/src/generated.rs" \
      --swarmui-defaults-doc "${ROOT_DIR}/docs/snippets/swarmui_defaults.md"
}

run_coh_rtc_codegen() {
    run_coh_rtc_codegen_for_manifest \
      "${MANIFEST_PATH}" \
      "${ROOT_DIR}/out/manifests/root_task_resolved.json"
}

restore_canonical_codegen() {
    if [[ "${RESTORE_CANONICAL_CODEGEN}" -eq 0 ]]; then
        return 0
    fi
    log "Restoring canonical manifest artifacts via coh-rtc (${CANONICAL_MANIFEST_PATH})"
    run_coh_rtc_codegen_for_manifest \
      "${CANONICAL_MANIFEST_PATH}" \
      "${ROOT_DIR}/out/manifests/root_task_resolved.json"
}

cleanup() {
    local status=$?
    trap - EXIT
    if ! restore_canonical_codegen; then
        status=1
    fi
    exit "$status"
}

sync_resolved_manifest_json() {
    local manifest_json="${ROOT_DIR}/out/manifests/root_task_resolved.json"
    local src_real
    local dst_real
    mkdir -p "${ROOT_DIR}/out/manifests"

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

    if [[ "${MANIFEST_PATH}" == *.toml ]]; then
        log "Regenerating manifest artifacts via coh-rtc"
        run_coh_rtc_codegen
    elif [[ "${MANIFEST_PATH}" == *.json ]]; then
        log "Using pre-resolved manifest JSON (${MANIFEST_PATH})"
        sync_resolved_manifest_json
    else
        fail "unsupported --manifest extension (expected .toml or .json): ${MANIFEST_PATH}"
    fi

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
setenv bootdelay 0
setenv coh_image __COH_IMAGE__
setenv coh_image_fallback __COH_IMAGE_FALLBACK__
setenv coh_addr 0x10000000
setenv coh_dtb_addr 0x14000000
setenv coh_dtb_file bcm2711-rpi-4-b.dtb
setenv coh_policy_addr 0x02100000
setenv coh_policy_file cohesix.env
setenv coh_logo_addr 0x02000000
setenv coh_logo_file __COH_LOGO_FILE__
setenv coh_logo_bootstd_file __COH_BOOTSTD_LOGO_FILE__
setenv coh_logo_delay 2
setenv coh_logo_x 20
setenv coh_logo_y 20
setenv coh_reset_policy 'setenv coh_net_mode ""; setenv coh_net_interface ""; setenv coh_static_ip ""; setenv coh_static_prefix_len ""; setenv coh_static_gateway ""; setenv coh_wifi_ssid ""; setenv coh_wifi_psk ""'
setenv coh_clear_saved_policy 'run coh_reset_policy; setenv coh_show_logo ""'
setenv coh_usb_capture_diag 'echo "[cohesix] USB input diagnostics"; if env exists usb_pgood_delay; then printenv usb_pgood_delay; else echo "usb_pgood_delay=<unset>"; fi; printenv stdin; printenv stdout; printenv stderr; coninfo; if usb tree; then true; else echo "[cohesix] WARNING: usb tree failed"; fi; if usb info; then true; else echo "[cohesix] WARNING: usb info failed"; fi'
setenv coh_usb_menu_diag 'if test "${coh_usb_diag_done}" != "1"; then setenv coh_usb_diag_done 1; run coh_usb_capture_diag; fi'
setenv coh_usb_cold_diag 'echo "[cohesix] USB cold re-enumeration diagnostics"; setenv coh_usb_saved_pgood ${usb_pgood_delay}; setenv usb_pgood_delay 8000; echo "[cohesix] usb_pgood_delay=8000 (diagnostic only)"; if usb stop; then true; else echo "[cohesix] WARNING: usb stop failed during diagnostics"; fi; if usb start; then true; else echo "[cohesix] WARNING: usb start failed during diagnostics"; fi; run coh_usb_capture_diag; setenv usb_pgood_delay ${coh_usb_saved_pgood}; setenv stdin usbkbd,serial; setenv stdout serial,vidconsole; setenv stderr serial,vidconsole'
setenv coh_usb_trace_diag 'run coh_prepare_input; cls; echo "[cohesix] USB keyboard diagnostics"; echo "[cohesix] Press keys on the USB keyboard now"; echo "[cohesix] Type x on serial to exit if the keyboard is still dead"; run coh_usb_capture_diag; conitrace; run coh_usb_cold_diag; echo "[cohesix] USB keyboard diagnostics complete"'
setenv coh_prepare_input 'if test "${coh_usb_input_ready}" != "1"; then setenv coh_usb_input_ready 1; echo "[cohesix] reusing preboot USB session for menu input"; fi; setenv stdin usbkbd,serial; setenv stdout serial,vidconsole; setenv stderr serial,vidconsole; run coh_usb_menu_diag'
setenv coh_quiesce_usb 'setenv stdin serial; if usb stop; then echo "[cohesix] USB host quiesced before handoff"; else echo "[cohesix] WARNING: usb stop failed before handoff"; fi'
setenv coh_toggle_logo 'if test "${coh_show_logo}" = "1"; then setenv coh_show_logo 0; echo "[cohesix] HDMI logo disabled"; else setenv coh_show_logo 1; echo "[cohesix] HDMI logo enabled"; fi'
setenv coh_detect_saved_config 'setenv coh_has_saved_config 0; if test -n "${coh_net_mode}"; then setenv coh_has_saved_config 1; fi; if test -n "${coh_net_interface}"; then setenv coh_has_saved_config 1; fi; if test -n "${coh_static_ip}"; then setenv coh_has_saved_config 1; fi; if test -n "${coh_static_prefix_len}"; then setenv coh_has_saved_config 1; fi; if test -n "${coh_static_gateway}"; then setenv coh_has_saved_config 1; fi; if test -n "${coh_wifi_ssid}"; then setenv coh_has_saved_config 1; fi; if test -n "${coh_wifi_psk}"; then setenv coh_has_saved_config 1; fi'
setenv coh_load_saved_policy 'run coh_clear_saved_policy; if fatload mmc 0:1 ${coh_policy_addr} ${coh_policy_file}; then if env import -d -t ${coh_policy_addr} ${filesize} coh_net_mode coh_net_interface coh_static_ip coh_static_prefix_len coh_static_gateway coh_wifi_ssid coh_wifi_psk coh_show_logo; then echo "[cohesix] loaded saved settings from ${coh_policy_file}"; else echo "[cohesix] WARNING: failed to import ${coh_policy_file}; ignoring saved settings"; run coh_clear_saved_policy; fi; fi; if test -z "${coh_show_logo}"; then setenv coh_show_logo 1; fi'
setenv coh_persist_policy 'if env export -t ${coh_policy_addr} coh_net_mode coh_net_interface coh_static_ip coh_static_prefix_len coh_static_gateway coh_wifi_ssid coh_wifi_psk coh_show_logo; then if fatwrite mmc 0:1 ${coh_policy_addr} ${coh_policy_file} ${filesize}; then echo "[cohesix] saved settings to ${coh_policy_file}"; else echo "[cohesix] ERROR: failed to write ${coh_policy_file}"; fi; else echo "[cohesix] ERROR: failed to export saved settings"; fi'
setenv coh_show_logo_splash 'if test "${coh_show_logo}" = "1"; then if test "${coh_logo_shown}" != "1"; then cls; if fatload mmc 0:1 ${coh_logo_addr} ${coh_logo_bootstd_file}; then if bmp display ${coh_logo_addr} m m; then echo "[cohesix] loading boot options..."; sleep ${coh_logo_delay}; setenv coh_logo_shown 1; else echo "[cohesix] logo draw failed: ${coh_logo_bootstd_file}"; fi; else echo "[cohesix] logo splash skipped: ${coh_logo_bootstd_file}"; fi; fi; fi'
setenv coh_load_runtime_dtb 'setenv coh_boot_error 0; if fatload mmc 0:1 ${coh_dtb_addr} ${coh_dtb_file}; then if fdt addr ${coh_dtb_addr}; then echo "[cohesix] loaded ${coh_dtb_file} to ${coh_dtb_addr}"; else echo "[cohesix] ERROR: failed to select ${coh_dtb_file}"; setenv coh_boot_error 1; fi; else echo "[cohesix] ERROR: failed to load ${coh_dtb_file}"; setenv coh_boot_error 1; fi'
setenv coh_apply_dtb_policy 'if test "${coh_boot_error}" = "1"; then true; fi; if test "${coh_boot_error}" != "1" && env exists coh_xhci_mmio_raw && env exists coh_xhci_mmio; then echo "[cohesix] xhci-mmio raw=${coh_xhci_mmio_raw} phys=${coh_xhci_mmio}"; fi; if test "${coh_boot_error}" != "1" && env exists coh_xhci_pci_cmd; then echo "[cohesix] xhci-pci-cmd=${coh_xhci_pci_cmd}"; fi; if test "${coh_boot_error}" != "1" && env exists coh_xhci_mmio; then if fdt set /chosen cohesix,xhci-mmio "${coh_xhci_mmio}"; then echo "[cohesix] dtb chosen cohesix,xhci-mmio=${coh_xhci_mmio}"; else echo "[cohesix] ERROR: failed to set cohesix,xhci-mmio"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && env exists coh_xhci_pci_cmd; then if fdt set /chosen cohesix,xhci-pci-cmd "${coh_xhci_pci_cmd}"; then echo "[cohesix] dtb chosen cohesix,xhci-pci-cmd=${coh_xhci_pci_cmd}"; else echo "[cohesix] ERROR: failed to set cohesix,xhci-pci-cmd"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_net_mode}"; then if fdt set /chosen cohesix,net-mode "${coh_net_mode}"; then echo "[cohesix] dtb chosen cohesix,net-mode=${coh_net_mode}"; else echo "[cohesix] ERROR: failed to set cohesix,net-mode"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_net_interface}"; then if fdt set /chosen cohesix,net-interface "${coh_net_interface}"; then echo "[cohesix] dtb chosen cohesix,net-interface=${coh_net_interface}"; else echo "[cohesix] ERROR: failed to set cohesix,net-interface"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_static_ip}"; then if fdt set /chosen cohesix,static-ipv4 "${coh_static_ip}"; then echo "[cohesix] dtb chosen cohesix,static-ipv4=${coh_static_ip}"; else echo "[cohesix] ERROR: failed to set cohesix,static-ipv4"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_static_prefix_len}"; then if fdt set /chosen cohesix,static-prefix-len "${coh_static_prefix_len}"; then echo "[cohesix] dtb chosen cohesix,static-prefix-len=${coh_static_prefix_len}"; else echo "[cohesix] ERROR: failed to set cohesix,static-prefix-len"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_static_gateway}"; then if fdt set /chosen cohesix,static-gateway "${coh_static_gateway}"; then echo "[cohesix] dtb chosen cohesix,static-gateway=${coh_static_gateway}"; else echo "[cohesix] ERROR: failed to set cohesix,static-gateway"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_wifi_ssid}"; then if fdt set /chosen cohesix,wifi-ssid "${coh_wifi_ssid}"; then echo "[cohesix] dtb chosen cohesix,wifi-ssid=<set>"; else echo "[cohesix] ERROR: failed to set cohesix,wifi-ssid"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_wifi_psk}"; then if fdt set /chosen cohesix,wifi-psk "${coh_wifi_psk}"; then echo "[cohesix] dtb chosen cohesix,wifi-psk=<set>"; else echo "[cohesix] ERROR: failed to set cohesix,wifi-psk"; setenv coh_boot_error 1; fi; fi'
setenv coh_emit_policy_summary 'if test -n "${coh_net_mode}"; then echo "[cohesix] mode=${coh_net_mode}"; else echo "[cohesix] mode=manifest"; fi; if test -n "${coh_net_interface}"; then echo "[cohesix] interface=${coh_net_interface}"; else echo "[cohesix] interface=manifest"; fi; if test -n "${coh_static_ip}"; then echo "[cohesix] static-ip=${coh_static_ip}/${coh_static_prefix_len} gateway=${coh_static_gateway}"; fi; if test -n "${coh_wifi_ssid}"; then echo "[cohesix] wifi-ssid=${coh_wifi_ssid}"; fi'
setenv coh_boot_loaded_image 'run coh_load_runtime_dtb; run coh_apply_dtb_policy; if test "${coh_boot_error}" = "1"; then echo "[cohesix] ERROR: boot aborted before kernel handoff"; else run coh_quiesce_usb; echo "[cohesix] loaded ${coh_image} to ${coh_addr}; bootm with ${coh_dtb_file}"; bootm ${coh_addr} - ${coh_dtb_addr}; echo "[cohesix] returned from image"; fi'
setenv coh_boot_sequence 'run coh_emit_policy_summary; if fatload mmc 0:1 ${coh_addr} ${coh_image}; then run coh_boot_loaded_image; else echo "[cohesix] primary image load failed: ${coh_image}"; if fatload mmc 0:1 ${coh_addr} ${coh_image_fallback}; then setenv coh_image ${coh_image_fallback}; run coh_boot_loaded_image; else echo "[cohesix] ERROR: failed to load ${coh_image} or fallback ${coh_image_fallback} from mmc 0:1"; echo "[cohesix] manual: fatls mmc 0:1"; echo "[cohesix] manual: fatload mmc 0:1 0x10000000 ${coh_image}"; echo "[cohesix] manual: fatload mmc 0:1 0x14000000 ${coh_dtb_file}"; echo "[cohesix] manual: bootm 0x10000000 - 0x14000000"; fi; fi'
setenv coh_prompt_dhcp 'run coh_prepare_input; cls; echo "[cohesix] Guided network setup"; echo "[cohesix] Select address acquisition mode"; echo "  1. DHCP ON (automatic address)"; echo "  2. DHCP OFF (static IPv4)"; echo "  3. Back to boot options"; setenv coh_choice; askenv coh_choice "Select option [1]: " 1; if test -z "${coh_choice}"; then setenv coh_choice 1; fi; if test "${coh_choice}" = "1"; then setenv coh_net_mode dhcp; setenv coh_static_ip ""; setenv coh_static_prefix_len ""; setenv coh_static_gateway ""; run coh_prompt_interface; elif test "${coh_choice}" = "2"; then setenv coh_net_mode static; run coh_prompt_interface; elif test "${coh_choice}" = "3"; then run coh_prompt_root; elif test "${coh_choice}" = "0"; then exit; else echo "[cohesix] invalid selection"; run coh_prompt_dhcp; fi'
setenv coh_prompt_interface 'run coh_prepare_input; cls; echo "[cohesix] Guided network setup"; echo "[cohesix] Select active interface"; echo "  1. Wired Ethernet (GENET)"; echo "  2. Wi-Fi (CYW43455)"; echo "  3. Back to DHCP selection"; setenv coh_choice; askenv coh_choice "Select option [1]: " 1; if test -z "${coh_choice}"; then setenv coh_choice 1; fi; if test "${coh_choice}" = "1"; then setenv coh_net_interface wired; setenv coh_wifi_ssid ""; setenv coh_wifi_psk ""; run coh_after_interface; elif test "${coh_choice}" = "2"; then setenv coh_net_interface wifi; run coh_after_interface; elif test "${coh_choice}" = "3"; then run coh_prompt_dhcp; elif test "${coh_choice}" = "0"; then exit; else echo "[cohesix] invalid selection"; run coh_prompt_interface; fi'
setenv coh_wifi_setup 'run coh_prepare_input; cls; echo "[cohesix] Configure Wi-Fi credentials"; askenv coh_wifi_ssid "Wi-Fi SSID (required): " 32; if test -z "${coh_wifi_ssid}"; then echo "[cohesix] Wi-Fi SSID is required"; run coh_prompt_interface; fi; askenv coh_wifi_psk "Wi-Fi PSK (blank for open network): " 64; if test "${coh_net_mode}" = "static"; then run coh_static_setup; else run coh_confirm_prompt; fi'
setenv coh_static_setup 'run coh_prepare_input; cls; echo "[cohesix] Configure static IPv4 for ${coh_net_interface}"; askenv coh_static_ip "Static IPv4 address (required): " 15; if test -z "${coh_static_ip}"; then echo "[cohesix] Static IPv4 address is required"; run coh_static_setup; fi; askenv coh_static_prefix_len "Prefix length (required, 1-32): " 2; if test -z "${coh_static_prefix_len}"; then echo "[cohesix] Prefix length is required"; run coh_static_setup; fi; askenv coh_static_gateway "Gateway IPv4 (optional): " 15; run coh_confirm_prompt'
setenv coh_after_interface 'if test "${coh_net_interface}" = "wifi"; then run coh_wifi_setup; elif test "${coh_net_mode}" = "static"; then run coh_static_setup; else run coh_confirm_prompt; fi'
setenv coh_confirm_prompt 'run coh_prepare_input; cls; echo "[cohesix] Review network settings"; run coh_emit_policy_summary; echo "  1. Boot with these settings"; echo "  2. Save settings and reboot"; echo "  3. Edit settings"; echo "  4. Discard changes and return"; echo "  0. Exit to U-Boot prompt"; setenv coh_choice; askenv coh_choice "Select option [1]: " 1; if test -z "${coh_choice}"; then setenv coh_choice 1; fi; if test "${coh_choice}" = "1"; then run coh_boot_sequence; elif test "${coh_choice}" = "2"; then run coh_persist_policy; reset; elif test "${coh_choice}" = "3"; then run coh_prompt_dhcp; elif test "${coh_choice}" = "4"; then run coh_load_saved_policy; run coh_prompt_root; elif test "${coh_choice}" = "0"; then exit; else echo "[cohesix] invalid selection"; run coh_confirm_prompt; fi'
setenv coh_prompt_root 'run coh_prepare_input; run coh_detect_saved_config; run coh_show_logo_splash; cls; echo "[cohesix] Cohesix boot options"; if test "${coh_has_saved_config}" = "1"; then echo "[cohesix] Saved network settings detected"; run coh_emit_policy_summary; echo "  1. Continue with existing config"; else echo "[cohesix] No saved network settings; manifest defaults remain active"; echo "  1. Boot with manifest defaults"; fi; echo "  2. Configure networking"; echo "  3. Toggle HDMI logo"; echo "  4. Restore manifest defaults"; echo "  5. Save current settings and reboot"; echo "  6. USB keyboard diagnostics"; echo "  0. Exit to U-Boot prompt"; setenv coh_choice; askenv coh_choice "Select option [1]: " 1; if test -z "${coh_choice}"; then setenv coh_choice 1; fi; if test "${coh_choice}" = "1"; then run coh_boot_sequence; elif test "${coh_choice}" = "2"; then run coh_prompt_dhcp; elif test "${coh_choice}" = "3"; then run coh_toggle_logo; run coh_prompt_root; elif test "${coh_choice}" = "4"; then run coh_reset_policy; run coh_persist_policy; echo "[cohesix] manifest defaults restored"; run coh_prompt_root; elif test "${coh_choice}" = "5"; then run coh_persist_policy; reset; elif test "${coh_choice}" = "6"; then run coh_usb_trace_diag; run coh_prompt_root; elif test "${coh_choice}" = "0"; then exit; else echo "[cohesix] invalid selection"; run coh_prompt_root; fi'
run coh_load_saved_policy
run coh_prompt_root
EOF
    sed -i '' "s/__COH_IMAGE__/${coh_image}/g" "$out"
    sed -i '' "s/__COH_IMAGE_FALLBACK__/${fallback_image}/g" "$out"
    sed -i '' "s/__COH_LOGO_FILE__/${COHESIX_LOGO_STAGE_NAME}/g" "$out"
    sed -i '' "s/__COH_BOOTSTD_LOGO_FILE__/${BOOTSTD_LOGO_STAGE_NAME}/g" "$out"
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
    stage_pi4_dtb "${FIRMWARE_DIR}/bcm2711-rpi-4-b.dtb" "${STAGE_DIR}/bcm2711-rpi-4-b.dtb"
    cp -f "${FIRMWARE_DIR}/overlays/miniuart-bt.dtbo" "${stage_overlays}/miniuart-bt.dtbo"
    cp -f "${FIRMWARE_DIR}/overlays/upstream-pi4.dtbo" "${stage_overlays}/upstream-pi4.dtbo"
    cp -f "$U_BOOT_BIN" "${STAGE_DIR}/u-boot.bin"
    cp -f "$sel4_image" "${STAGE_DIR}/${COHESIX_IMAGE_NAME}"
    # Keep legacy fallback filename in sync with the staged Cohesix image so a
    # fallback boot path cannot silently run stale bits.
    cp -f "${STAGE_DIR}/${COHESIX_IMAGE_NAME}" "$fallback_image"
    stage_uboot_logo "${STAGE_DIR}/${COHESIX_LOGO_STAGE_NAME}"
    if [[ -f "${STAGE_DIR}/${COHESIX_LOGO_STAGE_NAME}" ]]; then
        cp -f "${STAGE_DIR}/${COHESIX_LOGO_STAGE_NAME}" "${STAGE_DIR}/${BOOTSTD_LOGO_STAGE_NAME}"
    fi

    cat > "${STAGE_DIR}/config.txt" <<EOF
arm_64bit=1
arm_boost=1
enable_uart=1
uart_2ndstage=1
enable_gic=1
kernel=u-boot.bin
dtoverlay=upstream-pi4
# Keep mini-UART on GPIO14/15 to match seL4 bcm2711 serial1 console routing.
core_freq=250
total_mem=${PI4_TOTAL_MEM_MB}
EOF

    write_boot_cmd "${STAGE_DIR}/boot.cmd" "${COHESIX_IMAGE_NAME}" "${SEL4_UPSTREAM_IMAGE_NAME}"
    verify_boot_cmd_diagnostics "${STAGE_DIR}/boot.cmd"
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
    local wait_attempts=30

    command -v diskutil >/dev/null 2>&1 || fail "diskutil not found"
    command -v rsync >/dev/null 2>&1 || fail "rsync not found"

    [[ "$disk" == /dev/disk* ]] || fail "--flash-disk must look like /dev/diskN"
    diskutil info "$disk" >/dev/null 2>&1 || fail "disk not found: ${disk}"

    log "Flashing ${disk} (this erases the target disk)"
    diskutil unmountDisk force "$disk" >/dev/null 2>&1 || true
    diskutil eraseDisk FAT32 "$DISK_LABEL" MBRFormat "$disk" >/dev/null

    local part=""
    local volume="/Volumes/${DISK_LABEL}"
    local attempt
    for attempt in $(seq 1 "$wait_attempts"); do
        if diskutil info "${disk}s1" >/dev/null 2>&1; then
            part="${disk}s1"
            break
        fi
        sleep 1
    done
    [[ -n "$part" ]] || fail "failed to find FAT partition after erasing ${disk}"

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
    trap cleanup EXIT

    local manifest_real
    manifest_real="$(realpath_py "${MANIFEST_PATH}")"
    if [[ "${manifest_real}" != "$(realpath_py "${CANONICAL_MANIFEST_PATH}")" ]]; then
        RESTORE_CANONICAL_CODEGEN=1
    fi

    require_file "$MANIFEST_PATH"
    require_file "$U_BOOT_BIN"
    verify_u_boot_pi4_target
    require_dir "$FIRMWARE_DIR"
    require_dir "$SEL4_BUILD_DIR"

    local mkimage_bin
    local cpio_bin
    mkimage_bin="$(resolve_mkimage)"
    export PATH="$(dirname "${mkimage_bin}"):${PATH}"
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
