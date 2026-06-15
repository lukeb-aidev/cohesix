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
DEFAULT_HOME_SEL4_BUILD_DIR="${HOME}/seL4/build_UBOOT"
if [[ -d "${DEFAULT_REPO_SEL4_BUILD_DIR}" ]]; then
    SEL4_BUILD_DIR="${DEFAULT_REPO_SEL4_BUILD_DIR}"
else
    SEL4_BUILD_DIR="${DEFAULT_HOME_SEL4_BUILD_DIR}"
fi
SEL4_VENV_DIR="${ROOT_DIR}/.venv"
U_BOOT_BIN="${ROOT_DIR}/third_party/u-boot/u-boot.bin"
OBJCOPY_WRAPPER="${ROOT_DIR}/scripts/aarch64-objcopy-stdout.sh"
FIRMWARE_DIR="${ROOT_DIR}/out/uefi/pi4-followup/firmware/v1.50"
STAGE_DIR="${ROOT_DIR}/out/pi4-sd"
SEL4_UPSTREAM_IMAGE_NAME="sel4test-driver-image-arm-bcm2711"
COHESIX_IMAGE_NAME="cohesix-image-arm-bcm2711"
COHESIX_LOGO_SOURCE="${ROOT_DIR}/docs/COHESIX_LOGO.png"
COHESIX_LOGO_STAGE_NAME="cohesix-logo.bmp"
BOOTSTD_LOGO_STAGE_NAME="boot.bmp"
BRCMFMAC_CMDLINE_STAGE_NAME="brcmfmac-dyndbg.cmdline"
BRCMFMAC_DYNAMIC_DEBUG_STAGE_NAME="brcmfmac-dyndbg.sh"
DRIVER_RUNTIME_CPIO_STAGE_NAME="cohesix-driver-runtimes.cpio.uimg"
DRIVER_RUNTIME_EMBED_DIR="${ROOT_DIR}/out/pi4-driver-runtime-embed"
DRIVER_RUNTIME_EMBED_CPIO_NAME="cohesix-driver-runtimes.cpio"
ROOT_TASK_STRIP_DIR="${ROOT_DIR}/out/pi4-root-task-stripped"
FLASH_DISK=""
DISK_LABEL="COHESIX"
ROOT_TASK_FEATURES="release-pi4,bootstrap-trace"
SKIP_BUILD=0
CLEAN_BUILD=0
PI4_TOTAL_MEM_MB=2048
RESTORE_CANONICAL_CODEGEN=0
PI4_DTB_PADDED_SIZE=$((128 * 1024))
U_BOOT_CROSS_COMPILE="aarch64-linux-gnu-"
U_BOOT_MENU_INPUT="${COHESIX_UBOOT_MENU_INPUT:-usb}"

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
To erase and flash an SD card, pass --flash-disk /dev/diskN explicitly.

Options:
  --manifest <path>         Manifest input for root-task build:
                            TOML (coh-rtc source) or resolved JSON
                            (default: configs/root_task_pi4_uboot_aarch64.toml)
  --sel4-build-dir <dir>    seL4 Pi4 build directory (default: repo seL4/build_UBOOT
                            when present, otherwise ~/seL4/build_UBOOT)
  --venv <dir>              Python venv containing build tooling (default: <repo>/.venv)
  --u-boot-bin <path>       U-Boot binary (default: third_party/u-boot/u-boot.bin)
  --firmware-dir <dir>      Pi firmware directory (default: out/uefi/pi4-followup/firmware/v1.50)
  --stage-dir <dir>         Output staging directory (default: out/pi4-sd)
  --image-name <name>       Staged/boot image filename on FAT partition
                            (default: cohesix-image-arm-bcm2711)
  --root-task-features <f>  Comma-separated root-task feature list
                            (default: release-pi4,bootstrap-trace)
  --uboot-menu-input <m>    U-Boot setup menu input mode: usb or serial
                            (default: usb; env: COHESIX_UBOOT_MENU_INPUT)
  --clean                   Clean and rebuild root-task, Pi4 seL4/U-Boot outputs,
                            and the Pi 4 U-Boot binary before staging/flashing
  --skip-build              Skip rebuild and reuse existing seL4 image in sel4 build dir
  --flash-disk <device>     Erase + flash SD card (example: /dev/disk16)
  --disk-label <name>       FAT32 label when flashing (default: COHESIX)
  -h, --help                Show this help

Environment:
  COHESIX_UBOOT_MENU_INPUT may be serial or usb.
  USB is always staged as Cohesix-owned cold boot. U-Boot xHCI handoff export is disabled.
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

find_aarch64_strip() {
    local candidate
    for candidate in \
        /opt/homebrew/opt/aarch64-elf-binutils/bin/aarch64-elf-strip \
        /opt/homebrew/bin/aarch64-elf-strip \
        /opt/homebrew/bin/aarch64-linux-gnu-strip \
        "$(command -v aarch64-elf-strip 2>/dev/null || true)" \
        "$(command -v aarch64-linux-gnu-strip 2>/dev/null || true)"; do
        [[ -n "$candidate" && -x "$candidate" ]] || continue
        printf '%s\n' "$candidate"
        return 0
    done
    return 1
}

