# Author: Lukas Bower
# Purpose: Test fail-closed Pi 4 WiFi cold and warm boot repeatability scoring.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4_wifi_repeatability.py."""

from __future__ import annotations

import importlib.util
import hashlib
import json
import os
import pathlib
import struct
import sys
import types
import zlib

import pytest


MODULE_PATH = (
    pathlib.Path(__file__).resolve().parents[1]
    / "scripts"
    / "pi4_wifi_repeatability.py"
)
SPEC = importlib.util.spec_from_file_location("pi4_wifi_repeatability", MODULE_PATH)
repeatability = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = repeatability
SPEC.loader.exec_module(repeatability)


def make_sealed_uimage(
    *,
    git_hash: str = "abc1234",
    payload_prefix: bytes = b"cohesix-image-prefix",
) -> tuple[bytes, str]:
    """Return one structurally valid sealed image and its runtime marker."""

    marker = (
        f"[BUILD] {git_hash} 2026-07-16T00:00:00Z "
        f"image-id={repeatability.image_identity.UNSEALED_IMAGE_ID} "
        "features=[kernel:1 bootstrap-trace:1 serial-console:1 net:1 "
        "net-console:1 qemu-driver-task-smoke:0]"
    )
    payload = payload_prefix + b"\0" + marker.encode("ascii") + b"\0suffix"
    image = bytearray(repeatability.image_identity.UIMAGE_HEADER_BYTES + len(payload))
    struct.pack_into(">I", image, 0, repeatability.image_identity.UIMAGE_MAGIC)
    struct.pack_into(">I", image, 12, len(payload))
    struct.pack_into(
        ">I", image, 16, repeatability.image_identity.EXPECTED_UIMAGE_LOAD_ADDRESS
    )
    struct.pack_into(
        ">I", image, 20, repeatability.image_identity.EXPECTED_UIMAGE_ENTRY_POINT
    )
    image[28:32] = bytes(
        (
            repeatability.image_identity.UIMAGE_OS_LINUX,
            repeatability.image_identity.UIMAGE_ARCH_ARM64,
            repeatability.image_identity.UIMAGE_TYPE_KERNEL,
            repeatability.image_identity.UIMAGE_COMPRESSION_NONE,
        )
    )
    image[repeatability.image_identity.UIMAGE_HEADER_BYTES :] = payload
    struct.pack_into(
        ">I",
        image,
        24,
        zlib.crc32(image[repeatability.image_identity.UIMAGE_HEADER_BYTES :])
        & 0xFFFF_FFFF,
    )
    struct.pack_into(
        ">I", image, 4, zlib.crc32(image[:64]) & 0xFFFF_FFFF
    )
    sealed, _image_id, sealed_marker = (
        repeatability.image_identity.seal_image_bytes(bytes(image))
    )
    return sealed, sealed_marker


def append_to_uimage(image: bytes, extra: bytes) -> bytes:
    """Append test payload bytes while keeping the U-Boot envelope valid."""

    extended = bytearray(image + extra)
    struct.pack_into(">I", extended, 12, len(extended) - 64)
    struct.pack_into(">I", extended, 24, zlib.crc32(extended[64:]) & 0xFFFF_FFFF)
    extended[4:8] = b"\0" * 4
    struct.pack_into(">I", extended, 4, zlib.crc32(extended[:64]) & 0xFFFF_FFFF)
    return bytes(extended)


VALID_READBACK_BYTES, VALID_BUILD_MARKER = make_sealed_uimage()
STALE_READBACK_BYTES, STALE_BUILD_MARKER = make_sealed_uimage(
    git_hash="deadbeef", payload_prefix=b"stale-image-prefix"
)
VALID_SHA256 = hashlib.sha256(VALID_READBACK_BYTES).hexdigest()
VALID_GIT_COMMIT = "abc1234" + ("0" * 33)
STALE_GIT_COMMIT = "deadbeef" + ("0" * 32)
BUILD_TIMESTAMP = "2026-07-16T00:00:00Z"


def marker_image_id(marker: str) -> str:
    """Extract the sealed image ID from one canonical marker fixture."""

    match = repeatability.IMAGE_ID_RE.search(marker)
    assert match is not None
    return match.group("image_id")


def canonical_build_id(git_commit: str, timestamp: str, image_id: str) -> str:
    """Derive the capture-binding build ID used by identity schema v2."""

    return repeatability.image_identity.canonical_build_id(
        git_commit, timestamp, image_id
    )


VALID_BUILD_ID = canonical_build_id(
    VALID_GIT_COMMIT, BUILD_TIMESTAMP, marker_image_id(VALID_BUILD_MARKER)
)


def make_classic_pcap(payload: bytes, capture_epoch: int) -> bytes:
    """Create one valid little-endian microsecond classic pcap fixture."""

    return b"".join(
        (
            b"\xd4\xc3\xb2\xa1",
            struct.pack("<HHiIII", 2, 4, 0, 0, 65_535, 1),
            struct.pack("<IIII", capture_epoch, 0, len(payload), len(payload)),
            payload,
        )
    )


@pytest.fixture(autouse=True)
def synthetic_boot_summaries(monkeypatch: pytest.MonkeyPatch) -> None:
    """Map compact synthetic log tokens to normalizer-shaped boot summaries."""

    def blockers(gates: dict[str, object]) -> list[str]:
        return list(gates.get("_blockers", []))  # type: ignore[arg-type]

    def summarize(lines: list[str]) -> list[dict[str, object]]:
        summaries: list[dict[str, object]] = []
        boot_starts = [
            index for index, line in enumerate(lines) if line.startswith("BOOT ")
        ]
        for boot_index, line_start in enumerate(boot_starts, start=1):
            line_end = (
                boot_starts[boot_index]
                if boot_index < len(boot_starts)
                else len(lines)
            )
            token = lines[line_start].removeprefix("BOOT ")
            if token == "skip":
                summaries.append(
                    {
                        "blockers": [],
                        "boot": boot_index,
                        "gates": {},
                        "kind": "uboot-menu-save-reset",
                        "line_end": line_end,
                        "line_start": line_start + 1,
                        "score": "skip",
                    }
                )
                continue
            net_active = "wired" if token == "wired" else "wifi"
            boot_blockers = ["panic"] if token == "fail" else []
            supervisor_seen = "no" if token == "no-supervisor" else "yes"
            supervisor_ready = "no" if token == "no-supervisor" else "yes"
            summaries.append(
                {
                    "blockers": boot_blockers,
                    "boot": boot_index,
                    "gates": {
                        "CYW43_BOOTSTRAP_SUPERVISOR_READY": supervisor_ready,
                        "CYW43_BOOTSTRAP_SUPERVISOR_SEEN": supervisor_seen,
                        "NET_ACTIVE": net_active,
                        "_blockers": boot_blockers,
                    },
                    "kind": "cohesix-boot",
                    "line_end": line_end,
                    "line_start": line_start + 1,
                    "score": "fail" if token == "fail" else "pass",
                }
            )
        return summaries

    monkeypatch.setattr(
        repeatability.trace_normalizer,
        "boot_evidence_blockers",
        blockers,
    )
    monkeypatch.setattr(
        repeatability.trace_normalizer,
        "summarize_boot_slices",
        summarize,
    )


def write_log(
    path: pathlib.Path,
    tokens: list[str],
    *,
    markers: list[str | None] | None = None,
) -> pathlib.Path:
    """Write one compact synthetic serial-log fixture."""

    selected_markers = (
        [VALID_BUILD_MARKER] * len(tokens) if markers is None else markers
    )
    assert len(selected_markers) == len(tokens)
    lines: list[str] = [f"CAPTURE {path.name}"]
    for index, (token, marker) in enumerate(
        zip(tokens, selected_markers, strict=True), start=1
    ):
        lines.append(f"BOOT {token}")
        lines.append(f"RUN {path.name}:{index}")
        if marker is not None:
            lines.append(marker)
    content = "\n".join(lines)
    if lines:
        content += "\n"
    path.write_text(content, encoding="utf-8")
    return path


