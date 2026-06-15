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
        "DRIVER_TASK_AFFINITY_MANIFEST_PROOF": "no",
        "DRIVER_TASK_AFFINITY_MANIFEST_MATCHES": 0,
        "DRIVER_TASK_AFFINITY_MANIFEST_MISSING": 7,
        "DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES": 0,
        "DRIVER_TASK_NOTIFICATION_BIND_DEFERRED": "no",
        "DRIVER_TASK_VSPACE_PROOF": "no",
        "DRIVER_TASK_POINTER_FREE_IPC_PROOF": "no",
        "DRIVER_TASK_OWNER_STATE_PROOF": "no",
        "DRIVER_TASK_ACTIVE_NET": "unknown",
        "DRIVER_TASK_BUDGET_OVERRUNS": 0,
        "DRIVER_TASK_LATENCY_PROOFS": 0,
        "DRIVER_TASK_RING_CALL_BEGIN": 0,
        "DRIVER_TASK_RING_CALL_RETURN": 0,
        "DRIVER_TASK_RING_CALL_OUTSTANDING": 0,
        "DRIVER_TASK_RING_CALL_TIMEOUT": 0,
        "DRIVER_TASK_RING_CALL_KEEP_ACTIVE": 0,
        "DRIVER_TASK_RING_CALL_ABORT": 0,
        "DRIVER_TASK_BOOTSTRAP_DEFERRED": 0,
        "DRIVER_TASK_RESOURCE_INIT": 0,
        "DRIVER_TASK_RESOURCE_BLOCKER": "none",
        "DRIVER_TASK_RESOURCE_CURRENT_BLOCKER": "none",
        "DRIVER_TASK_COUNTER_SNAPSHOTS": 0,
        "DRIVER_TASK_COUNTER_INVALID": 0,
        "DRIVER_TASK_COUNTER_BUSY": 0,
        "DRIVER_TASK_COUNTER_SAME_REQUEST": 0,
        "DRIVER_TASK_COUNTER_TIMEOUTS": 0,
        "DRIVER_TASK_COUNTER_KEEP_ACTIVE": 0,
        "DRIVER_TASK_COUNTER_ABORTS": 0,
        "DRIVER_TASK_COUNTER_STAGED_BYTES": 0,
        "DRIVER_TASK_COUNTER_CACHE_OPS": 0,
        "DRIVER_TASK_COUNTER_CACHE_BYTES": 0,
        "DRIVER_TASK_COUNTER_RX_FRAMES": 0,
        "DRIVER_TASK_COUNTER_TX_FRAMES": 0,
        "DRIVER_TASK_COUNTER_RX_BYTES": 0,
        "DRIVER_TASK_COUNTER_TX_BYTES": 0,
        "SERIAL_DRIVER_ACCEPTED": "no",
        "SERIAL_FALLBACK_ACTIVE": "no",
        "SERIAL_RUNTIME_FRONTIER": "none",
        "HDMI_DESCRIPTOR_READY": "no",
        "HDMI_ENGINE_READY": "no",
        "HDMI_OWNER_STATE_READY": "no",
        "HDMI_RUNTIME_FRONTIER": "none",
        "USB_DRIVER_TASK_FRONTIER": "none",
        "WIFI_REPLAY_FRONTIER": "none",
        "NET_DRIVER_TASK_REPLAY_EVENTS": 0,
        "NET_DRIVER_TASK_REPLAY_BLOCKER": "none",
        "SDIO_DRIVER_TASK_REPLAY_EVENTS": 0,
        "SDIO_DRIVER_TASK_REPLAY_BLOCKER": "none",
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
    assert record["DRIVER_TASK_AFFINITY_MANIFEST_PROOF"] == "no"
    assert record["DRIVER_TASK_AFFINITY_MANIFEST_MATCHES"] == 0
    assert record["DRIVER_TASK_AFFINITY_MANIFEST_MISSING"] == 7
    assert record["DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES"] == 0
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


def test_gate_summary_tracks_driver_task_counter_snapshots() -> None:
    """Counter snapshots are diagnostic evidence and must be activity-bearing."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_COUNTER contract=cyw43455 hot_path=cyw43-wifi "
            "source=root-ring sequence=12 submitted=3 completed=2 idle=1 "
            "fault=0 budget=0 frame=1 desc=1 staged_bytes=256 clean_ops=4 "
            "clean_bytes=512 inv_ops=3 inv_bytes=60 sends=8 yields=8 busy=1 "
            "same_request=2 timeouts=3 keep_active=2 aborts=1 rx_frames=5 "
            "rx_bytes=1500 tx_frames=4 tx_bytes=1200",
            "DRIVER_TASK_COUNTER contract=usb-local-seat hot_path=usb-keyboard "
            "source=root-ring sequence=0 submitted=0 completed=0 idle=0 "
            "fault=0 budget=0 frame=0 desc=0 staged_bytes=0 clean_ops=0 "
            "clean_bytes=0 inv_ops=0 inv_bytes=0 sends=0 yields=0 busy=0 "
            "same_request=0 timeouts=0 keep_active=0 aborts=0 rx_frames=0 "
            "rx_bytes=0 tx_frames=0 tx_bytes=0",
            "DRIVER_TASK_COUNTER contract=sdio-host hot_path=sdio-host "
            "source=root-ring sequence=7 submitted=1 completed=1 idle=0 "
            "fault=0 budget=0 frame=0 desc=0 staged_bytes=64 clean_ops=2",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_COUNTER_SNAPSHOTS"] == 3
    assert record["DRIVER_TASK_COUNTER_INVALID"] == 2
    assert record["DRIVER_TASK_COUNTER_BUSY"] == 1
    assert record["DRIVER_TASK_COUNTER_SAME_REQUEST"] == 2
    assert record["DRIVER_TASK_COUNTER_TIMEOUTS"] == 3
    assert record["DRIVER_TASK_COUNTER_KEEP_ACTIVE"] == 2
    assert record["DRIVER_TASK_COUNTER_ABORTS"] == 1
    assert record["DRIVER_TASK_COUNTER_STAGED_BYTES"] == 256
    assert record["DRIVER_TASK_COUNTER_CACHE_OPS"] == 7
    assert record["DRIVER_TASK_COUNTER_CACHE_BYTES"] == 572
    assert record["DRIVER_TASK_COUNTER_RX_FRAMES"] == 5
    assert record["DRIVER_TASK_COUNTER_TX_FRAMES"] == 4
    assert record["DRIVER_TASK_COUNTER_RX_BYTES"] == 1500
    assert record["DRIVER_TASK_COUNTER_TX_BYTES"] == 1200
    assert record["DRIVER_TASK_DEDICATED_READY"] == "no"


def test_gate_summary_treats_serial_input_trace_as_responsive_proof() -> None:
    events = normalizer.parse_events(
        [
            "SERIAL_INPUT_TRACE stage=route route=bcm2711-mini-uart "
            "driver_runtime_attached=1 client_active=0 rx_proven=0 "
            "root_context_service=skipped reason=driver-task-rx-proof-missing",
            "SERIAL_INPUT_TRACE stage=uart-rx route=bcm2711-mini-uart "
            "bytes=5 rx_depth=5 line_len=0 first=0x68 last=0x0a",
            "SERIAL_INPUT_TRACE stage=line-ready route=bcm2711-mini-uart "
            "line_len=4 rx_depth=0 partial_len=0",
            "SERIAL_INPUT_TRACE stage=consume-line route=bcm2711-mini-uart "
            "line_len=4 rx_depth=0 partial_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["SERIAL_RESPONSIVE_PROOF"] == "yes"


def test_gate_summary_tracks_driver_task_substrate_proof_fields() -> None:
    """Dedicated closure must prove substrate, capset, fault, revoke, and scheduling."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 mcs=0 "
            "task_count=9 failed_count=0 live_tcb_count=9 "
            "root_authority=admission-descriptor-diagnostics-only hardware_owner=linked-runtime fault_endpoint_ready=yes revoke_ready=yes "
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
            "DRIVER_TASK role=net contract=driver-genet isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated "
            "capset=network-frame-transport unexpected_caps=0 fault_probe=pass revoke_ready=yes "
            "priority=160 observed_service_us=69",
            "DRIVER_TASK role=sdio contract=sdio-host isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated "
            "capset=device-only unexpected_caps=0 fault_probe=pass revoke_ready=yes "
            "priority=180 observed_service_us=47",
            "DRIVER_TASK role=pcie contract=pcie-root isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated "
            "capset=device-only unexpected_caps=0 fault_probe=pass revoke_ready=yes "
            "priority=170 observed_service_us=51",
            "DRIVER_TASK_ACCEPTANCE dedicated_ready=yes substrate=active capset=pass "
            "fault=pass revoke=pass sched=pass affinity=pass active_net=cyw43 required=7 "
            "dedicated=7 compatibility=0 vspace=isolated ipc_abi=shared-ring-command "
            "pointer_free_ipc=yes owner_state=driver-owned",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_CONTRACTS"] == 7
    assert record["DRIVER_TASK_DEDICATED"] == 7
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


def test_gate_summary_tracks_driver_task_manifest_affinity_from_boot_lines() -> None:
    """Per-driver boot breadcrumbs must prove the generated Pi 4 core map."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=serial tcb=0x064e runtime_descriptor=yes",
            "DRIVER_TASK_BOOT contract=serial role=serial started=yes affinity_core=1",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=usb-local-seat tcb=0x08f2 runtime_descriptor=yes",
            "DRIVER_TASK_BOOT contract=usb-local-seat role=usb started=yes affinity_core=1",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=hdmi-text tcb=0x0a7e runtime_descriptor=yes",
            "DRIVER_TASK_BOOT contract=hdmi-text role=display started=yes affinity_core=2",
            "DRIVER_TASK_BOOT contract=pcie-root role=pcie started=yes affinity_core=2",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=sdio-host tcb=0x0713 runtime_descriptor=yes",
            "DRIVER_TASK_BOOT contract=sdio-host role=sdio started=yes affinity_core=3",
            "DRIVER_TASK_BOOT contract=bcmgenet-v5 role=net started=yes affinity_core=3",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=cyw43455 tcb=0x1915 runtime_descriptor=yes",
            "DRIVER_TASK_BOOT contract=cyw43455 role=net started=yes affinity_core=3",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_AFFINITY_MANIFEST_PROOF"] == "yes"
    assert record["DRIVER_TASK_AFFINITY_MANIFEST_MATCHES"] == 7
    assert record["DRIVER_TASK_AFFINITY_MANIFEST_MISSING"] == 0
    assert record["DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES"] == 0

    mismatched_events = normalizer.parse_events(
        [
            "DRIVER_TASK_BOOT contract=serial role=serial started=yes affinity_core=1",
            "DRIVER_TASK_BOOT contract=usb-local-seat role=usb started=yes affinity_core=1",
            "DRIVER_TASK_BOOT contract=hdmi-text role=display started=yes affinity_core=2",
            "DRIVER_TASK_BOOT contract=pcie-root role=pcie started=yes affinity_core=2",
            "DRIVER_TASK_BOOT contract=sdio-host role=sdio started=yes affinity_core=3",
            "DRIVER_TASK_BOOT contract=bcmgenet-v5 role=net started=yes affinity_core=0",
            "DRIVER_TASK_BOOT contract=cyw43455 role=net started=yes affinity_core=3",
        ]
    )

    mismatch_record = normalizer.summarize_gates(mismatched_events).to_record()
    assert mismatch_record["DRIVER_TASK_AFFINITY_MANIFEST_PROOF"] == "no"
    assert mismatch_record["DRIVER_TASK_AFFINITY_MANIFEST_MATCHES"] == 6
    assert mismatch_record["DRIVER_TASK_AFFINITY_MANIFEST_MISSING"] == 1
    assert mismatch_record["DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES"] == 1


def test_gate_summary_affinity_follows_selected_pi4_network_profile() -> None:
    """Selected-only Pi 4 boots must prove only the active network driver path."""

    wifi_events = normalizer.parse_events(
        [
            "DRIVER_TASK_SELECTED profile=pi4-hardware selection=wifi required_roles=0x3f",
            "DRIVER_TASK_BOOT contract=serial role=serial started=yes affinity_core=1",
            "DRIVER_TASK_BOOT contract=usb-local-seat role=usb started=yes affinity_core=1",
            "DRIVER_TASK_BOOT contract=hdmi-text role=display started=yes affinity_core=2",
            "DRIVER_TASK_BOOT contract=pcie-root role=pcie started=yes affinity_core=2",
            "DRIVER_TASK_BOOT contract=sdio-host role=sdio started=yes affinity_core=3",
            "DRIVER_TASK_BOOT contract=cyw43455 role=net started=yes affinity_core=3",
            "[smp] activity selected profile=pi4-hardware net=wifi active_contracts=selected-only",
        ]
    )

    wifi_record = normalizer.summarize_gates(wifi_events).to_record()
    assert wifi_record["DRIVER_TASK_AFFINITY_MANIFEST_PROOF"] == "yes"
    assert wifi_record["DRIVER_TASK_AFFINITY_MANIFEST_MATCHES"] == 6
    assert wifi_record["DRIVER_TASK_AFFINITY_MANIFEST_MISSING"] == 0
    assert wifi_record["DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES"] == 0

    wired_events = normalizer.parse_events(
        [
            "DRIVER_TASK_SELECTED profile=pi4-hardware selection=wired required_roles=0x2f",
            "DRIVER_TASK_BOOT contract=serial role=serial started=yes affinity_core=1",
            "DRIVER_TASK_BOOT contract=usb-local-seat role=usb started=yes affinity_core=1",
            "DRIVER_TASK_BOOT contract=hdmi-text role=display started=yes affinity_core=2",
            "DRIVER_TASK_BOOT contract=pcie-root role=pcie started=yes affinity_core=2",
            "DRIVER_TASK_BOOT contract=bcmgenet-v5 role=net started=yes affinity_core=3",
            "[smp] activity selected profile=pi4-hardware net=wired active_contracts=selected-only",
        ]
    )

    wired_record = normalizer.summarize_gates(wired_events).to_record()
    assert wired_record["DRIVER_TASK_AFFINITY_MANIFEST_PROOF"] == "yes"
    assert wired_record["DRIVER_TASK_AFFINITY_MANIFEST_MATCHES"] == 5
    assert wired_record["DRIVER_TASK_AFFINITY_MANIFEST_MISSING"] == 0
    assert wired_record["DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES"] == 0


def test_gate_summary_explicit_pointer_free_ipc_no_overrides_abi_label() -> None:
    """A contradictory proof line must fail closed on the explicit proof field."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_SUBSTRATE active=yes task_count=9 failed_count=0 live_tcb_count=9 "
            "root_authority=admission-descriptor-diagnostics-only hardware_owner=linked-runtime fault_endpoint_ready=yes revoke_ready=yes "
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
            "root_authority=admission-descriptor-diagnostics-only hardware_owner=linked-runtime fault_endpoint_ready=yes revoke_ready=yes "
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


def test_gate_summary_rejects_owner_state_without_explicit_hot_path() -> None:
    """Contract or role labels must not stand in for hot-path owner proof."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_OWNER_STATE contract=serial "
            "owner_state=driver-owned descriptor=present root_pointer=no",
            "DRIVER_TASK_OWNER_STATE role=usb "
            "owner_state=driver-owned descriptor=present root_pointer=no",
            "DRIVER_TASK_OWNER_STATE driver=hdmi "
            "owner_state=driver-owned descriptor=present root_pointer=no",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_OWNER_STATE_PROOF"] == "no"


def test_gate_summary_rejects_owner_state_truthy_aliases() -> None:
    """Owner-state proof requires the exact driver-owned state label."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_OWNER_STATE contract=serial hot_path=serial-console "
            "owner_state=yes descriptor=present root_pointer=no",
            "DRIVER_TASK_OWNER_STATE contract=usb-local-seat hot_path=usb-keyboard "
            "owner_state=pass descriptor=present root_pointer=no",
            "DRIVER_TASK_OWNER_STATE contract=hdmi-text hot_path=hdmi-text "
            "owner_state=true descriptor=present root_pointer=no",
            "DRIVER_TASK_OWNER_STATE contract=bcmgenet-v5 hot_path=genet-nic "
            "owner_state=owned descriptor=present root_pointer=no",
            "DRIVER_TASK_OWNER_STATE contract=cyw43455 hot_path=cyw43-wifi "
            "owner_state=driver descriptor=present root_pointer=no",
            "DRIVER_TASK_OWNER_STATE contract=sdio-host hot_path=sdio-host "
            "owner_state=1 descriptor=present root_pointer=no",
            "DRIVER_TASK_OWNER_STATE contract=pcie-root hot_path=pcie-root "
            "owner_state=driver-owned descriptor=yes root_pointer=no",
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


