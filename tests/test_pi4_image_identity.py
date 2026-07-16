# Author: Lukas Bower
# Purpose: Test content-derived identities for complete Pi 4 U-Boot images.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4_image_identity.py."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import pathlib
import struct
import subprocess
import sys
import types
import zlib

import pytest


MODULE_PATH = (
    pathlib.Path(__file__).resolve().parents[1]
    / "scripts"
    / "pi4_image_identity.py"
)
SPEC = importlib.util.spec_from_file_location("pi4_image_identity", MODULE_PATH)
identity = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = identity
SPEC.loader.exec_module(identity)

FULL_COMMIT = "0123456789abcdef0123456789abcdef01234567"
OTHER_COMMIT = "89abcdef0123456789abcdef0123456789abcdef"
EMBEDDED_COMMIT = FULL_COMMIT[:12]
BUILD_TIMESTAMP = "2026-07-16T00:00:00Z"
ELF_LOAD_ADDRESS = 0x0040_0000
ELF_MARKER_OFFSET = 0x200


def build_marker(
    image_id: str = identity.UNSEALED_IMAGE_ID,
    *,
    git_hash: str = EMBEDDED_COMMIT,
    dirty: bool = False,
    build_timestamp: str = BUILD_TIMESTAMP,
) -> str:
    """Return a marker with exact production identity fields."""

    dirty_suffix = "-dirty" if dirty else ""
    return (
        f"[BUILD] {git_hash}{dirty_suffix} {build_timestamp} image-id={image_id} "
        "features=[kernel:1 bootstrap-trace:1 serial-console:1 net:1 "
        "net-console:1 qemu-driver-task-smoke:0]"
    )


def repair_crcs(image: bytearray) -> None:
    """Repair legacy U-Boot CRCs after a deliberate test mutation."""

    image[identity.UIMAGE_DATA_CRC_OFFSET : identity.UIMAGE_DATA_CRC_OFFSET + 4] = (
        struct.pack(
            ">I", zlib.crc32(image[identity.UIMAGE_HEADER_BYTES :]) & 0xFFFF_FFFF
        )
    )
    image[
        identity.UIMAGE_HEADER_CRC_OFFSET : identity.UIMAGE_HEADER_CRC_OFFSET + 4
    ] = b"\0" * 4
    image[
        identity.UIMAGE_HEADER_CRC_OFFSET : identity.UIMAGE_HEADER_CRC_OFFSET + 4
    ] = struct.pack(
        ">I",
        zlib.crc32(image[: identity.UIMAGE_HEADER_BYTES]) & 0xFFFF_FFFF,
    )


def make_uimage(
    *,
    marker: str | None = None,
    prefix: bytes = b"kernel-elfloader-root-prefix",
    suffix: bytes = b"root-suffix",
) -> bytes:
    """Build a minimal valid legacy uImage carrying one build marker."""

    marker_bytes = (marker or build_marker()).encode("ascii")
    return make_uimage_from_payload(prefix + b"\0" + marker_bytes + b"\0" + suffix)


def make_uimage_from_payload(payload: bytes) -> bytes:
    """Wrap exact payload bytes in the accepted legacy-image envelope."""

    image = bytearray(identity.UIMAGE_HEADER_BYTES + len(payload))
    struct.pack_into(">I", image, 0, identity.UIMAGE_MAGIC)
    struct.pack_into(">I", image, identity.UIMAGE_DATA_SIZE_OFFSET, len(payload))
    struct.pack_into(
        ">I", image, 16, identity.EXPECTED_UIMAGE_LOAD_ADDRESS
    )
    struct.pack_into(
        ">I", image, 20, identity.EXPECTED_UIMAGE_ENTRY_POINT
    )
    image[28:32] = bytes(
        (
            identity.UIMAGE_OS_LINUX,
            identity.UIMAGE_ARCH_ARM64,
            identity.UIMAGE_TYPE_KERNEL,
            identity.UIMAGE_COMPRESSION_NONE,
        )
    )
    image[identity.UIMAGE_HEADER_BYTES :] = payload
    repair_crcs(image)
    return bytes(image)


