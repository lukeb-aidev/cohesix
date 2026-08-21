# Author: Lukas Bower
# Purpose: Guard host installers, repository Python setup, and quickstart docs.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import pathlib
import re
import subprocess


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
MACOS_SETUP = REPO_ROOT / "toolchain" / "setup_macos_arm64.sh"
LINUX_SETUP = REPO_ROOT / "toolchain" / "setup_linux_arm64.sh"
VENV_SETUP = REPO_ROOT / "toolchain" / "setup_repo_venv.sh"
RELEASE_SETUP = REPO_ROOT / "scripts" / "setup_environment.sh"


def _read(path: pathlib.Path) -> str:
    return path.read_text(encoding="utf-8")


def _run_help(path: pathlib.Path) -> str:
    completed = subprocess.run(
        ["bash", str(path), "--help"],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout


def test_setup_scripts_are_valid_and_help_skips_preflight() -> None:
    scripts = (MACOS_SETUP, LINUX_SETUP, VENV_SETUP, RELEASE_SETUP)
    subprocess.run(
        ["bash", "-n", *(str(path) for path in scripts)],
        cwd=REPO_ROOT,
        check=True,
    )

    assert "macOS 26 or later on Apple Silicon" in _run_help(MACOS_SETUP)
    assert "Ubuntu 22.04, 24.04, or 26.04 ARM64" in _run_help(LINUX_SETUP)
    assert "repository-local .venv" in _run_help(VENV_SETUP)
    assert "runtime dependencies for a Cohesix release bundle" in _run_help(
        RELEASE_SETUP
    )


def test_linux_setup_has_strict_host_and_package_contract() -> None:
    setup = _read(LINUX_SETUP)
    package_block = re.search(r"PACKAGES=\((.*?)\n\)", setup, re.DOTALL)
    assert package_block is not None
    packages = package_block.group(1)

    assert '[[ "$(uname -s)" == "Linux" ]]' in setup
    assert "aarch64|arm64" in setup
    assert "22.04|24.04|26.04" in setup
    assert "qemu-system-arm" in packages
    assert "qemu-system-aarch64" not in packages
    assert "libnvidia-ml-dev" not in packages
    for package in (
        "libfuse3-dev",
        "libssl-dev",
        "libwebkit2gtk-4.1-dev",
        "libjavascriptcoregtk-4.1-dev",
        "libgtk-3-dev",
        "protobuf-compiler",
        "ripgrep",
    ):
        assert package in packages
    assert "python3.11-venv" in setup
    assert "${SETUP_REPO_VENV}" in setup
    assert "required TCG accelerator" in setup


def test_macos_setup_enforces_primary_host_contract() -> None:
    setup = _read(MACOS_SETUP)

    assert '[[ "$(uname -s)" == "Darwin" ]]' in setup
    assert '[[ "$(uname -m)" == "arm64" ]]' in setup
    assert "MACOS_MAJOR >= 26" in setup
    assert "xcode-select -p" in setup
    assert "xcrun --find clang" in setup
    assert "ripgrep" in setup
    assert "${SETUP_REPO_VENV}" in setup
    assert "required HVF accelerator" in setup


def test_repository_venv_is_locked_bounded_and_shared() -> None:
    setup = _read(VENV_SETUP)
    macos = _read(MACOS_SETUP)
    linux = _read(LINUX_SETUP)

    assert "sys.version_info < (3, 11)" in setup
    assert "--require-hashes" in setup
    assert "--only-binary=:all:" in setup
    assert "--no-build-isolation" in setup
    assert "--no-deps" in setup
    assert '"${REPO_ROOT}/tools/cohesix-py"' in setup
    assert '[[ -L "${VENV_DIR}" ]]' in setup
    assert "integrations" not in setup
    assert "[ml]" not in setup
    assert "setup_repo_venv.sh" in macos
    assert "setup_repo_venv.sh" in linux


def test_release_setup_is_fail_closed_and_uses_runtime_package_names() -> None:
    setup = _read(RELEASE_SETUP)

    assert "22.04|24.04|26.04" in setup
    assert "COHESIX_ALLOW_UNSUPPORTED_UBUNTU" not in setup
    assert 'missing+=("qemu-system-arm")' in setup
    assert 'missing+=("qemu-system-aarch64")' not in setup
    assert 'gtk_runtime="libgtk-3-0"' in setup
    assert 'gtk_runtime="libgtk-3-0t64"' in setup
    assert '"libfuse3-3"' in setup
    assert '"libxdo3"' in setup
    assert "enable_ubuntu_universe" in setup
    assert "the Linux release bundle requires ARM64" in setup
    assert "the macOS release bundle requires Apple Silicon" in setup
    assert "macOS 26 or later is required" in setup
    assert "require_qemu_accel hvf" in setup
    assert "require_qemu_accel tcg" in setup
    assert "--check" in setup
    assert 'wheels=("${BUNDLE_ROOT}"/python/dist/*.whl)' in setup
    assert 'local venv_dir="${BUNDLE_ROOT}/.venv"' in setup
    assert "--no-deps" in setup
    assert "--force-reinstall" in setup
    assert "refusing symlinked release Python environment" in setup
    assert 'runtime_pkgs+=("python3.11" "python3.11-venv")' in setup
    assert 'runtime_pkgs+=("python3" "python3-venv")' in setup

    inventory = _read(REPO_ROOT / "configs" / "implementation_surfaces.toml")
    release_bundle = _read(REPO_ROOT / "scripts" / "release_bundle.sh")
    assert '"scripts/setup_environment.sh"' in inventory
    assert 'require_file "${ROOT_DIR}/scripts/setup_environment.sh"' in release_bundle


def test_quickstart_projects_one_command_for_each_supported_host() -> None:
    for path in (REPO_ROOT / "README.md", REPO_ROOT / "docs" / "QUICKSTART.md"):
        document = _read(path)
        assert "./toolchain/setup_macos_arm64.sh" in document
        assert "./toolchain/setup_linux_arm64.sh" in document
        assert "source .venv/bin/activate" in document
        assert "host-tool" in document
        assert "diagnostic QEMU" in document
