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

VALID_BUILD_MARKER = "[BUILD] cohesix-test-build-123"
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
            summaries.append(
                {
                    "blockers": boot_blockers,
                    "boot": boot_index,
                    "gates": {
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
    lines: list[str] = []
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
    readback_image: pathlib.Path | None = None,
    output: pathlib.Path | None = None,
) -> tuple[int, dict[str, object], str]:
    """Run the CLI entry point and parse its deterministic JSON output."""

    if readback_image is None:
        evidence_logs = [*cold_logs, *warm_logs]
        assert evidence_logs
        readback_image = evidence_logs[0].parent / "readback.img"
        readback_image.write_bytes(VALID_READBACK_BYTES)

    argv: list[str] = []
    for path in cold_logs:
        argv.extend(("--cold-log", str(path)))
    for path in warm_logs:
        argv.extend(("--warm-log", str(path)))
    argv.extend(("--image-sha256", image_sha256))
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
        "identity_role": "external-readback",
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
        "hash_match": True,
        "marker_occurrence_count": 1,
        "path": str(tmp_path / "readback.img"),
        "sha256": VALID_SHA256,
        "size_bytes": len(VALID_READBACK_BYTES),
        "status": "verified",
        "verified": True,
    }
    assert report["identity_binding"]["valid"] is True
    assert report["classes"]["cold"]["counts"]["passing_wifi_slices"] == 10
    assert report["classes"]["warm"]["counts"]["passing_wifi_slices"] == 10
    assert len(report["classes"]["cold"]["logs"]) == 2
    assert len(report["classes"]["warm"]["logs"]) == 2
    assert output.read_text(encoding="utf-8") == rendered


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
        "identity_role": "external-readback",
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
        ("cohesix-test-build-123", VALID_BUILD_MARKER),
    ],
)
def test_build_marker_argument_accepts_exact_line_or_unambiguous_payload(
    argument: str,
    expected: str,
) -> None:
    """The preferred full line and a plain payload normalize deterministically."""

    assert repeatability.exact_build_marker(argument) == expected


@pytest.mark.parametrize("argument", ["", " ", "[BUILD]", "x [BUILD] y", "x\n"])
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
    assert report["failure_reasons"] == ["readback-image-hash-mismatch"]


@pytest.mark.parametrize(
    ("artifact", "status", "reason", "occurrences"),
    [
        (
            b"image-with-no-build-marker",
            "marker-absent",
            "readback-image-build-marker-absent",
            0,
        ),
        (
            VALID_BUILD_MARKER.encode("utf-8")
            + b"\x00"
            + VALID_BUILD_MARKER.encode("utf-8"),
            "marker-ambiguous",
            "readback-image-build-marker-ambiguous",
            2,
        ),
    ],
)
def test_readback_requires_one_unambiguous_build_marker(
    tmp_path: pathlib.Path,
    capsys: pytest.CaptureFixture[str],
    artifact: bytes,
    status: str,
    reason: str,
    occurrences: int,
) -> None:
    """Absent and duplicate marker bytes cannot bind serial boots to an image."""

    cold = write_log(tmp_path / "cold.log", ["pass"] * 10)
    warm = write_log(tmp_path / "warm.log", ["pass"] * 10)
    readback = tmp_path / "readback.img"
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
    assert report["readback_image"]["marker_occurrence_count"] == occurrences
    assert report["readback_image"]["status"] == status
    assert report["failure_reasons"] == [reason]
