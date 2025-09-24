# CLASSIFICATION: COMMUNITY
# Filename: trace_diff.py v0.1
# Author: Lukas Bower
# Date Modified: 2029-01-26
"""Compare validator snapshots stored beneath history/snapshots."""

from __future__ import annotations

import argparse
import logging
import os
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, List, Optional, Sequence
import difflib

LOGGER = logging.getLogger("trace_diff")


@dataclass
class SnapshotDiff:
    """Collection of differences between two snapshot directories."""

    baseline: Path
    target: Path
    added: List[Path] = field(default_factory=list)
    removed: List[Path] = field(default_factory=list)
    changed: Dict[Path, str] = field(default_factory=dict)
    unchanged: List[Path] = field(default_factory=list)


def resolve_snapshot(base_dir: Path, identifier: str) -> Path:
    """Resolve a snapshot identifier to an existing directory."""

    candidate = Path(identifier)
    search_paths = []
    if candidate.is_absolute():
        search_paths.append(candidate)
    else:
        search_paths.append(base_dir / candidate)
        search_paths.append(Path.cwd() / candidate)
    seen = set()
    for path in search_paths:
        if path in seen:
            continue
        seen.add(path)
        expanded = path.expanduser()
        if expanded.exists():
            resolved = expanded.resolve()
            if not resolved.is_dir():
                raise NotADirectoryError(f"Snapshot path {resolved} is not a directory")
            return resolved
    raise FileNotFoundError(f"Snapshot '{identifier}' not found relative to {base_dir}")


def collect_files(root: Path) -> Dict[Path, Path]:
    mapping: Dict[Path, Path] = {}
    for path in sorted(root.rglob("*")):
        if path.is_file():
            mapping[path.relative_to(root)] = path
    return mapping


def diff_files(baseline: Path, target: Path, rel_path: Path) -> Optional[str]:
    baseline_path = baseline / rel_path
    target_path = target / rel_path
    try:
        baseline_text = baseline_path.read_text(encoding="utf-8").splitlines()
        target_text = target_path.read_text(encoding="utf-8").splitlines()
    except UnicodeDecodeError:
        if baseline_path.read_bytes() == target_path.read_bytes():
            return None
        return "<binary differs>"
    if baseline_text == target_text:
        return None
    diff_lines = difflib.unified_diff(
        baseline_text,
        target_text,
        fromfile=str(rel_path),
        tofile=str(rel_path),
        lineterm="",
    )
    return "\n".join(diff_lines)


def diff_snapshots(baseline: Path, target: Path) -> SnapshotDiff:
    baseline_files = collect_files(baseline)
    target_files = collect_files(target)

    baseline_keys = set(baseline_files)
    target_keys = set(target_files)

    result = SnapshotDiff(baseline=baseline, target=target)
    result.added = sorted(target_keys - baseline_keys)
    result.removed = sorted(baseline_keys - target_keys)

    for rel_path in sorted(baseline_keys & target_keys):
        diff_text = diff_files(baseline, target, rel_path)
        if diff_text is None:
            result.unchanged.append(rel_path)
        else:
            result.changed[rel_path] = diff_text
    return result


def select_tmp_root() -> Path:
    for key in ("COHESIX_TRACE_TMP", "COHESIX_ENS_TMP", "TMPDIR"):
        candidate = os.environ.get(key)
        if candidate:
            path = Path(candidate)
            path.mkdir(parents=True, exist_ok=True)
            return path
    return Path(tempfile.gettempdir())


def atomic_write(path: Path, content: str) -> None:
    tmp_dir = select_tmp_root()
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=tmp_dir, delete=False) as handle:
        handle.write(content)
        temp_path = Path(handle.name)
    temp_path.replace(path)


def format_summary(diff: SnapshotDiff) -> str:
    lines: List[str] = []
    lines.append(f"Baseline : {diff.baseline}")
    lines.append(f"Target   : {diff.target}")
    lines.append(f"Added    : {len(diff.added)}")
    lines.append(f"Removed  : {len(diff.removed)}")
    lines.append(f"Changed  : {len(diff.changed)}")
    lines.append(f"Unchanged: {len(diff.unchanged)}")

    if diff.added:
        lines.append("\n# Added files")
        lines.extend(f"+ {path}" for path in diff.added)
    if diff.removed:
        lines.append("\n# Removed files")
        lines.extend(f"- {path}" for path in diff.removed)
    if diff.changed:
        lines.append("\n# Changed files")
        for rel_path, diff_text in diff.changed.items():
            lines.append(f"\n## Diff for {rel_path}")
            lines.append(diff_text)
    return "\n".join(lines).strip() + "\n"


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Compare validator snapshot directories")
    parser.add_argument("baseline", help="Baseline snapshot name or path")
    parser.add_argument("target", help="Target snapshot name or path")
    parser.add_argument(
        "--snapshots-dir",
        default=Path("/history/snapshots"),
        type=Path,
        help="Directory that contains snapshot archives (default: /history/snapshots)",
    )
    parser.add_argument("--output", type=Path, help="Write diff summary to this path")
    parser.add_argument("--fail-on-diff", action="store_true", help="Exit with code 2 when differences are detected")
    parser.add_argument("--quiet", action="store_true", help="Suppress informational logging")
    return parser.parse_args(argv)


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parse_args(list(argv) if argv is not None else sys.argv[1:])
    logging.basicConfig(level=logging.WARNING if args.quiet else logging.INFO, format="[trace-diff] %(message)s")

    snapshots_dir = args.snapshots_dir.expanduser()
    try:
        baseline = resolve_snapshot(snapshots_dir, args.baseline)
        target = resolve_snapshot(snapshots_dir, args.target)
    except (FileNotFoundError, NotADirectoryError) as exc:
        LOGGER.error("%s", exc)
        return 1

    diff = diff_snapshots(baseline, target)
    summary = format_summary(diff)
    print(summary, end="")

    if args.output:
        out_path = args.output.expanduser()
        out_path.parent.mkdir(parents=True, exist_ok=True)
        atomic_write(out_path, summary)
        LOGGER.info("Wrote diff output to %s", out_path)

    if args.fail_on_diff and (diff.added or diff.removed or diff.changed):
        return 2
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
