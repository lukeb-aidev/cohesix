<!-- Author: Lukas Bower -->
<!-- Purpose: Record Milestone 26c Mermaid GitHub compatibility and render evidence. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Mermaid GitHub Render Audit

Status: `PASS-ACTIVE / RELEASE-WARNINGS`

## Inventory

- Tracked Markdown files: `186`
- Mermaid blocks inventoried: `84`
- Inventory source: `docs/audit/M26C_MERMAID_INVENTORY.csv`
- Markdown list: `out/audit/m26c_markdown_inventory.txt`

## Commands

| Command | Result |
| --- | --- |
| `scripts/ci/mermaid_inventory.py --markdown-list out/audit/m26c_markdown_inventory.txt --out docs/audit/M26C_MERMAID_INVENTORY.csv` | PASS, 84 blocks |
| `scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt` | PASS for active surfaces, 32 release-snapshot warnings |
| `scripts/ci/render_mermaid_github.sh --markdown-list out/audit/m26c_markdown_inventory.txt --out out/audit/m26c-mermaid-rendered` | PASS extraction; SVG render skipped because `mmdc` was not installed |

## Canonical Fixes Applied

Raw HTML line-break labels were removed from canonical Mermaid diagrams in:

- `docs/HOST_TOOLS.md`
- `docs/NETWORK_CONFIG.md`
- `docs/USE_CASES.md`

The edited labels preserve the same as-built relationships and only remove
GitHub-hostile HTML syntax.

## Release Snapshot Warnings

The checker reports 32 warnings under `releases/**` for release-derived copies
of diagrams that still contain raw HTML labels. These snapshots are
inventory-only for 26c unless the release-cut flow is run. They were not
hand-edited for style.

## Render Evidence

The extraction manifest and Mermaid source files live under
`out/audit/m26c-mermaid-rendered/`. Because `mmdc` was unavailable, SVG render
proof remains an environment gap rather than a diagram syntax failure for active
surfaces.
