# CLASSIFICATION: COMMUNITY
# Filename: test_batch_tools.py v0.1
# Author: Lukas Bower
# Date Modified: 2029-01-26
"""Unit tests for Cohesix batch tooling utilities."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

from tools.annotate_batch import annotate_metadata
from tools.trace_diff import diff_snapshots, format_summary


def run_command(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, check=True, text=True, capture_output=True)


def test_simulate_replay_and_annotate(tmp_path: Path) -> None:
    origin = "test://unit"
    batch_dir = tmp_path
    result = run_command([
        "tools/simulate_batch.sh",
        "--size",
        "2",
        "--origin",
        origin,
        "--outdir",
        str(batch_dir),
    ])
    assert "BATCH_DIR=" in result.stdout

    docs_dir = batch_dir / "docs"
    metadata_path = batch_dir / "METADATA.md"
    hydration_log = batch_dir / "hydration.log"
    staging_dir = batch_dir / "staging"

    assert docs_dir.exists()
    assert metadata_path.exists()
    assert hydration_log.exists()

    subprocess.run(["tools/validate_batch.sh", str(docs_dir)], check=True)
    subprocess.run(["tools/replay_batch.sh", str(hydration_log)], check=True)

    doc_names = sorted(path.name for path in docs_dir.glob("*.md"))
    for name in doc_names:
        assert (staging_dir / name).exists()

    annotate_metadata(metadata_path, doc_names, origin, size=len(doc_names))
    table_lines = metadata_path.read_text(encoding="utf-8").splitlines()
    for doc in doc_names:
        matching = [line for line in table_lines if line.startswith("|") and doc in line]
        assert matching, f"metadata row missing for {doc}"
        cells = [cell.strip() for cell in matching[0].strip().split("|")[1:-1]]
        assert cells[4] == str(len(doc_names))
        assert cells[5] == origin


def test_perf_log_records_summary(tmp_path: Path) -> None:
    log_path = tmp_path / "perf.json"
    command = [
        "tools/perf_log.sh",
        "--build-cmd",
        "echo build",
        "--boot-cmd",
        "echo boot",
        "--log-file",
        str(log_path),
        "--tag",
        "pytest",
    ]
    subprocess.run(command, check=True)

    data = json.loads(log_path.read_text(encoding="utf-8"))
    assert data["build"]["status"] == "success"
    assert data["boot"]["status"] == "success"
    build_log = log_path.parent / "pytest_build.log"
    boot_log = log_path.parent / "pytest_boot.log"
    assert build_log.exists()
    assert boot_log.exists()


def test_trace_diff_reports_changes(tmp_path: Path) -> None:
    baseline = tmp_path / "baseline"
    target = tmp_path / "target"
    (baseline / "alpha").mkdir(parents=True)
    (target / "alpha").mkdir(parents=True)

    (baseline / "alpha" / "file.txt").write_text("one\n", encoding="utf-8")
    (target / "alpha" / "file.txt").write_text("one\nupdated\n", encoding="utf-8")
    (baseline / "old.log").write_text("legacy\n", encoding="utf-8")
    (target / "alpha" / "new.log").write_text("fresh\n", encoding="utf-8")

    diff = diff_snapshots(baseline, target)
    assert Path("alpha/file.txt") in diff.changed
    assert Path("alpha/new.log") in diff.added
    assert Path("old.log") in diff.removed

    summary = format_summary(diff)
    assert "Added    : 1" in summary
    assert "Removed  : 1" in summary
    assert "Changed  : 1" in summary
    assert "## Diff for alpha/file.txt" in summary
