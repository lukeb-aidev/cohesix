# Author: Lukas Bower
# Purpose: Unit tests for scripts/pi4_trace_normalize.py Pi 4 USB/WiFi log normalization.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4_trace_normalize.py."""

import importlib.util
import json
import pathlib
import sys

MODULE_PATH = (
    pathlib.Path(__file__).resolve().parents[1]
    / "scripts"
    / "pi4_trace_normalize.py"
)

spec = importlib.util.spec_from_file_location("pi4_trace_normalize", MODULE_PATH)
normalizer = importlib.util.module_from_spec(spec)
assert spec.loader is not None
sys.modules[spec.name] = normalizer
spec.loader.exec_module(normalizer)

JOIN_COMPLETE_OPEN = (
    "[cyw43] join complete mode=deferred polls=3 secure=no "
    "completion_rule=set-ssid set_ssid=yes fwsup=no psk_sup=no "
    "psk_status=0x00000000 carrier=no"
)

JOIN_COMPLETE_SECURE = (
    "[cyw43] join complete mode=deferred polls=3 secure=yes "
    "completion_rule=firmware-supplicant-psk-sup set_ssid=yes fwsup=yes "
    "psk_sup=yes psk_status=0x00000006 carrier=yes"
)

JOIN_COMPLETE_HOST_EAPOL = (
    "[cyw43] join complete mode=host-eapol secure=yes "
    "completion_rule=host-eapol-required m1=yes m2=yes m3=yes m4=yes "
    "wsec_key=ptk+gtk key_order=m4-before-wsec carrier=yes"
)


def test_usb_trace_line_extracts_stage_and_tokens() -> None:
    event = normalizer.parse_line(
        "[cohesix:usb-trace] stage=handoff-usb-reset-begin input=0 "
        "reprobe=failed",
        7,
    )

    assert event is not None
    assert event.domain == "usb"
    assert event.source == "uboot"
    assert event.stage == "handoff-usb-reset-begin"
    assert event.fields["input"] == "0"
    assert event.fields["reprobe"] == "failed"


def test_cohesix_wifi_dump_state_line_extracts_blocker() -> None:
    event = normalizer.parse_line(
        "wifi: snapshot source=live stage=console-dump-state "
        "exact_error=cyw43-ht-clock-timeout-before-function2",
        12,
    )

    assert event is not None
    assert event.domain == "wifi"
    assert event.source == "cohesix"
    assert event.stage == "console-dump-state"
    assert event.fields["exact_error"] == (
        "cyw43-ht-clock-timeout-before-function2"
    )


def test_linux_brcmfmac_line_is_wifi_known_good_source() -> None:
    event = normalizer.parse_line(
        "brcmfmac: brcmf_sdio_htclk: HT Avail timeout clkctl=0x50",
        19,
    )

    assert event is not None
    assert event.domain == "wifi"
    assert event.source == "linux"
    assert "brcmf_sdio_htclk" in event.message


def test_parse_events_filters_unrelated_lines() -> None:
    lines = [
        "cohesix> nettest",
        "ERR NETTEST reason=policy detail=net-disabled",
        "[local-seat] xhci.diag stage=0x0230 tag=reset-write a=0x20",
        "[INFO root_task::hal::pi4_wifi] [pi4-wifi] hal init: begin",
    ]

    events = normalizer.parse_events(lines)

    assert [event.domain for event in events] == ["console", "wifi", "usb", "wifi"]
    assert events[2].stage == "0x0230"
    assert events[2].fields["tag"] == "reset-write"


def test_pi4_wifi_mailbox_usb_power_lines_are_usb_platform_evidence() -> None:
    event = normalizer.parse_line(
        "[pi4-wifi] mailbox vl805-usb-hcd-power action=begin module=0x00000003",
        31,
    )

    assert event is not None
    assert event.domain == "usb"
    assert event.source == "cohesix"
    assert event.fields["action"] == "begin"


def test_pi4_wifi_mailbox_module3_power_on_line_is_usb_platform_evidence() -> None:
    event = normalizer.parse_line(
        "[pi4-wifi] mailbox power-on module=0x00000003",
        32,
    )

    assert event is not None
    assert event.domain == "usb"
    assert event.source == "cohesix"
    assert event.fields["module"] == "0x00000003"


def test_parse_events_splits_interleaved_uart_segments() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_rele[pi4-wifi] firmware stage=debug-probe-ht "
            "action=ht-clock-ladder-exhausted "
            "exact_error=cyw43-ht-clock-timeout-before-function2",
            "usb: ownership_proof cfg_replay=yes mailbox=unattemp"
            "[local-seat] xhci.diag stage=0x0226 "
            "tag=reset-pre-usbcmd-source a=0 b=0 c=0",
        ]
    )

    assert any(
        event.raw.startswith("[pi4-wifi] firmware stage=debug-probe-ht")
        for event in events
    )
    assert any(
        event.raw.startswith("[local-seat] xhci.diag stage=0x0226")
        for event in events
    )


def test_nettest_policy_error_is_wifi_terminal_evidence() -> None:
    event = normalizer.parse_line(
        "ERR NETTEST reason=policy detail=net-disabled "
        "cause=cyw43-armcr4-release-readback-unavailable",
        23,
    )

    assert event is not None
    assert event.domain == "wifi"
    assert event.fields["cause"] == "cyw43-armcr4-release-readback-unavailable"


def test_uppercase_usb_and_wifi_prefixes_are_classified() -> None:
    usb_event = normalizer.parse_line("USB: stage=controller-ready", 24)
    wifi_event = normalizer.parse_line("WiFi: stage=firmware-ready", 25)

    assert usb_event is not None
    assert wifi_event is not None
    assert usb_event.domain == "usb"
    assert wifi_event.domain == "wifi"


def test_summary_tracks_latest_and_blockers() -> None:
    events = normalizer.parse_events(
        [
            "[cohesix:usb-trace] stage=handoff-usb-stop-begin input=0",
            "usb: verdict=policy-skip-before-run focus=fresh-ownership",
            "wifi: preserved_failure source=live stage=cyw43-load-firmware-fail "
            "exact=cyw43-device-on-timeout-before-ht",
        ]
    )

    summary = normalizer.summarize_events(events)

    assert summary["events"] == 3
    assert summary["domains"] == {"usb": 2, "wifi": 1}
    assert summary["latest"]["wifi"]["stage"] == "cyw43-load-firmware-fail"
    assert len(summary["blockers"]) == 2
    assert summary["gates"]["WIFI_BLOCKER"] == "devon-timeout"


def test_cli_summary_uses_latest_boot_slice(
    tmp_path: pathlib.Path, capsys
) -> None:
    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "U-Boot 2026.01-dirty",
                "[cohesix:root-task] Cohesix boot: root-task online",
                "usb: runtime_gate proof_gate=10 blocker=none",
                "OK NETTEST detail=pass scope=serial-local",
                "U-Boot 2026.01-dirty",
                "[cohesix:root-task] Cohesix boot: root-task online",
                "[local-seat] xhci.diag stage=0x0226 "
                "tag=reset-pre-usbcmd-source a=0 b=0 c=0",
                "Kernel entry via Interrupt, irq 27",
                "wifi: firmware_release fw=609309 rstvec=0xb83ef198 "
                "armcr4_release=1",
                "[pi4-wifi] firmware stage=debug-probe-ht "
                "action=ht-clock-ladder-exhausted "
                "exact_error=cyw43-ht-clock-timeout-before-function2 csr=0x50",
            ]
        ),
        encoding="utf-8",
    )

    result = normalizer.main([str(log_path), "--summary"])
    captured = capsys.readouterr()
    summary = json.loads(captured.out)

    assert result == 0
    assert summary["gates"]["USB_BLOCKER"] == "reset-pre-usbcmd-source"
    assert summary["gates"]["WIFI_BLOCKER"] == "ht-clock-timeout"


def test_latest_boot_slice_keeps_same_boot_uboot_usb_evidence() -> None:
    lines = [
        "U-Boot 2026.01-dirty",
        "[cohesix] USB host stopped; xHCI stop seed exported as diagnostic only",
        "[cohesix] dtb chosen cohesix,xhci-usbcmd=0x00000000",
        "Starting kernel ...",
        "[cohesix:root-task] Cohesix boot: root-task online",
        "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write doorbell=0x100",
        "halting...",
        "Kernel entry via Interrupt, irq 27",
    ]

    events = normalizer.parse_events(normalizer.latest_boot_lines(lines))
    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["USB_BOOTLOADER_HANDOFF_SEEN"] == "yes"
    assert gates.to_record()["BOOT_HALT_REASON"] == "kernel-halt"


def test_latest_boot_slice_prefers_later_uboot_chain() -> None:
    lines = [
        "U-Boot 2026.01-dirty",
        "[cohesix] USB host stopped; xHCI stop seed exported as diagnostic only",
        "Starting kernel ...",
        "wifi: boot_failure source=live exact=old-failure",
        "U-Boot 2026.01-dirty",
        "[cohesix] USB host session was not active; xHCI cold boot starts unseeded",
        "Starting kernel ...",
        "wifi: boot_failure source=live exact=new-failure",
    ]

    events = normalizer.parse_events(normalizer.latest_boot_lines(lines))
    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["USB_BOOTLOADER_HANDOFF_SEEN"] == "no"
    assert gates.to_record()["USB_COLD_BOOT_SEEN"] == "yes"
    assert gates.to_record()["WIFI_EXACT"] == "new-failure"


def test_gate_summary_tracks_usb_command_ring_and_wifi_ht_blockers() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "usb: contract current=controller-ready expected=command-ring-recovery",
            "[local-seat] xhci.diag stage=0x0368 tag=cmd-gate-post-doorbell-plan-0 "
            "usbcmd_usbsts=0x0000000500000000 config_dnctrl=0x0000002000000002 "
            "expected_ptr=0x0000000404024000",
            "[local-seat] xhci.diag stage=0x036c tag=cmd-gate-timeout-plan-0 "
            "usbcmd_usbsts=0x0000000500000000 config_dnctrl=0x0000002000000002 "
            "expected_ptr=0x0000000404024000",
            "usb: diag_contract stage=0x030b diag_fresh=yes "
            "tag=cmd-poll-only-timeout exact=cmd-poll-only-timeout",
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: contract current=wait-ht-clock expected=chipclkcsr-ht-avail",
            "wifi: boot_failure source=live stage=cyw43-load-firmware-fail "
            "exact=cyw43-ht-clock-timeout-before-function2",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.to_record() == {
        "USB_GATE": 3,
        "USB_BLOCKER": "cmd-poll-only-timeout",
        "WIFI_GATE": 4,
        "WIFI_BLOCKER": "ht-clock-timeout",
        "WIFI_EXACT": "cyw43-ht-clock-timeout-before-function2",
        "WIFI_PHASE": "cyw43-load-firmware-fail",
        "WIFI_BLOCKER_LINE": 8,
        "SERIAL_CLEAN": "yes",
        "BOOT_HALTED": "no",
        "TIMER_IRQ27_SEEN": "no",
        "BOOT_HALT_REASON": "none",
        "PANIC_SEEN": "no",
        "PANIC_REASON": "none",
        "USB_BOOTLOADER_HANDOFF_SEEN": "no",
        "USB_COLD_BOOT_SEEN": "no",
        "USB_STALE_UEFI_HINT_SEEN": "no",
        "USB_EVENT_RING_ALIVE": "no",
        "USB_PSC_DRAIN_COUNT": 0,
        "USB_PSC_DRAIN_MASK": "0x00000000",
        "ROOT_CONSOLE_READY": "no",
        "ROOT_PROMPT_SEEN": "no",
        "SDIO_IRQ158_SEEN": "no",
        "SDIO_IRQ158_BOUND": "no",
        "SDIO_IRQ158_LINE": 0,
        "NET_ACTIVE": "unknown",
        "NET_ADDR_SRC": "unknown",
        "NET_DHCP": "unknown",
        "DRIVER_TASK_DEFAULT_REQUESTED": "no",
        "DRIVER_TASK_LIVE_HOT_PATHS": "no",
        "DRIVER_TASK_CONTRACTS": 0,
        "DRIVER_TASK_DEDICATED": 0,
        "DRIVER_TASK_COMPATIBILITY": 0,
        "DRIVER_TASK_DEDICATED_READY": "no",
        "DRIVER_TASK_SERIAL_DEDICATED": "no",
        "DRIVER_TASK_USB_DEDICATED": "no",
        "DRIVER_TASK_DISPLAY_DEDICATED": "no",
        "DRIVER_TASK_NET_DEDICATED": "no",
        "DRIVER_TASK_SDIO_DEDICATED": "no",
        "DRIVER_TASK_PCIE_DEDICATED": "no",
        "DRIVER_TASK_SUBSTRATE_READY": "no",
        "DRIVER_TASK_FAILED_COUNT": 0,
        "DRIVER_TASK_CAPSET_PROOF": "no",
        "DRIVER_TASK_FAULT_PROOF": "no",
        "DRIVER_TASK_REVOKE_PROOF": "no",
        "DRIVER_TASK_SCHED_PROOF": "no",
        "DRIVER_TASK_AFFINITY_PROOF": "no",
        "DRIVER_TASK_AFFINITY_CONFIGURED": 0,
        "DRIVER_TASK_AFFINITY_APPLIED": 0,
        "DRIVER_TASK_VSPACE_PROOF": "no",
        "DRIVER_TASK_POINTER_FREE_IPC_PROOF": "no",
        "DRIVER_TASK_OWNER_STATE_PROOF": "no",
        "DRIVER_TASK_ACTIVE_NET": "unknown",
        "DRIVER_TASK_BUDGET_OVERRUNS": 0,
        "DRIVER_TASK_LATENCY_PROOFS": 0,
        "SERIAL_RESPONSIVE_PROOF": "no",
        "USB_BURST_PROOF": "no",
        "USB_BURST_DROPS": -1,
        "HDMI_RESPONSIVE_PROOF": "no",
    }