STRIPPED_ROOT_TASK_ELF=""
strip_root_task_for_pi_image() {
    local src="$1"
    local strip_tool
    local src_bytes
    local dst_bytes

    strip_tool="$(find_aarch64_strip || true)"
    [[ -n "$strip_tool" ]] || fail "aarch64 strip tool not found"

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
    grep -q '^CONFIG_CMD_BMP=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_CMD_BMP; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    grep -q '^CONFIG_CMD_BOOTM=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_CMD_BOOTM; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
    grep -q '^CONFIG_LEGACY_IMAGE_FORMAT=y$' "${config_file}" || \
      fail "u-boot.bin is missing CONFIG_LEGACY_IMAGE_FORMAT; run: make -C third_party/u-boot rpi_4_defconfig && make -C third_party/u-boot CROSS_COMPILE=aarch64-linux-gnu- -j\$(sysctl -n hw.ncpu)"
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
    local image="${SEL4_BUILD_DIR}/images/${SEL4_UPSTREAM_IMAGE_NAME}"
    local input=""
    local stale=""
    local -a freshness_inputs=(
        "${MANIFEST_PATH}"
        "${ROOT_DIR}/apps/root-task/Cargo.toml"
        "${ROOT_DIR}/apps/root-task/build.rs"
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

verify_boot_cmd_handoff() {
    local path="$1"

    require_file "$path"

    grep -q "setenv coh_menu_input ${U_BOOT_MENU_INPUT}" "$path" || fail "boot.cmd menu input mode does not match ${U_BOOT_MENU_INPUT}"
    grep -q 'test "${coh_menu_input}" = "usb"' "$path" || fail "boot.cmd is missing guarded USB menu-input setup"
    grep -q 'run coh_quiesce_usb' "$path" || fail "boot.cmd is missing USB quiesce step"
    grep -q 'run coh_clear_xhci_handoff_live' "$path" || fail "boot.cmd is missing xHCI stale-token clearing before usb stop"
    grep -q 'setenv coh_xhci_mmio;' "$path" || fail "boot.cmd does not clear stale xHCI MMIO before usb stop"
    grep -q 'setenv coh_xhci_pci_cmd;' "$path" || fail "boot.cmd does not clear stale xHCI PCI command before usb stop"
    grep -q 'xHCI state discarded before Cohesix cold boot' "$path" || fail "boot.cmd does not discard stopped U-Boot xHCI state before Cohesix cold boot"
    grep -q 'xHCI cold boot starts unseeded' "$path" || fail "boot.cmd does not make the no-U-Boot-USB cold-boot path explicit"
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

verify_pi4_sel4_xhci_device_untyped() {
    local sel4_source_dir="$1"
    local overlay_path=""
    local generated_dts="${SEL4_BUILD_DIR}/kernel/kernel.dts"
    local candidate=""
    local -a overlay_candidates=(
        "${sel4_source_dir}/kernel/src/plat/bcm2711/overlay-rpi4.dts"
        "${sel4_source_dir}/../kernel/src/plat/bcm2711/overlay-rpi4.dts"
        "${sel4_source_dir}/../../kernel/src/plat/bcm2711/overlay-rpi4.dts"
    )

    for candidate in "${overlay_candidates[@]}"; do
        if [[ -f "${candidate}" ]]; then
            overlay_path="${candidate}"
            break
        fi
    done
    [[ -n "${overlay_path}" ]] || \
      fail "required file missing: Pi4 seL4 overlay-rpi4.dts under ${sel4_source_dir}"

    grep -q 'device-untypes@600000000' "${overlay_path}" || \
      fail "Pi4 seL4 overlay is missing device-untypes@600000000 (${overlay_path}); update the external seL4 tree intentionally and rebuild. This proof script does not patch kernel sources."

    if [[ -f "${generated_dts}" ]]; then
        grep -q 'device-untypes@600000000' "${generated_dts}" || \
          fail "generated seL4 kernel.dts is missing device-untypes@600000000 (${generated_dts}); reconfigure/rebuild ${SEL4_BUILD_DIR}"
    fi

    log "Verified Pi4 seL4 device-untyped source/artifact for VL805 BAR0 (${overlay_path})"
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
    # seL4 archive rules invoke "cpio" by name from nested bash commands. Keep
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
    local pkg_config_libs=""
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
        pkg_config_libs="$(PKG_CONFIG_PATH="${PKG_CONFIG_PATH:-}" pkg-config --libs libssl libcrypto 2>/dev/null || true)"
        [[ -n "${pkg_config_cflags}" ]] && append_env_flag HOSTCFLAGS "${pkg_config_cflags}"
        [[ -n "${pkg_config_libs}" ]] && append_env_flag HOSTLDLIBS "${pkg_config_libs}"
        log "Using OpenSSL from ${prefix} for Pi4 U-Boot host tools"
        return 0
    done

    fail "could not resolve a Homebrew OpenSSL prefix for Pi4 U-Boot; install openssl@3 or use a prebuilt default u-boot.bin without --clean"
}

clean_root_task_build() {
    log "Cleaning root-task cargo artifacts"
    cargo clean -p root-task
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
    jobs="$(sysctl -n hw.ncpu)"

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

rebuild_sel4_pi4_uboot_tree() {
    local sel4_source_dir=""
    local jobs=""

    sel4_source_dir="$(resolve_sel4_source_dir)"
    verify_pi4_sel4_xhci_device_untyped "${sel4_source_dir}"
    configure_pi4_sel4_build "${sel4_source_dir}"

    jobs="$(sysctl -n hw.ncpu)"

    log "Cleaning Pi4 seL4 U-Boot build tree in ${SEL4_BUILD_DIR}"
    cmake --build "${SEL4_BUILD_DIR}" --target clean
    log "Rebuilding Pi4 seL4 U-Boot build tree"
    cmake --build "${SEL4_BUILD_DIR}" -j"${jobs}"

    require_file "${SEL4_BUILD_DIR}/libsel4/libsel4.a"
    require_file "${SEL4_BUILD_DIR}/images/${SEL4_UPSTREAM_IMAGE_NAME}"
}

clean_pi4_build() {
    clean_root_task_build
    rebuild_u_boot_pi4
    rebuild_sel4_pi4_uboot_tree
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
    case "${U_BOOT_MENU_INPUT}" in
        serial|usb) ;;
        *) fail "--uboot-menu-input must be serial or usb (got ${U_BOOT_MENU_INPUT})" ;;
    esac
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
    STAGE_DIR="$(realpath_py "${STAGE_DIR}")"
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
    local root_task_elf
    local embedded_rootserver="${SEL4_BUILD_DIR}/elfloader/rootserver"
    local sel4_source_dir
    local jobs
    local root_hash_expected
    local root_hash_actual

    export SEL4_BUILD_DIR
    export SEL4_BUILD="$SEL4_BUILD_DIR"
    export SEL4_LD="${ROOT_DIR}/apps/root-task/sel4.ld"

    root_task_elf="$(root_task_release_elf_path)"

    sel4_source_dir="$(resolve_sel4_source_dir)"
    verify_pi4_sel4_xhci_device_untyped "$sel4_source_dir"
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

    log "Building Pi4 isolated driver runtime images"
    cargo build \
      --target aarch64-unknown-none \
      --release \
      -p pi4-driver-runtime

    mkdir -p "${DRIVER_RUNTIME_EMBED_DIR}"
    local embedded_runtime_cpio="${DRIVER_RUNTIME_EMBED_DIR}/${DRIVER_RUNTIME_EMBED_CPIO_NAME}"
    package_driver_runtime_raw_cpio "${embedded_runtime_cpio}"

    log "Building root-task (${ROOT_TASK_FEATURES})"
    COHESIX_PI4_DRIVER_RUNTIME_PAYLOAD="${embedded_runtime_cpio}" \
      cargo build \
        --target aarch64-unknown-none \
        --release \
        -p root-task \
        --no-default-features \
        --features "$ROOT_TASK_FEATURES"

    jobs="$(sysctl -n hw.ncpu)"
    require_file "$root_task_elf"
    log "Built root-task ELF: ${root_task_elf}"
    strip_root_task_for_pi_image "$root_task_elf"
    log "Rebuilding Pi4 seL4 image in ${SEL4_BUILD_DIR}"
    cmake --build "$SEL4_BUILD_DIR" \
      --target "images/${SEL4_UPSTREAM_IMAGE_NAME}" \
      -j"$jobs"

    require_file "$embedded_rootserver"
    cp -f "$STRIPPED_ROOT_TASK_ELF" "$embedded_rootserver"
    log "Injected root-task into ${embedded_rootserver}"

    # Repack the image after injection. The second build should not regenerate
    # rootserver if sel4test-driver has not changed.
    cmake --build "$SEL4_BUILD_DIR" \
      --target "images/${SEL4_UPSTREAM_IMAGE_NAME}" \
      -j"$jobs"

    root_hash_expected="$(shasum -a 256 "$STRIPPED_ROOT_TASK_ELF" | awk '{print $1}')"
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
setenv coh_runtime_cpio_addr 0x15000000
setenv coh_runtime_cpio_file __COH_RUNTIME_CPIO_FILE__
setenv coh_logo_addr 0x02000000
setenv coh_logo_file __COH_LOGO_FILE__
setenv coh_logo_bootstd_file __COH_BOOTSTD_LOGO_FILE__
setenv coh_logo_delay 2
setenv coh_logo_x 20
setenv coh_logo_y 20
setenv coh_menu_input __COH_MENU_INPUT__
setenv coh_reset_policy 'setenv coh_net_mode ""; setenv coh_net_interface ""; setenv coh_static_ip ""; setenv coh_static_prefix_len ""; setenv coh_static_gateway ""; setenv coh_wifi_ssid ""; setenv coh_wifi_psk ""'
setenv coh_clear_saved_policy 'run coh_reset_policy; setenv coh_show_logo ""'
setenv coh_bootstrap_usb_session 'if test "${coh_menu_input}" = "usb"; then if test "${coh_usb_input_ready}" != "1"; then echo "[cohesix] starting USB host session for menu/input"; pci enum; if usb start; then setenv coh_usb_input_ready 1; echo "[cohesix] USB host session active"; else setenv coh_usb_input_ready 0; echo "[cohesix] WARNING: usb start failed before menu/input"; fi; fi; else setenv coh_usb_input_ready 0; fi'
setenv coh_prepare_input 'run coh_bootstrap_usb_session; if test "${coh_usb_input_ready}" = "1"; then echo "[cohesix] USB keyboard input active"; setenv stdin usbkbd,serial; else echo "[cohesix] USB keyboard input unavailable; serial only"; setenv stdin serial; fi; setenv stdout serial,vidconsole; setenv stderr serial,vidconsole'
setenv coh_clear_xhci_handoff_live 'setenv coh_xhci_mmio; setenv coh_xhci_pci_cmd; setenv coh_xhci_handoff_ready; setenv coh_xhci_irq_quiesced; setenv coh_xhci_halted; setenv coh_xhci_handoff_safe; setenv coh_xhci_usbcmd; setenv coh_xhci_usbsts; setenv coh_xhci_iman0'
setenv coh_quiesce_usb 'setenv stdin serial; run coh_clear_xhci_handoff_live; if test "${coh_usb_input_ready}" = "1"; then if usb stop; then run coh_clear_xhci_handoff_live; echo "[cohesix] USB host stopped; xHCI state discarded before Cohesix cold boot"; else run coh_clear_xhci_handoff_live; echo "[cohesix] WARNING: usb stop failed before Cohesix boot"; fi; else run coh_clear_xhci_handoff_live; echo "[cohesix] USB host session was not active; xHCI cold boot starts unseeded"; fi'
setenv coh_toggle_logo 'if test "${coh_show_logo}" = "1"; then setenv coh_show_logo 0; echo "[cohesix] HDMI logo disabled"; else setenv coh_show_logo 1; echo "[cohesix] HDMI logo enabled"; fi'
setenv coh_detect_saved_config 'setenv coh_has_saved_config 0; if test -n "${coh_net_mode}"; then setenv coh_has_saved_config 1; fi; if test -n "${coh_net_interface}"; then setenv coh_has_saved_config 1; fi; if test -n "${coh_static_ip}"; then setenv coh_has_saved_config 1; fi; if test -n "${coh_static_prefix_len}"; then setenv coh_has_saved_config 1; fi; if test -n "${coh_static_gateway}"; then setenv coh_has_saved_config 1; fi; if test -n "${coh_wifi_ssid}"; then setenv coh_has_saved_config 1; fi; if test -n "${coh_wifi_psk}"; then setenv coh_has_saved_config 1; fi'
setenv coh_load_saved_policy 'run coh_clear_saved_policy; if fatload mmc 0:1 ${coh_policy_addr} ${coh_policy_file}; then if env import -d -t ${coh_policy_addr} ${filesize} coh_net_mode coh_net_interface coh_static_ip coh_static_prefix_len coh_static_gateway coh_wifi_ssid coh_wifi_psk coh_show_logo; then echo "[cohesix] loaded saved settings from ${coh_policy_file}"; else echo "[cohesix] WARNING: failed to import ${coh_policy_file}; ignoring saved settings"; run coh_clear_saved_policy; fi; fi; if test -z "${coh_show_logo}"; then setenv coh_show_logo 1; fi'
setenv coh_persist_policy 'if env export -t ${coh_policy_addr} coh_net_mode coh_net_interface coh_static_ip coh_static_prefix_len coh_static_gateway coh_wifi_ssid coh_wifi_psk coh_show_logo; then if fatwrite mmc 0:1 ${coh_policy_addr} ${coh_policy_file} ${filesize}; then echo "[cohesix] saved settings to ${coh_policy_file}"; else echo "[cohesix] ERROR: failed to write ${coh_policy_file}"; fi; else echo "[cohesix] ERROR: failed to export saved settings"; fi'
setenv coh_show_logo_splash 'if test "${coh_show_logo}" = "1"; then if test "${coh_logo_shown}" != "1"; then cls; if fatload mmc 0:1 ${coh_logo_addr} ${coh_logo_bootstd_file}; then if bmp display ${coh_logo_addr} m m; then echo "[cohesix] loading boot options..."; if test "${coh_logo_delay}" != "0"; then sleep ${coh_logo_delay}; fi; setenv coh_logo_shown 1; else echo "[cohesix] logo draw failed: ${coh_logo_bootstd_file}"; fi; else echo "[cohesix] logo splash skipped: ${coh_logo_bootstd_file}"; fi; fi; fi'
setenv coh_load_runtime_dtb 'setenv coh_boot_error 0; if fatload mmc 0:1 ${coh_dtb_addr} ${coh_dtb_file}; then if fdt addr ${coh_dtb_addr}; then echo "[cohesix] loaded ${coh_dtb_file} to ${coh_dtb_addr}"; else echo "[cohesix] ERROR: failed to select ${coh_dtb_file}"; setenv coh_boot_error 1; fi; else echo "[cohesix] ERROR: failed to load ${coh_dtb_file}"; setenv coh_boot_error 1; fi'
setenv coh_apply_dtb_policy 'if test "${coh_boot_error}" != "1" && test -n "${coh_net_mode}"; then if fdt set /chosen cohesix,net-mode "${coh_net_mode}"; then echo "[cohesix] dtb chosen cohesix,net-mode=${coh_net_mode}"; else echo "[cohesix] ERROR: failed to set cohesix,net-mode"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_net_interface}"; then if fdt set /chosen cohesix,net-interface "${coh_net_interface}"; then echo "[cohesix] dtb chosen cohesix,net-interface=${coh_net_interface}"; else echo "[cohesix] ERROR: failed to set cohesix,net-interface"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_static_ip}"; then if fdt set /chosen cohesix,static-ipv4 "${coh_static_ip}"; then echo "[cohesix] dtb chosen cohesix,static-ipv4=${coh_static_ip}"; else echo "[cohesix] ERROR: failed to set cohesix,static-ipv4"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_static_prefix_len}"; then if fdt set /chosen cohesix,static-prefix-len "${coh_static_prefix_len}"; then echo "[cohesix] dtb chosen cohesix,static-prefix-len=${coh_static_prefix_len}"; else echo "[cohesix] ERROR: failed to set cohesix,static-prefix-len"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_static_gateway}"; then if fdt set /chosen cohesix,static-gateway "${coh_static_gateway}"; then echo "[cohesix] dtb chosen cohesix,static-gateway=${coh_static_gateway}"; else echo "[cohesix] ERROR: failed to set cohesix,static-gateway"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_wifi_ssid}"; then if fdt set /chosen cohesix,wifi-ssid "${coh_wifi_ssid}"; then echo "[cohesix] dtb chosen cohesix,wifi-ssid=<set>"; else echo "[cohesix] ERROR: failed to set cohesix,wifi-ssid"; setenv coh_boot_error 1; fi; fi; if test "${coh_boot_error}" != "1" && test -n "${coh_wifi_psk}"; then if fdt set /chosen cohesix,wifi-psk "${coh_wifi_psk}"; then echo "[cohesix] dtb chosen cohesix,wifi-psk=<set>"; else echo "[cohesix] ERROR: failed to set cohesix,wifi-psk"; setenv coh_boot_error 1; fi; fi'
setenv coh_emit_policy_summary 'if test -n "${coh_net_mode}"; then echo "[cohesix] mode=${coh_net_mode}"; else echo "[cohesix] mode=manifest"; fi; if test -n "${coh_net_interface}"; then echo "[cohesix] interface=${coh_net_interface}"; else echo "[cohesix] interface=manifest"; fi; if test -n "${coh_static_ip}"; then echo "[cohesix] static-ip=${coh_static_ip}/${coh_static_prefix_len} gateway=${coh_static_gateway}"; fi; if test -n "${coh_wifi_ssid}"; then echo "[cohesix] wifi-ssid=${coh_wifi_ssid}"; fi'
setenv coh_load_driver_runtimes 'if fatload mmc 0:1 ${coh_runtime_cpio_addr} ${coh_runtime_cpio_file}; then echo "[cohesix] loaded ${coh_runtime_cpio_file} to ${coh_runtime_cpio_addr}"; else echo "[cohesix] ERROR: failed to load ${coh_runtime_cpio_file}"; setenv coh_boot_error 1; fi'
setenv coh_boot_loaded_image 'run coh_load_runtime_dtb; if test "${coh_boot_error}" = "1"; then echo "[cohesix] ERROR: boot aborted before driver runtime load"; else run coh_load_driver_runtimes; if test "${coh_boot_error}" = "1"; then echo "[cohesix] ERROR: boot aborted before USB quiesce"; else run coh_quiesce_usb; run coh_apply_dtb_policy; if test "${coh_boot_error}" = "1"; then echo "[cohesix] ERROR: boot aborted before kernel handoff"; else echo "[cohesix] loaded ${coh_image} and ${coh_runtime_cpio_file}; bootm with ${coh_dtb_file}"; bootm ${coh_addr} ${coh_runtime_cpio_addr} ${coh_dtb_addr}; echo "[cohesix] returned from image"; fi; fi; fi'
setenv coh_boot_sequence 'run coh_emit_policy_summary; if fatload mmc 0:1 ${coh_addr} ${coh_image}; then run coh_boot_loaded_image; else echo "[cohesix] primary image load failed: ${coh_image}"; if fatload mmc 0:1 ${coh_addr} ${coh_image_fallback}; then setenv coh_image ${coh_image_fallback}; run coh_boot_loaded_image; else echo "[cohesix] ERROR: failed to load ${coh_image} or fallback ${coh_image_fallback} from mmc 0:1"; echo "[cohesix] manual: fatls mmc 0:1"; echo "[cohesix] manual: fatload mmc 0:1 0x10000000 ${coh_image}"; echo "[cohesix] manual: fatload mmc 0:1 0x14000000 ${coh_dtb_file}"; echo "[cohesix] manual: bootm 0x10000000 - 0x14000000"; fi; fi'
setenv coh_prompt_dhcp 'run coh_prepare_input; cls; echo "[cohesix] Guided network setup"; echo "[cohesix] Select address acquisition mode"; echo "  1. DHCP ON (automatic address)"; echo "  2. DHCP OFF (static IPv4)"; echo "  3. Back to boot options"; setenv coh_choice; askenv coh_choice "Select option [1]: " 1; if test -z "${coh_choice}"; then setenv coh_choice 1; fi; if test "${coh_choice}" = "1"; then setenv coh_net_mode dhcp; setenv coh_static_ip ""; setenv coh_static_prefix_len ""; setenv coh_static_gateway ""; run coh_prompt_interface; elif test "${coh_choice}" = "2"; then setenv coh_net_mode static; run coh_prompt_interface; elif test "${coh_choice}" = "3"; then run coh_prompt_root; elif test "${coh_choice}" = "0"; then exit; else echo "[cohesix] invalid selection"; run coh_prompt_dhcp; fi'
setenv coh_prompt_interface 'run coh_prepare_input; cls; echo "[cohesix] Guided network setup"; echo "[cohesix] Select active interface"; echo "  1. Wired Ethernet (GENET)"; echo "  2. Wi-Fi (CYW43455)"; echo "  3. Back to DHCP selection"; setenv coh_choice; askenv coh_choice "Select option [1]: " 1; if test -z "${coh_choice}"; then setenv coh_choice 1; fi; if test "${coh_choice}" = "1"; then setenv coh_net_interface wired; setenv coh_wifi_ssid ""; setenv coh_wifi_psk ""; run coh_after_interface; elif test "${coh_choice}" = "2"; then setenv coh_net_interface wifi; run coh_after_interface; elif test "${coh_choice}" = "3"; then run coh_prompt_dhcp; elif test "${coh_choice}" = "0"; then exit; else echo "[cohesix] invalid selection"; run coh_prompt_interface; fi'
setenv coh_wifi_setup 'run coh_prepare_input; cls; echo "[cohesix] Configure Wi-Fi credentials"; askenv coh_wifi_ssid "Wi-Fi SSID (required): " 32; if test -z "${coh_wifi_ssid}"; then echo "[cohesix] Wi-Fi SSID is required"; run coh_prompt_interface; fi; askenv coh_wifi_psk "Wi-Fi PSK (blank for open network): " 64; if test "${coh_net_mode}" = "static"; then run coh_static_setup; else run coh_confirm_prompt; fi'
setenv coh_static_setup 'run coh_prepare_input; cls; echo "[cohesix] Configure static IPv4 for ${coh_net_interface}"; askenv coh_static_ip "Static IPv4 address (required): " 15; if test -z "${coh_static_ip}"; then echo "[cohesix] Static IPv4 address is required"; run coh_static_setup; fi; askenv coh_static_prefix_len "Prefix length (required, 1-32): " 2; if test -z "${coh_static_prefix_len}"; then echo "[cohesix] Prefix length is required"; run coh_static_setup; fi; askenv coh_static_gateway "Gateway IPv4 (optional): " 15; run coh_confirm_prompt'
setenv coh_after_interface 'if test "${coh_net_interface}" = "wifi"; then run coh_wifi_setup; elif test "${coh_net_mode}" = "static"; then run coh_static_setup; else run coh_confirm_prompt; fi'
setenv coh_confirm_prompt 'run coh_prepare_input; cls; echo "[cohesix] Review network settings"; run coh_emit_policy_summary; echo "  1. Boot with these settings"; echo "  2. Save settings and reboot"; echo "  3. Edit settings"; echo "  4. Discard changes and return"; echo "  0. Exit to U-Boot prompt"; setenv coh_choice; askenv coh_choice "Select option [1]: " 1; if test -z "${coh_choice}"; then setenv coh_choice 1; fi; if test "${coh_choice}" = "1"; then run coh_boot_sequence; elif test "${coh_choice}" = "2"; then run coh_persist_policy; reset; elif test "${coh_choice}" = "3"; then run coh_prompt_dhcp; elif test "${coh_choice}" = "4"; then run coh_load_saved_policy; run coh_prompt_root; elif test "${coh_choice}" = "0"; then exit; else echo "[cohesix] invalid selection"; run coh_confirm_prompt; fi'
setenv coh_prompt_root 'run coh_show_logo_splash; run coh_prepare_input; run coh_detect_saved_config; cls; echo "[cohesix] Cohesix boot options"; if test "${coh_has_saved_config}" = "1"; then echo "[cohesix] Saved network settings detected"; run coh_emit_policy_summary; echo "  1. Continue with existing config"; else echo "[cohesix] No saved network settings; manifest defaults remain active"; echo "  1. Boot with manifest defaults"; fi; echo "  2. Configure networking"; echo "  3. Toggle HDMI logo"; echo "  4. Restore manifest defaults"; echo "  5. Save current settings and reboot"; echo "  0. Exit to U-Boot prompt"; setenv coh_choice; askenv coh_choice "Select option [1]: " 1; if test -z "${coh_choice}"; then setenv coh_choice 1; fi; if test "${coh_choice}" = "1"; then run coh_boot_sequence; elif test "${coh_choice}" = "2"; then run coh_prompt_dhcp; elif test "${coh_choice}" = "3"; then run coh_toggle_logo; run coh_prompt_root; elif test "${coh_choice}" = "4"; then run coh_reset_policy; run coh_persist_policy; echo "[cohesix] manifest defaults restored"; run coh_prompt_root; elif test "${coh_choice}" = "5"; then run coh_persist_policy; reset; elif test "${coh_choice}" = "0"; then exit; else echo "[cohesix] invalid selection"; run coh_prompt_root; fi'
run coh_load_saved_policy
run coh_prompt_root
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
    local manifest_json="${ROOT_DIR}/out/manifests/root_task_resolved.json"
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
    local raw_dir
    raw_dir="$(dirname "$raw_cpio")"
    local runtime_root="${raw_dir}/driver-runtime-root"
    local runtime_bin="${runtime_root}/cohesix/bin"
    local runtime_artifact_dir="${ROOT_DIR}/target/aarch64-unknown-none/release"
    local strip_tool
    local bin
    local generic_runtime="${runtime_bin}/pi4-driver-runtime"
    local dedup_driver_runtimes=1

    assert_driver_runtime_elf_budgets "$runtime_artifact_dir"
    strip_tool="$(find_aarch64_strip || true)"
    [[ -n "$strip_tool" ]] || fail "aarch64 strip tool not found"
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
        require_file "${runtime_artifact_dir}/${bin}"
        install -m 0755 "${runtime_artifact_dir}/${bin}" "${runtime_bin}/${bin}"
        "$strip_tool" \
            --strip-all \
            --remove-section=.comment \
            --remove-section=.eh_frame \
            --remove-section=.eh_frame_hdr \
            "${runtime_bin}/${bin}"
        log "Staged isolated driver runtime: ${bin}"
    done
    cp -f "${runtime_bin}/pi4-driver-serial" "$generic_runtime"
    for bin in \
        pi4-driver-serial \
        pi4-driver-usb \
        pi4-driver-hdmi \
        pi4-driver-genet \
        pi4-driver-cyw43 \
        pi4-driver-sdio \
        pi4-driver-pcie
    do
        if ! cmp -s "$generic_runtime" "${runtime_bin}/${bin}"; then
            dedup_driver_runtimes=0
            break
        fi
    done
    if [[ "$dedup_driver_runtimes" == "1" ]]; then
        rm -f \
            "${runtime_bin}/pi4-driver-serial" \
            "${runtime_bin}/pi4-driver-usb" \
            "${runtime_bin}/pi4-driver-hdmi" \
            "${runtime_bin}/pi4-driver-genet" \
            "${runtime_bin}/pi4-driver-cyw43" \
            "${runtime_bin}/pi4-driver-sdio" \
            "${runtime_bin}/pi4-driver-pcie"
        log "Deduplicated identical Pi4 driver runtimes as cohesix/bin/pi4-driver-runtime"
    else
        rm -f "$generic_runtime"
        log "Pi4 driver runtimes differ; keeping per-role runtime images"
    fi

    (
        cd "$runtime_root"
        find cohesix -print | LC_ALL=C sort | cpio --reproducible -o -H newc > "$raw_cpio"
    )
    require_file "$raw_cpio"
    log "Packaged Pi4 driver runtime raw CPIO at ${raw_cpio}"
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
    stage_driver_runtime_payload "$mkimage_bin"
    write_linux_wifi_debug_helpers

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

    require_file "${STAGE_DIR}/boot.scr.uimg"
    log "Staged Pi4 payload at ${STAGE_DIR}"
}

flash_sd_card() {
    local disk="$1"
    local wait_attempts=45

    command -v diskutil >/dev/null 2>&1 || fail "diskutil not found"
    command -v rsync >/dev/null 2>&1 || fail "rsync not found"

    [[ "$disk" == /dev/disk* ]] || fail "--flash-disk must look like /dev/diskN"
    diskutil info "$disk" >/dev/null 2>&1 || fail "disk not found: ${disk}"

    log "Flashing ${disk} (this erases the target disk)"
    diskutil unmountDisk force "$disk" >/dev/null 2>&1 || true
    local erase_status=0
    if diskutil eraseDisk FAT32 "$DISK_LABEL" MBRFormat "$disk" >/dev/null; then
        erase_status=0
    else
        erase_status=$?
        log "diskutil eraseDisk returned status=${erase_status}; checking for mounted ${DISK_LABEL} partition before failing"
    fi

    local part=""
    local volume=""
    if ! resolve_flash_partition_after_erase "$disk" "$DISK_LABEL" "$wait_attempts"; then
        fail "failed to find mounted FAT partition after erasing ${disk}"
    fi
    part="$FLASH_PARTITION_DEVICE"
    volume="$FLASH_PARTITION_MOUNT"
    [[ -n "$part" && -d "$volume" ]] || fail "failed to find mounted FAT partition after erasing ${disk}"
    if [[ "$erase_status" -ne 0 ]]; then
        log "Continuing after recoverable eraseDisk status=${erase_status}; using ${part} at ${volume}"
    fi
    disable_spotlight_for_flash_volume "$volume"

    COPYFILE_DISABLE=1 rsync -a --delete \
      --exclude=".Spotlight-V100" \
      --exclude=".fseventsd" \
      --exclude=".Trashes" \
      --exclude=".metadata_never_index" \
      --exclude="._*" \
      "${STAGE_DIR}/" "${volume}/"

    disable_spotlight_for_flash_volume "$volume"
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

    unmount_flashed_disk "$disk" "$volume"
    log "Flash complete and unmounted: ${disk}"
}

diskutil_info_value() {
    local target="$1"
    local key="$2"
    diskutil info "$target" 2>/dev/null | awk -F: -v key="$key" '
        $1 ~ "^[[:space:]]*" key "$" {
            value=$2
            sub(/^[[:space:]]+/, "", value)
            sub(/[[:space:]]+$/, "", value)
            print value
            exit
        }
    '
}

FLASH_PARTITION_DEVICE=""
FLASH_PARTITION_MOUNT=""
resolve_flash_partition_after_erase() {
    local disk="$1"
    local label="$2"
    local wait_attempts="$3"
    local attempt
    local candidate
    local part
    local volume

    FLASH_PARTITION_DEVICE=""
    FLASH_PARTITION_MOUNT=""
    for attempt in $(seq 1 "$wait_attempts"); do
        for candidate in "${disk}s1" "/Volumes/${label}" /Volumes/"${label}"\ *; do
            [[ -e "$candidate" || -d "$candidate" ]] || continue
            part="$(diskutil_info_value "$candidate" "Device Node")"
            volume="$(diskutil_info_value "$candidate" "Mount Point")"
            if [[ -n "$part" ]]; then
                if [[ -z "$volume" || "$volume" == "Not mounted" ]]; then
                    diskutil mount "$part" >/dev/null 2>&1 || true
                    volume="$(diskutil_info_value "$part" "Mount Point")"
                fi
                if [[ -d "$volume" ]]; then
                    FLASH_PARTITION_DEVICE="$part"
                    FLASH_PARTITION_MOUNT="$volume"
                    return 0
                fi
            fi
        done

        part="$(diskutil list | awk -v label="$label" '$0 ~ label { print "/dev/" $NF; exit }')"
        if [[ -n "$part" && "$part" == /dev/disk*s* ]]; then
            diskutil mount "$part" >/dev/null 2>&1 || true
            volume="$(diskutil_info_value "$part" "Mount Point")"
            if [[ -d "$volume" ]]; then
                FLASH_PARTITION_DEVICE="$part"
                FLASH_PARTITION_MOUNT="$volume"
                return 0
            fi
        fi

        if diskutil info "$disk" >/dev/null 2>&1; then
            diskutil mountDisk "$disk" >/dev/null 2>&1 || true
        fi
        sleep 1
    done
    return 1
}

disable_spotlight_for_flash_volume() {
    local volume="$1"

    # macOS can start Spotlight metadata sync on a freshly erased FAT volume
    # before the final unmount, which makes diskutil report an mdsync dissenter.
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

    log "Final volume unmount was blocked; forcing whole-disk unmount for ${disk}"
    if ! output="$(diskutil unmountDisk force "$disk" 2>&1)"; then
        fail "failed to unmount flashed disk ${disk}: ${output}"
    fi
}

main() {
    parse_args "$@"
    validate_menu_input_mode
    canonicalize_input_paths

    cd "$ROOT_DIR"
    trap cleanup EXIT

    if [[ "${CLEAN_BUILD}" -eq 1 && "${SKIP_BUILD}" -eq 1 ]]; then
        fail "--clean cannot be combined with --skip-build"
    fi
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
        local sel4_source_dir
        sel4_source_dir="$(resolve_sel4_source_dir)"
        verify_pi4_sel4_xhci_device_untyped "$sel4_source_dir"
        verify_skip_build_image_fresh
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
