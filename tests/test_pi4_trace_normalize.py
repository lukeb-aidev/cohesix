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

    assert [event.domain for event in events] == ["wifi", "usb", "wifi"]
    assert events[1].stage == "0x0230"
    assert events[1].fields["tag"] == "reset-write"


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
        "WIFI_EXACT": "ht-clock-timeout",
        "WIFI_PHASE": "cyw43-load-firmware-fail",
        "WIFI_BLOCKER_LINE": 8,
        "SERIAL_CLEAN": "yes",
        "BOOT_HALTED": "no",
        "TIMER_IRQ27_SEEN": "no",
        "BOOT_HALT_REASON": "none",
        "USB_BOOTLOADER_HANDOFF_SEEN": "no",
    }


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


def test_usb_proof_summary_advances_command_gate() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb proof_summary gate=3 blocker=cmd-poll-pending "
            "controller=ready command=no-op-unproven event=missing irq27=timer-only",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-poll-pending"


def test_parse_fields_preserves_unsupported_operation_detail() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] firmware core-ctrl access stage=prereset-zero-ioctrl "
            "err=unsupported operation: sdio-cmd53-r5-error",
        ]
    )

    assert events[0].fields["err"] == "sdio-cmd53-r5-error"


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

    assert gates.usb_gate == 3
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
    assert gates.boot_halt_reason == "timer-irq27-without-halt"


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
    assert gates.to_record()["BOOT_HALT_REASON"] == "kernel-halt+timer-irq27"


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


