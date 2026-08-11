# Copyright 2026 Lukas Bower
# SPDX-License-Identifier: Apache-2.0
# Purpose: Test fail-closed Worker ELF canonicalization and archive admission.
# Author: Lukas Bower

"""Tests for the Milestone 26e Worker image pipeline."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import re
import struct
import sys

import pytest


ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "scripts" / "worker_image_manifest.py"
SPEC = importlib.util.spec_from_file_location("worker_image_manifest", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
worker_images = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = worker_images
SPEC.loader.exec_module(worker_images)


def _source_image(name: str, role: int) -> bytes:
    """Build a minimal valid source ELF with metadata and one symbol table."""

    section_names = b"\0.shstrtab\0.strtab\0.symtab\0.cohesix.worker\0.text\0"
    strings = b"\0_start\0"
    metadata = struct.pack(
        "<IHHHHI32s16s",
        worker_images.WORKER_METADATA_MAGIC,
        1,
        64,
        role,
        1,
        3,
        b"_start" + bytes(26),
        bytes(16),
    )
    entry = 0x201000
    text = b"\xc0\x03\x5f\xd6"
    phoff = 64
    metadata_offset = 0x200
    text_offset = 0x1000
    shstr_offset = 0x1100
    strtab_offset = shstr_offset + len(section_names)
    symtab_offset = (strtab_offset + len(strings) + 7) & ~7
    section_offset = symtab_offset + 48
    file_size = section_offset + 6 * 64
    data = bytearray(file_size)
    data[:16] = b"\x7fELF\x02\x01\x01\x00" + bytes(8)
    struct.pack_into(
        "<HHIQQQIHHHHHH",
        data,
        16,
        2,
        183,
        1,
        entry,
        phoff,
        section_offset,
        0,
        64,
        56,
        2,
        64,
        6,
        1,
    )
    struct.pack_into(
        "<IIQQQQQQ", data, phoff, 1, 4, 0, 0x200000, 0x200000, 0x240, 0x240, 0x1000
    )
    struct.pack_into(
        "<IIQQQQQQ",
        data,
        phoff + 56,
        1,
        5,
        text_offset,
        entry,
        entry,
        len(text),
        len(text),
        0x1000,
    )
    data[metadata_offset : metadata_offset + 64] = metadata
    data[text_offset : text_offset + len(text)] = text
    data[shstr_offset : shstr_offset + len(section_names)] = section_names
    data[strtab_offset : strtab_offset + len(strings)] = strings
    struct.pack_into("<IBBHQQ", data, symtab_offset + 24, 1, 0x12, 0, 5, entry, len(text))

    def section(
        index: int,
        section_name: bytes,
        kind: int,
        flags: int,
        address: int,
        offset: int,
        size: int,
        link: int,
        alignment: int,
        entry_size: int,
    ) -> None:
        name_offset = section_names.index(section_name)
        struct.pack_into(
            "<IIQQQQIIQQ",
            data,
            section_offset + index * 64,
            name_offset,
            kind,
            flags,
            address,
            offset,
            size,
            link,
            0,
            alignment,
            entry_size,
        )

    section(1, b".shstrtab", 3, 0, 0, shstr_offset, len(section_names), 0, 1, 0)
    section(2, b".strtab", 3, 0, 0, strtab_offset, len(strings), 0, 1, 0)
    section(3, b".symtab", 2, 0, 0, symtab_offset, 48, 2, 8, 24)
    section(
        4,
        b".cohesix.worker",
        1,
        2 | worker_images.SHF_GNU_RETAIN,
        0x200000 + metadata_offset,
        metadata_offset,
        64,
        0,
        8,
        0,
    )
    section(5, b".text", 1, 6, entry, text_offset, len(text), 0, 4, 0)
    return bytes(data)


def _source_dir(tmp_path: Path) -> Path:
    image_dir = tmp_path / "images"
    image_dir.mkdir(parents=True)
    for name, role in (("worker-heart", 1), ("worker-gpu", 2), ("worker-lora", 3)):
        (image_dir / name).write_bytes(_source_image(name, role))
    return image_dir


def _build(tmp_path: Path) -> tuple[Path, Path]:
    archive = tmp_path / "cohesix-worker-images.cpio"
    manifest = tmp_path / "cohesix-worker-image-manifest.json"
    worker_images.build_manifest(
        _source_dir(tmp_path),
        tmp_path / "canonical",
        archive,
        manifest,
        "aarch64-unknown-none",
        "release",
    )
    return archive, manifest


def test_build_is_deterministic_and_role_complete(tmp_path: Path) -> None:
    archive, manifest = _build(tmp_path)
    first_archive = archive.read_bytes()
    first_manifest = manifest.read_bytes()
    worker_images.build_manifest(
        tmp_path / "images",
        tmp_path / "canonical-two",
        archive,
        manifest,
        "aarch64-unknown-none",
        "release",
    )
    assert archive.read_bytes() == first_archive
    assert manifest.read_bytes() == first_manifest
    document = worker_images.verify_manifest(manifest, archive)
    assert [row["name"] for row in document["images"]] == list(
        worker_images.EXPECTED_NAMES
    )
    assert [row["role"] for row in document["images"]] == [
        "worker-heartbeat",
        "worker-gpu",
        "worker-lora",
    ]
    for row in document["images"]:
        assert row["image_bytes"] < row["source_sha256"].__len__() * 100


def test_wrong_architecture_and_writable_executable_segment_fail(tmp_path: Path) -> None:
    image = bytearray(_source_image("worker-heart", 1))
    struct.pack_into("<H", image, 18, 62)
    path = tmp_path / "wrong-machine"
    path.write_bytes(image)
    with pytest.raises(worker_images.WorkerImageError, match="AArch64"):
        worker_images.inspect_image(path, "worker-heart")

    image = bytearray(_source_image("worker-heart", 1))
    struct.pack_into("<I", image, 64 + 56 + 4, 7)
    path.write_bytes(image)
    with pytest.raises(worker_images.WorkerImageError, match="writable executable"):
        worker_images.inspect_image(path, "worker-heart")


def test_role_metadata_and_entrypoint_tampering_fail(tmp_path: Path) -> None:
    image = bytearray(_source_image("worker-heart", 1))
    struct.pack_into("<H", image, 0x200 + 8, 2)
    path = tmp_path / "wrong-role"
    path.write_bytes(image)
    with pytest.raises(worker_images.WorkerImageError, match="role"):
        worker_images.inspect_image(path, "worker-heart")

    image = bytearray(_source_image("worker-heart", 1))
    struct.pack_into("<Q", image, 24, 0x201004)
    path.write_bytes(image)
    with pytest.raises(worker_images.WorkerImageError, match="entrypoint|_start"):
        worker_images.inspect_image(path, "worker-heart")


def test_archive_and_manifest_tampering_fail(tmp_path: Path) -> None:
    archive, manifest = _build(tmp_path)
    tampered = bytearray(archive.read_bytes())
    tampered[512] ^= 1
    archive.write_bytes(tampered)
    with pytest.raises(worker_images.WorkerImageError, match="identity"):
        worker_images.verify_manifest(manifest, archive)

    archive, manifest = _build(tmp_path / "second")
    document = json.loads(manifest.read_text())
    document["images"][0]["role"] = "worker-gpu"
    manifest.write_text(json.dumps(document))
    with pytest.raises(worker_images.WorkerImageError, match="role"):
        worker_images.verify_manifest(manifest, archive)


def test_missing_lora_and_duplicate_archive_names_fail(tmp_path: Path) -> None:
    image_dir = _source_dir(tmp_path)
    (image_dir / "worker-lora").unlink()
    with pytest.raises(worker_images.WorkerImageError, match="missing"):
        worker_images.build_manifest(
            image_dir,
            tmp_path / "canonical",
            tmp_path / "archive",
            tmp_path / "manifest",
            "aarch64-unknown-none",
            "release",
        )
    with pytest.raises(worker_images.WorkerImageError, match="unique and sorted"):
        worker_images.build_newc((("same", b"one"), ("same", b"two")))


def test_qemu_build_orders_worker_identity_before_root_and_keeps_archive_separate() -> None:
    script = (ROOT / "scripts" / "cohesix-build-run.sh").read_text(encoding="utf-8")
    package_line = next(
        line for line in script.splitlines() if line.strip().startswith("SEL4_COMPONENT_PACKAGES=")
    )
    assert "worker-heart" in package_line
    assert "worker-gpu" in package_line
    assert "worker-lora" in package_line
    component_build = script.index('cargo "${SEL4_BUILD_ARGS[@]}"')
    worker_build = script.index('python3 "$WORKER_MANIFEST_TOOL" build')
    root_build = script.index('COHESIX_WORKER_IMAGE_ARCHIVE="$WORKER_ARCHIVE_PATH"')
    assert component_build < worker_build < root_build
    rootfs_start = script.index("ROOTFS_COMPONENT_BINS=(")
    rootfs_end = script.index(")", rootfs_start)
    rootfs_block = script[rootfs_start:rootfs_end]
    assert "worker-heart" not in rootfs_block
    assert "worker-gpu" not in rootfs_block
    assert "worker-lora" not in rootfs_block
    assert "cohesix/artifacts/cohesix-worker-images.cpio" in script
    assert "cohesix/artifacts/cohesix-worker-image-manifest.json" in script
    assert "has_root_task_feature release-qemu" in script
    assert "has_root_task_feature bootstrap-trace" in script
    for feature in (
        "nine-door-runtime/qemu-evidence",
        "console-network-runtime/qemu-evidence",
        "worker-heart/qemu-evidence",
        "worker-gpu/qemu-evidence",
        "worker-lora/qemu-evidence",
    ):
        assert feature in script


def test_qemu_evidence_symbols_are_gated_and_have_no_authority_path() -> None:
    runtime = (ROOT / "apps" / "worker-heart" / "src" / "target_runtime.rs").read_text(
        encoding="utf-8"
    )
    symbols = (
        "cohesix_worker_qemu_evidence_control_handler",
        "cohesix_worker_qemu_evidence_standard_fault",
        "cohesix_worker_qemu_evidence_timeout_spin",
    )
    declared = set(
        re.findall(
            r'pub extern "C" fn (cohesix_worker_qemu_evidence_[a-z_]+)', runtime
        )
    )
    assert declared == set(symbols)
    for symbol in symbols:
        declaration = runtime.index(f'pub extern "C" fn {symbol}')
        prefix = runtime[max(0, declaration - 160) : declaration]
        assert '#[cfg(feature = "qemu-evidence")]' in prefix
    assert (
        '#[cfg(feature = "qemu-evidence")]\n'
        "    cohesix_worker_qemu_evidence_control_handler();"
    ) in runtime
    hook_block = runtime.split("/// Stable external-QEMU evidence hook", maxsplit=1)[1]
    hook_block = hook_block.split("/// Enter one isolated Worker", maxsplit=1)[0]
    assert "sel4_sys" not in hook_block
    assert "enqueue" not in hook_block
    assert "enter_standard_fault()" in runtime
    assert "core::hint::spin_loop()" in runtime


def test_root_build_requires_both_target_qualified_worker_identities() -> None:
    build_rs = (ROOT / "apps" / "root-task" / "build.rs").read_text(encoding="utf-8")
    assert "COHESIX_WORKER_IMAGE_MANIFEST" in build_rs
    assert "COHESIX_WORKER_IMAGE_ARCHIVE" in build_rs
    assert "target root-task builds require" in build_rs
    loader = (
        ROOT / "apps" / "root-task" / "src" / "hal" / "worker_image.rs"
    ).read_text(encoding="utf-8")
    assert "WORKER_ARCHIVE_SHA256" in loader
    assert "WORKER_MANIFEST_SHA256" in loader
    assert "WorkerSegmentRights::ReadExecute" in loader