def make_marker_elf(
    *,
    marker: str | None = None,
    allocated: bool = True,
    marker_in_load: bool = True,
    duplicate_load: bool = False,
) -> bytes:
    """Build a strict minimal AArch64 ELF64 with one marker section."""

    marker_bytes = (marker or build_marker()).encode("ascii")
    strings = b"\0.cohesix_build_marker\0.shstrtab\0"
    marker_name = 1
    strings_name = strings.index(b".shstrtab")
    strings_offset = ELF_MARKER_OFFSET + len(marker_bytes)
    section_offset = (strings_offset + len(strings) + 7) & ~7
    section_count = 3
    program_count = 2 if duplicate_load else 1
    image = bytearray(
        section_offset + section_count * identity.ELF_SECTION_HEADER_BYTES
    )
    image[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<H", image, 16, identity.ELF_ET_EXEC)
    struct.pack_into("<H", image, 18, identity.ELF_EM_AARCH64)
    struct.pack_into("<I", image, 20, identity.ELF_VERSION_CURRENT)
    struct.pack_into("<Q", image, 32, identity.ELF_HEADER_BYTES)
    struct.pack_into("<Q", image, 40, section_offset)
    struct.pack_into("<H", image, 52, identity.ELF_HEADER_BYTES)
    struct.pack_into("<H", image, 54, identity.ELF_PROGRAM_HEADER_BYTES)
    struct.pack_into("<H", image, 56, program_count)
    struct.pack_into("<H", image, 58, identity.ELF_SECTION_HEADER_BYTES)
    struct.pack_into("<H", image, 60, section_count)
    struct.pack_into("<H", image, 62, 2)

    program_size = ELF_MARKER_OFFSET + (
        len(marker_bytes) if marker_in_load else len(marker_bytes) - 1
    )
    for program_index in range(program_count):
        header = identity.ELF_HEADER_BYTES + (
            program_index * identity.ELF_PROGRAM_HEADER_BYTES
        )
        struct.pack_into("<I", image, header, identity.ELF_PT_LOAD)
        struct.pack_into("<I", image, header + 4, identity.ELF_PF_R)
        struct.pack_into("<Q", image, header + 8, 0)
        struct.pack_into("<Q", image, header + 16, ELF_LOAD_ADDRESS)
        struct.pack_into("<Q", image, header + 24, ELF_LOAD_ADDRESS)
        struct.pack_into("<Q", image, header + 32, program_size)
        struct.pack_into("<Q", image, header + 40, program_size)
        struct.pack_into("<Q", image, header + 48, 0x1000)

    image[ELF_MARKER_OFFSET : ELF_MARKER_OFFSET + len(marker_bytes)] = marker_bytes
    image[strings_offset : strings_offset + len(strings)] = strings

    marker_header = section_offset + identity.ELF_SECTION_HEADER_BYTES
    struct.pack_into("<I", image, marker_header, marker_name)
    struct.pack_into("<I", image, marker_header + 4, identity.ELF_SHT_PROGBITS)
    struct.pack_into(
        "<Q",
        image,
        marker_header + 8,
        identity.ELF_SHF_ALLOC if allocated else 0,
    )
    struct.pack_into(
        "<Q", image, marker_header + 16, ELF_LOAD_ADDRESS + ELF_MARKER_OFFSET
    )
    struct.pack_into("<Q", image, marker_header + 24, ELF_MARKER_OFFSET)
    struct.pack_into("<Q", image, marker_header + 32, len(marker_bytes))
    struct.pack_into("<Q", image, marker_header + 48, 1)

    strings_header = section_offset + 2 * identity.ELF_SECTION_HEADER_BYTES
    struct.pack_into("<I", image, strings_header, strings_name)
    struct.pack_into("<I", image, strings_header + 4, identity.ELF_SHT_STRTAB)
    struct.pack_into("<Q", image, strings_header + 24, strings_offset)
    struct.pack_into("<Q", image, strings_header + 32, len(strings))
    struct.pack_into("<Q", image, strings_header + 48, 1)
    return bytes(image)


def _newc_header(
    *,
    ino: int,
    mode: int,
    file_size: int,
    name_size: int,
    checksum: int = 0,
) -> bytes:
    """Render one deterministic newc header."""

    values = (
        ino,
        mode,
        0,
        0,
        1,
        0,
        file_size,
        0,
        0,
        0,
        0,
        name_size,
        checksum,
    )
    return identity.NEWC_MAGIC + b"".join(
        f"{value:08x}".encode("ascii") for value in values
    )


def make_newc(
    entries: list[tuple[bytes, bytes, int]],
    *,
    block_padding: bool = True,
) -> bytes:
    """Build a deterministic strict newc archive."""

    archive = bytearray()
    for ino, (name, data, mode) in enumerate(entries, start=1):
        name_field = name + b"\0"
        archive.extend(
            _newc_header(
                ino=ino,
                mode=mode,
                file_size=len(data),
                name_size=len(name_field),
            )
        )
        archive.extend(name_field)
        archive.extend(b"\0" * ((-len(archive)) % 4))
        archive.extend(data)
        archive.extend(b"\0" * ((-len(archive)) % 4))
    trailer_field = identity.NEWC_TRAILER + b"\0"
    archive.extend(
        _newc_header(
            ino=len(entries) + 1,
            mode=0,
            file_size=0,
            name_size=len(trailer_field),
        )
    )
    archive.extend(trailer_field)
    archive.extend(b"\0" * ((-len(archive)) % 4))
    if block_padding:
        archive.extend(b"\0" * ((-len(archive)) % 512))
    return bytes(archive)


def make_root_cpio(root_elf: bytes, *, mode: int = 0o100755) -> bytes:
    """Build the exact elfloader rootserver archive used by wrapper tests."""

    return make_newc(
        [
            (b"kernel.elf", b"kernel-bytes", 0o100644),
            (identity.ROOTSERVER_MEMBER, root_elf, mode),
        ]
    )


def write_packaging_fixture(
    tmp_path: pathlib.Path,
    *,
    sealed: bool,
    marker: str | None = None,
    payload_prefix: bytes = b"elfloader-prefix",
    payload_suffix: bytes = b"elfloader-suffix",
) -> tuple[pathlib.Path, pathlib.Path, pathlib.Path]:
    """Write one root ELF, exact newc, and wrapper fixture."""

    tmp_path.mkdir(parents=True, exist_ok=True)
    root_data = make_marker_elf(marker=marker)
    cpio_data = make_root_cpio(root_data)
    image_data = make_uimage_from_payload(payload_prefix + cpio_data + payload_suffix)
    if sealed:
        image_data, _image_id, _marker = identity.seal_image_bytes(image_data)
    root_path = tmp_path / "root-task"
    cpio_path = tmp_path / "archive.archive.o.cpio"
    image_path = tmp_path / "cohesix-image-arm-bcm2711"
    root_path.write_bytes(root_data)
    cpio_path.write_bytes(cpio_data)
    image_path.write_bytes(image_data)
    return image_path, root_path, cpio_path


def publish_args(
    image_path: pathlib.Path,
    metadata_path: pathlib.Path,
    root_path: pathlib.Path,
    cpio_path: pathlib.Path,
    *,
    git_commit: str = FULL_COMMIT,
) -> list[str]:
    """Return exact CLI arguments for v2 metadata publication."""

    return [
        "verify",
        "--image",
        str(image_path),
        "--metadata",
        str(metadata_path),
        "--git-commit",
        git_commit,
        "--source-tree-clean",
        "--expected-root-elf",
        str(root_path),
        "--expected-root-cpio",
        str(cpio_path),
    ]


def test_seal_binds_marker_to_complete_image_and_repairs_crcs() -> None:
    """A sealed marker verifies as the normalized digest of every image byte."""

    sealed, image_id, marker = identity.seal_image_bytes(make_uimage())
    record = identity.inspect_image_bytes(sealed)

    assert record.schema == identity.SCHEMA
    assert record.image_id == image_id
    assert record.build_marker == marker
    assert record.embedded_git_commit == EMBEDDED_COMMIT
    assert record.build_timestamp == BUILD_TIMESTAMP
    assert record.git_commit is None
    assert record.build_id is None
    assert image_id in marker
    assert identity.UNSEALED_IMAGE_ID not in marker
    assert record.image_sha256 != image_id


def test_different_complete_images_cannot_share_one_serial_marker() -> None:
    """Kernel or elfloader differences produce distinct UART identities."""

    first, first_id, first_marker = identity.seal_image_bytes(
        make_uimage(prefix=b"kernel-a")
    )
    second, second_id, second_marker = identity.seal_image_bytes(
        make_uimage(prefix=b"kernel-b")
    )

    assert first_id != second_id
    assert first_marker != second_marker
    assert identity.inspect_image_bytes(first).image_id == first_id
    assert identity.inspect_image_bytes(second).image_id == second_id


def test_valid_crc_mutation_still_fails_content_identity() -> None:
    """Repairing U-Boot CRCs cannot hide a changed complete image."""

    sealed, _image_id, _marker = identity.seal_image_bytes(make_uimage())
    mutated = bytearray(sealed)
    mutated[identity.UIMAGE_HEADER_BYTES] ^= 0x01
    repair_crcs(mutated)

    with pytest.raises(identity.ImageIdentityError, match="normalized image"):
        identity.inspect_image_bytes(bytes(mutated))


def test_seal_rejects_duplicate_or_already_sealed_markers() -> None:
    """Ambiguous or already sealed normalization candidates fail closed."""

    marker = build_marker()
    duplicate = make_uimage(marker=marker, suffix=marker.encode("ascii"))
    with pytest.raises(identity.ImageIdentityError, match="found 2"):
        identity.seal_image_bytes(duplicate)

    sealed, _image_id, _marker = identity.seal_image_bytes(make_uimage())
    with pytest.raises(identity.ImageIdentityError, match="already sealed"):
        identity.seal_image_bytes(sealed)


def test_inspection_rejects_bad_uimage_crc() -> None:
    """Identity verification includes the bootloader integrity envelope."""

    sealed, _image_id, _marker = identity.seal_image_bytes(make_uimage())
    corrupted = bytearray(sealed)
    corrupted[-1] ^= 0x01

    with pytest.raises(identity.ImageIdentityError, match="data CRC mismatch"):
        identity.inspect_image_bytes(bytes(corrupted))


@pytest.mark.parametrize(
    ("offset", "replacement", "message"),
    [
        (0, struct.pack(">I", 0xDEADBEEF), "not a legacy U-Boot"),
        (16, struct.pack(">I", 0x2000_0000), "load address"),
        (20, struct.pack(">I", 0x2000_0000), "entry point"),
        (28, bytes((0,)), "OS/architecture/type/compression"),
        (29, bytes((2,)), "OS/architecture/type/compression"),
        (30, bytes((4,)), "OS/architecture/type/compression"),
        (31, bytes((1,)), "OS/architecture/type/compression"),
    ],
)
def test_structural_uimage_fields_are_exact(
    offset: int,
    replacement: bytes,
    message: str,
) -> None:
    """Wrong target envelopes fail even when their CRCs are repaired."""

    sealed, _image_id, _marker = identity.seal_image_bytes(make_uimage())
    mutated = bytearray(sealed)
    mutated[offset : offset + len(replacement)] = replacement
    repair_crcs(mutated)

    with pytest.raises(identity.ImageIdentityError, match=message):
        identity.inspect_image_bytes(bytes(mutated))


def test_truncation_trailing_bytes_and_normalized_fields_fail() -> None:
    """The digest covers exactly the declared file and constrains exclusions."""

    sealed, _image_id, marker = identity.seal_image_bytes(make_uimage())
    for malformed in (sealed[:-1], sealed + b"trailing"):
        with pytest.raises(identity.ImageIdentityError, match="payload size"):
            identity.inspect_image_bytes(malformed)

    changed_id = bytearray(sealed)
    id_start = (
        changed_id.index(marker.encode("ascii"))
        + marker.index("image-id=")
        + len("image-id=")
    )
    changed_id[id_start] = ord("1") if changed_id[id_start] != ord("1") else ord("2")
    repair_crcs(changed_id)
    with pytest.raises(identity.ImageIdentityError, match="normalized image"):
        identity.inspect_image_bytes(bytes(changed_id))


def test_stable_reader_opens_once_and_records_descriptor_identity(
    tmp_path: pathlib.Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Stable reads retain descriptor identity without reopening by path."""

    path = tmp_path / "evidence.bin"
    path.write_bytes(b"stable evidence")
    original_open = identity.os.open
    opens = 0

    def counted_open(candidate: object, *args: object, **kwargs: object) -> int:
        nonlocal opens
        if pathlib.Path(candidate) == path:
            opens += 1
        return original_open(candidate, *args, **kwargs)

    monkeypatch.setattr(identity.os, "open", counted_open)
    snapshot = identity.read_stable_regular_file(path)

    assert opens == 1
    assert snapshot.data == b"stable evidence"
    assert snapshot.device == path.stat().st_dev
    assert snapshot.inode == path.stat().st_ino
    assert snapshot.size_bytes == len(snapshot.data)


def test_stable_reader_rejects_fifo_without_waiting_for_a_writer(
    tmp_path: pathlib.Path,
) -> None:
    """A named pipe is rejected after nonblocking open instead of hanging."""

    fifo = tmp_path / "evidence.fifo"
    os.mkfifo(fifo)

    with pytest.raises(identity.ImageIdentityError, match="not a regular file"):
        identity.read_stable_regular_file(fifo)


def test_stable_reader_reports_nul_path_as_typed_identity_error() -> None:
    """Malformed path text cannot escape through an uncaught ValueError."""

    with pytest.raises(identity.ImageIdentityError, match="failed to read"):
        identity.read_stable_regular_file(pathlib.Path("bad\0path"))


@pytest.mark.parametrize(
    "changed_field",
    ["st_dev", "st_ino", "st_mode", "st_size", "st_mtime_ns", "st_ctime_ns"],
)
def test_stable_reader_rejects_descriptor_changes(
    tmp_path: pathlib.Path,
    monkeypatch: pytest.MonkeyPatch,
    changed_field: str,
) -> None:
    """Every required fstat identity field must remain unchanged."""

    path = tmp_path / "evidence.bin"
    path.write_bytes(b"stable evidence")
    original_fstat = identity.os.fstat
    calls = 0

    def controlled_fstat(fd: int) -> object:
        nonlocal calls
        calls += 1
        status = original_fstat(fd)
        values = {
            "st_dev": status.st_dev,
            "st_ino": status.st_ino,
            "st_mode": status.st_mode,
            "st_size": status.st_size,
            "st_mtime_ns": status.st_mtime_ns,
            "st_ctime_ns": status.st_ctime_ns,
        }
        if calls == 2:
            values[changed_field] += 1
        return types.SimpleNamespace(**values)

    monkeypatch.setattr(identity.os, "fstat", controlled_fstat)
    with pytest.raises(identity.ImageIdentityError, match="changed while"):
        identity.read_stable_regular_file(path)


def test_stable_reader_rejects_nonregular_input(tmp_path: pathlib.Path) -> None:
    """Directories and other nonregular evidence cannot be accepted."""

    with pytest.raises(identity.ImageIdentityError):
        identity.read_stable_regular_file(tmp_path)


def test_strict_elf_marker_positive(tmp_path: pathlib.Path) -> None:
    """A valid ELF64 AArch64 executable maps its marker exactly once."""

    artifact = tmp_path / "root-task"
    artifact.write_bytes(make_marker_elf())

    assert identity.verify_unsealed_elf_marker(artifact) == build_marker()


def mutate_elf(data: bytes, case: str) -> bytes:
    """Return one ELF carrying the selected structural violation."""

    mutated = bytearray(data)
    section_offset = struct.unpack_from("<Q", data, 40)[0]
    marker_header = section_offset + identity.ELF_SECTION_HEADER_BYTES
    strings_header = section_offset + 2 * identity.ELF_SECTION_HEADER_BYTES
    if case == "wrong-class":
        mutated[4] = 1
    elif case == "wrong-endian":
        mutated[5] = 2
    elif case == "et-dyn":
        struct.pack_into("<H", mutated, 16, 3)
    elif case == "wrong-machine":
        struct.pack_into("<H", mutated, 18, 62)
    elif case == "bad-header-size":
        struct.pack_into("<H", mutated, 52, 63)
    elif case == "sht-nobits":
        struct.pack_into("<I", mutated, marker_header + 4, 8)
    elif case == "not-allocated":
        struct.pack_into("<Q", mutated, marker_header + 8, 0)
    elif case == "bad-string-table":
        struct.pack_into("<I", mutated, strings_header + 4, 1)
    elif case == "no-pf-r":
        struct.pack_into("<I", mutated, identity.ELF_HEADER_BYTES + 4, 2)
    elif case == "filesz-gt-memsz":
        struct.pack_into("<Q", mutated, identity.ELF_HEADER_BYTES + 40, 1)
    elif case == "file-coverage":
        struct.pack_into(
            "<Q", mutated, identity.ELF_HEADER_BYTES + 32, ELF_MARKER_OFFSET
        )
    elif case == "memory-coverage":
        struct.pack_into(
            "<Q", mutated, identity.ELF_HEADER_BYTES + 40, ELF_MARKER_OFFSET
        )
    elif case == "incongruent-address":
        address = struct.unpack_from("<Q", mutated, marker_header + 16)[0]
        struct.pack_into("<Q", mutated, marker_header + 16, address + 1)
    elif case == "bad-load-alignment":
        struct.pack_into("<Q", mutated, identity.ELF_HEADER_BYTES + 48, 3)
    else:
        raise AssertionError(f"unknown mutation: {case}")
    return bytes(mutated)


@pytest.mark.parametrize(
    "case",
    [
        "wrong-class",
        "wrong-endian",
        "et-dyn",
        "wrong-machine",
        "bad-header-size",
        "sht-nobits",
        "not-allocated",
        "bad-string-table",
        "no-pf-r",
        "filesz-gt-memsz",
        "file-coverage",
        "memory-coverage",
        "incongruent-address",
        "bad-load-alignment",
    ],
)
def test_strict_elf_rejects_structural_and_mapping_violations(
    tmp_path: pathlib.Path,
    case: str,
) -> None:
    """Every executable, section, and PT_LOAD invariant fails closed."""

    artifact = tmp_path / "root-task"
    artifact.write_bytes(mutate_elf(make_marker_elf(), case))

    with pytest.raises(identity.ImageIdentityError):
        identity.verify_unsealed_elf_marker(artifact)


def test_strict_elf_rejects_zero_or_duplicate_containing_loads(
    tmp_path: pathlib.Path,
) -> None:
    """The marker must belong to exactly one readable file-backed PT_LOAD."""

    artifact = tmp_path / "root-task"
    artifact.write_bytes(make_marker_elf(marker_in_load=False))
    with pytest.raises(identity.ImageIdentityError, match="exactly one readable"):
        identity.verify_unsealed_elf_marker(artifact)

    artifact.write_bytes(make_marker_elf(duplicate_load=True))
    with pytest.raises(identity.ImageIdentityError, match="exactly one readable"):
        identity.verify_unsealed_elf_marker(artifact)


def test_newc_parser_accepts_exact_archive_and_rootserver() -> None:
    """A strict archive exposes the exact regular rootserver bytes."""

    root_data = make_marker_elf()
    archive = make_root_cpio(root_data)
    members = identity.parse_newc(archive)

    assert [member.name for member in members] == [b"kernel.elf", b"rootserver"]
    assert members[-1].data == root_data
    assert members[-1].mode & identity.NEWC_FILE_TYPE_MASK == (
        identity.NEWC_REGULAR_FILE
    )


def malformed_newc(data: bytes, case: str) -> bytes:
    """Return one archive carrying the selected strict-parser violation."""

    mutated = bytearray(data)
    if case == "bad-magic":
        mutated[0] = ord("9")
    elif case == "nonhex-field":
        mutated[6] = ord("x")
    elif case == "nonzero-check":
        mutated[102:110] = b"00000001"
    elif case == "empty-name":
        mutated[94:102] = b"00000000"
    elif case == "unterminated-name":
        name_size = int(mutated[94:102], 16)
        mutated[identity.NEWC_HEADER_BYTES + name_size - 1] = ord("x")
    elif case == "nonzero-name-padding":
        name_size = int(mutated[94:102], 16)
        padding_start = identity.NEWC_HEADER_BYTES + name_size
        mutated[padding_start] = 1
    elif case == "truncated-header":
        return bytes(mutated[:100])
    elif case == "missing-trailer":
        trailer = data.find(identity.NEWC_MAGIC, 1)
        assert trailer > 0
        return data[:trailer]
    elif case == "nonzero-tail":
        mutated[-1] = 1
    else:
        raise AssertionError(f"unknown mutation: {case}")
    return bytes(mutated)


@pytest.mark.parametrize(
    "case",
    [
        "bad-magic",
        "nonhex-field",
        "nonzero-check",
        "empty-name",
        "unterminated-name",
        "nonzero-name-padding",
        "truncated-header",
        "missing-trailer",
        "nonzero-tail",
    ],
)
def test_newc_parser_rejects_malformed_archives(case: str) -> None:
    """Malformed headers, names, padding, truncation, and tails fail closed."""

    archive = make_newc([(b"rootserver", b"x", 0o100755)])

    with pytest.raises(identity.ImageIdentityError):
        identity.parse_newc(malformed_newc(archive, case))


def test_newc_parser_rejects_nonzero_data_padding() -> None:
    """Member data padding is part of the strict archive envelope."""

    archive = bytearray(make_newc([(b"a", b"x", 0o100644)]))
    name_size = int(archive[94:102], 16)
    data_offset = identity._align4(identity.NEWC_HEADER_BYTES + name_size)
    archive[data_offset + 1] = 1

    with pytest.raises(identity.ImageIdentityError, match="data padding"):
        identity.parse_newc(bytes(archive))


@pytest.mark.parametrize("sealed", [False, True])
def test_exact_newc_rootserver_membership_survives_sealing(
    tmp_path: pathlib.Path,
    sealed: bool,
) -> None:
    """The final wrapper embeds one exact archive before and after normalization."""

    image_path, root_path, cpio_path = write_packaging_fixture(
        tmp_path, sealed=sealed
    )
    membership = identity.verify_uimage_root_archive(
        image_path, root_path, cpio_path
    )

    assert membership.sealed is sealed
    assert membership.rootserver_sha256 == hashlib.sha256(
        root_path.read_bytes()
    ).hexdigest()
    assert membership.rootserver_cpio_sha256 == hashlib.sha256(
        cpio_path.read_bytes()
    ).hexdigest()
    if not sealed:
        assert (
            identity.verify_unsealed_uimage_embeds_root(
                image_path, root_path, cpio_path
            )
            == build_marker()
        )


@pytest.mark.parametrize(
    "case",
    ["missing", "duplicate", "nonregular", "mismatch"],
)
def test_rootserver_member_must_be_unique_regular_and_exact(
    tmp_path: pathlib.Path,
    case: str,
) -> None:
    """CPIO membership cannot be replaced by names or approximate bytes."""

    image_path, root_path, cpio_path = write_packaging_fixture(
        tmp_path, sealed=False
    )
    root_data = root_path.read_bytes()
    if case == "missing":
        archive = make_newc([(b"kernel.elf", b"kernel", 0o100644)])
    elif case == "duplicate":
        archive = make_newc(
            [
                (b"rootserver", root_data, 0o100755),
                (b"rootserver", root_data, 0o100755),
            ]
        )
    elif case == "nonregular":
        archive = make_newc([(b"rootserver", root_data, 0o040755)])
    else:
        archive = make_newc([(b"rootserver", root_data + b"x", 0o100755)])
    cpio_path.write_bytes(archive)

    with pytest.raises(identity.ImageIdentityError):
        identity.verify_uimage_root_archive(image_path, root_path, cpio_path)


@pytest.mark.parametrize("case", ["loose-root", "duplicate-archive", "root-decoy"])
def test_wrapper_rejects_loose_decoy_or_duplicate_elf_bytes(
    tmp_path: pathlib.Path,
    case: str,
) -> None:
    """Only the root member inside the one exact embedded archive is accepted."""

    root_data = make_marker_elf()
    cpio_data = make_root_cpio(root_data)
    if case == "loose-root":
        payload = b"loader" + root_data + b"tail"
    elif case == "duplicate-archive":
        payload = b"loader" + cpio_data + b"gap" + cpio_data + b"tail"
    else:
        payload = b"loader" + root_data + b"decoy" + cpio_data + b"tail"
    image_path = tmp_path / "image"
    root_path = tmp_path / "root-task"
    cpio_path = tmp_path / "archive.cpio"
    image_path.write_bytes(make_uimage_from_payload(payload))
    root_path.write_bytes(root_data)
    cpio_path.write_bytes(cpio_data)

    with pytest.raises(identity.ImageIdentityError):
        identity.verify_uimage_root_archive(image_path, root_path, cpio_path)


def test_sealed_wrapper_rejects_a_different_self_consistent_archive(
    tmp_path: pathlib.Path,
) -> None:
    """A sealed image cannot be checked against a stale rootserver archive."""

    image_path, root_path, cpio_path = write_packaging_fixture(
        tmp_path, sealed=True
    )
    stale_cpio = tmp_path / "stale.cpio"
    stale_cpio.write_bytes(
        make_newc(
            [
                (b"kernel.elf", b"different-kernel", 0o100644),
                (b"rootserver", root_path.read_bytes(), 0o100755),
            ]
        )
    )

    with pytest.raises(identity.ImageIdentityError, match="archive"):
        identity.verify_uimage_root_archive(image_path, root_path, stale_cpio)
    assert cpio_path.read_bytes() != stale_cpio.read_bytes()


@pytest.mark.parametrize("command", ["seal", "verify"])
@pytest.mark.parametrize("alias_kind", ["equal", "resolved", "symlink", "hardlink"])
def test_metadata_alias_is_rejected_before_any_image_mutation(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
    command: str,
    alias_kind: str,
) -> None:
    """Every path and inode alias is refused before seal or metadata writes."""

    image_path = tmp_path / "image"
    image_path.write_bytes(make_uimage())
    original = image_path.read_bytes()
    if alias_kind == "equal":
        metadata_path = image_path
    elif alias_kind == "resolved":
        nested = tmp_path / "nested"
        nested.mkdir()
        metadata_path = nested / ".." / image_path.name
    elif alias_kind == "symlink":
        metadata_path = tmp_path / "metadata-link"
        metadata_path.symlink_to(image_path)
    else:
        metadata_path = tmp_path / "metadata-hardlink"
        metadata_path.hardlink_to(image_path)

    def unexpected_seal(_path: pathlib.Path) -> object:
        pytest.fail("aliased metadata must be rejected before sealing")

    monkeypatch.setattr(identity, "seal_image", unexpected_seal)
    result = identity.main(
        [command, "--image", str(image_path), "--metadata", str(metadata_path)]
    )

    assert result == 2
    assert "aliases the image" in capsys.readouterr().err
    assert image_path.read_bytes() == original
    assert identity.paths_alias(image_path, metadata_path)


def test_cli_publishes_v2_provenance_and_verifies_sidecar(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Metadata binds image, clean full commit, build ID, root ELF, and CPIO."""

    image_path, root_path, cpio_path = write_packaging_fixture(
        tmp_path, sealed=True
    )
    metadata_path = tmp_path / "pi4-image-identity.json"

    assert identity.main(
        publish_args(image_path, metadata_path, root_path, cpio_path)
    ) == 0
    stdout_record = json.loads(capsys.readouterr().out)
    metadata_record = json.loads(metadata_path.read_text(encoding="utf-8"))
    inspected = identity.inspect_image(image_path)

    assert stdout_record == metadata_record
    assert metadata_record["schema"] == identity.SCHEMA
    assert metadata_record["git_commit"] == FULL_COMMIT
    assert metadata_record["embedded_git_commit"] == EMBEDDED_COMMIT
    assert metadata_record["source_tree_clean"] is True
    assert metadata_record["build_timestamp"] == BUILD_TIMESTAMP
    assert metadata_record["image_id"] == inspected.image_id
    assert metadata_record["image_sha256"] == inspected.image_sha256
    assert metadata_record["build_id"] == identity.canonical_build_id(
        FULL_COMMIT, BUILD_TIMESTAMP, inspected.image_id
    )
    assert metadata_record["rootserver_sha256"] == hashlib.sha256(
        root_path.read_bytes()
    ).hexdigest()
    assert metadata_record["rootserver_cpio_sha256"] == hashlib.sha256(
        cpio_path.read_bytes()
    ).hexdigest()
    assert metadata_record["rootserver_member"] == "rootserver"
    assert identity.read_metadata(metadata_path) == identity.ImageIdentity(
        **metadata_record
    )

    assert identity.main(
        [
            "verify-metadata",
            "--image",
            str(image_path),
            "--metadata",
            str(metadata_path),
            "--expected-git-commit",
            FULL_COMMIT,
            "--expected-build-id",
            metadata_record["build_id"],
            "--expected-root-elf",
            str(root_path),
            "--expected-root-cpio",
            str(cpio_path),
        ]
    ) == 0


@pytest.mark.parametrize(
    ("marker", "git_commit", "extra_args", "message"),
    [
        (build_marker(dirty=True), FULL_COMMIT, ["--source-tree-clean"], "-dirty"),
        (build_marker(), OTHER_COMMIT, ["--source-tree-clean"], "prefix"),
        (build_marker(), "abc123", ["--source-tree-clean"], "full lowercase"),
        (build_marker(), FULL_COMMIT, [], "source-tree-clean"),
    ],
)
def test_metadata_publication_rejects_untrusted_git_provenance(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    marker: str,
    git_commit: str,
    extra_args: list[str],
    message: str,
) -> None:
    """Dirty, partial, or mismatched Git provenance cannot publish v2 metadata."""

    image_path, root_path, cpio_path = write_packaging_fixture(
        tmp_path, sealed=True, marker=marker
    )
    metadata_path = tmp_path / "identity.json"
    argv = [
        "verify",
        "--image",
        str(image_path),
        "--metadata",
        str(metadata_path),
        "--git-commit",
        git_commit,
        *extra_args,
        "--expected-root-elf",
        str(root_path),
        "--expected-root-cpio",
        str(cpio_path),
    ]

    assert identity.main(argv) == 2
    assert message in capsys.readouterr().err
    assert not metadata_path.exists()


@pytest.mark.parametrize("mutation", ["truncate", "same-bytes-new-inode"])
def test_post_metadata_reinspection_detects_image_changes(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
    mutation: str,
) -> None:
    """Metadata publication cannot race an image mutation or path replacement."""

    image_path, root_path, cpio_path = write_packaging_fixture(
        tmp_path, sealed=True
    )
    metadata_path = tmp_path / "identity.json"
    original_write = identity.write_metadata

    def mutating_write(
        path: pathlib.Path, record: identity.ImageIdentity
    ) -> None:
        original_write(path, record)
        if mutation == "truncate":
            image_path.write_bytes(image_path.read_bytes()[:-1])
        else:
            replacement = tmp_path / "replacement-image"
            replacement.write_bytes(image_path.read_bytes())
            os.replace(replacement, image_path)

    monkeypatch.setattr(identity, "write_metadata", mutating_write)

    assert identity.main(
        publish_args(image_path, metadata_path, root_path, cpio_path)
    ) == 2
    assert "image" in capsys.readouterr().err


def publish_valid_metadata(
    tmp_path: pathlib.Path,
) -> tuple[pathlib.Path, pathlib.Path, pathlib.Path, pathlib.Path]:
    """Publish and return one complete valid v2 fixture."""

    image_path, root_path, cpio_path = write_packaging_fixture(
        tmp_path, sealed=True
    )
    metadata_path = tmp_path / "identity.json"
    assert identity.main(
        publish_args(image_path, metadata_path, root_path, cpio_path)
    ) == 0
    return image_path, root_path, cpio_path, metadata_path


@pytest.mark.parametrize(
    "case",
    [
        "schema",
        "missing-field",
        "extra-field",
        "dirty-state",
        "git-commit",
        "embedded-commit",
        "build-id",
        "marker-sha",
        "marker-image-id",
        "rootserver-sha",
        "cpio-sha",
        "rootserver-member",
        "crc-format",
        "non-ascii-marker",
    ],
)
def test_metadata_reader_rejects_tampered_schema_and_provenance(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    case: str,
) -> None:
    """Every v2 schema and provenance field is validated fail-closed."""

    _image, _root, _cpio, metadata_path = publish_valid_metadata(tmp_path)
    capsys.readouterr()
    record = json.loads(metadata_path.read_text(encoding="utf-8"))
    if case == "schema":
        record["schema"] = "cohesix-pi4-image-identity/v1"
    elif case == "missing-field":
        del record["build_id"]
    elif case == "extra-field":
        record["unexpected"] = True
    elif case == "dirty-state":
        record["source_tree_clean"] = False
    elif case == "git-commit":
        record["git_commit"] = OTHER_COMMIT
    elif case == "embedded-commit":
        record["embedded_git_commit"] = "f" * 12
    elif case == "build-id":
        record["build_id"] = "f" * 64
    elif case == "marker-sha":
        record["build_marker_sha256"] = "f" * 64
    elif case == "marker-image-id":
        record["image_id"] = "f" * 64
    elif case == "rootserver-sha":
        record["rootserver_sha256"] = "not-a-hash"
    elif case == "cpio-sha":
        record["rootserver_cpio_sha256"] = "not-a-hash"
    elif case == "rootserver-member":
        record["rootserver_member"] = "decoy"
    elif case == "crc-format":
        record["uimage_header_crc32"] = "not-crc"
    else:
        record["build_marker"] = "not-ascii-☃"
    metadata_path.write_text(json.dumps(record), encoding="utf-8")

    with pytest.raises(identity.ImageIdentityError):
        identity.read_metadata(metadata_path)


@pytest.mark.parametrize("expected", ["commit", "build-id", "image-content"])
def test_verify_metadata_rejects_external_expectation_or_image_mismatch(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    expected: str,
) -> None:
    """A stale coherent sidecar cannot cross the external commit/build boundary."""

    image_path, root_path, cpio_path, metadata_path = publish_valid_metadata(tmp_path)
    published = identity.read_metadata(metadata_path)
    capsys.readouterr()
    expected_commit = OTHER_COMMIT if expected == "commit" else FULL_COMMIT
    expected_build_id = "f" * 64 if expected == "build-id" else published.build_id
    if expected == "image-content":
        stale_image, _stale_root, _stale_cpio = write_packaging_fixture(
            tmp_path / "stale", sealed=True, payload_prefix=b"different-loader"
        )
        image_path = stale_image

    result = identity.main(
        [
            "verify-metadata",
            "--image",
            str(image_path),
            "--metadata",
            str(metadata_path),
            "--expected-git-commit",
            expected_commit,
            "--expected-build-id",
            expected_build_id or "",
            "--expected-root-elf",
            str(root_path),
            "--expected-root-cpio",
            str(cpio_path),
        ]
    )

    assert result == 2
    assert "metadata" in capsys.readouterr().err


def test_root_membership_reads_each_input_from_one_snapshot(
    tmp_path: pathlib.Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """One membership decision never reopens an evidence path mid-check."""

    image_path, root_path, cpio_path = write_packaging_fixture(
        tmp_path, sealed=False
    )
    original_read = identity.read_stable_regular_file
    reads: dict[pathlib.Path, int] = {}

    def counted_read(path: pathlib.Path) -> identity.StableFileSnapshot:
        reads[path] = reads.get(path, 0) + 1
        return original_read(path)

    monkeypatch.setattr(identity, "read_stable_regular_file", counted_read)
    identity.verify_uimage_root_archive(image_path, root_path, cpio_path)

    assert reads == {root_path: 1, cpio_path: 1, image_path: 1}


def test_canonical_build_id_is_domain_separated_and_input_sensitive() -> None:
    """The public helper derives one stable ID from all provenance dimensions."""

    sealed, image_id, _marker = identity.seal_image_bytes(make_uimage())
    first = identity.canonical_build_id(FULL_COMMIT, BUILD_TIMESTAMP, image_id)
    assert len(first) == 64
    assert first == identity.canonical_build_id(
        FULL_COMMIT, BUILD_TIMESTAMP, identity.inspect_image_bytes(sealed).image_id
    )
    assert first != identity.canonical_build_id(OTHER_COMMIT, BUILD_TIMESTAMP, image_id)
    assert first != identity.canonical_build_id(
        FULL_COMMIT, "2026-07-16T00:00:01Z", image_id
    )


def test_cli_requires_complete_root_pair_and_exclusive_unsealed_modes(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Partial or conflicting root verification modes never degrade silently."""

    image_path, root_path, cpio_path = write_packaging_fixture(
        tmp_path, sealed=False
    )
    assert identity.main(
        [
            "verify-unsealed-marker",
            "--artifact",
            str(image_path),
            "--expected-root-elf",
            str(root_path),
        ]
    ) == 2
    assert "supplied together" in capsys.readouterr().err

    assert identity.main(
        [
            "verify-unsealed-marker",
            "--artifact",
            str(image_path),
            "--require-elf-load-section",
            "--expected-root-elf",
            str(root_path),
            "--expected-root-cpio",
            str(cpio_path),
        ]
    ) == 2
    assert "exclusive" in capsys.readouterr().err


def test_verify_unsealed_cli_emits_skip_build_provenance_contract(
    tmp_path: pathlib.Path,
) -> None:
    """The real CLI emits every marker field consumed by skip-build adoption."""

    image_path, root_path, cpio_path = write_packaging_fixture(
        tmp_path, sealed=False
    )
    completed = subprocess.run(
        [
            sys.executable,
            str(MODULE_PATH),
            "verify-unsealed-marker",
            "--artifact",
            str(image_path),
            "--expected-root-elf",
            str(root_path),
            "--expected-root-cpio",
            str(cpio_path),
        ],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    record = json.loads(completed.stdout)

    assert record["embedded_git_commit"] == EMBEDDED_COMMIT
    assert record["build_timestamp"] == BUILD_TIMESTAMP
    assert record["marker_dirty"] is False
    assert record["marker_image_id"] == identity.UNSEALED_IMAGE_ID


def test_all_modified_files_retain_required_header() -> None:
    """The implementation and test file retain required 2026 provenance."""

    for path in (MODULE_PATH, pathlib.Path(__file__)):
        prefix = path.read_text(encoding="utf-8").splitlines()[:5]
        assert any("Author: Lukas Bower" in line for line in prefix)
        assert any("Purpose:" in line for line in prefix)
        assert any("Copyright 2026 Lukas Bower" in line for line in prefix)