def test_gate_summary_tracks_driver_task_notification_bind_deferral() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_NOTIFICATION_BIND_DEFERRED contract=serial tcb=0x05ad "
            "notification=0x05a8 reason=pi4-early-tcb-notification-bind-boot-stall-guard",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_NOTIFICATION_BIND_DEFERRED"] == "yes"
    assert record["DRIVER_TASK_DEDICATED_READY"] == "no"


def test_gate_summary_tracks_driver_task_ring_call_no_reply_frontier() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RING_CALL_BEGIN contract=serial endpoint=0x05ae "
            "request=1 opcode=1 flags=0x4000 arg0=1 arg1=1",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_RING_CALL_BEGIN"] == 1
    assert record["DRIVER_TASK_RING_CALL_RETURN"] == 0
    assert record["DRIVER_TASK_RING_CALL_OUTSTANDING"] == 1


def test_gate_summary_tracks_driver_task_ring_call_return() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RING_CALL_BEGIN contract=serial endpoint=0x05ae "
            "request=1 opcode=1 flags=0x4000 arg0=1 arg1=1",
            "DRIVER_TASK_RING_CALL_RETURN contract=serial endpoint=0x05ae "
            "request=1 sequence=1 code=1 detail=0 result=1",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_RING_CALL_BEGIN"] == 1
    assert record["DRIVER_TASK_RING_CALL_RETURN"] == 1
    assert record["DRIVER_TASK_RING_CALL_OUTSTANDING"] == 0


