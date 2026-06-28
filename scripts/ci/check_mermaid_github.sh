#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Check tracked Markdown Mermaid blocks for GitHub-hostile syntax.
# Copyright 2026 Lukas Bower

set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/ci/check_mermaid_github.sh --markdown-list <path>

Checks Mermaid fenced blocks from an explicit tracked-Markdown list for syntax
that GitHub's online Mermaid renderer does not accept reliably.
USAGE
}

markdown_list=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --markdown-list)
      shift
      [[ $# -gt 0 ]] || {
        echo "--markdown-list requires a value" >&2
        exit 2
      }
      markdown_list="$1"
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

[[ -n "${markdown_list}" ]] || {
  usage >&2
  exit 2
}

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

python3 - "$repo_root" "$markdown_list" <<'PY'
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
markdown_list = pathlib.Path(sys.argv[2])

fence_re = re.compile(r"^\s*```\s*mermaid\s*$", re.I)
end_re = re.compile(r"^\s*```\s*$")
unsupported = [
    ("custom init directive", re.compile(r"^\s*%%\{", re.M)),
    (
        "raw HTML label",
        re.compile(
            r"<\s*/?\s*(br|div|span|p|b|i|strong|em|code|pre|table|ul|ol|li|font)\b[^>]*>",
            re.I,
        ),
    ),
    ("external click/href", re.compile(r"\bclick\b.*\bhref\b", re.I)),
    ("theme CSS", re.compile(r"\bthemeCSS\b")),
    (
        "experimental GitHub Mermaid syntax",
        re.compile(
            r"^\s*(architecture-beta|block-beta|kanban|packet-beta|sankey-beta|xychart-beta)\b",
            re.I | re.M,
        ),
    ),
]

errors = 0
warnings = 0
blocks = 0

def fail_for_path(rel):
    inventory_only_prefixes = ("releases/", "third_party/", "seL4/")
    inventory_only_paths = ("docs/nist/REPORT.md",)
    generated_prefixes = ("docs/snippets/",)
    if rel.startswith(inventory_only_prefixes) or rel.startswith(generated_prefixes):
        return False
    if rel in inventory_only_paths:
        return False
    return True

for rel in markdown_list.read_text(encoding="utf-8").splitlines():
    rel = rel.strip()
    if not rel:
        continue
    path = root / rel
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    in_block = False
    start = 0
    body = []
    for line_no, line in enumerate(lines, start=1):
        if not in_block:
            if fence_re.match(line):
                in_block = True
                start = line_no
                body = []
            continue
        if end_re.match(line):
            blocks += 1
            block_text = "\n".join(body)
            for label, pattern in unsupported:
                if pattern.search(block_text):
                    if fail_for_path(rel):
                        print(f"{rel}:{start}: unsupported Mermaid for GitHub: {label}", file=sys.stderr)
                        errors += 1
                    else:
                        print(
                            f"{rel}:{start}: warning: inventory-only Mermaid has GitHub-hostile syntax: {label}",
                            file=sys.stderr,
                        )
                        warnings += 1
            in_block = False
            continue
        body.append(line)
    if in_block:
        print(f"{rel}:{start}: unterminated Mermaid fence", file=sys.stderr)
        errors += 1

if errors:
    sys.exit(1)
print(f"mermaid GitHub compatibility checks ok: {blocks} block(s), warnings={warnings}")
PY
