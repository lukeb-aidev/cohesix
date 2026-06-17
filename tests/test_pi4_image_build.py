# Author: Lukas Bower
# Purpose: Regression tests for the Raspberry Pi 4 image build wrapper.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4-image-build.sh."""

import pathlib


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "pi4-image-build.sh"


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
