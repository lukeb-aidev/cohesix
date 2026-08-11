# Author: Lukas Bower
# Purpose: Guard host bootpd supervisor recovery after disposable output cleanup.
# Copyright 2026 Lukas Bower

"""Tests for the Pi 4 direct-link bootpd supervisor."""

import pathlib
import plistlib


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
HOST_BOOTPD_DIR = REPO_ROOT / "tools" / "host-bootpd"
SCRIPT_PATH = HOST_BOOTPD_DIR / "start-en8-bootpd.zsh"
INSTALLER_PATH = HOST_BOOTPD_DIR / "install-root-bootpd.zsh"
LAUNCHD_PLIST_PATH = (
    HOST_BOOTPD_DIR / "com.lukasbower.cohesix.en8-bootpd.plist"
)
EXTERNAL_RUNTIME_DIR = "/Users/lukasbower/cohesix/host-bootpd"


def test_supervisor_recreates_runtime_dir_before_each_service_turn() -> None:
    """Cleaning out must not strand a live supervisor without bootpd."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    ensure_runtime_dir = source[
        source.index("ensure_runtime_dir() {") : source.index("\nlog() {")
    ]
    loop = source[source.index("while true; do") :]
    loop_lines = loop.splitlines()

    assert ensure_runtime_dir.index("validate_runtime_dir") < (
        ensure_runtime_dir.index('mkdir -p "${runtime_dir}"')
    )
    assert 'print "$$" > "${pid_file}"' in ensure_runtime_dir
    assert loop_lines[1].strip() == "ensure_runtime_dir"
    assert loop.index("ensure_runtime_dir") < loop.index(
        '/usr/libexec/bootpd'
    )


def test_supervisor_requires_the_launchdaemon_runtime_binding() -> None:
    """The root supervisor must never infer a writable repo runtime path."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert f'readonly expected_runtime_dir="{EXTERNAL_RUNTIME_DIR}"' in source
    assert 'readonly runtime_dir="${COHESIX_BOOTPD_RUNTIME_DIR-}"' in source
    assert 'if [[ -z "${runtime_dir}" ]]' in source
    assert 'if [[ "${runtime_dir}" != "${expected_runtime_dir}" ]]' in source
    assert 'readonly config="${repo_root}/tools/host-bootpd/bootpd.plist"' in source
    assert 'readonly runtime_dir="${repo_root}/out/host-bootpd"' not in source


def test_supervisor_fails_closed_on_legacy_or_dual_runtime_state() -> None:
    """A stale repo runtime must not be mistaken for current evidence."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    validation = source[
        source.index("validate_runtime_dir() {") : source.index(
            "\nensure_runtime_dir() {"
        )
    ]

    dual_check = (
        'if path_present "${legacy_runtime_dir}" && '
        'path_present "${runtime_dir}"; then'
    )
    assert dual_check in validation
    assert "ambiguous runtime state" in validation
    assert 'if path_present "${legacy_runtime_dir}"; then' in validation
    assert "rerun install-root-bootpd.zsh" in validation
    assert validation.index(dual_check) < validation.index(
        'if path_present "${legacy_runtime_dir}"; then'
    )


def test_installer_migrates_one_legacy_runtime_and_rejects_two() -> None:
    """Installation must move one legacy tree and never merge two trees."""

    source = INSTALLER_PATH.read_text(encoding="utf-8")
    validation = source[
        source.index("validate_runtime_state() {") : source.index(
            "\nprepare_runtime_dir() {"
        )
    ]
    prepare = source[
        source.index("prepare_runtime_dir() {") : source.index("\nif (( EUID")
    ]
    main = source[source.index('owner_uid="$(id -u') :]

    assert f'readonly runtime_dir="{EXTERNAL_RUNTIME_DIR}"' in source
    assert "ambiguous runtime state" in validation
    assert 'mv "${legacy_runtime_dir}" "${runtime_dir}"' in prepare
    assert main.index('launchctl bootout system "${daemon_plist}"') < main.index(
        "validate_runtime_state"
    )
    assert main.index('pkill -f -x "${bootpd_command}"') < main.index(
        "wait_for_shutdown"
    )
    assert main.index("wait_for_shutdown") < main.index(
        "validate_runtime_state"
    )
    assert main.index("wait_for_shutdown") < main.index("prepare_runtime_dir")
    shutdown = source[
        source.index("wait_for_shutdown() {") : source.index(
            "\nvalidate_runtime_state() {"
        )
    ]
    assert "for attempt in {1..50}; do" in shutdown
    assert "if ! supervisor_running && ! bootpd_running; then" in shutdown
    assert "could not stop the exact bootpd supervisor and child" in shutdown
    assert 'mkdir -p "${repo_root}/out/host-bootpd"' not in source
    assert 'cat > "${daemon_plist}"' not in source
    assert (
        '/usr/bin/install -o root -g wheel -m 644 '
        '"${agent_plist}" "${daemon_plist}"'
    ) in source


def test_launchdaemon_binds_external_runtime_and_external_logs() -> None:
    """The checked-in launchd template must match the installed path contract."""

    with LAUNCHD_PLIST_PATH.open("rb") as plist_file:
        launchd = plistlib.load(plist_file)

    assert launchd["EnvironmentVariables"] == {
        "COHESIX_BOOTPD_RUNTIME_DIR": EXTERNAL_RUNTIME_DIR
    }
    assert launchd["StandardOutPath"] == (
        f"{EXTERNAL_RUNTIME_DIR}/root-launchd.out.log"
    )
    assert launchd["StandardErrorPath"] == (
        f"{EXTERNAL_RUNTIME_DIR}/root-launchd.err.log"
    )
    assert launchd["ProgramArguments"] == [str(SCRIPT_PATH)]
    assert "/out/host-bootpd/" not in str(launchd)
