# Author: Lukas Bower
# Purpose: Regression tests for the Raspberry Pi 4 image build wrapper.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4-image-build.sh."""

import hashlib
import os
import pathlib
import plistlib
import shlex
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


def test_pi4_image_build_uses_square_logo_source() -> None:
    """The Pi 4 splash conversion must use the square Cohesix logo asset."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'COHESIX_LOGO_SOURCE="${ROOT_DIR}/docs/COHESIX_LOGO_SQ.png"' in source


def test_pi4_image_build_respects_cargo_target_dir_for_all_runtimes() -> None:
    """Every staged runtime must come from the target dir Cargo built."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'local target_dir="${CARGO_TARGET_DIR:-${ROOT_DIR}/target}"' in source
    assert 'root_task_elf="$(root_task_release_elf_path)"' in source
    assert (
        'runtime_artifact_dir="$(root_task_target_dir)/aarch64-unknown-none/release"'
        in source
    )
    assert (
        'package_driver_runtime_raw_cpio "$raw_cpio" "$runtime_artifact_dir"'
        in source
    )
    assert (
        'local root_task_elf="${ROOT_DIR}/target/aarch64-unknown-none/release/root-task"'
        not in source
    )


def test_pi4_image_build_defaults_to_pi4_release_features() -> None:
    """The image path must compile the same Pi 4 release feature bundle as tests."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'ROOT_TASK_FEATURES="release-pi4,bootstrap-trace"' in source
    assert "(default: release-pi4,bootstrap-trace)" in source


def test_pi4_image_build_honors_the_staged_runner_job_budget() -> None:
    """U-Boot rebuilds must not bypass test-plan CPU limits."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    resolver = source[
        source.index("resolve_build_jobs()") : source.index(
            "rebuild_u_boot_pi4()"
        )
    ]

    assert (
        'TP_HOST_JOBS:-${CARGO_BUILD_JOBS:-${CMAKE_BUILD_PARALLEL_LEVEL:-}}'
        in resolver
    )
    assert source.count('jobs="$(resolve_build_jobs)"') == 1
    assert source.count('jobs="$(sysctl -n hw.ncpu)"') == 1


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

    assert (
        'DEFAULT_REPO_SEL4_BUILD_DIR="${ROOT_DIR}/seL4/build_UBOOT"'
        in source
    )
    assert 'SEL4_BUILD_DIR="${DEFAULT_REPO_SEL4_BUILD_DIR}"' in source
    assert "(must resolve to seL4/build_UBOOT)" in source
    assert "alternate or out/ seL4 inputs are not supported" in source
    assert "out/sel4" not in source


def test_pi4_image_build_rejects_noncanonical_sel4_input(
    tmp_path: pathlib.Path,
) -> None:
    """The command-line override cannot restore a deleted out/ input lane."""

    script = _copy_sourceable_build_script(tmp_path)
    accepted = _source_function(
        script,
        (
            'SEL4_BUILD_DIR="$(realpath_py "$DEFAULT_REPO_SEL4_BUILD_DIR")"; '
            "validate_canonical_sel4_build_dir"
        ),
    )
    rejected = _source_function(
        script,
        'SEL4_BUILD_DIR="$(realpath_py "$ROOT_DIR/out/sel4")"; '
        "validate_canonical_sel4_build_dir",
    )

    assert accepted.returncode == 0, accepted.stderr
    assert rejected.returncode != 0
    assert "alternate or out/ seL4 inputs are not supported" in rejected.stderr


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
    assert 'COHESIX_SEL4_PROJECT_ROOT:PATH=' not in source
    assert '--repo-managed' in validator
    assert '--profile "$PI4_SEL4_PROFILE"' in validator
    assert '--require-source' not in validator
    assert '--require-artifacts' in validator
    assert '--for-runtime' in validator
    assert "cmake -S" not in validator
    assert "configure_pi4_sel4_build" not in source
    assert (
        'local canonical="${ROOT_DIR}/third_party/u-boot/tools/mkimage"'
        in source
    )
    assert "command -v mkimage" not in source
    assert "resolve_sel4_kernel_source_dir" not in source
    assert "--sel4-kernel-source-dir" not in source
    assert "SEL4_KERNEL_SOURCE_DIR" not in source

    domain_guard = source[
        source.index("verify_one_domain_schedule_cache_absent()") : source.index(
            "require_sel4_lib_available()"
        )
    ]
    assert "forbidden KernelDomainSchedule" in domain_guard
    assert "mktemp" not in domain_guard
    assert "mv " not in domain_guard

    skip_branch = source[source.index('if [[ "$SKIP_BUILD" -eq 0 ]]') :]
    assert skip_branch.index("validate_pi4_sel4_build") < skip_branch.index(
        "verify_skip_build_provenance"
    )


