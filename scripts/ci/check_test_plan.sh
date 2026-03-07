#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Validate docs/TEST_PLAN.md hashes, command alignment, and scripted stage references.
# Copyright 2026 Lukas Bower

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
doc_path="${repo_root}/docs/TEST_PLAN.md"

python3 - "$repo_root" "$doc_path" <<'PY'
import hashlib
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
doc = pathlib.Path(sys.argv[2])
text = doc.read_text()
pattern = re.compile(r'^- `([^`]+)` — `sha256:([0-9a-f]{64})`$', re.M)
entries = pattern.findall(text)
if not entries:
    print("no hash entries found in docs/TEST_PLAN.md", file=sys.stderr)
    sys.exit(1)

errors = 0
for rel_path, expected in entries:
    path = root / rel_path
    if not path.is_file():
        print(f"missing file: {rel_path}", file=sys.stderr)
        errors += 1
        continue
    data = path.read_bytes()
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected:
        print(f"hash mismatch: {rel_path}", file=sys.stderr)
        print(f"  expected: {expected}", file=sys.stderr)
        print(f"  actual:   {actual}", file=sys.stderr)
        errors += 1

required_snippets = [
    "## Mandatory Agent Execution Contract",
    "Defect resolution is mandatory before progression.",
    "scripts/ci/test_plan_run.sh",
    "scripts/ci/test_plan_stage_01_integrity.sh",
    "scripts/ci/test_plan_stage_02_host_fast.sh",
    "scripts/ci/test_plan_stage_03_qemu_tcp_regression.sh",
    "scripts/ci/test_plan_stage_04_rest_multiplexer.sh",
    "scripts/ci/test_plan_stage_05_due_diligence.sh",
    "scripts/cohsh/run_regression_batch.sh",
    "scripts/cohsh/REST_regression_batch.sh",
    "scripts/ci/due_diligence_gate.sh",
    "scripts/pi4-image-build.sh --manifest out/manifests/root_task_resolved.json",
    "scripts/uboot/qemu-uboot-smoke.sh --net user",
    "scripts/cohesix-build-run.sh --no-run --cargo-target aarch64-unknown-none",
    "cargo test -p tests",
    "cargo test --workspace",
]
for snippet in required_snippets:
    if snippet not in text:
        print(f"missing required TEST_PLAN entry: {snippet}", file=sys.stderr)
        errors += 1

inline_commands = re.findall(r'`([^`]+)`', text)
for command in inline_commands:
    if re.search(r'(^|\s)python(\s|$)', command) and "python3" not in command:
        print(
            f"non-portable python command in docs/TEST_PLAN.md: `{command}` (use python3)",
            file=sys.stderr,
        )
        errors += 1

if errors:
    sys.exit(1)
print("test plan integrity checks ok")
PY
