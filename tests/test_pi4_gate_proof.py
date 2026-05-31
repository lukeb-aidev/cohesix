# Author: Lukas Bower
# Purpose: Regression tests for the Raspberry Pi 4 USB/WiFi gate proof shell wrapper.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4_gate_proof.sh."""

import pathlib
import subprocess

import pytest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "pi4_gate_proof.sh"


def _driver_task_owner_state_lines() -> list[str]:
    return [
        "DRIVER_TASK_OWNER_STATE contract=serial hot_path=serial-console "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=usb-local-seat hot_path=usb-keyboard "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=hdmi-text hot_path=hdmi-text "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=bcmgenet-v5 hot_path=genet-nic "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=cyw43455 hot_path=cyw43-wifi "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=sdio-host hot_path=sdio-host "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=pcie-root hot_path=pcie-root "
        "owner_state=driver-owned descriptor=present root_pointer=no",
    ]


def _strong_driver_task_proof_lines() -> list[str]:
    return [
        "U-Boot 2026.01-dirty",
        "[cohesix] USB host session was not active; xHCI cold boot starts unseeded",
        "[Cohesix] Root console ready (type 'help' for commands)",
        "cohesix> driver proof",
        "usb: runtime_gate proof_gate=10 blocker=none",
        "OK NETTEST success",
        "netstats: active=wifi addr_src=dhcp-lease dhcp=bound wifi_assoc=1 "
        "wifi_link=1 eapol_secure=1 eapol_rx=1 rx_pkts=1 tx_pkts=1",
        "DRIVER_TASK_DEFAULT requested=dedicated required=yes live_hot_paths=yes",
        "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 "
        "task_count=9 failed_count=0 live_tcb_count=9 "
        "root_authority_retained=yes fault_endpoint_ready=yes revoke_ready=yes "
        "broad_caps_leaked=0 sched=yes affinity=per-driver "
        "affinity_configured=9 affinity_applied=9 vspace=isolated "
        "ipc_abi=shared-ring-command pointer_free_ipc=yes "
        "owner_state=driver-owned live_hot_paths=yes",
        *_driver_task_owner_state_lines(),
        "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=18",
        "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=22",
        "SCHED_CONTRACT contract=hdmi-text isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=44",
        "SCHED_CONTRACT contract=cyw43455 isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated active_net=cyw43 observed_service_us=91",
        "SCHED_CONTRACT contract=genet isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=73",
        "SCHED_CONTRACT contract=sdio-host isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=31",
        "SCHED_CONTRACT contract=pcie-root isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=36",
        "DRIVER_TASK_ACCEPTANCE dedicated_ready=yes reason=active-substrate "
        "substrate=active capset=pass fault=pass revoke=pass sched=pass "
        "affinity=pass vspace=isolated ipc_abi=shared-ring-command "
        "pointer_free_ipc=yes owner_state=driver-owned required=7 "
        "dedicated=7 compatibility=0",
        "SERIAL_ECHO p95_us=800 max_gap_us=1200",
        "USB_BURST bytes=256 drops=0 max_latency_us=900",
        "HDMI_RESPONSIVE max_gap_ms=9 mirrored_bytes=256",
    ]


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


def test_gate_proof_runs_smp_activity_for_post_prompt_driver_proof() -> None:
    """Default captures should refresh driver-task proof after prompt-side replay."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert '"smp activity"' in source
    assert source.index('"smp activity"') < source.index('"wifi diag"')


def test_gate_proof_refuses_existing_capture_log(tmp_path: pathlib.Path) -> None:
    """Active capture must not truncate an existing serial log."""

    venv_dir = tmp_path / "venv"
    python_path = venv_dir / "bin" / "python"
    python_path.parent.mkdir(parents=True)
    python_path.write_text("", encoding="utf-8")

    log_path = tmp_path / "pi4-serial.log"
    original = "keep this boot evidence\n"
    log_path.write_text(original, encoding="utf-8")

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--skip-build",
            "--venv",
            str(venv_dir),
            "--serial-device",
            str(tmp_path / "tty"),
            "--log",
            str(log_path),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 1
    assert "refusing to capture to existing log without truncating" in result.stderr
    assert log_path.read_text(encoding="utf-8") == original


def test_gate_proof_rejects_generic_usb_unavailable_summary() -> None:
    """A generic keyboard-unavailable summary must not mask the real USB gate."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert '"USB_BLOCKER=cmd-event-ring-timeout"' in source
    assert '"USB_BLOCKER=pcie-xhci-device-coverage-missing"' in source
    assert '"USB_BLOCKER=pcie-vl805-config-contract-missing"' in source
    assert '"USB_BLOCKER=keyboard-not-ready"' in source
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


