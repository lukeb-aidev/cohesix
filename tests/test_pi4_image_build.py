# Author: Lukas Bower
# Purpose: Regression tests for the Raspberry Pi 4 image build wrapper.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4-image-build.sh."""

import pathlib


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "pi4-image-build.sh"
U_BOOT_DEFCONFIG_PATH = (
    REPO_ROOT / "third_party" / "u-boot" / "configs" / "rpi_4_defconfig"
)
U_BOOT_GENERATED_DEFCONFIG_PATH = (
    REPO_ROOT / "third_party" / "u-boot" / "generated_defconfig-e"
)


def test_pi4_image_build_respects_cargo_target_dir_for_root_task() -> None:
    """The flashed root-task must come from the same target dir Cargo built."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'local target_dir="${CARGO_TARGET_DIR:-${ROOT_DIR}/target}"' in source
    assert 'root_task_elf="$(root_task_release_elf_path)"' in source
    assert (
        'local root_task_elf="${ROOT_DIR}/target/aarch64-unknown-none/release/root-task"'
        not in source
    )


def test_pi4_image_build_defaults_to_pi4_release_features() -> None:
    """The image path must compile the same Pi 4 release feature bundle as tests."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'ROOT_TASK_FEATURES="release-pi4,bootstrap-trace"' in source
    assert "(default: release-pi4,bootstrap-trace)" in source


def test_pi4_image_build_prefers_repo_local_sel4_build_tree() -> None:
    """Default staging must not silently use a stale home-directory Pi image."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'DEFAULT_REPO_SEL4_BUILD_DIR="${ROOT_DIR}/seL4/build_UBOOT"' in source
    assert 'SEL4_BUILD_DIR="${DEFAULT_REPO_SEL4_BUILD_DIR}"' in source
    assert "default: repo seL4/build_UBOOT" in source


def test_pi4_image_build_skip_build_rejects_stale_selected_image() -> None:
    """Flash-only retries must fail closed when source is newer than the image."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert "verify_skip_build_image_fresh" in source
    assert "--skip-build selected stale seL4 image" in source
    assert 'apps/root-task/src' in source
    assert 'apps/root-task/src/generated' in source
    assert 'apps/pi4-driver-runtime/src' in source


def test_pi4_image_build_defaults_to_usb_uboot_menu_input() -> None:
    """The HDMI setup menu must keep USB keyboard input working by default."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'U_BOOT_MENU_INPUT="${COHESIX_UBOOT_MENU_INPUT:-usb}"' in source
    assert "--uboot-menu-input <m>" in source
    assert 'validate_menu_input_mode' in source
    assert 'setenv coh_menu_input __COH_MENU_INPUT__' in source
    assert 'test "${coh_menu_input}" = "usb"' in source
    assert 'sed -i \'\' "s/__COH_MENU_INPUT__/${U_BOOT_MENU_INPUT}/g" "$out"' in source
    assert "setenv coh_logo_delay 1" in source


def test_pi4_image_build_keeps_dtb_policy_handoff_common() -> None:
    """Menu input changes must not bypass DTB policy handoff to seL4."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'setenv coh_apply_dtb_policy' in source
    for prop in (
        "cohesix,net-mode",
        "cohesix,net-interface",
        "cohesix,static-ipv4",
        "cohesix,static-prefix-len",
        "cohesix,static-gateway",
        "cohesix,wifi-ssid",
        "cohesix,wifi-psk",
    ):
        assert prop in source
    assert 'bootm ${coh_addr} ${coh_runtime_cpio_addr} ${coh_dtb_addr}' in source