def test_pi4_image_build_uses_one_absolute_binutils_family(
    tmp_path: pathlib.Path,
) -> None:
    """Root-task and driver-runtime stripping must match the composer tool family."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    script = _copy_sourceable_build_script(tmp_path)
    prefix = tmp_path / "toolchain" / "aarch64-linux-gnu-"
    strip_tool = pathlib.Path(f"{prefix}strip")
    strip_tool.parent.mkdir()
    strip_tool.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    strip_tool.chmod(0o755)

    selected = _source_function(
        script,
        f"COHESIX_AARCH64_BINUTILS_PREFIX={str(prefix)!r}; find_aarch64_strip",
    )
    rejected = _source_function(
        script,
        "COHESIX_AARCH64_BINUTILS_PREFIX=relative-prefix-; find_aarch64_strip",
    )

    assert (
        "COHESIX_AARCH64_BINUTILS_PREFIX:-/opt/homebrew/bin/"
        "aarch64-linux-gnu-"
    ) in source
    assert source.count('strip_tool="$(find_aarch64_strip)"') == 2
    assert "command -v aarch64-elf-strip" not in source
    assert selected.returncode == 0, selected.stderr
    assert selected.stdout.strip() == str(strip_tool)
    assert rejected.returncode != 0
    assert "must be an absolute tool prefix" in rejected.stderr


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
    assert "disable_overscan=1" in config_template
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


def test_pi4_image_build_does_not_echo_wifi_password_to_serial() -> None:
    """Wi-Fi password entry must stay inside the USB local console."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]

    wifi_capture = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_capture_wifi_credentials ")
    )

    assert "askenv coh_wifi_psk_new" in wifi_capture
    assert "run coh_begin_wifi_secret_input" in wifi_capture
    assert "run coh_end_wifi_secret_input" in wifi_capture
    assert wifi_capture.index("run coh_begin_wifi_secret_input") < wifi_capture.index(
        "askenv coh_wifi_psk_new"
    )
    assert wifi_capture.index("askenv coh_wifi_psk_new") < wifi_capture.rindex(
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
    assert (
        "Privacy notice: Wi-Fi network name and password are visible on this display; "
        "they are hidden from serial output"
    ) in boot_template
    assert (
        "Wi-Fi password entry is unavailable over serial because U-Boot echoes typed input"
    ) in boot_template
    assert 'askenv coh_wifi_psk_new "Wi-Fi password (leave blank for an open network): "' in (
        boot_template
    )
    assert "boot.cmd does not suppress serial echo during Wi-Fi secret entry" in source
    assert (
        "boot.cmd does not collect replacement Wi-Fi passwords in the protected "
        "USB-only prompt"
    ) in source


def test_pi4_image_build_serial_wifi_missing_policy_uses_simple_prompt() -> None:
    """Serial-only Wi-Fi setup must use the proven non-secret staging prompt."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]
    wifi_capture = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_capture_wifi_credentials ")
    )

    assert "setenv coh_wifi_serial_recovery " not in boot_template
    assert "run coh_wifi_serial_recovery" not in wifi_capture
    assert "U-Boot policy missing:" not in boot_template
    assert "file-based policy recovery" not in boot_template
    assert "do not type PSK on serial" not in boot_template
    assert (
        "Wi-Fi password entry is unavailable over serial because U-Boot echoes typed input"
    ) in wifi_capture
    assert (
        "Connect a USB keyboard or create ${coh_policy_file} on the SD boot partition, "
        "then restart"
    ) in wifi_capture
    assert "Existing Wi-Fi settings were not changed" in wifi_capture
    assert (
        "No Wi-Fi network is configured and local USB input is unavailable"
    ) in wifi_capture
    assert "setenv coh_menu_page interface" in wifi_capture


def test_pi4_image_build_validates_exact_target_before_preserving_policy() -> None:
    """Normal reflash must bind policy to the exact child of the supplied disk."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    flash_start = source.index("flash_sd_card() {")
    flash_body = source[flash_start : source.index("\nvalidated_flash_target_identity() {")]

    assert 'part="$(canonical_flash_partition "$disk" "$DISK_LABEL"' in flash_body
    assert 'diskutil mount "$part" >/dev/null 2>&1 || true' in flash_body
    assert 'validated_flash_partition_mount "$disk" "$part" "$DISK_LABEL"' in flash_body
    assert 'diskutil mountDisk "$disk"' not in flash_body
    assert 'preflash_volume="/Volumes/${DISK_LABEL}"' not in flash_body
    assert 'cp -f "${preflash_volume}/${policy_file}" "$preserved_policy"' in flash_body
    assert 'diskutil list | awk' not in source
    assert '/Volumes/"${label}"\\ *' not in source


def test_pi4_image_build_retains_policy_after_interrupted_media_mutation() -> None:
    """A post-copy failure must not delete the only saved policy copy."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    cleanup_start = source.index("cleanup() {")
    cleanup_body = source[cleanup_start : source.index("\nsync_resolved_manifest_json() {")]
    flash_start = source.index("flash_sd_card() {")
    flash_body = source[flash_start : source.index("\nvalidated_flash_target_identity() {")]

    assert 'FLASH_MEDIA_MUTATION_STARTED=1' in flash_body
    assert '"${FLASH_MEDIA_MUTATION_STARTED:-0}" -eq 1' in cleanup_body
    assert "Retained saved policy after interrupted media update" in cleanup_body
    assert "Retry with --policy-recovery-file" in cleanup_body
    assert 'rm -f "$PRESERVED_POLICY_TEMP"' in cleanup_body


def test_pi4_image_build_policy_recovery_is_explicit_bounded_and_consumed() -> None:
    """Retry policy input must be explicit, bounded, and removed only on success."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    flash_start = source.index("flash_sd_card() {")
    flash_body = source[flash_start : source.index("\nvalidated_flash_target_identity() {")]
    unmount_index = flash_body.index('unmount_flashed_disk "$disk" "$volume"')
    consume_index = flash_body.index('rm -f "${POLICY_RECOVERY_CONSUMED_FILE}"')

    assert "--policy-recovery-file <path>" in source
    assert 'fail "--policy-recovery-file requires --flash-disk"' in source
    assert "--policy-recovery-file exceeds the 384-byte Cohesix policy bound" in flash_body
    assert "--policy-recovery-file refuses to replace an existing non-empty" in flash_body
    assert 'POLICY_RECOVERY_CONSUMED_FILE="${POLICY_RECOVERY_FILE}"' in flash_body
    assert consume_index > unmount_index


def test_pi4_image_build_cleanup_keeps_post_mutation_policy_copy(
    tmp_path: pathlib.Path,
) -> None:
    """The executable cleanup path must retain a private copy past mutation."""

    script = _copy_sourceable_build_script(tmp_path)
    policy = tmp_path / "cohesix-policy.test"
    policy.write_text("coh_net_mode=dhcp\n", encoding="utf-8")
    policy.chmod(0o600)

    result = _source_function(
        script,
        (
            f"PRESERVED_POLICY_TEMP={str(policy)!r}; "
            "POLICY_RECOVERY_CONSUMED_FILE=''; "
            "FLASH_MEDIA_MUTATION_STARTED=1; "
            "COMPOSITION_ROOT=''; RESTORE_CANONICAL_CODEGEN=0; "
            "EXACT_GIT_COMMIT=''; set +e; false; cleanup"
        ),
    )

    assert result.returncode == 1
    assert policy.is_file()
    assert policy.stat().st_mode & 0o777 == 0o600
    assert "Retained saved policy after interrupted media update" in result.stdout
    assert "Retry with --policy-recovery-file" in result.stdout


def test_pi4_image_build_refreshes_exact_child_without_erasing(
    tmp_path: pathlib.Path,
) -> None:
    """Normal reflashes may mount but never recreate partition or FAT topology."""

    fixture = _write_flash_command_fixture(tmp_path)
    stage = tmp_path / "stage"
    nested = stage / "overlays"
    nested.mkdir(parents=True)
    (stage / "config.txt").write_text("kernel=u-boot.bin\n", encoding="utf-8")
    (nested / "upstream.dtbo").write_bytes(b"overlay\n")
    policy = fixture["volume"] / "cohesix.env"
    policy.write_bytes(b"coh_net_mode=dhcp\n")
    stale = fixture["volume"] / "stale.txt"
    stale.write_text("remove me\n", encoding="utf-8")

    result = _source_function(
        fixture["script"],
        (
            f"PATH={str(fixture['bin'])!r}:$PATH; export PATH; "
            f"DISKUTIL_LOG={str(fixture['diskutil_log'])!r}; "
            f"CAFFEINATE_LOG={str(fixture['caffeinate_log'])!r}; "
            "export DISKUTIL_LOG CAFFEINATE_LOG; "
            f"STAGE_DIR={str(stage)!r}; DISK_LABEL=COHESIX; "
            "INITIALIZE_DISK=0; POLICY_RECOVERY_FILE=''; "
            "POLICY_RECOVERY_CONSUMED_FILE=''; PRESERVED_POLICY_TEMP=''; "
            "FLASH_MEDIA_MUTATION_STARTED=0; FLASH_CAFFEINATE_PID=''; "
            "trap stop_flash_caffeinate EXIT; flash_sd_card /dev/disk20"
        ),
    )

    assert result.returncode == 0, result.stderr
    commands = fixture["diskutil_log"].read_text(encoding="utf-8").splitlines()
    assert "mount /dev/disk20s1" in commands
    assert f"unmount {fixture['volume']}" in commands
    assert all("eraseDisk" not in command for command in commands)
    assert all("eraseVolume" not in command for command in commands)
    assert all("mountDisk" not in command for command in commands)
    assert all("unmountDisk" not in command for command in commands)
    assert all(command != "list" for command in commands)
    assert policy.read_bytes() == b"coh_net_mode=dhcp\n"
    assert not stale.exists()
    assert (fixture["volume"] / "config.txt").read_bytes() == (
        stage / "config.txt"
    ).read_bytes()
    assert (fixture["volume"] / "overlays" / "upstream.dtbo").read_bytes() == (
        nested / "upstream.dtbo"
    ).read_bytes()
    assert "Verified all 2 staged regular files" in result.stdout
    assert "-dimsu -t 3600 -w" in fixture["caffeinate_log"].read_text(
        encoding="utf-8"
    )


def test_pi4_image_build_initializes_whole_disk_only_with_explicit_opt_in(
    tmp_path: pathlib.Path,
) -> None:
    """Whole-disk partitioning must be reachable only through --initialize-disk."""

    fixture = _write_flash_command_fixture(tmp_path)
    stage = tmp_path / "stage"
    stage.mkdir()
    (stage / "config.txt").write_text("kernel=u-boot.bin\n", encoding="utf-8")

    parsed_default = _source_function(
        fixture["script"],
        "INITIALIZE_DISK=0; parse_args --flash-disk /dev/disk20; "
        'printf "%s\\n" "$INITIALIZE_DISK"',
    )
    parsed_explicit = _source_function(
        fixture["script"],
        "INITIALIZE_DISK=0; parse_args --flash-disk /dev/disk20 "
        '--initialize-disk; printf "%s\\n" "$INITIALIZE_DISK"',
    )
    assert parsed_default.stdout.strip() == "0"
    assert parsed_explicit.stdout.strip() == "1"

    result = _source_function(
        fixture["script"],
        (
            f"PATH={str(fixture['bin'])!r}:$PATH; export PATH; "
            f"DISKUTIL_LOG={str(fixture['diskutil_log'])!r}; "
            f"CAFFEINATE_LOG={str(fixture['caffeinate_log'])!r}; "
            "export DISKUTIL_LOG CAFFEINATE_LOG; "
            f"STAGE_DIR={str(stage)!r}; DISK_LABEL=COHESIX; "
            "INITIALIZE_DISK=1; POLICY_RECOVERY_FILE=''; "
            "POLICY_RECOVERY_CONSUMED_FILE=''; PRESERVED_POLICY_TEMP=''; "
            "FLASH_MEDIA_MUTATION_STARTED=0; FLASH_CAFFEINATE_PID=''; "
            "trap stop_flash_caffeinate EXIT; flash_sd_card /dev/disk20"
        ),
    )

    assert result.returncode == 0, result.stderr
    commands = fixture["diskutil_log"].read_text(encoding="utf-8").splitlines()
    assert commands.count(
        "eraseDisk FAT32 COHESIX MBRFormat /dev/disk20"
    ) == 1
    assert all("eraseVolume" not in command for command in commands)
    assert all("unmountDisk" not in command for command in commands)


def test_pi4_image_build_refuses_locked_console_before_media_update(
    tmp_path: pathlib.Path,
) -> None:
    """A loginwindow-locked Mac must fail before copy or initialization."""

    fixture = _write_flash_command_fixture(tmp_path, locked=True)
    stage = tmp_path / "stage"
    stage.mkdir()
    (stage / "config.txt").write_text("kernel=u-boot.bin\n", encoding="utf-8")
    policy = fixture["volume"] / "cohesix.env"
    policy.write_bytes(b"coh_net_mode=dhcp\n")

    result = _source_function(
        fixture["script"],
        (
            f"PATH={str(fixture['bin'])!r}:$PATH; export PATH; "
            f"DISKUTIL_LOG={str(fixture['diskutil_log'])!r}; "
            f"CAFFEINATE_LOG={str(fixture['caffeinate_log'])!r}; "
            "export DISKUTIL_LOG CAFFEINATE_LOG; "
            f"STAGE_DIR={str(stage)!r}; DISK_LABEL=COHESIX; "
            "INITIALIZE_DISK=0; POLICY_RECOVERY_FILE=''; "
            "POLICY_RECOVERY_CONSUMED_FILE=''; PRESERVED_POLICY_TEMP=''; "
            "FLASH_MEDIA_MUTATION_STARTED=0; FLASH_CAFFEINATE_PID=''; "
            "trap stop_flash_caffeinate EXIT; flash_sd_card /dev/disk20"
        ),
    )

    assert result.returncode != 0
    assert "console is locked" in result.stderr
    assert policy.read_bytes() == b"coh_net_mode=dhcp\n"
    commands = (
        fixture["diskutil_log"].read_text(encoding="utf-8").splitlines()
        if fixture["diskutil_log"].exists()
        else []
    )
    assert all("erase" not in command for command in commands)
    assert all("unmount" not in command for command in commands)
    assert not fixture["caffeinate_log"].exists()


def test_pi4_image_build_rechecks_lock_immediately_before_initialization(
    tmp_path: pathlib.Path,
) -> None:
    """A lock transition after preflight must still prevent whole-disk erase."""

    fixture = _write_flash_command_fixture(tmp_path, lock_after_check=2)
    stage = tmp_path / "stage"
    stage.mkdir()
    (stage / "config.txt").write_text("kernel=u-boot.bin\n", encoding="utf-8")

    result = _source_function(
        fixture["script"],
        (
            f"PATH={str(fixture['bin'])!r}:$PATH; export PATH; "
            f"DISKUTIL_LOG={str(fixture['diskutil_log'])!r}; "
            f"CAFFEINATE_LOG={str(fixture['caffeinate_log'])!r}; "
            "export DISKUTIL_LOG CAFFEINATE_LOG; "
            f"STAGE_DIR={str(stage)!r}; DISK_LABEL=COHESIX; "
            "INITIALIZE_DISK=1; POLICY_RECOVERY_FILE=''; "
            "POLICY_RECOVERY_CONSUMED_FILE=''; PRESERVED_POLICY_TEMP=''; "
            "FLASH_MEDIA_MUTATION_STARTED=0; FLASH_CAFFEINATE_PID=''; "
            "trap stop_flash_caffeinate EXIT; flash_sd_card /dev/disk20"
        ),
    )

    assert result.returncode != 0
    assert "console is locked" in result.stderr
    commands = fixture["diskutil_log"].read_text(encoding="utf-8").splitlines()
    assert all("erase" not in command for command in commands)
    assert all("unmount" not in command for command in commands)


def test_pi4_image_build_rechecks_lock_immediately_before_normal_copy(
    tmp_path: pathlib.Path,
) -> None:
    """A lock transition after mounting must still prevent in-place mutation."""

    fixture = _write_flash_command_fixture(tmp_path, lock_after_check=2)
    stage = tmp_path / "stage"
    stage.mkdir()
    (stage / "config.txt").write_text("kernel=u-boot.bin\n", encoding="utf-8")
    policy = fixture["volume"] / "cohesix.env"
    policy.write_bytes(b"coh_net_mode=dhcp\n")
    stale = fixture["volume"] / "stale.txt"
    stale.write_text("must remain\n", encoding="utf-8")

    result = _source_function(
        fixture["script"],
        (
            f"PATH={str(fixture['bin'])!r}:$PATH; export PATH; "
            f"DISKUTIL_LOG={str(fixture['diskutil_log'])!r}; "
            f"CAFFEINATE_LOG={str(fixture['caffeinate_log'])!r}; "
            "export DISKUTIL_LOG CAFFEINATE_LOG; "
            f"STAGE_DIR={str(stage)!r}; DISK_LABEL=COHESIX; "
            "INITIALIZE_DISK=0; POLICY_RECOVERY_FILE=''; "
            "POLICY_RECOVERY_CONSUMED_FILE=''; PRESERVED_POLICY_TEMP=''; "
            "FLASH_MEDIA_MUTATION_STARTED=0; FLASH_CAFFEINATE_PID=''; "
            "trap stop_flash_caffeinate EXIT; flash_sd_card /dev/disk20"
        ),
    )

    assert result.returncode != 0
    assert "console is locked" in result.stderr
    assert policy.read_bytes() == b"coh_net_mode=dhcp\n"
    assert stale.read_text(encoding="utf-8") == "must remain\n"
    assert not (fixture["volume"] / "config.txt").exists()
    commands = fixture["diskutil_log"].read_text(encoding="utf-8").splitlines()
    assert all("erase" not in command for command in commands)
    assert all("unmount" not in command for command in commands)


def test_pi4_image_build_rejects_oversize_existing_policy_before_copy(
    tmp_path: pathlib.Path,
) -> None:
    """The implicit on-card policy path must enforce the canonical bound."""

    fixture = _write_flash_command_fixture(tmp_path)
    stage = tmp_path / "stage"
    stage.mkdir()
    (stage / "config.txt").write_text("kernel=u-boot.bin\n", encoding="utf-8")
    policy = fixture["volume"] / "cohesix.env"
    policy.write_bytes(b"x" * 385)

    result = _source_function(
        fixture["script"],
        (
            f"PATH={str(fixture['bin'])!r}:$PATH; export PATH; "
            f"DISKUTIL_LOG={str(fixture['diskutil_log'])!r}; "
            f"CAFFEINATE_LOG={str(fixture['caffeinate_log'])!r}; "
            "export DISKUTIL_LOG CAFFEINATE_LOG; "
            f"STAGE_DIR={str(stage)!r}; DISK_LABEL=COHESIX; "
            "INITIALIZE_DISK=0; POLICY_RECOVERY_FILE=''; "
            "POLICY_RECOVERY_CONSUMED_FILE=''; PRESERVED_POLICY_TEMP=''; "
            "FLASH_MEDIA_MUTATION_STARTED=0; FLASH_CAFFEINATE_PID=''; "
            "trap stop_flash_caffeinate EXIT; flash_sd_card /dev/disk20"
        ),
    )

    assert result.returncode != 0
    assert "exceeds the 384-byte Cohesix policy bound" in result.stderr
    assert policy.read_bytes() == b"x" * 385
    commands = fixture["diskutil_log"].read_text(encoding="utf-8").splitlines()
    assert all("erase" not in command for command in commands)
    assert all("unmount" not in command for command in commands)


def test_pi4_image_build_never_follows_changed_flash_identity(
    tmp_path: pathlib.Path,
) -> None:
    """The critical-section recheck must reject even the same reused BSD node."""

    fixture = _write_flash_command_fixture(
        tmp_path, change_identity_after_first=True
    )
    stage = tmp_path / "stage"
    stage.mkdir()
    (stage / "config.txt").write_text("kernel=u-boot.bin\n", encoding="utf-8")

    result = _source_function(
        fixture["script"],
        (
            f"PATH={str(fixture['bin'])!r}:$PATH; export PATH; "
            f"DISKUTIL_LOG={str(fixture['diskutil_log'])!r}; "
            f"CAFFEINATE_LOG={str(fixture['caffeinate_log'])!r}; "
            "export DISKUTIL_LOG CAFFEINATE_LOG; "
            f"STAGE_DIR={str(stage)!r}; DISK_LABEL=COHESIX; "
            "INITIALIZE_DISK=0; POLICY_RECOVERY_FILE=''; "
            "POLICY_RECOVERY_CONSUMED_FILE=''; PRESERVED_POLICY_TEMP=''; "
            "FLASH_MEDIA_MUTATION_STARTED=0; FLASH_CAFFEINATE_PID=''; "
            "trap stop_flash_caffeinate EXIT; flash_sd_card /dev/disk20"
        ),
    )

    assert result.returncode != 0
    assert "identity changed before the critical section" in result.stderr
    commands = fixture["diskutil_log"].read_text(encoding="utf-8").splitlines()
    assert all("erase" not in command for command in commands)
    assert all("unmount" not in command for command in commands)


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
        "run coh_start_menu"
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
        generated_tail.index("run coh_start_menu")
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
    assert 'elif test "${coh_choice}" = "2"; then setenv coh_menu_page dhcp' in (
        root_menu
    )


def test_pi4_image_build_bounds_and_validates_saved_policy() -> None:
    """Saved policy must be bounded, allowlisted, CRLF-safe, and coherent."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]
    policy_load = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_load_saved_policy ")
    )
    policy_detect = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_detect_saved_config ")
    )

    assert "setenv coh_policy_max_size 0x180" in boot_template
    assert "fatsize mmc 0:1 ${coh_policy_file}" in policy_load
    assert policy_load.index("fatsize mmc") < policy_load.index("fatload mmc")
    assert "itest ${filesize} <= ${coh_policy_max_size}" in policy_load
    assert "env import -r -t ${coh_policy_addr} ${filesize}" in policy_load
    assert "env import -d" not in policy_load
    for field in (
        "coh_net_mode",
        "coh_net_interface",
        "coh_static_ip",
        "coh_static_prefix_len",
        "coh_static_gateway",
        "coh_wifi_ssid",
        "coh_wifi_psk",
        "coh_show_logo",
    ):
        assert field in policy_load
    assert "coh_policy_load_state oversized" in policy_load
    assert "coh_policy_load_state invalid" in policy_load
    assert "coh_policy_load_state empty" in policy_load
    assert "invalid coh_show_logo value" in policy_load
    assert 'test "${coh_show_logo}" = "0"' in policy_load
    assert 'test "${coh_show_logo}" = "1"' in policy_load
    for reason in (
        "net-mode-invalid",
        "net-interface-invalid",
        "static-ip-missing",
        "static-prefix-missing",
        "wifi-ssid-missing",
        "wifi-psk-too-short",
    ):
        assert reason in policy_detect
    assert "using default settings" in policy_detect
    assert "run coh_reset_policy" in policy_detect


def test_pi4_image_build_redacts_untrusted_wifi_ssid_from_output() -> None:
    """Imported SSIDs must not become forged serial or terminal evidence."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]
    summary = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_emit_policy_summary ")
    )

    assert 'echo "[cohesix] Wi-Fi network: Configured (name hidden)"' in summary
    assert "wifi-ssid=${coh_wifi_ssid}" not in boot_template


def test_pi4_image_build_verifies_policy_before_reboot() -> None:
    """A failed export, write, or readback must never trigger reset or success."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]
    persist = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_persist_policy ")
    )
    confirm = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_confirm_prompt ")
    )
    root = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_prompt_root ")
    )
    reset = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_confirm_reset ")
    )

    assert "setenv coh_policy_persisted 0" in persist
    assert "env export -t -s ${coh_policy_max_size}" in persist
    assert "fatwrite mmc 0:1" in persist
    assert "fatsize mmc 0:1" in persist
    assert "fatload mmc 0:1 ${coh_policy_verify_addr}" in persist
    assert "setenv stdout nulldev; setenv stderr nulldev; if cmp.b" in persist
    assert "setenv coh_policy_persisted 1" in persist
    assert "not restarting" in persist
    assert "reset" not in persist
    assert (
        'if test "${coh_policy_persisted}" = "1"; then echo '
        '"[cohesix] Saved settings verified; restarting"; reset'
    ) in confirm
    assert "Save failed; review settings and retry" in confirm
    assert "setenv coh_menu_page reset" in root
    assert "run coh_clear_saved_policy" not in root
    assert "Reset saved settings?" in reset
    assert "Confirm reset" in reset
    assert "run coh_read_cancel_choice" in reset
    assert 'askenv coh_choice "Select option [0]: "' in boot_template
    assert "Could not reset saved settings; reloading settings from SD" in reset
    assert "run coh_load_saved_policy" in reset
    assert "Saved settings reset to defaults" in reset


def test_pi4_image_build_wifi_credentials_are_replaceable_and_atomic() -> None:
    """Existing Wi-Fi credentials must support keep, replace, and safe retry."""

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
    wifi_capture = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_capture_wifi_credentials ")
    )

    assert "Keep current Wi-Fi settings" in wifi_setup
    assert "Change Wi-Fi network" in wifi_setup
    assert "No Wi-Fi network is configured" in wifi_setup
    assert "Enter Wi-Fi network" in wifi_setup
    assert 'echo "  0. Back"' in wifi_setup
    assert "run coh_capture_wifi_credentials" in wifi_setup
    assert "askenv coh_wifi_ssid_new" in wifi_capture
    assert "askenv coh_wifi_psk_new" in wifi_capture
    assert wifi_capture.index('setenv coh_wifi_ssid "${coh_wifi_ssid_new}"') > (
        wifi_capture.index("run coh_end_wifi_secret_input")
    )
    assert "existing settings were not changed" in wifi_capture
    assert "setenv coh_wifi_ssid_new; setenv coh_wifi_psk_new" in wifi_capture


def test_pi4_image_build_menu_navigation_is_bounded_and_discardable() -> None:
    """Menu pages must dispatch iteratively and reload policy on discard."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]
    menu_loop = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_menu_loop ")
    )
    dhcp = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_prompt_dhcp ")
    )
    confirm = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_confirm_prompt ")
    )

    assert 'while test "${coh_menu_running}" = "1"' in menu_loop
    for page in ("root", "dhcp", "interface", "wifi", "static", "confirm", "reset"):
        assert f'"${{coh_menu_page}}" = "{page}"' in menu_loop
    assert "run coh_load_saved_policy; setenv coh_menu_page root" in dhcp
    assert "run coh_load_saved_policy; setenv coh_menu_page root" in confirm
    for line in boot_template.splitlines():
        if line.startswith(
            (
                "setenv coh_prompt_root ",
                "setenv coh_prompt_dhcp ",
                "setenv coh_prompt_interface ",
                "setenv coh_wifi_setup ",
                "setenv coh_static_setup ",
                "setenv coh_confirm_prompt ",
                "setenv coh_confirm_reset ",
            )
        ):
            assert "run coh_prompt_" not in line