def test_gate_proof_rejects_unproved_driver_task_runtime_blockers(
    tmp_path: pathlib.Path,
) -> None:
    """Default proof policy must fail the current no-reply runtime frontier."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "U-Boot 2026.01-dirty",
                "[cohesix] USB host session was not active; xHCI cold boot starts unseeded",
                "[Cohesix] Root console ready (type 'help' for commands)",
                "cohesix> usb status",
                "usb: ownership_blocker current=pcie-vl805-config-contract-missing "
                "expected=vl805-config-window+command+bar0+mailbox "
                "observed=missing-or-disabled blocker=pcie-vl805-config-contract-missing",
                "usb: runtime_gate keyboard=no first_report=no first_byte=no "
                "proof_gate=0 target_gate=10 next=keyboard-ready blocker=keyboard-not-ready",
                "[net-console] deferred failed detail=cyw43-wifi driver-task runtime "
                "is pending hardware service",
                "ERR NETTEST reason=policy detail=net-disabled "
                "cause=cyw43-wifi driver-task runtime is pending hardware service",
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
    assert "USB_BLOCKER=pcie-vl805-config-contract-missing" in result.stdout
    assert "WIFI_BLOCKER=wifi-driver-task-runtime-unproved" in result.stdout
    assert (
        "USB_BLOCKER rejected pcie-vl805-config-contract-missing"
        in result.stderr
    )
    assert (
        "WIFI_BLOCKER rejected wifi-driver-task-runtime-unproved"
        in result.stderr
    )


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
                "DRIVER_TASK_DEFAULT requested=dedicated required=yes live_hot_paths=yes",
                "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated max_service_us=40 observed_service_us=18",
                "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated max_service_us=40 observed_service_us=22",
                "SCHED_CONTRACT contract=hdmi-text isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated max_service_us=80 observed_service_us=44",
                "SCHED_CONTRACT contract=genet isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated max_service_us=120 observed_service_us=73",
                "SCHED_CONTRACT contract=cyw43455 isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated max_service_us=120 observed_service_us=91",
                "SCHED_CONTRACT contract=sdio-host isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated max_service_us=80 observed_service_us=31",
                "SCHED_CONTRACT contract=pcie-root isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated max_service_us=80 observed_service_us=36",
                "DRIVER_TASK_ACCEPTANCE dedicated_ready=no reason=dedicated-sel4-substrate-not-active required=7 dedicated=7 compatibility=0",
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
    assert "DRIVER_TASK_DEDICATED=7" in result.stdout
    assert "DRIVER_TASK_DEDICATED_READY=no" in result.stdout
    assert "DRIVER_TASK_SERIAL_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_USB_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_DISPLAY_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_NET_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_SDIO_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_PCIE_DEDICATED=yes" in result.stdout
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
                "DRIVER_TASK_DEFAULT requested=dedicated required=yes live_hot_paths=yes",
                "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 mcs=0 task_count=9 failed_count=0 live_tcb_count=9 root_authority_retained=yes fault_endpoint_ready=yes revoke_ready=yes broad_caps_leaked=0 sched=yes affinity=per-driver affinity_configured=9 affinity_applied=9 vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes owner_state=driver-owned live_hot_paths=yes",
                "SCHED_CONTRACT contract=genet isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=73",
                "SCHED_CONTRACT contract=cyw43455 isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=91",
                "SCHED_CONTRACT contract=rtl8139 isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=62",
                "SCHED_CONTRACT contract=virtio-net isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=64",
                "SCHED_CONTRACT contract=sdio-host isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=31",
                "SCHED_CONTRACT contract=pcie-root isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=36",
                "DRIVER_TASK_ACCEPTANCE dedicated_ready=yes reason=active-substrate substrate=active capset=pass fault=pass revoke=pass sched=pass affinity=pass vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes owner_state=driver-owned required=6 dedicated=6 compatibility=0",
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
    assert "DRIVER_TASK_DEDICATED=6" in result.stdout
    assert "DRIVER_TASK_NET_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_SDIO_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_PCIE_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_SERIAL_DEDICATED=no" in result.stdout
    assert "DRIVER_TASK_USB_DEDICATED=no" in result.stdout
    assert "DRIVER_TASK_DISPLAY_DEDICATED=no" in result.stdout
    assert "DRIVER_TASK_SERIAL_DEDICATED expected yes got no" in result.stderr


def test_gate_proof_rejects_isolated_vspace_without_pointer_free_ipc(
    tmp_path: pathlib.Path,
) -> None:
    """Shared VSpace closure also requires the pointer-free command ABI."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "DRIVER_TASK_DEFAULT requested=dedicated required=yes live_hot_paths=yes",
                "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 task_count=9 failed_count=0 live_tcb_count=9 root_authority_retained=yes fault_endpoint_ready=yes revoke_ready=yes broad_caps_leaked=0 sched=yes affinity=per-driver affinity_configured=9 affinity_applied=9 vspace=isolated ipc_abi=callback-pointer pointer_free_ipc=no owner_state=driver-owned live_hot_paths=yes",
                "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=18",
                "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=22",
                "SCHED_CONTRACT contract=hdmi-text isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=44",
                "SCHED_CONTRACT contract=cyw43455 isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=91",
                "SCHED_CONTRACT contract=genet isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=73",
                "SCHED_CONTRACT contract=sdio-host isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=31",
                "SCHED_CONTRACT contract=pcie-root isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=36",
                "DRIVER_TASK_ACCEPTANCE dedicated_ready=no reason=driver-task-pointer-free-ipc-not-proven substrate=active capset=pass fault=pass revoke=pass sched=pass affinity=pass vspace=isolated ipc_abi=callback-pointer pointer_free_ipc=no owner_state=driver-owned required=7 dedicated=7 compatibility=0",
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
    assert "DRIVER_TASK_VSPACE_PROOF=yes" in result.stdout
    assert "DRIVER_TASK_POINTER_FREE_IPC_PROOF=no" in result.stdout
    assert "DRIVER_TASK_POINTER_FREE_IPC_PROOF expected yes got no" in result.stderr


