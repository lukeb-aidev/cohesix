#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Inventory Mermaid diagrams from tracked Markdown for 26c audit evidence.
# Copyright 2026 Lukas Bower

"""Inventory Mermaid fenced blocks from an explicit Markdown file list."""

from __future__ import annotations

import argparse
import csv
import pathlib
import re
from dataclasses import dataclass


FENCE_RE = re.compile(r"^\s*```\s*mermaid\s*$", re.IGNORECASE)
END_FENCE_RE = re.compile(r"^\s*```\s*$")
HEADING_RE = re.compile(r"^(#{1,6})\s+(.+?)\s*$")


@dataclass(frozen=True)
class MermaidBlock:
    path: str
    block_index: int
    line_start: int
    line_end: int
    heading: str
    diagram_type: str
    disposition: str
    owner: str
    evidence_source: str
    github_status: str
    update_rule: str


def classify_markdown(path: str) -> tuple[str, str, str]:
    if path.startswith("third_party/"):
        return (
            "vendored reference",
            "external-reference-owner",
            "inventory only; update through upstream vendor import",
        )
    if path.startswith("seL4/"):
        return (
            "external reference mirror",
            "kernel-reference-owner",
            "inventory only; update through accepted seL4 reference refresh",
        )
    if path.startswith("releases/"):
        return (
            "release snapshot",
            "release-owner",
            "update only through release-cut flow",
        )
    if path.startswith("docs/snippets/"):
        return (
            "generated snippet",
            "generator-owner",
            "update only through coh-rtc or source generator",
        )
    if path == "docs/nist/REPORT.md":
        return (
            "generated report",
            "generator-owner",
            "update only through NIST report generator",
        )
    if path.startswith("docs/audit/"):
        return (
            "live audit register",
            "audit-owner",
            "append or update only with matching evidence command",
        )
    return (
        "human-edited canonical source",
        "docs-owner",
        "edit directly with generated/as-built evidence",
    )


def diagram_type(lines: list[str]) -> str:
    for line in lines:
        stripped = line.strip()
        if not stripped or stripped.startswith("%%"):
            continue
        return stripped.split(maxsplit=1)[0]
    return "empty"


def iter_blocks(path: pathlib.Path, rel_path: str) -> list[MermaidBlock]:
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    heading = "(document)"
    blocks: list[MermaidBlock] = []
    in_block = False
    start = 0
    block_lines: list[str] = []
    disposition, owner, update_rule = classify_markdown(rel_path)

    for index, line in enumerate(lines, start=1):
        if not in_block:
            heading_match = HEADING_RE.match(line)
            if heading_match:
                heading = heading_match.group(2).strip()
            if FENCE_RE.match(line):
                in_block = True
                start = index
                block_lines = []
            continue

        if END_FENCE_RE.match(line):
            blocks.append(
                MermaidBlock(
                    path=rel_path,
                    block_index=len(blocks) + 1,
                    line_start=start,
                    line_end=index,
                    heading=heading,
                    diagram_type=diagram_type(block_lines),
                    disposition=disposition,
                    owner=owner,
                    evidence_source=rel_path,
                    github_status="pending-check",
                    update_rule=update_rule,
                )
            )
            in_block = False
            continue
        block_lines.append(line)

    if in_block:
        raise SystemExit(f"unterminated mermaid fence in {rel_path}:{start}")
    return blocks


def read_markdown_list(path: pathlib.Path) -> list[str]:
    entries = []
    for line in path.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped:
            entries.append(stripped)
    return entries


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--markdown-list", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    list_path = pathlib.Path(args.markdown_list)
    out_path = pathlib.Path(args.out)
    root = pathlib.Path.cwd()
    rows: list[MermaidBlock] = []
    for rel_path in read_markdown_list(list_path):
        path = root / rel_path
        if not path.is_file():
            raise SystemExit(f"missing Markdown file listed for Mermaid inventory: {rel_path}")
        rows.extend(iter_blocks(path, rel_path))

    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", encoding="utf-8", newline="") as handle:
        handle.write("# Author: Lukas Bower\n")
        handle.write("# Purpose: Inventory tracked Markdown Mermaid diagrams for Milestone 26c.\n")
        handle.write("# Copyright 2026 Lukas Bower\n")
        writer = csv.writer(handle)
        writer.writerow(
            [
                "path",
                "block_index",
                "line_start",
                "line_end",
                "heading",
                "diagram_type",
                "disposition",
                "owner",
                "evidence_source",
                "github_status",
                "update_rule",
            ]
        )
        for row in rows:
            writer.writerow(
                [
                    row.path,
                    row.block_index,
                    row.line_start,
                    row.line_end,
                    row.heading,
                    row.diagram_type,
                    row.disposition,
                    row.owner,
                    row.evidence_source,
                    row.github_status,
                    row.update_rule,
                ]
            )
    print(f"wrote {len(rows)} Mermaid inventory row(s) to {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
