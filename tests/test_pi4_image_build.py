# Author: Lukas Bower
# Purpose: Regression tests for the Raspberry Pi 4 image build wrapper.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4-image-build.sh."""

import hashlib
import pathlib
import shutil
import subprocess

import pytest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "pi4-image-build.sh"
ROOT_TASK_BUILD_RS_PATH = REPO_ROOT / "apps" / "root-task" / "build.rs"
U_BOOT_DEFCONFIG_PATH = (
    REPO_ROOT / "third_party" / "u-boot" / "configs" / "rpi_4_defconfig"
)
U_BOOT_GENERATED_DEFCONFIG_PATH = (
    REPO_ROOT / "third_party" / "u-boot" / "generated_defconfig-e"
)
PI4_WIFI_BUNDLE_PATH = (
    REPO_ROOT
    / "third_party"
    / "raspberry-pi-firmware"
    / "v1.50"
    / "firmware"
    / "cyw43455-linux-capture"
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


def test_pi4_image_build_uses_third_party_wifi_firmware_bundle() -> None:
    """Pi 4 release builds must not depend on generated capture outputs."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'FIRMWARE_DIR="${ROOT_DIR}/third_party/raspberry-pi-firmware/v1.50"' in (
        source
    )
    assert (
        'PI4_WIFI_FIRMWARE_DIR="${COHESIX_PI4_WIFI_FIRMWARE_DIR:-${FIRMWARE_DIR}/firmware/cyw43455-linux-capture}"'
        in source
    )
    assert 'COHESIX_PI4_WIFI_FIRMWARE_DIR="${PI4_WIFI_FIRMWARE_DIR}"' in source
    assert 'out/pi4-linux-capture' not in source


def test_root_task_build_defaults_to_checked_in_wifi_firmware_bundle() -> None:
    """Root-task must find the Pi 4 Wi-Fi bundle after deleting out/."""

    source = ROOT_TASK_BUILD_RS_PATH.read_text(encoding="utf-8")

    assert (
        '"third_party/raspberry-pi-firmware/v1.50/firmware/cyw43455-linux-capture"'
        in source
    )
    assert "out/pi4-linux-capture" not in source
    assert "resources/fixtures/pi4-linux-capture" not in source


def test_checked_in_wifi_firmware_bundle_matches_release_contract() -> None:
    """The default Pi 4 Wi-Fi bundle must be the pinned Linux Pi 4B identity."""

    expected = {
        "cyfmac43455-sdio.bin": (
            609309,
            "d608f866582519c0a28d86db43040f4f1b98dd1d153e72e9752586546b4a36c3",
        ),
        "brcmfmac43455-sdio.raspberrypi,4-model-b.txt": (
            2074,
            "ca709be81a78bdb6932936374f39943acbd7af07fae6151011127599a3ce9e3d",
        ),
        "cyfmac43455-sdio.clm_blob": (
            2676,
            "9823842cae9fb9a5dd1e5fb31f595516ec7deee341354bef30bb3026eee29cc1",
        ),
    }

    for filename, (size, digest) in expected.items():
        data = (PI4_WIFI_BUNDLE_PATH / filename).read_bytes()

        assert len(data) == size
        assert hashlib.sha256(data).hexdigest() == digest


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


def test_pi4_image_build_requires_canonical_runtime_profile_and_mkimage() -> None:
    """Image staging must not silently reconfigure or use ambient host tools."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    validator = source[
        source.index("validate_pi4_sel4_build()") : source.index(
            "resolve_mkimage()"
        )
    ]

    assert 'PI4_SEL4_PROFILE="pi4_diagnostic"' in source
    assert 'COHESIX_SEL4_PROJECT_ROOT:PATH=' in source
    assert '--profile "$PI4_SEL4_PROFILE"' in validator
    assert '--require-source' in validator
    assert '--require-artifacts' in validator
    assert '--for-runtime' in validator
    assert "cmake -S" not in validator
    assert "configure_pi4_sel4_build" not in source
    assert (
        'local canonical="${ROOT_DIR}/out/toolchain/u-boot-tools-build/tools/mkimage"'
        in source
    )
    assert "command -v mkimage" not in source
    assert 'third_party/u-boot/tools/mkimage"' not in source
    assert "DEFAULT_SEL4_KERNEL_SOURCE_DIR" not in source

    kernel_resolver = source[
        source.index("resolve_sel4_kernel_source_dir()") : source.index(
            "verify_pi4_sel4_xhci_device_untyped()"
        )
    ]
    assert "COHESIX_SEL4_PROJECT_ROOT:PATH=" in kernel_resolver
    assert 'cached="${source_root}/kernel"' in kernel_resolver

    domain_guard = source[
        source.index("verify_one_domain_schedule_cache_absent()") : source.index(
            "ensure_sel4_lib_available()"
        )
    ]
    assert "forbidden KernelDomainSchedule" in domain_guard
    assert "mktemp" not in domain_guard
    assert "mv " not in domain_guard

    skip_branch = source[source.index('if [[ "$SKIP_BUILD" -eq 0 ]]') :]
    assert skip_branch.index("validate_pi4_sel4_build") < skip_branch.index(
        "verify_skip_build_provenance"
    )