def test_gate_proof_rejects_pointer_free_ring_without_owner_state(
    tmp_path: pathlib.Path,
) -> None:
    """Pointer-free rings do not prove driver-owned hardware state by themselves."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "DRIVER_TASK_DEFAULT requested=dedicated required=yes live_hot_paths=yes",
                "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 task_count=9 failed_count=0 live_tcb_count=9 root_authority_retained=yes fault_endpoint_ready=yes revoke_ready=yes broad_caps_leaked=0 sched=yes affinity=per-driver affinity_configured=9 affinity_applied=9 vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes owner_state=root-owned live_hot_paths=yes",
                "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=18",
                "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=22",
                "SCHED_CONTRACT contract=hdmi-text isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=44",
                "SCHED_CONTRACT contract=cyw43455 isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=91",
                "SCHED_CONTRACT contract=genet isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=73",
                "SCHED_CONTRACT contract=sdio-host isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=31",
                "SCHED_CONTRACT contract=pcie-root isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=36",
                "DRIVER_TASK_ACCEPTANCE dedicated_ready=no reason=driver-task-owner-state-not-proven substrate=active capset=pass fault=pass revoke=pass sched=pass affinity=pass vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes owner_state=root-owned required=7 dedicated=7 compatibility=0",
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
    assert "DRIVER_TASK_POINTER_FREE_IPC_PROOF=yes" in result.stdout
    assert "DRIVER_TASK_OWNER_STATE_PROOF=no" in result.stdout
    assert "DRIVER_TASK_OWNER_STATE_PROOF expected yes got no" in result.stderr


def test_gate_proof_accepts_per_hot_path_owner_state_descriptors(
    tmp_path: pathlib.Path,
) -> None:
    """Strong driver-task proof needs every current acceptance descriptor."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    lines = _strong_driver_task_proof_lines()
    log_path.write_text("\n".join(lines), encoding="utf-8")

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

    assert result.returncode == 0
    assert "DRIVER_TASK_OWNER_STATE_PROOF=yes" in result.stdout
    assert "DRIVER_TASK_DEDICATED_READY=yes" in result.stdout
    assert "DRIVER_TASK_SDIO_DEDICATED=yes" in result.stdout


def test_gate_proof_rejects_outstanding_driver_task_ring_call(
    tmp_path: pathlib.Path,
) -> None:
    """Strong proof must reject a driver call that never returned."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    lines = _strong_driver_task_proof_lines()
    lines.append(
        "DRIVER_TASK_RING_CALL_BEGIN contract=hdmi-text endpoint=0x0649 "
        "request=9 opcode=2 flags=0x0000 arg0=3 arg1=4 aux0=0x00000000 "
        "aux1=0 frame_len=21"
    )
    log_path.write_text("\n".join(lines), encoding="utf-8")

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
    assert "DRIVER_TASK_RING_CALL_OUTSTANDING=1" in result.stdout
    assert "DRIVER_TASK_RING_CALL_OUTSTANDING expected 0 got 1" in result.stderr


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
