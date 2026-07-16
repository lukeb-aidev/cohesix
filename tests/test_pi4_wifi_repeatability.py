# Author: Lukas Bower
# Purpose: Test fail-closed Pi 4 WiFi cold and warm boot repeatability scoring.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4_wifi_repeatability.py."""

from __future__ import annotations

import importlib.util
import hashlib
import json
import pathlib
import sys

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

VALID_BUILD_MARKER = (
    "[BUILD] abc123 2026-07-16T00:00:00Z "
    "features=[kernel:1 bootstrap-trace:1 serial-console:1 net:1 "
    "net-console:1 qemu-driver-task-smoke:0]"
)
STALE_BUILD_MARKER = (
    "[BUILD] deadbeef 2026-07-15T00:00:00Z "
    "features=[kernel:1 bootstrap-trace:1 serial-console:1 net:1 "
    "net-console:1 qemu-driver-task-smoke:0]"
)
VALID_READBACK_BYTES = (
    b"cohesix-image-prefix\x00"
    + VALID_BUILD_MARKER.encode("utf-8")
    + b"\x00cohesix-image-suffix"
)
VALID_SHA256 = hashlib.sha256(VALID_READBACK_BYTES).hexdigest()


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
    for token, marker in zip(tokens, selected_markers, strict=True):
        lines.append(f"BOOT {token}")
        if marker is not None:
            lines.append(marker)
    content = "\n".join(lines)
    if lines:
        content += "\n"
    path.write_text(content, encoding="utf-8")
    return path


def run_report(
    capsys: pytest.CaptureFixture[str],
    cold_logs: list[pathlib.Path],
    warm_logs: list[pathlib.Path],
    *,
    image_sha256: str = VALID_SHA256,
    build_marker: str = VALID_BUILD_MARKER,
    staged_image: pathlib.Path | None = None,
    readback_image: pathlib.Path | None = None,
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

    argv: list[str] = []
    for path in cold_logs:
        argv.extend(("--cold-log", str(path)))
    for path in warm_logs:
        argv.extend(("--warm-log", str(path)))
    argv.extend(("--image-sha256", image_sha256))
    argv.extend(("--staged-image", str(staged_image)))
    argv.extend(("--readback-image", str(readback_image)))
    argv.extend(("--build-marker", build_marker))
    if output is not None:
        argv.extend(("--output", str(output)))

    result = repeatability.main(argv)
    rendered = capsys.readouterr().out
    return result, json.loads(rendered), rendered


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
    assert report["readback_image"] == {
        "conflicting_marker_count": 0,
        "distinct_marker_line_sha256": [
            hashlib.sha256(VALID_BUILD_MARKER.encode("utf-8")).hexdigest()
        ],
        "hash_match": True,
        "marker_occurrence_count": 1,
        "path": str(tmp_path / "readback.img"),
        "sha256": VALID_SHA256,
        "size_bytes": len(VALID_READBACK_BYTES),
        "status": "verified",
        "verified": True,
    }
    assert report["staged_image"] == {
        "conflicting_marker_count": 0,
        "distinct_marker_line_sha256": [
            hashlib.sha256(VALID_BUILD_MARKER.encode("utf-8")).hexdigest()
        ],
        "hash_match": True,
        "marker_occurrence_count": 1,
        "path": str(tmp_path / "staged.img"),
        "sha256": VALID_SHA256,
        "size_bytes": len(VALID_READBACK_BYTES),
        "status": "verified",
        "verified": True,
    }
    assert report["staged_readback_binding"] == {
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
    assert report["failure_reasons"] == ["image-sha256-invalid"]
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
    original_open = pathlib.Path.open

    def controlled_open(
        path: pathlib.Path, *args: object, **kwargs: object
    ) -> object:
        if path == readback:
            raise PermissionError("synthetic unreadable image")
        return original_open(path, *args, **kwargs)

    monkeypatch.setattr(pathlib.Path, "open", controlled_open)

    result, report, _ = run_report(
        capsys, [cold], [warm], readback_image=readback
    )

    assert result == 2
    assert report["readback_image"]["status"] == "unreadable"
    assert report["failure_reasons"] == ["readback-image-unreadable"]


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
    assert report["readback_image"]["hash_match"] is True
    assert report["readback_image"]["marker_occurrence_count"] == 0
    assert report["readback_image"]["status"] == "marker-absent"
    assert report["failure_reasons"] == [
        "readback-image-build-marker-absent",
        "staged-image-build-marker-absent",
    ]


def test_repeated_identical_marker_bytes_remain_valid(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Linker duplication of one exact identity is not conflicting evidence."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    artifact = (
        VALID_BUILD_MARKER.encode("utf-8")
        + b"\x00"
        + VALID_BUILD_MARKER.encode("utf-8")
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

    assert result == 0
    assert report["result"] == "PASS"
    assert report["readback_image"]["marker_occurrence_count"] == 2
    assert report["readback_image"]["conflicting_marker_count"] == 0
    assert report["readback_image"]["distinct_marker_line_sha256"] == [
        hashlib.sha256(VALID_BUILD_MARKER.encode("utf-8")).hexdigest()
    ]
    assert report["readback_image"]["status"] == "verified"


@pytest.mark.parametrize(
    ("conflict_role", "expected_reason"),
    [
        ("staged", "staged-image-build-marker-conflict"),
        ("readback", "readback-image-build-marker-conflict"),
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
    conflicting = clean + b"\x00" + STALE_BUILD_MARKER.encode("utf-8")
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
    assert conflicted["conflicting_marker_count"] == 1
    assert conflicted["status"] == (
        "marker-conflict" if conflicted["hash_match"] else "hash-mismatch"
    )


def test_conflicting_marker_scanner_covers_chunk_boundary_and_eof(
    tmp_path: pathlib.Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Canonical conflict discovery retains an EOF marker split by reads."""

    artifact = (
        VALID_BUILD_MARKER.encode("utf-8")
        + b"\x00abcde"
        + STALE_BUILD_MARKER.encode("utf-8")
    )
    path = tmp_path / "conflict-boundary.img"
    path.write_bytes(artifact)
    monkeypatch.setattr(repeatability, "READBACK_CHUNK_BYTES", 7)

    record = repeatability.assess_readback_image(
        path, hashlib.sha256(artifact).hexdigest(), VALID_BUILD_MARKER
    )

    assert record["status"] == "marker-conflict"
    assert record["marker_occurrence_count"] == 1
    assert record["conflicting_marker_count"] == 1


def test_marker_scanner_counts_cross_chunk_and_eof_match(
    tmp_path: pathlib.Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Streaming identity scans retain matches split at chunk boundaries."""

    marker = VALID_BUILD_MARKER.encode("utf-8")
    artifact = b"abcde" + marker
    path = tmp_path / "boundary.img"
    path.write_bytes(artifact)
    monkeypatch.setattr(repeatability, "READBACK_CHUNK_BYTES", 7)

    record = repeatability.assess_readback_image(
        path, hashlib.sha256(artifact).hexdigest(), VALID_BUILD_MARKER
    )

    assert record["status"] == "verified"
    assert record["marker_occurrence_count"] == 1
