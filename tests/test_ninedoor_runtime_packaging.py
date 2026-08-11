# Author: Lukas Bower
# Purpose: Verify QEMU packaging selects the isolated NineDoor runtime and retires stubs.
# Copyright 2026 Lukas Bower

"""Milestone 26e NineDoor target packaging regression tests."""

import pathlib


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
BUILD_SCRIPT = REPO_ROOT / "scripts" / "cohesix-build-run.sh"
HOST_CRATE = REPO_ROOT / "apps" / "nine-door"
ROOT_BUILD = REPO_ROOT / "apps" / "root-task" / "build.rs"
TARGET_KERNEL = REPO_ROOT / "apps" / "nine-door-runtime" / "src" / "kernel.rs"
TARGET_MANIFEST = REPO_ROOT / "apps" / "nine-door-runtime" / "Cargo.toml"


def test_qemu_payload_selects_isolated_ninedoor_runtime() -> None:
    """The selected target package and payload must use the child runtime."""

    source = BUILD_SCRIPT.read_text(encoding="utf-8")

    assert "SEL4_COMPONENT_PACKAGES=(nine-door-runtime " in source
    assert "ROOTFS_COMPONENT_BINS=(\n        nine-door-runtime\n" in source
    assert "SEL4_COMPONENT_PACKAGES=(nine-door " not in source
    assert "ROOTFS_COMPONENT_BINS=(\n        nine-door\n" not in source


def test_root_task_binds_exact_ninedoor_runtime_image() -> None:
    """Target root construction must embed and validate the selected child ELF."""

    script = BUILD_SCRIPT.read_text(encoding="utf-8")
    build = ROOT_BUILD.read_text(encoding="utf-8")

    assert (
        'COHESIX_NINEDOOR_RUNTIME_IMAGE="$SEL4_ARTIFACT_DIR/'
        'nine-door-runtime"'
    ) in script
    assert 'const IMAGE_ENV: &str = "COHESIX_NINEDOOR_RUNTIME_IMAGE";' in build
    assert 'generated_service_image_pages("ninedoor_service", "NineDoor")' in build
    assert "NINEDOOR_RUNTIME_SHA256" in build
    assert "NINEDOOR_RUNTIME_ENTRY_VADDR" in build


def test_host_ninedoor_has_no_stub_binary_entrypoint() -> None:
    """Host NineDoor remains a library and exposes no target spin stub."""

    manifest = (HOST_CRATE / "Cargo.toml").read_text(encoding="utf-8")

    assert "autobins = false" in manifest
    assert not (HOST_CRATE / "src" / "main.rs").exists()
    assert not (HOST_CRATE / "src" / "kernel.rs").exists()


def test_target_runtime_faults_instead_of_spinning() -> None:
    """Invalid init and panic paths must enter standard-fault containment."""

    source = TARGET_KERNEL.read_text(encoding="utf-8")

    assert 'core::arch::asm!("brk #0", options(noreturn, nostack, nomem))' in source
    assert "core::hint::spin_loop" not in source


def test_target_runtime_exposes_only_qemu_gated_request_fault_hooks() -> None:
    """External GDB uses stable symbols without creating target authority."""

    source = TARGET_KERNEL.read_text(encoding="utf-8")
    manifest = TARGET_MANIFEST.read_text(encoding="utf-8")

    assert "qemu-evidence = []" in manifest
    assert "cohesix_ninedoor_qemu_evidence_request_handler" in source
    assert "cohesix_ninedoor_qemu_evidence_standard_fault" in source
    assert source.count('#[cfg(feature = "qemu-evidence")]') == 3
    hook = source.split("/// Stable external-QEMU evidence hook", maxsplit=1)[1]
    hook = hook.split("/// Target entrypoint", maxsplit=1)[0]
    assert "seL4_" not in hook
    assert "enter_standard_fault()" in hook
