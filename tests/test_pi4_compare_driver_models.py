# Author: Lukas Bower
# Purpose: Test Pi driver-log and qualified QEMU/Pi benchmark comparison.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4_compare_driver_models.py."""

from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import pathlib
import subprocess
import sys
import time
import types

import pytest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
MODULE_PATH = REPO_ROOT / "scripts" / "pi4_compare_driver_models.py"

spec = importlib.util.spec_from_file_location(
    "pi4_compare_driver_models", MODULE_PATH
)
comparator = importlib.util.module_from_spec(spec)
assert spec.loader is not None
sys.modules[spec.name] = comparator
spec.loader.exec_module(comparator)

HARNESS_MODULE_PATH = REPO_ROOT / "scripts" / "rest_perf_harness.py"
harness_spec = importlib.util.spec_from_file_location(
    "rest_perf_harness_for_comparator", HARNESS_MODULE_PATH
)
harness = importlib.util.module_from_spec(harness_spec)
assert harness_spec.loader is not None
sys.modules[harness_spec.name] = harness
harness_spec.loader.exec_module(harness)


def _old_good_log() -> list[str]:
    return [
        "U-Boot 2026.01-dirty",
        "[cohesix:root-task] Cohesix boot: root-task online",
        "[Cohesix] Root console ready (type 'help' for commands)",
        "cohesix> help",
        "[mark] root-console.start.ok",
        "SERIAL_ECHO p95_us=700 max_gap_us=900",
        "DRIVER_TASK_OWNER_STATE contract=hdmi-text hot_path=hdmi-text "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "HDMI_RESPONSIVE max_gap_ms=9 mirrored_bytes=256 visible=yes",
        "DRIVER_TASK_RESOURCE_INIT contract=pcie-root stage=hal-prep "
        "status=ok",
        "DRIVER_TASK_RING_CALL_BEGIN contract=pcie-root request=1 "
        "opcode=engine-init",
        "DRIVER_TASK_RING_CALL_RETURN contract=pcie-root request=1 "
        "opcode=engine-init status=ok",
        "DRIVER_TASK_OWNER_STATE contract=usb-local-seat "
        "hot_path=usb-keyboard owner_state=driver-owned",
        "[local-seat] keyboard route=usb-keyboard parser=shared",
        "[local-seat] runtime keyboard first-byte read=1 ascii=0x54 key=0x17 "
        "source=linked-runtime-hid",
        "[local-seat] pi4 keyboard runtime proof result=online gate=10 "
        "source=linked-runtime-hid first_byte_source=linked-runtime-hid",
        "USB_BURST bytes=16 drops=0",
        "DRIVER_TASK_SDIO_DEDICATED=yes",
        "DRIVER_TASK_OWNER_STATE contract=cyw43455 hot_path=cyw43-wifi "
        "owner_state=driver-owned",
        "[pi4-wifi] sdio function-ready fn=2 ready=0x06",
        "[cyw43] control-plane ready",
        "[dhcp] lease bound ip=192.168.86.154/24 gateway=192.168.86.1",
        "wifi: net backend=cyw43 mode=dhcp active=wifi dhcp_phase=bound",
        "OK NETTEST detail=pass scope=serial-local",
    ]


def _new_halted_log() -> list[str]:
    return [
        "U-Boot 2026.01-dirty",
        "[cohesix:root-task] Cohesix boot: root-task online",
        "[mark] root-console.start.begin",
        "DRIVER_TASK_RESOURCE_INIT contract=pcie-root stage=hal-prep "
        "status=ok",
        "DRIVER_TASK_RING_CALL_BEGIN contract=hdmi-text request=9 "
        "opcode=render-frame",
        "DRIVER_TASK_RING_CALL_TIMEOUT contract=hdmi-text request=9 "
        "attempts=4096",
        "[local-seat] usb keyboard unavailable "
        "detail=pcie-vl805-config-contract-missing",
        "wifi: snapshot source=live stage=console-dump-state "
        "exact_error=cyw43-ht-clock-timeout-before-function2",
        "halting...",
        "Kernel entry via Interrupt, irq 27",
    ]


def _partial_old_log() -> list[str]:
    return [
        "U-Boot 2026.01-dirty",
        "[cohesix:root-task] Cohesix boot: root-task online",
        "[Cohesix] Root console ready (type 'help' for commands)",
        "cohesix> wifi diag",
        "wifi: snapshot source=live stage=console-dump-state "
        "detail=net-disabled",
    ]


def _write_log(
    tmp_path: pathlib.Path, name: str, lines: list[str]
) -> pathlib.Path:
    path = tmp_path / name
    path.write_text("\n".join(lines), encoding="utf-8")
    return path


def _parse_env(output: str) -> dict[str, str]:
    pairs: dict[str, str] = {}
    for line in output.splitlines():
        key, value = line.split("=", 1)
        pairs[key] = value
    return pairs