def test_pi4_image_build_defaults_to_usb_uboot_menu_input() -> None:
    """The HDMI setup menu must keep USB keyboard input working by default."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'U_BOOT_MENU_INPUT="usb"' in source
    assert 'U_BOOT_MENU_INPUT="${COHESIX_UBOOT_MENU_INPUT:-usb}"' not in source
    assert "--uboot-menu-input <m>" in source
    assert 'validate_menu_input_mode' in source
    assert 'setenv coh_menu_input __COH_MENU_INPUT__' in source
    assert 'test "${coh_menu_input}" = "usb"' in source
    assert 'sed -i \'\' "s/__COH_MENU_INPUT__/${U_BOOT_MENU_INPUT}/g" "$out"' in source
    assert "setenv coh_logo_delay 1" in source


def test_pi4_image_build_ignores_legacy_menu_input_environment() -> None:
    """A stale shell variable must not silently disable the USB U-Boot menu."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'U_BOOT_MENU_INPUT_SOURCE="default"' in source
    assert 'U_BOOT_MENU_INPUT_SOURCE="cli"' in source
    assert 'U_BOOT_MENU_INPUT="${COHESIX_UBOOT_MENU_INPUT:-usb}"' not in source
    assert "Ignoring COHESIX_UBOOT_MENU_INPUT=" in source
    assert "use --uboot-menu-input for explicit serial lab captures" in source