def write_evidence_sidecars(
    directory: pathlib.Path,
    cold_logs: list[pathlib.Path],
    warm_logs: list[pathlib.Path],
    *,
    image_sha256: str,
    build_marker: str,
    expected_git_commit: str | None = None,
    expected_build_id: str | None = None,
    staged_image: pathlib.Path | None = None,
) -> tuple[pathlib.Path, pathlib.Path, str, str]:
    """Create exact identity-v2 and capture-manifest-v2 fixtures."""

    embedded_commit = build_marker.split()[1]
    if expected_git_commit is None:
        expected_git_commit = (
            STALE_GIT_COMMIT
            if embedded_commit.startswith("deadbeef")
            else VALID_GIT_COMMIT
        )
    image_id = marker_image_id(build_marker)
    if expected_build_id is None:
        expected_build_id = canonical_build_id(
            expected_git_commit, BUILD_TIMESTAMP, image_id
        )
    staged_image = staged_image or directory / "staged.img"
    fixture_bytes = (
        STALE_READBACK_BYTES
        if image_sha256.lower() == hashlib.sha256(STALE_READBACK_BYTES).hexdigest()
        else VALID_READBACK_BYTES
    )
    image_record = repeatability.image_identity.inspect_image_bytes(
        fixture_bytes,
        path=staged_image,
    )
    try:
        staged_stat = staged_image.stat()
    except OSError:
        device = inode = mtime_ns = ctime_ns = 0
    else:
        device = staged_stat.st_dev
        inode = staged_stat.st_ino
        mtime_ns = staged_stat.st_mtime_ns
        ctime_ns = staged_stat.st_ctime_ns
    identity_path = directory / "pi4-image-identity.json"
    identity_path.write_text(
        json.dumps(
            {
                "schema": "cohesix-pi4-image-identity/v2",
                "path": str(staged_image),
                "git_commit": expected_git_commit,
                "embedded_git_commit": embedded_commit,
                "source_tree_clean": True,
                "build_timestamp": BUILD_TIMESTAMP,
                "build_id": expected_build_id,
                "image_sha256": image_sha256.lower(),
                "image_id": image_id,
                "build_marker": build_marker,
                "build_marker_sha256": hashlib.sha256(
                    build_marker.encode("ascii")
                ).hexdigest(),
                "size_bytes": len(fixture_bytes),
                "device": device,
                "inode": inode,
                "mtime_ns": mtime_ns,
                "ctime_ns": ctime_ns,
                "uimage_header_crc32": image_record.uimage_header_crc32,
                "uimage_data_crc32": image_record.uimage_data_crc32,
                "rootserver_sha256": "1" * 64,
                "rootserver_cpio_sha256": "2" * 64,
                "rootserver_member": "rootserver",
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    runs: list[dict[str, object]] = []
    for boot_class, logs in (("cold", cold_logs), ("warm", warm_logs)):
        for log_path in logs:
            log = repeatability.assess_log(str(log_path), build_marker)
            boots = log["boots"]
            assert isinstance(boots, list)
            for boot in boots:
                assert isinstance(boot, dict)
                if boot["source_score"] == "skip":
                    continue
                slice_index = int(boot["serial_slice_index"])
                pcap_path = directory / (
                    f"{boot_class}-{log_path.name}-{slice_index}.pcap"
                )
                pcap_payload = (
                    f"PCAP {boot_class} {log_path.name} {slice_index}\n"
                ).encode("ascii")
                capture_epoch = 2_000_000_000 + len(runs)
                pcap_bytes = make_classic_pcap(pcap_payload, capture_epoch)
                pcap_path.write_bytes(pcap_bytes)
                runs.append(
                    {
                        "run_id": f"{boot_class}-{log_path.stem}-{slice_index}",
                        "boot_class": boot_class,
                        "serial_path": str(log["resolved_path"]),
                        "serial_sha256": log["sha256"],
                        "serial_slice_index": slice_index,
                        "serial_slice_sha256": boot["raw_slice_sha256"],
                        "pcap_path": str(pcap_path),
                        "pcap_sha256": hashlib.sha256(pcap_bytes).hexdigest(),
                        "image_id": image_id,
                        "git_commit": expected_git_commit,
                        "build_id": expected_build_id,
                        "capture_epoch": capture_epoch,
                    }
                )
    capture_path = directory / "pi4-wifi-capture-manifest.json"
    identity_metadata_sha256 = hashlib.sha256(identity_path.read_bytes()).hexdigest()
    capture_path.write_text(
        json.dumps(
            {
                "schema": repeatability.CAPTURE_MANIFEST_SCHEMA,
                "image_identity_metadata_sha256": identity_metadata_sha256,
                "runs": runs,
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    return identity_path, capture_path, expected_git_commit, expected_build_id


def read_json_object(path: pathlib.Path) -> dict[str, object]:
    """Read one JSON object fixture with an asserted mapping root."""

    value = json.loads(path.read_text(encoding="utf-8"))
    assert isinstance(value, dict)
    return value


def write_json_object(path: pathlib.Path, value: dict[str, object]) -> None:
    """Write one deterministic JSON object fixture."""

    path.write_text(json.dumps(value, sort_keys=True) + "\n", encoding="utf-8")


def run_report(
    capsys: pytest.CaptureFixture[str],
    cold_logs: list[pathlib.Path],
    warm_logs: list[pathlib.Path],
    *,
    image_sha256: str = VALID_SHA256,
    build_marker: str = VALID_BUILD_MARKER,
    staged_image: pathlib.Path | None = None,
    readback_image: pathlib.Path | None = None,
    image_identity_metadata: pathlib.Path | None = None,
    capture_manifest: pathlib.Path | None = None,
    expected_git_commit: str | None = None,
    expected_build_id: str | None = None,
    expected_image_identity_sha256: str | None = None,
    output: pathlib.Path | None = None,
) -> tuple[int, dict[str, object], str]:
    """Run the CLI entry point and parse its deterministic JSON output."""

    if readback_image is None:
        evidence_logs = [*cold_logs, *warm_logs]
        assert evidence_logs
        readback_image = evidence_logs[0].parent / "readback.img"
        readback_image.write_bytes(VALID_READBACK_BYTES)
    if staged_image is None:
        staged_image = readback_image.parent / "staged.img"
        try:
            staged_bytes = readback_image.read_bytes()
        except OSError:
            staged_bytes = VALID_READBACK_BYTES
        staged_image.write_bytes(staged_bytes)
    if image_identity_metadata is None or capture_manifest is None:
        (
            generated_identity,
            generated_capture,
            generated_git_commit,
            generated_build_id,
        ) = write_evidence_sidecars(
            readback_image.parent,
            cold_logs,
            warm_logs,
            image_sha256=image_sha256,
            build_marker=build_marker,
            expected_git_commit=expected_git_commit,
            expected_build_id=expected_build_id,
            staged_image=staged_image,
        )
        image_identity_metadata = image_identity_metadata or generated_identity
        capture_manifest = capture_manifest or generated_capture
        expected_git_commit = expected_git_commit or generated_git_commit
        expected_build_id = expected_build_id or generated_build_id
    assert expected_git_commit is not None
    assert expected_build_id is not None
    if expected_image_identity_sha256 is None:
        expected_image_identity_sha256 = hashlib.sha256(
            image_identity_metadata.read_bytes()
        ).hexdigest()

    argv: list[str] = []
    for path in cold_logs:
        argv.extend(("--cold-log", str(path)))
    for path in warm_logs:
        argv.extend(("--warm-log", str(path)))
    argv.extend(("--image-sha256", image_sha256))
    argv.extend(("--staged-image", str(staged_image)))
    argv.extend(("--readback-image", str(readback_image)))
    argv.extend(("--image-identity-metadata", str(image_identity_metadata)))
    argv.extend(("--capture-manifest", str(capture_manifest)))
    argv.extend(("--expected-git-commit", expected_git_commit))
    argv.extend(("--expected-build-id", expected_build_id))
    argv.extend(
        (
            "--expected-image-identity-sha256",
            expected_image_identity_sha256,
        )
    )
    argv.extend(("--build-marker", build_marker))
    if output is not None:
        argv.extend(("--output", str(output)))

    result = repeatability.main(argv)
    rendered = capsys.readouterr().out
    return result, json.loads(rendered), rendered


def repeatability_argv(
    *,
    cold: pathlib.Path,
    warm: pathlib.Path,
    staged: pathlib.Path,
    readback: pathlib.Path,
    output: pathlib.Path,
) -> list[str]:
    """Return a complete repeatability CLI argument vector for alias tests."""

    identity, capture, git_commit, build_id = write_evidence_sidecars(
        output.parent,
        [cold],
        [warm],
        image_sha256=VALID_SHA256,
        build_marker=VALID_BUILD_MARKER,
        staged_image=staged,
    )
    identity_sha256 = hashlib.sha256(identity.read_bytes()).hexdigest()

    return [
        "--cold-log",
        str(cold),
        "--warm-log",
        str(warm),
        "--image-sha256",
        VALID_SHA256,
        "--staged-image",
        str(staged),
        "--readback-image",
        str(readback),
        "--image-identity-metadata",
        str(identity),
        "--capture-manifest",
        str(capture),
        "--expected-git-commit",
        git_commit,
        "--expected-build-id",
        build_id,
        "--expected-image-identity-sha256",
        identity_sha256,
        "--build-marker",
        VALID_BUILD_MARKER,
        "--output",
        str(output),
    ]


def test_ten_cold_and_ten_warm_pass_across_repeatable_logs(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Ten blocker-free WiFi slices in each class pass for one image."""

    cold_logs = [
        write_log(tmp_path / "cold-a.log", ["pass"] * 4),
        write_log(tmp_path / "cold-b.log", ["pass"] * 6),
    ]
    warm_logs = [
        write_log(tmp_path / "warm-a.log", ["pass"] * 3),
        write_log(tmp_path / "warm-b.log", ["pass"] * 7),
    ]
    output = tmp_path / "repeatability.json"

    result, report, rendered = run_report(
        capsys,
        cold_logs,
        warm_logs,
        output=output,
    )

    assert result == 0
    assert report["result"] == "PASS"
    assert report["image"] == {
        "identity_role": "staged-source-and-external-readback",
        "sha256": VALID_SHA256,
        "valid": True,
    }
    assert report["build_marker"] == {
        "expected_line_sha256": repeatability._sha256_text(VALID_BUILD_MARKER),
        "identity_role": "serial-boot-binding",
        "line": VALID_BUILD_MARKER,
        "valid": True,
    }
    marker_image_id = repeatability.IMAGE_ID_RE.search(VALID_BUILD_MARKER)
    assert marker_image_id is not None
    for role in ("readback_image", "staged_image"):
        artifact = report[role]
        assert artifact["conflicting_marker_count"] == 0
        assert artifact["distinct_marker_line_sha256"] == [
            hashlib.sha256(VALID_BUILD_MARKER.encode("utf-8")).hexdigest()
        ]
        assert artifact["hash_match"] is True
        assert artifact["image_id"] == marker_image_id.group("image_id")
        assert artifact["image_id_match"] is True
        assert artifact["marker_occurrence_count"] == 1
        assert artifact["sha256"] == VALID_SHA256
        assert artifact["size_bytes"] == len(VALID_READBACK_BYTES)
        assert artifact["status"] == "verified"
        assert artifact["verified"] is True
        assert isinstance(artifact["device"], int)
        assert isinstance(artifact["inode"], int)
    assert report["staged_readback_binding"] == {
        "distinct_open_files": True,
        "distinct_paths": True,
        "sha256_match": True,
        "valid": True,
    }
    assert report["identity_binding"]["valid"] is True
    assert report["classes"]["cold"]["counts"]["passing_wifi_slices"] == 10
    assert report["classes"]["warm"]["counts"]["passing_wifi_slices"] == 10
    assert len(report["classes"]["cold"]["logs"]) == 2
    assert len(report["classes"]["warm"]["logs"]) == 2
    assert output.read_text(encoding="utf-8") == rendered


def test_image_a_uart_logs_cannot_cross_bind_to_valid_image_b(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Different complete images cannot share accepted serial evidence."""

    cold = write_log(tmp_path / "cold-a.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm-a.log", ["pass"] * 10)
    image_b, marker_b = make_sealed_uimage(
        git_hash="abc1234",
        payload_prefix=b"different-kernel-and-elfloader",
    )
    staged = tmp_path / "staged-b.img"
    readback = tmp_path / "readback-b.img"
    staged.write_bytes(image_b)
    readback.write_bytes(image_b)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        image_sha256=hashlib.sha256(image_b).hexdigest(),
        build_marker=marker_b,
        staged_image=staged,
        readback_image=readback,
    )

    assert result == 2
    assert report["staged_image"]["status"] == "verified"
    assert report["readback_image"]["status"] == "verified"
    assert report["classes"]["cold"]["counts"]["marker_failed_slices"] == 10
    assert report["classes"]["warm"]["counts"]["marker_failed_slices"] == 10
    assert report["result"] == "FAIL"


@pytest.mark.parametrize("input_role", ["staged", "readback", "cold", "warm"])
@pytest.mark.parametrize("alias_kind", ["equal", "symlink", "hardlink"])
def test_output_cannot_alias_any_evidence_input_before_read_or_write(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
    input_role: str,
    alias_kind: str,
) -> None:
    """Equal, symbolic, and hard-link outputs cannot overwrite evidence."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    staged = tmp_path / "staged.img"
    readback = tmp_path / "readback.img"
    staged.write_bytes(VALID_READBACK_BYTES)
    readback.write_bytes(VALID_READBACK_BYTES)
    inputs = {
        "staged": staged,
        "readback": readback,
        "cold": cold,
        "warm": warm,
    }
    protected = inputs[input_role]
    before = protected.read_bytes()
    if alias_kind == "equal":
        output = protected
    else:
        output = tmp_path / f"{input_role}-{alias_kind}.json"
        if alias_kind == "symlink":
            output.symlink_to(protected)
        else:
            output.hardlink_to(protected)

    def unexpected_report(**_kwargs: object) -> dict[str, object]:
        pytest.fail("aliased output must be rejected before evidence is read")

    monkeypatch.setattr(repeatability, "build_report", unexpected_report)

    with pytest.raises(SystemExit) as error:
        repeatability.main(
            repeatability_argv(
                cold=cold,
                warm=warm,
                staged=staged,
                readback=readback,
                output=output,
            )
        )

    captured = capsys.readouterr()
    expected_option = {
        "staged": "--staged-image",
        "readback": "--readback-image",
        "cold": "--cold-log",
        "warm": "--warm-log",
    }[input_role]
    assert error.value.code == 2
    assert "--output must not alias evidence input" in captured.err
    assert expected_option in captured.err
    assert protected.read_bytes() == before
    assert output.read_bytes() == before


@pytest.mark.parametrize(
    ("option", "label"),
    [
        ("--image-identity-metadata", "--image-identity-metadata"),
        ("--capture-manifest", "--capture-manifest"),
    ],
)
@pytest.mark.parametrize("alias_kind", ["equal", "symlink", "hardlink"])
def test_output_cannot_alias_evidence_sidecars_before_read(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
    option: str,
    label: str,
    alias_kind: str,
) -> None:
    """The output path cannot replace either required evidence sidecar."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    staged = tmp_path / "staged.img"
    readback = tmp_path / "readback.img"
    staged.write_bytes(VALID_READBACK_BYTES)
    readback.write_bytes(VALID_READBACK_BYTES)
    argv = repeatability_argv(
        cold=cold,
        warm=warm,
        staged=staged,
        readback=readback,
        output=tmp_path / "report.json",
    )
    protected = pathlib.Path(argv[argv.index(option) + 1])
    before = protected.read_bytes()
    if alias_kind == "equal":
        output = protected
    else:
        output = tmp_path / f"sidecar-{alias_kind}-report.json"
        if alias_kind == "symlink":
            output.symlink_to(protected)
        else:
            output.hardlink_to(protected)
    argv[argv.index("--output") + 1] = str(output)

    def unexpected_report(**_kwargs: object) -> dict[str, object]:
        pytest.fail("sidecar alias must be rejected before evidence is read")

    monkeypatch.setattr(repeatability, "build_report", unexpected_report)

    with pytest.raises(SystemExit) as error:
        repeatability.main(argv)

    captured = capsys.readouterr()
    assert error.value.code == 2
    assert label in captured.err
    assert protected.read_bytes() == before


@pytest.mark.parametrize("alias_kind", ["equal", "symlink", "hardlink"])
def test_output_cannot_alias_manifest_pcap_before_write(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    alias_kind: str,
) -> None:
    """A pcap listed inside the manifest cannot become the report output."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    staged = tmp_path / "staged.img"
    readback = tmp_path / "readback.img"
    staged.write_bytes(VALID_READBACK_BYTES)
    readback.write_bytes(VALID_READBACK_BYTES)
    argv = repeatability_argv(
        cold=cold,
        warm=warm,
        staged=staged,
        readback=readback,
        output=tmp_path / "report.json",
    )
    capture = pathlib.Path(argv[argv.index("--capture-manifest") + 1])
    manifest = read_json_object(capture)
    runs = manifest["runs"]
    assert isinstance(runs, list) and runs and isinstance(runs[0], dict)
    protected = pathlib.Path(str(runs[0]["pcap_path"]))
    before = protected.read_bytes()
    if alias_kind == "equal":
        output = protected
    else:
        output = tmp_path / f"pcap-{alias_kind}-report.json"
        if alias_kind == "symlink":
            output.symlink_to(protected)
        else:
            output.hardlink_to(protected)
    argv[argv.index("--output") + 1] = str(output)

    with pytest.raises(SystemExit) as error:
        repeatability.main(argv)

    captured = capsys.readouterr()
    assert error.value.code == 2
    assert "capture-manifest pcap" in captured.err
    assert protected.read_bytes() == before


def test_missing_manifest_pcap_path_cannot_be_created_as_output(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Even a failed declared pcap path remains protected from report writes."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    staged = tmp_path / "staged.img"
    readback = tmp_path / "readback.img"
    staged.write_bytes(VALID_READBACK_BYTES)
    readback.write_bytes(VALID_READBACK_BYTES)
    missing = tmp_path / "missing-capture.pcap"
    argv = repeatability_argv(
        cold=cold,
        warm=warm,
        staged=staged,
        readback=readback,
        output=missing,
    )
    capture = pathlib.Path(argv[argv.index("--capture-manifest") + 1])
    manifest = read_json_object(capture)
    runs = manifest["runs"]
    assert isinstance(runs, list) and runs and isinstance(runs[0], dict)
    old_pcap = pathlib.Path(str(runs[0]["pcap_path"]))
    old_pcap.unlink()
    runs[0]["pcap_path"] = str(missing)
    runs[0]["pcap_sha256"] = "0" * 64
    write_json_object(capture, manifest)

    with pytest.raises(SystemExit) as error:
        repeatability.main(argv)

    captured = capsys.readouterr()
    assert error.value.code == 2
    assert "capture-manifest pcap" in captured.err
    assert not missing.exists()


def test_repeated_log_path_cannot_inflate_boot_count(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """One capture repeated on the CLI is still only one observation."""

    one_cold = write_log(tmp_path / "cold.log", ["pass"])
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)

    result, report, _ = run_report(
        capsys, [one_cold] * 10, [warm]
    )

    assert result == 2
    assert "duplicate-log-paths" in report["failure_reasons"]
    assert "duplicate-log-content" in report["failure_reasons"]
    assert len(report["evidence_inputs"]["duplicate_paths"]) == 1


def test_copied_or_cross_class_capture_cannot_count_twice(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Byte-identical aliases cannot masquerade as distinct reset classes."""

    source = write_log(tmp_path / "source.log", ["pass"] * 10)
    copied = tmp_path / "copied.log"
    copied.write_bytes(source.read_bytes())

    result, report, _ = run_report(capsys, [source], [copied])

    assert result == 2
    assert "duplicate-log-content" in report["failure_reasons"]
    assert "cold-warm-log-content-overlap" in report["failure_reasons"]


def test_duplicate_raw_slice_in_distinct_logs_cannot_count_twice(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Different whole-log hashes cannot disguise a replayed boot slice."""

    shared_slice = (
        f"BOOT pass\nRUN shared-run:1\n{VALID_BUILD_MARKER}\n"
    )
    first = tmp_path / "cold-a.log"
    second = tmp_path / "cold-b.log"
    first.write_text("CAPTURE first\n" + shared_slice, encoding="utf-8")
    second.write_text("CAPTURE second\n" + shared_slice, encoding="utf-8")
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)

    result, report, _ = run_report(capsys, [first, second], [warm])

    assert result == 2
    assert first.read_bytes() != second.read_bytes()
    assert "duplicate-raw-boot-slices" in report["failure_reasons"]
    assert len(report["evidence_inputs"]["duplicate_raw_slice_sha256"]) == 1


def test_duplicate_raw_slice_within_one_log_cannot_count_twice(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """One captured boot replayed in one serial file remains one observation."""

    shared_slice = f"BOOT pass\nRUN shared:1\n{VALID_BUILD_MARKER}\n"
    cold = tmp_path / "cold.log"
    cold.write_text("CAPTURE cold\n" + shared_slice + shared_slice, encoding="utf-8")
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)

    result, report, _ = run_report(capsys, [cold], [warm])

    assert result == 2
    assert "duplicate-raw-boot-slices" in report["failure_reasons"]
    duplicate_digests = report["evidence_inputs"]["duplicate_raw_slice_sha256"]
    assert duplicate_digests == [hashlib.sha256(shared_slice.encode()).hexdigest()]


def test_duplicate_raw_slice_across_cold_and_warm_classes_fails_closed(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A replayed raw boot cannot satisfy both reset classes."""

    shared_slice = f"BOOT pass\nRUN shared:1\n{VALID_BUILD_MARKER}\n"
    cold = tmp_path / "cold.log"
    warm = tmp_path / "warm.log"
    cold.write_text("CAPTURE cold\n" + shared_slice, encoding="utf-8")
    warm.write_text("CAPTURE warm\n" + shared_slice, encoding="utf-8")

    result, report, _ = run_report(capsys, [cold], [warm])

    assert result == 2
    assert "cold-warm-raw-boot-slice-overlap" in report["failure_reasons"]


def test_duplicate_serial_slice_index_is_not_silently_overwritten() -> None:
    """Manifest indexing fails explicitly when normalizer slice IDs collide."""

    duplicate_boot = {
        "serial_slice_index": 1,
        "source_score": "pass",
        "raw_slice_sha256": "a" * 64,
    }
    cold = {
        "logs": [
            {
                "resolved_path": "/capture/cold.log",
                "sha256": "b" * 64,
                "boots": [duplicate_boot, dict(duplicate_boot)],
            }
        ]
    }

    expected, collisions = repeatability._expected_capture_slices(cold, {"logs": []})

    assert len(expected) == 1
    assert collisions == [("/capture/cold.log", 1)]


@pytest.mark.parametrize("alias_kind", ["symlink", "hardlink"])
def test_cold_warm_log_inode_aliases_fail_closed(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    alias_kind: str,
) -> None:
    """Path spelling cannot hide one serial capture reused across classes."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = tmp_path / f"warm-{alias_kind}.log"
    if alias_kind == "symlink":
        warm.symlink_to(cold)
    else:
        warm.hardlink_to(cold)

    result, report, _ = run_report(capsys, [cold], [warm])

    assert result == 2
    assert "duplicate-log-open-file-identity" in report["failure_reasons"]
    assert "cold-warm-log-open-file-overlap" in report["failure_reasons"]


@pytest.mark.parametrize(
    "changed_field",
    ["st_dev", "st_ino", "st_size", "st_mtime_ns", "st_ctime_ns"],
)
def test_stable_reader_rejects_mid_read_metadata_change(
    tmp_path: pathlib.Path,
    monkeypatch: pytest.MonkeyPatch,
    changed_field: str,
) -> None:
    """A descriptor whose identity metadata changes during one read is rejected."""

    path = tmp_path / "serial.log"
    path.write_bytes(b"captured bytes\n")
    real_fstat = repeatability.os.fstat
    calls = 0

    def changing_fstat(fd: int) -> object:
        nonlocal calls
        calls += 1
        observed = real_fstat(fd)
        if calls == 1:
            return observed
        values = {
            name: getattr(observed, name)
            for name in (
                "st_mode",
                "st_dev",
                "st_ino",
                "st_size",
                "st_mtime_ns",
                "st_ctime_ns",
            )
        }
        values[changed_field] += 1
        return types.SimpleNamespace(**values)

    monkeypatch.setattr(repeatability.os, "fstat", changing_fstat)

    with pytest.raises(repeatability.EvidenceReadError, match="changed while open"):
        repeatability._read_stable_regular_file(path)


def test_stable_reader_uses_one_descriptor_for_complete_read(
    tmp_path: pathlib.Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The stable reader never reopens a path between identity checks."""

    path = tmp_path / "serial.log"
    expected = b"captured bytes\n"
    path.write_bytes(expected)
    real_open = repeatability.os.open
    real_fstat = repeatability.os.fstat
    real_read = repeatability.os.read
    real_close = repeatability.os.close
    opened: list[int] = []
    fstat_descriptors: list[int] = []
    read_descriptors: list[int] = []
    closed: list[int] = []

    def tracked_open(path_arg: object, flags: int) -> int:
        descriptor = real_open(path_arg, flags)
        opened.append(descriptor)
        return descriptor

    def tracked_fstat(descriptor: int) -> os.stat_result:
        fstat_descriptors.append(descriptor)
        return real_fstat(descriptor)

    def tracked_read(descriptor: int, length: int) -> bytes:
        read_descriptors.append(descriptor)
        return real_read(descriptor, length)

    def tracked_close(descriptor: int) -> None:
        closed.append(descriptor)
        real_close(descriptor)

    monkeypatch.setattr(repeatability.os, "open", tracked_open)
    monkeypatch.setattr(repeatability.os, "fstat", tracked_fstat)
    monkeypatch.setattr(repeatability.os, "read", tracked_read)
    monkeypatch.setattr(repeatability.os, "close", tracked_close)

    observed = repeatability._read_stable_regular_file(path)

    assert observed["data"] == expected
    assert len(opened) == 1
    descriptor = opened[0]
    assert fstat_descriptors == [descriptor, descriptor]
    assert read_descriptors and set(read_descriptors) == {descriptor}
    assert closed == [descriptor]


def test_stable_reader_requires_a_regular_file() -> None:
    """Character devices and other non-regular evidence sources are rejected."""

    with pytest.raises(repeatability.EvidenceReadError, match="not a regular file"):
        repeatability._read_stable_regular_file(pathlib.Path("/dev/null"))


def test_stable_reader_rejects_fifo_without_waiting_for_a_writer(
    tmp_path: pathlib.Path,
) -> None:
    """Named-pipe evidence is rejected through a nonblocking descriptor."""

    fifo = tmp_path / "serial.fifo"
    os.mkfifo(fifo)

    with pytest.raises(repeatability.EvidenceReadError, match="not a regular file"):
        repeatability._read_stable_regular_file(fifo)


def test_stable_reader_reports_nul_path_as_typed_evidence_error() -> None:
    """Malformed path text cannot produce an uncaught ValueError."""

    with pytest.raises(repeatability.EvidenceReadError, match="cannot read"):
        repeatability._read_stable_regular_file(pathlib.Path("bad\0path"))


@pytest.mark.parametrize(
    ("tamper", "expected_failure"),
    [
        ("run-id", "capture-run-id-duplicate"),
        ("class", "capture-run-class-mismatch"),
        ("serial-hash", "capture-run-serial-sha256-mismatch"),
        ("slice-hash", "capture-run-slice-sha256-mismatch"),
        ("pcap-hash", "capture-run-pcap-sha256-mismatch"),
        ("pcap-format", "capture-run-pcap-format-invalid"),
        ("pcap-epoch", "capture-run-pcap-epoch-mismatch"),
        ("image-id", "capture-run-image-id-mismatch"),
        ("git-commit", "capture-run-git-commit-mismatch"),
        ("build-id", "capture-run-build-id-mismatch"),
        ("capture-epoch", "capture-run-before-build"),
    ],
)
def test_capture_manifest_tampering_fails_closed(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    tamper: str,
    expected_failure: str,
) -> None:
    """Each capture-binding field is independently enforced."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    identity, capture, git_commit, build_id = write_evidence_sidecars(
        tmp_path,
        [cold],
        [warm],
        image_sha256=VALID_SHA256,
        build_marker=VALID_BUILD_MARKER,
    )
    manifest = read_json_object(capture)
    runs = manifest["runs"]
    assert isinstance(runs, list) and len(runs) >= 2
    first = runs[0]
    second = runs[1]
    assert isinstance(first, dict) and isinstance(second, dict)
    if tamper == "run-id":
        second["run_id"] = first["run_id"]
    elif tamper == "class":
        first["boot_class"] = "warm"
    elif tamper == "serial-hash":
        first["serial_sha256"] = "0" * 64
    elif tamper == "slice-hash":
        first["serial_slice_sha256"] = "0" * 64
    elif tamper == "pcap-hash":
        first["pcap_sha256"] = "0" * 64
    elif tamper == "pcap-format":
        pcap_path = pathlib.Path(str(first["pcap_path"]))
        pcap_path.write_bytes(b"not a packet capture\n")
        first["pcap_sha256"] = hashlib.sha256(pcap_path.read_bytes()).hexdigest()
    elif tamper == "pcap-epoch":
        first["capture_epoch"] = int(first["capture_epoch"]) + 1
    elif tamper == "image-id":
        first["image_id"] = "0" * 64
    elif tamper == "git-commit":
        first["git_commit"] = STALE_GIT_COMMIT
    elif tamper == "build-id":
        first["build_id"] = "0" * 64
    else:
        assert tamper == "capture-epoch"
        first["capture_epoch"] = 0
    write_json_object(capture, manifest)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        image_identity_metadata=identity,
        capture_manifest=capture,
        expected_git_commit=git_commit,
        expected_build_id=build_id,
    )

    assert result == 2
    capture_record = report["capture_manifest"]
    observed = {
        reason
        for run in capture_record["runs"]
        for reason in run["failure_reasons"]
    }
    assert expected_failure in observed
    assert "capture-run-invalid" in report["failure_reasons"]


def test_partial_identity_v2_sidecar_is_rejected(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A plausible subset cannot masquerade as the complete identity schema."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    staged = tmp_path / "staged.img"
    readback = tmp_path / "readback.img"
    staged.write_bytes(VALID_READBACK_BYTES)
    readback.write_bytes(VALID_READBACK_BYTES)
    identity, capture, git_commit, build_id = write_evidence_sidecars(
        tmp_path,
        [cold],
        [warm],
        image_sha256=VALID_SHA256,
        build_marker=VALID_BUILD_MARKER,
        staged_image=staged,
    )
    metadata = read_json_object(identity)
    metadata.pop("rootserver_sha256")
    write_json_object(identity, metadata)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        staged_image=staged,
        readback_image=readback,
        image_identity_metadata=identity,
        capture_manifest=capture,
        expected_git_commit=git_commit,
        expected_build_id=build_id,
    )

    assert result == 2
    assert "image-identity-metadata-invalid" in report["failure_reasons"]


def test_identity_root_hash_forgery_breaks_independent_metadata_hash(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Syntactically valid root provenance cannot replace the trusted sidecar."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    staged = tmp_path / "staged.img"
    readback = tmp_path / "readback.img"
    staged.write_bytes(VALID_READBACK_BYTES)
    readback.write_bytes(VALID_READBACK_BYTES)
    identity, capture, git_commit, build_id = write_evidence_sidecars(
        tmp_path,
        [cold],
        [warm],
        image_sha256=VALID_SHA256,
        build_marker=VALID_BUILD_MARKER,
        staged_image=staged,
    )
    trusted_sha256 = hashlib.sha256(identity.read_bytes()).hexdigest()
    metadata = read_json_object(identity)
    metadata["rootserver_sha256"] = "f" * 64
    write_json_object(identity, metadata)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        staged_image=staged,
        readback_image=readback,
        image_identity_metadata=identity,
        capture_manifest=capture,
        expected_git_commit=git_commit,
        expected_build_id=build_id,
        expected_image_identity_sha256=trusted_sha256,
    )

    assert result == 2
    assert "image-identity-metadata-sha256-mismatch" in report["failure_reasons"]
    assert "capture-manifest-identity-binding-invalid" in report["failure_reasons"]


@pytest.mark.parametrize("invalid_class", [[], {}, 1, True, None])
def test_capture_manifest_class_type_fails_as_report_not_exception(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    invalid_class: object,
) -> None:
    """Non-string class values remain typed evidence failures."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    staged = tmp_path / "staged.img"
    readback = tmp_path / "readback.img"
    staged.write_bytes(VALID_READBACK_BYTES)
    readback.write_bytes(VALID_READBACK_BYTES)
    identity, capture, git_commit, build_id = write_evidence_sidecars(
        tmp_path,
        [cold],
        [warm],
        image_sha256=VALID_SHA256,
        build_marker=VALID_BUILD_MARKER,
        staged_image=staged,
    )
    manifest = read_json_object(capture)
    runs = manifest["runs"]
    assert isinstance(runs, list) and runs and isinstance(runs[0], dict)
    runs[0]["boot_class"] = invalid_class
    write_json_object(capture, manifest)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        staged_image=staged,
        readback_image=readback,
        image_identity_metadata=identity,
        capture_manifest=capture,
        expected_git_commit=git_commit,
        expected_build_id=build_id,
    )

    assert result == 2
    observed = {
        reason
        for run in report["capture_manifest"]["runs"]
        for reason in run["failure_reasons"]
    }
    assert "capture-run-class-invalid" in observed


def test_zero_length_pcap_packet_is_not_independent_boot_evidence(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A timestamp-only record with no captured frame cannot count as a pcap."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    staged = tmp_path / "staged.img"
    readback = tmp_path / "readback.img"
    staged.write_bytes(VALID_READBACK_BYTES)
    readback.write_bytes(VALID_READBACK_BYTES)
    identity, capture, git_commit, build_id = write_evidence_sidecars(
        tmp_path,
        [cold],
        [warm],
        image_sha256=VALID_SHA256,
        build_marker=VALID_BUILD_MARKER,
        staged_image=staged,
    )
    manifest = read_json_object(capture)
    runs = manifest["runs"]
    assert isinstance(runs, list) and runs and isinstance(runs[0], dict)
    first = runs[0]
    pcap = pathlib.Path(str(first["pcap_path"]))
    capture_epoch = int(first["capture_epoch"])
    empty_record = b"".join(
        (
            b"\xd4\xc3\xb2\xa1",
            struct.pack("<HHiIII", 2, 4, 0, 0, 65_535, 1),
            struct.pack("<IIII", capture_epoch, 0, 0, 0),
        )
    )
    pcap.write_bytes(empty_record)
    first["pcap_sha256"] = hashlib.sha256(empty_record).hexdigest()
    write_json_object(capture, manifest)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        staged_image=staged,
        readback_image=readback,
        image_identity_metadata=identity,
        capture_manifest=capture,
        expected_git_commit=git_commit,
        expected_build_id=build_id,
    )

    assert result == 2
    observed = {
        reason
        for run in report["capture_manifest"]["runs"]
        for reason in run["failure_reasons"]
    }
    assert "capture-run-pcap-format-invalid" in observed


def test_capture_manifest_rejects_hardlinked_pcaps(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Different pcap path strings cannot disguise one open file identity."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    identity, capture, git_commit, build_id = write_evidence_sidecars(
        tmp_path,
        [cold],
        [warm],
        image_sha256=VALID_SHA256,
        build_marker=VALID_BUILD_MARKER,
    )
    manifest = read_json_object(capture)
    runs = manifest["runs"]
    assert isinstance(runs, list) and len(runs) >= 2
    first = runs[0]
    second = runs[1]
    assert isinstance(first, dict) and isinstance(second, dict)
    first_pcap = pathlib.Path(str(first["pcap_path"]))
    second_pcap = pathlib.Path(str(second["pcap_path"]))
    second_pcap.unlink()
    second_pcap.hardlink_to(first_pcap)
    second["pcap_sha256"] = first["pcap_sha256"]
    write_json_object(capture, manifest)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        image_identity_metadata=identity,
        capture_manifest=capture,
        expected_git_commit=git_commit,
        expected_build_id=build_id,
    )

    assert result == 2
    observed = {
        reason
        for run in report["capture_manifest"]["runs"]
        for reason in run["failure_reasons"]
    }
    assert "capture-run-pcap-open-file-duplicate" in observed
    assert "capture-run-pcap-content-duplicate" in observed


@pytest.mark.parametrize(
    "evidence_role",
    ["serial", "identity", "manifest", "staged", "readback"],
)
@pytest.mark.parametrize("alias_kind", ["symlink", "hardlink"])
def test_pcap_cannot_alias_any_other_evidence_file(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    evidence_role: str,
    alias_kind: str,
) -> None:
    """A paired capture must be an independent regular file and inode."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    staged = tmp_path / "staged.img"
    readback = tmp_path / "readback.img"
    staged.write_bytes(VALID_READBACK_BYTES)
    readback.write_bytes(VALID_READBACK_BYTES)
    identity, capture, git_commit, build_id = write_evidence_sidecars(
        tmp_path,
        [cold],
        [warm],
        image_sha256=VALID_SHA256,
        build_marker=VALID_BUILD_MARKER,
    )
    protected = {
        "serial": cold,
        "identity": identity,
        "manifest": capture,
        "staged": staged,
        "readback": readback,
    }[evidence_role]
    manifest = read_json_object(capture)
    runs = manifest["runs"]
    assert isinstance(runs, list) and runs and isinstance(runs[0], dict)
    first = runs[0]
    pcap = pathlib.Path(str(first["pcap_path"]))
    pcap.unlink()
    if alias_kind == "symlink":
        pcap.symlink_to(protected)
    else:
        pcap.hardlink_to(protected)
    first["pcap_sha256"] = hashlib.sha256(protected.read_bytes()).hexdigest()
    write_json_object(capture, manifest)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        staged_image=staged,
        readback_image=readback,
        image_identity_metadata=identity,
        capture_manifest=capture,
        expected_git_commit=git_commit,
        expected_build_id=build_id,
    )

    assert result == 2
    observed = {
        reason
        for run in report["capture_manifest"]["runs"]
        for reason in run["failure_reasons"]
    }
    if alias_kind == "symlink":
        assert "capture-run-pcap-evidence-path-alias" in observed
    assert "capture-run-pcap-evidence-open-file-alias" in observed


def test_stale_coherent_bundle_cannot_satisfy_current_commit_expectation(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Self-consistent stale image, serial, and manifests remain stale evidence."""

    cold = write_log(
        tmp_path / "cold.log", ["pass"] * 10, markers=[STALE_BUILD_MARKER] * 10
    )
    warm = write_log(
        tmp_path / "warm.log", ["pass"] * 10, markers=[STALE_BUILD_MARKER] * 10
    )
    stale_sha256 = hashlib.sha256(STALE_READBACK_BYTES).hexdigest()
    identity, capture, _stale_commit, _stale_build_id = write_evidence_sidecars(
        tmp_path,
        [cold],
        [warm],
        image_sha256=stale_sha256,
        build_marker=STALE_BUILD_MARKER,
    )
    staged = tmp_path / "staged.img"
    readback = tmp_path / "readback.img"
    staged.write_bytes(STALE_READBACK_BYTES)
    readback.write_bytes(STALE_READBACK_BYTES)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        image_sha256=stale_sha256,
        build_marker=STALE_BUILD_MARKER,
        staged_image=staged,
        readback_image=readback,
        image_identity_metadata=identity,
        capture_manifest=capture,
        expected_git_commit=VALID_GIT_COMMIT,
        expected_build_id=VALID_BUILD_ID,
    )

    assert result == 2
    assert "image-identity-git-commit-mismatch" in report["failure_reasons"]
    assert "image-identity-build-id-mismatch" in report["failure_reasons"]
    assert "capture-run-invalid" in report["failure_reasons"]


@pytest.mark.parametrize("commit_length", [39, 41, 63, 65])
def test_malformed_identity_commit_fails_as_report_not_exception(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    commit_length: int,
) -> None:
    """Near-boundary object IDs cannot escape validation through a traceback."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    identity, capture, git_commit, build_id = write_evidence_sidecars(
        tmp_path,
        [cold],
        [warm],
        image_sha256=VALID_SHA256,
        build_marker=VALID_BUILD_MARKER,
    )
    metadata = read_json_object(identity)
    metadata["git_commit"] = "a" * commit_length
    write_json_object(identity, metadata)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        image_identity_metadata=identity,
        capture_manifest=capture,
        expected_git_commit=git_commit,
        expected_build_id=build_id,
    )

    assert result == 2
    assert "image-identity-git-commit-invalid" in report["failure_reasons"]


@pytest.mark.parametrize(
    "timestamp",
    [
        "not-a-timestamp",
        "2026-07-16T00:00:00",
        "2026-07-16T01:00:00+01:00",
    ],
)
def test_identity_timestamp_must_match_marker_and_be_timezone_qualified(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    timestamp: str,
) -> None:
    """Metadata time cannot be self-consistent yet differ from the image marker."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    identity, capture, git_commit, build_id = write_evidence_sidecars(
        tmp_path,
        [cold],
        [warm],
        image_sha256=VALID_SHA256,
        build_marker=VALID_BUILD_MARKER,
    )
    metadata = read_json_object(identity)
    metadata["build_timestamp"] = timestamp
    write_json_object(identity, metadata)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        image_identity_metadata=identity,
        capture_manifest=capture,
        expected_git_commit=git_commit,
        expected_build_id=build_id,
    )

    assert result == 2
    assert "image-identity-build-timestamp-mismatch" in report["failure_reasons"]
    if timestamp.endswith("+01:00"):
        assert "image-identity-build-id-not-canonical" in report["failure_reasons"]
    else:
        assert "image-identity-build-timestamp-invalid" in report["failure_reasons"]


def test_one_failed_slice_fails_even_with_ten_other_passes(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A failed boot cannot be hidden by enough successful boot slices."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10 + ["fail"])
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)

    result, report, _ = run_report(capsys, [cold], [warm])

    assert result == 2
    assert report["result"] == "FAIL"
    assert report["classes"]["cold"]["counts"]["failed_slices"] == 1
    assert "cold-log-failures-present" in report["failure_reasons"]
    failed_boot = report["classes"]["cold"]["logs"][0]["boots"][-1]
    assert failed_boot["blockers"] == ["panic"]
    assert failed_boot["counted"] is False


def test_wrong_active_transport_does_not_count(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A blocker-free wired boot is not CYW43 repeatability evidence."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 9 + ["wired"])
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)

    result, report, _ = run_report(capsys, [cold], [warm])

    assert result == 2
    cold_record = report["classes"]["cold"]
    assert cold_record["counts"]["passing_wifi_slices"] == 9
    wrong_boot = cold_record["logs"][0]["boots"][-1]
    assert wrong_boot["net_active"] == "wired"
    assert wrong_boot["failure_reasons"] == ["net-active-not-wifi"]


def test_current_profile_requires_terminal_bootstrap_supervisor_proof(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Historical no-supervisor logs cannot close current-image reliability."""

    cold = write_log(
        tmp_path / "cold.log", ["pass"] * 9 + ["no-supervisor"]
    )
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)

    result, report, _ = run_report(capsys, [cold], [warm])

    assert result == 2
    missing = report["classes"]["cold"]["logs"][0]["boots"][-1]
    assert missing["supervisor_seen"] == "no"
    assert missing["supervisor_ready"] == "no"
    assert missing["failure_reasons"] == ["bootstrap-supervisor-not-seen"]


def test_insufficient_class_count_fails(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Nine cold successes cannot satisfy the default ten-boot threshold."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 9)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)

    result, report, _ = run_report(capsys, [cold], [warm])

    assert result == 2
    assert report["classes"]["cold"]["result"] == "FAIL"
    assert "cold-passing-slices-insufficient" in report["failure_reasons"]


def test_invalid_image_sha256_emits_fail_report(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Image evidence without a 64-hex identity fails closed."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        image_sha256="not-a-sha256",
    )

    assert result == 2
    assert report["result"] == "FAIL"
    assert report["image"] == {
        "identity_role": "staged-source-and-external-readback",
        "sha256": "not-a-sha256",
        "valid": False,
    }
    assert "image-sha256-invalid" in report["failure_reasons"]
    assert "image-identity-metadata-invalid" in report["failure_reasons"]
    assert report["readback_image"]["status"] == "expected-sha256-invalid"
    assert report["identity_binding"]["valid"] is False


@pytest.mark.parametrize(
    ("tokens", "expected_reason"),
    [
        (["skip"], "skip-only"),
        ([], "boot-slices-missing"),
    ],
)
def test_skip_only_or_missing_boot_slices_fail_closed(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    tokens: list[str],
    expected_reason: str,
) -> None:
    """Non-proof logs never contribute inferred boot success."""

    cold = write_log(tmp_path / "cold.log", tokens)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)

    result, report, _ = run_report(capsys, [cold], [warm])

    assert result == 2
    cold_log = report["classes"]["cold"]["logs"][0]
    assert cold_log["result"] == "FAIL"
    assert expected_reason in cold_log["failure_reasons"]


def test_missing_build_marker_fails_its_boot_slice(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A successful WiFi slice without the exact marker cannot be counted."""

    cold = write_log(
        tmp_path / "cold.log",
        ["pass"] * 10,
        markers=[VALID_BUILD_MARKER] * 9 + [None],
    )
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)

    result, report, _ = run_report(capsys, [cold], [warm])

    assert result == 2
    missing_boot = report["classes"]["cold"]["logs"][0]["boots"][-1]
    assert missing_boot["build_marker"]["status"] == "missing"
    assert missing_boot["failure_reasons"] == ["build-marker-missing"]
    assert report["classes"]["cold"]["counts"]["passing_wifi_slices"] == 9


def test_mismatched_build_marker_fails_its_boot_slice(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A marker from a different build cannot satisfy serial identity binding."""

    cold = write_log(
        tmp_path / "cold.log",
        ["pass"] * 10,
        markers=[VALID_BUILD_MARKER] * 9 + ["[BUILD] another-image"],
    )
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)

    result, report, _ = run_report(capsys, [cold], [warm])

    assert result == 2
    mismatch = report["classes"]["cold"]["logs"][0]["boots"][-1]
    assert mismatch["build_marker"]["status"] == "mismatch"
    assert mismatch["failure_reasons"] == ["build-marker-mismatch"]
    assert mismatch["build_marker"]["observed_count"] == 1


def test_each_boot_range_requires_its_own_marker(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """One marker cannot bind a later boot slice outside its line range."""

    cold = write_log(
        tmp_path / "cold.log",
        ["pass"] * 10,
        markers=[VALID_BUILD_MARKER] * 9 + [None],
    )
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)

    result, report, _ = run_report(capsys, [cold], [warm])

    assert result == 2
    boots = report["classes"]["cold"]["logs"][0]["boots"]
    assert [boot["build_marker"]["status"] for boot in boots] == [
        "match",
    ] * 9 + ["missing"]
    assert boots[-2]["line_end"] < boots[-1]["line_start"]


@pytest.mark.parametrize(
    ("argument", "expected"),
    [
        (VALID_BUILD_MARKER, VALID_BUILD_MARKER),
        (VALID_BUILD_MARKER.removeprefix("[BUILD] "), VALID_BUILD_MARKER),
    ],
)
def test_build_marker_argument_accepts_exact_line_or_unambiguous_payload(
    argument: str,
    expected: str,
) -> None:
    """The preferred full line and a plain payload normalize deterministically."""

    assert repeatability.exact_build_marker(argument) == expected


@pytest.mark.parametrize(
    "argument",
    ["", " ", "[BUILD]", "[BUILD] legacy-image", "x [BUILD] y", "x\n"],
)
def test_build_marker_argument_rejects_empty_or_ambiguous_values(
    argument: str,
) -> None:
    """Malformed marker identity is rejected before evidence is assessed."""

    with pytest.raises(repeatability.argparse.ArgumentTypeError) as error:
        repeatability.exact_build_marker(argument)
    assert "build marker" in str(error.value)


def test_missing_readback_image_fails_closed(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A caller-supplied digest cannot replace the actual read-back bytes."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    missing = tmp_path / "missing-readback.img"

    result, report, _ = run_report(
        capsys, [cold], [warm], readback_image=missing
    )

    assert result == 2
    assert report["readback_image"]["status"] == "missing"
    assert report["readback_image"]["sha256"] is None
    assert report["failure_reasons"] == ["readback-image-missing"]


def test_missing_staged_source_image_fails_closed(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A target readback is not proof of what source artifact was copied."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    readback = tmp_path / "readback.img"
    readback.write_bytes(VALID_READBACK_BYTES)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        staged_image=tmp_path / "missing-staged.img",
        readback_image=readback,
    )

    assert result == 2
    assert report["staged_image"]["status"] == "missing"
    assert report["failure_reasons"] == ["staged-image-missing"]


def test_staged_and_readback_paths_must_not_alias(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Reading the source file twice is not independent target readback."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    artifact = tmp_path / "same.img"
    artifact.write_bytes(VALID_READBACK_BYTES)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        staged_image=artifact,
        readback_image=artifact,
    )

    assert result == 2
    assert report["staged_readback_binding"]["distinct_paths"] is False
    assert report["failure_reasons"] == ["staged-readback-path-alias"]


def test_unreadable_readback_image_fails_closed(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """An image that cannot be streamed never receives identity credit."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    readback = tmp_path / "unreadable.img"
    readback.write_bytes(VALID_READBACK_BYTES)
    original_open = repeatability.image_identity.os.open

    def controlled_open(
        path: object, *args: object, **kwargs: object
    ) -> int:
        if pathlib.Path(path) == readback:
            raise PermissionError("synthetic unreadable image")
        return original_open(path, *args, **kwargs)

    monkeypatch.setattr(repeatability.image_identity.os, "open", controlled_open)

    result, report, _ = run_report(
        capsys, [cold], [warm], readback_image=readback
    )

    assert result == 2
    assert report["readback_image"]["status"] == "image-identity-invalid"
    assert "synthetic unreadable image" in report["readback_image"]["identity_error"]
    assert report["failure_reasons"] == ["readback-image-identity-invalid"]


def test_readback_hash_must_equal_supplied_image_sha256(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A valid but different digest cannot identify the read-back artifact."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    readback = tmp_path / "readback.img"
    readback.write_bytes(VALID_READBACK_BYTES)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        image_sha256="b" * 64,
        readback_image=readback,
    )

    assert result == 2
    assert report["readback_image"]["sha256"] == VALID_SHA256
    assert report["readback_image"]["hash_match"] is False
    assert report["readback_image"]["status"] == "hash-mismatch"
    assert report["failure_reasons"] == [
        "readback-image-hash-mismatch",
        "staged-image-hash-mismatch",
    ]


def test_staged_and_readback_images_require_the_build_marker(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """An artifact without the serial identity cannot bind the boot logs."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    readback = tmp_path / "readback.img"
    artifact = b"image-with-no-build-marker"
    readback.write_bytes(artifact)
    artifact_sha256 = hashlib.sha256(artifact).hexdigest()

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        image_sha256=artifact_sha256,
        readback_image=readback,
    )

    assert result == 2
    assert report["readback_image"]["hash_match"] is False
    assert report["readback_image"]["marker_occurrence_count"] == 0
    assert report["readback_image"]["status"] == "image-identity-invalid"
    assert "readback-image-identity-invalid" in report["failure_reasons"]


def test_repeated_identical_marker_bytes_fail_closed(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A second normalization candidate is invalid even when text matches."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    artifact = append_to_uimage(
        VALID_READBACK_BYTES,
        b"\x00" + VALID_BUILD_MARKER.encode("utf-8"),
    )
    readback = tmp_path / "readback.img"
    readback.write_bytes(artifact)

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        image_sha256=hashlib.sha256(artifact).hexdigest(),
        readback_image=readback,
    )

    assert result == 2
    assert report["result"] == "FAIL"
    assert report["readback_image"]["conflicting_marker_count"] == 0
    assert report["readback_image"]["status"] == "image-identity-invalid"
    assert "readback-image-identity-invalid" in report["failure_reasons"]


@pytest.mark.parametrize(
    ("conflict_role", "expected_reason"),
    [
        ("staged", "staged-image-identity-invalid"),
        ("readback", "readback-image-identity-invalid"),
    ],
)
def test_distinct_canonical_marker_in_either_artifact_fails_closed(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    conflict_role: str,
    expected_reason: str,
) -> None:
    """A stale canonical identity cannot hide beside the expected marker."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    clean = VALID_READBACK_BYTES
    conflicting = append_to_uimage(
        clean, b"\x00" + STALE_BUILD_MARKER.encode("utf-8")
    )
    staged = tmp_path / "staged.img"
    readback = tmp_path / "readback.img"
    if conflict_role == "staged":
        staged.write_bytes(conflicting)
        readback.write_bytes(clean)
        expected_sha256 = hashlib.sha256(conflicting).hexdigest()
    else:
        staged.write_bytes(clean)
        readback.write_bytes(conflicting)
        expected_sha256 = hashlib.sha256(clean).hexdigest()

    result, report, _ = run_report(
        capsys,
        [cold],
        [warm],
        image_sha256=expected_sha256,
        staged_image=staged,
        readback_image=readback,
    )

    assert result == 2
    assert expected_reason in report["failure_reasons"]
    conflicted = report[f"{conflict_role}_image"]
    assert conflicted["status"] == "image-identity-invalid"


def test_conflicting_marker_at_eof_fails_complete_image_identity(
    tmp_path: pathlib.Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A structurally valid envelope cannot hide a second marker at EOF."""

    artifact = append_to_uimage(
        VALID_READBACK_BYTES,
        b"\x00abcde" + STALE_BUILD_MARKER.encode("utf-8"),
    )
    path = tmp_path / "conflict-boundary.img"
    path.write_bytes(artifact)
    monkeypatch.setattr(repeatability, "READBACK_CHUNK_BYTES", 7)

    record = repeatability.assess_readback_image(
        path, hashlib.sha256(artifact).hexdigest(), VALID_BUILD_MARKER
    )

    assert record["status"] == "image-identity-invalid"


def test_valid_sealed_marker_near_payload_eof_is_verified(
    tmp_path: pathlib.Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The fixed marker remains verifiable near the declared payload EOF."""

    artifact = VALID_READBACK_BYTES
    path = tmp_path / "boundary.img"
    path.write_bytes(artifact)
    monkeypatch.setattr(repeatability, "READBACK_CHUNK_BYTES", 7)

    record = repeatability.assess_readback_image(
        path, hashlib.sha256(artifact).hexdigest(), VALID_BUILD_MARKER
    )

    assert record["status"] == "verified"
    assert record["marker_occurrence_count"] == 1