def test_pi4_image_build_menu_uses_consistent_operator_language() -> None:
    """Menu labels must use familiar terms and one navigation convention."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]
    commands = {
        line.split(" ", 2)[1]: line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_")
    }

    root = commands["coh_prompt_root"]
    assert "Cohesix boot menu" in root
    assert "Boot with saved settings" in root
    assert "Boot with default settings" in root
    assert "Change network settings" in root
    assert "Boot logo: On (select to turn off)" in root
    assert "Boot logo: Off (select to turn on)" in root
    assert "Reset saved settings to defaults" in root
    assert "Save settings and restart" in root
    assert 'echo "  9. Advanced: Open U-Boot shell"' in root

    for command in (
        "coh_prompt_dhcp",
        "coh_prompt_interface",
        "coh_wifi_setup",
        "coh_static_setup",
        "coh_confirm_prompt",
        "coh_confirm_reset",
    ):
        assert 'echo "  0.' in commands[command]
        assert 'echo "  9. Advanced: Open U-Boot shell"' in commands[command]
        assert 'test "${coh_choice}" = "9"' in commands[command]

    assert 'test "${coh_choice}" = "  9"' in commands["coh_normalize_choice"]
    assert "Automatic (DHCP)" in commands["coh_prompt_dhcp"]
    assert "Manual (static IPv4)" in commands["coh_prompt_dhcp"]
    assert "Ethernet (wired)" in commands["coh_prompt_interface"]
    assert "Wi-Fi (wireless)" in commands["coh_prompt_interface"]
    assert "Boot once without saving" in commands["coh_confirm_prompt"]

    for obsolete in (
        "DHCP ON",
        "DHCP OFF",
        "Wired Ethernet (GENET)",
        "Wi-Fi (CYW43455)",
        "Continue with existing config",
        "Boot with manifest defaults",
        "Exit to U-Boot prompt",
    ):
        assert obsolete not in boot_template


def test_pi4_image_build_static_entry_has_validation_and_back_navigation() -> None:
    """Static entry must reject malformed bounds without recursive fallthrough."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    boot_template = source[
        source.index('echo "[cohesix] pi4 autoboot script"') : source.index(
            '\nEOF\n    sed -i \'\' "s/__COH_IMAGE__/'
        )
    ]
    static_setup = next(
        line
        for line in boot_template.splitlines()
        if line.startswith("setenv coh_static_setup ")
    )

    assert "Enter manual IPv4 settings" in static_setup
    assert 'echo "  0. Back"' in static_setup
    assert 'test "${coh_static_ip}" =~ "${coh_ipv4_text_regex}"' in static_setup
    assert (
        'test "${coh_static_prefix_len}" =~ "${coh_prefix_text_regex}"'
        in static_setup
    )
    assert "itest ${coh_static_prefix_len} < 1" in static_setup
    assert "itest ${coh_static_prefix_len} > 32" in static_setup
    assert "setenv coh_menu_page interface" in static_setup
    assert "run coh_static_setup" not in static_setup


