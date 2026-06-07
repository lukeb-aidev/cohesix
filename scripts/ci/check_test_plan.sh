#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Validate docs/TEST_PLAN.md hashes, command alignment, and scripted stage references.
# Copyright 2026 Lukas Bower

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
doc_path="${repo_root}/docs/TEST_PLAN.md"
stage_02_path="${repo_root}/scripts/ci/test_plan_stage_02_host_fast.sh"
due_diligence_path="${repo_root}/scripts/ci/due_diligence_gate.sh"

python3 - "$repo_root" "$doc_path" "$stage_02_path" "$due_diligence_path" <<'PY'
import hashlib
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
doc = pathlib.Path(sys.argv[2])
stage_02 = pathlib.Path(sys.argv[3])
due_diligence = pathlib.Path(sys.argv[4])
text = doc.read_text()
stage_02_text = stage_02.read_text()
due_diligence_text = due_diligence.read_text()
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
    "TP_STAGE4_GATEWAY_BIND",
    "self-contained local QEMU by default",
    "scripts/pi4-image-build.sh --manifest configs/root_task_pi4_uboot_aarch64.toml",
    "scripts/pi4_gate_proof.sh --log <fresh-pi4-serial.log> --require-usb-ready --require-wired-ready --require-driver-task-proof --require-input-responsive",
    "scripts/pi4_gate_proof.sh --log <fresh-pi4-serial.log> --require-ready",
    "DRIVER_TASK_CONTRACTS",
    "DRIVER_TASK_DEDICATED_READY=yes",
    "DRIVER_TASK_SERIAL_DEDICATED",
    "DRIVER_TASK_USB_DEDICATED",
    "DRIVER_TASK_DISPLAY_DEDICATED",
    "DRIVER_TASK_NET_DEDICATED",
    "DRIVER_TASK_SDIO_DEDICATED=yes",
    "DRIVER_TASK_PCIE_DEDICATED",
    "DRIVER_TASK_FAILED_COUNT=0",
    "DRIVER_TASK_AFFINITY_CONFIGURED",
    "DRIVER_TASK_AFFINITY_APPLIED",
    "DRIVER_TASK_AFFINITY_MANIFEST_PROOF",
    "DRIVER_TASK_VSPACE_PROOF",
    "DRIVER_TASK_POINTER_FREE_IPC_PROOF",
    "DRIVER_TASK_OWNER_STATE_PROOF",
    "DRIVER_TASK_OWNER_STATE ... descriptor=present root_pointer=no",
    "serial-console",
    "usb-keyboard",
    "hdmi-text",
    "genet-nic",
    "cyw43-wifi",
    "sdio-host",
    "pcie-root",
    "live_tcb=yes",
    "hot_path=dedicated",
    "declared `max_service_us` budgets are diagnostic",
    "SERIAL_RESPONSIVE_PROOF=yes",
    "USB_BURST_DROPS=0",
    "HDMI_RESPONSIVE_PROOF=yes",
    "NET_ACTIVE=wired",
    "ROOT_PROMPT_SEEN=yes",
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
    "cargo test -p pi4-driver-abi",
    "cargo test -p pi4-driver-runtime",
    "cargo check -p pi4-driver-runtime --target aarch64-unknown-none",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::driver_task",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::poll_io_obeys_driver_task_budget",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::flush_tx_backpressure_does_not_count_as_budget_overrun",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::runtime_serial_write_moves_bytes_without_root_port_pointer",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::runtime_serial_poll_moves_rx_bytes_without_root_port_pointer",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::serial_input_skips_ready_network_data_poll_for_driver_task_turn",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::serial_input_defers_buffered_network_console_lines_for_driver_task_turn",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_pcie",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat_pi4::driver_coverage_tests::",
    "cargo test -p root-task --no-default-features --features cache-maintenance --test cache_maintenance",
    "--features release-qemu",
    "--features release-pi4",
    "cargo test -p root-task --no-default-features --features net-console --lib net:: -- --nocapture",
    "CARGO_INCREMENTAL=0 cargo test --workspace --exclude swarmui",
    "CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings",
    "CARGO_INCREMENTAL=0 cargo check --workspace",
    "pytest tests/test_pi4_trace_normalize.py",
    "pytest tests/test_pi4_gate_proof.py",
]
for snippet in required_snippets:
    if snippet not in text:
        print(f"missing required TEST_PLAN entry: {snippet}", file=sys.stderr)
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
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::driver_task",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::poll_io_obeys_driver_task_budget",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::flush_tx_backpressure_does_not_count_as_budget_overrun",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::runtime_serial_write_moves_bytes_without_root_port_pointer",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::runtime_serial_poll_moves_rx_bytes_without_root_port_pointer",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::serial_input_skips_ready_network_data_poll_for_driver_task_turn",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::serial_input_defers_buffered_network_console_lines_for_driver_task_turn",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_pcie",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::",
    "cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat_pi4::driver_coverage_tests::",
    "cargo test -p root-task --no-default-features --features cache-maintenance --test cache_maintenance",
    "--features release-qemu",
    "--features release-pi4",
    "cargo test -p root-task --no-default-features --features net-console --lib net:: -- --nocapture",
    "CARGO_INCREMENTAL=0 cargo test --workspace --exclude swarmui",
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

required_due_diligence_commands = [
    "env CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings",
    "env CARGO_INCREMENTAL=0 cargo check --workspace",
    "env CARGO_INCREMENTAL=0 cargo test --workspace",
]
for command in required_due_diligence_commands:
    if command not in due_diligence_text:
        print(
            f"missing required due diligence command in {due_diligence.relative_to(root)}: {command}",
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
