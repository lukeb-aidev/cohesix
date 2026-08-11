# Copyright 2026 Lukas Bower
# SPDX-License-Identifier: Apache-2.0
# Purpose: Verify exact target binding and payload placement for the isolated console-network runtime.
# Author: Lukas Bower

"""Milestone 26e console-network image binding and packaging regressions."""

from __future__ import annotations

import pathlib


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
BUILD_SCRIPT = REPO_ROOT / "scripts" / "cohesix-build-run.sh"
ROOT_BUILD = REPO_ROOT / "apps" / "root-task" / "build.rs"
RUNTIME_MANIFEST = REPO_ROOT / "apps" / "console-network-runtime" / "Cargo.toml"
RUNTIME_MAIN = REPO_ROOT / "apps" / "console-network-runtime" / "src" / "main.rs"
RUNTIME_KERNEL = REPO_ROOT / "apps" / "console-network-runtime" / "src" / "kernel.rs"
ROOT_MANIFEST = REPO_ROOT / "configs" / "root_task.toml"


def test_target_runtime_is_built_bound_then_staged_at_compiler_path() -> None:
    """The one compiled target ELF must be root-bound and packaged once."""

    source = BUILD_SCRIPT.read_text(encoding="utf-8")

    assert "SEL4_COMPONENT_PACKAGES=(nine-door-runtime console-network-runtime " in source
    assert (
        'COHESIX_CONSOLE_NETWORK_RUNTIME_IMAGE="$SEL4_ARTIFACT_DIR/'
        'console-network-runtime"' in source
    )
    assert '"$ARTIFACTS_DIR/console-network-runtime"' in source
    rootfs_block = source.split("ROOTFS_COMPONENT_BINS=(", maxsplit=1)[1].split(
        ")", maxsplit=1
    )[0]
    assert "console-network-runtime" not in rootfs_block
    assert 'MANIFEST_INPUTS+=("cohesix/artifacts/console-network-runtime")' in source
    assert '"console_network_runtime": {' in source


def test_root_build_fails_closed_and_binds_exact_elf_identity() -> None:
    """Runtime-eligible root builds require validated AArch64 W^X bytes."""

    source = ROOT_BUILD.read_text(encoding="utf-8")

    assert 'const IMAGE_ENV: &str = "COHESIX_CONSOLE_NETWORK_RUNTIME_IMAGE";' in source
    assert "target root-task builds require {IMAGE_ENV}" in source
    assert "validate_console_network_elf(&image)?" in source
    assert "console_network_has_exact_entry_symbol(bytes, entry)?" in source
    assert "console-network ELF load segment violates bounds or W^X" in source
    assert "identity.load_pages != expected_pages" in source
    assert "CONSOLE_NETWORK_RUNTIME_SHA256" in source
    assert "include_bytes!({include_path})" in source


def test_runtime_and_compiler_contract_remain_no_std_and_path_identical() -> None:
    """The target child has no host fallback authority or packaging alias."""

    manifest = RUNTIME_MANIFEST.read_text(encoding="utf-8")
    main = RUNTIME_MAIN.read_text(encoding="utf-8")
    compiler_manifest = ROOT_MANIFEST.read_text(encoding="utf-8")

    assert 'name = "console-network-runtime"' in manifest
    assert 'target_os = "none"' in manifest
    assert "#![cfg_attr(target_os = \"none\", no_std)]" in main
    assert 'image_path = "cohesix/artifacts/console-network-runtime"' in compiler_manifest
    assert 'entry_symbol = "_start"' in compiler_manifest


def test_runtime_qemu_fault_hooks_are_diagnostic_and_control_path_bound() -> None:
    """The GDB hooks are gated and reached only by admitted control turns."""

    manifest = RUNTIME_MANIFEST.read_text(encoding="utf-8")
    source = RUNTIME_KERNEL.read_text(encoding="utf-8")

    assert "qemu-evidence = []" in manifest
    assert "cohesix_console_network_qemu_evidence_control_handler" in source
    assert "cohesix_console_network_qemu_evidence_standard_fault" in source
    assert "cohesix_console_network_qemu_evidence_timeout_spin" in source
    control_branch = source.split("if badge & WAKE_CONTROL != 0", maxsplit=1)[1]
    control_branch = control_branch.split("if badge & WAKE_SHUTDOWN != 0", maxsplit=1)[0]
    assert "cohesix_console_network_qemu_evidence_control_handler();" in control_branch
    hook = source.split("/// Stable external-QEMU evidence hook", maxsplit=1)[1]
    hook = hook.split("/// Target entry", maxsplit=1)[0]
    assert "seL4_" not in hook