def test_gate_summary_tracks_net_and_driver_task_proof_fields() -> None:
    """Pi 4 closure gates must be machine-checkable beyond USB/WiFi ready."""

    events = normalizer.parse_events(
        [
            "netstats: mode=static policy=wired active=wired standby=wifi "
            "addr_src=static ip=192.168.1.50 gateway=192.168.1.1 dhcp=off",
            "DRIVER_TASK contract=serial service_class=realtime isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated max_service_us=40 observed_service_us=18",
            "SCHED_CONTRACT contract=genet service_class=network-data isolation=root-task-compatibility max_service_us=120 service_us=90",
            "DRIVER_TASK_ACCEPTANCE dedicated_ready=no reason=root-task-compatibility-contracts-active required=4 dedicated=1 compatibility=1",
            "SERIAL_ECHO p95_us=800 max_gap_us=1200",
            "USB_BURST bytes=256 drops=0 max_latency_us=900",
            "HDMI_RESPONSIVE max_gap_ms=9 mirrored_bytes=256",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["NET_ACTIVE"] == "wired"
    assert record["NET_ADDR_SRC"] == "static"
    assert record["NET_DHCP"] == "off"
    assert record["DRIVER_TASK_CONTRACTS"] == 2
    assert record["DRIVER_TASK_DEDICATED"] == 1
    assert record["DRIVER_TASK_COMPATIBILITY"] == 1
    assert record["DRIVER_TASK_DEDICATED_READY"] == "no"
    assert record["DRIVER_TASK_SERIAL_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_USB_DEDICATED"] == "no"
    assert record["DRIVER_TASK_DISPLAY_DEDICATED"] == "no"
    assert record["DRIVER_TASK_NET_DEDICATED"] == "no"
    assert record["DRIVER_TASK_SDIO_DEDICATED"] == "no"
    assert record["DRIVER_TASK_PCIE_DEDICATED"] == "no"
    assert record["DRIVER_TASK_SUBSTRATE_READY"] == "no"
    assert record["DRIVER_TASK_CAPSET_PROOF"] == "no"
    assert record["DRIVER_TASK_FAULT_PROOF"] == "no"
    assert record["DRIVER_TASK_REVOKE_PROOF"] == "no"
    assert record["DRIVER_TASK_SCHED_PROOF"] == "no"
    assert record["DRIVER_TASK_AFFINITY_PROOF"] == "no"
    assert record["DRIVER_TASK_AFFINITY_CONFIGURED"] == 0
    assert record["DRIVER_TASK_AFFINITY_APPLIED"] == 0
    assert record["DRIVER_TASK_VSPACE_PROOF"] == "no"
    assert record["DRIVER_TASK_POINTER_FREE_IPC_PROOF"] == "no"
    assert record["DRIVER_TASK_OWNER_STATE_PROOF"] == "no"
    assert record["DRIVER_TASK_ACTIVE_NET"] == "unknown"
    assert record["DRIVER_TASK_BUDGET_OVERRUNS"] == 0
    assert record["DRIVER_TASK_LATENCY_PROOFS"] == 2
    assert record["SERIAL_RESPONSIVE_PROOF"] == "yes"
    assert record["USB_BURST_PROOF"] == "yes"
    assert record["USB_BURST_DROPS"] == 0
    assert record["HDMI_RESPONSIVE_PROOF"] == "yes"


def test_gate_summary_tracks_driver_task_substrate_proof_fields() -> None:
    """Dedicated closure must prove substrate, capset, fault, revoke, and scheduling."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 mcs=0 "
            "task_count=9 failed_count=0 live_tcb_count=9 "
            "root_authority_retained=yes fault_endpoint_ready=yes revoke_ready=yes "
            "broad_caps_leaked=0 sched=yes affinity=per-driver "
            "affinity_configured=9 affinity_applied=9 "
            "vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes "
            "owner_state=driver-owned live_hot_paths=yes",
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
            "DRIVER_TASK role=serial contract=driver-serial isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated "
            "capset=console-transport unexpected_caps=0 fault_probe=pass revoke_ready=yes "
            "priority=240 observed_service_us=18",
            "DRIVER_TASK role=usb contract=driver-usb isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated "
            "capset=device-only unexpected_caps=0 fault_probe=pass revoke_ready=yes "
            "priority=240 observed_service_us=22",
            "DRIVER_TASK role=display contract=driver-display isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated "
            "capset=display-sink unexpected_caps=0 fault_probe=pass revoke_ready=yes "
            "priority=120 observed_service_us=44",
            "DRIVER_TASK role=net contract=driver-wifi isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated "
            "active_net=cyw43 capset=network-frame-transport unexpected_caps=0 "
            "fault_probe=pass revoke_ready=yes priority=160 observed_service_us=73",
            "DRIVER_TASK role=sdio contract=sdio-host isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated "
            "capset=device-only unexpected_caps=0 fault_probe=pass revoke_ready=yes "
            "priority=180 observed_service_us=47",
            "DRIVER_TASK role=pcie contract=pcie-root isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated "
            "capset=device-only unexpected_caps=0 fault_probe=pass revoke_ready=yes "
            "priority=170 observed_service_us=51",
            "DRIVER_TASK_ACCEPTANCE dedicated_ready=yes substrate=active capset=pass "
            "fault=pass revoke=pass sched=pass affinity=pass active_net=cyw43 required=6 "
            "dedicated=6 compatibility=0 vspace=isolated ipc_abi=shared-ring-command "
            "pointer_free_ipc=yes owner_state=driver-owned",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_CONTRACTS"] == 6
    assert record["DRIVER_TASK_DEDICATED"] == 6
    assert record["DRIVER_TASK_DEDICATED_READY"] == "yes"
    assert record["DRIVER_TASK_SUBSTRATE_READY"] == "yes"
    assert record["DRIVER_TASK_CAPSET_PROOF"] == "yes"
    assert record["DRIVER_TASK_FAULT_PROOF"] == "yes"
    assert record["DRIVER_TASK_REVOKE_PROOF"] == "yes"
    assert record["DRIVER_TASK_SCHED_PROOF"] == "yes"
    assert record["DRIVER_TASK_AFFINITY_PROOF"] == "yes"
    assert record["DRIVER_TASK_FAILED_COUNT"] == 0
    assert record["DRIVER_TASK_AFFINITY_CONFIGURED"] == 9
    assert record["DRIVER_TASK_AFFINITY_APPLIED"] == 9
    assert record["DRIVER_TASK_VSPACE_PROOF"] == "yes"
    assert record["DRIVER_TASK_POINTER_FREE_IPC_PROOF"] == "yes"
    assert record["DRIVER_TASK_OWNER_STATE_PROOF"] == "yes"
    assert record["DRIVER_TASK_LIVE_HOT_PATHS"] == "yes"
    assert record["DRIVER_TASK_SDIO_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_PCIE_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_ACTIVE_NET"] == "cyw43"


def test_gate_summary_explicit_pointer_free_ipc_no_overrides_abi_label() -> None:
    """A contradictory proof line must fail closed on the explicit proof field."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_SUBSTRATE active=yes task_count=9 failed_count=0 live_tcb_count=9 "
            "root_authority_retained=yes fault_endpoint_ready=yes revoke_ready=yes "
            "broad_caps_leaked=0 sched=yes affinity=per-driver "
            "affinity_configured=9 affinity_applied=9 "
            "vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=no "
            "owner_state=driver-owned live_hot_paths=yes",
            "DRIVER_TASK role=serial contract=driver-serial isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated capset=console-transport "
            "unexpected_caps=0 fault_probe=pass revoke_ready=yes priority=240 "
            "observed_service_us=18",
            "DRIVER_TASK role=usb contract=driver-usb isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated capset=device-only unexpected_caps=0 "
            "fault_probe=pass revoke_ready=yes priority=240 observed_service_us=22",
            "DRIVER_TASK role=display contract=driver-display isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated capset=display-sink unexpected_caps=0 "
            "fault_probe=pass revoke_ready=yes priority=120 observed_service_us=44",
            "DRIVER_TASK role=net contract=driver-wifi isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated active_net=cyw43 "
            "capset=network-frame-transport unexpected_caps=0 fault_probe=pass "
            "revoke_ready=yes priority=160 observed_service_us=73",
            "DRIVER_TASK role=sdio contract=sdio-host isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated capset=device-only unexpected_caps=0 "
            "fault_probe=pass revoke_ready=yes priority=180 observed_service_us=47",
            "DRIVER_TASK role=pcie contract=pcie-root isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated capset=device-only unexpected_caps=0 "
            "fault_probe=pass revoke_ready=yes priority=170 observed_service_us=51",
            "DRIVER_TASK_ACCEPTANCE dedicated_ready=yes substrate=active capset=pass "
            "fault=pass revoke=pass sched=pass affinity=pass vspace=isolated "
            "ipc_abi=shared-ring-command pointer_free_ipc=no owner_state=driver-owned required=6 "
            "dedicated=6 compatibility=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_POINTER_FREE_IPC_PROOF"] == "no"
    assert record["DRIVER_TASK_DEDICATED_READY"] == "no"


def test_gate_summary_requires_per_hot_path_owner_state_descriptors() -> None:
    """Aggregate owner_state text cannot prove concrete driver-owned hardware."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 mcs=0 "
            "task_count=9 failed_count=0 live_tcb_count=9 "
            "root_authority_retained=yes fault_endpoint_ready=yes revoke_ready=yes "
            "broad_caps_leaked=0 sched=yes affinity=per-driver "
            "affinity_configured=9 affinity_applied=9 "
            "vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes "
            "owner_state=driver-owned live_hot_paths=yes",
            "DRIVER_TASK role=serial contract=driver-serial isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=18",
            "DRIVER_TASK role=usb contract=driver-usb isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=22",
            "DRIVER_TASK role=display contract=driver-display isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=44",
            "DRIVER_TASK role=net contract=driver-wifi isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated active_net=cyw43 observed_service_us=73",
            "DRIVER_TASK role=sdio contract=sdio-host isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=47",
            "DRIVER_TASK role=pcie contract=pcie-root isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=51",
            "DRIVER_TASK_ACCEPTANCE dedicated_ready=yes substrate=active capset=pass "
            "fault=pass revoke=pass sched=pass affinity=pass active_net=cyw43 required=6 "
            "dedicated=6 compatibility=0 vspace=isolated ipc_abi=shared-ring-command "
            "pointer_free_ipc=yes owner_state=driver-owned",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_OWNER_STATE_PROOF"] == "no"
    assert record["DRIVER_TASK_DEDICATED_READY"] == "no"


def test_gate_summary_rejects_owner_state_hot_path_in_descriptor_field() -> None:
    """The descriptor field proves presence only; hot-path identity is explicit."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_OWNER_STATE contract=serial descriptor=serial-console "
            "owner_state=driver-owned root_pointer=no",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_OWNER_STATE_PROOF"] == "no"


def test_gate_summary_requires_live_hot_path_for_dedicated_role() -> None:
    """Static contract isolation alone must not prove dedicated hot-path ownership."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK role=net contract=driver-wifi isolation=dedicated-sel4-task "
            "live_tcb=no hot_path=root-task-compatibility observed_service_us=73",
            "DRIVER_TASK role=sdio contract=sdio-host isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=root-task-compatibility observed_service_us=47",
            "DRIVER_TASK role=pcie contract=pcie-root isolation=dedicated-sel4-task "
            "live_tcb=no hot_path=dedicated observed_service_us=51",
            "DRIVER_TASK role=serial contract=driver-serial isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=18",
            "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task "
            "service_max_us=40",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_DEDICATED"] == 5
    assert record["DRIVER_TASK_SERIAL_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_USB_DEDICATED"] == "no"
    assert record["DRIVER_TASK_NET_DEDICATED"] == "no"
    assert record["DRIVER_TASK_SDIO_DEDICATED"] == "no"
    assert record["DRIVER_TASK_PCIE_DEDICATED"] == "no"
    assert record["DRIVER_TASK_LATENCY_PROOFS"] == 4


def test_gate_summary_counts_driver_task_budget_overruns() -> None:
    """A budget overrun breadcrumb must be visible to hardware acceptance."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK contract=cyw43 service_class=network-data max_service_us=250",
            "BUDGET_OVERRUN contract=cyw43 budget_overrun=1 service_us=900",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_CONTRACTS"] == 1
    assert record["DRIVER_TASK_BUDGET_OVERRUNS"] == 1
    assert record["DRIVER_TASK_LATENCY_PROOFS"] == 1


def test_gate_summary_tracks_root_console_readiness() -> None:
    events = normalizer.parse_events(
        [
            "[Cohesix] Root console ready (type 'help' for commands)",
            "Cohesix console ready",
            "cohesix> wifi diag",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["ROOT_CONSOLE_READY"] == "yes"
    assert gates.to_record()["ROOT_PROMPT_SEEN"] == "yes"


def test_gate_summary_marks_jumbled_wifi_serial_unclean() -> None:
    events = normalizer.parse_events(
        [
            "wifi:frorte/pi=ap/did/cnn ke= a_dttcrrai=r-ni2eyel",
            "wifi: boot_failure source=live stage=cyw43-load-firmware-fail "
            "exact=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.serial_clean is False
    assert gates.to_record()["SERIAL_CLEAN"] == "no"
    assert any(event.fields.get("serial_error") for event in events)


def test_gate_summary_marks_jumbled_usb_serial_as_usb_unclean() -> None:
    events = normalizer.parse_events(
        [
            "usb:ownership_contract cfg_window=mappedusb: runtime_gate keyboard=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.serial_clean is False
    assert events[0].domain == "usb"
    assert events[0].fields["serial_error"] == "usb-prefix-glued"


def test_gate_summary_marks_bootinfo_panic_as_unclean_halt() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::net::stack] [bootinfo:net] mark=net.init.device "
            "pre=0x0b0f1ce5ca4ecafe post=0x00000000001e2839",
            "BOOTINFO_SNAPSHOT_CORRUPTED phase=net.init last_mark=net.init.device "
            "pre=0x0b0f1ce5ca4ecafe post=0x00000000001e2839 "
            "expected_pre=0x0b0f1ce5ca4ecafe expected_post=0x9ddf1ce5f00dbeef",
            "[PANIC] panicked at apps/root-task/src/bootstrap/bootinfo_snapshot.rs:499:9:",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.serial_clean is False
    assert gates.boot_halted is True
    assert gates.to_record()["PANIC_SEEN"] == "yes"
    assert gates.to_record()["PANIC_REASON"] == "bootinfo-snapshot-corrupted"
    assert gates.to_record()["BOOT_HALT_REASON"] == "bootinfo-snapshot-corrupted"


def test_usb_proof_summary_advances_command_gate() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb proof_summary gate=3 blocker=cmd-event-ring-timeout "
            "controller=ready command=enable-slot-timeout event=missing irq27=timer-only",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_usb_gate_reports_keyboard_runtime_init_blocked_by_net_init() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] pi4 keyboard preseed begin",
            "[local-seat] pi4 xhci preseed begin",
            "[local-seat] xhci mmio preseeded mmio=0x0000000600000000 bytes=0x40000 trusted=0",
            "[local-seat] pi4 keyboard preseed end",
            "[INFO root_task::net::stack] [net-console] init: bringing up backend=bcmgenet-v5 device=cyw43455 mode=dhcp interface=wifi ip=0.0.0.0/0 netmask=0.0.0.0 gateway=0.0.0.0",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x1d100020 len=2048 phase=command-r5 resp=0x00009000 r5=0x8000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 1
    assert gates.usb_blocker == "keyboard-runtime-init-blocked-by-net-init"


def test_parse_fields_preserves_unsupported_operation_detail() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware core-ctrl access stage=prereset-zero-ioctrl "
            "err=unsupported operation: sdio-cmd53-r5-error",
        ]
    )

    assert events[0].fields["err"] == "sdio-cmd53-r5-error"


def test_wifi_exact_prefers_transport_error_over_retry_reason() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware_verify retry retry_reason=initial-fail "
            "err=unsupported operation: sdio-cmd53-r5-error",
            "wifi: contract current=firmware-upload expected=wait-ht-clock "
            "blocker=ht-backplane-cmd53-r5-rejected",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_blocker == "sdio-cmd53-r5-error"
    assert gates.wifi_exact == "sdio-cmd53-r5-error"


def test_gate_summary_preserves_wifi_direct_sdio_r5_over_later_ht_symptom() -> None:
    events = normalizer.parse_events(
        [
            "[cyw43] init failure err=unsupported operation: sdio-cmd53-r5-error",
            "wifi: boot_failure source=live stage=cyw43-load-firmware-fail "
            "exact=cyw43-function2-disabled",
            "wifi: contract current=wait-ht-clock expected=chipclkcsr-ht-avail",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "sdio-cmd53-r5-error"


def test_gate_summary_promotes_post_release_ht_timeout_over_readback_unavailable() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware_verify outcome=readback-unavailable "
            "action=continue-before-armcr4-release "
            "err=unsupported operation: sdio-cmd53-r5-error verified=no",
            "[pi4-wifi] firmware stage=armcr4-release-proof "
            "armcr4-release-proof=cpuhalt-clear-core-up io=0x01 reset=0x00",
            "[pi4-wifi] firmware stage=wait-ht-clock "
            "action=ht-clock-terminal reason=active-ht-timeout-no-ladder "
            "exact_error=cyw43-ht-clock-timeout-before-function2 csr=0x50",
            "[cyw43] init failure stage=cyw43-load-firmware-fail "
            "err=unsupported operation: cyw43-ht-clock-timeout-before-function2",
            "wifi: contract current=firmware-core-control "
            "expected=f1-backplane-core-control "
            "observed=exact=sdio-cmd52-write+clock=41666666Hz "
            "blocker=firmware-core-control path=strict-control-plane",
            "wifi: f2_gate policy=pre-f2-core-control "
            "gate=core-control-blocked-before-f2 f2_enabled=no f2_ready=no "
            "ioex=0x02 iordy=0x02 blocker=sdio-cmd52-write "
            "blocker_phase=pre-f2-core-control",
            "wifi: firmware_proof source=cached upload=upload-range-ok "
            "nvram_tail=nvram-tail-ok rstvec=reset-vector-programmed "
            "cpuhalt=cpuhalt-clear-core-up",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43-ht-clock-timeout-before-function2",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "ht-clock-timeout"
    assert gates.wifi_exact == "cyw43-ht-clock-timeout-before-function2"


def test_gate_summary_preserves_usb_pcie_irq_quiesce_blocker() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "usb: irq_contract irq27=no bridge=yes intx=yes "
            "controller_gate=pcie-irq-quiesce-failed",
            "usb: contract current=controller-init expected=controller-gate-clear "
            "blocker=pcie-irq-quiesce-failed strategy=platform-reset-complete",
            "usb: verdict=policy-skip-before-run focus=controller-gate",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "pcie-irq-quiesce-failed"


def test_gate_summary_refines_usb_timeout_with_live_crcr_snapshot() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "usb: contract current=controller-ready expected=command-ring-recovery",
            "[local-seat] xhci.diag stage=0x0374 tag=cmd-gate-timeout-live-crcr "
            "live_crcr=0x0000000404024000 expected_ptr=0x0000000404024000 "
            "ptr_match=0x0000000000000001",
            "usb: diag_contract stage=0x030b diag_fresh=yes "
            "tag=cmd-poll-only-timeout exact=cmd-poll-only-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-fetch-timeout"


def test_gate_summary_refines_usb_timeout_when_crcr_moves_without_event() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "usb: contract current=controller-ready expected=command-ring-recovery",
            "[local-seat] xhci.diag stage=0x0374 tag=cmd-gate-timeout-live-crcr "
            "live_crcr=0x0000000404024010 expected_ptr=0x0000000404024000 "
            "ptr_match=0x0000000000000000",
            "usb: diag_contract stage=0x030b diag_fresh=yes "
            "tag=cmd-poll-only-timeout exact=cmd-poll-only-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_tracks_usb_command_doorbell_timer_as_poll_pending() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "usb: contract current=controller-ready expected=command-ring-recovery",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] vl805 posted-write flush stage=0x031f "
            "role=command-doorbell offset=0x0100 value=0x00000000 "
            "source=hal-ext-cfg",
            "[local-seat] xhci.diag stage=0x031f tag=cmd-doorbell-post-barrier "
            "doorbell=0x000000000100 target=0x0",
            "Kernel entry via Interrupt, irq 27",
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: contract current=wait-ht-clock expected=chipclkcsr-ht-avail",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-poll-pending"


def test_gate_summary_requires_hal_flush_for_command_doorbell_proof() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "usb: contract current=controller-ready expected=command-ring-recovery",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] xhci.diag stage=0x031f tag=cmd-doorbell-post-barrier "
            "doorbell=0x000000000100 target=0x0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-doorbell-flush-unproven"


def test_gate_summary_endpoint_doorbell_does_not_prove_command_flush() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "usb: contract current=controller-ready expected=command-ring-recovery",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] vl805 posted-write flush stage=0x031f "
            "role=endpoint-doorbell offset=0x010c value=0x00000004 "
            "source=hal-ext-cfg",
            "[local-seat] xhci.diag stage=0x031f tag=cmd-doorbell-post-barrier "
            "doorbell=0x000000000100 target=0x0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-doorbell-flush-unproven"


def test_gate_summary_tracks_usb_halt_during_command_doorbell_write() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000 status_control=0x0000000000002400",
            "[local-seat] xhci.diag stage=0x0353 tag=cmd-event-ring-before-0 "
            "param=0x0000000000000000 status_control=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x0000000000000100 target=0x0000000000000000 "
            "skip_readback=0x0000000000000001",
            "halting...",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-doorbell-write-halt"
    assert gates.timer_irq27_seen


def test_gate_summary_does_not_classify_irq27_as_usb_without_usb_edge() -> None:
    events = normalizer.parse_events(["Kernel entry via Interrupt, irq 27"])

    gates = normalizer.summarize_gates(events)

    assert len(events) == 1
    assert events[0].domain == "kernel"
    assert events[0].stage == "timer-irq"
    assert events[0].fields["irq"] == "27"
    assert gates.usb_gate == 0
    assert gates.usb_blocker == "missing"
    assert gates.timer_irq27_seen
    assert gates.boot_halt_reason == "timer-irq27-observed"
    assert not gates.sdio_irq158_seen
    assert gates.to_record()["SDIO_IRQ158_SEEN"] == "no"


def test_gate_summary_tracks_wifi_sdio_irq158_separately_from_timer_irq27() -> None:
    events = normalizer.parse_events(
        [
            "Kernel entry via Interrupt, irq 27",
            "[pi4-wifi] sdio irq bind irq=158 trigger=Level "
            "handler=3218 notification=3219 badge=159",
            "[pi4-wifi] sdio irq contract irq=158 trigger=level "
            "bound=1 badge=0x9f device_clear=sdio-intstatus+sdhci-cardint "
            "ack=after-clear int_status=0x00000000 int_enable=0x027f003b "
            "signal=0x00000000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.timer_irq27_seen
    assert gates.sdio_irq158_seen
    assert gates.sdio_irq158_bound
    assert gates.sdio_irq158_line == 2
    assert gates.to_record()["TIMER_IRQ27_SEEN"] == "yes"
    assert gates.to_record()["SDIO_IRQ158_SEEN"] == "yes"
    assert gates.to_record()["SDIO_IRQ158_BOUND"] == "yes"


def test_gate_summary_derives_sdio_irq158_bound_from_hal_init_when_irq_breadcrumbs_suppressed() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] hal init: clock=41666666Hz bus_width=4 ioex=0x06 "
            "iordy=0x06 irq_bound=true",
            "[local-seat] pi4 keyboard runtime proof result=online gate=10 "
            "source=first-byte",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.sdio_irq158_seen
    assert gates.sdio_irq158_bound
    assert gates.sdio_irq158_line == 1
    assert gates.to_record()["SDIO_IRQ158_SEEN"] == "yes"
    assert gates.to_record()["SDIO_IRQ158_BOUND"] == "yes"


def test_gate_summary_does_not_mark_fail_closed_sdio_irq158_bound() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio irq bind irq=158 action=fail-closed "
            "reason=cap-mint-failed",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.sdio_irq158_seen
    assert not gates.sdio_irq158_bound
    assert gates.to_record()["SDIO_IRQ158_BOUND"] == "no"


def test_gate_summary_reports_kernel_halt_and_timer_irq27() -> None:
    events = normalizer.parse_events(
        [
            "halting...",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["BOOT_HALTED"] == "yes"
    assert gates.to_record()["TIMER_IRQ27_SEEN"] == "yes"
    assert gates.to_record()["BOOT_HALT_REASON"] == "kernel-halt"


def test_gate_summary_flags_usb_bootloader_handoff_evidence() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci probe params attempt=1/2 "
            "policy=reset-owned-stop-seed origin=reset-owned-stop-seed "
            "mode=none run=run-skip",
            "[local-seat] xhci probe params attempt=2/2 "
            "policy=platform-reset-complete origin=mailbox-reset-complete "
            "mode=platform-reset-complete run=run-cold",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["USB_BOOTLOADER_HANDOFF_SEEN"] == "yes"


def test_gate_summary_flags_legacy_usb_handoff_stage() -> None:
    events = normalizer.parse_events(
        ["[cohesix:usb-trace] stage=handoff-usb-stop-begin input=0"]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["USB_BOOTLOADER_HANDOFF_SEEN"] == "yes"


def test_gate_summary_flags_structured_usb_preserve_state_handoff() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci probe params attempt=1/1 "
            "policy=preserve-state origin=preserve-state run=run-uboot",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["USB_BOOTLOADER_HANDOFF_SEEN"] == "yes"


def test_gate_summary_flags_structured_usb_stop_state_seed() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci probe params attempt=1/1 "
            "policy=full-reset-start origin=live-runtime-default seed=stop-state",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["USB_BOOTLOADER_HANDOFF_SEEN"] == "yes"


def test_gate_summary_flags_compound_usb_stop_seed_route() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb golden_path preflight "
            "route=trusted-high-bar-stop-seed-primary attempt=1/2"
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["USB_BOOTLOADER_HANDOFF_SEEN"] == "yes"


def test_gate_summary_keeps_cold_usb_path_handoff_clean() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci boot contract raw_hint=0x0/0 mmio=0x0/0 "
            "pci_cmd=0x0000/0 handoff=0 irq_quiesced=0 reset_auth=0 "
            "cap_snapshot=0 stop_snapshot=1 "
            "irq_policy=fw-handoff-cold-start-from-snapshot poll_only=1",
            "[local-seat] xhci probe params attempt=1/1 "
            "policy=full-reset-start origin=live-runtime-default "
            "mode=none pre=mailbox-reset-required run=run-default",
            "[local-seat] xhci probe params promoted attempt=1/1 "
            "policy=platform-reset-complete origin=mailbox-reset-complete "
            "mode=platform-reset-complete run=run-cold",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["USB_BOOTLOADER_HANDOFF_SEEN"] == "no"
    assert gates.to_record()["USB_COLD_BOOT_SEEN"] == "yes"
    assert gates.to_record()["USB_STALE_UEFI_HINT_SEEN"] == "no"


def test_gate_summary_flags_stale_uefi_usb_hint() -> None:
    events = normalizer.parse_events(
        [
            "[cohesix] USB host session was not active; xHCI cold boot starts unseeded",
            "[local-seat] pi4 keyboard unavailable detail=usb-keyboard-missing "
            'hint="UEFI vars: XhciPci=0 XhciReload=1 SystemTableMode=1"',
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["USB_COLD_BOOT_SEEN"] == "yes"
    assert gates.to_record()["USB_STALE_UEFI_HINT_SEEN"] == "yes"


def test_gate_summary_ignores_disabled_handoff_label_fields() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci boot contract raw_hint=0x0/0 mmio=0x0/0 "
            "irq_policy=fw-handoff-cold-start-from-snapshot poll_only=1",
            "[local-seat] xhci probe params attempt=1/1 "
            "policy=full-reset-start origin=live-runtime-default "
            "handoff=none seed=none run=run-default",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["USB_BOOTLOADER_HANDOFF_SEEN"] == "no"


def test_gate_summary_preserves_proof_summary_timeout_over_deferred_root_port() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000 status=0x00000000 control=0x00002401",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] xhci.diag stage=0x0357 "
            "tag=cmd-event-ring-timeout-0 param=0x0000000000000000",
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-linux-event-unproven bus=pcie-window "
            "detail=cmd-event-ring-timeout irq27_role=timer-only",
            "[local-seat] usb proof_summary gate=3 "
            "blocker=cmd-event-ring-timeout controller=ready "
            "command=enable-slot-linux-event-unproven event=missing",
            "[local-seat] usb probe path pathway=1 attempt=1/1 "
            "outcome=root-port-sample-deferred progress=controller-ready "
            "command_probe=enable-slot-linux-event-unproven",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_reports_psc_events_as_event_ring_alive() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000 status=0x00000000 control=0x00002401",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x000000000001 target=0x0",
            "[local-seat] xhci.diag stage=0x0308 tag=cmd-wait-other-event "
            "param=0x0000000001000000 status_control=0x0100000000008801 "
            "trb_type=0x0000000000000022",
            "[local-seat] xhci.diag stage=0x0406 tag=cmd-prompt-safe-psc-preserved "
            "psc_count=0x0000000000000005 psc_mask=0x000000000000001f "
            "event_syncs=0x0000000000000005",
            "[local-seat] xhci.diag stage=0x0357 "
            "tag=cmd-event-ring-timeout-0 param=0x0000000000000000",
            "[local-seat] usb proof_summary gate=3 "
            "blocker=cmd-event-ring-timeout controller=ready "
            "command=enable-slot-linux-event-unproven event=psc-only",
        ]
    )

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"
    assert record["USB_EVENT_RING_ALIVE"] == "yes"
    assert record["USB_PSC_DRAIN_COUNT"] == 5
    assert record["USB_PSC_DRAIN_MASK"] == "0x0000001f"


def test_gate_summary_preserves_command_timeout_over_runtime_unavailable() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0357 "
            "tag=cmd-event-ring-timeout-0 param=0x0000000001000000",
            "[local-seat] xhci.diag stage=0x03e4 "
            "tag=cmd-recovery-retry-timeout cmd_addr=0x0000000404027000",
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-timeout bus=pcie-window detail=EnableSlotTimeout "
            "event_generation=uboot-poll-preserved-leading-events",
            "[local-seat] usb probe path pathway=1 attempt=1/1 "
            "outcome=root-port-sample-deferred progress=controller-ready "
            "command_probe=enable-slot-timeout diag_tag=cmd-recovery-retry-timeout",
            "[local-seat] pi4 keyboard unavailable detail=usb-keyboard-missing",
            "[local-seat] pi4 keyboard runtime init result=unavailable",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_promotes_usb_command_timeout_over_pending_timer() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] xhci.diag stage=0x0307 tag=cmd-timeout "
            "a=0x0000000001312d00 b=0x0000000404024010 c=0x14",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-timeout"


def test_gate_summary_treats_usb_doorbell_edges_precisely() -> None:
    expected_by_tag = {
        "cmd-doorbell-write": "cmd-doorbell-write-halt",
        "cmd-doorbell-write-done": "cmd-poll-pending",
    }
    for tag, expected in expected_by_tag.items():
        events = normalizer.parse_events(
            [
                "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
                "usb: contract current=controller-ready expected=command-ring-recovery",
                f"[local-seat] xhci.diag stage=0x030f tag={tag} "
                "doorbell=0x000000000100 target=0x0",
                "Kernel entry via Interrupt, irq 27",
                "wifi: contract current=wait-ht-clock expected=chipclkcsr-ht-avail",
            ]
        )

        gates = normalizer.summarize_gates(events)

        assert gates.usb_gate == 3
        assert gates.usb_blocker == expected


def test_gate_summary_treats_usb_prompt_safe_return_as_event_timeout() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "[local-seat] xhci.diag stage=0x0379 "
            "tag=cmd-prompt-safe-return-to-shell "
            "a=0x0000000404024000 b=0 c=64",
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-timeout bus=pcie-window "
            "action=return-to-shell detail=poll-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_tracks_latest_usb_pre_doorbell_timer_halt() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x031f tag=cmd-doorbell-post-barrier "
            "doorbell=0x000000000100 target=0x0",
            "Kernel entry via Interrupt, irq 27",
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x0353 tag=cmd-event-ring-before-0 "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x0356 tag=cmd-event-ring-before-3 "
            "param=0x0000000000000000",
            "Kernel entry via Interrupt, irq 27",
            "wifi: contract current=wait-ht-clock expected=chipclkcsr-ht-avail",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-pre-doorbell-proof-timer-preempted"


def test_gate_summary_tracks_usb_cmd_submit_timer_halt_after_policy_skip() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb probe path outcome=enumeration-disabled-bootloader-owned "
            "progress=no-controller",
            "[local-seat] xhci root-port command-probe begin "
            "event_candidate_mask=0x0000 verb=no-op bus=pcie-window",
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "enumeration-disabled-bootloader-owned"


def test_gate_summary_keeps_usb_timeout_plan_ahead_of_timer_halt() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] xhci.diag stage=0x036c tag=cmd-gate-timeout-plan-0 "
            "expected_usbcmd_usbsts=0x0000000500000000",
            "[local-seat] xhci.diag stage=0x036f tag=cmd-gate-timeout-plan-3 "
            "expected_erdp=0x0000000404025000",
            "Kernel entry via Interrupt, irq 27",
            "wifi: contract current=wait-ht-clock expected=chipclkcsr-ht-avail",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-live-timeout-snapshot-missing"


def test_gate_summary_reports_linux_captured_command_queued_behind_stale_crcr() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x036c tag=cmd-gate-timeout-plan-0 "
            "expected_usbcmd_usbsts=0x0000000500000000 "
            "expected_ptr=0x000000040404f010",
            "[local-seat] xhci.diag stage=0x036d tag=cmd-gate-timeout-plan-1 "
            "crcr_plan=0x000000040404f001 "
            "dcbaap_plan=0x000000040402b000",
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-linux-captured-timeout bus=pcie-window "
            "action=return-to-shell detail=cmd-event-ring-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-stale-crcr-dequeue"


def test_gate_summary_promotes_prompt_safe_event_ring_timeout_snapshot() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] xhci.diag stage=0x0364 tag=cmd-ring-timeout-0 "
            "param=0x0000000000000000 status_control=0x0000000000005c01",
            "[local-seat] xhci.diag stage=0x0357 tag=cmd-event-ring-timeout-0 "
            "param=0x0000000000000000 status_control=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x036c tag=cmd-gate-timeout-plan-0 "
            "expected_usbcmd_usbsts=0x0000000500000000",
            "[local-seat] xhci.diag stage=0x0377 "
            "tag=cmd-gate-timeout-live-snapshot-deferred "
            "expected_ptr=0x0000000404024000 event_syncs=0x0000000000000010",
            "[local-seat] xhci.diag stage=0x0379 "
            "tag=cmd-prompt-safe-return-to-shell "
            "a=0x0000000404024000 b=0x0000000000000000 "
            "c=0x0000000000000100",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_preserves_usb_live_snapshot_ahead_of_timer_halt() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] xhci.diag stage=0x031f tag=cmd-doorbell-post-barrier "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] xhci.diag stage=0x0374 "
            "tag=cmd-gate-timeout-live-crcr "
            "live_crcr=0x0000000404024001 expected_ptr=0x0000000404024000 "
            "ptr_match=0x0000000000000001",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-fetch-timeout"


def test_gate_summary_preserves_usb_poll_timeout_after_doorbell_timer_halt() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] xhci.diag stage=0x031f tag=cmd-doorbell-post-barrier "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
            "waited=0x00000000001e8480 expected_ptr=0x0000000404024000 "
            "event_syncs=0x0000000000000002",
            "[local-seat] xhci root-port command-probe result=no-op-timeout "
            "bus=pcie-window action=retry-raw-phys detail=Timeout",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-poll-only-timeout"


def test_gate_summary_resets_usb_timeout_detail_on_later_cmd_submit() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
            "waited=0x00000000001e8480 expected_ptr=0x0000000404024000 "
            "event_syncs=0x0000000000000002",
            "usb: xhci_recent[7] line=116 stage=0x030b "
            "tag=cmd-poll-only-timeout exact=cmd-poll-only-timeout",
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] vl805 posted-write flush stage=0x031f "
            "role=command-doorbell offset=0x0100 value=0x00000000 "
            "source=hal-ext-cfg",
            "[local-seat] xhci.diag stage=0x031f tag=cmd-doorbell-post-barrier "
            "doorbell=0x000000000100 target=0x0",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-poll-pending"


def test_gate_summary_tracks_usb_pcie_timeout_then_raw_phys_poll_pending() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci root-port command-probe begin "
            "event_candidate_mask=0x0000 verb=enable-slot "
            "bus=pcie-window",
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
            "waited=0x0000000000000040 expected_ptr=0x0000000404024000 "
            "event_syncs=0x0000000000000001",
            "[local-seat] xhci root-port command-probe result=enable-slot-timeout "
            "bus=pcie-window action=retry-raw-phys detail=Timeout",
            "[local-seat] xhci probe fallback mmio=0x0000000600000000 "
            "from_bus=pcie-window to_bus=phys reason=enable-slot-timeout",
            "[local-seat] xhci root-port command-probe begin "
            "event_candidate_mask=0x0000 verb=enable-slot bus=phys",
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] vl805 posted-write flush stage=0x031f "
            "role=command-doorbell offset=0x0100 value=0x00000000 "
            "source=hal-ext-cfg",
            "[local-seat] xhci.diag stage=0x031f tag=cmd-doorbell-post-barrier "
            "doorbell=0x000000000100 target=0x0",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-poll-pending"


