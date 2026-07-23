#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Generate Milestone 26c tracked-Markdown inventory artifacts.
# Copyright 2026 Lukas Bower

"""Generate the 26c Markdown disposition CSV and readable report."""

from __future__ import annotations

import argparse
import csv
import pathlib


def classify(path: str) -> tuple[str, str, str, str]:
    if path.startswith("third_party/"):
        return (
            "vendored reference",
            "external-reference-owner",
            "inventory only; update through upstream vendor import",
            "third-party import",
        )
    if path.startswith("seL4/"):
        return (
            "external reference mirror",
            "kernel-reference-owner",
            "inventory only; update through accepted seL4 reference refresh",
            "external seL4 reference",
        )
    if path.startswith("releases/"):
        return (
            "release snapshot",
            "release-owner",
            "update only through release-cut flow",
            "release package snapshot",
        )
    if path.startswith("docs/snippets/"):
        return (
            "generated snippet",
            "generator-owner",
            "update only through coh-rtc or source generator",
            "generated documentation snippet",
        )
    if path == "docs/nist/REPORT.md":
        return (
            "generated report",
            "generator-owner",
            "update only through NIST report generator",
            "generated compliance report",
        )
    if path.startswith("docs/audit/AUDIT_REPORT_") or path == "docs/audit/M26B_COMPLETION_EVIDENCE.md":
        return (
            "append-only audit evidence",
            "audit-owner",
            "append only with dated evidence; do not rewrite for style",
            "historical audit evidence",
        )
    if path.startswith("docs/audit/"):
        return (
            "live audit register",
            "audit-owner",
            "update only with matching evidence command",
            "audit control surface",
        )
    return (
        "human-edited canonical source",
        "docs-owner",
        "edit directly with generated/as-built evidence",
        "canonical Cohesix-authored documentation",
    )


def read_markdown_list(path: pathlib.Path) -> list[str]:
    return [line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def write_csv(rows: list[tuple[str, str, str, str, str]], out: pathlib.Path) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", encoding="utf-8", newline="") as handle:
        handle.write("# Author: Lukas Bower\n")
        handle.write("# Purpose: Inventory every tracked Markdown file for Milestone 26c disposition control.\n")
        handle.write("# Copyright 2026 Lukas Bower\n")
        writer = csv.writer(handle, lineterminator="\n")
        writer.writerow(["path", "disposition", "owner", "update_rule", "evidence_source"])
        writer.writerows(rows)


def write_report(rows: list[tuple[str, str, str, str, str]], out: pathlib.Path) -> None:
    counts: dict[str, int] = {}
    for _, disposition, _, _, _ in rows:
        counts[disposition] = counts.get(disposition, 0) + 1

    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", encoding="utf-8") as handle:
        handle.write("<!-- Author: Lukas Bower -->\n")
        handle.write("<!-- Purpose: Summarize the Milestone 26c tracked-Markdown inventory and dispositions. -->\n")
        handle.write("<!-- Copyright 2026 Lukas Bower -->\n\n")
        handle.write("# M26C Markdown Inventory\n\n")
        handle.write("This report is derived from `docs/audit/M26C_MARKDOWN_INVENTORY.csv`.\n\n")
        handle.write("## Summary\n\n")
        handle.write(f"- tracked Markdown files: {len(rows)}\n")
        for disposition in sorted(counts):
            handle.write(f"- {disposition}: {counts[disposition]}\n")
        handle.write("\n## Disposition Rules\n\n")
        for disposition in sorted(counts):
            sample = next(row for row in rows if row[1] == disposition)
            handle.write(f"- `{disposition}`: {sample[3]}\n")
        handle.write("\n## Inventory\n\n")
        handle.write("| Path | Disposition | Owner | Update rule |\n")
        handle.write("| --- | --- | --- | --- |\n")
        for path, disposition, owner, update_rule, _ in rows:
            handle.write(f"| `{path}` | {disposition} | {owner} | {update_rule} |\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--markdown-list", required=True)
    parser.add_argument("--csv-out", required=True)
    parser.add_argument("--report-out", required=True)
    args = parser.parse_args()

    rows = []
    for path in read_markdown_list(pathlib.Path(args.markdown_list)):
        disposition, owner, update_rule, evidence_source = classify(path)
        rows.append((path, disposition, owner, update_rule, evidence_source))

    write_csv(rows, pathlib.Path(args.csv_out))
    write_report(rows, pathlib.Path(args.report_out))
    print(f"wrote {len(rows)} Markdown inventory row(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
