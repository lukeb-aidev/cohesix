<!-- Author: Lukas Bower -->
<!-- Purpose: Record Milestone 26c Mermaid inventory, GitHub compatibility, and render evidence. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Mermaid GitHub Render Audit

Status: `PASS / 76-OF-76-RENDERED / RELEASE-SNAPSHOT-WARNINGS`

## Inventory

- Markdown files in the intended change set: `204`
- Mermaid blocks inventoried: `76`
- Inventory compatibility status: `44 pass`, `32 warning-render-pass`, `0 pending`
- README-linked suite diagrams: `10` blocks across `9` documents, reduced from `18` blocks
- Inventory: `docs/audit/M26C_MERMAID_INVENTORY.csv`
- Markdown list: `out/audit/m26c-doc-remediation-markdown.txt`
- Render evidence: `out/audit/m26c-doc-remediation-mermaid-rendered`

## Validation

| Command | Result |
| --- | --- |
| `scripts/ci/mermaid_inventory.py --markdown-list out/audit/m26c-doc-remediation-markdown.txt --out docs/audit/M26C_MERMAID_INVENTORY.csv` | PASS; 76 blocks inventoried exactly once. |
| `scripts/ci/check_mermaid_github.sh --markdown-list out/audit/m26c-doc-remediation-markdown.txt` | PASS for every active surface; 32 release-snapshot warnings. |
| `npm exec --offline --package=@mermaid-js/mermaid-cli -- scripts/ci/render_mermaid_github.sh --markdown-list out/audit/m26c-doc-remediation-markdown.txt --out out/audit/m26c-doc-remediation-mermaid-rendered` | PASS with Mermaid CLI 11.16.0; 76 source blocks produced 76 SVG files. |

GitHub renders Mermaid from fenced `mermaid` blocks. The active diagrams avoid
custom initialization directives, raw HTML labels, external links/assets,
theme CSS, and experimental diagram types. This matches GitHub's documented
[diagram syntax](https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/creating-diagrams)
and the repository's stricter compatibility checker.

## README-Linked Suite Diagrams

| File | Diagram responsibility | As-built evidence boundary |
| --- | --- | --- |
| `README.md` | Host, target, kernel, worker, and driver overview | Architecture, selected profiles, and current source tree |
| `docs/ARCHITECTURE.md` | Build-host, operator-host, and target trust boundaries | Manifest compiler, root-task, host NineDoor, and target adapter sources |
| `docs/INTERFACES.md` | TCP authentication, attach, and bounded stream sequence | Console server, event pump, and `NineDoorBridge` |
| `docs/DRIVERS.md` | HAL admission and isolated driver-runtime service turn | HAL, driver ABI, and runtime-init descriptors |
| `docs/HOST_TOOLS.md` | Mutually exclusive direct and gateway console ownership | Current host-tool transports and gateway topology |
| `docs/GPU_NODES.md` | Host GPU/executor boundary and bounded VM projection | GPU bridge, worker role, and live-versus-simulation paths |
| `docs/HARDWARE_BRINGUP.md` | Build-to-benchmark evidence ladder | Image scripts, Test Plan, and current proof policy |
| `docs/BOOT_REFERENCE.md` | QEMU and Pi 4 boot-path convergence | Selected seL4 and Pi U-Boot handoff profiles |
| `docs/USE_CASES.md` | Control-plane boundary plus the scoped AI-agent admission and receipt sequence | Current source and host integration contracts; no deployment acceptance claim |

Every suite diagram was reviewed for semantic ownership as well as syntax. The
removed diagrams duplicated schemas, lifecycle prose, or aspirational
deployment patterns better owned by focused text and generated snippets.
The hardware evidence ladder retains all nine independent proof states: build,
flash, readback, boot, saved policy, device/network, console, Test Plan, and
benchmark.

## Release Snapshot Warnings

The checker reports 32 raw-HTML-label warnings under `releases/**`. Those files
are immutable release snapshots for this remediation. Editing them would
require the release-cut workflow and a minor-version increment, so they remain
inventory-only and do not weaken the active-doc PASS result. All 32 warning
blocks nevertheless rendered successfully with the same CLI during the
76-block render pass.
