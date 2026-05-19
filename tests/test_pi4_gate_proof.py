# Author: Lukas Bower
# Purpose: Regression tests for the Raspberry Pi 4 USB/WiFi gate proof shell wrapper.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4_gate_proof.sh."""

import pathlib
import subprocess

import pytest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "pi4_gate_proof.sh"


def test_gate_proof_does_not_emit_leading_carriage_return() -> None:
    """Serial proof commands should not manufacture empty console commands."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert "printf '%s\\r'" in source
    assert "printf '\\r%s\\r'" not in source


def test_gate_proof_waits_for_prompt_at_line_start() -> None:
    """Capture readiness must not match debug prose containing the prompt text."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert "console_prompt_seen()" in source
    assert 'line.startswith(b"cohesix>")' in source
    assert "grep -q 'cohesix>'" not in source


def test_gate_proof_rejects_generic_usb_unavailable_summary() -> None:
    """A generic keyboard-unavailable summary must not mask the real USB gate."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert '"USB_BLOCKER=cmd-event-ring-timeout"' in source
    assert '"USB_BLOCKER=unavailable"' in source


def test_gate_proof_rejects_current_usb_and_wifi_blockers(tmp_path: pathlib.Path) -> None:
    """Default proof policy must reject stale USB reset and WiFi HT blockers."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "U-Boot 2026.01-dirty",
                "[cohesix] USB host session was not active; xHCI cold boot starts unseeded",
                "[cohesix:root-task] Cohesix boot: root-task online",
                "[local-seat] xhci probe begin mmio=0x0000000600000000 "
                "attempt=2/2 policy=platform-reset-complete",
                "[local-seat] xhci.diag stage=0x0226 "
                "tag=reset-pre-usbcmd-source a=0 b=0 c=0",
                "halting...",
                "Kernel entry via Interrupt, irq 27",
                "wifi: boot_failure source=live stage=cyw43-load-firmware-fail "
                "exact=cyw43-ht-clock-timeout-before-function2",
            ]
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--venv",
            str(venv_dir),
            "--log",
            str(log_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    assert "USB_BLOCKER=reset-pre-usbcmd-source" in result.stdout
    assert "WIFI_BLOCKER=ht-clock-timeout" in result.stdout
    assert (
        "USB_BLOCKER rejected reset-pre-usbcmd-source"
        in result.stderr
    )
    assert "WIFI_BLOCKER rejected ht-clock-timeout" in result.stderr


def test_gate_proof_rejects_unknown_default_gate_evidence(
    tmp_path: pathlib.Path,
) -> None:
    """Generic USB/WiFi lines are not enough to pass the proof loop."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "U-Boot 2026.01-dirty",
                "[cohesix] USB host session was not active; xHCI cold boot starts unseeded",
                "[cohesix:root-task] Cohesix boot: root-task online",
                "[local-seat] xhci runtime candidates=1 hint=no pci_cfg_ready=no",
                "[pi4-wifi] mailbox request page paddr=0x04000000 "
                "action=reuse-shared",
            ]
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--venv",
            str(venv_dir),
            "--log",
            str(log_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    assert "USB_BLOCKER=unknown" in result.stdout
    assert "WIFI_BLOCKER=unknown" in result.stdout
    assert "USB_BLOCKER rejected unknown" in result.stderr
    assert "WIFI_BLOCKER rejected unknown" in result.stderr


def test_gate_proof_rejects_local_seat_wifi_boot_deferral(
    tmp_path: pathlib.Path,
) -> None:
    """Default hardware proof must fail if local-seat boot skips WiFi."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "U-Boot 2026.01-dirty",
                "[cohesix] USB host session was not active; xHCI cold boot starts unseeded",
                "[cohesix:root-task] Cohesix boot: root-task online",
                "[local-seat] xhci enumerate outcome=keyboard-ready",
                "[net-console] deferred reason=pi4-local-seat-explicit-wifi "
                "action=root-console-wait-for-wifi",
            ]
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--venv",
            str(venv_dir),
            "--log",
            str(log_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    assert "WIFI_BLOCKER=boot-waiting-for-wifi" in result.stdout
    assert "WIFI_BLOCKER rejected boot-waiting-for-wifi" in result.stderr


def test_gate_proof_rejects_missing_root_console_prompt(
    tmp_path: pathlib.Path,
) -> None:
    """Default hardware proof must fail if boot never reaches the root prompt."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "U-Boot 2026.01-dirty",
                "[cohesix] USB host session was not active; xHCI cold boot starts unseeded",
                "[cohesix:root-task] Cohesix boot: root-task online",
                "[local-seat] xhci root-port command-probe result=enable-slot-ok",
                "wifi: firmware-ready",
            ]
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--venv",
            str(venv_dir),
            "--log",
            str(log_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    assert "ROOT_CONSOLE_READY=no" in result.stdout
    assert "ROOT_PROMPT_SEEN=no" in result.stdout
    assert "ROOT_CONSOLE_READY expected yes got no" in result.stderr
    assert "ROOT_PROMPT_SEEN expected yes got no" in result.stderr


def test_gate_proof_rejects_dedicated_contracts_without_substrate_ready(
    tmp_path: pathlib.Path,
) -> None:
    """Dedicated-looking contract counts still need substrate-ready proof."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task max_service_us=40 observed_service_us=18",
                "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task max_service_us=40 observed_service_us=22",
                "SCHED_CONTRACT contract=hdmi-text isolation=dedicated-sel4-task max_service_us=80 observed_service_us=44",
                "SCHED_CONTRACT contract=genet isolation=dedicated-sel4-task max_service_us=120 observed_service_us=73",
                "DRIVER_TASK_ACCEPTANCE dedicated_ready=no reason=dedicated-sel4-substrate-not-active required=4 dedicated=4 compatibility=0",
                "SERIAL_ECHO p95_us=800 max_gap_us=1200",
                "USB_BURST bytes=256 drops=0 max_latency_us=900",
                "HDMI_RESPONSIVE max_gap_ms=9 mirrored_bytes=256",
            ]
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--require-driver-task-proof",
            "--venv",
            str(venv_dir),
            "--log",
            str(log_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    assert "DRIVER_TASK_DEDICATED=4" in result.stdout
    assert "DRIVER_TASK_DEDICATED_READY=no" in result.stdout
    assert "DRIVER_TASK_SERIAL_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_USB_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_DISPLAY_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_NET_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_DEDICATED_READY expected yes got no" in result.stderr


def test_gate_proof_rejects_aggregate_dedicated_count_without_required_roles(
    tmp_path: pathlib.Path,
) -> None:
    """Aggregate dedicated counts cannot replace serial/USB/display/net proof."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "SCHED_CONTRACT contract=genet isolation=dedicated-sel4-task observed_service_us=73",
                "SCHED_CONTRACT contract=cyw43455 isolation=dedicated-sel4-task observed_service_us=91",
                "SCHED_CONTRACT contract=rtl8139 isolation=dedicated-sel4-task observed_service_us=62",
                "SCHED_CONTRACT contract=virtio-net isolation=dedicated-sel4-task observed_service_us=64",
                "DRIVER_TASK_ACCEPTANCE dedicated_ready=yes reason=active-substrate required=4 dedicated=4 compatibility=0",
                "SERIAL_ECHO p95_us=800 max_gap_us=1200",
                "USB_BURST bytes=256 drops=0 max_latency_us=900",
                "HDMI_RESPONSIVE max_gap_ms=9 mirrored_bytes=256",
            ]
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--require-driver-task-proof",
            "--venv",
            str(venv_dir),
            "--log",
            str(log_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    assert "DRIVER_TASK_DEDICATED=4" in result.stdout
    assert "DRIVER_TASK_NET_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_SERIAL_DEDICATED=no" in result.stdout
    assert "DRIVER_TASK_USB_DEDICATED=no" in result.stdout
    assert "DRIVER_TASK_DISPLAY_DEDICATED=no" in result.stdout
    assert "DRIVER_TASK_SERIAL_DEDICATED expected yes got no" in result.stderr


def test_gate_proof_rejects_root_task_panic(tmp_path: pathlib.Path) -> None:
    """Default hardware proof must fail on root-task panic evidence."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "U-Boot 2026.01-dirty",
                "[cohesix] USB host session was not active; xHCI cold boot starts unseeded",
                "[cohesix:root-task] Cohesix boot: root-task online",
                "[local-seat] xhci root-port command-probe result=enable-slot-ok",
                "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
                "BOOTINFO_SNAPSHOT_CORRUPTED phase=net.init last_mark=net.init.device "
                "pre=0x0b0f1ce5ca4ecafe post=0x00000000001e2839 "
                "expected_pre=0x0b0f1ce5ca4ecafe expected_post=0x9ddf1ce5f00dbeef",
                "[PANIC] panicked at apps/root-task/src/bootstrap/bootinfo_snapshot.rs:499:9:",
            ]
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--venv",
            str(venv_dir),
            "--log",
            str(log_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    assert "PANIC_SEEN=yes" in result.stdout
    assert "PANIC_REASON=bootinfo-snapshot-corrupted" in result.stdout
    assert "PANIC_SEEN expected no got yes" in result.stderr


def test_gate_proof_rejects_stale_uefi_usb_hint(tmp_path: pathlib.Path) -> None:
    """The default proof loop must fail if a stale pre-cold-boot image ran."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "U-Boot 2026.01-dirty",
                "[cohesix] USB host session was not active; xHCI cold boot starts unseeded",
                "[cohesix:root-task] Cohesix boot: root-task online",
                "[local-seat] pi4 keyboard unavailable detail=usb-keyboard-missing "
                'hint="UEFI vars: XhciPci=0 XhciReload=1 SystemTableMode=1"',
                "wifi: power=on reset=deasserted card=yes rca=0x0001",
            ]
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--venv",
            str(venv_dir),
            "--log",
            str(log_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    assert "USB_STALE_UEFI_HINT_SEEN=yes" in result.stdout
    assert (
        "USB_STALE_UEFI_HINT_SEEN expected no got yes"
        in result.stderr
    )


@pytest.mark.parametrize(
    "flag",
    [
        "--require-usb-ready",
        "--require-wired-ready",
        "--require-driver-task-proof",
        "--require-input-responsive",
    ],
)
def test_gate_proof_rejects_summary_only_ready_requirements(
    tmp_path: pathlib.Path,
    flag: str,
) -> None:
    """Ready gates must keep safety expectations enabled."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text("", encoding="utf-8")

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--allow-summary-only",
            flag,
            "--venv",
            str(venv_dir),
            "--log",
            str(log_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    assert (
        "--allow-summary-only cannot be combined with ready-gate requirements"
        in result.stderr
    )
