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

    assert [event.domain for event in events] == ["usb", "wifi"]
    assert events[0].stage == "0x0230"
    assert events[0].fields["tag"] == "reset-write"


def test_nettest_policy_error_is_wifi_terminal_evidence() -> None:
    event = normalizer.parse_line(
        "ERR NETTEST reason=policy detail=net-disabled "
        "cause=cyw43-armcr4-release-readback-unavailable",
        23,
    )

    assert event is not None
    assert event.domain == "wifi"
    assert event.fields["cause"] == "cyw43-armcr4-release-readback-unavailable"


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
    }


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


def test_gate_summary_tracks_usb_command_doorbell_timer_halt() -> None:
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
    assert gates.usb_blocker == "cmd-doorbell-timer-halt"


def test_gate_summary_treats_usb_doorbell_write_edges_as_timer_halt() -> None:
    for tag in ("cmd-doorbell-write", "cmd-doorbell-write-done"):
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
        assert gates.usb_blocker == "cmd-doorbell-timer-halt"


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
    assert gates.usb_blocker == "cmd-pre-doorbell-timer-halt"


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
            "action=diagnostic-ht-timeout-backplane-live addr=0x00258000 "
            "bytes=24 mode=cmd52-windowed chipclk=0x52 "
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


def test_gate_summary_tracks_wifi_ht_backplane_cmd52_unreadable() -> None:
    events = normalizer.parse_events(
        [
            "wifi: firmware_release fw=609309 rstvec=0xb83ef198 armcr4_release=1",
            "wifi: ht_state chipclk=0x52 ht_req=yes ht_avail=no alp_avail=yes",
            "[pi4-wifi] firmware stage=debug-probe-ht "
            "action=diagnostic-ht-timeout-backplane-unreadable "
            "addr=0x00258000 bytes=24 mode=cmd52-windowed chipclk=0x52 "
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