def test_pi4_uboot_defconfigs_pin_every_menu_dependency() -> None:
    """The Pi 4 U-Boot config must not rely on implicit menu command defaults."""

    required = (
        "CONFIG_HUSH_PARSER=y",
        "CONFIG_CMD_EXPORTENV=y",
        "CONFIG_CMD_IMPORTENV=y",
        "CONFIG_CMD_ITEST=y",
        "CONFIG_CMD_MEMORY=y",
        "CONFIG_CMD_SOURCE=y",
        "CONFIG_CMD_SETEXPR=y",
        "CONFIG_CMD_FAT=y",
        "CONFIG_FAT_WRITE=y",
        "CONFIG_REGEX=y",
        "CONFIG_SYS_DEVICE_NULLDEV=y",
    )
    for path in (U_BOOT_DEFCONFIG_PATH, U_BOOT_GENERATED_DEFCONFIG_PATH):
        config = path.read_text(encoding="utf-8")
        for setting in required:
            assert setting in config


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

    assert 'EXACT_ROOT_CPIO="${PI4_ASSEMBLY_DIR}/archive.archive.o.cpio"' in source
    assert 'local expected_root_cpio="$EXACT_ROOT_CPIO"' in source
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


def test_pi4_image_composition_never_writes_canonical_profile_tree() -> None:
    """Rootserver composition must publish only into the output assembly."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    compose_start = source.index("compose_pi4_assembly() {")
    compose_end = source.index("\nbuild_pi4_image() {", compose_start)
    compose_body = source[compose_start:compose_end]
    validator_start = source.index("validate_pi4_sel4_build() {")
    validator_end = source.index("\nresolve_mkimage() {", validator_start)
    validator = source[validator_start:validator_end]

    assert "cmake --build" not in source
    assert '--sel4-build-dir "$SEL4_BUILD_DIR"' in compose_body
    assert '--rootserver "$STRIPPED_ROOT_TASK_ELF"' in compose_body
    assert '--output-dir "$PI4_ASSEMBLY_DIR"' in compose_body
    assert '--timestamp "$composition_epoch"' in compose_body
    assert 'cp -f "${SEL4_BUILD_DIR}/CMakeCache.txt"' in compose_body
    assert 'cp -f "$STRIPPED_ROOT_TASK_ELF" "${SEL4_BUILD_DIR}' not in source
    assert "generate_pi4_elfloader_platform_info" not in source
    assert "cmake --build" not in validator
    assert "mkdir -p" not in validator
    assert 'verify_pi4_elfloader_platform_info' in validator


def test_pi4_image_composition_revalidates_unchanged_canonical_input() -> None:
    """The selected stamped tree is fingerprinted across artifact composition."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    build_start = source.index("build_pi4_image() {")
    build_end = source.index("\nstage_uboot_logo() {", build_start)
    build_body = source[build_start:build_end]

    capture = build_body.index("capture_canonical_sel4_state")
    compose = build_body.index("compose_pi4_assembly")
    post_compose = build_body.index(
        'verify_canonical_sel4_state "after rootserver composition"'
    )
    revalidate = build_body.rindex("validate_pi4_sel4_build")
    post_validate = build_body.index(
        'verify_canonical_sel4_state "after post-composition validation"'
    )
    provenance = build_body.index("write_sel4_image_provenance")

    assert capture < compose < post_compose < revalidate < post_validate < provenance


def test_pi4_stage_dir_cannot_alias_out_or_derived_assembly(
    tmp_path: pathlib.Path,
) -> None:
    """Staging cleanup cannot erase the durable derived exact-image inputs."""

    script = _copy_sourceable_build_script(tmp_path)
    for stage in ("$ROOT_DIR/out", "$ROOT_DIR/out/pi4-image-assembly"):
        result = _source_function(
            script,
            f'COHESIX_IMAGE_NAME=image.bin; STAGE_DIR="{stage}"; '
            'PI4_ASSEMBLY_DIR="$ROOT_DIR/out/pi4-image-assembly"; '
            "validate_output_paths",
        )

        assert result.returncode != 0
        assert "--stage-dir" in result.stderr


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


def _write_flash_command_fixture(
    tmp_path: pathlib.Path,
    *,
    locked: bool = False,
    change_identity_after_first: bool = False,
    lock_after_check: int | None = None,
) -> dict[str, pathlib.Path]:
    """Create deterministic diskutil/ioreg/caffeinate commands for flash tests."""

    script = _copy_sourceable_build_script(tmp_path)
    bin_dir = tmp_path / "fake-bin"
    bin_dir.mkdir()
    volume = tmp_path / "COHESIX"
    volume.mkdir()
    diskutil_log = tmp_path / "diskutil.log"
    caffeinate_log = tmp_path / "caffeinate.log"
    info_count = tmp_path / "whole-info-count"
    lock_count = tmp_path / "lock-count"

    whole_info = {
        "DeviceIdentifier": "disk20",
        "DeviceNode": "/dev/disk20",
        "WholeDisk": True,
        "ParentWholeDisk": "disk20",
        "Content": "FDisk_partition_scheme",
        "Writable": True,
        "WritableMedia": True,
        "RemovableMediaOrExternalDevice": True,
        "Removable": True,
        "Ejectable": True,
        "VirtualOrPhysical": "Physical",
        "SystemImage": False,
        "OSInternalMedia": False,
        "TotalSize": 63_864_569_856,
        "IOKitSize": 63_864_569_856,
        "DeviceBlockSize": 512,
        "MediaName": "SD Card Reader",
        "BusProtocol": "Secure Digital",
        "DeviceTreePath": "IODeviceTree:/arm-io/sdxc",
        "IORegistryEntryName": "SD Card Reader Media",
    }
    changed_info = dict(whole_info)
    changed_info["TotalSize"] += 512
    partition_info = {
        "DeviceIdentifier": "disk20s1",
        "DeviceNode": "/dev/disk20s1",
        "WholeDisk": False,
        "ParentWholeDisk": "disk20",
        "Content": "DOS_FAT_32",
        "VolumeName": "COHESIX",
        "Writable": True,
        "WritableMedia": True,
        "MountPoint": str(volume),
    }
    listing = {
        "AllDisksAndPartitions": [
            {
                "DeviceIdentifier": "disk20",
                "Content": "FDisk_partition_scheme",
                "Partitions": [
                    {
                        "DeviceIdentifier": "disk20s1",
                        "Content": "DOS_FAT_32",
                        "VolumeName": "COHESIX",
                    }
                ],
            }
        ]
    }
    console = {
        "IOConsoleLocked": locked,
        "IOConsoleUsers": [
            {
                "kCGSSessionOnConsoleKey": True,
                "kCGSessionLoginDoneKey": True,
                "kCGSSessionUserIDKey": os.getuid(),
                **(
                    {"CGSSessionScreenIsLocked": True}
                    if locked
                    else {}
                ),
            }
        ],
    }
    locked_console = {
        "IOConsoleLocked": True,
        "IOConsoleUsers": [
            {
                "kCGSSessionOnConsoleKey": True,
                "kCGSessionLoginDoneKey": True,
                "kCGSSessionUserIDKey": os.getuid(),
                "CGSSessionScreenIsLocked": True,
            }
        ],
    }

    payloads = {
        "whole.plist": plistlib.dumps(whole_info),
        "whole-changed.plist": plistlib.dumps(changed_info),
        "partition.plist": plistlib.dumps(partition_info),
        "list.plist": plistlib.dumps(listing),
        "console.plist": plistlib.dumps(console),
        "console-locked.plist": plistlib.dumps(locked_console),
    }
    for filename, payload in payloads.items():
        (tmp_path / filename).write_bytes(payload)

    diskutil = bin_dir / "diskutil"
    diskutil.write_text(
        f"""#!/usr/bin/env bash
set -euo pipefail
printf '%s\\n' "$*" >> "$DISKUTIL_LOG"
case "$1" in
  info)
    [[ "$2" == "-plist" ]]
    if [[ "$3" == "/dev/disk20" ]]; then
      count=0
      [[ ! -f {shlex.quote(str(info_count))} ]] || count="$(<{shlex.quote(str(info_count))})"
      count=$((count + 1))
      printf '%s\\n' "$count" > {shlex.quote(str(info_count))}
      if [[ {int(change_identity_after_first)} -eq 1 && "$count" -gt 1 ]]; then
        /bin/cat {shlex.quote(str(tmp_path / "whole-changed.plist"))}
      else
        /bin/cat {shlex.quote(str(tmp_path / "whole.plist"))}
      fi
    elif [[ "$3" == "/dev/disk20s1" ]]; then
      /bin/cat {shlex.quote(str(tmp_path / "partition.plist"))}
    else
      exit 2
    fi
    ;;
  list)
    [[ "$2" == "-plist" && "$3" == "/dev/disk20" ]]
    /bin/cat {shlex.quote(str(tmp_path / "list.plist"))}
    ;;
  mount)
    [[ "$2" == "/dev/disk20s1" ]]
    ;;
  eraseDisk)
    [[ "$2" == "FAT32" && "$3" == "COHESIX" && "$4" == "MBRFormat" && "$5" == "/dev/disk20" ]]
    ;;
  unmount)
    [[ "$2" == {shlex.quote(str(volume))} || "$3" == {shlex.quote(str(volume))} ]]
    ;;
  *)
    exit 2
    ;;
esac
""",
        encoding="utf-8",
    )
    diskutil.chmod(0o755)

    ioreg = bin_dir / "ioreg"
    ioreg.write_text(
        f"""#!/usr/bin/env bash
set -euo pipefail
count=0
[[ ! -f {shlex.quote(str(lock_count))} ]] || count="$(<{shlex.quote(str(lock_count))})"
count=$((count + 1))
printf '%s\\n' "$count" > {shlex.quote(str(lock_count))}
if [[ {lock_after_check if lock_after_check is not None else 0} -gt 0 \
      && "$count" -gt {lock_after_check if lock_after_check is not None else 0} ]]; then
  /bin/cat {shlex.quote(str(tmp_path / "console-locked.plist"))}
else
  /bin/cat {shlex.quote(str(tmp_path / "console.plist"))}
fi
""",
        encoding="utf-8",
    )
    ioreg.chmod(0o755)

    caffeinate = bin_dir / "caffeinate"
    caffeinate.write_text(
        """#!/usr/bin/env bash
set -eu
printf '%s\n' "$*" >> "$CAFFEINATE_LOG"
trap 'exit 0' TERM INT
while :; do
  /bin/sleep 1
done
""",
        encoding="utf-8",
    )
    caffeinate.chmod(0o755)

    return {
        "script": script,
        "bin": bin_dir,
        "volume": volume,
        "diskutil_log": diskutil_log,
        "caffeinate_log": caffeinate_log,
    }


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


def test_sel4_tree_state_digest_binds_bytes_modes_and_symlinks(
    tmp_path: pathlib.Path,
) -> None:
    """Canonical-profile preservation detects content, mode, and link drift."""

    script = _copy_sourceable_build_script(tmp_path)
    tree = tmp_path / "sel4-tree"
    tree.mkdir()
    artifact = tree / "artifact"
    artifact.write_bytes(b"first\n")
    artifact.chmod(0o644)
    link = tree / "selected"
    link.symlink_to("artifact")
    command = 'sel4_tree_state_digest "$ROOT_DIR/sel4-tree"'
    initial = _source_function(script, command)
    assert initial.returncode == 0, initial.stderr

    artifact.write_bytes(b"other\n")
    content = _source_function(script, command)
    artifact.chmod(0o755)
    mode = _source_function(script, command)
    link.unlink()
    link.symlink_to("missing")
    target = _source_function(script, command)

    observed = {
        initial.stdout.strip(),
        content.stdout.strip(),
        mode.stdout.strip(),
        target.stdout.strip(),
    }
    assert len(observed) == 4


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
        "canonical_profile_stamp_sha256",
        "canonical_profile_state_sha256",
        "composition_record_sha256",
        "composition_cmake_cache_sha256",
        "composition_timer_header_sha256",
        "wrapper_sha256",
        "rootserver_sha256",
        "rootserver_cpio_sha256",
    ):
        assert field in source