def test_gate_summary_tracks_usb_pcie_window_enable_slot_timeout() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci root-port command-probe begin "
            "event_candidate_mask=0x0000 verb=enable-slot "
            "bus=pcie-window",
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
            "waited=0x0000000000000040 expected_ptr=0x0000000404024000 "
            "event_syncs=0x0000000000000001",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "pcie-window-enable-slot-timeout"


def test_gate_summary_tracks_usb_raw_phys_poll_timeout_after_fallback() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci probe fallback mmio=0x0000000600000000 "
            "from_bus=pcie-window to_bus=phys reason=no-op-timeout",
            "[local-seat] xhci root-port command-probe begin "
            "event_candidate_mask=0x0000 verb=no-op bus=phys",
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
            "waited=0x0000000000000040 expected_ptr=0x0000000004048000 "
            "event_syncs=0x0000000000000001",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "raw-phys-cmd-poll-only-timeout"


def test_gate_summary_latest_boot_drops_stale_usb_timeout() -> None:
    lines = [
        "U-Boot 2026.01-dirty",
        "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
        "param=0x0000000000000000",
        "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
        "waited=0x00000000001e8480 expected_ptr=0x0000000404024000 "
        "event_syncs=0x0000000000000002",
        "U-Boot 2026.01-dirty",
        "[cohesix:root-task] Cohesix boot: root-task online",
        "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
        "param=0x0000000000000000",
        "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
        "doorbell=0x000000000100 target=0x0",
        "[local-seat] vl805 posted-write flush stage=0x031f "
        "role=command-doorbell offset=0x0100 value=0x00000000 "
        "source=hal-ext-cfg",
        "[local-seat] xhci.diag stage=0x031f tag=cmd-doorbell-post-barrier "
        "doorbell=0x000000000100 target=0x0",
        "Kernel entry via Interrupt, irq 27",
    ]

    events = normalizer.parse_events(normalizer.latest_boot_lines(lines))
    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-poll-pending"


def test_gate_summary_reports_root_port_read_timer_after_controller_ready() -> None:
    lines = [
        "U-Boot 2026.01-dirty",
        "[local-seat] xhci probe skipped mmio=0x0000000600000000 "
        "attempt=1/2 policy=reset-owned-stop-seed origin=reset-owned-stop-seed "
        "reason=bootloader-owned-no-fresh-ownership action=fallback-next",
        "usb: golden_path outcome=enumeration-disabled-bootloader-owned pathway=1",
        "U-Boot 2026.01-dirty",
        "[cohesix:root-task] Cohesix boot: root-task online",
        "[local-seat] xhci.diag stage=0x0110 tag=controller-init-complete "
        "ready=0x00000000",
        "[local-seat] xhci root-port sample begin ports=5 passes=4",
        "[local-seat] xhci root-port read-begin index=0 port=1 sample_ports=5",
        "halting...",
        "Kernel entry via Interrupt, irq 27",
    ]

    events = normalizer.parse_events(normalizer.latest_boot_lines(lines))
    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "root-port-read-timer-preempted"


def test_gate_summary_reports_root_port_sample_timer_without_read_breadcrumb() -> None:
    lines = [
        "U-Boot 2026.01-dirty",
        "[cohesix:root-task] Cohesix boot: root-task online",
        "[local-seat] xhci.diag stage=0x0110 tag=controller-init-complete "
        "ready=0x00000000",
        "[local-seat] xhci root-port sample begin ports=5 passes=1 "
        "timer_irq=27 timer_role=kernel-vtimer",
        "halting...",
        "Kernel entry via Interrupt, irq 27",
    ]

    events = normalizer.parse_events(normalizer.latest_boot_lines(lines))
    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "root-port-read-timer-preempted"


def test_gate_summary_reports_reset_pre_usbcmd_timer_after_first_attempt() -> None:
    lines = [
        "U-Boot 2026.01-dirty",
        "[local-seat] xhci probe skipped mmio=0x0000000600000000 "
        "attempt=1/2 policy=reset-owned-stop-seed origin=reset-owned-stop-seed "
        "reason=bootloader-owned-no-fresh-ownership action=fallback-next",
        "usb: golden_path outcome=enumeration-disabled-bootloader-owned pathway=1",
        "[local-seat] xhci probe begin mmio=0x0000000600000000 "
        "attempt=2/2 policy=platform-reset-complete",
        "[local-seat] xhci.diag stage=0x0226 tag=reset-pre-usbcmd-source "
        "a=0x0000000000000000 b=0x0000000000000000 c=0x0000000000000000",
        "halting...",
        "Kernel entry via Interrupt, irq 27",
    ]

    events = normalizer.parse_events(normalizer.latest_boot_lines(lines))
    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "reset-pre-usbcmd-source"


def test_gate_summary_does_not_keep_reset_pre_usbcmd_timer_after_later_diag() -> None:
    lines = [
        "U-Boot 2026.01-dirty",
        "[local-seat] xhci.diag stage=0x0226 tag=reset-pre-usbcmd-source "
        "a=0x0000000000000000 b=0x0000000000000000 c=0x0000000000000000",
        "[local-seat] xhci.diag stage=0x0214 tag=reset-pre-usbsts-read "
        "a=0x0000000000000001 b=0x0000000000000001 c=0x0000000000000000",
        "halting...",
        "Kernel entry via Interrupt, irq 27",
    ]

    events = normalizer.parse_events(normalizer.latest_boot_lines(lines))
    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker != "reset-pre-usbcmd-source-timer-preempted"


def test_gate_summary_reports_brcm_axi_setup_irq27_after_controller_probe() -> None:
    lines = [
        "U-Boot 2026.01-dirty",
        "[cohesix:root-task] Cohesix boot: root-task online",
        "[local-seat] xhci probe begin mmio=0x0000000600000000 "
        "attempt=2/2 policy=platform-reset-complete",
        "[local-seat] xhci.diag stage=0x0111 a=0x00000000a06dd000 "
        "b=0x0000000000000c08 c=0x0000000000000000",
        "halting...",
        "Kernel entry via Interrupt, irq 27",
    ]

    events = normalizer.parse_events(normalizer.latest_boot_lines(lines))
    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "brcm-axi-setup-read"


def test_gate_summary_reports_platform_reset_port_access_disabled() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0110 tag=controller-init-complete",
            "[local-seat] xhci root-port sample skipped "
            "reason=platform-reset-portsc-toxic",
            "[local-seat] xhci root-port command-probe skipped "
            "reason=platform-reset-portsc-toxic "
            "probe=deferred-platform-reset-portsc-toxic",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "port-register-access-disabled"


