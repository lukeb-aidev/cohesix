# Author: Lukas Bower
# Purpose: Unit tests for the Pi 4 driver-model log comparator.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4_compare_driver_models.py."""

from __future__ import annotations

import importlib.util
import pathlib
import subprocess
import sys


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
MODULE_PATH = REPO_ROOT / "scripts" / "pi4_compare_driver_models.py"

spec = importlib.util.spec_from_file_location(
    "pi4_compare_driver_models", MODULE_PATH
)
comparator = importlib.util.module_from_spec(spec)
assert spec.loader is not None
sys.modules[spec.name] = comparator
spec.loader.exec_module(comparator)


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
        "[local-seat] runtime keyboard first-byte read=1 ascii=0x54 key=0x17",
        "[local-seat] pi4 keyboard runtime proof result=online gate=10 source=first-byte",
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
            "blocker=xhci-ready detail=0x0201 result=0x03000001",
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
