# Author: Lukas Bower
# Purpose: Verify immutable Pi 4 seL4 artifact composition and image wrapping.
# Copyright 2026 Lukas Bower

"""Focused tests for scripts/pi4_prebuilt_composition.py."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import re
import sys

import pytest


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "pi4_prebuilt_composition.py"
LIB_DIR = REPO_ROOT / "scripts" / "lib"
sys.path.insert(0, str(LIB_DIR))

from strip_elfloader_modules import _build_cpio, _parse_cpio  # noqa: E402

SPEC = importlib.util.spec_from_file_location(
    "pi4_prebuilt_composition",
    SCRIPT_PATH,
)
assert SPEC is not None and SPEC.loader is not None
composition = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(composition)


def _cpio_entry(name: str, data: bytes) -> dict[str, object]:
    """Build one minimal newc entry for archive-rewrite tests."""

    fields = [
        1,
        0o100755,
        0,
        0,
        1,
        0,
        len(data),
        0,
        0,
        0,
        0,
        len(name.encode("utf-8")) + 1,
        0,
    ]
    return {"name": name, "fields": fields, "data": bytearray(data)}


def _sample_archive(rootserver: bytes) -> bytes:
    """Return one block-aligned kernel/rootserver newc archive."""

    archive = _build_cpio(
        [
            _cpio_entry("kernel.elf", b"kernel"),
            _cpio_entry("rootserver", rootserver),
            _cpio_entry("TRAILER!!!", b""),
        ]
    )
    return archive + b"\x00" * (-len(archive) % composition.CPIO_BLOCK_SIZE)


def _valid_link_edge(objects: str) -> str:
    """Build a supported synthetic elfloader Ninja link edge."""

    return (
        "build elfloader/elfloader: "
        "C_EXECUTABLE_LINKER__elfloader_Debug "
        f"elfloader/archive.o {objects} | "
        "apps/sel4test-driver/util_libs/libcpio/libcpio.a "
        "elfloader/linker.lds_pp || elfloader/elfloader_linker\n"
    )


def test_rootserver_archive_can_grow_and_preserves_exact_payload() -> None:
    """Artifact relinking must allow a rootserver larger than baseline capacity."""

    baseline = _sample_archive(b"old")
    rootserver = bytes(range(256)) * 9

    rebuilt, evidence = composition.build_rootserver_archive(
        baseline,
        rootserver,
    )

    entries = _parse_cpio(rebuilt)
    embedded = [
        bytes(entry["data"])
        for entry in entries
        if entry["name"] == "rootserver"
    ]
    assert embedded == [rootserver]
    assert evidence["baseline_rootserver_size"] == 3
    assert evidence["rootserver_size"] == len(rootserver)
    assert evidence["archive_size"] > evidence["baseline_archive_size"]
    assert len(rebuilt) % composition.CPIO_BLOCK_SIZE == 0


def test_parse_elfloader_object_order_preserves_exact_order(
    tmp_path: Path,
) -> None:
    """The relink must follow the canonical Ninja object order exactly."""

    graph = tmp_path / "build.ninja"
    graph.write_text(
        _valid_link_edge(
            "elfloader/CMakeFiles/elfloader.dir/a.c.obj "
            "elfloader/CMakeFiles/elfloader.dir/b.S.obj"
        ),
        encoding="utf-8",
    )

    objects = composition.parse_elfloader_object_order(graph)

    assert objects == [
        Path("elfloader/CMakeFiles/elfloader.dir/a.c.obj"),
        Path("elfloader/CMakeFiles/elfloader.dir/b.S.obj"),
    ]


@pytest.mark.parametrize(
    "unsafe",
    (
        "../escape.c.obj",
        "/absolute.c.obj",
        "elfloader/$variable.c.obj",
        r"elfloader\windows.c.obj",
        "elfloader/colon:c.obj",
    ),
)
def test_parse_elfloader_object_order_rejects_unsafe_paths(
    tmp_path: Path,
    unsafe: str,
) -> None:
    """No build-graph path may escape or be reinterpreted by another parser."""

    graph = tmp_path / "build.ninja"
    graph.write_text(_valid_link_edge(unsafe), encoding="utf-8")

    with pytest.raises(composition.CompositionError, match="unsafe"):
        composition.parse_elfloader_object_order(graph)


def test_legacy_uimage_header_and_crcs_round_trip() -> None:
    """The Python wrapper must produce a valid exact 64-byte legacy header."""

    payload = b"Cohesix seL4 payload" * 97
    timestamp = 1_785_000_123

    image = composition.build_uimage(
        payload,
        timestamp=timestamp,
        name="Cohesix",
    )
    metadata, observed_payload = composition.parse_uimage(image)

    assert len(image) == composition.UIMAGE_HEADER_SIZE + len(payload)
    assert observed_payload == payload
    assert metadata["timestamp"] == timestamp
    assert metadata["load_address"] == "0x10000000"
    assert metadata["entry_point"] == "0x10000000"
    assert metadata["name"] == "Cohesix"

    corrupted = bytearray(image)
    corrupted[-1] ^= 0xFF
    with pytest.raises(composition.CompositionError, match="data CRC mismatch"):
        composition.parse_uimage(bytes(corrupted))


def test_legacy_uimage_rejects_empty_or_unrepresentable_payloads() -> None:
    """Legacy image size bounds must fail explicitly before struct packing."""

    with pytest.raises(composition.CompositionError, match="payload size"):
        composition.build_uimage(b"", timestamp=0)
    with pytest.raises(composition.CompositionError, match="payload size"):
        composition._validate_uimage_payload_size(0x1_0000_0000)


DEFAULT_TOOLS_PRESENT = all(
    Path(f"{composition.DEFAULT_BINUTILS_PREFIX}{tool}").is_file()
    for tool in composition.REQUIRED_BINUTILS
)
CURRENT_MCS_CONTRACT_VALUES_SHA256 = (
    "195a0086266d46973acf4f95d9d61fe3ef01afd2326bd2bebe83c725c91e83b0"
)
CLASSIC_ORACLE_CONTRACT_VALUES_SHA256 = (
    "62b2f3f34fc1531c556d85f0bd3ae6459f2b02a3f9ac371f770d174e3d0afa92"
)


def _tracked_build_contract_values_sha256(build_dir: Path) -> str:
    """Return the source-contract identity recorded by a tracked build."""

    stamp = json.loads(
        (build_dir / composition.PROFILE_STAMP).read_text(encoding="utf-8")
    )
    identity = stamp.get("contract_values_sha256")
    assert isinstance(identity, str)
    assert re.fullmatch(r"[0-9a-f]{64}", identity)
    return identity


def test_tracked_repository_matches_current_mcs_contract() -> None:
    """The controlled tracked refresh must match the current MCS contract."""

    build_dir = REPO_ROOT / "seL4" / "build_UBOOT"
    evidence = composition.validate_repo_build(build_dir)

    assert evidence["profile"] == composition.PROFILE_NAME
    assert (
        _tracked_build_contract_values_sha256(build_dir)
        == CURRENT_MCS_CONTRACT_VALUES_SHA256
    )
    assert re.fullmatch(r"[0-9a-f]{40}", evidence["git_tree"])


@pytest.mark.skipif(
    not DEFAULT_TOOLS_PRESENT,
    reason="canonical macOS AArch64 GNU binutils family is unavailable",
)
def test_tracked_repository_publishes_deterministically_after_mcs_refresh(
    tmp_path: Path,
) -> None:
    """Fixed current inputs must publish one deterministic artifact identity."""

    build_dir = REPO_ROOT / "seL4" / "build_UBOOT"
    rootserver = (
        build_dir / "apps" / "sel4test-driver" / "sel4test-driver"
    )
    output_dirs = (tmp_path / "assembly-a", tmp_path / "assembly-b")
    provenances = [
        composition.compose(
            build_dir=build_dir,
            rootserver=rootserver,
            output_dir=output_dir,
            timestamp=1_784_801_980,
        )
        for output_dir in output_dirs
    ]

    for output_dir, provenance in zip(output_dirs, provenances, strict=True):
        assert provenance["status"] == "complete"
        assert (
            provenance["repository_build"]["profile"]
            == composition.PROFILE_NAME
        )
        assert provenance["baseline_oracle"]["passed"] is True
        assert (output_dir / composition.OUTPUT_PROVENANCE).is_file()
    artifact_identities = [
        {
            name: (record["size"], record["sha256"])
            for name, record in provenance["artifacts"].items()
        }
        for provenance in provenances
    ]
    assert artifact_identities[0] == artifact_identities[1]
    oracle_a = provenances[0]["baseline_oracle"]
    oracle_b = provenances[1]["baseline_oracle"]
    for field in (
        "tracked_image",
        "tracked_elfloader",
        "tracked_archive",
        "relinked_elfloader",
        "relinked_payload",
    ):
        assert (
            oracle_a[field]["size"],
            oracle_a[field]["sha256"],
        ) == (
            oracle_b[field]["size"],
            oracle_b[field]["sha256"],
        )
    for field in (
        "passed",
        "payload_sha256",
        "image_metadata",
        "elf_layout",
    ):
        assert oracle_a[field] == oracle_b[field]


@pytest.mark.skipif(
    not DEFAULT_TOOLS_PRESENT,
    reason="canonical macOS AArch64 GNU binutils family is unavailable",
)
def test_tracked_classic_reference_remains_byte_exact_toolchain_oracle(
    tmp_path: Path,
) -> None:
    """Retain one source-bound classic oracle without claiming MCS/Pi proof."""

    build_dir = REPO_ROOT / "seL4" / "build_UBOOT"
    if (
        _tracked_build_contract_values_sha256(build_dir)
        != CLASSIC_ORACLE_CONTRACT_VALUES_SHA256
    ):
        pytest.skip(
            "historical byte oracle requires its exact classic build identity"
        )
    tools = composition.resolve_binutils()
    objects = composition.parse_elfloader_object_order(
        build_dir / "build.ninja"
    )
    scratch = tmp_path / "oracle"
    scratch.mkdir()
    oracle = composition.run_baseline_oracle(
        build_dir,
        objects,
        tools,
        scratch,
    )

    assert oracle["passed"] is True
    assert oracle["payload_sha256"] == (
        "4dd17b4d3bdec29c456b19e1ef6f32340cecc17930b3eca03d6a2037b226b8fc"
    )
    assert oracle["elf_layout"]["entry"] == composition.UIMAGE_LOAD_ADDRESS
    assert (
        oracle["elf_layout"]["file_size"]
        == oracle["image_metadata"]["data_size"]
    )