def test_pi4_image_build_keeps_firmware_second_stage_debug_quiet() -> None:
    """The staged Pi firmware config must not flood HDMI/serial with MESS spam."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    config_start = source.index('cat > "${STAGE_DIR}/config.txt" <<EOF')
    config_template = source[config_start : source.index("\nEOF", config_start)]

    assert "enable_uart=1" in config_template
    assert "uart_2ndstage=1" not in config_template


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


def test_pi4_image_build_serial_wifi_missing_policy_uses_simple_prompt() -> None:
    """Serial-only Wi-Fi setup must use the proven non-secret staging prompt."""

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

    assert "setenv coh_wifi_serial_recovery " not in boot_template
    assert "run coh_wifi_serial_recovery" not in wifi_setup
    assert "U-Boot policy missing:" not in boot_template
    assert "file-based policy recovery" not in boot_template
    assert "do not type PSK on serial" not in boot_template
    assert "Serial Wi-Fi password entry is disabled because U-Boot echoes input" in (
        wifi_setup
    )
    assert (
        "Stage coh_wifi_ssid and coh_wifi_psk in ${coh_policy_file} on the boot "
        "partition, then reboot"
    ) in wifi_setup
    assert "run coh_prompt_interface" in wifi_setup


def test_pi4_image_build_mounts_target_before_preserving_policy() -> None:
    """Reflash must not drop saved Wi-Fi policy just because the card is unmounted."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    flash_start = source.index("flash_sd_card() {")
    flash_body = source[flash_start : source.index("\ndiskutil_info_value() {")]

    assert 'diskutil mountDisk "$disk" >/dev/null 2>&1 || true' in flash_body
    assert 'preflash_volume="/Volumes/${DISK_LABEL}"' in flash_body
    assert 'disk_basename="${disk#/dev/}"' in flash_body
    assert 'diskutil_info_value "$preflash_volume" "Part of Whole"' in flash_body
    assert '[[ "$preflash_whole" != "$disk_basename" ]]' in flash_body
    assert 'cp -f "${preflash_volume}/${policy_file}" "$preserved_policy"' in flash_body


def test_pi4_image_build_keeps_per_role_driver_runtime_artifacts() -> None:
    """Runtime CPIO entries must match generated per-role artifact identity."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert "Keeping per-role Pi4 driver runtime images" in source
    assert "verify_driver_runtime_cpio_entries" in source
    assert "missing Pi4 driver runtime artifact in CPIO" in source
    assert "generic Pi4 driver runtime artifact is not" in source
    assert "cohesix/bin/pi4-driver-cyw43" in source
    assert "Deduplicated identical Pi4 driver runtimes" not in source
    assert 'local generic_runtime="${runtime_bin}/pi4-driver-runtime"' not in source


def test_pi4_image_build_reports_reset_markers_without_autoboot() -> None:
    """Saved policy must not bypass the interactive U-Boot menu."""

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
    assert reset_check not in detect_fastboot
    assert 'test "${coh_has_saved_config}" = "1"' not in detect_fastboot
    assert "software-reset-saved-policy" not in boot_template
    assert "reboot fast boot: source=${coh_fastboot_source}" not in boot_template
    assert "reset=${coh_fastboot_rsts_reset} saved=${coh_has_saved_config}" in boot_template
    generated_tail = boot_template[boot_template.rindex("run coh_force_serial_preboot") :]
    assert generated_tail.index("run coh_load_saved_policy") < generated_tail.index(
        "run coh_detect_saved_config"
    )
    assert generated_tail.index("run coh_detect_saved_config") < generated_tail.index(
        "run coh_detect_fastboot"
    )
    assert generated_tail.index("run coh_report_fastboot_miss") < generated_tail.index(
        "run coh_prompt_root"
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


def test_pi4_image_build_quiesces_uboot_usb_unconditionally() -> None:
    """Cohesix handoff must request U-Boot USB stop even on serial menu paths."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]
    quiesce = next(
        line for line in boot_template.splitlines() if line.startswith("setenv coh_quiesce_usb ")
    )

    assert 'if usb stop; then' in quiesce
    assert 'test "${coh_usb_input_ready}" = "1"' not in quiesce
    assert "usb stop failed or was inactive before Cohesix boot" in quiesce


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


def test_pi4_image_build_enters_interactive_menu_after_marker_diagnostics() -> None:
    """The generated boot script must default to the interactive menu."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]

    generated_tail = boot_template[boot_template.rindex("run coh_force_serial_preboot") :]

    assert "setenv bootdelay 0" not in boot_template
    assert "coh_maybe_fastboot_or_recovery" not in boot_template
    assert "run coh_maybe_fastboot" not in boot_template
    assert "reboot fast boot" not in boot_template
    assert generated_tail.index("run coh_detect_saved_config") < generated_tail.index(
        "run coh_detect_fastboot"
    )
    assert generated_tail.index("run coh_detect_fastboot") < generated_tail.index(
        "run coh_report_fastboot_miss"
    )
    assert generated_tail.index("run coh_report_fastboot_miss") < (
        generated_tail.index("run coh_prompt_root")
    )


def test_pi4_image_build_normalizes_menu_choices_before_dispatch() -> None:
    """Serial-paced U-Boot choices must dispatch after bounded normalization."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]

    assert "setenv coh_normalize_choice" in boot_template
    assert 'askenv coh_choice "Select option [1]: " 4' in boot_template
    assert 'test "${coh_choice}" = " 2"' in boot_template
    assert "run coh_read_choice; if test" in boot_template
    root_menu = boot_template[boot_template.index("setenv coh_prompt_root") :]
    assert 'elif test "${coh_choice}" = "2"; then run coh_prompt_dhcp' in root_menu


def test_pi4_image_build_binds_one_clean_repository_snapshot() -> None:
    """Root compilation and metadata publication must share one clean HEAD."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert "capture_exact_source_identity" in source
    assert "status --porcelain=v1 --untracked-files=all" in source
    assert "exact Pi image builds require a clean checkout" in source
    assert 'EXACT_GIT_COMMIT="$(git -C "$ROOT_DIR" rev-parse --verify HEAD)"' in source
    assert 'COHESIX_EXACT_GIT_COMMIT="$EXACT_GIT_COMMIT"' in source
    assert "COHESIX_EXACT_SOURCE_CLEAN=1" in source
    assert 'capture_build_repository_state' in source
    for phase in (
        "after root-task build",
        "after final seL4 wrapper build",
        "before identity metadata publication",
        "after identity metadata publication",
    ):
        assert f'verify_build_repository_state "{phase}"' in source


def test_pi4_image_build_proves_root_archive_and_v2_identity() -> None:
    """The sealed wrapper must bind exact rootserver membership and provenance."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'embedded_root_cpio="${SEL4_BUILD_DIR}/elfloader/archive.archive.o.cpio"' in source
    assert '--expected-root-elf "$expected_root_elf"' in source
    assert '--expected-root-cpio "$expected_root_cpio"' in source
    assert '--metadata "$identity_metadata"' in source
    assert '--git-commit "$EXACT_GIT_COMMIT"' in source
    assert "--source-tree-clean" in source
    assert "verify-metadata" in source
    assert '--expected-git-commit "$EXACT_GIT_COMMIT"' in source
    assert '--expected-build-id "$EXACT_BUILD_ID"' in source
    assert 'cmp -s "$staged_image" "$fallback_image"' in source
    assert '"$mkimage_bin" -l "$staged_image"' in source
    assert '"$mkimage_bin" -l "$fallback_image"' in source
    assert "PI4_IMAGE_IDENTITY_SCHEME=cohesix-pi4-image-identity/v2" in source


def test_pi4_image_build_publishes_metadata_after_final_image_rename() -> None:
    """Identity-v2 metadata must record the final public image path."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    seal_start = source.index("seal_staged_pi4_image() {")
    seal_end = source.index("\nfind_aarch64_strip() {", seal_start)
    seal_body = source[seal_start:seal_end]

    seal_call = 'pi4_image_identity.py" seal'
    rename = 'mv -f "$unsealed_image" "$staged_image"'
    publish = 'pi4_image_identity.py" verify'
    assert seal_body.index(seal_call) < seal_body.index(rename)
    assert seal_body.index(rename) < seal_body.index(publish)
    pre_rename = seal_body[seal_body.index(seal_call) : seal_body.index(rename)]
    assert '--metadata "$identity_metadata"' not in pre_rename


def test_pi4_image_build_rechecks_identity_after_proof_publication() -> None:
    """No late staging operation can escape final image and fallback checks."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    stage_start = source.index("stage_sd_payload() {")
    stage_end = source.index("\nflash_sd_card() {", stage_start)
    stage_body = source[stage_start:stage_end]

    assert stage_body.index("write_pi4_runtime_dma_build_proof") < stage_body.index(
        'verify_final_staged_pi4_image "$mkimage_bin"'
    )
    assert "after final staged-image verification" in source


def _copy_sourceable_build_script(tmp_path: pathlib.Path) -> pathlib.Path:
    """Create a minimal repository containing a sourceable build wrapper."""

    scripts = tmp_path / "scripts"
    scripts.mkdir()
    copied = scripts / SCRIPT_PATH.name
    shutil.copy2(SCRIPT_PATH, copied)
    (tmp_path / "tracked.txt").write_text("alpha\n", encoding="utf-8")
    subprocess.run(("git", "init", "-q"), cwd=tmp_path, check=True)
    subprocess.run(
        ("git", "config", "user.email", "tests@example.invalid"),
        cwd=tmp_path,
        check=True,
    )
    subprocess.run(
        ("git", "config", "user.name", "Cohesix tests"),
        cwd=tmp_path,
        check=True,
    )
    subprocess.run(("git", "add", "."), cwd=tmp_path, check=True)
    subprocess.run(
        ("git", "commit", "-q", "-m", "fixture"), cwd=tmp_path, check=True
    )
    return copied


def _source_function(script: pathlib.Path, command: str) -> subprocess.CompletedProcess[str]:
    """Source a copied wrapper and execute one function without running main."""

    return subprocess.run(
        ("bash", "-c", 'source "$1"; eval "$2"', "build-test", str(script), command),
        cwd=script.parent.parent,
        check=False,
        capture_output=True,
        text=True,
    )


def test_repository_state_digest_binds_tracked_and_untracked_contents(
    tmp_path: pathlib.Path,
) -> None:
    """Equal status paths with different bytes cannot share one build snapshot."""

    script = _copy_sourceable_build_script(tmp_path)
    initial = _source_function(script, "repository_state_digest")
    assert initial.returncode == 0, initial.stderr

    (tmp_path / "tracked.txt").write_text("bravo\n", encoding="utf-8")
    tracked_first = _source_function(script, "repository_state_digest")
    (tmp_path / "tracked.txt").write_text("delta\n", encoding="utf-8")
    tracked_second = _source_function(script, "repository_state_digest")
    assert tracked_first.stdout.strip() != tracked_second.stdout.strip()

    (tmp_path / "untracked.txt").write_text("first\n", encoding="utf-8")
    untracked_first = _source_function(script, "repository_state_digest")
    (tmp_path / "untracked.txt").write_text("other\n", encoding="utf-8")
    untracked_second = _source_function(script, "repository_state_digest")
    assert untracked_first.stdout.strip() != untracked_second.stdout.strip()


@pytest.mark.parametrize(
    "image_name",
    [
        "nested/image",
        ".",
        "..",
        "sel4test-driver-image-arm-bcm2711",
        "pi4-image-identity.json",
        "config.txt",
        "Config.txt",
        "u-boot.bin",
        "boot.scr.uimg",
        "BOOT.SCR.UIMG",
        "start4.elf",
        "cohesix.env",
        "PI4-IMAGE-IDENTITY.JSON",
        "image.bin.",
    ],
)
def test_image_name_aliases_fail_before_stage_deletion(
    tmp_path: pathlib.Path,
    image_name: str,
) -> None:
    """Unsafe or aliased output names are rejected before any stage write."""

    script = _copy_sourceable_build_script(tmp_path)
    result = _source_function(
        script,
        f'COHESIX_IMAGE_NAME={image_name!r}; STAGE_DIR="$ROOT_DIR/out"; validate_output_paths',
    )

    assert result.returncode != 0
    assert "--image-name" in result.stderr


def test_stage_dir_cannot_delete_tracked_checkout_subtrees(
    tmp_path: pathlib.Path,
) -> None:
    """The unconditional staging cleanup is confined to the checkout's out tree."""

    script = _copy_sourceable_build_script(tmp_path)
    result = _source_function(
        script,
        'COHESIX_IMAGE_NAME=image.bin; STAGE_DIR="$ROOT_DIR/apps"; validate_output_paths',
    )

    assert result.returncode != 0
    assert "strictly under" in result.stderr


def test_skip_build_requires_exact_manifest_feature_and_profile_provenance() -> None:
    """Reused wrappers cannot be relabelled from a self-consistent wrong build."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert "write_sel4_image_provenance" in source
    assert "verify_skip_build_provenance" in source
    skip_branch = source[source.index('if [[ "$SKIP_BUILD" -eq 0 ]]') :]
    assert skip_branch.index("verify_skip_build_provenance") < skip_branch.index(
        "run_coh_rtc_codegen"
    )
    assert skip_branch.index("run_coh_rtc_codegen") < skip_branch.index(
        "capture_build_repository_state"
    )
    for field in (
        "source_manifest_sha256",
        "root_task_features",
        "sel4_cmake_cache_sha256",
        "sel4_timer_header_sha256",
        "wrapper_sha256",
        "rootserver_sha256",
        "rootserver_cpio_sha256",
    ):
        assert field in source


@pytest.mark.parametrize(
    "tampered_relative_path",
    [
        "sel4-build/images/sel4test-driver-image-arm-bcm2711",
        "sel4-build/elfloader/rootserver",
        "sel4-build/elfloader/archive.archive.o.cpio",
        "manifest.toml",
        "sel4-build/CMakeCache.txt",
        "sel4-build/kernel/gen_headers/plat/platform_gen.h",
    ],
)
def test_skip_build_provenance_rejects_each_bound_artifact_tamper(
    tmp_path: pathlib.Path,
    tampered_relative_path: str,
) -> None:
    """Every source, profile, root, archive, and wrapper digest is enforced."""

    script = _copy_sourceable_build_script(tmp_path)
    artifacts = {
        "sel4-build/images/sel4test-driver-image-arm-bcm2711": b"wrapper\n",
        "sel4-build/elfloader/rootserver": b"rootserver\n",
        "sel4-build/elfloader/archive.archive.o.cpio": b"cpio\n",
        "manifest.toml": b"manifest\n",
        "sel4-build/CMakeCache.txt": b"cache\n",
        "sel4-build/kernel/gen_headers/plat/platform_gen.h": b"timer\n",
    }
    for relative_path, payload in artifacts.items():
        path = tmp_path / relative_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(payload)

    shell_state = (
        'SEL4_BUILD_DIR="$ROOT_DIR/sel4-build"; '
        'SEL4_UPSTREAM_IMAGE_NAME="sel4test-driver-image-arm-bcm2711"; '
        'MANIFEST_PATH="$ROOT_DIR/manifest.toml"; '
        f'EXACT_GIT_COMMIT={"a" * 40!r}; '
        "EXACT_BUILD_TIMESTAMP='2026-07-16T00:00:00Z'; "
        "ROOT_TASK_FEATURES='release-pi4,bootstrap-trace'; "
    )
    published = _source_function(
        script,
        f"{shell_state} write_sel4_image_provenance; verify_skip_build_provenance",
    )
    assert published.returncode == 0, published.stderr

    tampered = tmp_path / tampered_relative_path
    tampered.write_bytes(tampered.read_bytes() + b"tamper\n")
    verified = _source_function(
        script,
        f"{shell_state} verify_skip_build_provenance",
    )

    assert verified.returncode != 0
    assert "provenance does not match" in verified.stderr