def test_pi4_image_build_does_not_echo_wifi_psk_to_serial() -> None:
    """Wi-Fi PSK entry must stay inside the USB local console."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]

    wifi_setup = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_wifi_setup ")
    )

    assert "askenv coh_wifi_psk" in wifi_setup
    assert "run coh_begin_wifi_secret_input" in wifi_setup
    assert "run coh_end_wifi_secret_input" in wifi_setup
    assert wifi_setup.index("run coh_begin_wifi_secret_input") < wifi_setup.index(
        "askenv coh_wifi_psk"
    )
    assert wifi_setup.index("askenv coh_wifi_psk") < wifi_setup.rindex(
        "run coh_end_wifi_secret_input"
    )
    assert (
        "setenv coh_begin_wifi_secret_input "
        "'setenv stdin usbkbd; setenv stdout vidconsole; setenv stderr vidconsole'"
    ) in boot_template
    assert (
        "setenv coh_end_wifi_secret_input "
        "'if test \"${coh_usb_input_ready}\" = \"1\"; then setenv stdin usbkbd,serial; "
        "else setenv stdin serial; fi; setenv stdout serial,vidconsole; "
        "setenv stderr serial,vidconsole'"
    ) in boot_template
    assert "USB-only Wi-Fi credential entry; serial echo disabled" in boot_template
    assert "Serial Wi-Fi password entry is disabled because U-Boot echoes input" in (
        boot_template
    )
    assert "coh_wifi_psk in ${coh_policy_file}" in boot_template
    assert "boot.cmd does not suppress serial echo during Wi-Fi secret entry" in source
    assert "boot.cmd does not collect Wi-Fi PSKs in the protected USB-only prompt" in source


def test_pi4_image_build_fastboot_prefers_marker_and_falls_back_to_saved_policy_reset() -> None:
    """Software-reset fallback must be gated by saved Cohesix policy."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]
    detect_fastboot = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_detect_fastboot ")
    )
    marker_check = 'itest.l ${coh_fastboot_rsts_marker} == ${coh_fastboot_rsts_magic}'
    reset_check = 'itest.l ${coh_fastboot_rsts_reset} == ${coh_fastboot_rsts_reset_mask}'

    assert "setenv coh_fastboot_rsts_addr 0xfe100020" in source
    assert "setenv coh_fastboot_rsts_mask 0x00ff0000" in source
    assert "setenv coh_fastboot_rsts_magic 0x00430000" in source
    assert "setenv coh_fastboot_rsts_reset_mask 0x00000400" in source
    assert "setenv coh_fastboot_rsts_clear_mask 0xff00ffff" in source
    assert "setenv coh_fastboot_rsts_low_mask 0x00000020" not in source
    assert marker_check in detect_fastboot
    assert "coh_fastboot_rsts_reset" in detect_fastboot
    assert 'test "${coh_fastboot}" != "1"' in detect_fastboot
    assert reset_check in detect_fastboot
    assert 'test "${coh_has_saved_config}" = "1"' in detect_fastboot
    assert "software-reset-saved-policy" in boot_template
    assert "reboot fast boot: source=${coh_fastboot_source}" in boot_template
    assert "reset=${coh_fastboot_rsts_reset} saved=${coh_has_saved_config}" in boot_template
    generated_tail = boot_template[boot_template.rindex("run coh_force_serial_preboot") :]
    assert generated_tail.index("run coh_load_saved_policy") < generated_tail.index(
        "run coh_detect_saved_config"
    )
    assert generated_tail.index("run coh_detect_saved_config") < generated_tail.index(
        "run coh_maybe_fastboot"
    )


def test_pi4_image_build_allows_serial_menu_without_usb_keyboard() -> None:
    """The explicit serial menu opt-out must not require U-Boot USB keyboard support."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    usb_gate = 'if [[ "${U_BOOT_MENU_INPUT}" == "usb" ]]; then'
    usb_keyboard = "u-boot.bin is missing CONFIG_USB_KEYBOARD for --uboot-menu-input usb"
    usb_poll = (
        "u-boot.bin is missing a supported USB keyboard polling mode "
        "for --uboot-menu-input usb"
    )

    assert usb_gate in source
    assert source.index(usb_gate) < source.index(usb_keyboard)
    assert source.index(usb_gate) < source.index(usb_poll)


def test_pi4_uboot_defconfigs_keep_remote_recovery_bootdelay() -> None:
    """Pi 4 U-Boot must leave a serial window for remote menu recovery."""

    for path in (U_BOOT_DEFCONFIG_PATH, U_BOOT_GENERATED_DEFCONFIG_PATH):
        source = path.read_text(encoding="utf-8")

        assert "CONFIG_BOOTDELAY=2" in source
        assert "CONFIG_BOOTDELAY=0" not in source


def test_pi4_image_build_verifies_remote_recovery_bootdelay() -> None:
    """The image wrapper must reject stale U-Boot binaries with no abort window."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert "CONFIG_BOOTDELAY=2" in source
    assert "2-second serial autoboot abort window" in source


def test_pi4_image_build_keeps_recovery_menu_escape_before_fastboot() -> None:
    """Manual U-Boot recovery must bypass saved-policy fastboot on request."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]

    recovery_guard = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_maybe_fastboot_or_recovery ")
    )
    generated_tail = boot_template[boot_template.rindex("run coh_force_serial_preboot") :]

    assert "setenv bootdelay 0" not in boot_template
    assert 'test "${coh_recovery_menu}" = "1"' in recovery_guard
    assert "recovery menu requested: skipping reboot fast boot" in recovery_guard
    assert "else run coh_maybe_fastboot" in recovery_guard
    assert generated_tail.index("run coh_detect_saved_config") < generated_tail.index(
        "run coh_maybe_fastboot_or_recovery"
    )
    assert generated_tail.index("run coh_maybe_fastboot_or_recovery") < (
        generated_tail.index("run coh_prompt_root")
    )