def test_old_good_vs_new_halted_reports_regression(
    tmp_path: pathlib.Path,
) -> None:
    old_path = _write_log(tmp_path, "old.log", _old_good_log())
    new_path = _write_log(tmp_path, "new.log", _new_halted_log())

    result = subprocess.run(
        [
            sys.executable,
            str(MODULE_PATH),
            "--old",
            str(old_path),
            "--new",
            str(new_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    fields = _parse_env(result.stdout)

    assert result.returncode == 0
    assert fields["OLD_SERIAL_PROMPT_SEEN"] == "yes"
    assert fields["OLD_HDMI_VISIBLE_SEEN"] == "yes"
    assert fields["OLD_PCIE_ENGINE_INIT_RETURN_SEEN"] == "yes"
    assert fields["OLD_USB_FIRST_BYTE_SEEN"] == "yes"
    assert fields["OLD_WIFI_DHCP_SEEN"] == "yes"
    assert fields["NEW_SERIAL_PROMPT_SEEN"] == "no"
    assert fields["NEW_HALT_SEEN"] == "yes"
    assert fields["NEW_HALT_REASON"] == "kernel-halt"
    assert fields["NEW_RING_CALL_OUTSTANDING"] == "1"
    assert fields["NEW_RING_CALL_TIMEOUTS"] == "1"
    assert fields["NEW_RING_CALL_TIMEOUT_CONTRACTS"] == "hdmi-text"
    assert fields["NEW_USB_BLOCKER"] == "pcie-vl805-config-contract-missing"
    assert fields["NEW_WIFI_BLOCKER"] == "ht-clock-timeout"
    assert fields["COMPARISON_VERDICT"] == "regression"
    assert "serial_prompt" in fields["COMPARISON_REGRESSIONS"]
    assert "ring_call_outstanding" in fields["COMPARISON_REGRESSIONS"]
    assert fields["MILESTONE_COMPARISON_SUMMARY"].startswith(
        "regression: old=interactive-local-seat-network new=halted"
    )


def test_new_driver_model_advancement_keeps_stable_keys(
    tmp_path: pathlib.Path,
) -> None:
    old_path = _write_log(tmp_path, "old.log", _partial_old_log())
    new_path = _write_log(tmp_path, "new.log", _old_good_log())

    result = subprocess.run(
        [
            sys.executable,
            str(MODULE_PATH),
            "--old",
            str(old_path),
            "--new",
            str(new_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    fields = _parse_env(result.stdout)

    assert result.returncode == 0
    assert fields["COMPARISON_VERDICT"] == "advancement"
    assert "wifi_blocker" in fields["COMPARISON_ADVANCEMENTS"]
    assert "hdmi_visible" in fields["COMPARISON_ADVANCEMENTS"]
    assert "usb_first_byte" in fields["COMPARISON_ADVANCEMENTS"]
    assert "wifi_dhcp" in fields["COMPARISON_ADVANCEMENTS"]
    for prefix in ("OLD", "NEW"):
        assert f"{prefix}_HDMI_MAP_SEEN" in fields
        assert f"{prefix}_PCIE_HAL_PREP_SEEN" in fields
        assert f"{prefix}_USB_KEYBOARD_ROUTE_SEEN" in fields
        assert f"{prefix}_WIFI_NET_DIAG_SEEN" in fields


def test_latest_diagnostics_do_not_credit_usb_burst_or_dhcp_next_as_acceptance(
    tmp_path: pathlib.Path,
) -> None:
    old_path = _write_log(tmp_path, "old.log", _old_good_log())
    new_path = _write_log(
        tmp_path,
        "new.log",
        [
            "U-Boot 2026.01-dirty",
            "[Cohesix] Root console ready (type 'help' for commands)",
            "USB_BURST bytes=16 drops=0",
            "usb: runtime_gate keyboard=no first_report=no first_byte=no "
            "proof_gate=5 target_gate=10 next=device-descriptor "
            "blocker=address-device-failed",
            "wifi: gate 8 name=firmware-channel status=blocked "
            "evidence=dependency=not-reached next=dhcp-bound",
            "wifi: evidence sdio_cmd53 func=1 addr=0x0001a000 len=256 "
            "increment=yes block_mode=no mode=byte-narrow op=2 "
            "source=owner-terminal",
            "wifi: evidence sdio_status "
            "descriptor_status=cyw43-firmware-retry-exhausted "
            "transfer_stage=response transfer_status=0x000800 "
            "transfer_reason=sdio-r5-response r5=0x0800 "
            "retry=byte-narrow-fallback-exhausted host=0x06 clock=0x5007",
            "wifi: evidence sdio_payload first=0x11 last=0x22 xor=0x33 "
            "sum=0x00004444 owner_window=sdio-shared-8192",
        ],
    )

    result = subprocess.run(
        [
            sys.executable,
            str(MODULE_PATH),
            "--old",
            str(old_path),
            "--new",
            str(new_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    fields = _parse_env(result.stdout)

    assert result.returncode == 0
    assert fields["NEW_USB_FIRST_BYTE_SEEN"] == "no"
    assert fields["NEW_USB_BLOCKER_SEEN"] == "yes"
    assert fields["NEW_USB_BLOCKER"] == "address-device-failed"
    assert fields["NEW_WIFI_DHCP_SEEN"] == "no"
    assert fields["NEW_WIFI_BLOCKER_SEEN"] == "yes"
    assert fields["NEW_WIFI_BLOCKER"] == "cyw43-firmware-retry-exhausted"
    assert fields["COMPARISON_VERDICT"] == "regression"


def test_unsourced_usb_first_byte_does_not_count_as_linked_runtime_proof(
    tmp_path: pathlib.Path,
) -> None:
    old_path = _write_log(tmp_path, "old.log", _partial_old_log())
    new_path = _write_log(
        tmp_path,
        "new.log",
        [
            "U-Boot 2026.01-dirty",
            "[Cohesix] Root console ready (type 'help' for commands)",
            "[local-seat] keyboard route=usb-keyboard parser=shared",
            "[local-seat] runtime keyboard first-byte read=1 ascii=0x54 key=0x17",
            "[local-seat] pi4 keyboard runtime proof result=online gate=10 "
            "source=first-byte",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "proof_gate=10 target_gate=10 blocker=none first_byte_source=local-seat-queue",
        ],
    )

    result = subprocess.run(
        [
            sys.executable,
            str(MODULE_PATH),
            "--old",
            str(old_path),
            "--new",
            str(new_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    fields = _parse_env(result.stdout)

    assert result.returncode == 0
    assert fields["NEW_USB_KEYBOARD_ROUTE_SEEN"] == "yes"
    assert fields["NEW_USB_FIRST_BYTE_SEEN"] == "no"
    assert "usb" in fields["NEW_MILESTONE_STATE"].split("-", 1)[1].split("+")


def test_latest_diagnostics_classify_cyw43_descriptor_invalid(
    tmp_path: pathlib.Path,
) -> None:
    old_path = _write_log(tmp_path, "old.log", _old_good_log())
    new_path = _write_log(
        tmp_path,
        "new.log",
        [
            "U-Boot 2026.01-dirty",
            "[Cohesix] Root console ready (type 'help' for commands)",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-chunk op=2 flags=0x0000 target=0x00200000 "
            "payload_off=4096 payload_len=8192 total_len=609309 detail=21258 "
            "reason=cyw43-descriptor-invalid result=0x00000004",
            "wifi: evidence cyw43 stage=cyw43-firmware-chunk op=2 flags=0x0000 "
            "target=0x00200000 payload_off=4096 payload_len=8192 "
            "total_len=609309 detail=0x530a reason=cyw43-descriptor-invalid "
            "result=0x00000004",
        ],
    )

    result = subprocess.run(
        [
            sys.executable,
            str(MODULE_PATH),
            "--old",
            str(old_path),
            "--new",
            str(new_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    fields = _parse_env(result.stdout)

    assert result.returncode == 0
    assert fields["NEW_WIFI_BLOCKER_SEEN"] == "yes"
    assert fields["NEW_WIFI_BLOCKER"] == "cyw43-descriptor-invalid"


def test_pair_recovery_diagnostic_is_a_wifi_blocker(
    tmp_path: pathlib.Path,
) -> None:
    """Canonical pair recovery remains visible after terminal retirement."""

    old_path = _write_log(tmp_path, "old.log", _old_good_log())
    new_path = _write_log(
        tmp_path,
        "new.log",
        [
            "U-Boot 2026.01-dirty",
            "[Cohesix] Root console ready (type 'help' for commands)",
            "cohesix> wifi diag",
            "wifi: gate 8 name=firmware-channel status=fail "
            "evidence=exact=pair-recovery-required control_stage=none "
            "sdhci=unknown reply_mode=unknown dependency=pair-recovery-required "
            "next=dhcp-bound",
            "wifi: next_action=run-pair-recovery-after-terminal-retirement "
            "blocker=pair-recovery-required proof_gate=7 target_gate=10",
        ],
    )

    result = subprocess.run(
        [
            sys.executable,
            str(MODULE_PATH),
            "--old",
            str(old_path),
            "--new",
            str(new_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    fields = _parse_env(result.stdout)

    assert result.returncode == 0
    assert fields["NEW_WIFI_BLOCKER_SEEN"] == "yes"
    assert fields["NEW_WIFI_BLOCKER"] == "pair-recovery-required"


def test_usb_runtime_ring_busy_overrides_stale_link_blocker(
    tmp_path: pathlib.Path,
) -> None:
    old_path = _write_log(tmp_path, "old.log", _old_good_log())
    new_path = _write_log(
        tmp_path,
        "new.log",
        [
            "U-Boot 2026.01-dirty",
            "[Cohesix] Root console ready (type 'help' for commands)",
            "[local-seat] usb keyboard unavailable detail=link-or-rc-not-ready",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=runtime-ring-submit status=busy "
            "acceptance=no code=none detail=none result=none frame_len=0",
            "usb: runtime_gate keyboard=no first_report=no first_byte=no "
            "proof_gate=3 target_gate=10 next=command-ring-ready "
            "blocker=command-event-ring-not-proven detail=0x0201 result=0x03000001",
        ],
    )

    result = subprocess.run(
        [
            sys.executable,
            str(MODULE_PATH),
            "--old",
            str(old_path),
            "--new",
            str(new_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    fields = _parse_env(result.stdout)

    assert result.returncode == 0
    assert fields["NEW_USB_BLOCKER_SEEN"] == "yes"
    assert fields["NEW_USB_BLOCKER"] == "runtime-ring-submit-busy"


def test_latest_boot_slice_ignores_stale_good_prefix() -> None:
    stale_then_halted = _old_good_log() + _new_halted_log()

    summary = comparator.summarize_log("new", stale_then_halted)

    assert summary.boot_slice_start == len(_old_good_log())
    assert summary.serial_prompt_seen is False
    assert summary.halt_seen is True
    assert summary.ring_call_outstanding == 1


def _benchmark_report(
    target: str,
    transport: str,
    *,
    ok_ops_per_s: float,
    captured_unix_s: int = 10_000,
    latency_s: float = 0.01,
) -> dict[str, object]:
    proof_class = "qemu" if target == "qemu" else "fresh-pi"
    source_sha256 = hashlib.sha256(b"same-source").hexdigest()
    workload = {
        "mode": "simulate",
        "population_mode": "executable",
        "control_write_outcome": "admitted",
        "scenario": "mixed",
        "seed": 2608,
        "entropy": 5.0,
        "workers_min": 256,
        "workers_max": 256,
        "worker_cap": 256,
        "multi_hive": False,
        "hives": 1,
        "workers_per_hive": 256,
        "intensity_min": 8,
        "intensity_max": 8,
        "base_rps": 4.0,
        "target_rps_min": 8192.0,
        "target_rps_max": 8192.0,
        "duration_s": 120.0,
        "ramp_step_secs": 30,
        "max_inflight_configured": 32,
        "tail_bytes": 4096,
        "telemetry_reference_chunk_bytes": 16 * 1024 * 1024,
        "include_lifecycle": False,
        "auto_approve": True,
        "strict_control_errors": True,
        "transient_retries": False,
        "error_budget_rate": 0.01,
        "request_timeout_s": 10.0,
        "request_auth_enabled": True,
        "role": "queen",
    }
    hashes = {
        "source_sha256": source_sha256,
        "manifest_sha256": hashlib.sha256(f"{target}-manifest".encode()).hexdigest(),
        "image_sha256": hashlib.sha256(f"{target}-image".encode()).hexdigest(),
        "root_image_sha256": hashlib.sha256(f"{target}-root".encode()).hexdigest(),
        "target_session_sha256": hashlib.sha256(f"{target}-session".encode()).hexdigest(),
        "runtime_evidence_sha256": hashlib.sha256(f"{target}-runtime".encode()).hexdigest(),
        "network_evidence_sha256": hashlib.sha256(
            f"{target}-{transport}-network".encode()
        ).hexdigest(),
    }
    identity = {
        "target": target,
        "transport": transport,
        "proof_class": proof_class,
        **hashes,
        "component_acceptance_sha256": (
            hashlib.sha256(b"qemu-component-acceptance").hexdigest()
            if target == "qemu"
            else None
        ),
        "captured_unix_s": captured_unix_s,
    }
    provenance = {
        "schema": comparator.BENCHMARK_PROVENANCE_SCHEMA,
        "qualification": "target-qualified",
        **identity,
        "performance_qualification_sha256": comparator.canonical_json_sha256(
            identity
        ),
        "workload_sha256": comparator.canonical_json_sha256(workload),
    }
    topology_sha256 = hashlib.sha256(f"{target}-topology".encode()).hexdigest()
    roles = ("worker-heartbeat", "worker-gpu", "worker-lora")
    inventory = {
        "tcbs": 1,
        "scheduling_contexts": 1,
        "reply_objects": 1,
        "vspaces": 1,
        "cnodes": 1,
        "page_tables": 8,
        "asids": 1,
        "frames": 12,
        "endpoints": 1,
        "notifications": 0,
        "fault_caps": 1,
        "timeout_fault_caps": 1,
        "cspace_slots": 32,
        "untyped_bytes": 131_072,
    }
    snapshots = {}
    for phase in ("pre", "post"):
        snapshots[phase] = {
            "workers": [
                {
                    "role": role,
                    "slot": 0,
                    "lease_epoch": 10 + index,
                    "supervisor_generation": (
                        120
                        if phase == "post" and role == "worker-heartbeat"
                        else 20 + index
                    ),
                    "cap_generation": 30 + index,
                    "worker": (
                        "worker-4"
                        if phase == "post" and role == "worker-heartbeat"
                        else f"worker-{index + 1}"
                    ),
                    "lifecycle": "ready",
                    "artifact": "verified",
                    "receipt": (
                        "none" if role == "worker-heartbeat" else "confirmed"
                    ),
                    "execution_proof": proof_class,
                    "ready_sequence": (
                        140
                        if phase == "post" and role == "worker-heartbeat"
                        else 40 + index
                    ),
                    "control_sequence": 50 + index,
                    "receipt_sequence": (
                        61 + index
                        if phase == "post" and role != "worker-heartbeat"
                        else 60 + index
                    ),
                    "completion_sequence": (
                        71 + index
                        if phase == "post" and role != "worker-heartbeat"
                        else 70 + index
                    ),
                    "image_sha256": hashlib.sha256(role.encode()).hexdigest(),
                    "core": comparator.EXECUTABLE_ROLE_CORES[role],
                    "scheduling_context": {"budget_us": 0, "period_us": 0},
                    "object_inventory": dict(inventory),
                }
                for index, role in enumerate(roles)
            ],
            "ready_census": {
                "maximum_live_tasks": 256,
                "discovered": 256,
                "ready": 256,
                "topology_sha256": topology_sha256,
            },
            "proc": {},
        }
    executable_state: dict[str, object] = {
        "topology_sha256": topology_sha256,
        "target_session": {
            "manifest_sha256": hashes["manifest_sha256"],
            "root_image_sha256": hashes["root_image_sha256"],
            "worker_archive_sha256": hashlib.sha256(
                b"target-neutral-worker-archive"
            ).hexdigest(),
            "worker_image_manifest_sha256": hashlib.sha256(
                b"target-neutral-worker-manifest"
            ).hexdigest(),
            "worker_abi_sha256": hashlib.sha256(
                b"target-neutral-worker-abi"
            ).hexdigest(),
        },
        **snapshots,
    }
    if target == "pi4":
        executable_state["target_evidence"] = {
            "schema": comparator.BENCHMARK_TARGET_EVIDENCE_SCHEMA,
            **identity,
            "performance_qualification_sha256": provenance[
                "performance_qualification_sha256"
            ],
        }
    ok_count = int(round(ok_ops_per_s * float(workload["duration_s"])))
    exact_ok_ops_per_s = ok_count / float(workload["duration_s"])
    return {
        "schema": comparator.BENCHMARK_REPORT_SCHEMA,
        "workload": workload,
        "provenance": provenance,
        "population": {
            "mode": "executable",
            "backend_class": "console-projection",
            "maximum_live_tasks": 256,
            "requested": 256,
            "discovered": 256,
            "ready": 256,
            "proof_class": proof_class,
            "observations": [
                {
                    "requested": 256,
                    "discovered": 256,
                    "ready": 256,
                    "backend_class": "console-projection",
                    "proof_class": proof_class,
                }
            ],
        },
        "throughput": {
            "ops_per_s": exact_ok_ops_per_s,
            "ok_ops_per_s": exact_ok_ops_per_s,
            "err_ops_per_s": 0.0,
        },
        "reliability": {
            "error_rate": 0.0,
            "error_budget_rate": 0.01,
            "error_budget_pass": True,
            "ok": ok_count,
            "err": 0,
            "count": ok_count,
        },
        "backpressure": {
            field: 0 for field in comparator.PARITY_BACKPRESSURE_FIELDS
        },
        "latency": {
            "avg_s": latency_s,
            "min_s": latency_s,
            "max_s": latency_s,
            "p50_s": latency_s,
            "p90_s": latency_s,
            "p95_s": latency_s,
            "p99_s": latency_s,
        },
        "executable_state": executable_state,
    }


def _set_report_errors(report: dict[str, object], err: int) -> None:
    """Keep one fixture's reliability and throughput metrics internally exact."""

    reliability = report["reliability"]
    throughput = report["throughput"]
    workload = report["workload"]
    assert isinstance(reliability, dict)
    assert isinstance(throughput, dict)
    assert isinstance(workload, dict)
    ok = int(reliability["ok"])
    count = ok + err
    duration_s = float(workload["duration_s"])
    error_rate = 0.0 if count == 0 else err / count
    reliability.update(
        {
            "err": err,
            "count": count,
            "error_rate": error_rate,
            "error_budget_pass": error_rate
            <= float(reliability["error_budget_rate"]),
        }
    )
    throughput.update(
        {
            "ops_per_s": count / duration_s,
            "ok_ops_per_s": ok / duration_s,
            "err_ops_per_s": err / duration_s,
        }
    )


def _compare(
    qemu: dict[str, object],
    pi: dict[str, object],
    *,
    reference_unix_s: int = 10_010,
    max_age_secs: int = 60,
    min_throughput_ratio: float = 1.0,
    genet_max_p95_ms: float = 100.0,
    wifi_report: dict[str, object] | None = None,
    wifi_min_ok_ops_per_s: float | None = None,
    wifi_max_p95_ms: float | None = None,
) -> dict[str, object]:
    return comparator.compare_benchmark_reports(
        qemu,
        pi,
        reference_unix_s=reference_unix_s,
        max_age_secs=max_age_secs,
        min_throughput_ratio=min_throughput_ratio,
        genet_max_p95_ms=genet_max_p95_ms,
        qemu_input_sha256=comparator.canonical_json_sha256(qemu),
        pi_input_sha256=comparator.canonical_json_sha256(pi),
        wifi_report=wifi_report,
        wifi_min_ok_ops_per_s=wifi_min_ok_ops_per_s,
        wifi_max_p95_ms=wifi_max_p95_ms,
        wifi_input_sha256=(
            None
            if wifi_report is None
            else comparator.canonical_json_sha256(wifi_report)
        ),
    )


def test_comparator_accepts_real_harness_population_projection() -> None:
    baseline = _benchmark_report("qemu", "qemu", ok_ops_per_s=600.0)
    args = types.SimpleNamespace(
        scenario=None,
        telemetry_reference_chunk_bytes=16 * 1024 * 1024,
        seed=2608,
        entropy=5.0,
        workers_min=256,
        workers_max=256,
        multi_hive=False,
        hives=1,
        workers_per_hive=256,
        intensity_min=8,
        intensity_max=8,
        base_rps=4.0,
        duration_mins=2.0,
        ramp_step_secs=30,
        max_inflight=32,
        tail_bytes=4096,
        include_lifecycle=False,
        auto_approve=True,
        transient_retries=False,
        strict_control_errors=True,
        error_budget_rate=0.01,
        timeout=10.0,
        request_auth_token="test-only-token",
        role="queen",
        population_mode="executable",
    )
    overall = harness.OpStats(
        count=72_000,
        ok=72_000,
        err=0,
        total_s=72.0,
        min_s=0.001,
        max_s=0.001,
        samples=[0.001],
    )
    report = harness.benchmark_report_payload(
        args,
        overall,
        {},
        [],
        256,
        0.0,
        True,
        None,
        {
            "configured_max_inflight": 32,
            "observed_high_water": 32,
            "current_inflight": 0,
            "submitted": 72_000,
            "completed": 72_000,
        },
        population=copy.deepcopy(baseline["population"]),
        executable_state=copy.deepcopy(baseline["executable_state"]),
        target_provenance=copy.deepcopy(baseline["provenance"]),
    )

    validated = comparator.validate_benchmark_report(
        report,
        target="qemu",
        transport="qemu",
        proof_class="qemu",
        reference_unix_s=10_010,
        max_age_secs=60,
    )

    assert validated["population"]["observations"][0]["ready"] == 256


def test_component_acceptance_is_required_for_qemu_and_forbidden_for_pi() -> None:
    """Performance qualification cannot manufacture cross-target acceptance."""

    assert comparator.BENCHMARK_PROVENANCE_SCHEMA.endswith("/v2")
    assert comparator.BENCHMARK_TARGET_EVIDENCE_SCHEMA.endswith("/v2")
    qemu = _benchmark_report("qemu", "qemu", ok_ops_per_s=600.0)
    pi = _benchmark_report("pi4", "genet", ok_ops_per_s=600.0)

    qemu_missing = copy.deepcopy(qemu)
    del qemu_missing["provenance"]["component_acceptance_sha256"]
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="QEMU benchmark requires an exact component-acceptance hash",
    ):
        _compare(qemu_missing, pi)

    qemu_null = copy.deepcopy(qemu)
    qemu_null["provenance"]["component_acceptance_sha256"] = None
    qemu_null["provenance"]["performance_qualification_sha256"] = (
        comparator.canonical_json_sha256(
            comparator.performance_qualification_identity(
                qemu_null["provenance"]
            )
        )
    )
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="QEMU benchmark requires an exact component-acceptance hash",
    ):
        _compare(qemu_null, pi)

    pi_missing = copy.deepcopy(pi)
    del pi_missing["provenance"]["component_acceptance_sha256"]
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="Pi performance qualification requires exact null",
    ):
        _compare(qemu, pi_missing)

    pi_embedded_claim = copy.deepcopy(pi)
    pi_embedded_claim["executable_state"]["target_evidence"][
        "component_acceptance_sha256"
    ] = hashlib.sha256(b"embedded-pi-component-acceptance").hexdigest()
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="Pi report differs from embedded target evidence",
    ):
        _compare(qemu, pi_embedded_claim)

    pi_claim = copy.deepcopy(pi)
    pi_claim["provenance"]["component_acceptance_sha256"] = hashlib.sha256(
        b"forged-pi-component-acceptance"
    ).hexdigest()
    pi_claim["provenance"]["performance_qualification_sha256"] = (
        comparator.canonical_json_sha256(
            comparator.performance_qualification_identity(
                pi_claim["provenance"]
            )
        )
    )
    embedded = pi_claim["executable_state"]["target_evidence"]
    embedded["component_acceptance_sha256"] = pi_claim["provenance"][
        "component_acceptance_sha256"
    ]
    embedded["performance_qualification_sha256"] = pi_claim["provenance"][
        "performance_qualification_sha256"
    ]
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="Pi performance qualification cannot claim component acceptance",
    ):
        _compare(qemu, pi_claim)


def test_genet_parity_pass_excludes_qemu_latency_from_verdict() -> None:
    qemu = _benchmark_report("qemu", "qemu", ok_ops_per_s=600.0, latency_s=0.001)
    pi = _benchmark_report("pi4", "genet", ok_ops_per_s=605.0, latency_s=0.500)

    result = _compare(qemu, pi)

    assert result["verdict"] == "PASS"
    assert result["inputs"]["qemu_sha256"] == comparator.canonical_json_sha256(qemu)
    assert result["freshness"]["qemu"] == {
        "captured_unix_s": 10_000,
        "age_secs": 10,
    }
    assert result["throughput"]["pi_to_qemu_ratio"] > 1.0
    assert result["latency"]["included_in_verdict"] is False
    assert result["latency"]["pi"]["p99_s"] == 0.5
    assert result["latency"]["physical_norm_status"] == "FLAG"


def test_genet_parity_fails_below_qemu_successful_throughput() -> None:
    result = _compare(
        _benchmark_report("qemu", "qemu", ok_ops_per_s=600.0),
        _benchmark_report("pi4", "genet", ok_ops_per_s=599.99),
    )

    assert result["verdict"] == "FAIL"
    assert result["throughput"]["pass"] is False


def test_genet_parity_uses_explicit_ratio_and_existing_error_budgets() -> None:
    qemu = _benchmark_report("qemu", "qemu", ok_ops_per_s=600.0)
    pi = _benchmark_report("pi4", "genet", ok_ops_per_s=630.0)
    result = _compare(qemu, pi, min_throughput_ratio=1.10)
    assert result["verdict"] == "FAIL"
    assert result["throughput"]["minimum_ratio"] == 1.10

    pi = _benchmark_report("pi4", "genet", ok_ops_per_s=660.0)
    _set_report_errors(pi, 20_000)
    result = _compare(qemu, pi, min_throughput_ratio=1.10)
    assert result["verdict"] == "FAIL"
    assert result["errors"]["error_budget_pass"] is False


def test_benchmark_comparator_rejects_workload_and_internal_image_drift() -> None:
    qemu = _benchmark_report("qemu", "qemu", ok_ops_per_s=600.0)
    pi = _benchmark_report("pi4", "genet", ok_ops_per_s=600.0)
    mismatched_workload = copy.deepcopy(pi)
    mismatched_workload["workload"]["seed"] = 27
    provenance = mismatched_workload["provenance"]
    provenance["workload_sha256"] = comparator.canonical_json_sha256(
        mismatched_workload["workload"]
    )
    try:
        _compare(
            qemu,
            mismatched_workload,
        )
    except comparator.BenchmarkComparisonError as exc:
        assert "canonical M26e HIGH" in str(exc)
    else:
        raise AssertionError("mismatched workload must be rejected")

    mismatched_image = copy.deepcopy(pi)
    mismatched_image["executable_state"]["target_session"][
        "root_image_sha256"
    ] = "f" * 64
    try:
        _compare(
            qemu,
            mismatched_image,
        )
    except comparator.BenchmarkComparisonError as exc:
        assert "manifest/image" in str(exc)
    else:
        raise AssertionError("mismatched image session must be rejected")

    mismatched_worker_archive = copy.deepcopy(pi)
    mismatched_worker_archive["executable_state"]["target_session"][
        "worker_archive_sha256"
    ] = "e" * 64
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="target-neutral Worker artifacts",
    ):
        _compare(qemu, mismatched_worker_archive)

    mismatched_role_image = copy.deepcopy(pi)
    for phase in ("pre", "post"):
        mismatched_role_image["executable_state"][phase]["workers"][1][
            "image_sha256"
        ] = "e" * 64
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="Worker role image identities",
    ):
        _compare(qemu, mismatched_role_image)


def test_benchmark_comparator_rejects_forged_metrics_and_shape() -> None:
    qemu = _benchmark_report("qemu", "qemu", ok_ops_per_s=600.0)
    pi = _benchmark_report("pi4", "genet", ok_ops_per_s=600.0)
    forged = copy.deepcopy(pi)
    forged["throughput"]["ok_ops_per_s"] = 60_000.0
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="counts and rates are inconsistent",
    ):
        _compare(qemu, forged)

    missing_latency = copy.deepcopy(pi)
    del missing_latency["latency"]["p95_s"]
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="latency metrics are invalid",
    ):
        _compare(qemu, missing_latency)

    inverted_latency = copy.deepcopy(pi)
    inverted_latency["latency"]["p90_s"] = 0.2
    inverted_latency["latency"]["p95_s"] = 0.1
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="latency ordering is inconsistent",
    ):
        _compare(qemu, inverted_latency)

    wrong_backend = copy.deepcopy(pi)
    wrong_backend["population"]["backend_class"] = "host-model"
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="aggregate READY population",
    ):
        _compare(qemu, wrong_backend)

    stale_population_observation = copy.deepcopy(pi)
    stale_population_observation["population"]["observations"][0]["ready"] = 255
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="aggregate READY population",
    ):
        _compare(qemu, stale_population_observation)

    partial_exemplar = copy.deepcopy(pi)
    partial_exemplar["executable_state"]["pre"]["workers"][0] = {
        "role": "worker-heartbeat"
    }
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="exemplar schema",
    ):
        _compare(qemu, partial_exemplar)

    stale_lifecycle = copy.deepcopy(pi)
    stale_lifecycle["executable_state"]["post"]["workers"] = copy.deepcopy(
        stale_lifecycle["executable_state"]["pre"]["workers"]
    )
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="fresh-generation recreation",
    ):
        _compare(qemu, stale_lifecycle)

    low_pressure = copy.deepcopy(pi)
    low_pressure["workload"]["intensity_min"] = 1
    low_pressure["workload"]["intensity_max"] = 1
    low_pressure["workload"]["target_rps_min"] = 1024.0
    low_pressure["workload"]["target_rps_max"] = 1024.0
    low_pressure["provenance"]["workload_sha256"] = (
        comparator.canonical_json_sha256(low_pressure["workload"])
    )
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="canonical M26e HIGH",
    ):
        _compare(qemu, low_pressure)

    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="comparison bounds",
    ):
        _compare(
            qemu,
            pi,
            max_age_secs=comparator.MAX_BENCHMARK_AGE_SECS + 1,
        )


def test_benchmark_comparator_rejects_stale_or_source_mismatched_inputs() -> None:
    qemu = _benchmark_report("qemu", "qemu", ok_ops_per_s=600.0)
    stale_pi = _benchmark_report(
        "pi4", "genet", ok_ops_per_s=600.0, captured_unix_s=9_000
    )
    try:
        _compare(
            qemu,
            stale_pi,
            reference_unix_s=10_010,
            max_age_secs=60,
        )
    except comparator.BenchmarkComparisonError as exc:
        assert "stale" in str(exc)
    else:
        raise AssertionError("stale Pi report must be rejected")

    pi = _benchmark_report("pi4", "genet", ok_ops_per_s=600.0)
    changed_source = copy.deepcopy(pi)
    identity = comparator.performance_qualification_identity(
        changed_source["provenance"]
    )
    identity["source_sha256"] = "f" * 64
    changed_source["provenance"]["source_sha256"] = "f" * 64
    changed_source["provenance"][
        "performance_qualification_sha256"
    ] = comparator.canonical_json_sha256(identity)
    changed_source["executable_state"]["target_evidence"][
        "source_sha256"
    ] = "f" * 64
    changed_source["executable_state"]["target_evidence"][
        "performance_qualification_sha256"
    ] = changed_source["provenance"]["performance_qualification_sha256"]
    try:
        _compare(
            qemu,
            changed_source,
        )
    except comparator.BenchmarkComparisonError as exc:
        assert "source identities" in str(exc)
    else:
        raise AssertionError("source-mismatched inputs must be rejected")


def test_backpressure_and_wifi_norm_are_separate_verdicts() -> None:
    qemu = _benchmark_report("qemu", "qemu", ok_ops_per_s=600.0)
    pi = _benchmark_report("pi4", "genet", ok_ops_per_s=600.0)
    pi["backpressure"]["timeout_rejections"] = 1
    _set_report_errors(pi, 25)
    result = _compare(qemu, pi)
    assert result["verdict"] == "PASS"
    assert result["backpressure"]["included_in_verdict"] is False
    assert result["errors"]["comparative_counts_included_in_verdict"] is False

    wifi = _benchmark_report("pi4", "wifi", ok_ops_per_s=100.0, latency_s=0.02)
    result = _compare(
        qemu,
        _benchmark_report("pi4", "genet", ok_ops_per_s=600.0),
        wifi_report=wifi,
        wifi_min_ok_ops_per_s=150.0,
        wifi_max_p95_ms=10.0,
    )
    assert result["verdict"] == "PASS"
    assert result["wifi"]["status"] == "FAIL"
    assert result["wifi"]["included_in_wired_verdict"] is False
    assert result["wifi"]["latency_norm_status"] == "FLAG"

    mismatched_image = copy.deepcopy(wifi)
    mismatched_image["provenance"]["image_sha256"] = "e" * 64
    identity = comparator.performance_qualification_identity(
        mismatched_image["provenance"]
    )
    mismatched_image["provenance"]["performance_qualification_sha256"] = (
        comparator.canonical_json_sha256(identity)
    )
    mismatched_image["executable_state"]["target_evidence"][
        "image_sha256"
    ] = "e" * 64
    mismatched_image["executable_state"]["target_evidence"][
        "performance_qualification_sha256"
    ] = mismatched_image["provenance"]["performance_qualification_sha256"]
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="WiFi diagnostic provenance does not match",
    ):
        _compare(
            qemu,
            pi,
            wifi_report=mismatched_image,
            wifi_min_ok_ops_per_s=50.0,
            wifi_max_p95_ms=100.0,
        )

    mismatched_worker_archive = copy.deepcopy(wifi)
    mismatched_worker_archive["executable_state"]["target_session"][
        "worker_archive_sha256"
    ] = "d" * 64
    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="WiFi diagnostic target-neutral Worker artifacts do not match",
    ):
        _compare(
            qemu,
            pi,
            wifi_report=mismatched_worker_archive,
            wifi_min_ok_ops_per_s=50.0,
            wifi_max_p95_ms=100.0,
        )


def test_benchmark_reader_rejects_duplicate_keys_and_nonfinite_numbers(
    tmp_path: pathlib.Path,
) -> None:
    duplicate = tmp_path / "duplicate.json"
    duplicate.write_text('{"report":{},"report":{}}\n', encoding="utf-8")
    nonfinite = tmp_path / "nonfinite.json"
    nonfinite.write_text('{"report":{"value":NaN}}\n', encoding="utf-8")

    for path in (duplicate, nonfinite):
        try:
            comparator.read_benchmark_report(path)
        except comparator.BenchmarkComparisonError as exc:
            assert "not valid JSON" in str(exc)
        else:
            raise AssertionError("unsafe JSON input must fail closed")


def test_benchmark_io_rejects_symlinked_ancestor(tmp_path: pathlib.Path) -> None:
    real = tmp_path / "real"
    real.mkdir()
    report_path = real / "report.json"
    report_path.write_text(
        json.dumps({"report": _benchmark_report("qemu", "qemu", ok_ops_per_s=600.0)}),
        encoding="utf-8",
    )
    alias = tmp_path / "alias"
    alias.symlink_to(real, target_is_directory=True)

    with pytest.raises(
        comparator.BenchmarkComparisonError,
        match="cannot open benchmark report",
    ):
        comparator.read_benchmark_report(alias / "report.json")
    with pytest.raises(OSError):
        comparator.write_exclusive_output(alias / "result.json", "{}\n")
    assert not (real / "result.json").exists()


def test_benchmark_cli_refuses_to_overwrite_output(tmp_path: pathlib.Path) -> None:
    now = int(time.time())
    qemu = tmp_path / "qemu.json"
    pi = tmp_path / "pi.json"
    qemu.write_text(
        json.dumps(
            {
                "report": _benchmark_report(
                    "qemu",
                    "qemu",
                    ok_ops_per_s=600.0,
                    captured_unix_s=now,
                )
            }
        ),
        encoding="utf-8",
    )
    pi.write_text(
        json.dumps(
            {
                "report": _benchmark_report(
                    "pi4",
                    "genet",
                    ok_ops_per_s=600.0,
                    captured_unix_s=now,
                )
            }
        ),
        encoding="utf-8",
    )
    output = tmp_path / "comparison.json"
    output.write_text("preserve\n", encoding="utf-8")

    result = subprocess.run(
        [
            sys.executable,
            str(MODULE_PATH),
            "--qemu-report",
            str(qemu),
            "--pi-report",
            str(pi),
            "--reference-unix-s",
            str(now),
            "--max-age-secs",
            "60",
            "--genet-max-p95-ms",
            "100",
            "--output",
            str(output),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    assert "output refused" in result.stderr
    assert output.read_text(encoding="utf-8") == "preserve\n"


def test_benchmark_cli_rejects_operator_backdated_reference(
    tmp_path: pathlib.Path,
) -> None:
    qemu = tmp_path / "qemu.json"
    pi = tmp_path / "pi.json"
    qemu.write_text(
        json.dumps({"report": _benchmark_report("qemu", "qemu", ok_ops_per_s=600.0)}),
        encoding="utf-8",
    )
    pi.write_text(
        json.dumps({"report": _benchmark_report("pi4", "genet", ok_ops_per_s=600.0)}),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            sys.executable,
            str(MODULE_PATH),
            "--qemu-report",
            str(qemu),
            "--pi-report",
            str(pi),
            "--reference-unix-s",
            "10010",
            "--max-age-secs",
            "60",
            "--genet-max-p95-ms",
            "100",
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode != 0
    assert "within 300 seconds of the host clock" in result.stderr