@pytest.mark.parametrize(
    "tampered_relative_path",
    [
        "assembly/sel4test-driver-image-arm-bcm2711",
        "assembly/rootserver",
        "assembly/archive.archive.o.cpio",
        "manifest.toml",
        "sel4-build/cohesix-profile-build-inputs.json",
        "assembly/composition-profile-build-inputs.json",
        "assembly/composition-CMakeCache.txt",
        "assembly/composition-platform_gen.h",
    ],
)
def test_skip_build_provenance_rejects_each_bound_artifact_tamper(
    tmp_path: pathlib.Path,
    tampered_relative_path: str,
) -> None:
    """Every source, profile, root, archive, and wrapper digest is enforced."""

    script = _copy_sourceable_build_script(tmp_path)
    artifacts = {
        "assembly/sel4test-driver-image-arm-bcm2711": b"wrapper\n",
        "assembly/rootserver": b"rootserver\n",
        "assembly/archive.archive.o.cpio": b"cpio\n",
        "manifest.toml": b"manifest\n",
        "sel4-build/cohesix-profile-build-inputs.json": b"canonical stamp\n",
        "assembly/composition-profile-build-inputs.json": b"composition stamp\n",
        "assembly/composition-CMakeCache.txt": b"cache\n",
        "assembly/composition-platform_gen.h": b"timer\n",
    }
    for relative_path, payload in artifacts.items():
        path = tmp_path / relative_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(payload)

    shell_state = (
        'SEL4_BUILD_DIR="$ROOT_DIR/sel4-build"; '
        'EXACT_PI4_IMAGE="$ROOT_DIR/assembly/sel4test-driver-image-arm-bcm2711"; '
        'EXACT_ROOT_ELF="$ROOT_DIR/assembly/rootserver"; '
        'EXACT_ROOT_CPIO="$ROOT_DIR/assembly/archive.archive.o.cpio"; '
        'EXACT_CANONICAL_PROFILE_STAMP="$ROOT_DIR/sel4-build/cohesix-profile-build-inputs.json"; '
        'EXACT_COMPOSITION_RECORD="$ROOT_DIR/assembly/composition-profile-build-inputs.json"; '
        'EXACT_COMPOSITION_CACHE="$ROOT_DIR/assembly/composition-CMakeCache.txt"; '
        'EXACT_COMPOSITION_TIMER_HEADER="$ROOT_DIR/assembly/composition-platform_gen.h"; '
        f'CANONICAL_SEL4_STATE_DIGEST={"b" * 64!r}; '
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