def test_gate_summary_treats_usb_prompt_safe_return_as_poll_pending() -> None:
    events = normalizer.parse_events(
        [
            "usb: ownership_contract cfg_window=mapped cfg_source=runtime-mapped",
            "[local-seat] xhci.diag stage=0x0379 "
            "tag=cmd-prompt-safe-return-to-shell "
            "a=0x0000000404024000 b=0 c=256",
            "[local-seat] xhci root-port command-probe "
            "result=no-op-unproven bus=pcie-window "
            "action=return-to-shell detail=poll-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-poll-pending"


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
            "expected_erdp=0x0000000404025008",
            "Kernel entry via Interrupt, irq 27",
            "wifi: contract current=wait-ht-clock expected=chipclkcsr-ht-avail",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-live-timeout-snapshot-missing"


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
            "event_candidate_mask=0x0000 verb=no-op bus=pcie-window",
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
            "waited=0x0000000000000040 expected_ptr=0x0000000404024000 "
            "event_syncs=0x0000000000000001",
            "[local-seat] xhci root-port command-probe result=no-op-timeout "
            "bus=pcie-window action=retry-raw-phys detail=Timeout",
            "[local-seat] xhci probe fallback mmio=0x0000000600000000 "
            "from_bus=pcie-window to_bus=phys reason=no-op-timeout",
            "[local-seat] xhci root-port command-probe begin "
            "event_candidate_mask=0x0000 verb=no-op bus=phys",
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030f tag=cmd-doorbell-write "
            "doorbell=0x000000000100 target=0x0",
            "[local-seat] xhci.diag stage=0x031f tag=cmd-doorbell-post-barrier "
            "doorbell=0x000000000100 target=0x0",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-poll-pending"


def test_gate_summary_tracks_usb_pcie_window_no_op_timeout() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci root-port command-probe begin "
            "event_candidate_mask=0x0000 verb=no-op bus=pcie-window",
            "[local-seat] xhci.diag stage=0x0300 tag=cmd-submit "
            "param=0x0000000000000000",
            "[local-seat] xhci.diag stage=0x030b tag=cmd-poll-only-timeout "
            "waited=0x0000000000000040 expected_ptr=0x0000000404024000 "
            "event_syncs=0x0000000000000001",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "pcie-window-no-op-timeout"


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
    assert gates.usb_blocker == "root-port-read-begin"


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


def test_gate_summary_promotes_deferred_capture_after_command_proof() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci.diag stage=0x0110 tag=controller-init-complete",
            "[local-seat] xhci root-port sample skipped "
            "reason=platform-reset-portsc-toxic",
            "[local-seat] xhci root-port command-probe result=no-op-ok",
            "[local-seat] xhci root-port deferred-capture "
            "mask=0x0001 source=pi4-linux-capture command_probe=no-op-ok",
            "[local-seat] usb root-enum deferred-port "
            "port=1 speed=3 source=pi4-linux-capture reset=skip",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 5
    assert gates.usb_blocker == "address-device-pending"


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
    assert gates.to_record()["BOOT_HALT_REASON"] == "kernel-halt+timer-irq27"


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
            "irq27_role=timer-only pcie_irqs=175,180",
            "Kernel entry via Interrupt, irq 27",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-event-ring-timeout"


def test_gate_summary_preserves_enable_slot_event_timeout_without_summary() -> None:
    for result in ("enable-slot-unproven", "enable-slot-linux-event-unproven"):
        events = normalizer.parse_events(
            [
                "[local-seat] xhci root-port command-probe "
                f"result={result} bus=pcie-window "
                "action=return-to-shell detail=cmd-event-ring-timeout "
                "irq27_role=timer-only pcie_irqs=175,180",
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
            "event_generation=poll-only",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "none"


def test_gate_summary_rejects_legacy_linux_event_generation_as_command_proof() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] xhci root-port command-probe "
            "result=enable-slot-linux-event-ok-cleanup-failed "
            "bus=pcie-window slot=1 cleanup=disable-slot-timeout "
            "event_generation=linux-shaped-bounded",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate < 4
    assert gates.usb_blocker != "none"


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
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 3
    assert gates.usb_blocker == "cmd-controller-not-running"


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
        "pcie-window-no-op-timeout": "pcie-window-no-op-timeout",
        "pcie-irq-quiesce-failed": "pcie-irq-quiesce-failed",
        "pcie-irq-quiesce-missing": "pcie-irq-quiesce-missing",
        "raw-phys-cmd-poll-only-timeout": "raw-phys-cmd-poll-only-timeout",
        "pcie-config-replay": "pcie-config-replay",
        "brcm-axi-setup-read": "brcm-axi-setup-read",
        "xhci.diag stage=0x0111": "brcm-axi-setup-read",
        "reset-pre-usbcmd-source-timer-preempted": "reset-pre-usbcmd-source",
        "xhci.diag stage=0x0226": "reset-pre-usbcmd-source",
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
        "sdio cmd53 r5 fail arg=0x95700004": (
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
            "[local-seat] xhci root-port command-probe result=no-op-ok",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 4
    assert gates.usb_blocker == "none"


def test_gate_summary_keeps_usb_post_command_outcome_blocker() -> None:
    events = normalizer.parse_events(
        [
            "usb: golden_path outcome=root-port-sample-deferred "
            "progress=controller-ready command_probe=no-op-ok",
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
            "[pi4-wifi] sdio function-ready fn=2 ioex=0x06 iordy=0x06",
            "wifi: f2_gate policy=post-ht-proof f2_enabled=yes f2_ready=yes",
            "wifi: snapshot source=live stage=post-firmware-ready-function2-strict-repoll-fail "
            "exact=firmware-channel-f2",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 6
    assert gates.wifi_blocker == "firmware-channel-f2"


def test_gate_summary_tracks_wifi_control_plane_breadcrumb_failures() -> None:
    events = normalizer.parse_events(
        [
            "wifi: ht_state chipclk=0x52 ht_req=yes ht_avail=yes",
            "[pi4-wifi] sdio function-ready fn=2 ioex=0x06 iordy=0x06",
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
            "[pi4-wifi] sdio function-ready fn=2 ioex=0x06 iordy=0x06",
            "[cyw43] control-plane step=event-mask action=fail err=ioctl-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "ioctl-timeout"


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


def test_gate_summary_tracks_wifi_join_and_dhcp_gates() -> None:
    events = normalizer.parse_events(
        [
            "[cyw43] ready: mac=02:43:4f:48:58:55 clock=41666666Hz "
            "bus_width=4bit ioex=0x06",
            "[cyw43] join complete mode=deferred polls=3",
            "[net] not-ready gate tripped: want=net-selftest reason=dhcp-pending",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 8
    assert gates.wifi_blocker == "dhcp-pending"


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


def test_gate_summary_tracks_wifi_dhcp_failure_evidence() -> None:
    events = normalizer.parse_events(
        [
            "[cyw43] join complete mode=deferred polls=3",
            "[dhcp] failed reason=discover-timeout",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 8
    assert gates.wifi_blocker == "dhcp-failed"


def test_gate_summary_tracks_wifi_dhcp_transition_evidence() -> None:
    events = normalizer.parse_events(
        [
            "[cyw43] join complete mode=deferred polls=3",
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
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 10
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
