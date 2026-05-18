#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Validate docs/TEST_PLAN.md hashes, command alignment, and scripted stage references.
# Copyright 2026 Lukas Bower

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
doc_path="${repo_root}/docs/TEST_PLAN.md"
stage_02_path="${repo_root}/scripts/ci/test_plan_stage_02_host_fast.sh"

python3 - "$repo_root" "$doc_path" "$stage_02_path" <<'PY'
import hashlib
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
doc = pathlib.Path(sys.argv[2])
stage_02 = pathlib.Path(sys.argv[3])
text = doc.read_text()
stage_02_text = stage_02.read_text()
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
    "Respect the current target boundary.",
    "As built, `scripts/ci/test_plan_run.sh` does not accept `--target`.",
    "Target-qualified `--target qemu|pi4` staged PASS semantics are Milestone 26c work, not part of the current runner.",
    "Stage 04 requires both a gateway URL and a request auth token",
    "TP_PYTHON_BIN=$REPO/.venv/bin/python3",
    "Stage 03 QEMU/TCP regression batch remains mandatory",
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
    "cargo check -p swarmui --bin swarmui",
    "python3 scripts/ci/check_swarmui_dependencies.py",
    "cargo test -p host-ticket-agent",
    "cargo test -p tests",
    "python3 scripts/ci/check_driver_test_coverage.py",
    "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::rtl8139",
    "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::virtio",
    "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::pci",
    "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::virtio_mmio",
    "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::uart",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::bcmgenet",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::cyw43",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::bcmgenet",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_pcie",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat_pi4::driver_coverage_tests::",
    "cargo test -p root-task --no-default-features --features cache-maintenance --test cache_maintenance",
    "--features release-qemu",
    "--features release-pi4",
    "cargo test -p root-task --no-default-features --features net-console --lib net:: -- --nocapture",
    "cargo test --workspace",
    "pytest tests/test_pi4_trace_normalize.py",
    "pytest tests/test_pi4_gate_proof.py",
]
for snippet in required_snippets:
    if snippet not in text:
        print(f"missing required TEST_PLAN entry: {snippet}", file=sys.stderr)
        errors += 1

stale_snippets = [
    "TCP/QEMU batch remains a local bring-up tool only",
]
for snippet in stale_snippets:
    if snippet in text:
        print(f"stale TEST_PLAN wording remains: {snippet}", file=sys.stderr)
        errors += 1

required_stage_02_commands = [
    "cargo check -p swarmui --bin swarmui",
    "python3 scripts/ci/check_swarmui_dependencies.py",
    "cargo test -p host-ticket-agent",
    "cargo test -p tests",
    "python3 scripts/ci/check_driver_test_coverage.py",
    "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::rtl8139",
    "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::virtio",
    "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::pci",
    "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::virtio_mmio",
    "cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::uart",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::bcmgenet",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::cyw43",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::bcmgenet",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_pcie",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat_pi4::driver_coverage_tests::",
    "cargo test -p root-task --no-default-features --features cache-maintenance --test cache_maintenance",
    "--features release-qemu",
    "--features release-pi4",
    "cargo test -p root-task --no-default-features --features net-console --lib net:: -- --nocapture",
    "cargo test --workspace",
    "pytest tests/test_pi4_trace_normalize.py",
    "pytest tests/test_pi4_gate_proof.py",
]
for command in required_stage_02_commands:
    if command not in stage_02_text:
        print(
            f"missing required stage 02 command in {stage_02.relative_to(root)}: {command}",
            file=sys.stderr,
        )
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