def test_gate_summary_rejects_deferred_capture_after_command_proof() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0110 tag=controller-init-complete",
            "[local-seat] xhci root-port sample skipped "
            "reason=platform-reset-portsc-toxic",
            "[local-seat] xhci root-port command-probe result=enable-slot-ok",
            "[local-seat] xhci root-port deferred-capture "
            "mask=0x0001 source=pi4-linux-capture command_probe=enable-slot-ok",
            "[local-seat] usb root-enum deferred-port "
            "port=1 speed=3 source=pi4-linux-capture reset=skip",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "captured-root-port-enum"


def test_gate_summary_reports_root_port_reset_timeout_after_connection() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0110 tag=controller-init-complete",
            "[local-seat] xhci root-port stage=detect-slow-hit port=1 "
            "portsc=0x00000203 ccs=1 ped=0 speed=3 pls=0",
            "[local-seat] usb root-enum classify port=1 stage=address "
            "kind=port-reset-timeout dma=high bus=pcie-window",
            "[local-seat] usb root-enum failed port=1 stage=address "
            "dma=high bus=pcie-window detail=PortResetTimeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 5
    assert gates.usb_blocker == "port-reset-timeout"


def test_gate_summary_keeps_usb_poll_timeout_ahead_of_later_timer_halt() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x036c tag=cmd-gate-timeout-plan-0 "
            "expected_usbcmd_usbsts=0x0000000500000000",
            "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
            "waited=0x0000000001312d00 expected_ptr=0x0000000404024000 "
            "event_syncs=0x0000000000000014",
            "[local-seat] xhci.diag stage=0x0377 "
            "tag=cmd-gate-timeout-live-snapshot-deferred "
            "expected_ptr=0x0000000404024000 event_syncs=0x0000000000000014",
            "Kernel entry via Interrupt, irq 27",
            "wifi: contract current=wait-ht-clock expected=chipclkcsr-ht-avail",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-poll-only-timeout"


def test_gate_summary_promotes_usb_fetch_timeout_from_live_crcr_snapshot() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
            "waited=0x0000000001312d00 expected_ptr=0x0000000404024000 "
            "event_syncs=0x0000000000000014",
            "[local-seat] xhci.diag stage=0x0374 "
            "tag=cmd-gate-timeout-live-crcr "
            "live_crcr=0x0000000404024001 expected_ptr=0x0000000404024000 "
            "ptr_match=0x0000000000000001",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-fetch-timeout"


def test_gate_summary_promotes_usb_event_ring_timeout_from_live_crcr_snapshot() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
            "waited=0x0000000001312d00 expected_ptr=0x0000000404024000 "
            "event_syncs=0x0000000000000014",
            "[local-seat] xhci.diag stage=0x0374 "
            "tag=cmd-gate-timeout-live-crcr "
            "live_crcr=0x0000000404024011 expected_ptr=0x0000000404024000 "
            "ptr_match=0x0000000000000000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_preserves_usbcmd_reset_bit_over_event_timeout() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x02e5 "
            "tag=usbcmd-run-write-done reg=0x0000000000000003",
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x0357 "
            "tag=cmd-event-ring-timeout-0 param=0x0000000000000000",
            "[local-seat] usb proof_summary gate=3 "
            "blocker=cmd-event-ring-timeout controller=ready",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "usbcmd-run-preserved-reset-bit"


def test_gate_summary_reports_run_posted_flush_timer_halt() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x02e5 "
            "tag=usbcmd-run-write-done reg=0x0000000000000000 "
            "value=0x0000000000000001 mode=0x0000000000000003",
            "halting...",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "usbcmd-run-posted-flush-halt"
    assert gates.to_record()["BOOT_HALT_REASON"] == "kernel-halt"


