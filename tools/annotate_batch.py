# CLASSIFICATION: COMMUNITY
# Filename: annotate_batch.py v0.1
# Author: Lukas Bower
# Date Modified: 2029-01-26
"""Metadata batch annotator for Cohesix documentation tables."""

from __future__ import annotations

import argparse
import logging
import os
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import List, Sequence

LOGGER = logging.getLogger("annotate_batch")


@dataclass
class AnnotationResult:
    """Summary of metadata annotations applied to a Markdown table."""

    updated: bool
    batch_size: int
    touched_entries: List[str]
    metadata_path: Path


def _resolve_temp_dir() -> Path:
    for key in ("COHESIX_ENS_TMP", "COHESIX_TRACE_TMP", "TMPDIR"):
        candidate = os.environ.get(key)
        if candidate:
            path = Path(candidate)
            path.mkdir(parents=True, exist_ok=True)
            return path
    return Path(tempfile.gettempdir())


def _atomic_write(target: Path, content: str) -> None:
    temp_dir = _resolve_temp_dir()
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=temp_dir, delete=False) as handle:
        handle.write(content)
        temp_path = Path(handle.name)
    temp_path.chmod(target.stat().st_mode if target.exists() else 0o644)
    temp_path.replace(target)


def _format_cell(value: str, width: int) -> str:
    display = f" {value.strip()} "
    if len(display) < width:
        display = display.ljust(width)
    return display


def _apply_annotations(lines: List[str], entries: Sequence[str], origin: str, batch_size: int) -> tuple[List[str], List[str]]:
    normalized_targets = {entry.strip(): entry.strip() for entry in entries}
    matched: List[str] = []
    updated_lines = []

    for line in lines:
        if not line.startswith("|") or line.strip().startswith("|-"):
            updated_lines.append(line)
            continue
        parts = line.rstrip("\n").split("|")
        if len(parts) < 3:
            updated_lines.append(line)
            continue
        cells = parts[1:-1]
        stripped_cells = [cell.strip() for cell in cells]
        filename = stripped_cells[0]
        if filename in normalized_targets:
            matched.append(filename)
            widths = [len(cell) for cell in cells]
            if len(cells) < 6:
                LOGGER.warning("Row for %s does not contain BATCH columns; skipping", filename)
                updated_lines.append(line)
                continue
            cells[4] = _format_cell(str(batch_size), widths[4])
            cells[5] = _format_cell(origin, widths[5])
            rebuilt = "|" + "|".join(cells) + "|"
            updated_lines.append(rebuilt)
        else:
            updated_lines.append(line)

    return updated_lines, matched


def annotate_metadata(metadata_path: Path, entries: Sequence[str], origin: str, size: int | None, dry_run: bool = False) -> AnnotationResult:
    if not entries:
        raise ValueError("At least one entry must be provided")
    if not origin:
        raise ValueError("BATCH_ORIGIN cannot be empty")
    if size is not None and size <= 0:
        raise ValueError("BATCH_SIZE must be positive when provided")

    metadata_path = metadata_path.expanduser().resolve()
    LOGGER.debug("Loading metadata file: %s", metadata_path)
    if not metadata_path.exists():
        raise FileNotFoundError(metadata_path)

    original = metadata_path.read_text(encoding="utf-8").splitlines()
    unique_entries = list(dict.fromkeys(entries))
    batch_size = size or len(unique_entries)
    updated, matched = _apply_annotations(original, unique_entries, origin, batch_size)

    if not matched:
        raise ValueError("None of the requested entries were found in the metadata table")

    updated_text = "\n".join(updated) + "\n"
    if not dry_run and updated != original:
        _atomic_write(metadata_path, updated_text)
    elif not dry_run:
        LOGGER.info("No changes required; metadata already up to date")

    return AnnotationResult(
        updated=updated != original,
        batch_size=batch_size,
        touched_entries=matched,
        metadata_path=metadata_path,
    )


def _parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Populate BATCH metadata columns in Markdown tables")
    parser.add_argument("entries", nargs="+", help="Document entries to update (match Filename column values)")
    parser.add_argument(
        "--metadata",
        default="workspace/docs/community/governance/METADATA.md",
        help="Path to the Markdown metadata table",
    )
    parser.add_argument("--origin", required=True, help="Value to use for the BATCH_ORIGIN column")
    parser.add_argument("--size", type=int, help="Explicit batch size; defaults to the number of entries")
    parser.add_argument("--dry-run", action="store_true", help="Do not write changes; print the result")
    parser.add_argument("--quiet", action="store_true", help="Suppress informational logging")
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = _parse_args(argv or sys.argv[1:])
    logging.basicConfig(level=logging.WARNING if args.quiet else logging.INFO, format="[annotate-batch] %(message)s")
    metadata_path = Path(args.metadata)

    try:
        result = annotate_metadata(metadata_path, args.entries, args.origin, args.size, dry_run=args.dry_run)
    except Exception as exc:  # pylint: disable=broad-except
        LOGGER.error("%s", exc)
        return 1

    if args.dry_run:
        preview = metadata_path.read_text(encoding="utf-8").splitlines()
        updated, _ = _apply_annotations(preview, args.entries, args.origin, result.batch_size)
        print("\n".join(updated))
    else:
        LOGGER.info(
            "Updated %s (batch size %s) for entries: %s",
            result.metadata_path,
            result.batch_size,
            ", ".join(result.touched_entries),
        )
    return 0


if __name__ == "__main__":  # pragma: no cover
    sys.exit(main())