def test_gate_summary_does_not_double_count_prompt_line_driver_task_return() -> None:
    events = normalizer.parse_events(
        [
            "cohesix> DRIVER_TASK_RING_CALL_RETURN contract=serial endpoint=0x05ae "
            "request=1 sequence=1 code=1 detail=0 result=1",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["ROOT_PROMPT_SEEN"] == "yes"
    assert record["DRIVER_TASK_RING_CALL_BEGIN"] == 0
    assert record["DRIVER_TASK_RING_CALL_RETURN"] == 1
    assert record["DRIVER_TASK_RING_CALL_OUTSTANDING"] == 0


def test_gate_summary_tracks_driver_task_ring_call_timeout() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RING_CALL_BEGIN contract=serial endpoint=0x05ae "
            "request=1 opcode=1 flags=0x4000 arg0=1 arg1=1",
            "DRIVER_TASK_RING_CALL_TIMEOUT contract=serial endpoint=0x05ae "
            "request=1 mode=nonblocking attempts=4096 opcode=1 arg0=1 "
            "aux0=0x00000000 frame_len=0 owner=linked-runtime "
            "marker_valid=no marker_sequence=0 marker_phase=0 "
            "marker_phase_name=none marker_aux0=0x00000000 "
            "blocker=runtime-progress-missing next_action=check-keep-active",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_RING_CALL_BEGIN"] == 1
    assert record["DRIVER_TASK_RING_CALL_RETURN"] == 0
    assert record["DRIVER_TASK_RING_CALL_OUTSTANDING"] == 1
    assert record["DRIVER_TASK_RING_CALL_TIMEOUT"] == 1


def test_gate_summary_tracks_driver_task_ring_call_keep_active() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RING_CALL_BEGIN contract=usb-local-seat endpoint=0x07a4 "
            "request=8 opcode=1 flags=0x2000 arg0=2 arg1=2 aux0=0x55534245",
            "DRIVER_TASK_RING_CALL_TIMEOUT contract=usb-local-seat endpoint=0x07a4 "
            "request=8 mode=prompt-slice attempts=512 opcode=1 arg0=2 "
            "aux0=0x55534245 frame_len=0 owner=linked-runtime "
            "marker_valid=yes marker_sequence=8 marker_phase=318 "
            "marker_phase_name=usb-hub-descriptor-wait-begin marker_aux0=0x55534245 "
            "blocker=usb-hub-descriptor-wait-begin next_action=check-keep-active",
            "DRIVER_TASK_RING_CALL_KEEP_ACTIVE contract=usb-local-seat endpoint=0x07a4 "
            "request=8 mode=prompt-slice timeout_count=2 keep_limit=8 "
            "progress_advanced=no opcode=1 arg0=2 aux0=0x55534245 frame_len=0 "
            "owner=linked-runtime marker_valid=yes marker_sequence=8 marker_phase=318 "
            "marker_phase_name=usb-hub-descriptor-wait-begin marker_aux0=0x55534245 "
            "blocker=usb-hub-descriptor-wait-begin next_action=poll-same-request",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_RING_CALL_BEGIN"] == 1
    assert record["DRIVER_TASK_RING_CALL_RETURN"] == 0
    assert record["DRIVER_TASK_RING_CALL_TIMEOUT"] == 1
    assert record["DRIVER_TASK_RING_CALL_KEEP_ACTIVE"] == 1
    assert record["DRIVER_TASK_RING_CALL_ABORT"] == 0
    assert record["DRIVER_TASK_RING_CALL_OUTSTANDING"] == 1


def test_gate_summary_tracks_driver_task_ring_call_abort() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RING_CALL_BEGIN contract=usb-local-seat endpoint=0x07a4 "
            "request=5 opcode=1 flags=0x2000 arg0=2 arg1=2 aux0=0x55534245",
            "DRIVER_TASK_RING_CALL_TIMEOUT contract=usb-local-seat endpoint=0x07a4 "
            "request=5 mode=prompt-slice attempts=512 opcode=1 arg0=2 "
            "aux0=0x55534245 frame_len=0 owner=linked-runtime "
            "marker_valid=yes marker_sequence=5 marker_phase=407 "
            "marker_phase_name=usb-hub-set-configuration-status-event-ignored "
            "marker_aux0=0x55534245 "
            "blocker=usb-hub-set-configuration-status-event-ignored "
            "next_action=check-keep-active",
            "DRIVER_TASK_RING_CALL_ABORT contract=usb-local-seat endpoint=0x07a4 "
            "request=5 mode=prompt-slice reason=timeout-resume-limit "
            "timeout_count=3 opcode=1 arg0=2 aux0=0x55534245 frame_len=0 "
            "owner=linked-runtime marker_valid=yes marker_sequence=5 marker_phase=407 "
            "marker_phase_name=usb-hub-set-configuration-status-event-ignored "
            "marker_aux0=0x55534245 "
            "blocker=usb-hub-set-configuration-status-event-ignored "
            "next_action=retry-fresh-request-after-blocker-fix",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_RING_CALL_BEGIN"] == 1
    assert record["DRIVER_TASK_RING_CALL_RETURN"] == 0
    assert record["DRIVER_TASK_RING_CALL_TIMEOUT"] == 1
    assert record["DRIVER_TASK_RING_CALL_ABORT"] == 1
    assert record["DRIVER_TASK_RING_CALL_OUTSTANDING"] == 0


def test_gate_summary_preserves_pcie_vl805_usb_blocker_over_keyboard_ready() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_blocker current=pcie-vl805-config-contract-missing "
            "expected=vl805-config-window+command+bar0+mailbox "
            "observed=missing-or-disabled blocker=pcie-vl805-config-contract-missing",
            "usb: runtime_gate keyboard=no first_report=no first_byte=no "
            "proof_gate=0 target_gate=10 next=keyboard-ready blocker=keyboard-not-ready",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_blocker == "pcie-vl805-config-contract-missing"


def test_gate_summary_labels_driver_task_wifi_runtime_unproved() -> None:
    events = normalizer.parse_events(
        [
            "[net-console] deferred failed detail=cyw43-wifi driver-task runtime "
            "is pending hardware service",
            "wifi: debug subcommand=diag result=error "
            "error=unsupported operation: pi4-wifi-driver-task-runtime-required",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43-wifi driver-task runtime is pending hardware service",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_blocker == "wifi-driver-task-runtime-unproved"
    assert gates.wifi_exact == "wifi-driver-task-runtime-unproved"


def test_gate_summary_tracks_wifi_startup_blackbox_gates() -> None:
    events = normalizer.parse_events(
        [
            "wifi: gate 1 name=hal-power-reset status=pass evidence=power=on next=sdio-card-select",
            "wifi: gate 2 name=sdio-card-select status=pass evidence=card=yes next=cccr-fbr-ready",
            "wifi: gate 3 name=cccr-fbr-ready status=pass evidence=ioex=0x02 next=ht-clock",
            "wifi: gate 4 name=ht-clock status=pass evidence=chipclk=0xd0 next=backplane-window",
            "wifi: gate 5 name=backplane-window status=pass evidence=programmed=0x00198000 next=firmware-upload",
            "wifi: gate 6 name=firmware-upload status=fail evidence=uploaded=no fault_detail=0x5103 next=function2-ready",
            "wifi: evidence cyw43 stage=firmware-upload op=1 target=0x00198400 payload_len=8192 total_len=8192 detail=0x5103 reason=sdio-descriptor-transfer-failed result=0x00000000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 5
    assert gates.wifi_blocker == "cyw43-sdio-descriptor-transfer-failed"


def test_gate_summary_keeps_early_wifi_startup_failure_over_later_no_reply() -> None:
    events = normalizer.parse_events(
        [
            "wifi: cyw43 linked_runtime_progress marker_valid=yes sequence=1 "
            "phase=142 phase_name=cyw43-sdio-owner-wait-begin "
            "aux0=0x43595734 gate=2 "
            "blocker=cyw43-sdio-owner-completion-pending "
            "next_action=inspect-linked-sdio-owner-command-service",
            "wifi: gate 1 name=hal-power-reset status=pass evidence=power=on "
            "reset=deasserted source=hal-runtime-required next=sdio-card-select",
            "wifi: gate 2 name=sdio-card-select status=inferred "
            "evidence=stage=cyw43-transport-init detail=0x0000 "
            "result=0x00000000 next=cccr-fbr-ready",
            "wifi: gate 3 name=cccr-fbr-ready status=fail "
            "evidence=ioex=n/a iordy=n/a fbr1_blk=n/a fbr2_blk=n/a "
            "next=ht-clock",
            "wifi: gate 4 name=ht-clock status=blocked "
            "evidence=chipclk=n/a clock=0Hz width=unknown next=backplane-window",
            "wifi: evidence cyw43 stage=cyw43-transport-init op=1 flags=0x0000 "
            "target=0x00000000 payload_off=0 payload_len=0 total_len=0 "
            "detail=0x0000 reason=cyw43-runtime-command-no-reply "
            "result=0x00000000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 2
    assert gates.wifi_blocker == "cccr-fbr-ready"


def test_gate_summary_names_cyw43_firmware_retry_exhaustion() -> None:
    events = normalizer.parse_events(
        [
            "wifi: gate 5 name=backplane-window status=pass "
            "evidence=programmed=0x00198000 next=firmware-upload",
            "wifi: gate 6 name=firmware-upload status=fail "
            "evidence=uploaded=no fault_detail=0x5329 next=function2-ready",
            "wifi: evidence cyw43 stage=cyw43-firmware-chunk op=2 flags=0x0001 "
            "target=0x00198000 payload_len=512 total_len=609309 detail=0x5329 "
            "reason=cyw43-firmware-retry-exhausted result=0x04208040",
            "wifi: evidence sdio_cmd53 func=1 addr=0x00198000 len=512 "
            "increment=yes block_mode=byte-retry op=2 "
            "source=owner-terminal",
            "wifi: evidence sdio_status "
            "descriptor_status=cyw43-firmware-retry-exhausted "
            "transfer_stage=data-end transfer_status=0x208040 "
            "transfer_reason=sdhci-transfer-finish-data-crc r5=0x0000 "
            "retry=byte512-fallback-exhausted host=0x06 clock=0x5007",
            "wifi: evidence sdio_payload first=0x11 last=0x22 xor=0x33 "
            "sum=0x00004444 owner_window=sdio-shared-8192",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 5
    assert gates.wifi_blocker == "cyw43-firmware-retry-exhausted"
    assert gates.wifi_exact == "cyw43-firmware-retry-exhausted"
    assert gates.wifi_phase == "cyw43-firmware-chunk"


def test_gate_summary_names_june7_cyw43_firmware_owner_fault_shape() -> None:
    events = normalizer.parse_events(
        [
            "wifi: gate 5 name=backplane-window status=pass "
            "evidence=programmed=0x00198000 next=firmware-upload",
            "wifi: gate 6 name=firmware-upload status=fail "
            "evidence=uploaded=no fault_detail=0x5329 next=function2-ready",
            "wifi: evidence cyw43 stage=cyw43-firmware-chunk op=2 flags=0x0000 "
            "target=0x0021a000 payload_len=8192 total_len=609309 detail=0x5329 "
            "reason=cyw43-firmware-retry-exhausted result=0x05000800",
            "wifi: evidence sdio_cmd53 func=1 addr=0x0021b800 len=64 "
            "increment=yes block_mode=no op=2 source=owner-terminal",
            "wifi: evidence sdio_status "
            "descriptor_status=cyw43-firmware-retry-exhausted "
            "transfer_stage=response transfer_status=0x000800 "
            "transfer_reason=sdio-r5-response r5=0x0800 "
            "retry=byte-narrow-conservative-exhausted host=0x06 clock=0x5007",
            "wifi: evidence sdio_payload first=0x11 last=0x22 xor=0x33 "
            "sum=0x00004444 owner_window=sdio-shared-8192",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 5
    assert gates.wifi_blocker == "cyw43-firmware-retry-exhausted"
    assert gates.wifi_exact == "cyw43-firmware-retry-exhausted"
    assert gates.wifi_phase == "cyw43-firmware-chunk"


def test_gate_summary_preserves_june12_cyw43_firmware_recovery_frontier() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_STREAM_PROGRESS contract=cyw43455 "
            "stage=cyw43-firmware-chunk uploaded=557056 total_len=609309 "
            "target=0x0021e000 chunk_len=8192",
            "CYW43_DRIVER_TASK_FIRMWARE_RECOVERY contract=cyw43455 "
            "attempt=158 resume_offset=565248 force_byte=false "
            "same_offset_attempts=4",
            "DRIVER_TASK_RING_CALL_RETURN contract=cyw43455 endpoint=0x1975 "
            "request=243 sequence=243 code=5 detail=21289 result=83888128",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-chunk op=2 flags=0x0000 "
            "target=0x00222000 payload_off=4096 payload_len=8192 "
            "total_len=609309 detail=21289 "
            "reason=cyw43-firmware-retry-exhausted result=83888128",
            "CYW43_SDIO_OWNER_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-chunk op=2 cmd=53 arg=0x95540040 "
            "fn=1 win=0x0aa00 target=0x00222000 effective=0x00222a00 "
            "chunk_off=2560 payload_off=6656 inc=yes write=yes mode=byte "
            "len=64 blksz=64 blkcnt=1 tm=0x0002 host=0x00 power=0x0f "
            "clock=0x5007 present=0x01ff0506 int=0x00000010 "
            "resp0=0x00001800 blkreg=0x00010040 detail=0x5329 "
            "reason=cyw43-firmware-retry-exhausted xfer_stage=response "
            "xfer_status=0x000800 xfer_reason=sdio-r5-response r5=0x0800 "
            "owner_window=sdio-shared-8192 "
            "retry=byte-narrow-conservative-exhausted",
            "CYW43_SDIO_PAYLOAD_CMP contract=cyw43455 "
            "stage=cyw43-firmware-chunk op=2 target=0x00222000 "
            "off=2560 len=64 status=match pf=0x58 pl=0x13 px=0xb0 "
            "ps=0x00001038 of=0x58 ol=0x13 ox=0xb0 os=0x00001038",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-firmware-chunk status=fault "
            "acceptance=no code=5 detail=21289 result=83888128 frame_len=56",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-firmware-chunk "
            "status=stream-fault-owner-recovery-required acceptance=no "
            "code=5 detail=21289 result=83888128 frame_len=56",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 5
    assert gates.wifi_blocker == "cyw43-firmware-retry-exhausted"
    assert gates.wifi_exact == "cyw43-firmware-retry-exhausted"
    assert gates.wifi_phase == "cyw43-firmware-chunk"
    assert gates.wifi_blocker != "cyw43-transport-init"


def test_gate_summary_refines_cyw43_release_no_reply_with_last_release_marker() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-firmware blocker=failed",
            "DRIVER_TASK_RING_PROGRESS contract=cyw43455 request=99 "
            "expected_aux0=0x43595734 marker_valid=yes marker_sequence=99 "
            "marker_phase=207 marker_phase_name=cyw43-release-reset-vector-begin "
            "marker_aux0=0x43595734",
            "DRIVER_TASK_RING_PROGRESS contract=cyw43455 request=99 "
            "expected_aux0=0x43595734 marker_valid=yes marker_sequence=99 "
            "marker_phase=142 marker_phase_name=cyw43-sdio-owner-wait-begin "
            "marker_aux0=0x43595734",
            "CYW43_DRIVER_TASK_COMMAND_NO_REPLY contract=cyw43455 "
            "stage=cyw43-firmware-release op=5 flags=0x0000 "
            "target=0x00000000 payload_off=0 payload_len=0 total_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-firmware-release status=no-reply acceptance=no "
            "code=none detail=none result=none frame_len=0",
            "wifi: driver-task replay failure detail=net-disabled "
            "cause=cyw43-command-completion driver-task runtime init failed",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_blocker == "cyw43-firmware-runtime-replay"
    assert gates.wifi_exact == "cyw43-release-reset-vector-no-reply"
    assert gates.wifi_phase == "cyw43-release-reset-vector-no-reply"


def test_gate_summary_tracks_cyw43_post_release_mailbox_ready_fault() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-firmware blocker=failed",
            "DRIVER_TASK_RING_PROGRESS contract=cyw43455 request=99 "
            "expected_aux0=0x43595734 marker_valid=yes marker_sequence=99 "
            "marker_phase=216 marker_phase_name=cyw43-release-firmware-ready-begin "
            "marker_aux0=0x43595734",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-release op=5 flags=0x0000 "
            "target=0x00000000 payload_off=0 payload_len=0 total_len=0 "
            "detail=21293 reason=cyw43-post-release-mailbox-ready result=0x00000000",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-firmware-release status=fault acceptance=no "
            "code=5 detail=21293 result=0x00000000 frame_len=0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-post-release-mailbox-ready"
    assert gates.wifi_exact == "cyw43-post-release-mailbox-ready"
    assert gates.wifi_phase == "cyw43-firmware-release"


def test_gate_summary_keeps_wifi_blackbox_fault_over_later_prompt_replay() -> None:
    events = normalizer.parse_events(
        [
            "wifi: gate 5 name=backplane-window status=pass evidence=programmed=0x00198000 next=firmware-upload",
            "wifi: gate 6 name=firmware-upload status=fail evidence=uploaded=no fault_detail=0x5103 next=function2-ready",
            "wifi: evidence cyw43 stage=cyw43-firmware-chunk op=2 flags=0x0001 target=0x00199c00 payload_len=1024 total_len=609309 detail=0x5103 reason=sdio-descriptor-transfer-failed result=0x05000100",
            "wifi: evidence sdio_cmd53 func=1 addr=0x00199c00 len=1024 increment=yes block_mode=byte-retry op=2 source=owner-terminal",
            "wifi: evidence sdio_status descriptor_status=descriptor-transfer-failed transfer_stage=response transfer_status=0x000100 transfer_reason=sdio-r5-response r5=0x0100 retry=byte512-fallback host=0x06 clock=0x5007",
            "wifi: evidence sdio_payload first=0x44 last=0x55 xor=0x11 sum=0x00006666 owner_window=sdio-shared-8192",
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host selected=wifi-owner-link attempted=yes stage=load-fw blocker=unsupported",
            "wifi: debug subcommand=load-fw result=error error=unsupported",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 5
    assert gates.wifi_blocker == "cyw43-sdio-descriptor-transfer-failed"
    assert gates.wifi_exact == "cyw43-sdio-descriptor-transfer-failed"
    assert gates.wifi_phase == "cyw43-firmware-chunk"


def test_gate_summary_fails_closed_on_cyw43_firmware_upload_pass_with_fault() -> None:
    events = normalizer.parse_events(
        [
            "wifi: gate 5 name=backplane-window status=pass evidence=programmed=0x00198000 next=firmware-upload",
            "wifi: gate 6 name=firmware-upload status=pass evidence=uploaded=no verified=no fault_detail=0x0001 next=function2-ready",
            "wifi: gate 7 name=function2-ready status=fail evidence=f2_enabled=no f2_ready=no next=firmware-channel",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 stage=cyw43-firmware-chunk op=2 flags=0x0000 target=0x00198800 payload_len=1024 total_len=609309 detail=1 reason=unknown result=0",
            "wifi: evidence cyw43 stage=cyw43-firmware-chunk op=2 flags=0x0000 target=0x00198800 payload_len=1024 total_len=609309 detail=0x0001 reason=unknown result=0x00000000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 5
    assert gates.wifi_blocker == "cyw43-runtime-command-rejected"
    assert gates.wifi_exact == "cyw43-runtime-command-rejected"
    assert gates.wifi_phase == "cyw43-firmware-chunk"


def test_gate_summary_prefers_cyw43_transport_admission_reason() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-firmware blocker=failed",
            "wifi: gate 5 name=backplane-window status=pass "
            "evidence=programmed=0x00198000 next=firmware-upload",
            "wifi: gate 6 name=firmware-upload status=fail "
            "evidence=uploaded=no fault_detail=0x0001 next=function2-ready",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-transport-init op=1 flags=0x0000 "
            "target=0x00000000 payload_off=0 payload_len=0 total_len=0 "
            "detail=1 reason=cyw43-transport-command-admission result=0",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-transport-init status=fault acceptance=no "
            "code=5 detail=1 result=0 frame_len=0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_blocker == "cyw43-runtime-command-rejected"
    assert gates.wifi_exact == "cyw43-transport-command-admission"
    assert gates.wifi_phase == "cyw43-transport-init"


def test_gate_summary_tracks_cyw43_descriptor_invalid_fault() -> None:
    events = normalizer.parse_events(
        [
            "wifi: gate 5 name=backplane-window status=pass evidence=programmed=0x00198000 next=firmware-upload",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-chunk op=2 flags=0x0000 target=0x00200000 "
            "payload_off=4096 payload_len=8192 total_len=609309 detail=21258 "
            "reason=cyw43-descriptor-invalid result=0x00000004",
            "wifi: evidence cyw43 stage=cyw43-firmware-chunk op=2 flags=0x0000 "
            "target=0x00200000 payload_off=4096 payload_len=8192 "
            "total_len=609309 detail=0x530a reason=cyw43-descriptor-invalid "
            "result=0x00000004",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 5
    assert gates.wifi_blocker == "cyw43-descriptor-invalid"
    assert gates.wifi_exact == "cyw43-descriptor-invalid"
    assert gates.wifi_phase == "cyw43-firmware-chunk"


def test_gate_summary_labels_pre_prompt_wifi_sdio_driver_task_deferral() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=sdio-host hot_path=sdio-host "
            "stage=runtime-descriptor-record status=deferred acceptance=no "
            "code=none detail=none result=none frame_len=0",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=sdio-host tcb=0x064c "
            "runtime_descriptor=yes reason=root-shell-before-first-service-proof",
            "DRIVER_TASK_BUS_LINK contract=cyw43455 owner=sdio-host "
            "channel=cyw43-sdio endpoint_slot=0x0008 ring_vaddr=0x70e00000",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=runtime-descriptor-record status=deferred acceptance=no "
            "code=none detail=none result=none frame_len=0",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=cyw43455 tcb=0x101f "
            "runtime_descriptor=yes reason=root-shell-before-first-service-proof",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_GATE"] == 1
    assert record["WIFI_BLOCKER"] == "wifi-driver-task-runtime-unproved"


def test_gate_summary_tracks_driver_task_bootstrap_deferred() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=serial tcb=0x05b1 "
            "runtime_descriptor=yes reason=root-shell-before-first-service-proof",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_BOOTSTRAP_DEFERRED"] == 1


def test_gate_summary_tracks_driver_task_resource_init_blocker() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=hdmi-text hot_path=hdmi-text "
            "stage=hdmi-engine-init status=ready acceptance=no code=1 "
            "detail=0 result=1 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-xhci-init status=no-reply "
            "acceptance=no code=none detail=none result=none frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=runtime-ring-submit status=busy "
            "acceptance=no code=none detail=none result=none frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_RESOURCE_INIT"] == 3
    assert (
        record["DRIVER_TASK_RESOURCE_BLOCKER"]
        == "usb-keyboard:usb-xhci-init:no-reply"
    )
    assert (
        record["DRIVER_TASK_RESOURCE_CURRENT_BLOCKER"]
        == "cyw43-wifi:runtime-ring-submit:busy"
    )


def test_driver_task_resource_init_preserves_request_context_fields() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=runtime-ring-submit status=busy "
            "acceptance=no code=none detail=none result=none frame_len=0 "
            "owner=linked-runtime root_action=submit-turn "
            "blocker=runtime-ring-submit-busy next_action=poll-active-request "
            "active_request_valid=yes active_request=42 "
            "expected_request_valid=yes expected_request=42 "
            "expected_aux0_valid=yes expected_aux0=0x55534245 "
            "same_request_resume=yes "
            "progress_marker_valid=yes progress_sequence=42 progress_phase=407 "
            "progress_phase_name=usb-hub-set-configuration-status-event-ignored "
            "progress_aux0=0x55534245 progress_request_match=yes",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert events[0].fields["owner"] == "linked-runtime"
    assert events[0].fields["expected_request_valid"] == "yes"
    assert events[0].fields["expected_aux0_valid"] == "yes"
    assert events[0].fields["expected_aux0"] == "0x55534245"
    assert events[0].fields["same_request_resume"] == "yes"
    assert (
        events[0].fields["progress_phase_name"]
        == "usb-hub-set-configuration-status-event-ignored"
    )
    assert (
        record["DRIVER_TASK_RESOURCE_BLOCKER"]
        == "usb-keyboard:runtime-ring-submit:busy"
    )


def test_gate_summary_ignores_expected_deferred_resource_init() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=sdio-host "
            "hot_path=sdio-host stage=runtime-descriptor-record "
            "status=deferred acceptance=no code=none detail=none result=none "
            "frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=serial "
            "hot_path=serial-console stage=serial-runtime-init "
            "status=no-reply acceptance=no code=none detail=none result=none "
            "frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_RESOURCE_INIT"] == 2
    assert (
        record["DRIVER_TASK_RESOURCE_BLOCKER"]
        == "serial-console:serial-runtime-init:no-reply"
    )


def test_gate_summary_distinguishes_serial_fallback_from_driver_acceptance() -> None:
    fallback_events = normalizer.parse_events(
        [
            "SERIAL_RUNTIME_STATE owner=driver stage=serial-runtime-init "
            "status=no-reply acceptance=red",
            "[uart] driver-task runtime init failed owner=serial "
            "action=root-mini-uart-fallback acceptance=no",
            "SERIAL_RUNTIME_STATE owner=root stage=serial-runtime-init "
            "status=fallback acceptance=red reason=driver-task-no-reply",
        ]
    )

    fallback_record = normalizer.summarize_gates(fallback_events).to_record()
    assert fallback_record["SERIAL_FALLBACK_ACTIVE"] == "yes"
    assert fallback_record["SERIAL_DRIVER_ACCEPTED"] == "no"
    assert fallback_record["SERIAL_RUNTIME_FRONTIER"] == "serial-root-fallback"

    accepted_events = normalizer.parse_events(
        [
            "SERIAL_RUNTIME_STATE owner=driver stage=serial-runtime-init "
            "status=ready acceptance=green",
            "DRIVER_TASK_BOOT contract=serial role=serial started=yes "
            "pointer_free_ipc=yes owner_state=driver-owned",
        ]
    )

    accepted_record = normalizer.summarize_gates(accepted_events).to_record()
    assert accepted_record["SERIAL_FALLBACK_ACTIVE"] == "no"
    assert accepted_record["SERIAL_DRIVER_ACCEPTED"] == "yes"
    assert (
        accepted_record["SERIAL_RUNTIME_FRONTIER"]
        == "serial-driver-owner-state-ready"
    )

    cutover_events = normalizer.parse_events(
        [
            "SERIAL_RUNTIME_STATE owner=root stage=serial-runtime-init "
            "status=fallback acceptance=red reason=driver-task-deferred-until-prompt",
            "SERIAL_RUNTIME_STATE owner=driver stage=serial-runtime-init "
            "status=ready acceptance=green",
            "[uart] serial console cutover "
            "backend=driver-task-serial-client owner=serial",
            "SERIAL_RUNTIME_STATE owner=root stage=serial-runtime-init "
            "status=cutover acceptance=green reason=driver-task-attached",
            "DRIVER_TASK_OWNER_STATE contract=serial hot_path=serial-console "
            "owner_state=driver-owned descriptor=present root_pointer=no",
        ]
    )

    cutover_record = normalizer.summarize_gates(cutover_events).to_record()
    assert cutover_record["SERIAL_FALLBACK_ACTIVE"] == "no"
    assert cutover_record["SERIAL_DRIVER_ACCEPTED"] == "yes"
    assert (
        cutover_record["SERIAL_RUNTIME_FRONTIER"]
        == "serial-driver-owner-state-ready"
    )

    cutover_only_events = normalizer.parse_events(
        [
            "SERIAL_RUNTIME_STATE owner=root stage=serial-runtime-init "
            "status=fallback acceptance=red reason=driver-task-deferred-until-prompt",
            "[uart] serial console cutover "
            "backend=driver-task-serial-client owner=serial",
            "SERIAL_RUNTIME_STATE owner=root stage=serial-runtime-init "
            "status=cutover acceptance=green reason=driver-task-attached",
        ]
    )

    cutover_only_record = normalizer.summarize_gates(cutover_only_events).to_record()
    assert cutover_only_record["SERIAL_FALLBACK_ACTIVE"] == "no"
    assert cutover_only_record["SERIAL_DRIVER_ACCEPTED"] == "yes"

    cutover_deferred_events = normalizer.parse_events(
        [
            "SERIAL_RUNTIME_STATE owner=root stage=serial-runtime-init "
            "status=fallback acceptance=red reason=driver-task-deferred-until-prompt",
            "SERIAL_RUNTIME_STATE owner=driver stage=serial-runtime-init "
            "status=ready acceptance=green",
            "[uart] serial console cutover deferred backend=bcm2711-mini-uart "
            "reason=driver-task-rx-proof-missing action=root-uart-console",
            "SERIAL_RUNTIME_STATE owner=root stage=serial-runtime-init "
            "status=cutover-deferred acceptance=red reason=driver-task-rx-proof-missing",
        ]
    )

    cutover_deferred_record = normalizer.summarize_gates(
        cutover_deferred_events
    ).to_record()
    assert cutover_deferred_record["SERIAL_FALLBACK_ACTIVE"] == "yes"
    assert cutover_deferred_record["SERIAL_DRIVER_ACCEPTED"] == "no"
    assert (
        cutover_deferred_record["SERIAL_RUNTIME_FRONTIER"]
        == "serial-root-fallback"
    )


def test_gate_summary_tracks_current_serial_runtime_init_outstanding() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=serial hot_path=serial-console "
            "stage=runtime-descriptor-bootstrap status=no-reply "
            "acceptance=no code=none detail=none result=none frame_len=1472",
            "DRIVER_TASK_RING_CALL_BEGIN contract=serial endpoint=0x0649 "
            "request=10 opcode=1 flags=0x0000 arg0=0 arg1=3 "
            "aux0=0x53455249 aux1=0 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["SERIAL_DRIVER_ACCEPTED"] == "no"
    assert record["SERIAL_RUNTIME_FRONTIER"] == "serial-runtime-init-outstanding"


def test_gate_summary_tracks_hdmi_usb_and_wifi_frontiers() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=hdmi-text hot_path=hdmi-text "
            "stage=runtime-descriptor-bootstrap status=ready acceptance=no "
            "code=1 detail=0 result=1 frame_len=1472",
            "DRIVER_TASK_RESOURCE_INIT contract=hdmi-text hot_path=hdmi-text "
            "stage=hdmi-engine-init status=no-reply acceptance=no "
            "code=none detail=none result=none frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=runtime-descriptor-bootstrap "
            "status=ready acceptance=no code=1 detail=0 result=1 frame_len=1472",
            "DRIVER_TASK_RESOURCE_INIT contract=pcie-root hot_path=pcie-root "
            "stage=usb-prereq-pcie-replay status=no-reply acceptance=no "
            "code=none detail=none result=none frame_len=0",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=sdio-host tcb=0x05b8 "
            "runtime_descriptor=yes reason=root-shell-before-first-service-proof",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["HDMI_DESCRIPTOR_READY"] == "yes"
    assert record["HDMI_ENGINE_READY"] == "no"
    assert record["HDMI_OWNER_STATE_READY"] == "no"
    assert record["HDMI_RUNTIME_FRONTIER"] == "hdmi-engine-init-no-reply"
    assert record["USB_DRIVER_TASK_FRONTIER"] == "usb-prereq-pcie-replay-no-reply"
    assert record["USB_BLOCKER"] == "usb-prereq-pcie-replay-no-reply"
    assert record["WIFI_REPLAY_FRONTIER"] == "pre-prompt-deferred"


def test_gate_summary_tracks_hdmi_boot_failure_before_fallback_noise() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_BOOT contract=hdmi-text role=display status=failed "
            "err=seL4_NotEnoughMemory",
            "[local-seat] root HDMI diagnostic mirror unavailable "
            "detail=framebuffer-map action=serial-shell",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["HDMI_DESCRIPTOR_READY"] == "no"
    assert record["HDMI_RUNTIME_FRONTIER"] == "hdmi-engine-init-boot-failed"


def test_gate_summary_labels_prompt_gated_usb_runtime_deferral() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] cold-boot keyboard probe deferred "
            "reason=driver-task-runtime-unproved action=root-prompt-first",
            "[local-seat] cold-boot keyboard probe end stage=pre-net "
            "result=deferred-until-root-console polling_enabled=0",
            "[Cohesix] Root console ready (type 'help' for commands)",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["BOOT_HALTED"] == "no"
    assert record["ROOT_CONSOLE_READY"] == "yes"
    assert record["USB_GATE"] == 1
    assert record["USB_BLOCKER"] == "driver-task-runtime-deferred"


def test_gate_summary_labels_usb_engine_init_blocking_stall() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci-mmio-hint=none source=absent",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-engine-init status=begin "
            "acceptance=no code=none detail=none result=none frame_len=0",
            "DRIVER_TASK_RING_CALL_BEGIN contract=usb-local-seat "
            "endpoint=0x07a4 request=2 opcode=1 flags=0x0000 "
            "arg0=2 arg1=2 aux0=0x4c53494e aux1=0 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["USB_GATE"] == 2
    assert record["USB_BLOCKER"] == "usb-engine-init-blocking-call-stalled"


def test_gate_summary_labels_usb_engine_init_no_reply() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci-mmio-hint=none source=absent",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-engine-init status=begin "
            "acceptance=no code=none detail=none result=none frame_len=0",
            "DRIVER_TASK_RING_CALL_BEGIN contract=usb-local-seat "
            "endpoint=0x07a4 request=2 opcode=1 flags=0x2000 "
            "arg0=2 arg1=2 aux0=0x4c53494e aux1=0 frame_len=0",
            "DRIVER_TASK_RING_CALL_TIMEOUT contract=usb-local-seat "
            "endpoint=0x07a4 request=2 mode=nonblocking attempts=4096 "
            "opcode=1 arg0=2 aux0=0x4c53494e frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["USB_GATE"] == 2
    assert record["USB_BLOCKER"] == "usb-engine-init-no-reply"


def test_gate_summary_refines_stale_pcie_gate_after_usb_engine_init_progress() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=pcie-root "
            "hot_path=pcie-root stage=usb-prereq-pcie-engine-init "
            "status=ready acceptance=no code=1 detail=0 result=1 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=runtime-descriptor-bootstrap "
            "status=ready acceptance=no code=1 detail=0 result=2 frame_len=1472",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-engine-init status=begin "
            "acceptance=no code=none detail=none result=none frame_len=0",
            "DRIVER_TASK_RING_CALL_TIMEOUT contract=usb-local-seat "
            "endpoint=0x07a4 request=2 mode=nonblocking attempts=262144 "
            "opcode=1 arg0=2 aux0=0x4c53494e frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-engine-init status=no-reply "
            "acceptance=no code=none detail=none result=none frame_len=0",
            "usb: gate 1 name=hal-resources status=pass "
            "evidence=ownership_gate=1 next=pcie-vl805",
            "usb: gate 2 name=pcie-vl805 status=fail "
            "evidence=backend_attached=no linked_controller=no "
            "runtime_result=0x00000000 next=xhci-operational",
            "usb: next_action=inspect-linked-usb-runtime-progress "
            "blocker=linked-runtime-command-not-observed proof_gate=1 "
            "target_gate=10 detail=0x0000 result=0x00000000",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["USB_GATE"] == 2
    assert record["USB_BLOCKER"] == "usb-engine-init-no-reply"
    assert record["USB_DRIVER_TASK_FRONTIER"] == "usb-engine-init-no-reply"


def test_gate_summary_prefers_precise_usb_next_action_blocker() -> None:
    events = normalizer.parse_events(
        [
            "usb: gate 1 name=hal-resources status=pass "
            "evidence=ownership_gate=1 next=pcie-vl805",
            "usb: gate 2 name=pcie-vl805 status=pass "
            "evidence=pcie-owner-state-ready next=xhci-operational",
            "usb: gate 3 name=xhci-operational status=fail "
            "evidence=engine-init-no-reply next=command-event-rings",
            "usb: next_action=inspect-usb-xhci-mmio-entry "
            "blocker=usb-xhci-mmio-entry-no-reply proof_gate=2 "
            "target_gate=10 detail=0x0000 result=0x00000000",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["USB_GATE"] == 2
    assert record["USB_BLOCKER"] == "usb-xhci-mmio-entry-no-reply"


def test_gate_summary_preserves_usb_keyboard_frontier_after_root_port() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-engine-init status=ready "
            "acceptance=no code=1 detail=517 result=1 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-owner-state "
            "status=blocked-keyboard-enumeration acceptance=no code=1 "
            "detail=517 result=1 frame_len=0",
            "DRIVER_TASK_RING_CALL_TIMEOUT contract=usb-local-seat "
            "endpoint=0x07a4 request=3 mode=nonblocking attempts=128 "
            "opcode=1 arg0=2 aux0=0x00000000 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["USB_BLOCKER"] == "usb-keyboard-enumeration-no-reply"


def test_gate_summary_labels_linked_usb_hub_topology_blocker() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-engine-init status=ready "
            "acceptance=no code=1 detail=528 result=1 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-keyboard-enumeration "
            "status=hub-topology-no-keyboard acceptance=no code=1 "
            "detail=528 result=1 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["USB_BLOCKER"] == "hub-topology-no-keyboard"
    assert (
        record["USB_DRIVER_TASK_FRONTIER"]
        == "usb-keyboard-enumeration-hub-topology-no-keyboard"
    )


def test_gate_summary_labels_usb_first_report_enumeration_blocker() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-owner-state "
            "status=blocked-keyboard-enumeration acceptance=no code=1 "
            "detail=513 result=1 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-keyboard-first-report "
            "status=blocked-keyboard-enumeration acceptance=no code=3 "
            "detail=0 result=0 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert (
        record["USB_BLOCKER"]
        == "command-event-ring-not-proven"
    )
    assert (
        record["USB_DRIVER_TASK_FRONTIER"]
        == "usb-keyboard-first-report-blocked-keyboard-enumeration"
    )


def test_gate_summary_preserves_usb_runtime_enumeration_detail() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-keyboard-enumeration-retry "
            "status=root-port-connected acceptance=no code=1 "
            "detail=517 result=1 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert (
        record["USB_DRIVER_TASK_FRONTIER"]
        == "usb-keyboard-enumeration-retry-root-port-connected"
    )


def test_gate_summary_preserves_linked_usb_address_failure_detail() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-keyboard-enumeration-retry "
            "status=address-device-failed acceptance=no code=1 "
            "detail=531 result=1 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["USB_BLOCKER"] == "address-device-failed"
    assert (
        record["USB_DRIVER_TASK_FRONTIER"]
        == "usb-keyboard-enumeration-retry-address-device-failed"
    )


def test_gate_summary_labels_cyw43_transport_substage_faults() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-transport-init "
            "status=fault acceptance=no code=5 detail=21283 "
            "result=0 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_EXACT"] == "cyw43-backplane-chipcommon-read"
    assert record["WIFI_PHASE"] == "cyw43-transport-init"


def test_gate_summary_labels_pcie_hal_prep_gate() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=pcie-root hot_path=pcie-root "
            "stage=usb-prereq-pcie-replay status=ready acceptance=no "
            "code=none detail=none result=none frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=pcie-root hot_path=pcie-root "
            "stage=usb-prereq-pcie-engine-init status=blocked-hal-prep-required "
            "acceptance=no code=none detail=none result=none frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-xhci-init "
            "status=blocked-pcie-hal-prep acceptance=no code=none "
            "detail=none result=none frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["USB_DRIVER_TASK_FRONTIER"] == "usb-xhci-init-blocked-pcie-hal-prep"
    assert record["USB_BLOCKER"] == "usb-prereq-pcie-engine-init-blocked-hal-prep-required"


def test_gate_summary_tracks_net_and_sdio_replay_blockers() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=descriptor-replay blocker=ready",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=engine-init blocker=no-reply",
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host "
            "selected=wifi-owner-link attempted=yes stage=descriptor-replay "
            "blocker=ready",
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host "
            "selected=wifi-owner-link attempted=yes stage=sdio-first-command "
            "blocker=fault",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["NET_DRIVER_TASK_REPLAY_EVENTS"] == 2
    assert (
        record["NET_DRIVER_TASK_REPLAY_BLOCKER"]
        == "cyw43-wifi:engine-init:no-reply"
    )
    assert record["SDIO_DRIVER_TASK_REPLAY_EVENTS"] == 2
    assert (
        record["SDIO_DRIVER_TASK_REPLAY_BLOCKER"]
        == "sdio-host:sdio-first-command:fault"
    )
    assert record["WIFI_BLOCKER"] == "sdio-card-select"
    assert record["WIFI_REPLAY_FRONTIER"] == "sdio-driver-task-replay"


def test_gate_summary_keeps_boot_sdio_replay_over_later_wifi_prompt_error() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=descriptor-replay blocker=ready",
            "DRIVER_TASK_RESOURCE_INIT contract=sdio-host hot_path=sdio-host "
            "stage=cyw43-sdio-prereq status=begin acceptance=no code=none "
            "detail=none result=none frame_len=0",
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host "
            "selected=wifi-owner-link attempted=yes stage=descriptor-replay "
            "blocker=ready",
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host "
            "selected=wifi-owner-link attempted=yes stage=engine-init "
            "blocker=begin",
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host "
            "selected=wifi-owner-link attempted=yes stage=engine-init "
            "blocker=no-reply",
            "DRIVER_TASK_RESOURCE_INIT contract=sdio-host hot_path=sdio-host "
            "stage=sdio-engine-init status=no-reply acceptance=no code=none "
            "detail=none result=none frame_len=0",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-sdio-prereq blocker=failed",
            "[pi4-wifi] firmware stage=debug-probe-ht subcommand=probe-ht "
            "arg=0x12001c00 value=0x00000000 result=error",
            "wifi: debug subcommand=load-fw action=complete profile=stateful "
            "mode=one-shot result=error error=unsupported operation: "
            "pi4-wifi-driver-task-runtime-required",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_BLOCKER"] == "sdio-engine-init-no-reply"
    assert record["WIFI_EXACT"] == "sdio-engine-init-no-reply"
    assert record["WIFI_PHASE"] == "engine-init"
    assert record["WIFI_BLOCKER_LINE"] == 5


def test_gate_summary_reports_exact_sdio_engine_init_subfault() -> None:
    events = normalizer.parse_events(
        [
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host "
            "selected=wifi-owner-link attempted=yes stage=descriptor-replay "
            "blocker=ready",
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host "
            "selected=wifi-owner-link attempted=yes stage=engine-init "
            "blocker=clock-failed",
            "DRIVER_TASK_RESOURCE_INIT contract=sdio-host hot_path=sdio-host "
            "stage=sdio-engine-init status=clock-failed acceptance=no code=5 "
            "detail=0x5512 result=0 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_GATE"] == 2
    assert record["WIFI_BLOCKER"] == "sdio-engine-init-clock-failed"
    assert record["WIFI_EXACT"] == "sdio-engine-init-clock-failed"
    assert record["WIFI_PHASE"] == "engine-init"


def test_gate_summary_preserves_sdio_adopt_progress_blocker() -> None:
    events = normalizer.parse_events(
        [
            "wifi: sdio linked_runtime_progress marker_valid=yes sequence=2 "
            "phase=85 phase_name=sdio-adopt-present-read-begin "
            "aux0=0x454e474e gate=2 blocker=sdio-adopt-present-read-no-reply "
            "next_action=inspect-sdhci-present-state-read",
            "wifi: gate 1 name=hal-power-reset status=inferred "
            "evidence=power=unknown reset=unknown source=hal-runtime-required "
            "next=sdio-card-select",
            "wifi: gate 2 name=sdio-card-select status=fail "
            "evidence=stage=engine-init status=no-reply phase=85 "
            "phase_name=sdio-adopt-present-read-begin marker_valid=yes "
            "source=linked-runtime next=cccr-fbr-ready",
            "wifi: next_action=inspect-sdhci-present-state-read "
            "blocker=sdio-adopt-present-read-no-reply proof_gate=1 "
            "target_gate=10 source=hal-runtime-required",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_GATE"] == 1
    assert record["WIFI_BLOCKER"] == "sdio-adopt-present-read-no-reply"
    assert record["WIFI_EXACT"] == "sdio-adopt-present-read-no-reply"
    assert record["WIFI_PHASE"] == "sdio-adopt-present-read-no-reply"


def test_gate_summary_preserves_sdio_hardware_entry_progress_blocker() -> None:
    events = normalizer.parse_events(
        [
            "wifi: sdio linked_runtime_progress marker_valid=yes sequence=2 "
            "phase=157 phase_name=sdio-hw-entry aux0=0x454e474e "
            "gate=2 blocker=sdio-sdhci-mmio-entry-no-reply "
            "next_action=inspect-sdhci-first-mmio-access",
            "wifi: gate 1 name=hal-power-reset status=inferred "
            "evidence=power=unknown reset=unknown source=hal-runtime-required "
            "next=sdio-card-select",
            "wifi: gate 2 name=sdio-card-select status=fail "
            "evidence=stage=engine-init status=no-reply phase=157 "
            "phase_name=sdio-hw-entry marker_valid=yes source=linked-runtime "
            "next=cccr-fbr-ready",
            "wifi: next_action=inspect-sdhci-first-mmio-access "
            "blocker=sdio-sdhci-mmio-entry-no-reply proof_gate=1 "
            "target_gate=10 source=hal-runtime-required",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_GATE"] == 1
    assert record["WIFI_BLOCKER"] == "sdio-sdhci-mmio-entry-no-reply"
    assert record["WIFI_EXACT"] == "sdio-sdhci-mmio-entry-no-reply"
    assert record["WIFI_PHASE"] == "sdio-sdhci-mmio-entry-no-reply"


def test_gate_summary_preserves_sdio_shadow_reset_progress_blocker() -> None:
    events = normalizer.parse_events(
        [
            "wifi: sdio linked_runtime_progress marker_valid=yes sequence=2 "
            "phase=174 phase_name=sdio-shadow-reset-begin aux0=0x454e474e "
            "gate=2 blocker=sdio-shadow-reset-no-reply "
            "next_action=inspect-sdio-register-shadow-reset",
            "wifi: gate 1 name=hal-power-reset status=inferred "
            "evidence=power=unknown reset=unknown source=hal-runtime-required "
            "next=sdio-card-select",
            "wifi: gate 2 name=sdio-card-select status=fail "
            "evidence=stage=engine-init status=no-reply phase=174 "
            "phase_name=sdio-shadow-reset-begin marker_valid=yes "
            "source=linked-runtime next=cccr-fbr-ready",
            "wifi: next_action=inspect-sdio-register-shadow-reset "
            "blocker=sdio-shadow-reset-no-reply proof_gate=1 "
            "target_gate=10 source=hal-runtime-required",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_GATE"] == 1
    assert record["WIFI_BLOCKER"] == "sdio-shadow-reset-no-reply"
    assert record["WIFI_EXACT"] == "sdio-shadow-reset-no-reply"
    assert record["WIFI_PHASE"] == "sdio-shadow-reset-no-reply"


def test_gate_summary_promotes_cyw43_engine_init_replay_over_hal_power_noise() -> None:
    events = normalizer.parse_events(
        [
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host selected=wifi-owner-link "
            "attempted=yes stage=engine-init blocker=ready",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes policy=wifi "
            "attempted=yes stage=engine-init blocker=begin",
            "DRIVER_TASK_RING_CALL_BEGIN contract=cyw43455 endpoint=0x1975 "
            "request=2 opcode=1 flags=0x2000 arg0=5 arg1=8 aux0=0x494e4954",
            "DRIVER_TASK_RING_CALL_TIMEOUT contract=cyw43455 endpoint=0x1975 "
            "request=2 mode=nonblocking attempts=262144 opcode=1 arg0=5 "
            "aux0=0x494e4954 frame_len=0",
            "DRIVER_TASK_RING_PROGRESS contract=cyw43455 request=2 "
            "expected_aux0=0x494e4954 marker_valid=yes marker_sequence=2 "
            "marker_phase=172 marker_phase_name=engine-init-runtime-entry "
            "marker_aux0=0x494e4954",
            "wifi: cyw43 linked_runtime_progress marker_valid=yes sequence=2 "
            "phase=172 phase_name=engine-init-runtime-entry aux0=0x494e4954 "
            "gate=2 blocker=cyw43-engine-init-runtime-entry-no-reply "
            "next_action=inspect-linked-cyw43-engine-init-branch-entry",
            "wifi: sdio linked_runtime_progress marker_valid=yes sequence=0 "
            "phase=202 phase_name=runtime-poll-ready aux0=0x00000007 "
            "gate=0 blocker=sdio-linked-runtime-progress-no-reply "
            "next_action=inspect-linked-sdio-runtime-progress",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes policy=wifi "
            "attempted=yes stage=engine-init blocker=no-reply",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=net-engine-init status=no-reply acceptance=no",
            "wifi: gate 1 name=hal-power-reset status=fail evidence=power=unknown "
            "reset=unknown source=hal-runtime-required next=sdio-card-select",
            "wifi: next_action=verify-hal-power-reset-resources blocker=wifi-power-reset "
            "proof_gate=0 target_gate=10 source=hal-runtime-required",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_GATE"] == 1
    assert record["WIFI_BLOCKER"] == "cyw43-engine-init-no-reply"
    assert record["WIFI_EXACT"] == "cyw43-engine-init-runtime-entry-no-reply"
    assert record["WIFI_PHASE"] == "cyw43-engine-init-runtime-entry-no-reply"
    assert record["NET_DRIVER_TASK_REPLAY_BLOCKER"] == "cyw43-wifi:engine-init:no-reply"


def test_gate_summary_preserves_cyw43_engine_init_branch_progress_blocker() -> None:
    events = normalizer.parse_events(
        [
            "wifi: cyw43 linked_runtime_progress marker_valid=yes sequence=2 "
            "phase=176 phase_name=cyw43-engine-init-branch aux0=0x494e4954 "
            "gate=2 blocker=cyw43-engine-init-state-slot-no-reply "
            "next_action=inspect-cyw43-runtime-state-slot-entry",
            "wifi: gate 1 name=hal-power-reset status=inferred "
            "evidence=power=unknown reset=unknown source=hal-runtime-required "
            "next=sdio-card-select",
            "wifi: gate 2 name=sdio-card-select status=fail "
            "evidence=stage=cyw43-transport status=progress-only phase=176 "
            "phase_name=cyw43-engine-init-branch marker_valid=yes "
            "source=linked-runtime next=cccr-fbr-ready",
            "wifi: next_action=inspect-cyw43-runtime-state-slot-entry "
            "blocker=cyw43-engine-init-state-slot-no-reply proof_gate=1 "
            "target_gate=10 source=hal-runtime-required",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_GATE"] == 1
    assert record["WIFI_BLOCKER"] == "cyw43-engine-init-state-slot-no-reply"
    assert record["WIFI_EXACT"] == "cyw43-engine-init-state-slot-no-reply"
    assert record["WIFI_PHASE"] == "cyw43-engine-init-state-slot-no-reply"


def test_gate_summary_derives_cyw43_state_slot_from_raw_ring_progress() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes policy=wifi "
            "attempted=yes stage=engine-init blocker=begin",
            "DRIVER_TASK_RING_CALL_TIMEOUT contract=cyw43455 endpoint=0x1975 "
            "request=2 mode=nonblocking attempts=262144 opcode=1 arg0=5 "
            "aux0=0x494e4954 frame_len=0",
            "DRIVER_TASK_RING_PROGRESS contract=cyw43455 request=2 "
            "expected_aux0=0x494e4954 marker_valid=yes marker_sequence=2 "
            "marker_phase=176 marker_phase_name=cyw43-engine-init-branch "
            "marker_aux0=0x494e4954",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes policy=wifi "
            "attempted=yes stage=engine-init blocker=no-reply",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_GATE"] == 1
    assert record["WIFI_BLOCKER"] == "cyw43-engine-init-no-reply"
    assert record["WIFI_EXACT"] == "cyw43-engine-init-state-slot-no-reply"
    assert record["WIFI_PHASE"] == "cyw43-engine-init-state-slot-no-reply"
    assert record["NET_DRIVER_TASK_REPLAY_BLOCKER"] == "cyw43-wifi:engine-init:no-reply"


def test_gate_summary_tracks_split_sdio_command_probe_blockers() -> None:
    events = normalizer.parse_events(
        [
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host "
            "selected=wifi-owner-link attempted=yes stage=sdio-cmd0-go-idle "
            "blocker=ready",
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host "
            "selected=wifi-owner-link attempted=yes stage=sdio-cmd5-ocr "
            "blocker=fault",
            "DRIVER_TASK_RESOURCE_INIT contract=sdio-host "
            "hot_path=sdio-host stage=sdio-cmd5-ocr status=fault "
            "acceptance=no code=5 detail=20737 result=0 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["SDIO_DRIVER_TASK_REPLAY_EVENTS"] == 2
    assert (
        record["SDIO_DRIVER_TASK_REPLAY_BLOCKER"]
        == "sdio-host:sdio-cmd5-ocr:fault"
    )
    assert (
        record["DRIVER_TASK_RESOURCE_BLOCKER"]
        == "sdio-host:sdio-cmd5-ocr:fault"
    )
    assert record["WIFI_GATE"] == 2
    assert record["WIFI_BLOCKER"] == "sdio-card-select"
    assert record["WIFI_EXACT"] == "sdio-command-unavailable"
    assert record["WIFI_PHASE"] == "sdio-cmd5-ocr"


def test_gate_summary_tracks_sdio_cmd7_command_fault() -> None:
    events = normalizer.parse_events(
        [
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host "
            "selected=wifi-owner-link attempted=yes stage=sdio-cmd7-select-r1-fallback "
            "blocker=fault",
            "SDIO_DRIVER_TASK_COMMAND_FAULT contract=sdio-host "
            "stage=sdio-cmd7-select-r1-fallback cmd=7 arg=0x00010000 "
            "flags=0x0002 detail=0x5101 reason=sdio-command-unavailable "
            "result=0x02018040 xfer_stage=command xfer_status=0x018040 "
            "xfer_reason=sdhci-command",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_GATE"] == 2
    assert record["WIFI_BLOCKER"] == "sdio-card-select"
    assert record["WIFI_EXACT"] == "sdio-command-unavailable"
    assert record["WIFI_PHASE"] == "sdio-cmd7-select-r1-fallback"
    assert (
        record["SDIO_DRIVER_TASK_REPLAY_BLOCKER"]
        == "sdio-host:sdio-cmd7-select-r1-fallback:fault"
    )


def test_gate_summary_labels_cyw43_transport_substage_fault_detail() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-transport-init status=fault "
            "acceptance=no code=5 detail=21271 result=0 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_EXACT"] == "cyw43-transport-host-bus-width"
    assert record["WIFI_PHASE"] == "cyw43-transport-init"


def test_gate_summary_labels_generic_cyw43_transport_init_fault_detail() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-transport-init status=fault "
            "acceptance=no code=5 detail=21249 result=0 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_EXACT"] == "cyw43-transport-init"
    assert record["WIFI_PHASE"] == "cyw43-transport-init"


def test_gate_summary_labels_cyw43_backplane_substage_fault_detail() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-transport-init status=fault "
            "acceptance=no code=5 detail=21279 result=0 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_EXACT"] == "cyw43-backplane-armcr4-reset"
    assert record["WIFI_PHASE"] == "cyw43-transport-init"


def test_gate_summary_labels_cyw43_sdio_descriptor_fault_detail() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-transport-init status=fault "
            "acceptance=no code=5 detail=20738 result=0 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_EXACT"] == "cyw43-sdio-descriptor-unavailable"
    assert record["WIFI_PHASE"] == "cyw43-transport-init"


def test_gate_summary_preserves_cyw43_chunk_detail_across_aggregate_failure() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-firmware-chunk status=fault "
            "acceptance=no code=5 detail=20738 result=0 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-firmware status=failed "
            "acceptance=no code=none detail=none result=none frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_EXACT"] == "cyw43-sdio-descriptor-unavailable"
    assert record["WIFI_PHASE"] == "cyw43-firmware-chunk"


def test_gate_summary_labels_cyw43_firmware_prep_fault_detail() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-firmware-prep status=fault "
            "acceptance=no code=5 detail=21256 result=0 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_EXACT"] == "cyw43-firmware-prep"
    assert record["WIFI_PHASE"] == "cyw43-firmware-prep"


def test_gate_summary_preserves_cyw43_transport_detail_for_replay_blocker() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-firmware blocker=begin",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-transport-init status=fault "
            "acceptance=no code=5 detail=21279 result=0 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-firmware status=failed "
            "acceptance=no code=none detail=none result=none frame_len=0",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-firmware blocker=failed",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_BLOCKER"] == "cyw43-firmware-runtime-replay"
    assert record["WIFI_EXACT"] == "cyw43-backplane-armcr4-reset"
    assert record["WIFI_PHASE"] == "cyw43-transport-init"


def test_gate_summary_labels_deferred_wifi_start_without_replay() -> None:
    events = normalizer.parse_events(
        [
            "[Cohesix] Root console ready (type 'help' for commands)",
            "cohesix> [local-seat] xhci boot contract raw_hint=0x0000000000000000/0",
            "[net-console] deferred resume reason=root-prompt-printed action=start-wifi",
            "[trace] deferred Wi-Fi logs remain on serial",
            "cohesix> ",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["ROOT_PROMPT_SEEN"] == "yes"
    assert record["WIFI_GATE"] == 1
    assert record["WIFI_BLOCKER"] == "wifi-started-no-replay"
    assert record["WIFI_EXACT"] == "wifi-started-no-replay"
    assert record["WIFI_BLOCKER_LINE"] == 3
    assert record["NET_DRIVER_TASK_REPLAY_EVENTS"] == 0
    assert record["SDIO_DRIVER_TASK_REPLAY_EVENTS"] == 0


def test_gate_summary_keeps_deferred_wifi_replay_blocker_after_start() -> None:
    events = normalizer.parse_events(
        [
            "[net-console] deferred resume reason=root-prompt-printed action=start-wifi",
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host "
            "selected=wifi-owner-link attempted=yes stage=descriptor-replay "
            "blocker=ready",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=engine-init blocker=no-reply",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_BLOCKER"] == "cyw43-engine-init-no-reply"
    assert record["WIFI_REPLAY_FRONTIER"] == "cyw43-driver-task-replay"
    assert record["NET_DRIVER_TASK_REPLAY_EVENTS"] == 1
    assert record["NET_DRIVER_TASK_REPLAY_BLOCKER"] == "cyw43-wifi:engine-init:no-reply"
    assert record["SDIO_DRIVER_TASK_REPLAY_EVENTS"] == 1


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


def test_gate_summary_promotes_bootstrap_fatal_panic_detail() -> None:
    events = normalizer.parse_events(
        [
            "[PANIC] panicked at apps/root-task/src/kernel.rs:3213:38:",
            "[bootstrap:fatal] serial driver-task runtime missing after owner-state cutover",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.boot_halted is True
    assert gates.to_record()["PANIC_SEEN"] == "yes"
    assert (
        gates.to_record()["PANIC_REASON"]
        == "serial-driver-task-runtime-missing-after-owner-state-cutover"
    )


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


def test_gate_summary_reports_linked_command_snapshot_events_as_event_ring_alive() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0203 "
            "result=0x03000503 root_port_mask=0x00 slot=5 ep_id=2 "
            "scan_pass=0 root_port_power=yes cmd_path=no port_event=no "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no "
            "cmd_proof=yes cmd_events_seen=3 cmd_slot_or_polls=5 "
            "cmd_event_type=2 cmd_ack_failures=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_EVENT_RING_ALIVE"] == "yes"
    assert record["USB_PSC_DRAIN_COUNT"] == 3
    assert record["USB_PSC_DRAIN_MASK"] == "0x00000000"


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
            "usb: linked_runtime command-probe result=enable-slot-ok",
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


def test_gate_summary_reports_pcie_owner_ring_before_keyboard_not_ready() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb ownership_proof proof_gate=3 target_gate=10 "
            "cfg_live=yes cmd_live=yes mailbox=mailbox-acked",
            "DRIVER_TASK_RESOURCE_INIT contract=pcie-root hot_path=pcie-root "
            "stage=pcie-owner-turn status=fault acceptance=no code=5 detail=3 "
            "result=0 frame_len=0",
            "[local-seat] vl805 posted-write flush rejected "
            "reason=pcie-owner-ring-unavailable action=fail-closed",
            "[local-seat] xhci.diag stage=0x0331 tag=drop-skip-uninitialized",
            "usb: golden_path outcome=controller-init-failed command_probe=n/a "
            "progress=no-controller proof_gate=1 diag_stage=0x0331 "
            "diag_tag=drop-skip-uninitialized",
            "usb: runtime_gate keyboard=no first_report=no first_byte=no "
            "proof_gate=0 target_gate=10 next=keyboard-ready "
            "blocker=keyboard-not-ready",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "pcie-owner-ring-unavailable"


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
        "xhci-preseed map exact miss reason=no-device-coverage": (
            "pcie-xhci-device-coverage-missing"
        ),
        "pi4-pcie-root-cfg map exact miss reason=no-device-coverage": (
            "pcie-xhci-device-coverage-missing"
        ),
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
        "device-desc failed": "device-descriptor-failed",
        "config-desc failed": "config-descriptor-failed",
        "set-config timeout": "set-config",
        "hid-init-failed": "hid-init-failed",
        "keyboard-missing": "no-keyboard-found",
        "first-report timeout": "hid-first-report",
        "first-byte missing": "keyboard-first-byte",
        "usb-engine-init-mark-no-reply": "usb-engine-init-mark-no-reply",
        "usb-runtime-init-entry-no-reply": "usb-runtime-init-entry-no-reply",
        "usb-runtime-state-access-no-reply": "usb-runtime-state-access-no-reply",
        "usb-engine-init-state-reset-no-reply": "usb-engine-init-state-reset-no-reply",
        "usb-engine-init-hardware-entry-no-reply": "usb-engine-init-hardware-entry-no-reply",
        "usb-xhci-mmio-entry-no-reply": "usb-xhci-mmio-entry-no-reply",
        "usb-xhci-capability-read-no-reply": "usb-xhci-capability-read-no-reply",
        "usb-xhci-capability-invalid": "usb-xhci-capability-invalid",
        "usb-pcie-posted-write-flush-no-reply": "usb-pcie-posted-write-flush-no-reply",
        "usb-pcie-posted-write-flush-failed": "usb-pcie-posted-write-flush-failed",
        "usb-pcie-posted-write-flush-next-edge-no-reply": "usb-pcie-posted-write-flush-next-edge-no-reply",
        "unknown-usb-edge": "unknown-usb-edge",
    }

    for raw, expected in cases.items():
        assert normalizer.normalize_usb_blocker(raw) == expected


def test_usb_driver_task_blocker_gate_splits_engine_init_from_xhci_entry() -> None:
    assert normalizer.usb_driver_task_blocker_gate("usb-engine-init-no-reply") == 2
    assert normalizer.usb_driver_task_blocker_gate("usb-engine-init-mark-no-reply") == 2
    assert normalizer.usb_driver_task_blocker_gate("usb-engine-init-state-reset-no-reply") == 2
    assert normalizer.usb_driver_task_blocker_gate("usb-xhci-mmio-entry-no-reply") == 2
    assert normalizer.usb_driver_task_blocker_gate("usb-xhci-capability-read-no-reply") == 2
    assert normalizer.usb_driver_task_blocker_gate("usb-xhci-halt-wait-no-reply") == 3
    assert normalizer.usb_driver_task_blocker_gate("usb-pcie-posted-write-flush-no-reply") == 3
    assert normalizer.usb_driver_task_blocker_gate("enable-slot-completion-pending") == 4
    assert normalizer.usb_driver_task_blocker_gate("enable-slot-completion-poll-no-reply") == 4
    assert normalizer.usb_driver_task_blocker_gate("enable-slot-event-dma-load-done-no-reply") == 4
    assert normalizer.usb_driver_task_blocker_gate("enable-slot-event-invalidate-done-no-reply") == 4
    assert normalizer.usb_driver_task_blocker_gate("enable-slot-event-peek-no-reply") == 4
    assert normalizer.usb_driver_task_blocker_gate("enable-slot-event-read-begin-no-reply") == 4
    assert normalizer.usb_driver_task_blocker_gate("enable-slot-event-read-done-no-reply") == 4
    assert normalizer.usb_driver_task_blocker_gate("enable-slot-event-slot-empty") == 4
    assert normalizer.usb_driver_task_blocker_gate("enable-slot-event-cycle-mismatch") == 4
    assert normalizer.usb_driver_task_blocker_gate("root-port-reset-no-reply") == 5
    assert normalizer.usb_driver_task_blocker_gate("address-enable-slot-no-reply") == 5
    assert (
        normalizer.usb_driver_task_blocker_gate("address-device-context-publish-no-reply")
        == 5
    )
    assert (
        normalizer.usb_driver_task_blocker_gate("address-device-command-submit-no-reply")
        == 5
    )
    assert (
        normalizer.usb_driver_task_blocker_gate("address-device-command-completion-no-reply")
        == 5
    )
    assert normalizer.usb_driver_task_blocker_gate("address-device-publish-no-reply") == 6
    assert normalizer.usb_driver_task_blocker_gate("device-descriptor-no-reply") == 6


def test_gate_summary_keeps_xhci_device_coverage_miss_over_unavailable() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci-preseed map exact miss "
            "paddr=0x0000000600000000 reason=no-device-coverage",
            "[local-seat] pi4-pcie-root-cfg map exact miss "
            "paddr=0x00000000fd500000 reason=no-device-coverage",
            "[local-seat] vl805 live cfg unavailable; "
            "xhci probe forced no-live-bus-master cmd=0x0000",
            "[local-seat] pi4 keyboard unavailable detail=xhci-init",
            "[local-seat] pi4 keyboard runtime init result=unavailable",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "pcie-xhci-device-coverage-missing"


def test_normalize_wifi_blocker_alias_table_covers_post_ht_gates() -> None:
    cases = {
        "cyw43-firmware-ready-timeout": "firmware-ready-timeout",
        "0x532d": "cyw43-post-release-mailbox-ready",
        "21294": "cyw43-post-release-protocol-version",
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


def test_gate_summary_rejects_legacy_local_seat_command_probe_success() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "usb: contract current=controller-ready expected=command-ring-recovery",
            "[local-seat] xhci root-port command-probe result=enable-slot-ok",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_tracks_linked_runtime_command_ring_ready_success() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "usb: contract current=controller-ready expected=command-ring-recovery",
            "usb: linked_runtime command-probe result=enable-slot-ok",
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


def test_gate_summary_normalizes_legacy_hid_first_byte_blocker() -> None:
    events = normalizer.parse_events(
        [
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "proof_gate=9 target_gate=10 next=keyboard-first-byte "
            "blocker=hid-first-byte",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 9
    assert gates.usb_blocker == "keyboard-first-byte"


def test_gate_summary_tracks_usb_runtime_enum_snapshot_detail() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0204 "
            "result=0x0f00001f root_port_mask=0x1f slot=0 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0212 "
            "result=0x0f00001f root_port_mask=0x1f slot=0 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 5
    assert gates.usb_blocker == "enable-slot-failed"


def test_gate_summary_tracks_root_port_connected_detail() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0205 "
            "result=0x0f000001 root_port_mask=0x01 slot=0 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 5
    assert gates.usb_blocker == "root-port-connected"


def test_gate_summary_tracks_usb_hub_attach_substep_detail() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0219 "
            "result=0x0f000201 root_port_mask=0x00 slot=1 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 7
    assert gates.usb_blocker == "hub-descriptor-failed"


def test_gate_summary_tracks_usb_command_ring_pending_detail() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0203 "
            "result=0x03000001 root_port_mask=0x00 slot=0 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=no port_event=no "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no "
            "cmd_proof=yes cmd_events_seen=1 cmd_slot_or_polls=1 cmd_event_type=0 "
            "cmd_ack_failures=0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "enable-slot-completion-pending"


def test_gate_summary_preserves_command_pending_over_stale_startup_projection() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0203 "
            "result=0x03000000 root_port_mask=0x00 slot=0 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=no port_event=no "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no "
            "cmd_proof=yes cmd_events_seen=0 cmd_slot_or_polls=0 cmd_event_type=0 "
            "cmd_ack_failures=0",
            "usb: diag linked_runtime_progress marker_valid=yes sequence=5 phase=134 "
            "phase_name=usb-command-proof-doorbell-done aux0=0x55534245 gate=4 "
            "blocker=enable-slot-completion-pending "
            "next_action=poll-enable-slot-completion",
            "usb: gate 1 name=hal-resources status=pass "
            "evidence=hardware_owner=linked-runtime root_action=admission-descriptor-diagnostics linked_controller=yes detail=0x0203 "
            "next=pcie-vl805",
            "usb: gate 2 name=pcie-vl805 status=pass "
            "evidence=backend_attached=yes linked_controller=yes "
            "runtime_result=0x03000000 next=xhci-operational",
            "usb: gate 3 name=xhci-operational status=pass "
            "evidence=linked_detail=0x0203 linked_gate=4 "
            "next=command-event-rings",
            "usb: gate 4 name=command-event-rings status=pass "
            "evidence=queue_result=no queued_reports=0 doorbell=no "
            "preserved_events=0 transfer_events=0 next=root-port-connected",
            "usb: gate 5 name=root-port-connected status=fail "
            "evidence=linked_detail=0x0203 result=0x03000000 "
            "next=device-addressed",
            "usb: next_action=poll-enable-slot-completion "
            "blocker=enable-slot-completion-pending proof_gate=4 target_gate=10 "
            "detail=0x0203 result=0x03000000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "enable-slot-completion-pending"


def test_gate_summary_tracks_command_event_peek_begin_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0203 "
            "result=0x03000000 root_port_mask=0x00 slot=0 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=no port_event=no "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no "
            "cmd_proof=yes cmd_events_seen=0 cmd_slot_or_polls=0 cmd_event_type=0 "
            "cmd_ack_failures=0",
            "usb: linked_runtime_progress marker_valid=yes sequence=5 phase=186 "
            "phase_name=usb-command-proof-event-peek-begin aux0=0x55534245 "
            "gate=4 blocker=enable-slot-event-peek-no-reply "
            "next_action=inspect-event-ring-trb-read-or-cache-invalidate",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "enable-slot-event-peek-no-reply"


def test_gate_summary_tracks_raw_command_event_peek_begin_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0203 "
            "result=0x03000000 root_port_mask=0x00 slot=0 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=no port_event=no "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no "
            "cmd_proof=yes cmd_events_seen=0 cmd_slot_or_polls=0 cmd_event_type=0 "
            "cmd_ack_failures=0",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=5 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=5 "
            "marker_phase=186 marker_phase_name=usb-command-proof-event-peek-begin "
            "marker_aux0=0x55534245",
            "[local-seat] cold-boot keyboard probe end stage=pre-net "
            "result=keyboard-unavailable polling_enabled=0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "enable-slot-event-peek-no-reply"


def test_gate_summary_preserves_address_failure_over_live_root_reset_marker() -> None:
    events = normalizer.parse_events(
        [
            "usb: runtime_gate keyboard=no first_report=no first_byte=no "
            "first_byte_source=none proof_gate=6 target_gate=10 "
            "next=device-descriptor blocker=address-device-failed "
            "detail=0x0213 result=0x0f200201",
            "usb: linked_runtime_progress marker_valid=yes sequence=10 phase=190 "
            "phase_name=usb-root-port-reset-begin aux0=0x55534245 gate=5 "
            "blocker=root-port-reset-no-reply "
            "next_action=inspect-root-port-reset-completion",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 6
    assert gates.usb_blocker == "address-device-failed"


def test_gate_summary_tracks_raw_root_port_reset_substage_progress() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
            "marker_phase=329 marker_phase_name=usb-root-port-reset-poll-begin "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 5
    assert gates.usb_blocker == "root-port-reset-completion-no-reply"


def test_gate_summary_tracks_raw_address_command_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0205 "
            "result=0x0f000001 root_port_mask=0x01 slot=0 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
            "marker_phase=195 marker_phase_name=usb-address-command-begin "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 5
    assert gates.usb_blocker == "address-device-command-completion-no-reply"


def test_gate_summary_tracks_raw_device_addressed_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0206 "
            "result=0x0f000001 root_port_mask=0x01 slot=2 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
            "marker_phase=198 marker_phase_name=usb-device-addressed "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 6
    assert gates.usb_blocker == "device-descriptor-no-reply"


def test_gate_summary_tracks_device_descriptor_wait_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0206 "
            "result=0x0f000001 root_port_mask=0x01 slot=2 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
            "marker_phase=220 marker_phase_name=usb-device-descriptor-wait-begin "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 6
    assert gates.usb_blocker == "device-descriptor-transfer-no-reply"


def test_gate_summary_keeps_precise_descriptor_progress_over_gate_table() -> None:
    events = normalizer.parse_events(
        [
            "usb: linked_runtime_progress marker_valid=yes sequence=8 phase=220 "
            "phase_name=usb-device-descriptor-wait-begin aux0=0x55534245 "
            "gate=6 blocker=device-descriptor-transfer-no-reply "
            "next_action=poll-ep0-device-descriptor-transfer",
            "usb: gate 6 name=device-addressed status=pass "
            "evidence=linked_detail=0x0206 next=config-and-hid-descriptors",
            "usb: gate 7 name=config-and-hid-descriptors status=fail "
            "evidence=linked_detail=0x0206 next=keyboard-ready",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 6
    assert gates.usb_blocker == "device-descriptor-transfer-no-reply"


def test_gate_summary_tracks_device_descriptor_status_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0206 "
            "result=0x0f000001 root_port_mask=0x01 slot=2 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
            "marker_phase=222 marker_phase_name=usb-device-descriptor-status-event "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 7
    assert gates.usb_blocker == "config-descriptor-no-reply"


def test_gate_summary_tracks_config_descriptor_header_wait_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0207 "
            "result=0x0f000001 root_port_mask=0x01 slot=2 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
            "marker_phase=236 marker_phase_name=usb-config-descriptor-header-wait-begin "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 7
    assert gates.usb_blocker == "config-descriptor-header-transfer-no-reply"


def test_gate_summary_tracks_config_descriptor_header_event_empty_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0207 "
            "result=0x0f000001 root_port_mask=0x01 slot=2 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
            "marker_phase=289 "
            "marker_phase_name=usb-config-descriptor-header-transfer-event-slot-empty "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 7
    assert gates.usb_blocker == "config-descriptor-header-transfer-event-slot-empty"


def test_gate_summary_tracks_device_descriptor_event_cycle_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0206 "
            "result=0x0f000001 root_port_mask=0x01 slot=2 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
            "marker_phase=284 "
            "marker_phase_name=usb-device-descriptor-transfer-event-cycle-mismatch "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 6
    assert gates.usb_blocker == "device-descriptor-transfer-event-cycle-mismatch"


def test_gate_summary_tracks_config_descriptor_full_status_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0207 "
            "result=0x0f000001 root_port_mask=0x01 slot=2 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
            "marker_phase=246 marker_phase_name=usb-config-descriptor-full-status-event "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 7
    assert gates.usb_blocker == "hid-endpoint-not-ready"


def test_gate_summary_tracks_hid_endpoint_parse_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0211 "
            "result=0x0f000101 root_port_mask=0x01 slot=2 ep_id=1 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=yes preserved_event=no transfer_event=no endpoint_ready=no",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
            "marker_phase=256 marker_phase_name=usb-hid-endpoint-parse-begin "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 7
    assert gates.usb_blocker == "hid-endpoint-parse-no-reply"


def test_gate_summary_tracks_hid_endpoint_parse_miss_reasons() -> None:
    cases = {
        "usb-hid-endpoint-parse-no-interface": "hid-interface-not-found",
        "usb-hid-endpoint-parse-no-interrupt-in": "hid-interrupt-in-not-found",
        "usb-hid-endpoint-parse-malformed": "hid-config-descriptor-malformed",
        "usb-hub-scan-begin": "hub-child-scan-no-reply",
        "usb-hub-set-configuration-begin": "hub-set-configuration-no-reply",
        "usb-hub-set-configuration-doorbell-done": "hub-set-configuration-status-no-reply",
        "usb-hub-set-configuration-wait-begin": "hub-set-configuration-status-no-reply",
        "usb-hub-set-configuration-status-event": "hub-set-configuration-complete-no-reply",
        "usb-hub-set-configuration-status-event-slot-empty": "hub-set-configuration-status-event-slot-empty",
        "usb-hub-set-configuration-status-event-cycle-mismatch": "hub-set-configuration-status-event-cycle-mismatch",
        "usb-hub-set-configuration-status-event-ignored": "hub-set-configuration-status-event-ignored",
        "usb-hub-set-configuration-status-timeout": "hub-set-configuration-status-timeout",
        "usb-hub-set-configuration-failed": "hub-set-configuration-failed",
        "usb-hub-set-configuration-done": "hub-set-configuration-settle-no-reply",
        "usb-hub-descriptor-begin": "hub-descriptor-no-reply",
        "usb-hub-descriptor-doorbell-done": "hub-descriptor-transfer-no-reply",
        "usb-hub-descriptor-wait-begin": "hub-descriptor-transfer-no-reply",
        "usb-hub-descriptor-data-event": "hub-descriptor-status-no-reply",
        "usb-hub-descriptor-status-event": "hub-context-no-reply",
        "usb-hub-descriptor-failed": "hub-descriptor-transfer-failed",
        "usb-hub-descriptor-transfer-timeout": "hub-descriptor-transfer-timeout",
        "usb-hub-descriptor-status-timeout": "hub-descriptor-status-timeout",
        "usb-hub-descriptor-transfer-event-slot-empty": "hub-descriptor-transfer-event-slot-empty",
        "usb-hub-descriptor-transfer-event-cycle-mismatch": "hub-descriptor-transfer-event-cycle-mismatch",
        "usb-hub-descriptor-transfer-event-ignored": "hub-descriptor-transfer-event-ignored",
        "usb-hub-descriptor-status-event-slot-empty": "hub-descriptor-status-event-slot-empty",
        "usb-hub-descriptor-status-event-cycle-mismatch": "hub-descriptor-status-event-cycle-mismatch",
        "usb-hub-descriptor-status-event-ignored": "hub-descriptor-status-event-ignored",
        "usb-hub-descriptor-done": "hub-context-no-reply",
        "usb-hub-context-begin": "hub-context-no-reply",
        "usb-hub-context-done": "hub-port-power-no-reply",
        "usb-hub-port-power-begin": "hub-port-power-no-reply",
        "usb-hub-port-power-done": "hub-port-status-no-reply",
        "usb-hub-port-status-begin": "hub-port-status-no-reply",
        "usb-hub-port-status-doorbell-done": "hub-port-status-transfer-no-reply",
        "usb-hub-port-status-wait-begin": "hub-port-status-transfer-no-reply",
        "usb-hub-port-status-data-event": "hub-port-status-status-no-reply",
        "usb-hub-port-status-status-event": "hub-port-reset-no-reply",
        "usb-hub-port-status-transfer-timeout": "hub-port-status-transfer-timeout",
        "usb-hub-port-status-status-timeout": "hub-port-status-timeout",
        "usb-hub-port-status-transfer-event-slot-empty": "hub-port-status-transfer-event-slot-empty",
        "usb-hub-port-status-transfer-event-cycle-mismatch": "hub-port-status-transfer-event-cycle-mismatch",
        "usb-hub-port-status-transfer-event-ignored": "hub-port-status-transfer-event-ignored",
        "usb-hub-port-status-status-event-slot-empty": "hub-port-status-status-event-slot-empty",
        "usb-hub-port-status-status-event-cycle-mismatch": "hub-port-status-status-event-cycle-mismatch",
        "usb-hub-port-status-status-event-ignored": "hub-port-status-status-event-ignored",
        "usb-hub-port-status-done": "hub-port-reset-no-reply",
        "usb-hub-port-status-failed": "hub-port-status-failed",
        "usb-hub-port-reset-begin": "hub-port-reset-no-reply",
        "usb-hub-port-reset-set-begin": "hub-port-reset-set-no-reply",
        "usb-hub-port-reset-set-done": "hub-port-reset-completion-no-reply",
        "usb-hub-port-reset-set-failed": "hub-port-reset-set-failed",
        "usb-hub-port-ready": "hub-child-probe-no-reply",
        "usb-hub-child-probe-begin": "hub-child-probe-no-reply",
        "usb-hub-child-speed-fallback-begin": "hub-child-speed-fallback-no-reply",
        "usb-hub-scan-no-keyboard": "hub-topology-no-keyboard",
    }
    for phase_name, expected_blocker in cases.items():
        events = normalizer.parse_events(
            [
                "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0208 "
                "result=0x0f000001 root_port_mask=0x01 slot=2 ep_id=0 "
                "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
                "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
                "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
                "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
                f"marker_phase=274 marker_phase_name={phase_name} "
                "marker_aux0=0x55534245",
            ]
        )

        gates = normalizer.summarize_gates(events)

        assert gates.usb_gate == 7
        assert gates.usb_blocker == expected_blocker


def test_gate_summary_tracks_hid_configure_endpoint_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0211 "
            "result=0x0f000101 root_port_mask=0x01 slot=2 ep_id=1 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=yes preserved_event=no transfer_event=no endpoint_ready=no",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
            "marker_phase=259 marker_phase_name=usb-hid-configure-endpoint-begin "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 7
    assert gates.usb_blocker == "hid-configure-endpoint-no-reply"


def test_gate_summary_tracks_hid_interrupt_queue_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0211 "
            "result=0x0f000101 root_port_mask=0x01 slot=2 ep_id=1 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=yes preserved_event=no transfer_event=no endpoint_ready=yes",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
            "marker_phase=269 marker_phase_name=usb-hid-interrupt-queue-ready "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 9
    assert gates.usb_blocker == "hid-first-report"


def test_gate_summary_tracks_device_descriptor_prime_wait_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0206 "
            "result=0x0f000001 root_port_mask=0x01 slot=2 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=yes port_event=yes "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=8 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=8 "
            "marker_phase=228 marker_phase_name=usb-device-descriptor-prime-wait-begin "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 6
    assert gates.usb_blocker == "device-descriptor-prime-transfer-no-reply"


def test_gate_summary_tracks_command_event_read_done_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0203 "
            "result=0x03000000 root_port_mask=0x00 slot=0 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=no port_event=no "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no "
            "cmd_proof=yes cmd_events_seen=0 cmd_slot_or_polls=0 cmd_event_type=0 "
            "cmd_ack_failures=0",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=5 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=5 "
            "marker_phase=187 marker_phase_name=usb-command-proof-event-read-done "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "enable-slot-event-read-done-no-reply"


def test_gate_summary_tracks_command_event_read_begin_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0203 "
            "result=0x03000000 root_port_mask=0x00 slot=0 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=no port_event=no "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no "
            "cmd_proof=yes cmd_events_seen=0 cmd_slot_or_polls=0 cmd_event_type=0 "
            "cmd_ack_failures=0",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=5 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=5 "
            "marker_phase=188 marker_phase_name=usb-command-proof-event-read-begin "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "enable-slot-event-read-begin-no-reply"


def test_gate_summary_tracks_command_event_dma_load_done_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0203 "
            "result=0x03000000 root_port_mask=0x00 slot=0 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=no port_event=no "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no "
            "cmd_proof=yes cmd_events_seen=0 cmd_slot_or_polls=0 cmd_event_type=0 "
            "cmd_ack_failures=0",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=5 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=5 "
            "marker_phase=189 marker_phase_name=usb-command-proof-event-dma-load-done "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "enable-slot-event-dma-load-done-no-reply"


def test_gate_summary_tracks_command_event_invalidate_done_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0203 "
            "result=0x03000000 root_port_mask=0x00 slot=0 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=no port_event=no "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no "
            "cmd_proof=yes cmd_events_seen=0 cmd_slot_or_polls=0 cmd_event_type=0 "
            "cmd_ack_failures=0",
            "DRIVER_TASK_RING_PROGRESS contract=usb-local-seat request=5 "
            "expected_aux0=0x55534245 marker_valid=yes marker_sequence=5 "
            "marker_phase=199 marker_phase_name=usb-command-proof-event-invalidate-done "
            "marker_aux0=0x55534245",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "enable-slot-event-invalidate-done-no-reply"


def test_gate_summary_tracks_command_event_slot_empty_progress() -> None:
    events = normalizer.parse_events(
        [
            "USB_RUNTIME_ENUM_SNAPSHOT contract=usb-local-seat detail=0x0203 "
            "result=0x03000000 root_port_mask=0x00 slot=0 ep_id=0 "
            "scan_pass=0 root_port_power=yes cmd_path=no port_event=no "
            "hid_ep=no preserved_event=no transfer_event=no endpoint_ready=no "
            "cmd_proof=yes cmd_events_seen=0 cmd_slot_or_polls=0 cmd_event_type=0 "
            "cmd_ack_failures=0",
            "usb: linked_runtime_progress marker_valid=yes sequence=5 phase=184 "
            "phase_name=usb-command-proof-event-slot-empty aux0=0x55534245 "
            "gate=4 blocker=enable-slot-event-slot-empty "
            "next_action=inspect-event-ring-publication-or-controller-writeback",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "enable-slot-event-slot-empty"


def test_gate_summary_tracks_hub_set_configuration_status_event_progress() -> None:
    events = normalizer.parse_events(
        [
            "usb: linked_runtime_progress marker_valid=yes sequence=8 phase=407 "
            "phase_name=usb-hub-set-configuration-status-event-ignored "
            "aux0=0x55534245 gate=7 "
            "blocker=hub-set-configuration-status-event-ignored "
            "next_action=inspect-hub-set-configuration-status-event",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 7
    assert gates.usb_blocker == "hub-set-configuration-status-event-ignored"


def test_gate_summary_refines_hub_port_power_to_status_read() -> None:
    events = normalizer.parse_events(
        [
            "usb: linked_runtime_progress marker_valid=yes sequence=20 phase=408 "
            "phase_name=usb-hub-port-status-begin "
            "aux0=0x55534245 gate=7 "
            "blocker=hub-port-status-no-reply "
            "next_action=inspect-hub-port-status-control-transfer",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 7
    assert gates.usb_blocker == "hub-port-status-no-reply"


def test_gate_summary_tracks_usb_startup_blackbox_gates() -> None:
    events = normalizer.parse_events(
        [
            "usb: gate 1 name=hal-resources status=pass evidence=ownership_gate=1 next=pcie-vl805",
            "usb: gate 2 name=pcie-vl805 status=pass evidence=backend_attached=yes next=xhci-operational",
            "usb: gate 3 name=xhci-operational status=pass evidence=route_progress=controller-ready next=command-event-rings",
            "usb: gate 4 name=command-event-rings status=pass evidence=queued_reports=4 next=root-port-connected",
            "usb: gate 5 name=root-port-connected status=pass evidence=connected_mask=0x0002 next=device-addressed",
            "usb: gate 6 name=device-addressed status=pass evidence=linked_detail=0x0206 next=config-and-hid-descriptors",
            "usb: gate 7 name=config-and-hid-descriptors status=pass evidence=linked_detail=0x0211 next=keyboard-ready",
            "usb: gate 8 name=keyboard-ready status=pass evidence=runtime=yes next=first-hid-report",
            "usb: gate 9 name=first-hid-report status=fail evidence=first_report=no next=first-console-byte",
            "usb: next_action=inspect-xhci-event-ring-interrupt-delivery blocker=hid-first-report proof_gate=8",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 8
    assert gates.usb_blocker == "hid-first-report"


def test_gate_summary_preserves_usb_startup_gate_failure_over_driver_noise() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-engine-init status=begin "
            "acceptance=no code=none detail=none result=none frame_len=0",
            "DRIVER_TASK_RING_CALL_TIMEOUT contract=usb-local-seat "
            "endpoint=0x07a4 request=2 mode=nonblocking attempts=4096 "
            "opcode=1 arg0=2 aux0=0x4c53494e frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-xhci-init "
            "status=blocked-pcie-runtime acceptance=no code=1 detail=0 "
            "result=0 frame_len=0",
            "usb: gate 1 name=hal-resources status=pass "
            "evidence=ownership_gate=1 next=pcie-vl805",
            "usb: gate 2 name=pcie-vl805 status=fail "
            "evidence=backend_attached=no linked_controller=no "
            "runtime_result=0x00000000 next=xhci-operational",
            "usb: gate 3 name=xhci-operational status=blocked "
            "evidence=waiting-on-gate-2 next=command-event-rings",
            "usb: evidence boundary console_client=event-pump "
            "hal=admission-descriptor-diagnostics-only "
            "linked_runtime_owner=usb-local-seat "
            "failure_domain=linked-runtime-command-not-observed "
            "proof_gate=1 target_gate=10 proof_effect=acceptance-red",
            "usb: next_action=inspect-linked-usb-runtime-progress "
            "blocker=linked-runtime-command-not-observed proof_gate=1 "
            "target_gate=10 detail=0x0000 result=0x00000000 source=linked-runtime",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_GATE"] == 1
    assert record["USB_BLOCKER"] == "pcie-vl805"
    assert record["USB_DRIVER_TASK_FRONTIER"] == "usb-xhci-init-blocked-pcie-runtime"


def test_gate_summary_caps_later_usb_progress_after_startup_gate_failure() -> None:
    events = normalizer.parse_events(
        [
            "usb: gate 1 name=hal-resources status=pass "
            "evidence=ownership_gate=1 next=pcie-vl805",
            "usb: gate 2 name=pcie-vl805 status=fail "
            "evidence=backend_attached=no linked_controller=no "
            "runtime_result=0x00000000 next=xhci-operational",
            "USB: stage=controller-ready",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_GATE"] == 1
    assert record["USB_BLOCKER"] == "pcie-vl805"


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
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane "
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
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane "
            "step=apsta-enable action=fail err=unsupported operation: "
            "cyw43-control-plane-hintless-firstread-no-irq",
            "[WARN root_task::drivers::driver_task_net] [cyw43] init failure "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane step=join "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::driver_task_net] [cyw43] join: firmware-supplicant "
            "path=primary-plain unsupported status=0xffffffe9 action=try-bsscfg-wrapper "
            "reason=known-good-cyw43-fwsup-shape",
            "[WARN root_task::drivers::driver_task_net] [cyw43] join: firmware-supplicant "
            "path=bsscfg-wrapper unsupported status=0xffffffe9 "
            "action=continue-host-eapol-required reason=firmware-offload-unavailable",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane reply "
            "cmd=0x0000010c id=26 status=0xfffffffe response_len=0 copied=0",
            "[WARN root_task::drivers::driver_task_net] [cyw43] join: linux-pmk rejected "
            "action=retry-legacy-hex-pmk status=0xfffffffe",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane reply "
            "cmd=0x0000010c id=27 status=0xfffffffe response_len=0 copied=0",
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane step=join "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane step=join action=begin",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane reply "
            "cmd=0x00000107 id=31 status=0xffffffe9 response_len=0 copied=0",
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane step=join "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::driver_task_net] [cyw43] join: firmware-supplicant "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::driver_task_net] [cyw43] join: firmware-supplicant "
            "path=primary-plain unsupported status=0xffffffe9 action=try-bsscfg-wrapper "
            "reason=known-good-cyw43-fwsup-shape",
            "[WARN root_task::drivers::driver_task_net] [cyw43] join: firmware-supplicant "
            "path=bsscfg-wrapper unsupported status=0xffffffe9 action=fail-secure "
            "reason=host-eapol-supplicant-required",
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane step=join "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::driver_task_net] [cyw43] join: firmware-supplicant "
            "path=bsscfg-wrapper unsupported status=0xffffffe9 "
            "action=continue-host-eapol-required reason=firmware-offload-unavailable",
            "[INFO root_task::drivers::driver_task_net] [cyw43] join pending mode=deferred "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::driver_task_net] [cyw43] join: firmware-supplicant "
            "path=bsscfg-wrapper unsupported status=0xffffffe9 "
            "action=continue-host-eapol-required reason=firmware-offload-unavailable",
            "[INFO root_task::drivers::driver_task_net] [cyw43] join request "
            "path=primary-bsscfg:join action=ready ssid_len=12",
            "[WARN root_task::drivers::driver_task_net] [cyw43] join failed "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane step=join action=begin",
            "[WARN root_task::drivers::driver_task_net] [cyw43] join: firmware-supplicant "
            "path=bsscfg-wrapper unsupported status=0xffffffe9 "
            "action=continue-host-eapol-required reason=firmware-offload-unavailable",
            "[INFO root_task::drivers::driver_task_net] [cyw43] host-eapol proof window "
            "armed mode=join-submit polls=4608 pre_assoc_polls=512 "
            "post_assoc_polls=4096 rx_poll=eapol-only dhcp=blocked tx=blocked "
            "ssid_len=12 psk_len=12",
            "[WARN root_task::drivers::driver_task_net] [cyw43] host-eapol proof count=1 "
            "msg=m1 len=117 eapol_ver=2 type=3 body_len=99 body_ok=yes "
            "key_desc=2 key_info=0x008a key_ver=2 pairwise=yes ack=yes "
            "mic=no install=no secure=no encrypted=no nonce=yes replay=yes "
            "kde_len=0 next_action=derive-ptk-send-m2 action=drop "
            "status=host-eapol-required",
            "[INFO root_task::drivers::driver_task_net] [cyw43] host-eapol proof window "
            "result=eapol-seen mode=join-submit polls=92 assoc=link-up "
            "assoc_poll=88 post_assoc_polls=4 eapol_rx_delta=1 eapol_rx_total=1 "
            "events=2 control=0 empty_polls=89 action=continue-eapol-only-rx",
            "[INFO root_task::drivers::driver_task_net] [cyw43] host-eapol pending "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] host-eapol proof window "
            "result=not-yet-seen mode=join-submit polls=16384 assoc=link-up "
            "eapol_rx_delta=0 eapol_rx_total=0 action=defer-eapol-only-rx",
            "[INFO root_task::drivers::driver_task_net] [cyw43] host-eapol pending "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] join failed "
            "reason=host-eapol-required rx_poll=eapol-only dhcp=blocked tx=blocked "
            "mode=join-submit ssid_len=12 psk_len=12",
            "[INFO root_task::drivers::driver_task_net] [cyw43] host-eapol "
            "action=eapol-start mode=deferred polls=2048 limit=60000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "host-eapol-required"
    assert gates.wifi_exact == "host-eapol-required"


def test_gate_summary_tracks_structured_host_eapol_required_status() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=host-eapol-required polls=24576 starts=6 "
            "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 control_rx=0 "
            "empty_polls=24576 last_flags=0x0000 last_len=0 "
            "last_ethertype=0x0000 last_ethertype_valid=no "
            "next_action=inspect-cyw43-data-rx-path",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "host-eapol-required"
    assert gates.wifi_exact == "host-eapol-required"
    assert gates.wifi_phase == "join-security"


def test_gate_summary_refines_host_eapol_firstread_empty() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=host-eapol-required polls=24576 starts=6 "
            "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 control_rx=0 "
            "empty_polls=24576 rx_firstread_attempts=6 rx_firstread_empty=6 "
            "rx_firstread_invalid=0 rx_firstread_failed=0 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "last_rx_idle_detail=0x570a last_rx_idle_result=0x00000000 "
            "last_flags=0x0000 last_len=0 last_ethertype=0x0000 "
            "last_ethertype_valid=no next_action=inspect-ap-m1-or-cyw43-rx-latch",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-firstread-empty"
    assert gates.wifi_exact == "cyw43-data-rx-firstread-empty"
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_preserves_host_eapol_firstread_empty_with_rx_source() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=host-eapol-required polls=24576 starts=0 "
            "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 event_rx=0 "
            "control_rx=0 empty_polls=24576 associated=no link_up=no "
            "assoc_event=none assoc_poll=0 post_assoc_polls=0 "
            "rx_firstread_attempts=24576 rx_firstread_empty=24576 "
            "rx_firstread_invalid=0 rx_firstread_failed=0 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "control_rx_firstread_attempts=24576 control_rx_firstread_empty=24576 "
            "control_rx_firstread_failed=0 last_rx_idle_detail=0x570a "
            "last_rx_idle_result=0x80000200 last_control_rx_idle_detail=0x570a "
            "last_control_rx_idle_result=0x80000040 rxsrc_probe_len=512 "
            "rxsrc_ien=0x07 rxsrc_frame_ind=no rxsrc_host_int=no "
            "rxsrc_card_int=no rxsrc_f2_ready=yes control_rxsrc_probe_len=64 "
            "control_rxsrc_ien=0x07 control_rxsrc_frame_ind=no "
            "control_rxsrc_host_int=no control_rxsrc_card_int=no "
            "control_rxsrc_f2_ready=yes last_flags=0x0000 last_len=0 "
            "last_ethertype=0x0000 last_ethertype_valid=no "
            "next_action=inspect-cyw43-assoc-event-rx-or-ienx",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-firstread-empty"
    assert gates.wifi_exact == "cyw43-data-rx-firstread-empty"
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_preserves_host_eapol_firstread_over_nettest_symptom() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-host-eapol status=pending acceptance=no code=none "
            "detail=none result=none frame_len=0",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 status=pending "
            "reason=none polls=0 starts=0 tx_retries=0 data_rx=0 eapol_rx=0 "
            "non_eapol_rx=0 control_rx=0 empty_polls=0 rx_firstread_attempts=0 "
            "rx_firstread_empty=0 rx_firstread_invalid=0 rx_firstread_failed=0 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "last_rx_idle_detail=0x0000 last_rx_idle_result=0x00000000 "
            "last_flags=0x0000 last_len=0 last_ethertype=0x0000 "
            "last_ethertype_valid=no next_action=inspect-cyw43-data-rx-path",
            "CYW43_DRIVER_TASK_HOST_EAPOL_TX contract=cyw43455 "
            "stage=cyw43-host-eapol-start poll=1024 len=18 "
            "dst=01:80:c2:00:00:03 src=88:a2:9e:66:59:10 "
            "ethertype=0x888e bdc_priority=6",
            "CYW43_DRIVER_TASK_HOST_EAPOL_TX contract=cyw43455 "
            "stage=cyw43-host-eapol-start poll=4096 len=18 "
            "dst=01:80:c2:00:00:03 src=88:a2:9e:66:59:10 "
            "ethertype=0x888e bdc_priority=6",
            "CYW43_DRIVER_TASK_HOST_EAPOL_TX contract=cyw43455 "
            "stage=cyw43-host-eapol-start poll=8192 len=18 "
            "dst=01:80:c2:00:00:03 src=88:a2:9e:66:59:10 "
            "ethertype=0x888e bdc_priority=6",
            "CYW43_DRIVER_TASK_HOST_EAPOL_TX contract=cyw43455 "
            "stage=cyw43-host-eapol-start poll=12288 len=18 "
            "dst=01:80:c2:00:00:03 src=88:a2:9e:66:59:10 "
            "ethertype=0x888e bdc_priority=6",
            "CYW43_DRIVER_TASK_HOST_EAPOL_TX contract=cyw43455 "
            "stage=cyw43-host-eapol-start poll=16384 len=18 "
            "dst=01:80:c2:00:00:03 src=88:a2:9e:66:59:10 "
            "ethertype=0x888e bdc_priority=6",
            "CYW43_DRIVER_TASK_HOST_EAPOL_TX contract=cyw43455 "
            "stage=cyw43-host-eapol-start poll=20480 len=18 "
            "dst=01:80:c2:00:00:03 src=88:a2:9e:66:59:10 "
            "ethertype=0x888e bdc_priority=6",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=host-eapol-required polls=24576 starts=6 "
            "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 control_rx=0 "
            "empty_polls=24576 rx_firstread_attempts=11 rx_firstread_empty=11 "
            "rx_firstread_invalid=0 rx_firstread_failed=0 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "last_rx_idle_detail=0x570a last_rx_idle_result=0x00000000 "
            "last_flags=0x0000 last_len=0 last_ethertype=0x0000 "
            "last_ethertype_valid=no next_action=inspect-ap-m1-or-cyw43-rx-latch",
            "wifi: gate 8 name=firmware-channel status=fail "
            "evidence=exact=host-eapol-required",
            "wifi: evidence failure_domain=host-eapol-required direct_proof_gate=7 "
            "proof_gate=7 frontier_gate=7",
            "ERR NETTEST net-disabled cause=host-eapol-required",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-firstread-empty"
    assert gates.wifi_exact == "cyw43-data-rx-firstread-empty"
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_refines_host_eapol_firstread_invalid() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=host-eapol-required polls=24576 starts=6 "
            "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 control_rx=0 "
            "empty_polls=24576 rx_firstread_attempts=1 rx_firstread_empty=0 "
            "rx_firstread_invalid=1 rx_firstread_failed=0 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "last_rx_idle_detail=0x570b last_rx_idle_result=0x34120000 "
            "last_flags=0x0000 last_len=0 last_ethertype=0x0000 "
            "last_ethertype_valid=no next_action=inspect-cyw43-data-rx-firstread-prefix",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-firstread-invalid-sdpcm"
    assert gates.wifi_exact == "cyw43-data-rx-firstread-invalid-sdpcm"
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_preserves_host_eapol_after_ready_and_panic() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] sdio irq bind irq=158 trigger=Level handler=3225 "
            "notification=3226 badge=159",
            "[INFO root_task::drivers::driver_task_net] [cyw43] join pending mode=deferred "
            "polls=0 ssid_len=12 psk_len=12 secure=yes fwsup=no "
            "completion_rule=host-eapol-required",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane step=join action=ready",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane step=init-complete action=ready",
            "[INFO root_task::drivers::driver_task_net] [cyw43] ready: mac=88-a2-9e-66-59-10 "
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
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane "
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
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane "
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


def test_gate_summary_refines_cyw43_control_exchange_timeout_result() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-control-txglomalign op=11 flags=0x0000 "
            "target=0x00000000 payload_off=284 payload_len=36 total_len=36 "
            "detail=21259 reason=cyw43-control-exchange result=0x43030000",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=fault acceptance=no "
            "code=5 detail=21259 result=0x43030000 frame_len=0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-reply-idle-loop"
    assert gates.wifi_exact == "cyw43-control-rx-no-rframe"
    assert gates.wifi_phase == "cyw43-control-txglomalign"


def test_gate_summary_refines_cyw43_control_exchange_no_reply_progress() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "CYW43_DRIVER_TASK_COMMAND_NO_REPLY contract=cyw43455 "
            "stage=cyw43-control-exchange op=11 flags=0x0000 "
            "target=0x00000000 payload_off=284 payload_len=36 total_len=36 "
            "reason=cyw43-runtime-command-no-reply request=122962 resumes=63 "
            "progress_marker_valid=yes progress_sequence=122962 "
            "progress_phase=144 progress_phase_name=cyw43-sdio-owner-reply "
            "progress_aux0=0x43595734",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=no-reply acceptance=no "
            "code=none detail=none result=none frame_len=0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-reply-idle-loop"
    assert gates.wifi_exact == "cyw43-sdio-owner-reply"
    assert gates.wifi_phase == "cyw43-sdio-owner-reply"


def test_gate_summary_refines_cyw43_control_exchange_firstread_empty() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-control-txglomalign op=11 flags=0x0000 "
            "target=0x00000000 payload_off=284 payload_len=36 total_len=36 "
            "detail=21259 reason=cyw43-control-exchange result=0x430a0000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-reply-idle-loop"
    assert gates.wifi_exact == "cyw43-control-rx-firstread-empty"
    assert gates.wifi_phase == "cyw43-control-txglomalign"


def test_gate_summary_refines_cyw43_split_control_firstread_empty() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-control-txglomalign event=cyw43-control-split-no-reply "
            "poll=0 flags=0x0000 code=3 detail=0x570a result=0xd7000000 "
            "frame_off=0 frame_len=0 frame_flags=0x0000",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=poll-timeout acceptance=no "
            "code=5 detail=21259 result=0x430a0000 frame_len=0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-reply-idle-loop"
    assert gates.wifi_exact == "cyw43-control-rx-firstread-empty"
    assert gates.wifi_phase == "cyw43-control-txglomalign"


def test_gate_summary_ignores_transient_cyw43_split_control_idle_sample() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-control-txglomalign event=poll-complete "
            "poll=1 flags=0x0000 sequence=19 code=3 detail=0x570a "
            "result=0xd7000000 frame_off=0 frame_len=0 frame_flags=0x0000 "
            "expected_cmd=263 expected_cmd_hex=0x00000107 expected_id=9 "
            "header_mode=plain expected_response_len=0 iovar=txglomalign "
            "nonmatching_frames=0 malformed_frames=0",
            "CYW43_DRIVER_TASK_CONTROL_REPLY contract=cyw43455 "
            "stage=cyw43-control-txglomalign event=matched-reply poll=2 "
            "flags=0x0000 completion_sequence=20 cmd=263 cmd_hex=0x00000107 "
            "id=9 status=0x00000000 response_len=0 payload_available=0 "
            "expected_cmd=263 expected_cmd_hex=0x00000107 expected_id=9 "
            "header_mode=plain expected_response_len=0 iovar=txglomalign "
            "reply_match=yes nonmatching_frames=0 malformed_frames=0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-reply-idle-loop"
    assert gates.wifi_exact != "cyw43-control-rx-firstread-empty"


def test_gate_summary_refines_cyw43_split_control_terminal_nonmatching_reply() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "CYW43_DRIVER_TASK_CONTROL_REPLY contract=cyw43455 "
            "stage=cyw43-control-txglomalign event=nonmatching-reply poll=2 "
            "flags=0x0000 completion_sequence=20 cmd=262 cmd_hex=0x00000106 "
            "id=8 status=0x00000000 response_len=0 payload_available=0 "
            "expected_cmd=263 expected_cmd_hex=0x00000107 expected_id=9 "
            "header_mode=plain expected_response_len=0 iovar=txglomalign "
            "reply_match=no nonmatching_frames=1 malformed_frames=0",
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-control-txglomalign "
            "event=cyw43-control-reply-nonmatching poll=0 flags=0x0000 "
            "sequence=20 code=3 detail=0x5703 result=0xd7000000 "
            "frame_off=0 frame_len=0 frame_flags=0x0000 "
            "expected_cmd=263 expected_cmd_hex=0x00000107 expected_id=9 "
            "header_mode=plain expected_response_len=0 iovar=txglomalign "
            "nonmatching_frames=1 malformed_frames=0",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-control-txglomalign op=11 flags=0x0000 "
            "target=0x00000000 payload_off=284 payload_len=36 total_len=36 "
            "detail=21259 reason=cyw43-control-exchange result=0x43080001",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-reply-idle-loop"
    assert gates.wifi_exact == "cyw43-control-reply-nonmatching"
    assert gates.wifi_phase == "cyw43-control-txglomalign"


def test_gate_summary_refines_cyw43_split_control_tx_not_submitted() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-control-txglomalign "
            "event=cyw43-control-tx-not-submitted poll=0 flags=0x0000 "
            "code=3 detail=0x0000 result=0x00000000 "
            "frame_off=0 frame_len=0 frame_flags=0x0000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-reply-idle-loop"
    assert gates.wifi_exact == "cyw43-control-tx-not-submitted"
    assert gates.wifi_phase == "cyw43-control-txglomalign"


def test_gate_summary_refines_cyw43_revinfo_badarg_status() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-mac status=ready acceptance=no "
            "code=2 detail=0 result=20 frame_len=20",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-revinfo status=begin acceptance=no "
            "code=none detail=none result=none frame_len=0",
            "DRIVER_TASK_RING_CALL_RETURN contract=cyw43455 endpoint=0x1984 "
            "request=178 sequence=178 code=5 detail=21259 result=4294967294",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-control-exchange op=11 flags=0x0002 "
            "target=0x00000000 payload_off=284 payload_len=16 total_len=16 "
            "detail=21259 reason=cyw43-control-exchange result=0xfffffffe",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-exchange status=fault acceptance=no "
            "code=5 detail=21259 result=0xfffffffe frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-plane status=failed acceptance=no "
            "code=none detail=none result=none frame_len=0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-revinfo-badarg"
    assert gates.wifi_exact == "cyw43-control-revinfo-badarg"
    assert gates.wifi_phase == "cyw43-control-revinfo"


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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "preinit step=mpc action=begin",
            "[WARN root_task::drivers::driver_task_net] [cyw43] iovar set failed "
            "name=mpc err=unsupported operation: "
            "cyw43-control-plane-host-card-int-no-dongle-source",
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "step=bus-rxglom-disable-bounded-rx action=begin",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane reply "
            "cmd=0x00000107 id=10 status=0x00000000 response_len=15 "
            "copied=15 sdpcm_seq=10 sdpcm_credit=30",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "step=bus-rxglom-disable-bounded-rx action=ready",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "preinit step=mpc action=begin",
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane "
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
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "reply cmd=0x00000107 id=6 status=0x00000000 response_len=1420",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "step=up action=ready",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "step=gmode action=skip optional=yes "
            "reason=linux-station-path-does-not-set-legacy-gmode",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "reply cmd=0x00000002 id=18 status=0x00000000 response_len=0",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "step=up action=ready",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
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
            "[WARN root_task::drivers::driver_task_net] [cyw43] iovar set failed "
            "name=bsscfg:wsec err=cyw43 protocol error: ioctl-timeout",
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "step=join action=begin",
            "[INFO root_task::drivers::driver_task_net] [cyw43] iovar set begin "
            "name=bsscfg:sup_wpa len=8",
            "[WARN root_task::drivers::driver_task_net] [cyw43] iovar set failed "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "reply cmd=0x00000002 id=18 status=0x00000000 response_len=0",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "step=up action=ready",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
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
            "[WARN root_task::drivers::driver_task_net] [cyw43] iovar set failed "
            "name=wsec err=cyw43 protocol error: ioctl-timeout",
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "step=up action=ready",
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "step=join action=begin",
            "[INFO root_task::drivers::driver_task_net] [cyw43] join pending "
            "mode=deferred polls=0 ssid_len=12 psk_len=12 secure=yes "
            "completion_rule=host-eapol-required",
            "[INFO root_task::drivers::driver_task_net] [cyw43] host-eapol "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "step=join action=begin",
            "[INFO root_task::drivers::driver_task_net] [cyw43] iovar set begin "
            "name=wpa_auth len=4",
            "[pi4-wifi] firmware stage=control-plane-write "
            "action=linux-f2-write-shape frame_len=52",
            "[INFO root_task::drivers::driver_task_net] [cyw43] event type=54 "
            "flags=0x0000 status=0x00000000 reason=0x00000000 auth=0x00000000",
            *[
                "[pi4-wifi] firmware stage=control-plane-reply "
                "action=sdio-irq-host-latch-cleared irq=158 badge=0x9f "
                "source=0x00000000 source_readable=y card_int=y progress=no"
                for _ in range(8)
            ],
            "[WARN root_task::drivers::driver_task_net] [cyw43] ioctl "
            "no-progress-after-frame cmd=0x00000107 id=19 no_progress_polls=128 "
            "nonmatching_frames=1 cached_exact_error= action=fail-fast",
            "[WARN root_task::drivers::driver_task_net] [cyw43] iovar set failed "
            "name=wpa_auth err=cyw43 protocol error: ioctl-no-progress-after-frame",
            "[WARN root_task::drivers::driver_task_net] [cyw43] control-plane "
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
            "[INFO root_task::drivers::driver_task_net] [cyw43] control-plane "
            "step=join action=begin",
            "[INFO root_task::drivers::driver_task_net] [cyw43] iovar set begin "
            "name=wpa_auth len=4",
            "[INFO root_task::drivers::driver_task_net] [cyw43] iovar set ready "
            "name=wpa_auth len=4",
            "[INFO root_task::drivers::driver_task_net] [cyw43] iovar set begin "
            "name=wpa_auth len=4",
            "[WARN root_task::drivers::driver_task_net] [cyw43] ioctl "
            "no-progress-after-frame cmd=0x00000107 id=24 no_progress_polls=128 "
            "nonmatching_frames=1 cached_exact_error= action=fail-fast",
            "[WARN root_task::drivers::driver_task_net] [cyw43] iovar set failed "
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
