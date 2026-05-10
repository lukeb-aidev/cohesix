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


def test_gate_proof_rejects_summary_only_ready_requirements(
    tmp_path: pathlib.Path,
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
            "--require-usb-ready",
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