def test_gate_summary_classifies_wifi_pre_f2_core_control_failure() -> None:
    events = normalizer.parse_events(
        [
            "wifi: contract current=firmware-core-control "
            "expected=f1-backplane-core-control observed=cmd53-r5",
            "wifi: f2_gate policy=pre-f2-core-control "
            "gate=core-control-blocked-before-f2 f2_enabled=no",
            "wifi: boot_failure source=live stage=cyw43-load-firmware-fail "
            "exact=sdio-cmd53-r5-error sdhci=none",
            "cohesix> nettest",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "pre-f2-core-control"


def test_gate_summary_preserves_armcr4_reset_exact_over_pre_f2_phase() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware core-ctrl access stage=assert-reset op=write8 "
            "err=unsupported operation: sdio-cmd53-r5-error base=0x18103000 off=0x800",
            "wifi: contract current=firmware-core-control "
            "expected=f1-backplane-core-control observed=cmd53-r5",
            "wifi: f2_gate policy=pre-f2-core-control "
            "gate=core-control-blocked-before-f2 f2_enabled=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "armcr4-reset-assert-cmd53-r5-rejected"


def test_gate_summary_clears_armcr4_reset_assert_after_advisory_skip() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware core-ctrl access stage=assert-reset op=write8 "
            "err=unsupported operation: sdio-cmd53-r5-error base=0x18103000 off=0x800",
            "[pi4-wifi] firmware stage=armcr4-passive action=advisory-reset-skip "
            "err=unsupported operation: sdio-cmd53-r5-error "
            "reason=pre-upload-f1-reset-write-rejected",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "none"


def test_gate_summary_tracks_d11_passive_core_control_reject() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware stage=d11-disable core=0 "
            "base=0x18101000 action=upstream-passive",
            "[pi4-wifi] firmware core-ctrl access op=write8-prereset "
            "mode=cmd52-byte-transfer-window fallback=cmd53-byte-transfer-window "
            "base=0x18101000 off=0x408 addr=0x18101408 "
            "window=0x18100000 bus=0x09408",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x95281004 len=4 "
            "phase=command-r5 resp=0x00001800 r5=0x0800 r5_raw=0x1800",
            "[pi4-wifi] firmware stage=d11-disable core=0 "
            "base=0x18101000 action=terminal-disable-fail "
            "err=unsupported operation: sdio-cmd53-r5-error "
            "reason=pre-upload-f1-reset-write-rejected",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "d11-prereset-fgc-cmd53-r5-rejected"


def test_gate_summary_clears_d11_passive_reject_after_advisory_skip() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware stage=d11-disable core=0 "
            "base=0x18101000 action=terminal-disable-fail "
            "err=unsupported operation: sdio-cmd53-r5-error "
            "reason=pre-upload-f1-reset-write-rejected",
            "[pi4-wifi] firmware stage=d11-disable core=0 "
            "base=0x18101000 action=advisory-skip "
            "err=unsupported operation: sdio-cmd53-r5-error "
            "reason=pre-upload-f1-reset-write-rejected",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "none"


def test_gate_summary_classifies_armcr4_reset_assert_cmd52_failure() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware core-ctrl reset-write "
            "mode=cmd53-word-windowed fallback=cmd52-byte-transfer-window "
            "base=0x18103000 off=0x800 addr=0x18103800 "
            "window=0x18100000 bus=0x0b800 fallback_bus=0x03800 "
            "shift=0 inc=1 value=0x01",
            "[pi4-wifi] sdio cmd52 fail op=write-no-cmd53-fallback "
            "fn=1 addr=0x03800 val=0x01 resp=0x00001800 r5=0x0800",
            "wifi: f2_gate policy=pre-f2-core-control "
            "gate=core-control-blocked-before-f2 f2_enabled=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "armcr4-reset-assert-cmd52-r5-rejected"


def test_gate_summary_preserves_no_op_event_timeout_without_summary() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci root-port command-probe "
            "result=no-op-unproven bus=pcie-window "
            "action=return-to-shell detail=cmd-event-ring-timeout "
            "irq27_role=timer-only pcie_irqs=179,175,180",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_preserves_enable_slot_event_timeout_without_summary() -> None:
    for result in (
        "enable-slot-timeout",
        "enable-slot-unproven",
        "enable-slot-linux-event-unproven",
    ):
        events = normalizer.parse_events(
            [
                "[local-seat] xhci root-port command-probe "
                f"result={result} bus=pcie-window "
                "action=return-to-shell detail=cmd-event-ring-timeout "
                "irq27_role=timer-only pcie_irqs=179,175,180",
                "Kernel entry via Interrupt, irq 27",
            ]
        )

        gates = normalizer.summarize_gates(events)

        assert gates.usb_gate == 3
        assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_treats_enable_slot_cleanup_failure_as_command_proof() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-ok-cleanup-failed "
            "bus=pcie-window slot=1 cleanup=disable-slot-timeout "
            "event_generation=uboot-poll-preserved-leading-events",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "none"


def test_gate_summary_rejects_legacy_linux_event_generation_as_command_proof() -> None:
    for result in (
        "enable-slot-linux-event-ok-cleanup-failed",
        "enable-slot-ok",
    ):
        events = normalizer.parse_events(
            [
                "[local-seat] xhci root-port command-probe "
                f"result={result} "
                "bus=pcie-window slot=1 cleanup=disable-slot-timeout "
                "event_generation=linux-shaped-bounded",
            ]
        )

        gates = normalizer.summarize_gates(events)

        assert gates.usb_gate < 4
        assert gates.usb_blocker != "none"


def test_gate_summary_accepts_linux_captured_fallback_after_uboot_recovery_timeout() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-timeout bus=pcie-window "
            "action=recover-polling-event-generation detail=cmd-event-ring-timeout "
            "event_generation=uboot-poll-preserved-leading-events "
            "recovery_event_generation=uboot-timeout-polling-fresh-recovery",
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-recovery-timeout bus=pcie-window "
            "action=probe-linux-captured-event-generation "
            "detail=cmd-event-ring-timeout "
            "event_generation=uboot-timeout-polling-fresh-recovery "
            "uboot_event_generation=uboot-poll-preserved-leading-events "
            "fallback_event_generation=linux-captured-command-event-generation-after-uboot-timeout",
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-linux-captured-ok-cleanup-failed "
            "bus=pcie-window slot=1 cleanup=disable-slot-timeout "
            "cleanup_generation=linux-captured-command-event-generation "
            "action=unlock-port-sampling "
            "reason=linux-captured-command-event-generation-after-uboot-timeout "
            "recovery_source=enable-slot-recovery-timeout "
            "event_generation=linux-captured-command-event-generation-after-uboot-timeout "
            "uboot_event_generation=uboot-poll-preserved-leading-events "
            "uboot_recovery_event_generation=uboot-timeout-polling-fresh-recovery",
            "[local-seat] usb proof_summary gate=4 blocker=none "
            "controller=ready "
            "command=enable-slot-linux-captured-ok-cleanup-failed "
            "event=command-completion "
            "cleanup_generation=linux-captured-command-event-generation "
            "recovery_source=enable-slot-recovery-timeout "
            "event_generation=linux-captured-command-event-generation-after-uboot-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "none"


def test_gate_summary_rejects_linux_captured_fallback_without_recovery_timeout() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-timeout bus=pcie-window "
            "action=return-to-shell detail=cmd-event-ring-timeout "
            "event_generation=uboot-poll-preserved-leading-events",
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-linux-captured-ok "
            "bus=pcie-window slot=1 cleanup=disable-slot-ok "
            "cleanup_generation=linux-captured-command-event-generation "
            "action=unlock-port-sampling "
            "reason=linux-captured-command-event-generation-after-uboot-timeout "
            "event_generation=linux-captured-command-event-generation-after-uboot-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_rejects_recovery_with_linux_cleanup_generation() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-timeout bus=pcie-window "
            "action=recover-polling-event-generation detail=cmd-event-ring-timeout "
            "event_generation=uboot-poll-preserved-leading-events "
            "recovery_event_generation=uboot-timeout-polling-fresh-recovery",
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-recovery-ok bus=pcie-window slot=1 "
            "cleanup=disable-slot-ok cleanup_generation=linux-shaped-bounded "
            "action=unlock-port-sampling recovery_source=enable-slot-timeout "
            "event_generation=uboot-timeout-polling-fresh-recovery "
            "uboot_event_generation=uboot-poll-preserved-leading-events",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_accepts_fresh_polling_recovery_after_uboot_timeout() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-timeout bus=pcie-window "
            "action=recover-polling-event-generation detail=cmd-event-ring-timeout "
            "event_generation=uboot-poll-preserved-leading-events "
            "recovery_event_generation=uboot-timeout-polling-fresh-recovery",
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-recovery-ok bus=pcie-window slot=1 "
            "cleanup=disable-slot-ok cleanup_generation=uboot-poll-only "
            "action=unlock-port-sampling recovery_source=enable-slot-timeout "
            "event_generation=uboot-timeout-polling-fresh-recovery "
            "uboot_event_generation=uboot-poll-preserved-leading-events",
            "[local-seat] usb proof_summary gate=4 blocker=none "
            "controller=ready command=enable-slot-recovery-ok "
            "event=command-completion cleanup_generation=uboot-poll-only "
            "recovery_source=enable-slot-timeout "
            "event_generation=uboot-timeout-polling-fresh-recovery",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "none"


def test_gate_summary_rejects_fresh_polling_recovery_after_no_op_timeout() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci root-port command-probe "
            "result=no-op-timeout recovery_event_generation=uboot-timeout-polling-fresh-recovery "
            "bus=pcie-window action=recover-polling-event-generation "
            "detail=cmd-event-ring-timeout irq27_role=timer-only "
            "pcie_irqs=179,175,180 event_candidate_mask=0x0000 "
            "event_generation=uboot-poll-preserved-leading-events",
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-recovery-ok bus=pcie-window slot=1 "
            "cleanup=disable-slot-ok cleanup_generation=uboot-poll-only "
            "action=unlock-port-sampling recovery_source=no-op-timeout "
            "event_generation=uboot-timeout-polling-fresh-recovery "
            "uboot_event_generation=uboot-poll-preserved-leading-events",
            "[local-seat] usb proof_summary gate=4 blocker=none "
            "controller=ready command=enable-slot-recovery-ok "
            "event=command-completion cleanup_generation=uboot-poll-only "
            "recovery_source=no-op-timeout "
            "event_generation=uboot-timeout-polling-fresh-recovery",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_keeps_no_op_only_success_before_gate4() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci root-port command-probe "
            "result=no-op-ok bus=pcie-window action=unlock-port-sampling "
            "reason=empty-event-ring-command-isolation",
            "[local-seat] usb proof_summary gate=3 blocker=cmd-event-ring-timeout "
            "controller=ready command=no-op-ok event=command-completion",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_promotes_usb_controller_not_running_from_live_state() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x0375 "
            "tag=cmd-gate-timeout-live-state "
            "usbcmd_usbsts=0x0000000000000001 iman_erstsz=0x0000000000000001 "
            "dcbaap=0x0000000404003000",
            "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
            "waited=0x0000000001312d00 expected_ptr=0x0000000404024000 "
            "event_syncs=0x0000000000000014",
            "[local-seat] usb proof_summary gate=3 blocker=cmd-event-ring-timeout "
            "controller=ready command=enable-slot-linux-captured-timeout "
            "event=psc-only command_completion=missing",
            "usb: xhci_recent[6] line=344 stage=0x030e "
            "tag=cmd-poll-only-timeout-last-event",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-controller-not-running"


def test_gate_summary_promotes_usb_controller_not_ready_from_live_state() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x0375 "
            "tag=cmd-gate-timeout-live-state "
            "usbcmd_usbsts=0x0000000100000810 iman_erstsz=0x0000000000000001 "
            "dcbaap=0x0000000404003000",
            "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
            "waited=0x0000000001312d00 expected_ptr=0x0000000404024000 "
            "event_syncs=0x0000000000000014",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-controller-not-ready"


def test_gate_summary_promotes_usb_controller_halted_from_live_state() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x0375 "
            "tag=cmd-gate-timeout-live-state "
            "usbcmd_usbsts=0x0000000100000001 iman_erstsz=0x0000000000000001 "
            "dcbaap=0x0000000404003000",
            "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
            "waited=0x0000000001312d00 expected_ptr=0x0000000404024000 "
            "event_syncs=0x0000000000000014",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-controller-halted"


def test_gate_summary_reports_reset_cnr_timeout_before_command_gate() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb ownership_proof proof_gate=2 target_gate=10 "
            "cfg_live=yes cmd_live=yes mailbox=mailbox-acked",
            "[local-seat] xhci.diag stage=0x0232 tag=reset-cnr-timeout "
            "a=0x0000000000989680 b=0x0000000000000800 c=0x0000000000000000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 2
    assert gates.usb_blocker == "reset-controller-not-ready"


def test_gate_summary_reports_reset_hcrst_timeout_before_command_gate() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb ownership_proof proof_gate=2 target_gate=10 "
            "cfg_live=yes cmd_live=yes mailbox=mailbox-acked",
            "usb: golden_path route=trusted-high-bar-mailbox-reset "
            "current=controller-init next=controller-ready proof_gate=1",
            "[local-seat] xhci.diag stage=0x0231 tag=reset-hcrst-timeout "
            "a=0x0000000000989680 b=0x0000000000000002 c=0x0000000000000000",
            "usb: golden_path outcome=controller-init-failed command_probe=n/a "
            "progress=no-controller proof_gate=1 diag_stage=0x0331 "
            "diag_tag=drop-skip-uninitialized",
            "usb: verdict=controller-init-edge focus=controller-init",
            "usb: runtime_gate keyboard=no first_report=no first_byte=no "
            "proof_gate=0 target_gate=10 next=keyboard-ready "
            "blocker=keyboard-not-ready",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 2
    assert gates.usb_blocker == "reset-hcrst-timeout"


def test_gate_summary_reports_pre_hcrst_halt_timeout_before_command_gate() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb ownership_proof proof_gate=2 target_gate=10 "
            "cfg_live=yes cmd_live=yes mailbox=mailbox-acked",
            "usb: golden_path route=trusted-high-bar-mailbox-reset "
            "current=controller-init next=controller-ready proof_gate=1",
            "[local-seat] xhci.diag stage=0x0222 tag=stop-revalidation-timeout "
            "a=0x0000000005f5e100 b=0x0000000000000000 c=0x0000000005f5e100",
            "usb: golden_path outcome=controller-init-failed command_probe=n/a "
            "progress=no-controller proof_gate=1 diag_stage=0x0331 "
            "diag_tag=drop-skip-uninitialized",
            "usb: verdict=controller-init-edge focus=controller-init",
            "usb: runtime_gate keyboard=no first_report=no first_byte=no "
            "proof_gate=0 target_gate=10 next=keyboard-ready "
            "blocker=keyboard-not-ready",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 2
    assert gates.usb_blocker == "reset-controller-not-halted"


def test_gate_summary_reports_pre_hcrst_cnr_timeout_before_command_gate() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb ownership_proof proof_gate=2 target_gate=10 "
            "cfg_live=yes cmd_live=yes mailbox=mailbox-acked",
            "usb: golden_path route=trusted-high-bar-mailbox-reset "
            "current=controller-init next=controller-ready proof_gate=1",
            "[local-seat] xhci.diag stage=0x0214 tag=stop-revalidation-usbsts-read "
            "a=0x0000000000000811 b=0x0000000000000001 c=0x0000000000000001",
            "[local-seat] xhci.diag stage=0x022e tag=reset-pre-hcrst-cnr-timeout "
            "a=0x0000000005f5e100 b=0x0000000000000811 c=0x0000000005f5e100",
            "usb: golden_path outcome=controller-init-failed command_probe=n/a "
            "progress=no-controller proof_gate=1 diag_stage=0x0331 "
            "diag_tag=drop-skip-uninitialized",
            "usb: verdict=controller-init-edge focus=controller-init",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 2
    assert gates.usb_blocker == "reset-pre-hcrst-controller-not-ready"


def test_gate_summary_reports_recovery_cnr_timeout_after_command_gate() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x03ca "
            "tag=cmd-recovery-cnr-timeout "
            "a=0x0000000000989680 b=0x0000000000000800 c=0x0000000000000000",
            "[local-seat] usb proof_summary gate=3 blocker=cmd-event-ring-timeout "
            "controller=ready command=enable-slot-linux-captured-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "reset-controller-not-ready"


def test_gate_summary_reports_recovery_stop_timeout_as_not_halted() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x03c6 "
            "tag=cmd-recovery-stop-timeout "
            "a=0x0000000000989680 b=0x0000000000000000 c=0x0000000000989680",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 2
    assert gates.usb_blocker == "reset-controller-not-halted"


def test_gate_summary_prefers_latest_wifi_nettest_cause() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: contract current=wait-ht-clock expected=chipclkcsr-ht-avail",
            "wifi: boot_failure source=live stage=cyw43-load-firmware-fail "
            "exact=cyw43-ht-clock-timeout-before-function2",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43-armcr4-release-readback-unavailable",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "armcr4-release-readback-unavailable"


def test_gate_summary_classifies_wifi_devon_timeout() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: contract current=wait-ht-clock expected=chipclkcsr-ht-avail",
            "wifi: boot_failure source=live stage=cyw43-load-firmware-fail "
            "exact=cyw43-sleepcsr-devon-timeout-before-ht",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43-device-on-timeout-before-ht",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "devon-timeout"


def test_gate_summary_keeps_sleep_decode_from_becoming_devon_blocker() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: contract current=wait-ht-clock expected=chipclkcsr-ht-avail",
            "wifi: ht_state chipclk=0x50 ht_req=yes ht_avail=no alp_req=no "
            "alp_avail=yes force_ht=no wake_htwait=yes sleep=0x01 kso=yes "
            "devon=no cardcap=0x06 clock=41666666Hz width=4bit",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "ht-clock-timeout"


def test_gate_summary_tracks_forced_ht_miss_as_ht_timeout() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: ht_state chipclk=0x52 ht_req=yes ht_avail=no alp_req=no "
            "alp_avail=yes force_ht=yes",
            "wifi: f2_gate policy=post-ht-proof gate=block-f2-until-ht "
            "f2_enabled=no f2_ready=no",
            "[pi4-wifi] firmware stage=debug-probe-ht "
            "action=diagnostic-ht-timeout-backplane-live addr=0x0025dec4 "
            "bytes=24 mode=cmd52-byte-windowed chipclk=0x52 "
            "first=0x00000000 last=0x00000000 "
            "checksum=0x00000000 production_continue=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "ht-clock-timeout"


def test_gate_summary_tracks_wifi_ht_backplane_cmd53_data_wait() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: ht_state chipclk=0x52 ht_req=yes ht_avail=no alp_avail=yes",
            "[pi4-wifi] sdhci xfer error cmd=53 arg=0x15000018 len=24 "
            "phase=data-wait err=unsupported operation: sdhci-int-timeout",
            "[pi4-wifi] firmware stage=debug-probe-ht "
            "action=diagnostic-ht-timeout-backplane-unreadable "
            "addr=0x00258000 bytes=24 chipclk=0x52 "
            "err=unsupported operation: sdhci-int-timeout production_continue=no",
            "wifi: boot_failure source=live stage=cyw43-load-firmware-fail "
            "exact=cyw43-ht-clock-timeout-before-function2",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "ht-backplane-cmd53-data-wait"


def test_gate_summary_tracks_wifi_ht_backplane_cmd53_r5_rejection() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: ht_state chipclk=0x52 ht_req=yes ht_avail=no alp_avail=yes",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x1500bd88 len=24 "
            "phase=command-r5 resp=0x00001880 r5=0x0800",
            "[pi4-wifi] firmware stage=debug-probe-ht "
            "action=diagnostic-ht-timeout-backplane-unreadable "
            "addr=0x0025dec4 bytes=24 mode=cmd53-byte-unflagged-24-inc "
            "chipclk=0x52 err=unsupported operation: sdio-cmd53-r5-error "
            "production_continue=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "ht-backplane-cmd53-r5-rejected"


def test_gate_summary_tracks_wifi_chipcommon_socram_remap_r5_rejection() -> None:
    events = normalizer.parse_events(
        [
            "wifi: contract current=firmware-core-control expected=f1-backplane-core-control",
            "[pi4-wifi] firmware stage=chipcommon-config-write "
            "addr=0x18104010 value=0x00000003 path=cmd53-byte-windowed",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x95802004 len=4 "
            "phase=command-r5 resp=0x00001800 r5=0x0800",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "chipcommon-socram-remap-cmd53-r5-rejected"


def test_gate_summary_preserves_wifi_ht_recover_cmd5_timeout_over_nettest() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: ht_state chipclk=0x50 ht_req=yes ht_avail=no alp_avail=yes",
            "[pi4-wifi] sdio card-init phase=cmd5-probe",
            "[pi4-wifi] sdhci cmd error cmd=5 arg=0x00000000 "
            "st=0x00018000 why=timeout",
            "[pi4-wifi] firmware stage=debug-probe-ht "
            "action=ht-clock-recover-retry-fail "
            "exact_error=ht-retry-sdio-card-not-ready phase=card-ready "
            "err=unsupported operation: sdhci-command-error",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "ht-recover-cmd5-timeout"


def test_gate_summary_tracks_wifi_linux_probe_pmu_cmd53_r5_rejection() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware stage=linux-probe-attach-state begin",
            "[pi4-wifi] firmware stage=linux-probe-attach-state "
            "action=wlanreset cardctrl=0x01->0x03",
            "[pi4-wifi] sdhci xfer meta stage=command-r5 cmd=53 "
            "op=write fn=1 addr=0x00603 inc=0 blk=0 count=1 len=1 "
            "blksz=1 blkcnt=1 flagged=0 trn=0x0002",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x900c0601 len=1 "
            "phase=command-r5 resp=0x00001800 r5=0x0800",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "linux-probe-pmu-cmd53-r5-rejected"


def test_gate_summary_tracks_wifi_linux_probe_pmu_write_skip() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware stage=linux-probe-attach-state "
            "action=pmu-res-reload-write-skip addr=0x00180600 "
            "pmu=0x00000000->0x00000001 "
            "err=unsupported operation: sdio-cmd53-r5-error "
            "policy=best-effort exact_error=linux-probe-pmu-write-skip",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "linux-probe-pmu-write-skip"


def test_gate_summary_tracks_wifi_pmu_write_skip_without_exact_error() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware stage=linux-probe-attach-state "
            "action=pmu-res-reload-write-skip addr=0x18000600 "
            "pmu=0x01770181->0x01774181 "
            "err=unsupported operation: sdio-cmd53-r5-error policy=best-effort",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "linux-probe-pmu-write-skip"


def test_gate_summary_tracks_wifi_armcr4_prereset_after_pmu_skip() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware stage=linux-probe-attach-state begin",
            "[pi4-wifi] sdhci xfer meta stage=command-r5 cmd=53 "
            "op=write fn=1 addr=0x00603 inc=0 blk=0 count=1 len=1 "
            "blksz=1 blkcnt=1 flagged=0 trn=0x0002",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x900c0601 len=1 "
            "phase=command-r5 resp=0x00001800 r5=0x0800",
            "[pi4-wifi] firmware stage=linux-probe-attach-state "
            "action=pmu-res-reload-write-skip addr=0x18000600 "
            "pmu=0x01770181->0x01774181 "
            "err=unsupported operation: sdio-cmd53-r5-error policy=best-effort",
            "[pi4-wifi] firmware stage=pre-core-reset-sdio-clock "
            "action=core-reset-clock-ready effective=41666666Hz",
            "[pi4-wifi] firmware core-disable base=0x18103000 "
            "stage=prereset-fgc-clock value=0x23",
            "[pi4-wifi] firmware core-ctrl access op=write8 "
            "base=0x18103000 off=0x408 addr=0x18103408",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x95681004 len=4 "
            "phase=command-r5 resp=0x00001800 r5=0x0800",
            "[pi4-wifi] firmware core-ctrl access stage=prereset-fgc-clock "
            "op=write8 err=unsupported operation: sdio-cmd53-r5-error "
            "base=0x18103000 off=0x408",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "armcr4-prereset-fgc-cmd53-r5-rejected"


def test_gate_summary_tracks_wifi_armcr4_prereset_current_r5_arg() -> None:
    events = normalizer.parse_events(
        [
            "wifi: clock=41666666Hz preferred=50000000Hz width=4bit "
            "ioex=0x02 iordy=0x02",
            "[pi4-wifi] firmware core-disable base=0x18103000 "
            "stage=prereset-fgc-clock value=0x23",
            "[pi4-wifi] firmware core-ctrl access op=write8-prereset "
            "mode=cmd53-word-windowed-prereset fallback=cmd52-byte-current-window "
            "base=0x18103000 off=0x408 addr=0x18103408",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x95681004 len=4 "
            "phase=command-r5 resp=0x00001800 r5=0x0800",
            "[pi4-wifi] firmware core-ctrl access stage=prereset-fgc-clock "
            "op=write8-prereset err=unsupported operation: sdio-cmd53-r5-error "
            "base=0x18103000 off=0x408",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "armcr4-prereset-fgc-cmd53-r5-rejected"


def test_gate_summary_tracks_wifi_socram_prereset_fgc_after_advisory_skips() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware core-disable base=0x18103000 "
            "stage=prereset-fgc-clock action=defer-prereset-write "
            "value=0x23 err=unsupported operation: sdio-cmd53-r5-error "
            "next=assert-reset reason=armcr4-prereset-fgc-rejected",
            "[pi4-wifi] firmware stage=armcr4-passive action=advisory-reset-skip "
            "err=unsupported operation: sdio-cmd53-r5-error",
            "[pi4-wifi] firmware stage=d11-disable action=advisory-skip "
            "err=unsupported operation: sdio-cmd53-r5-error",
            "[pi4-wifi] firmware stage=socram-disable",
            "[pi4-wifi] firmware core-disable base=0x18104000 "
            "stage=prereset-fgc-clock value=0x03",
            "[pi4-wifi] firmware core-ctrl access op=write8-prereset "
            "mode=cmd53-word-windowed-prereset fallback=cmd52-byte-current-window "
            "base=0x18104000 off=0x408 addr=0x18104408",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x95881004 len=4 "
            "phase=command-r5 resp=0x00001800 r5=0x0800",
            "[pi4-wifi] firmware core-ctrl access stage=prereset-fgc-clock "
            "op=write8-prereset err=unsupported operation: sdio-cmd53-r5-error "
            "base=0x18104000 off=0x408",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "socram-prereset-fgc-cmd53-r5-rejected"


def test_gate_summary_tracks_wifi_socram_assert_reset_after_prereset_skip() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware stage=socram-disable",
            "[pi4-wifi] firmware core-disable base=0x18104000 "
            "stage=prereset-zero-ioctrl action=skip value=0x00 "
            "reason=redundant-upstream-socram-zero",
            "[pi4-wifi] firmware core-disable base=0x18104000 "
            "stage=assert-reset value=0x01",
            "[pi4-wifi] firmware core-ctrl access op=write8 "
            "base=0x18104000 off=0x800 addr=0x18104800",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x90810001 len=1 "
            "phase=command-r5 resp=0x00001800 r5=0x0800",
            "[pi4-wifi] firmware core-ctrl access stage=assert-reset "
            "op=write8 err=unsupported operation: sdio-cmd53-r5-error "
            "base=0x18104000 off=0x800",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "socram-assert-reset-cmd53-r5-rejected"


def test_gate_summary_tracks_wifi_socram_clear_reset_after_disable() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware core-disable base=0x18104000 "
            "stage=prereset-zero-ioctrl action=skip value=0x00 "
            "reason=redundant-upstream-socram-zero",
            "[pi4-wifi] firmware core-disable base=0x18104000 "
            "stage=assert-reset-settled detail=upstream-socram-disable-deferred",
            "[pi4-wifi] firmware core-reset base=0x18104000 "
            "stage=clear-reset-primary attempt=1 path=cmd53-word-windowed value=0x00",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x90810004 len=4 "
            "phase=command-r5 resp=0x00001800 r5=0x0800",
            "[pi4-wifi] firmware core-ctrl access stage=clear-reset-primary "
            "op=write8 err=unsupported operation: sdio-cmd53-r5-error "
            "base=0x18104000 off=0x800",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "socram-clear-reset-cmd53-r5-rejected"


def test_gate_summary_tracks_wifi_socram_postreset_clock_write() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware core-reset base=0x18104000 "
            "stage=clear-reset-readback reset=0x00",
            "[pi4-wifi] firmware core-reset base=0x18104000 "
            "stage=postreset-clock-en-write value=0x01",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x90881001 len=1 "
            "phase=command-r5 resp=0x00001800 r5=0x0800",
            "[pi4-wifi] firmware core-ctrl access stage=postreset-clock-en-write "
            "op=write8 err=unsupported operation: sdio-cmd53-r5-error "
            "base=0x18104000 off=0x408",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "socram-postreset-clock-cmd53-r5-rejected"


def test_gate_summary_prefers_terminal_wifi_cmd52_write_failure() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware core-disable base=0x18103000 "
            "stage=prereset-fgc-clock action=defer-prereset-write "
            "value=0x23 err=unsupported operation: sdio-cmd53-r5-error "
            "next=assert-reset reason=armcr4-prereset-fgc-rejected",
            "[pi4-wifi] firmware core-ctrl access stage=assert-reset "
            "op=write8 err=unsupported operation: sdio-cmd52-write "
            "base=0x18103000 off=0x800",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd52-write",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "sdio-cmd52-write"


def test_gate_summary_classifies_firmware_window_cmd52_write_failure() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware stage=write-firmware-window "
            "addr=0x001a8000 reg=high value=0x00 "
            "err=unsupported operation: sdio-cmd52-write "
            "action=window-write-fail blocker=firmware-window-cmd52-write",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=firmware-window-cmd52-write",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "firmware-window-cmd52-write"


def test_gate_summary_preserves_kso_timeout_before_alp() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware stage=pre-write-alp-clock "
            "action=kso-timeout-nonterminal "
            "err=unsupported operation: cyw43-kso-timeout policy=linux-alp-primary",
            "[pi4-wifi] firmware stage=pre-write-alp-clock timeout "
            "csr=0x40 reason=alp-not-ready",
            "[cyw43] init failure stage=cyw43-load-firmware-fail "
            "err=unsupported operation: cyw43-kso-timeout-before-alp",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "cyw43-kso-timeout-before-alp"


def test_gate_summary_caps_wifi_diag_control_plane_after_pre_f2_cmd52_failure() -> None:
    events = normalizer.parse_events(
        [
            "wifi: snapshot source=live stage=cyw43-load-firmware-fail "
            "exact=cyw43-control-plane-sideband-unreadable",
            "wifi: contract current=function1-sideband "
            "expected=f1-sideband-readable observed=blocked",
            "wifi: f2_state=unproven "
            "exact_error=cyw43-control-plane-sideband-unreadable",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=sdio-cmd52-write",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "sdio-cmd52-write"


def test_gate_summary_preserves_linux_shape_wifi_cmd53_data_wait() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: ht_state chipclk=0x52 ht_req=yes ht_avail=no alp_avail=yes",
            "[pi4-wifi] sdhci xfer error cmd=53 arg=0x15bd8818 len=24 "
            "phase=data-wait err=unsupported operation: sdhci-int-timeout",
            "[pi4-wifi] firmware stage=debug-probe-ht "
            "action=diagnostic-ht-timeout-backplane-try-fail "
            "addr=0x0025dec4 fn_addr=0x0dec4 bytes=24 "
            "mode=cmd53-byte-flagged-24-inc inc=1 linux_shape=y "
            "chipclk=0x52 err=unsupported operation: sdhci-int-timeout "
            "exact_error=ht-backplane-cmd53-data-wait production_continue=no",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x14bd8818 len=24 "
            "phase=command-r5 resp=0x00001800 r5=0x0800",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdio-cmd53-r5-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "ht-backplane-cmd53-data-wait"


def test_gate_summary_clears_ht_blocker_after_retry_ht_avail() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "[pi4-wifi] sdhci xfer error cmd=53 arg=0x15bd8818 len=24 "
            "phase=data-wait err=unsupported operation: sdhci-int-timeout",
            "[pi4-wifi] firmware stage=wait-ht-clock "
            "status=ht-retry-readback chipclk=0x70 ht_avail=yes exact_error=none",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 5
    assert gates.wifi_blocker == "none"


def test_gate_summary_promotes_wifi_cmd53_after_cmd52_rejection() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: ht_state chipclk=0x52 ht_req=yes ht_avail=no alp_avail=yes",
            "[pi4-wifi] firmware stage=debug-probe-ht "
            "action=diagnostic-ht-timeout-backplane-cmd52-rejected "
            "addr=0x0025dec4 fn_addr=0x05ec4 bytes=24 "
            "mode=cmd52-byte-windowed chipclk=0x52 "
            "err=unsupported operation: sdio-cmd52-read production_continue=no",
            "[pi4-wifi] firmware stage=debug-probe-ht "
            "action=diagnostic-ht-timeout-backplane-unreadable "
            "addr=0x0025dec4 bytes=24 mode=cmd53-byte-after-cmd52-r5 "
            "chipclk=0x52 err=unsupported operation: sdhci-int-timeout "
            "cmd52_err=unsupported operation: sdio-cmd52-read "
            "production_continue=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "ht-backplane-cmd53-data-wait"


def test_gate_summary_tracks_wifi_cmd52_rejection_before_fallback_result() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: ht_state chipclk=0x52 ht_req=yes ht_avail=no alp_avail=yes",
            "[pi4-wifi] firmware stage=debug-probe-ht "
            "action=diagnostic-ht-timeout-backplane-cmd52-rejected "
            "addr=0x0025dec4 fn_addr=0x05ec4 bytes=24 "
            "mode=cmd52-byte-windowed chipclk=0x52 "
            "err=unsupported operation: sdio-cmd52-read production_continue=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "ht-backplane-cmd52-r5-rejected"


def test_gate_summary_tracks_wifi_chipclkcsr_cmd52_pre_f2_failure() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: ht_state chipclk=0x50 ht_req=yes ht_avail=no alp_avail=yes",
            "[pi4-wifi] firmware stage=debug-probe-ht "
            "action=chipclkcsr-cmd52-read addr=0x0001000e "
            "err=unsupported operation: sdio-cmd52-read production_continue=no",
            "[pi4-wifi] sdhci cmd error cmd=52 arg=0x12001c00 "
            "st=0x00018000 why=timeout",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=chipclkcsr-cmd52-pre-f2",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "chipclkcsr-cmd52-pre-f2"


def test_gate_summary_keeps_chipclkcsr_pre_f2_over_generic_control_plane_error() -> None:
    events = normalizer.parse_events(
        [
            "wifi: f2_gate policy=post-ht-proof gate=block-f2-until-ht "
            "f2_enabled=no f2_ready=no blocker=cyw43-control-plane-sideband-unreadable",
            "[pi4-wifi] sdhci cmd error cmd=52 arg=0x12001c00 "
            "st=0x00018000 why=timeout",
            "[pi4-wifi] debug snapshot stage=console-diag-error source=cached "
            "exact_error=cyw43-control-plane-sideband-unreadable "
            "sdhci_read_diag=none f2_state=unproven",
            "ERR NETTEST reason=policy detail=net-disabled cause=sdhci-command-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "chipclkcsr-cmd52-pre-f2"


def test_gate_summary_does_not_promote_cached_wifi_snapshot_as_live_proof() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "[pi4-wifi] debug snapshot stage=console-diag-error source=cached "
            "exact_error=cyw43-control-plane-sideband-unreadable "
            "sdhci_read_diag=none f2_state=unproven",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 3
    assert gates.wifi_blocker == "unknown"


def test_gate_summary_tracks_wifi_ht_backplane_cmd52_unreadable() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: ht_state chipclk=0x52 ht_req=yes ht_avail=no alp_avail=yes",
            "[pi4-wifi] firmware stage=debug-probe-ht "
            "action=diagnostic-ht-timeout-backplane-unreadable "
            "addr=0x0025dec4 bytes=24 mode=cmd52-byte-windowed chipclk=0x52 "
            "err=unsupported operation: cmd52-timeout production_continue=no",
            "wifi: boot_failure source=live stage=cyw43-load-firmware-fail "
            "exact=cyw43-ht-clock-timeout-before-function2",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "ht-backplane-cmd52-unreadable"


def test_gate_summary_requires_wifi_ht_runtime_evidence() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: contract current=wait-ht-clock expected=chipclkcsr-ht-avail",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 3
    assert gates.wifi_blocker != "ht-clock-timeout"


def test_gate_summary_promotes_latest_wifi_ht_timeout_over_old_reset_edge() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware core-ctrl access stage=assert-reset "
            "base=0x18104000 off=0x800",
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x90080001",
            "wifi: diag stage=before-ht-probe",
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "[pi4-wifi] firmware stage=debug-probe-ht "
            "action=ht-clock-ladder-exhausted "
            "exact_error=cyw43-ht-clock-timeout-before-function2 csr=0x50",
            "wifi: contract current=wait-ht-clock expected=chipclkcsr-ht-avail "
            "observed=chipclk=0x68+clock=41666666Hz",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 4
    assert gates.wifi_blocker == "ht-clock-timeout"


def test_gate_summary_tracks_wifi_ht_available_as_terminal_proof() -> None:
    events = normalizer.parse_events(
        [
            "wifi: ht_state chipclk=0x52 ht_req=yes ht_avail=yes alp_req=no "
            "alp_avail=yes wake_htwait=yes sleep=0x01 kso=yes devon=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 5
    assert gates.wifi_blocker == "none"


def test_normalize_usb_blocker_alias_table_covers_remaining_gates() -> None:
    cases = {
        "event-ring-missing": "cmd-event-ring-timeout",
        "cmd-ring-timeout-0": "cmd-timeout",
        "cmd-fetch-missing": "cmd-fetch-timeout",
        "cmd-submit-timer-halt": "cmd-submit-proof-timer-preempted",
        "cmd-pre-doorbell-timer-halt": "cmd-pre-doorbell-proof-timer-preempted",
        "cmd-poll-pending": "cmd-poll-pending",
        "cmd-doorbell-vtimer-interrupt": "cmd-doorbell-proof-timer-preempted",
        "raw-phys-cmd-doorbell-proof-timer-preempted": (
            "raw-phys-cmd-doorbell-proof-timer-preempted"
        ),
        "cmd-raw-phys-doorbell-proof-timer-preempted": (
            "raw-phys-cmd-doorbell-proof-timer-preempted"
        ),
        "pcie-window-enable-slot-timeout": "pcie-window-enable-slot-timeout",
        "pcie-window-no-op-timeout": "pcie-window-no-op-timeout",
        "pcie-irq-quiesce-failed": "pcie-irq-quiesce-failed",
        "pcie-irq-quiesce-missing": "pcie-irq-quiesce-missing",
        "raw-phys-cmd-poll-only-timeout": "raw-phys-cmd-poll-only-timeout",
        "pcie-config-replay": "pcie-config-replay",
        "brcm-axi-setup-read": "brcm-axi-setup-read",
        "xhci.diag stage=0x0111": "brcm-axi-setup-read",
        "reset-pre-usbcmd-source-timer-preempted": "reset-pre-usbcmd-source",
        "xhci.diag stage=0x0226": "reset-pre-usbcmd-source",
        "halt-revalidation-timeout": "reset-controller-not-halted",
        "stop-revalidation-timeout": "reset-controller-not-halted",
        "reset-pre-hcrst-cnr-timeout": "reset-pre-hcrst-controller-not-ready",
        "pre-hcrst-controller-not-ready": "reset-pre-hcrst-controller-not-ready",
        "root-port-read-timer-preempted": "root-port-read-timer-preempted",
        "root-port-sample-deferred": "root-port-sample-deferred",
        "platform-reset-portsc-toxic": "port-register-access-disabled",
        "xhci.diag stage=0x03f5": "port-register-access-disabled",
        "port-reset-timeout": "port-reset-timeout",
        "port-enable-timeout": "port-enable-timeout",
        "device-not-found": "root-port-device-not-found",
        "address-device-timeout": "address-device-timeout",
        "no-connected-ports": "no-connected-ports",
        "address-failed": "address-failed",
        "invalid-config-value": "invalid-config-value",
        "device-desc failed": "device-descriptor",
        "config-desc failed": "config-descriptor",
        "set-config timeout": "set-config",
        "hid-init-failed": "hid-init-failed",
        "keyboard-missing": "no-keyboard-found",
        "first-report timeout": "hid-first-report",
        "first-byte missing": "keyboard-first-byte",
        "unknown-usb-edge": "unknown-usb-edge",
    }

    for raw, expected in cases.items():
        assert normalizer.normalize_usb_blocker(raw) == expected


def test_normalize_wifi_blocker_alias_table_covers_post_ht_gates() -> None:
    cases = {
        "cyw43-firmware-ready-timeout": "firmware-ready-timeout",
        "firmware-channel-f2": "firmware-channel-f2",
        "cyw43-control-plane-no-reply-linux-f2-armed": "control-plane-no-reply",
        "ht-retry-sdio-card-not-ready phase=card-ready": "ht-recover-cmd5-timeout",
        "linux-probe-pmu-write-skip": "linux-probe-pmu-write-skip",
        "linux-probe-pmu-cmd53-r5-rejected": "linux-probe-pmu-cmd53-r5-rejected",
        "policy=pre-f2-core-control gate=core-control-blocked-before-f2": (
            "pre-f2-core-control"
        ),
        "current=firmware-core-control expected=f1-backplane-core-control": (
            "firmware-core-control"
        ),
        "sdio cmd52 fail op=write-no-cmd53-fallback fn=1 addr=0x03800 val=0x01": (
            "armcr4-reset-assert-cmd52-r5-rejected"
        ),
        "stage=assert-reset base=0x18103000 off=0x800 err=unsupported operation: sdio-cmd53-r5-error": (
            "armcr4-reset-assert-cmd53-r5-rejected"
        ),
        "stage=assert-reset base=0x18102000 off=0x800 err=unsupported operation: sdio-cmd53-r5-error": (
            "armcr4-reset-assert-cmd53-r5-rejected"
        ),
        "sdio cmd53 r5 fail arg=0x95700004": (
            "armcr4-reset-assert-cmd53-r5-rejected"
        ),
        "sdio cmd53 r5 fail arg=0x95500004": (
            "armcr4-reset-assert-cmd53-r5-rejected"
        ),
        "socram-assert-reset-cmd53-r5-rejected": (
            "socram-assert-reset-cmd53-r5-rejected"
        ),
        "stage=assert-reset base=0x18104000 off=0x800 err=unsupported operation: sdio-cmd53-r5-error": (
            "socram-assert-reset-cmd53-r5-rejected"
        ),
        "socram-clear-reset-cmd53-r5-rejected": (
            "socram-clear-reset-cmd53-r5-rejected"
        ),
        "stage=clear-reset-primary base=0x18104000 err=unsupported operation: sdio-cmd53-r5-error": (
            "socram-clear-reset-cmd53-r5-rejected"
        ),
        "socram-postreset-clock-cmd53-r5-rejected": (
            "socram-postreset-clock-cmd53-r5-rejected"
        ),
        "stage=postreset-clock-en-write base=0x18104000 off=0x408 err=unsupported operation: sdio-cmd53-r5-error": (
            "socram-postreset-clock-cmd53-r5-rejected"
        ),
        "socram-prereset-zero-cmd53-r5-rejected": (
            "socram-prereset-zero-cmd53-r5-rejected"
        ),
        "stage=prereset-zero-ioctrl base=0x18104000 off=0x408 err=unsupported operation: sdio-cmd53-r5-error": (
            "socram-prereset-zero-cmd53-r5-rejected"
        ),
        "stage=prereset-fgc-clock base=0x18104000 off=0x408 err=unsupported operation: sdio-cmd53-r5-error": (
            "socram-prereset-fgc-cmd53-r5-rejected"
        ),
        "armcr4-prereset-fgc-cmd53-r5-rejected": (
            "armcr4-prereset-fgc-cmd53-r5-rejected"
        ),
        "sdio cmd53 r5 fail arg=0x90681001": (
            "armcr4-prereset-fgc-cmd53-r5-rejected"
        ),
        "sdio cmd53 r5 fail arg=0x95481004": (
            "armcr4-prereset-fgc-cmd53-r5-rejected"
        ),
        "sdio cmd53 r5 fail arg=0x95281004": (
            "d11-prereset-fgc-cmd53-r5-rejected"
        ),
        "stage=d11-disable action=terminal-disable-fail err=unsupported operation: sdio-cmd53-r5-error": (
            "d11-prereset-fgc-cmd53-r5-rejected"
        ),
        "stage=debug-probe-ht arg=0x12001c00": "chipclkcsr-cmd52-pre-f2",
        "chipclkcsr-cmd52-pre-f2": "chipclkcsr-cmd52-pre-f2",
        "cyw43-control-plane-sideband-unreadable": (
            "control-plane-sideband-unreadable"
        ),
        "cyw43-control-plane-linux-interrupts-deferred": (
            "control-plane-interrupts-deferred"
        ),
        "ioctl-timeout": "ioctl-timeout",
        "join-timeout": "join-timeout",
        "wifi-association-failed": "wifi-association-failed",
        "dhcp-pending": "dhcp-pending",
        "dhcp-failed": "dhcp-failed",
        "not-ready:ipc-buffer": "net-not-ready-ipc-buffer",
        "policy-disabled": "nettest-policy-disabled",
        "selftest-disabled": "nettest-selftest-disabled",
        "unsupported": "nettest-unsupported",
        "nettest timeout": "nettest-failed",
        "unknown-wifi-edge": "unknown-wifi-edge",
    }

    for raw, expected in cases.items():
        assert normalizer.normalize_wifi_blocker(raw) == expected


def test_gate_summary_tracks_usb_command_ring_ready_success() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "usb: contract current=controller-ready expected=command-ring-recovery",
            "[local-seat] xhci root-port command-probe result=enable-slot-ok",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "none"


def test_gate_summary_honors_explicit_usb_proof_gate_fields() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_proof proof_gate=2 target_gate=10 cfg_replay=yes",
            "usb: golden_path outcome=pending progress=controller-ready "
            "proof_gate=3 command_probe=n/a",
            "usb: golden_path outcome=root-port-sample-deferred "
            "progress=controller-ready proof_gate=4 command_probe=enable-slot-ok",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "root-port-sample-deferred"


def test_gate_summary_keeps_usb_post_command_outcome_blocker() -> None:
    events = normalizer.parse_events(
        [
            "usb: golden_path outcome=root-port-sample-deferred "
            "progress=controller-ready command_probe=enable-slot-ok",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "root-port-sample-deferred"


def test_gate_summary_tracks_usb_remaining_enumeration_gates() -> None:
    events = normalizer.parse_events(
        [
            "usb: enum_state phase=root-port-connected outcome=address-failed "
            "port=1 connected_mask=0x0001",
            "[local-seat] usb root-enum failed port=1 stage=address "
            "dma=high bus=pcie-window detail=AddressDeviceTimeout",
            "usb: enum_state phase=device-addressed outcome=device-desc-failed "
            "port=1 connected_mask=0x0001",
            "[local-seat] usb root-enum failed port=1 stage=device-desc "
            "detail=TransferTimeout",
            "usb: enum_state phase=device-configured outcome=hid-init-failed "
            "port=1 connected_mask=0x0001",
            "[local-seat] usb hid attach failed slot=1 iface=0 ep=0x81 "
            "source=direct detail=Protocol",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 7
    assert gates.usb_blocker == "hid-init-failed"


def test_gate_summary_tracks_usb_descriptor_and_set_config_success() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb device-desc ready port=1 vid=0x046d pid=0xc31c "
            "class=0x00 subclass=0x00 proto=0x00 mps0=64 configs=1 bcd_usb=0x0200",
            "[local-seat] usb config-desc ready port=1 total=59 interfaces=2 "
            "config_value=0x01 attrs=0xa0 max_power=50",
            "[local-seat] usb set-config ready port=1 value=0x01",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 7
    assert gates.usb_blocker == "none"


def test_gate_summary_tracks_usb_invalid_config_value_gate() -> None:
    events = normalizer.parse_events(
        [
            "usb: enum_state phase=config-parsed outcome=invalid-config-value "
            "port=1 connected_mask=0x0001",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 7
    assert gates.usb_blocker == "invalid-config-value"


def test_gate_summary_tracks_usb_keyboard_report_and_first_byte() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb hid keyboard ready slot=1 iface=0 ep=0x81 "
            "source=direct layout=boot subclass=0x01 protocol=0x01",
            "[local-seat] usb hid first report shift=0 keys=04,00,00,00,00,00",
            "[local-seat] runtime keyboard first-byte read=1",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 10
    assert gates.usb_blocker == "none"


def test_gate_summary_treats_usb_keyboard_runtime_online_as_ready() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] pi4 keyboard runtime proof result=online gate=10 "
            "source=first-byte",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 10
    assert gates.usb_blocker == "none"


def test_gate_summary_tracks_usb_runtime_gate_contract() -> None:
    events = normalizer.parse_events(
        [
            "usb: runtime_gate keyboard=yes first_report=no first_byte=no "
            "proof_gate=8 target_gate=10 next=hid-first-report "
            "blocker=hid-first-report",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "proof_gate=9 target_gate=10 next=keyboard-first-byte "
            "blocker=keyboard-first-byte",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "proof_gate=10 target_gate=10 next=none blocker=none",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 10
    assert gates.usb_blocker == "none"


def test_gate_summary_tracks_usb_hid_queue_and_report_blockers() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb hid queue-read failed slot=1 iface=0 ep=0x81 "
            "source=direct layout=boot detail=TransferTimeout",
            "[local-seat] usb hid first report pending detail=interrupt-in-no-event",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 8
    assert gates.usb_blocker == "hid-first-report"


def test_gate_summary_tracks_usb_hid_report_decode_diag() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb hid keyboard ready slot=1 iface=0 ep=0x81 "
            "source=direct layout=boot subclass=0x01 protocol=0x01",
            "[local-seat] xhci.diag stage=0x03c1 tag=usb-hid-report-decode-fail "
            "slot_ep=0x0000000100000003 code_payload=0x0000000100000008 "
            "decode_state=0x0000000000000001",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 8
    assert gates.usb_blocker == "hid-report-decode-fail"


def test_gate_summary_tracks_wifi_function2_and_firmware_channel() -> None:
    events = normalizer.parse_events(
        [
            "wifi: ht_state chipclk=0x52 ht_req=yes ht_avail=yes",
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "wifi: f2_gate policy=post-ht-proof f2_enabled=yes f2_ready=yes",
            "wifi: snapshot source=live stage=post-firmware-ready-function2-strict-repoll-fail "
            "exact=firmware-channel-f2",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 6
    assert gates.wifi_blocker == "firmware-channel-f2"


def test_gate_summary_does_not_credit_function2_without_ior2() -> None:
    events = normalizer.parse_events(
        [
            "wifi: ht_state chipclk=0x50 ht_req=yes ht_avail=no",
            "[pi4-wifi] sdio function-ready fn=2 poll=1/3000 ready=0x02 need=0x04",
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x02",
            "[pi4-wifi] sdio function-ready fn=2 action=experimental-continue-without-ready "
            "desired=0x06 ready=0x02",
            "[pi4-wifi] sdio function2 ready-snapshot timeout diagnosis=f2-enable-latched-not-ready "
            "ioex=0x06/y iorx=0x02/y",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate < 6


def test_gate_summary_tracks_wifi_control_plane_breadcrumb_failures() -> None:
    events = normalizer.parse_events(
        [
            "wifi: ht_state chipclk=0x52 ht_req=yes ht_avail=yes",
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[cyw43] control-plane step=event-mask action=fail err=ioctl-timeout",
            "wifi: boot_failure source=live stage=cyw43-init-control-plane-fail "
            "exact=cyw43-control-plane-no-reply-linux-f2-armed",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-no-reply"


def test_gate_summary_tracks_wifi_control_plane_step_err_without_snapshot() -> None:
    events = normalizer.parse_events(
        [
            "wifi: ht_state chipclk=0x52 ht_req=yes ht_avail=yes",
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[cyw43] control-plane step=event-mask action=fail err=ioctl-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "ioctl-timeout"


def test_gate_summary_tracks_missing_wifi_irq158_as_function2_blocker() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[pi4-wifi] firmware stage=setup-firmware-channel "
            "action=fail-closed reason=sel4-irq-unbound "
            "exact_error=cyw43-function2-interrupt-unbound irq=158 "
            "timer_irq=27 next=bind-sdio-irq158",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 6
    assert gates.wifi_blocker == "function2-interrupt-unbound"
    assert gates.wifi_exact == "cyw43-function2-interrupt-unbound"
    assert gates.timer_irq27_seen
    assert not gates.sdio_irq158_bound


def test_gate_summary_preserves_interrupts_deferred_over_cyw43_ioctl_timeout() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "wifi: boot_failure source=live stage=cyw43-init-control-plane-fail "
            "exact=cyw43-control-plane-linux-interrupts-deferred",
            "[cyw43] control-plane step=clm-download action=fail "
            "err=cyw43 protocol error: ioctl-timeout",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43 protocol error: ioctl-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-interrupts-deferred"
    assert gates.wifi_exact == "cyw43-control-plane-linux-interrupts-deferred"


def test_gate_summary_preserves_partial_hint_visibility_over_ioctl_timeout() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[pi4-wifi] control-plane snapshot ioctl-timeout "
            "exact-error=cyw43-control-plane-interrupt-programming-drift",
            "wifi: boot_failure source=live stage=cyw43-init-control-plane-fail "
            "exact=cyw43-control-plane-partial-hint-visibility",
            "[cyw43] control-plane step=clm-download action=fail "
            "err=cyw43 protocol error: ioctl-timeout",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43 protocol error: ioctl-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-partial-hint-visibility"
    assert gates.wifi_exact == "cyw43-control-plane-partial-hint-visibility"


def test_gate_summary_preserves_legacy_gmode_frontier_over_partial_hint_snapshot() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane "
            "step=gmode action=fail err=cyw43 protocol error: ioctl-timeout",
            "wifi: boot_failure source=live stage=cyw43-init-control-plane-fail "
            "exact=cyw43-control-plane-partial-hint-visibility",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43 protocol error: ioctl-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-legacy-gmode-stall"
    assert gates.wifi_exact == "cyw43-control-plane-legacy-gmode-stall"


def test_gate_summary_prefers_boot_control_plane_line_over_later_nettest() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[pi4-wifi] firmware stage=control-plane-reply "
            "action=post-write-no-irq-terminal empty_poll=64/64 "
            "exact_error=",
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane "
            "step=apsta-enable action=fail err=unsupported operation: "
            "cyw43-control-plane-hintless-firstread-no-irq",
            "[WARN root_task::drivers::cyw43] [cyw43] init failure "
            "stage=cyw43-init-control-plane-fail err=unsupported operation: "
            "cyw43-control-plane-hintless-firstread-no-irq",
            "cohesix> nettest",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=unsupported operation: cyw43-control-plane-hintless-firstread-no-irq",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-hintless-firstread-no-irq"
    assert gates.wifi_exact == "cyw43-control-plane-hintless-firstread-no-irq"
    assert gates.wifi_phase == "control-plane-reply"
    assert gates.wifi_blocker_line == 2


def test_gate_summary_tracks_wsec_pmk_bad_argument_over_stale_hint() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane step=join "
            "action=fail err=cyw43 ioctl 0x0000010c failed status=0xfffffffe",
            "wifi: boot_failure source=live stage=cyw43-init-control-plane-fail "
            "exact=cyw43-control-plane-partial-hint-visibility",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "wsec-pmk-bad-argument"
    assert gates.wifi_exact == "wsec-pmk-bad-argument"
    assert gates.wifi_phase == "join"


def test_gate_summary_tracks_wsec_pmk_bad_argument_after_supplicant_fallbacks() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::cyw43] [cyw43] join: firmware-supplicant "
            "path=primary-plain unsupported status=0xffffffe9 action=try-bsscfg-wrapper "
            "reason=known-good-cyw43-fwsup-shape",
            "[WARN root_task::drivers::cyw43] [cyw43] join: firmware-supplicant "
            "path=bsscfg-wrapper unsupported status=0xffffffe9 "
            "action=continue-host-eapol-required reason=firmware-offload-unavailable",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane reply "
            "cmd=0x0000010c id=26 status=0xfffffffe response_len=0 copied=0",
            "[WARN root_task::drivers::cyw43] [cyw43] join: linux-pmk rejected "
            "action=retry-legacy-hex-pmk status=0xfffffffe",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane reply "
            "cmd=0x0000010c id=27 status=0xfffffffe response_len=0 copied=0",
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane step=join "
            "action=fail err=cyw43 ioctl 0x0000010c failed status=0xfffffffe",
            "wifi: boot_failure source=live stage=cyw43-init-control-plane-fail "
            "exact=cyw43-control-plane-partial-hint-visibility",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "wsec-pmk-bad-argument"
    assert gates.wifi_exact == "wsec-pmk-bad-argument"
    assert gates.wifi_phase == "join"
    assert gates.wifi_blocker_line == 8


def test_gate_summary_tracks_firmware_supplicant_unsupported_over_stale_hint() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane step=join action=begin",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane reply "
            "cmd=0x00000107 id=31 status=0xffffffe9 response_len=0 copied=0",
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane step=join "
            "action=fail err=cyw43 ioctl 0x00000107 failed status=0xffffffe9",
            "wifi: f2_state=linux-configured exact_error="
            "cyw43-control-plane-partial-hint-visibility",
            "wifi: f2_gate current=control-plane expected=control-plane-ready "
            "blocker=control-plane-partial-hint-visibility",
            "wifi: boot_failure source=live stage=cyw43-init-control-plane-fail "
            "exact=cyw43-control-plane-partial-hint-visibility",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "firmware-supplicant-unsupported"
    assert gates.wifi_exact == "firmware-supplicant-unsupported"
    assert gates.wifi_phase == "join"


def test_gate_summary_labels_direct_firmware_supplicant_failure_as_join_security() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::cyw43] [cyw43] join: firmware-supplicant "
            "path=primary-plain unsupported status=0xffffffe9 action=try-bsscfg-wrapper "
            "reason=known-good-cyw43-fwsup-shape",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "firmware-supplicant-unsupported"
    assert gates.wifi_exact == "firmware-supplicant-unsupported"
    assert gates.wifi_phase == "join-security"


def test_gate_summary_prefers_terminal_wrapper_supplicant_failure_line() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::cyw43] [cyw43] join: firmware-supplicant "
            "path=primary-plain unsupported status=0xffffffe9 action=try-bsscfg-wrapper "
            "reason=known-good-cyw43-fwsup-shape",
            "[WARN root_task::drivers::cyw43] [cyw43] join: firmware-supplicant "
            "path=bsscfg-wrapper unsupported status=0xffffffe9 action=fail-secure "
            "reason=host-eapol-supplicant-required",
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane step=join "
            "action=fail err=cyw43 protocol error: firmware-supplicant-unsupported",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_blocker == "firmware-supplicant-unsupported"
    assert gates.wifi_exact == "firmware-supplicant-unsupported"
    assert gates.wifi_phase == "join"
    assert gates.wifi_blocker_line == 4


def test_gate_summary_reports_host_eapol_required_after_probe_join() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::cyw43] [cyw43] join: firmware-supplicant "
            "path=bsscfg-wrapper unsupported status=0xffffffe9 "
            "action=continue-host-eapol-required reason=firmware-offload-unavailable",
            "[INFO root_task::drivers::cyw43] [cyw43] join pending mode=deferred "
            "polls=0 ssid_len=6 psk_len=12 secure=yes fwsup=no "
            "completion_rule=host-eapol-required",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "host-eapol-required"
    assert gates.wifi_exact == "host-eapol-required"
    assert gates.wifi_phase == "join-security"


def test_gate_summary_reports_host_eapol_required_after_fail_closed_join_submit() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::cyw43] [cyw43] join: firmware-supplicant "
            "path=bsscfg-wrapper unsupported status=0xffffffe9 "
            "action=continue-host-eapol-required reason=firmware-offload-unavailable",
            "[INFO root_task::drivers::cyw43] [cyw43] join request "
            "path=primary-bsscfg:join action=ready ssid_len=12",
            "[WARN root_task::drivers::cyw43] [cyw43] join failed "
            "reason=host-eapol-required rx_poll=eapol-only dhcp=blocked tx=blocked "
            "mode=join-submit ssid_len=12 psk_len=12",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "host-eapol-required"
    assert gates.wifi_exact == "host-eapol-required"
    assert gates.wifi_phase == "join-security"


def test_gate_summary_reports_host_eapol_required_after_proof_window() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::cyw43] [cyw43] join: firmware-supplicant "
            "path=bsscfg-wrapper unsupported status=0xffffffe9 "
            "action=continue-host-eapol-required reason=firmware-offload-unavailable",
            "[INFO root_task::drivers::cyw43] [cyw43] host-eapol proof window "
            "armed mode=join-submit polls=4608 pre_assoc_polls=512 "
            "post_assoc_polls=4096 rx_poll=eapol-only dhcp=blocked tx=blocked "
            "ssid_len=12 psk_len=12",
            "[WARN root_task::drivers::cyw43] [cyw43] host-eapol proof count=1 "
            "msg=m1 len=117 eapol_ver=2 type=3 body_len=99 body_ok=yes "
            "key_desc=2 key_info=0x008a key_ver=2 pairwise=yes ack=yes "
            "mic=no install=no secure=no encrypted=no nonce=yes replay=yes "
            "kde_len=0 next_action=derive-ptk-send-m2 action=drop "
            "status=host-eapol-required",
            "[INFO root_task::drivers::cyw43] [cyw43] host-eapol proof window "
            "result=eapol-seen mode=join-submit polls=92 assoc=link-up "
            "assoc_poll=88 post_assoc_polls=4 eapol_rx_delta=1 eapol_rx_total=1 "
            "events=2 control=0 empty_polls=89 action=continue-eapol-only-rx",
            "[INFO root_task::drivers::cyw43] [cyw43] host-eapol pending "
            "mode=join-submit status=wifi-host-eapol-pending assoc=yes "
            "rx=eapol-only data=blocked creds=12/12 eapol_rx=1 "
            "limit=60000 action=wait-m1",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "host-eapol-required"
    assert gates.wifi_exact == "host-eapol-required"
    assert gates.wifi_phase == "join-security"


def test_gate_summary_preserves_live_host_eapol_pending_label() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] host-eapol proof window "
            "result=not-yet-seen mode=join-submit polls=16384 assoc=link-up "
            "eapol_rx_delta=0 eapol_rx_total=0 action=defer-eapol-only-rx",
            "[INFO root_task::drivers::cyw43] [cyw43] host-eapol pending "
            "mode=join-submit status=wifi-host-eapol-pending assoc=yes "
            "rx=eapol-only data=blocked creds=12/12 eapol_rx=0 "
            "limit=60000 action=wait-m1",
            "netstats: wifi_assoc=1 wifi_link=1 eapol_rx=0 eapol_start=0 eapol_secure=0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "wifi-host-eapol-pending"
    assert gates.wifi_exact == "wifi-host-eapol-pending"
    assert gates.wifi_phase == "join-security"


def test_gate_summary_preserves_host_eapol_required_over_deferred_eapol_start() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] join failed "
            "reason=host-eapol-required rx_poll=eapol-only dhcp=blocked tx=blocked "
            "mode=join-submit ssid_len=12 psk_len=12",
            "[INFO root_task::drivers::cyw43] [cyw43] host-eapol "
            "action=eapol-start mode=deferred polls=2048 limit=60000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "host-eapol-required"
    assert gates.wifi_exact == "host-eapol-required"


def test_gate_summary_preserves_host_eapol_after_ready_and_panic() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio irq bind irq=158 trigger=Level handler=3225 "
            "notification=3226 badge=159",
            "[INFO root_task::drivers::cyw43] [cyw43] join pending mode=deferred "
            "polls=0 ssid_len=12 psk_len=12 secure=yes fwsup=no "
            "completion_rule=host-eapol-required",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane step=join action=ready",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane step=init-complete action=ready",
            "[INFO root_task::drivers::cyw43] [cyw43] ready: mac=88-a2-9e-66-59-10 "
            "clock=41666666Hz bus_width=4 ioex=0x06",
            "[INFO root_task::net::stack] [net-console] cyw43455 device initialized: "
            "mac=88-a2-9e-66-59-10 interface=wifi bringup_status=wifi-host-eapol-required",
            "BOOTINFO_SNAPSHOT_CORRUPTED phase=net.init last_mark=net.init.device "
            "pre=0x0b0f1ce5ca4ecafe post=0x00000000001e2839 "
            "expected_pre=0x0b0f1ce5ca4ecafe expected_post=0x9ddf1ce5f00dbeef",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "host-eapol-required"
    assert gates.wifi_exact == "host-eapol-required"
    assert gates.panic_seen is True
    assert gates.serial_clean is False


def test_gate_summary_tracks_bdc_event_over_stale_partial_hint_visibility() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[pi4-wifi] firmware stage=control-plane-reply "
            "action=strict-frame-indicated-ready frame_len=12",
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane "
            "step=clm-download action=fail err=cyw43 protocol error: bdc-event",
            "[pi4-wifi] control-plane snapshot cyw43-init-control-plane-fail "
            "diag f2=set/set/set blocker=interrupt-programming-drift",
            "wifi: boot_failure source=live stage=cyw43-init-control-plane-fail "
            "exact=cyw43-control-plane-partial-hint-visibility",
            "[cyw43] init snapshot stage=cyw43-init-control-plane-fail snapshot=WifiDebugSnapshot",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-bdc-event"
    assert gates.wifi_exact == "cyw43-control-plane-bdc-event"


def test_gate_summary_tracks_bdc_event_after_firmware_readback_warning() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware_verify outcome=readback-unavailable "
            "action=continue-before-armcr4-release err=unsupported operation: "
            "sdio-cmd53-r5-error verified=no",
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[pi4-wifi] firmware stage=control-plane-write "
            "action=linux-f2-write-shape frame_len=1536",
            "[pi4-wifi] firmware stage=control-plane-reply "
            "action=post-write-hintless-firstread-ready frame_len=12",
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane "
            "step=clm-download action=fail err=cyw43 protocol error: bdc-event",
            "wifi: boot_failure source=live stage=cyw43-init-control-plane-fail "
            "exact=cyw43-control-plane-partial-hint-visibility",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-bdc-event"
    assert gates.wifi_exact == "cyw43-control-plane-bdc-event"


def test_gate_summary_tracks_post_control_write_idle_loop_over_stale_readback() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware_verify outcome=readback-unavailable "
            "action=continue-before-armcr4-release err=unsupported operation: "
            "sdio-cmd53-r5-error verified=no",
            "[pi4-wifi] firmware stage=wait-ht-clock active-ht-ready csr=0xd0",
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[pi4-wifi] firmware ready mailbox=0x00040008 version=4",
            "[pi4-wifi] firmware stage=control-plane-write "
            "action=linux-f2-write-shape frame_len=1536 request_len=1536",
            *[
                "[pi4-wifi] sdio xfer chunk fn=1 op=read base=0x0c020 "
                "chunk=0x0c020 off=0 len=4 inc=1"
                for _ in range(64)
            ],
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-reply-idle-loop"
    assert gates.wifi_exact == "control-plane-reply-idle-loop"


def test_gate_summary_tracks_hintless_firstread_no_irq_terminal() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[pi4-wifi] firmware stage=control-plane-reply "
            "action=post-write-no-irq-terminal empty_poll=64/64 "
            "exact_error=cyw43-control-plane-hintless-firstread-no-irq",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-hintless-firstread-no-irq"
    assert gates.wifi_exact == "cyw43-control-plane-hintless-firstread-no-irq"


def test_gate_summary_tracks_host_card_int_without_dongle_source() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio function-ready fn=2 block=512 ready=0x06",
            "[pi4-wifi] firmware stage=control-plane-reply "
            "action=post-write-no-frame-source-terminal empty_poll=64/64 "
            "int_status=0x00000000/y sdhci=0x00000000 observed_sdhci=0x00000100 "
            "card_int=y exact_error=cyw43-control-plane-host-card-int-no-dongle-source",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-host-card-int-no-dongle-source"
    assert gates.wifi_exact == "cyw43-control-plane-host-card-int-no-dongle-source"


def test_gate_summary_preserves_host_card_int_no_dongle_source_over_later_diag_snapshot_after_keyboard_proof() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] hal init: clock=41666666Hz bus_width=4 ioex=0x06 "
            "iordy=0x06 irq_bound=true",
            "[local-seat] pi4 keyboard runtime proof result=online gate=10 "
            "source=first-byte",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "preinit step=mpc action=begin",
            "[WARN root_task::drivers::cyw43] [cyw43] iovar set failed "
            "name=mpc err=unsupported operation: "
            "cyw43-control-plane-host-card-int-no-dongle-source",
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane "
            "preinit step=mpc action=fail err=unsupported operation: "
            "cyw43-control-plane-host-card-int-no-dongle-source",
            "wifi: boot_failure source=live stage=cyw43-init-control-plane-fail "
            "exact=cyw43-control-plane-partial-hint-visibility "
            "sdhci=none f2_state=linux-configured",
            "cohesix> ERR NETTEST reason=policy detail=net-disabled "
            "cause=unsupported operation: "
            "cyw43-control-plane-host-card-int-no-dongle-source",
            "cohesix> ERR NETSTATS reason=policy detail=net-disabled "
            "cause=unsupported operation: "
            "cyw43-control-plane-host-card-int-no-dongle-source",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 10
    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-host-card-int-no-dongle-source"
    assert gates.wifi_exact == "cyw43-control-plane-host-card-int-no-dongle-source"
    assert gates.wifi_phase == "mpc"


def test_gate_summary_treats_pre_mpc_bus_rxglom_disable_as_progress_not_rxglom_blocker() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=bus-rxglom-disable-bounded-rx action=begin",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane reply "
            "cmd=0x00000107 id=10 status=0x00000000 response_len=15 "
            "copied=15 sdpcm_seq=10 sdpcm_credit=30",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=bus-rxglom-disable-bounded-rx action=ready",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "preinit step=mpc action=begin",
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane "
            "preinit step=mpc action=fail err=unsupported operation: "
            "cyw43-control-plane-host-card-int-no-dongle-source",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-host-card-int-no-dongle-source"
    assert gates.wifi_blocker != "cyw43-rxglom-unsupported"


def test_gate_summary_tracks_wifi_preinit_substep_failure() -> None:
    events = normalizer.parse_events(
        [
            "[cyw43] control-plane preinit step=mpc action=begin",
            "[cyw43] control-plane preinit step=mpc action=fail err=ioctl-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "ioctl-timeout"


def test_gate_summary_tracks_pi4_wifi_boot_deferral_for_local_seat() -> None:
    events = normalizer.parse_events(
        [
            "[net-console] deferred reason=pi4-local-seat-explicit-wifi "
            "action=root-console-wait-for-wifi",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 1
    assert gates.wifi_blocker == "boot-waiting-for-wifi"


def test_gate_summary_keeps_legacy_serial_first_wifi_deferral_blocked() -> None:
    events = normalizer.parse_events(
        [
            "[net-console] deferred reason=pi4-local-seat-explicit-wifi "
            "action=serial-root-console-first",
            "Cohesix console ready",
            "cohesix> ",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 1
    assert gates.wifi_blocker == "boot-deferred-root-console"
    assert gates.root_console_ready
    assert gates.root_prompt_seen


def test_gate_summary_tracks_nettest_wifi_deferred_until_root_console() -> None:
    events = normalizer.parse_events(
        [
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=wifi-net-console-deferred-until-root-console:"
            "pi4-local-seat-explicit-wifi",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 1
    assert gates.wifi_blocker == "boot-deferred-root-console"


def test_gate_summary_tracks_pre_root_wifi_pending_detail() -> None:
    events = normalizer.parse_events(
        [
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=wifi-net-console-pending-before-root-console:"
            "pi4-local-seat-explicit-wifi",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 1
    assert gates.wifi_blocker == "boot-waiting-for-wifi"


def test_gate_summary_tracks_root_console_waiting_for_wifi() -> None:
    events = normalizer.parse_events(
        [
            "[net-console] root console waiting "
            "reason=wifi-not-ready action=wait-for-wifi",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 1
    assert gates.wifi_blocker == "boot-waiting-for-wifi"


def test_gate_summary_preserves_deferred_wifi_failure_over_root_wait() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] control-plane snapshot ioctl-timeout "
            "exact-error=cyw43-control-plane-interrupt-programming-drift",
            "[pi4-wifi] debug snapshot stage=cyw43-init-control-plane-fail "
            "exact_error=cyw43-control-plane-partial-hint-visibility",
            "[net-console] deferred failed detail=cyw43 protocol error: ioctl-timeout",
            "[net-console] root console waiting "
            "reason=wifi-not-ready action=wait-for-wifi",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-partial-hint-visibility"


def test_gate_summary_preserves_cyw43_mac_length_failure() -> None:
    events = normalizer.parse_events(
        [
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane "
            "step=read-mac action=fail err=cyw43 protocol error: "
            "cur-etheraddr-len",
            "[net-console] deferred failed detail=cyw43 protocol error: "
            "cur-etheraddr-len",
            "[INFO event] [event] root console banner emitted",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-cur-etheraddr-len"
    assert gates.wifi_exact == "cyw43-protocol-error-cur-etheraddr-len"
    assert gates.wifi_phase == "read-mac"
    assert gates.root_console_ready
    assert not gates.root_prompt_seen


def test_gate_summary_tracks_nettest_usb_first_boot_deferral() -> None:
    events = normalizer.parse_events(
        [
            "ERR NETTEST reason=policy detail=net-disabled cause=deferred "
            "for Pi4 local-seat USB boot (local-seat-usb-first-wifi)",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 1
    assert gates.wifi_blocker == "boot-deferred-local-seat-usb"


def test_gate_summary_tracks_wifi_join_and_dhcp_gates() -> None:
    events = normalizer.parse_events(
        [
            "[cyw43] ready: mac=02:43:4f:48:58:55 clock=41666666Hz "
            "bus_width=4bit ioex=0x06",
            JOIN_COMPLETE_SECURE,
            "[dhcp] start ready interface=wifi now_ms=100",
            "[net] not-ready gate tripped: want=net-selftest reason=dhcp-pending",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 8
    assert gates.wifi_blocker == "dhcp-pending"


def test_gate_summary_requires_secure_join_completion_proof_fields() -> None:
    events = normalizer.parse_events(
        [
            "[cyw43] ready: mac=02:43:4f:48:58:55 clock=41666666Hz "
            "bus_width=4bit ioex=0x06",
            "[cyw43] join complete mode=deferred polls=3 secure=yes "
            "completion_rule=set-ssid set_ssid=yes fwsup=no psk_sup=no "
            "psk_status=0x00000000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "join-completion-unproven"


def test_gate_summary_does_not_report_dhcp_pending_before_wifi_start_ready() -> None:
    events = normalizer.parse_events(
        [
            JOIN_COMPLETE_OPEN,
            "[net] not-ready gate tripped: want=net-selftest reason=dhcp-pending",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 8
    assert gates.wifi_blocker == "dhcp-not-started"


def test_gate_summary_tracks_link_down_after_join_before_dhcp() -> None:
    events = normalizer.parse_events(
        [
            JOIN_COMPLETE_SECURE,
            "[dhcp] start deferred reason=device-bringup "
            "status=wifi-link-down now_ms=125",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 8
    assert gates.wifi_blocker == "wifi-link-down"


def test_gate_summary_tracks_rxglom_as_explicit_gate8_blocker() -> None:
    events = normalizer.parse_events(
        [
            JOIN_COMPLETE_OPEN,
            "[cyw43] rx glom frame unsupported len=1024 descriptor=true "
            "action=drop reason=rxglom-disabled-bounded-rx",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 8
    assert gates.wifi_blocker == "cyw43-rxglom-unsupported"


def test_gate_summary_tracks_wifi_join_pending_evidence() -> None:
    events = normalizer.parse_events(
        [
            "[cyw43] control-plane step=init-complete action=ready",
            "[cyw43] join pending mode=deferred polls=0 ssid_len=8 psk_len=12",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "join-pending"


def test_gate_summary_reports_post_join_programming_latch_loop_over_recovered_r5() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio cmd53 r5 fail arg=0x95681004 len=4 "
            "phase=command-r5 resp=0x00009000 r5=0x8000",
            "[pi4-wifi] firmware stage=control-plane-reply "
            "action=strict-frame-indicated-ready frame_len=28",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "reply cmd=0x00000107 id=6 status=0x00000000 response_len=1420",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=up action=ready",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=gmode action=skip optional=yes "
            "reason=linux-station-path-does-not-set-legacy-gmode",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=join action=begin",
            *[
                "[pi4-wifi] sdio xfer chunk fn=1 op=read base=0x0c020 "
                "chunk=0x0c020 off=0 len=4 inc=1"
                for _ in range(16)
            ],
            *[
                "[pi4-wifi] firmware stage=control-plane-reply "
                "action=sdio-irq-device-clear intstatus=0x00800000/y "
                "serviced=0x00000000 sdhci=0x00000100 card_int=true "
                "source_state=host-card-int-latch-only"
                for _ in range(8)
            ],
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "join-programming-host-latch-loop"
    assert gates.wifi_exact == "cyw43-join-programming-host-latch-loop"
    assert gates.wifi_phase == "join"
    assert gates.wifi_blocker_line > 0


def test_gate_summary_reports_primary_bsscfg_wrapper_join_security_loop() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "reply cmd=0x00000002 id=18 status=0x00000000 response_len=0",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=up action=ready",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=join action=begin",
            "[pi4-wifi] firmware stage=control-plane-write "
            "action=linux-f2-write-shape frame_len=56",
            *[
                "[pi4-wifi] sdio xfer chunk fn=1 op=read base=0x0c020 "
                "chunk=0x0c020 off=0 len=4 inc=1"
                for _ in range(16)
            ],
            *[
                "[pi4-wifi] firmware stage=control-plane-reply "
                "action=sdio-irq-device-clear intstatus=0x00800000/y "
                "serviced=0x00000000 sdhci=0x00000100 card_int=true "
                "source_state=host-card-int-latch-only"
                for _ in range(8)
            ],
            "[WARN root_task::drivers::cyw43] [cyw43] iovar set failed "
            "name=bsscfg:wsec err=cyw43 protocol error: ioctl-timeout",
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane "
            "step=join action=fail err=cyw43 protocol error: ioctl-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "primary-bsscfg-wrapper-join-security-loop"
    assert gates.wifi_exact == "cyw43-primary-bsscfg-wrapper-join-security-loop"
    assert gates.wifi_phase == "join"
    assert gates.wifi_blocker_line > 0


def test_gate_summary_reports_bsscfg_supplicant_wrapper_join_security_loop() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=join action=begin",
            "[INFO root_task::drivers::cyw43] [cyw43] iovar set begin "
            "name=bsscfg:sup_wpa len=8",
            "[WARN root_task::drivers::cyw43] [cyw43] iovar set failed "
            "name=bsscfg:sup_wpa err=cyw43 protocol error: ioctl-no-progress-after-frame "
            "exact=cyw43-join-security-bsscfg-sup-wpa-loop",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "join-security-bsscfg-sup-wpa-loop"
    assert gates.wifi_exact == "cyw43-join-security-bsscfg-sup-wpa-loop"
    assert gates.wifi_phase == "join"


def test_gate_summary_reports_wsec_first_join_security_loop() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "reply cmd=0x00000002 id=18 status=0x00000000 response_len=0",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=up action=ready",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=join action=begin",
            "[pi4-wifi] firmware stage=control-plane-write "
            "action=linux-f2-write-shape frame_len=48",
            *[
                "[pi4-wifi] sdio xfer chunk fn=1 op=read base=0x0c020 "
                "chunk=0x0c020 off=0 len=4 inc=1"
                for _ in range(16)
            ],
            *[
                "[pi4-wifi] firmware stage=control-plane-reply "
                "action=sdio-irq-device-clear intstatus=0x00800000/y "
                "serviced=0x00000000 sdhci=0x00000100 card_int=true "
                "source_state=host-card-int-latch-only"
                for _ in range(8)
            ],
            "[WARN root_task::drivers::cyw43] [cyw43] iovar set failed "
            "name=wsec err=cyw43 protocol error: ioctl-timeout",
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane "
            "step=join action=fail err=cyw43 protocol error: ioctl-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "join-security-wsec-first-loop"
    assert gates.wifi_exact == "cyw43-join-security-wsec-first-loop"
    assert gates.wifi_phase == "join"
    assert gates.wifi_blocker_line > 0


def test_gate_summary_reports_runtime_rx_host_latch_spam_before_eapol() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=up action=ready",
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=join action=begin",
            "[INFO root_task::drivers::cyw43] [cyw43] join pending "
            "mode=deferred polls=0 ssid_len=12 psk_len=12 secure=yes "
            "completion_rule=host-eapol-required",
            "[INFO root_task::drivers::cyw43] [cyw43] host-eapol "
            "poll mode=deferred polls=1 limit=60000 assoc=yes eapol_rx=0 "
            "eapol_start_sent=7 action=wait-m1",
            *[
                "[pi4-wifi] firmware stage=runtime-rx "
                "action=irq-latched-firstread-invalid attempt=1 packet=0x0000 "
                "len_inv=0x0000 seq=0x00 channel=0x00 "
                "int_status=0x00000000/y sdhci=0x00000100"
                for _ in range(8)
            ],
            *[
                "[pi4-wifi] firmware stage=runtime-rx "
                "action=no-frame-source-after-firstread rframe=0x0000 "
                "int_status=0x00000000/y sdhci=0x00000100 card_int=y "
                "clear=defer-before-drain reason=preserve-eapol-rx-latch"
                for _ in range(8)
            ],
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "runtime-rx-host-latch-spam"
    assert gates.wifi_exact == "cyw43-runtime-rx-host-latch-spam"
    assert gates.wifi_phase == "runtime-rx"
    assert gates.wifi_blocker_line > 0


def test_gate_summary_reports_wpa_auth_initial_join_security_loop() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=join action=begin",
            "[INFO root_task::drivers::cyw43] [cyw43] iovar set begin "
            "name=wpa_auth len=4",
            "[pi4-wifi] firmware stage=control-plane-write "
            "action=linux-f2-write-shape frame_len=52",
            "[INFO root_task::drivers::cyw43] [cyw43] event type=54 "
            "flags=0x0000 status=0x00000000 reason=0x00000000 auth=0x00000000",
            *[
                "[pi4-wifi] firmware stage=control-plane-reply "
                "action=sdio-irq-host-latch-cleared irq=158 badge=0x9f "
                "source=0x00000000 source_readable=y card_int=y progress=no"
                for _ in range(8)
            ],
            "[WARN root_task::drivers::cyw43] [cyw43] ioctl "
            "no-progress-after-frame cmd=0x00000107 id=19 no_progress_polls=128 "
            "nonmatching_frames=1 cached_exact_error= action=fail-fast",
            "[WARN root_task::drivers::cyw43] [cyw43] iovar set failed "
            "name=wpa_auth err=cyw43 protocol error: ioctl-no-progress-after-frame",
            "[WARN root_task::drivers::cyw43] [cyw43] control-plane "
            "step=join action=fail err=cyw43 protocol error: ioctl-no-progress-after-frame",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "join-security-wpa-auth-initial-loop"
    assert gates.wifi_exact == "cyw43-join-security-wpa-auth-initial-loop"
    assert gates.wifi_phase == "join"
    assert gates.wifi_blocker_line > 0


def test_gate_summary_reports_wpa_auth_final_join_security_loop() -> None:
    events = normalizer.parse_events(
        [
            "[INFO root_task::drivers::cyw43] [cyw43] control-plane "
            "step=join action=begin",
            "[INFO root_task::drivers::cyw43] [cyw43] iovar set begin "
            "name=wpa_auth len=4",
            "[INFO root_task::drivers::cyw43] [cyw43] iovar set ready "
            "name=wpa_auth len=4",
            "[INFO root_task::drivers::cyw43] [cyw43] iovar set begin "
            "name=wpa_auth len=4",
            "[WARN root_task::drivers::cyw43] [cyw43] ioctl "
            "no-progress-after-frame cmd=0x00000107 id=24 no_progress_polls=128 "
            "nonmatching_frames=1 cached_exact_error= action=fail-fast",
            "[WARN root_task::drivers::cyw43] [cyw43] iovar set failed "
            "name=wpa_auth err=cyw43 protocol error: ioctl-no-progress-after-frame",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "join-security-wpa-auth-final-loop"
    assert gates.wifi_exact == "cyw43-join-security-wpa-auth-final-loop"
    assert gates.wifi_phase == "join"


def test_gate_summary_tracks_wifi_dhcp_failure_evidence() -> None:
    events = normalizer.parse_events(
        [
            JOIN_COMPLETE_OPEN,
            "[dhcp] failed reason=discover-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 8
    assert gates.wifi_blocker == "dhcp-failed"


def test_gate_summary_tracks_wifi_dhcp_transition_evidence() -> None:
    events = normalizer.parse_events(
        [
            JOIN_COMPLETE_OPEN,
            "[dhcp] tx queued kind=discover from=selecting to=selecting "
            "len=300 attempts=1 tx_packets=1",
            "[dhcp] rx transition from=selecting to=requesting "
            "action=send-queued len=300 attempts=0 rx_packets=1 invalid=0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 8
    assert gates.wifi_blocker == "dhcp-pending"


def test_gate_summary_tracks_wifi_dhcp_and_nettest_success() -> None:
    events = normalizer.parse_events(
        [
            "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
            "server=192.168.10.1 lease_s=3600",
            "[net-selftest] starting run (udp dst=192.168.10.1:31338 "
            "tcp dst=192.168.10.1:31339)",
            "[net-selftest] result tx_ok=true udp_echo_ok=true tcp_ok=true "
            "console_ok=true",
            "OK NETTEST detail=pass scope=serial-local",
            "netstats: rx_pkts=4 tx_pkts=9 rx_used=4 tx_used=9 polls=30",
            "netstats: mode=dhcp policy=wifi active=wifi standby=wired "
            "addr_src=dhcp-lease ip=192.168.10.50 gateway=192.168.10.1 dhcp=bound",
            "netstats: wifi_assoc=1 wifi_link=1 eapol_rx=2 eapol_start=1 eapol_secure=1",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 10
    assert gates.wifi_blocker == "none"


def test_gate_summary_requires_netstats_for_wifi_ready() -> None:
    events = normalizer.parse_events(
        [
            "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
            "server=192.168.10.1 lease_s=3600",
            "[net-selftest] result tx_ok=true udp_echo_ok=true tcp_ok=true "
            "console_ok=true",
            "OK NETTEST detail=pass scope=serial-local",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 9
    assert gates.wifi_blocker == "netstats-missing"


def test_gate_summary_treats_peer_assisted_nettest_as_ready_for_netstats() -> None:
    events = normalizer.parse_events(
        [
            "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
            "server=192.168.10.1 lease_s=3600",
            "[net-selftest] result tx_ok=true udp_echo_ok=false tcp_ok=false "
            "console_ok=true peer_assisted_ok=true",
            "OK NETTEST detail=started",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 9
    assert gates.wifi_blocker == "netstats-missing"


def test_gate_summary_clears_exact_after_peer_assisted_netstats_ready() -> None:
    events = normalizer.parse_events(
        [
            "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
            "server=192.168.10.1 lease_s=3600",
            "[net-selftest] result tx_ok=true udp_echo_ok=false tcp_ok=false "
            "console_ok=true peer_assisted_ok=true",
            "OK NETTEST detail=started",
            "netstats: rx_pkts=4 tx_pkts=9 rx_used=4 tx_used=9 polls=30",
            "netstats: mode=dhcp policy=wifi active=wifi standby=none "
            "addr_src=dhcp-lease ip=192.168.10.50 gateway=192.168.10.1 dhcp=bound",
            "netstats: wifi_assoc=1 wifi_link=1 eapol_rx=2 eapol_start=1 eapol_secure=1",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 10
    assert gates.wifi_blocker == "none"
    assert gates.wifi_exact == "none"


def test_gate_summary_does_not_downgrade_remote_cohsh_after_peer_echo_missing() -> None:
    events = normalizer.parse_events(
        [
            "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
            "server=192.168.10.1 lease_s=3600",
            "[cohsh-net][auth] auth OK, session established (conn_id=1)",
            "[net-selftest] result tx_ok=true udp_echo_ok=false tcp_ok=false "
            "console_ok=false",
            "OK NETTEST detail=started",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 9
    assert gates.wifi_blocker == "netstats-missing"


def test_gate_summary_accepts_host_eapol_secure_join_proof() -> None:
    events = normalizer.parse_events([JOIN_COMPLETE_HOST_EAPOL])

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 8
    assert gates.wifi_blocker == "none"


def test_gate_summary_tracks_wifi_nettest_readiness_blocker() -> None:
    events = normalizer.parse_events(
        [
            "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
            "server=192.168.10.1 lease_s=3600",
            "ERR NETTEST reason=policy detail=not-ready:ipc-buffer",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 9
    assert gates.wifi_blocker == "net-not-ready-ipc-buffer"


def test_gate_expectation_reports_mismatch(capsys) -> None:
    gates = normalizer.GateSummary(
        usb_gate=3,
        usb_blocker="cmd-poll-only-timeout",
        wifi_gate=4,
        wifi_blocker="ht-clock-timeout",
    )

    ok = normalizer.check_gate_expectations(
        gates, {"USB_GATE": "4"}, sys.stderr
    )

    captured = capsys.readouterr()
    assert not ok
    assert "USB_GATE expected 4 got 3" in captured.err


def test_gate_min_expectation_allows_advancement(capsys) -> None:
    gates = normalizer.GateSummary(
        usb_gate=4,
        usb_blocker="none",
        wifi_gate=4,
        wifi_blocker="devon-timeout",
    )

    ok = normalizer.check_gate_min_expectations(
        gates, {"USB_GATE": "3", "WIFI_GATE": "4"}, sys.stderr
    )

    captured = capsys.readouterr()
    assert ok
    assert captured.err == ""


def test_gate_min_expectation_reports_regression(capsys) -> None:
    gates = normalizer.GateSummary(
        usb_gate=2,
        usb_blocker="pcie-config-replay",
        wifi_gate=4,
        wifi_blocker="devon-timeout",
    )

    ok = normalizer.check_gate_min_expectations(
        gates, {"USB_GATE": "3"}, sys.stderr
    )

    captured = capsys.readouterr()
    assert not ok
    assert "USB_GATE min 3 got 2" in captured.err


def test_gate_not_expectation_rejects_stale_blocker(capsys) -> None:
    gates = normalizer.GateSummary(
        usb_gate=3,
        usb_blocker="cmd-poll-only-timeout",
        wifi_gate=4,
        wifi_blocker="devon-timeout",
    )

    ok = normalizer.check_gate_not_expectations(
        gates, {"USB_BLOCKER": "cmd-poll-only-timeout"}, sys.stderr
    )

    captured = capsys.readouterr()
    assert not ok
    assert "USB_BLOCKER rejected cmd-poll-only-timeout" in captured.err


def test_gate_not_expectation_preserves_duplicate_keys(capsys) -> None:
    gates = normalizer.GateSummary(
        usb_gate=3,
        usb_blocker="pcie-irq-quiesce-failed",
        wifi_gate=4,
        wifi_blocker="ht-recover-cmd5-timeout",
    )
    rejections = normalizer.parse_expectation_pairs(
        [
            "USB_BLOCKER=policy-skip-before-run",
            "USB_BLOCKER=pcie-irq-quiesce-failed",
            "WIFI_BLOCKER=ht-recover-cmd5-timeout",
        ]
    )

    ok = normalizer.check_gate_not_expectations(gates, rejections, sys.stderr)

    captured = capsys.readouterr()
    assert not ok
    assert "USB_BLOCKER rejected pcie-irq-quiesce-failed" in captured.err
    assert "WIFI_BLOCKER rejected ht-recover-cmd5-timeout" in captured.err


def test_gate_not_expectation_rejects_unknown_keys(capsys) -> None:
    gates = normalizer.GateSummary(
        usb_gate=3,
        usb_blocker="cmd-poll-only-timeout",
        wifi_gate=4,
        wifi_blocker="devon-timeout",
    )

    ok = normalizer.check_gate_not_expectations(
        gates, {"USB_BLOCKR": "cmd-poll-only-timeout"}, sys.stderr
    )

    captured = capsys.readouterr()
    assert not ok
    assert "unknown key USB_BLOCKR" in captured.err


def test_jsonl_output_is_stable() -> None:
    events = normalizer.parse_events(
        ["[cohesix:usb-trace] stage=handoff-final ready=1"]
    )
    record = events[0].to_record()
    encoded = json.dumps(record, sort_keys=True)

    assert '"domain": "usb"' in encoded
    assert '"stage": "handoff-final"' in encoded
