# Author: Lukas Bower
# Purpose: Regression tests for the Raspberry Pi 4 USB/WiFi gate proof shell wrapper.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4_gate_proof.sh."""

import http.server
import json
import pathlib
import subprocess
import threading

import pytest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "pi4_gate_proof.sh"


def _run_output_guard_probe(
    tmp_path: pathlib.Path,
    body: str,
    *extra_args: str,
) -> subprocess.CompletedProcess[str]:
    """Run only the shell wrapper's output-guard helpers in a probe process."""

    prefix = SCRIPT_PATH.read_text(encoding="utf-8").split(
        "\nwhile [[ $# -gt 0 ]]; do",
        1,
    )[0]
    prefix_path = tmp_path / "pi4-gate-helper-prefix.sh"
    prefix_path.write_text(prefix + "\n", encoding="utf-8")
    command = (
        'source "$1"\nPYTHON="$2"\nTMPDIR="$3"\n'
        'GATEWAY_TARGET_HOST="192.168.50.23"\nexport TMPDIR\n'
        + body
    )
    return subprocess.run(
        [
            "bash",
            "-c",
            command,
            "guard-probe",
            str(prefix_path),
            str(REPO_ROOT / ".venv" / "bin" / "python"),
            str(tmp_path),
            *extra_args,
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )


def _start_gateway_status_server(
    statuses: list[dict[str, object]],
) -> tuple[
    http.server.ThreadingHTTPServer,
    threading.Thread,
    str,
    list[tuple[str, str | None, str | None]],
]:
    """Serve bounded sequential gateway status responses on loopback."""

    pending = [
        {
            "target_host": "192.168.50.23",
            "target_port": 31337,
            **status,
        }
        for status in statuses
    ]
    requests: list[tuple[str, str | None, str | None]] = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802 - stdlib handler contract
            requests.append(
                (
                    self.path,
                    self.headers.get("Authorization"),
                    self.headers.get("x-cohesix-auth"),
                )
            )
            if not pending:
                self.send_error(500, "no status fixture remains")
                return
            body = json.dumps(pending.pop(0), separators=(",", ":")).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, _format: str, *args: object) -> None:
            del args

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    host, port = server.server_address
    return server, thread, f"http://{host}:{port}", requests


def _is_wifi_dpc_proof_line(line: str) -> bool:
    """Return whether a line belongs to the exact DPC proof triplet."""

    return line.startswith(
        (
            "CYW43_SDIO_DPC ",
            "CYW43_SDIO_DPC_SCOPE ",
            "CYW43_SDIO_DPC_TRUTH ",
        )
    )


def _driver_task_owner_state_lines() -> list[str]:
    return [
        "DRIVER_TASK_OWNER_STATE contract=serial hot_path=serial-console "
        "owner_state=driver-owned descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=none root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=usb-local-seat hot_path=usb-keyboard "
        "owner_state=driver-owned descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=valid root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=hdmi-text hot_path=hdmi-text "
        "owner_state=driver-owned descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=none root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=bcmgenet-v5 hot_path=genet-nic "
        "owner_state=driver-owned descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=none root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=cyw43455 hot_path=cyw43-wifi "
        "owner_state=driver-owned descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=valid root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=sdio-host hot_path=sdio-host "
        "owner_state=driver-owned descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=valid root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=pcie-root hot_path=pcie-root "
        "owner_state=driver-owned descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=none root_pointer=no",
    ]


def _driver_task_boot_affinity_lines() -> list[str]:
    return [
        "DRIVER_TASK_BOOT contract=serial role=serial started=yes affinity_core=1",
        "DRIVER_TASK_BOOT contract=usb-local-seat role=usb started=yes affinity_core=1",
        "DRIVER_TASK_BOOT contract=hdmi-text role=display started=yes affinity_core=2",
        "DRIVER_TASK_BOOT contract=pcie-root role=pcie started=yes affinity_core=2",
        "DRIVER_TASK_BOOT contract=sdio-host role=sdio started=yes affinity_core=3",
        "DRIVER_TASK_BOOT contract=bcmgenet-v5 role=net started=yes affinity_core=3",
        "DRIVER_TASK_BOOT contract=cyw43455 role=net started=yes affinity_core=3",
    ]


def _timer_arch_counter_lines() -> list[str]:
    return [
        "[timers] backend=arch-counter counter=vct timer_freq_hz=54000000",
    ]


def _driver_task_counter_lines() -> list[str]:
    return [
        "DRIVER_TASK_COUNTER contract=usb-local-seat hot_path=usb-keyboard "
        "source=root-ring sequence=1 submitted=2 completed=2 idle=0 fault=0 "
        "budget=0 frame=1 desc=1 staged_bytes=64 clean_ops=1 clean_bytes=64 "
        "inv_ops=1 inv_bytes=64 sends=2 yields=0 busy=0 same_request=0 "
        "timeouts=0 keep_active=0 aborts=0 overruns=0 drops=0 rx_frames=1 "
        "rx_bytes=8 tx_frames=1 tx_bytes=8 role_aux0=0 role_aux1=0 "
        "role_aux2=0 role_aux3=0",
        "DRIVER_TASK_COUNTER contract=cyw43455 hot_path=cyw43-wifi "
        "source=root-ring sequence=2 submitted=2 completed=2 idle=0 fault=0 "
        "budget=0 frame=1 desc=1 staged_bytes=64 clean_ops=0 clean_bytes=0 "
        "inv_ops=0 inv_bytes=0 sends=2 yields=0 busy=0 same_request=0 "
        "timeouts=0 keep_active=0 aborts=0 overruns=0 drops=0 rx_frames=1 "
        "rx_bytes=64 tx_frames=1 tx_bytes=64 role_aux0=0 role_aux1=0 "
        "role_aux2=0 role_aux3=0",
        "DRIVER_TASK_COUNTER contract=sdio-host hot_path=sdio-host "
        "source=root-ring sequence=2 submitted=2 completed=2 idle=0 fault=0 "
        "budget=0 frame=1 desc=1 staged_bytes=64 clean_ops=0 clean_bytes=0 "
        "inv_ops=0 inv_bytes=0 sends=2 yields=0 busy=0 same_request=0 "
        "timeouts=0 keep_active=0 aborts=0 overruns=0 drops=0 rx_frames=1 "
        "rx_bytes=64 tx_frames=1 tx_bytes=64 role_aux0=0 role_aux1=0 "
        "role_aux2=0 role_aux3=0",
    ]


def _strip_runtime_descriptor_seal_fields(lines: list[str]) -> list[str]:
    stripped: list[str] = []
    for line in lines:
        for token in (
            " descriptor_version=8",
            " descriptor_seal=valid",
            " artifact_hash=nonzero",
            " bus_link_seal=valid",
            " bus_link_seal=none",
        ):
            line = line.replace(token, "")
        stripped.append(line)
    return stripped


def _driver_task_dma_proof_lines(include_wifi: bool = True) -> list[str]:
    lines = [
        "DRIVER_TASK_DMA_PROOF contract=serial hot_path=serial-console "
        "status=ready profile=bounded-no-iommu descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=none root_pointer=no "
        "owner=linked-runtime mmio_pages=0 dma_pages=0 shared_pages=4 "
        "bus_address_policy=zero-dma cache_policy=uncached-plus-root-maintenance "
        "cache_clean_ops=0 cache_clean_bytes=0 cache_invalidate_ops=0 "
        "cache_invalidate_bytes=0 proof_effect=runtime-dma-proof-ready",
        "DRIVER_TASK_DMA_PROOF contract=usb-local-seat hot_path=usb-keyboard "
        "status=ready profile=bounded-no-iommu descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=valid root_pointer=no "
        "owner=linked-runtime mmio_pages=0 dma_pages=128 shared_pages=32 "
        "bus_address_policy=hal-bounded-bus-address "
        "cache_policy=uncached-plus-root-maintenance cache_clean_ops=1 "
        "cache_clean_bytes=64 cache_invalidate_ops=1 cache_invalidate_bytes=64 "
        "proof_effect=runtime-dma-proof-ready",
        "DRIVER_TASK_DMA_PROOF contract=hdmi-text hot_path=hdmi-text "
        "status=ready profile=bounded-no-iommu descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=none root_pointer=no "
        "owner=linked-runtime mmio_pages=0 dma_pages=0 shared_pages=16 "
        "bus_address_policy=zero-dma cache_policy=uncached-plus-root-maintenance "
        "cache_clean_ops=0 cache_clean_bytes=0 cache_invalidate_ops=0 "
        "cache_invalidate_bytes=0 proof_effect=runtime-dma-proof-ready",
        "DRIVER_TASK_DMA_PROOF contract=bcmgenet-v5 hot_path=genet-nic "
        "status=ready profile=bounded-no-iommu descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=none root_pointer=no "
        "owner=linked-runtime mmio_pages=6 dma_pages=64 shared_pages=32 "
        "bus_address_policy=hal-bounded-bus-address "
        "cache_policy=uncached-plus-root-maintenance cache_clean_ops=1 "
        "cache_clean_bytes=64 cache_invalidate_ops=1 cache_invalidate_bytes=64 "
        "proof_effect=runtime-dma-proof-ready",
        "DRIVER_TASK_DMA_PROOF contract=pcie-root hot_path=pcie-root "
        "status=ready profile=bounded-no-iommu descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=none root_pointer=no "
        "owner=linked-runtime mmio_pages=10 dma_pages=0 shared_pages=16 "
        "bus_address_policy=zero-dma cache_policy=uncached-plus-root-maintenance "
        "cache_clean_ops=0 cache_clean_bytes=0 cache_invalidate_ops=0 "
        "cache_invalidate_bytes=0 proof_effect=runtime-dma-proof-ready",
    ]
    if include_wifi:
        lines.extend(
            [
                "DRIVER_TASK_DMA_PROOF contract=cyw43455 hot_path=cyw43-wifi "
                "status=ready profile=bounded-no-iommu descriptor=present descriptor_version=8 "
                "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=valid root_pointer=no "
                "owner=linked-runtime mmio_pages=0 dma_pages=0 shared_pages=64 "
                "bus_address_policy=zero-dma "
                "cache_policy=uncached-plus-root-maintenance cache_clean_ops=0 "
                "cache_clean_bytes=0 cache_invalidate_ops=0 cache_invalidate_bytes=0 "
                "proof_effect=runtime-dma-proof-ready",
                "DRIVER_TASK_DMA_PROOF contract=sdio-host hot_path=sdio-host "
                "status=ready profile=bounded-no-iommu descriptor=present descriptor_version=8 "
                "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=valid root_pointer=no "
                "owner=linked-runtime mmio_pages=1 dma_pages=0 shared_pages=32 "
                "bus_address_policy=zero-dma "
                "cache_policy=uncached-plus-root-maintenance cache_clean_ops=0 "
                "cache_clean_bytes=0 cache_invalidate_ops=0 cache_invalidate_bytes=0 "
                "proof_effect=runtime-dma-proof-ready",
            ]
        )
    return lines


def _strong_driver_task_proof_lines() -> list[str]:
    return [
        "U-Boot 2026.01-dirty",
        "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
        "[Cohesix] Root console ready (type 'help' for commands)",
        "cohesix> driver proof",
        "[local-seat] usb keyboard command-ready source=linked-runtime-hid "
        "clean_polls=2 no_reply=0 recovery_pending=no",
        "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
        "first_byte_source=linked-runtime-hid command_ready=yes proof_gate=10 "
        "blocker=none",
        "usb: runtime_queue queue_valid=yes queued_reports=1 "
        "doorbell_pending=no transfer_events=1 report_status=produced-byte "
        "blocker=none",
        "OK NETTEST success",
        "netstats: active=wifi addr_src=dhcp-lease dhcp=bound wifi_assoc=1 "
        "wifi_link=1 eapol_secure=1 eapol_rx=1 rx_pkts=1 tx_pkts=1",
        "CYW43_SDIO_DPC generation=7 captures=4 published=4 consumed=4 "
        "rearms=4 overruns=0 epoch_errors=0 sequence_errors=0 "
        "ack_failures=0 owner_active=yes poisoned=no masked=no",
        "CYW43_SDIO_DPC_SCOPE captures=event-attempts published=ring-events "
        "poisoned=aggregate-client-or-ring source=card-int-or-source-probe "
        "physical_card_irq=not-exported",
        "CYW43_SDIO_DPC_TRUTH generation=7 owner_active=yes ring_poisoned=no "
        "client_sample_stale=no ring_consumer=4 sample_consumer=4 "
        "sample_reason=current authority=live-ring action=none",
        "DRIVER_TASK_DEFAULT requested=dedicated required=yes live_hot_paths=yes",
        "DRIVER_TASK_SELECTED profile=pi4-hardware selection=wifi "
        "active_net=cyw43 required_roles=0x3f required_hot_paths=0x7f "
        "required_tasks=6",
        *_timer_arch_counter_lines(),
        *_driver_task_boot_affinity_lines(),
        "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 "
        "task_count=9 failed_count=0 live_tcb_count=9 "
        "root_authority=admission-descriptor-diagnostics-only hardware_owner=linked-runtime fault_endpoint_ready=yes revoke_ready=yes "
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
        *_driver_task_dma_proof_lines(),
        *_driver_task_counter_lines(),
        "DRIVER_TASK_ACCEPTANCE dedicated_ready=yes reason=active-substrate "
        "substrate=active capset=pass fault=pass revoke=pass sched=pass "
        "affinity=pass vspace=isolated ipc_abi=shared-ring-command "
        "pointer_free_ipc=yes owner_state=driver-owned required=7 "
        "dedicated=7 compatibility=0 active_net=cyw43",
        "SERIAL_ECHO p95_us=800 max_gap_us=1200",
        "USB_BURST bytes=256 drops=0 max_latency_us=900",
        "usb: diag_liveness generation=1 status=pass proof=one-shot "
        "flow_delta=4/4/4/4 drop_delta=0 source=linked-runtime-hid "
        "next_action=none",
        "HDMI_RESPONSIVE max_gap_ms=9 mirrored_bytes=256",
    ]


def _strong_wired_driver_task_proof_lines() -> list[str]:
    return [
        "U-Boot 2026.01-dirty",
        "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
        "[Cohesix] Root console ready (type 'help' for commands)",
        "cohesix> driver proof",
        "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
        "first_byte_source=linked-runtime-hid proof_gate=10 blocker=none",
        "OK NETTEST success",
        "netstats: mode=dhcp policy=wired active=wired standby=wifi "
        "addr_src=dhcp-lease dhcp=bound ip=192.168.10.60 gateway=192.168.10.1 "
        "rx_pkts=4 tx_pkts=9",
        "DRIVER_TASK_SELECTED profile=pi4-hardware selection=wired "
        "active_net=genet required_roles=0x2f required_hot_paths=0x4f required_tasks=5",
        "[smp] activity selected profile=pi4-hardware net=wired "
        "active_contracts=selected-only",
        "DRIVER_TASK_DEFAULT requested=dedicated required=yes live_hot_paths=yes",
        *_timer_arch_counter_lines(),
        "DRIVER_TASK_BOOT contract=serial role=serial started=yes affinity_core=1",
        "DRIVER_TASK_BOOT contract=usb-local-seat role=usb started=yes affinity_core=1",
        "DRIVER_TASK_BOOT contract=hdmi-text role=display started=yes affinity_core=2",
        "DRIVER_TASK_BOOT contract=pcie-root role=pcie started=yes affinity_core=2",
        "DRIVER_TASK_BOOT contract=bcmgenet-v5 role=net started=yes affinity_core=3",
        "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 "
        "task_count=5 failed_count=0 live_tcb_count=5 "
        "root_authority=admission-descriptor-diagnostics-only hardware_owner=linked-runtime "
        "fault_endpoint_ready=yes revoke_ready=yes broad_caps_leaked=0 sched=yes "
        "affinity=per-driver affinity_configured=5 affinity_applied=5 vspace=isolated "
        "ipc_abi=shared-ring-command pointer_free_ipc=yes owner_state=driver-owned "
        "live_hot_paths=yes",
        "DRIVER_TASK_OWNER_STATE contract=serial hot_path=serial-console "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=usb-local-seat hot_path=usb-keyboard "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=hdmi-text hot_path=hdmi-text "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=bcmgenet-v5 hot_path=genet-nic "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=pcie-root hot_path=pcie-root "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=18",
        "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=22",
        "SCHED_CONTRACT contract=hdmi-text isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=44",
        "SCHED_CONTRACT contract=genet isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=73",
        "SCHED_CONTRACT contract=pcie-root isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=36",
        *_driver_task_dma_proof_lines(include_wifi=False),
        *_driver_task_counter_lines(),
        "DRIVER_TASK_ACCEPTANCE dedicated_ready=yes reason=active-substrate "
        "active_net=genet substrate=active capset=pass fault=pass revoke=pass sched=pass "
        "affinity=pass vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes "
        "owner_state=driver-owned required=5 dedicated=5 compatibility=0",
    ]


def _strong_wifi_selected_driver_task_proof_lines() -> list[str]:
    lines: list[str] = []
    selected_line = (
        "DRIVER_TASK_SELECTED profile=pi4-hardware selection=wifi "
        "active_net=cyw43 required_roles=0x3f required_hot_paths=0x77 "
        "required_tasks=6"
    )
    for line in _strong_driver_task_proof_lines():
        if "bcmgenet-v5" in line or "contract=genet" in line:
            continue
        if "task_count=9" in line:
            line = line.replace("task_count=9", "task_count=6")
            line = line.replace("live_tcb_count=9", "live_tcb_count=6")
            line = line.replace("affinity_configured=9", "affinity_configured=6")
            line = line.replace("affinity_applied=9", "affinity_applied=6")
        line = line.replace("required=7 dedicated=7", "required=6 dedicated=6")
        lines.append(line)
        if line.startswith("U-Boot "):
            lines.append(selected_line)
    return lines


def _oldgood_usb_replay_lines() -> list[str]:
    return [
        "usb: controller-ready source=linked-runtime",
        "usb: linked_runtime command-probe result=enable-slot-ok",
        "usb: phase=usb-root-port-reset-done cmd_path=yes",
        "usb: phase=usb-device-addressed slot=1",
        "usb: phase=usb-hub-set-configuration-done hub_slot=1 config_value=1",
        "usb: phase=usb-hub-context-done hub_slot=1 num_ports=4",
        "usb: phase=usb-hub-port-power-done hub_slot=1 hub_port=1",
        "usb: phase=usb-hub-port-status-done hub_slot=1 hub_port=1 "
        "connected=yes enabled=yes reset=no",
        "usb: phase=usb-hub-port-reset-set-done hub_slot=1 hub_port=1",
        "usb: phase=usb-hub-child-probe-begin hub_slot=1 hub_port=1 child_slot=2",
        "usb: phase=usb-hid-endpoint-parse-found slot=2 iface=0 class=0x03 "
        "subclass=0x01 protocol=0x01 ep=0x81 direction=in transfer=interrupt",
        "usb: phase=usb-hid-interrupt-queue-ready slot=2 ep=0x81 dci=3 queued=1",
        "[local-seat] usb hid first report source=linked-runtime-hid "
        "len=8 keys=0x17 transfer_event=yes",
        "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no "
        "clean_polls=2 no_reply=0 recovery_pending=no",
        "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
        "read=1 ascii=0x74 key=0x17",
        "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
        "first_byte_source=linked-runtime-hid proof_gate=10 target_gate=10 blocker=none",
    ]


def _retained_usb_oldgood_lines() -> list[str]:
    return [
        "USB_OLDGOOD_RETAINED v=1 task=12 token=0xdeadbeef "
        "link_epoch=7 link_token=0xc001d00d epoch=3 seq=14 "
        "mask=0x00003fff topology=0x10230581 input_gen=9 commit=14 "
        "source=linked-runtime-hid",
        "USB_OLDGOOD_CURRENT contracts=usb-local-seat+pcie-root "
        "owners=driver-owned+driver-owned descriptors=sealed+sealed "
        "command_ready=yes proof_gate=14 blocker=none root_pointer=no",
    ]


def _oldgood_wifi_replay_lines() -> list[str]:
    return [
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=begin backoff_ms=0 "
        "next_attempt_ms=100 serial=ready local_seat=enabled recovery=full "
        "console_seq=1 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes",
        "SDIO_DRIVER_TASK_REPLAY_STATUS stage=engine-init blocker=ready detail=0x5500",
        "wifi: cyw43-transport-ready owner=linked-runtime",
        "wifi: firmware_contract fw=609309 nvram=1744 clm=2676 "
        "fw_hash=d608f866582519c0a28d86db43040f4f1b98dd1d153e72e9752586546b4a36c3 "
        "nvram_hash=ca709be81a78bdb6932936374f39943acbd7af07fae6151011127599a3ce9e3d "
        "clm_hash=9823842cae9fb9a5dd1e5fb31f595516ec7deee341354bef30bb3026eee29cc1 "
        "board=raspberrypi,4-model-b rstvec=0xb83ef198 verified=yes "
        "armcr4_release=1 sr_kso=yes current_clock=41666666Hz preferred=41666666Hz",
        "wifi: cyw43-release-firmware-ready-done status=ready",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-function2 status=ready",
        "wifi: function2-ready f2_enabled=yes f2_ready=yes",
        "[pi4-wifi] sdio irq contract irq=158 trigger=level "
        "bound=1 badge=0x9f device_clear=sdio-intstatus+sdhci-cardint "
        "ack=after-clear int_status=0x00000000 int_enable=0x027f003b "
        "signal=0x00000100",
        "[pi4-wifi] firmware stage=setup-firmware-channel "
        "action=interrupts-armed path=sel4-irq "
        "source=hostintmask+cccr-ienx+sdhci-card-int "
        "fn_int_mask_policy=linux-unused ien=0x07",
        "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
        "stage=cyw43-control-txglomalign event=pre-tx-drain-ready poll=0",
        "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
        "stage=cyw43-control-txglomalign event=tx-complete result=16",
        "wifi: control_exchange step=cyw43-control-txglomalign "
        "status=matched bus:txglomalign=8",
        "wifi: control_exchange step=cyw43-control-ulp-sdioctrl "
        "status=unsupported tolerated=yes",
        "wifi: control_exchange step=cyw43-control-rxglom status=matched bus:rxglom=1",
        "wifi: control_exchange step=cyw43-control-cur-etheraddr status=matched",
        "wifi: control_exchange step=cyw43-control-revinfo status=matched",
        "CYW43_DRIVER_TASK_CLM contract=cyw43455 stage=cyw43-control-clmload "
        "action=ready index=2 offset=2676 len=2676 flags=0x0000",
        "CYW43_DRIVER_TASK_TEXT_IOVAR contract=cyw43455 "
        "stage=cyw43-control-firmware-version name=ver printable_len=48",
        "CYW43_DRIVER_TASK_TEXT_IOVAR contract=cyw43455 "
        "stage=cyw43-control-clm-version name=clmver printable_len=16",
        "wifi: control_exchange step=cyw43-control-up status=matched",
        "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
        "path=primary-bsscfg:join action=ready ssid_len=7 result=0x00000000",
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS status=required "
        "associated=yes link_up=yes eapol_rx=0",
        "wifi: host-eapol msg=m1 ethertype=0x888e last_ethertype_valid=yes",
        "wifi: host-eapol action=send-m2 msg=m2",
        "wifi: host-eapol msg=m3 ethertype=0x888e last_ethertype_valid=yes",
        "wifi: host-eapol action=send-m4 msg=m4",
        "[cyw43] host-eapol action=install-wsec-key kind=ptk result=ok",
        "[cyw43] host-eapol action=install-wsec-key kind=gtk result=ok",
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS status=secure "
        "associated=yes link_up=yes eapol_rx=2",
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=stabilizing backoff_ms=0 "
        "next_attempt_ms=150 serial=ready local_seat=enabled recovery=full "
        "console_seq=2 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes",
        "wifi: gate 8 subgate=8a-pair-generation status=pass "
        "pair_epoch=1 generation=9 blocker=none",
        "wifi: gate 8 subgate=8b-control-program status=pass "
        "pair_epoch=1 generation=9 blocker=none",
        "wifi: gate 8 subgate=8c-join-terminal status=pass "
        "pair_epoch=1 generation=9 blocker=none",
        "wifi: gate 8 subgate=8d-association-link status=pass "
        "pair_epoch=1 generation=9 blocker=none",
        "wifi: gate 8 subgate=8e-bssid-refresh status=pass "
        "pair_epoch=1 generation=9 blocker=none",
        "wifi: gate 8 subgate=8f-eapol-keys status=pass "
        "pair_epoch=1 generation=9 blocker=none",
        "wifi: gate 8 subgate=8g-post-key-maintenance status=pass "
        "pair_epoch=1 generation=9 blocker=none",
        "wifi: gate 8 subgate=8h-data-admission status=pass "
        "pair_epoch=1 generation=9 blocker=none",
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=ready backoff_ms=0 "
        "next_attempt_ms=200 serial=ready local_seat=enabled recovery=full "
        "console_seq=3 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes",
        "[dhcp] start ready interface=wifi",
        "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
        "server=192.168.10.1 lease_s=3600",
        "[net-selftest] result generation=9 tx_ok=true udp_echo_ok=false "
        "tcp_ok=false console_ok=true peer_assisted_ok=true "
        "result=peer-assisted-pass",
        "OK NETTEST detail=pass scope=serial-local generation=9",
        "netstats: rx_pkts=4 tx_pkts=9 rx_used=4 tx_used=9 polls=30",
        "netstats: generation=9 udp_rx=2 udp_tx=4 tcp_accepts=1 tcp_auth=1 "
        "tcp_rx_bytes=58 tcp_tx_bytes=6782",
        "netstats: mode=dhcp policy=wifi active=wifi standby=wired "
        "addr_src=dhcp-lease ip=192.168.10.50 gateway=192.168.10.1 dhcp=bound",
        "netstats: wifi_assoc=1 wifi_link=1 eapol_rx=2 eapol_start=1 eapol_secure=1",
        "netstatus: generation=9 ip=192.168.10.50 gateway=192.168.10.1 "
        "src=dhcp-lease dhcp=bound tcp_ready=yes",
        "[cohsh-net][auth] auth OK, session established (generation=9 conn_id=1)",
        "CYW43_SDIO_DPC generation=9 captures=6 published=6 consumed=6 "
        "rearms=6 overruns=0 epoch_errors=0 sequence_errors=0 "
        "ack_failures=0 owner_active=yes poisoned=no masked=no",
        "CYW43_SDIO_DPC_SCOPE captures=event-attempts published=ring-events "
        "poisoned=aggregate-client-or-ring source=card-int-or-source-probe "
        "physical_card_irq=not-exported",
        "CYW43_SDIO_DPC_TRUTH generation=9 owner_active=yes ring_poisoned=no "
        "client_sample_stale=no ring_consumer=6 sample_consumer=6 "
        "sample_reason=current authority=live-ring action=none",
    ]


def test_gate_proof_does_not_emit_leading_carriage_return() -> None:
    """Serial proof commands should not manufacture empty console commands."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert "send_console_line()" in source
    assert "printf '%s' \"${char}\"" in source
    assert "printf '\\r' > \"${SERIAL_DEVICE}\"" in source
    assert "printf '\\r%s\\r'" not in source


def test_gate_proof_waits_for_prompt_between_commands() -> None:
    """Serial proof commands must not overlap long diagnostic output."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert "console_prompt_count()" in source
    assert "wait_for_prompt_after_command" in source
    assert "COMMAND_PROMPT_TIMEOUT_SECONDS=30" in source
    assert "COMMAND_CHAR_DELAY_SECONDS=" in source


def test_gate_proof_waits_for_prompt_at_line_start() -> None:
    """Capture readiness must not match debug prose containing the prompt text."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert "console_prompt_seen()" in source
    assert 'line.startswith(b"cohesix>")' in source
    assert "grep -q 'cohesix>'" not in source


def test_gate_proof_advances_current_cohesix_boot_menu() -> None:
    """Capture automation must recognize the current root-menu heading."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert "\\[cohesix\\] Cohesix boot menu" in source
    assert "\\[cohesix\\] Cohesix boot options" not in source
    assert "Select option \\[1\\]:" in source
    assert "printf '1\\r'" in source


def test_gate_proof_runs_smp_activity_for_post_prompt_driver_proof() -> None:
    """Default captures should refresh driver-task proof after prompt-side replay."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert '"smp activity"' in source
    assert source.index('"netstats"') < source.index('"smp activity"')
    assert source.index('"smp activity"') < source.index('"wifi diag"')
    assert source.index('"nettest"') < source.index('"wifi diag"')
    assert "wait_for_wifi_supervisor_terminal" in source
    assert "wait_for_wifi_dhcp_bound" in source
    assert 'commands+=("wifi dump-state")' in source
    assert '"${REQUIRE_WIFI_READY}" -eq 1' in source


def test_gate_proof_defaults_to_passive_usb_diagnostics() -> None:
    """The default evidence lane must not execute an active keyboard probe."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    defaults = source.split("DEFAULT_COMMANDS=(", 1)[1].split(")", 1)[0]

    assert '"usb diag"' in defaults
    assert defaults.count('"usb status"') == 1
    assert '"usb probe-kbd"' not in defaults
    assert "--probe-usb-keyboard" in source


def test_gate_proof_seals_controlled_serial_network_capture_pair() -> None:
    """Fresh Pi proof can bind packets captured during the exact serial run."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert 'tcpdump_bin="$(command -v tcpdump || true)"' in source
    assert "PI4_RUNTIME_DMA_CAPTURE_PAIRING=controlled-concurrent" in source
    for field in (
        "PI4_RUNTIME_DMA_SERIAL_LOG_SHA256",
        "PI4_RUNTIME_DMA_SERIAL_LOG_BYTES",
        "PI4_RUNTIME_DMA_NETWORK_INTERFACE",
        "PI4_RUNTIME_DMA_NETWORK_CAPTURE",
        "PI4_RUNTIME_DMA_NETWORK_CAPTURE_SHA256",
        "PI4_RUNTIME_DMA_NETWORK_CAPTURE_BYTES",
        "PI4_RUNTIME_DMA_CAPTURE_STARTED_AT_UTC",
        "PI4_RUNTIME_DMA_CAPTURE_FINISHED_AT_UTC",
    ):
        assert field in source


def test_gate_proof_binds_gateway_continuity_into_runtime_proof() -> None:
    """Qualified proof captures boot before sealing pre-load gateway continuity."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    capture_body = source.split("run_capture() {", 1)[1].split(
        "\n}\n\nrun_normalizer()",
        1,
    )[0]

    assert "--gateway-status-url" in source
    assert capture_body.index("start_network_capture") < capture_body.index(
        "start_serial_capture"
    )
    assert capture_body.index("start_serial_capture") < capture_body.index(
        'sleep "${BOOT_WAIT_SECONDS}"'
    )
    assert capture_body.index('sleep "${BOOT_WAIT_SECONDS}"') < (
        capture_body.index("wait_for_console_ready")
    )
    assert capture_body.index("wait_for_console_ready") < capture_body.index(
        "wait_for_wifi_supervisor_terminal"
    )
    assert capture_body.index("wait_for_wifi_supervisor_terminal") < (
        capture_body.index("capture_gateway_continuity_start")
    )
    assert capture_body.index("capture_gateway_continuity_start") < (
        capture_body.index('log "console command: ${command}"')
    )
    assert capture_body.index("capture_gateway_continuity_end") < (
        capture_body.index('if [[ -n "${CAPTURE_PID}" ]]')
    )
    assert capture_body.index("capture_gateway_continuity_end") < (
        capture_body.index("finish_network_capture")
    )
    assert capture_body.index("finish_network_capture") < capture_body.index(
        "validate_gateway_capture_timeline"
    )
    assert 'serial_proof_source="${SERIAL_SNAPSHOT_PATH}"' in source
    for field in (
        "PI4_RUNTIME_DMA_GATEWAY_CONTINUITY=connected-single-session",
        "PI4_RUNTIME_DMA_GATEWAY_STATUS_ENDPOINT",
        "PI4_RUNTIME_DMA_GATEWAY_TARGET_HOST",
        "PI4_RUNTIME_DMA_GATEWAY_TARGET_PORT",
        "PI4_RUNTIME_DMA_GATEWAY_START_CAPTURED_UNIX_NS",
        "PI4_RUNTIME_DMA_GATEWAY_START_CONNECTED",
        "PI4_RUNTIME_DMA_GATEWAY_START_CONNECTS",
        "PI4_RUNTIME_DMA_GATEWAY_START_RECONNECTS",
        "PI4_RUNTIME_DMA_GATEWAY_START_LAST_CHANGE_UNIX_MS",
        "PI4_RUNTIME_DMA_GATEWAY_START_TARGET_HOST",
        "PI4_RUNTIME_DMA_GATEWAY_START_TARGET_PORT",
        "PI4_RUNTIME_DMA_GATEWAY_END_CAPTURED_UNIX_NS",
        "PI4_RUNTIME_DMA_GATEWAY_END_CONNECTED",
        "PI4_RUNTIME_DMA_GATEWAY_END_CONNECTS",
        "PI4_RUNTIME_DMA_GATEWAY_END_RECONNECTS",
        "PI4_RUNTIME_DMA_GATEWAY_END_LAST_CHANGE_UNIX_MS",
        "PI4_RUNTIME_DMA_GATEWAY_END_TARGET_HOST",
        "PI4_RUNTIME_DMA_GATEWAY_END_TARGET_PORT",
    ):
        assert field in source


def test_gate_proof_orders_wired_and_wifi_nettest_without_cross_gating() -> None:
    """GENET runs nettest directly; only WiFi waits for CYW43 and DHCP."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")
    capture_body = source.split("run_capture() {", 1)[1].split(
        "\n}\n\nrun_normalizer()",
        1,
    )[0]
    gateway_start = capture_body.index("capture_gateway_continuity_start")
    command_loop = capture_body.index('for command in "${commands[@]}"')
    wifi_wait = capture_body.index('if [[ "${REQUIRE_WIFI_READY}" -eq 1 ]]')
    nettest_wifi_gate = capture_body.index(
        'if [[ "${command}" == "nettest" && "${REQUIRE_WIFI_READY}" -eq 1 ]]'
    )
    command_send = capture_body.index('log "console command: ${command}"')

    assert wifi_wait < gateway_start < command_loop
    assert command_loop < nettest_wifi_gate < command_send
    assert 'if [[ "${wifi_commands}" -eq 1 ]]' not in capture_body
    assert '"${command}" == "nettest" ]]; then' not in capture_body
    assert "wait_for_wifi_supervisor_terminal" in capture_body[wifi_wait:gateway_start]
    assert "wait_for_wifi_dhcp_bound" in capture_body[nettest_wifi_gate:command_send]


def test_gateway_continuity_accepts_one_unchanged_authenticated_connection(
    tmp_path: pathlib.Path,
) -> None:
    """Both status samples must describe the same first gateway connection."""

    status = {
        "connected": True,
        "connects": 1,
        "reconnects": 0,
        "last_change_unix_ms": 1000,
        "broker": {},
    }
    server, thread, base_url, requests = _start_gateway_status_server(
        [status, status]
    )
    try:
        result = _run_output_guard_probe(
            tmp_path,
            '''
GATEWAY_STATUS_ENDPOINT="$(normalize_gateway_status_url "$4")"
GATEWAY_REQUEST_AUTH_TOKEN="test-token"
capture_gateway_continuity_start
capture_gateway_continuity_end
NETWORK_CAPTURE_STARTED_UNIX_NS=0
NETWORK_CAPTURE_FINISHED_UNIX_NS=9000000000000000000
validate_gateway_capture_timeline
printf '%s\n' \
  "${GATEWAY_START_CONNECTED}:${GATEWAY_START_CONNECTS}:${GATEWAY_START_RECONNECTS}:${GATEWAY_START_LAST_CHANGE_UNIX_MS}" \
  "${GATEWAY_END_CONNECTED}:${GATEWAY_END_CONNECTS}:${GATEWAY_END_RECONNECTS}:${GATEWAY_END_LAST_CHANGE_UNIX_MS}"
''',
            base_url,
        )
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)

    assert result.returncode == 0, result.stderr
    expected = "true:1:0:1000"
    assert result.stdout.splitlines() == [expected, expected]
    assert requests == [
        ("/v1/meta/status", "Bearer test-token", "test-token"),
        ("/v1/meta/status", "Bearer test-token", "test-token"),
    ]


def test_gateway_continuity_rejects_connection_before_capture(
    tmp_path: pathlib.Path,
) -> None:
    """A pre-capture connection cannot be relabelled as the boot session."""

    status = {
        "connected": True,
        "connects": 1,
        "reconnects": 0,
        "last_change_unix_ms": 1000,
        "broker": {},
    }
    server, thread, base_url, _requests = _start_gateway_status_server(
        [status, status]
    )
    try:
        result = _run_output_guard_probe(
            tmp_path,
            '''
GATEWAY_STATUS_ENDPOINT="$(normalize_gateway_status_url "$4")"
capture_gateway_continuity_start
capture_gateway_continuity_end
NETWORK_CAPTURE_STARTED_UNIX_NS=1001000000
NETWORK_CAPTURE_FINISHED_UNIX_NS=9000000000000000000
validate_gateway_capture_timeline
''',
            base_url,
        )
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)

    assert result.returncode == 1
    assert "outside the controlled capture" in result.stderr


def test_gateway_continuity_rejects_connection_change(
    tmp_path: pathlib.Path,
) -> None:
    """A changed connection timestamp fails even when counters look healthy."""

    start = {
        "connected": True,
        "connects": 1,
        "reconnects": 0,
        "last_change_unix_ms": 1_788_000_000_123,
        "broker": {},
    }
    end = dict(start, last_change_unix_ms=1_788_000_000_124)
    server, thread, base_url, _requests = _start_gateway_status_server([start, end])
    try:
        result = _run_output_guard_probe(
            tmp_path,
            '''
GATEWAY_STATUS_ENDPOINT="$(normalize_gateway_status_url "$4")"
capture_gateway_continuity_start
capture_gateway_continuity_end
''',
            base_url,
        )
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)

    assert result.returncode == 1
    assert "gateway connection changed during the controlled capture" in result.stderr


def test_gateway_continuity_rejects_nonfinite_json(
    tmp_path: pathlib.Path,
) -> None:
    """Python's non-standard NaN extension cannot enter a strict status proof."""

    status = {
        "connected": True,
        "connects": 1,
        "reconnects": 0,
        "last_change_unix_ms": 1_788_000_000_123,
        "broker": {"invalid": float("nan")},
    }
    server, thread, base_url, _requests = _start_gateway_status_server([status])
    try:
        result = _run_output_guard_probe(
            tmp_path,
            '''
GATEWAY_STATUS_ENDPOINT="$(normalize_gateway_status_url "$4")"
GATEWAY_READY_TIMEOUT_SECONDS=0
capture_gateway_continuity_start
''',
            base_url,
        )
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)

    assert result.returncode == 1
    assert "non-finite JSON constant: NaN" in result.stderr
    assert "gateway continuity did not become ready within 0s" in result.stderr


@pytest.mark.parametrize(
    ("field", "value", "message"),
    (
        ("target_host", "192.168.50.24", "differs from the selected Pi"),
        ("target_port", 31338, "not the Cohesix console port"),
    ),
)
def test_gateway_continuity_rejects_wrong_backend_target(
    tmp_path: pathlib.Path,
    field: str,
    value: object,
    message: str,
) -> None:
    """The gate cannot seal a gateway connected to another backend target."""

    status = {
        "connected": True,
        "connects": 1,
        "reconnects": 0,
        "last_change_unix_ms": 1000,
        field: value,
        "broker": {},
    }
    server, thread, base_url, _requests = _start_gateway_status_server([status])
    try:
        result = _run_output_guard_probe(
            tmp_path,
            '''
GATEWAY_STATUS_ENDPOINT="$(normalize_gateway_status_url "$4")"
GATEWAY_READY_TIMEOUT_SECONDS=0
capture_gateway_continuity_start
''',
            base_url,
        )
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)

    assert result.returncode == 1
    assert message in result.stderr


def test_gateway_status_url_requires_qualified_active_capture(
    tmp_path: pathlib.Path,
) -> None:
    """Offline and diagnostic-only runs cannot claim gateway continuity."""

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--gateway-status-url",
            "http://127.0.0.1:8080",
            "--gateway-target-host",
            "192.168.10.60",
            "--venv",
            str(REPO_ROOT / ".venv"),
            "--log",
            str(tmp_path / "unused.log"),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    assert (
        "--gateway-status-url requires active serial/network capture "
        "and --require-driver-task-proof"
    ) in result.stderr


def test_gateway_status_url_requires_canonical_target_host(
    tmp_path: pathlib.Path,
) -> None:
    """The retained target host is an exact IP, not an ambiguous host label."""

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--gateway-status-url",
            "http://127.0.0.1:8080",
            "--gateway-target-host",
            "192.168.010.060",
            "--network-interface",
            "en0",
            "--network-capture-out",
            str(tmp_path / "capture.pcap"),
            "--require-driver-task-proof",
            "--venv",
            str(REPO_ROOT / ".venv"),
            "--log",
            str(tmp_path / "serial.log"),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    assert "gateway target host must be an IP address" in result.stderr
    assert "--gateway-target-host is invalid" in result.stderr


@pytest.mark.parametrize("replacement", ("directory", "symlink"))
def test_output_guard_rejects_renamed_or_symlinked_ancestor_before_publication(
    tmp_path: pathlib.Path,
    replacement: str,
) -> None:
    """A prepared parent identity cannot be replaced before final publication."""

    result = _run_output_guard_probe(
        tmp_path,
        f'''
root="$3/probe"
mkdir -p "${{root}}"
target="${{root}}/output/proof.env"
IFS=$'\t' read -r source_path source_seal \
  <<<"$(create_sealed_temp_file "cohesix-guard-source-")"
printf 'exact-proof\n' >"${{source_path}}"
seal="$(prepare_fresh_output_path "proof" "${{target}}")"
mv "${{root}}/output" "${{root}}/retained-output"
if [[ "{replacement}" == "symlink" ]]; then
    mkdir "${{root}}/attacker-output"
    ln -s "${{root}}/attacker-output" "${{root}}/output"
else
    mkdir "${{root}}/output"
fi
if publish_file_exclusively \
  "proof" "${{source_path}}" "${{target}}" "${{seal}}" \
  "${{source_seal}}" >/dev/null; then
    exit 20
fi
[[ ! -e "${{root}}/retained-output/proof.env" ]]
[[ ! -e "${{root}}/output/proof.env" ]]
''',
    )

    assert result.returncode == 0, result.stderr
    assert "output ancestor identity changed" in result.stderr or "unsafe" in result.stderr


def test_output_guard_preserves_existing_destination_on_publish_race(
    tmp_path: pathlib.Path,
) -> None:
    """A leaf created after preparation must remain byte-exact and unmodified."""

    result = _run_output_guard_probe(
        tmp_path,
        '''
root="$3/probe"
mkdir -p "${root}"
target="${root}/output/proof.env"
IFS=$'\t' read -r source_path source_seal \
  <<<"$(create_sealed_temp_file "cohesix-guard-source-")"
printf 'exact-proof\n' >"${source_path}"
seal="$(prepare_fresh_output_path "proof" "${target}")"
printf 'attacker-owned\n' >"${target}"
if publish_file_exclusively \
  "proof" "${source_path}" "${target}" "${seal}" \
  "${source_seal}" >/dev/null; then
    exit 20
fi
[[ "$(cat "${target}")" == "attacker-owned" ]]
''',
    )

    assert result.returncode == 0, result.stderr
    assert "destination already exists" in result.stderr


def test_serial_output_reservation_is_idempotent_before_capture(
    tmp_path: pathlib.Path,
) -> None:
    """Pre-build reservation remains valid when capture starts later."""

    result = _run_output_guard_probe(
        tmp_path,
        '''
LOG_PATH="$3/probe/pi4-serial.log"
ensure_capture_log_is_fresh
first_seal="${LOG_OUTPUT_SEAL}"
ensure_capture_log_is_fresh
[[ "${LOG_OUTPUT_SEAL}" == "${first_seal}" ]]
[[ -f "${LOG_PATH}" ]]
[[ ! -s "${LOG_PATH}" ]]
''',
    )

    assert result.returncode == 0, result.stderr


def test_output_guard_rejects_replaced_temporary_source_inode(
    tmp_path: pathlib.Path,
) -> None:
    """A completed network/proof temporary cannot be swapped before publish."""

    result = _run_output_guard_probe(
        tmp_path,
        '''
root="$3/probe"
mkdir -p "${root}"
target="${root}/output/proof.env"
seal="$(prepare_fresh_output_path "proof" "${target}")"
IFS=$'\t' read -r source_path source_seal \
  <<<"$(create_sealed_temp_file "cohesix-guard-source-")"
printf 'exact-proof\n' >"${source_path}"
mv "${source_path}" "${source_path}.retained"
printf 'replacement\n' >"${source_path}"
if publish_file_exclusively \
  "proof" "${source_path}" "${target}" "${seal}" \
  "${source_seal}" >/dev/null; then
    exit 20
fi
[[ ! -e "${target}" ]]
[[ "$(cat "${source_path}")" == "replacement" ]]
''',
    )

    assert result.returncode == 0, result.stderr
    assert "temporary source" in result.stderr


def test_output_guard_never_writes_through_replaced_temporary_symlink(
    tmp_path: pathlib.Path,
) -> None:
    """Sealed proof writes cannot truncate a file reached through a swapped leaf."""

    result = _run_output_guard_probe(
        tmp_path,
        '''
IFS=$'\t' read -r source_path source_seal \
  <<<"$(create_sealed_temp_file "cohesix-guard-source-")"
victim="$3/victim.txt"
printf 'keep-me\n' >"${victim}"
rm "${source_path}"
ln -s "${victim}" "${source_path}"
if printf 'replacement\n' \
    | write_sealed_temp_file "proof" "${source_path}" "${source_seal}"; then
    exit 20
fi
[[ "$(cat "${victim}")" == "keep-me" ]]
''',
    )

    assert result.returncode == 0, result.stderr
    assert "temporary output" in result.stderr


def test_output_guard_rejects_replaced_reserved_serial_leaf(
    tmp_path: pathlib.Path,
) -> None:
    """Serial snapshots stay bound to the exclusively reserved capture inode."""

    result = _run_output_guard_probe(
        tmp_path,
        '''
root="$3/probe"
mkdir -p "${root}"
target="${root}/output/pi4-serial.log"
seal="$(prepare_fresh_output_path "serial" "${target}" yes)"
rm "${target}"
printf 'replacement\n' >"${target}"
if snapshot_sealed_output "serial" "${seal}" >/dev/null; then
    exit 20
fi
[[ "$(cat "${target}")" == "replacement" ]]
''',
    )

    assert result.returncode == 0, result.stderr
    assert "output leaf identity changed" in result.stderr


def test_gate_proof_publishes_only_an_exact_positive_cyw43_record() -> None:
    """The WiFi producer must bind one exact image and live serial/pcap pair."""

    source = SCRIPT_PATH.read_text(encoding="utf-8")

    assert '"schema": "cohesix-cyw43-coexistence-binding/v2"' in source
    assert '"producer": "pi4_gate_proof/v1"' in source
    assert '"classification": "positive-exact-image-live-closure"' in source
    assert '"session_projection": session_projection' in source
    assert '"runtime_evidence_sha256": sha256(runtime_raw)' in source
    assert '"serial_sha256": sha256(serial_raw)' in source
    assert '"sha256": sha256(capture_raw)' in source
    assert "validate_serial_image_identity(serial_raw, metadata)" in source
    assert (
        "validate_pi_network_log(serial_raw, harness.BENCHMARK_TRANSPORT_WIFI)"
        in source
    )
    assert "validate_pi_correlated_network_capture(" in source
    assert "pi_cyw43_outcomes_from_normalized_gate(summary_raw)" in source
    assert '"outcomes": derived_outcomes' in source
    assert "os.O_EXCL" in source
    assert "summary_raw = Path(summary_value).read_bytes()" in source


def test_gate_proof_rejects_offline_cyw43_record_publication(
    tmp_path: pathlib.Path,
) -> None:
    """A retained log cannot be upgraded into a positive coexistence record."""

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--venv",
            str(REPO_ROOT / ".venv"),
            "--log",
            str(tmp_path / "serial.log"),
            "--cyw43-coexistence-record-out",
            str(tmp_path / "cyw43.json"),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    assert "positive CYW43 output requires WiFi ready" in result.stderr


@pytest.mark.parametrize(
    "arguments",
    [
        ["--network-interface", "en0"],
        ["--network-capture-out", "capture.pcap"],
        [
            "--network-interface",
            "en0",
            "--network-capture-out",
            "capture.pcap",
        ],
    ],
)
def test_gate_proof_rejects_unpaired_or_offline_network_capture(
    tmp_path: pathlib.Path,
    arguments: list[str],
) -> None:
    """Only the active capture path can claim concurrent packet pairing."""

    venv_dir = REPO_ROOT / ".venv"
    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--venv",
            str(venv_dir),
            "--log",
            str(tmp_path / "serial.log"),
            *arguments,
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 2
    if len(arguments) == 2:
        assert "must be supplied together" in result.stderr
    else:
        assert "requires the active serial capture path" in result.stderr


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
    assert '"USB_BLOCKER=command-event-rings"' in source
    assert '"USB_BLOCKER=command-ring-ready"' in source
    assert '"USB_BLOCKER=root-port-connected"' in source
    assert '"USB_BLOCKER=root-port-reset-no-reply"' in source
    assert '"USB_BLOCKER=root-port-reset-completion-no-reply"' in source
    assert '"USB_BLOCKER=root-port-enable-timeout"' in source
    assert '"USB_BLOCKER=address-device-command-completion-no-reply"' in source
    assert '"USB_BLOCKER=address-device-publish-no-reply"' in source
    assert '"USB_BLOCKER=address-device-failed"' in source
    assert '"USB_BLOCKER=device-descriptor-no-reply"' in source
    assert '"USB_BLOCKER=device-addressed"' in source
    assert '"USB_BLOCKER=pcie-xhci-device-coverage-missing"' in source
    assert '"USB_BLOCKER=pcie-owner-ring-unavailable"' in source
    assert '"USB_BLOCKER=pcie-vl805-config-contract-missing"' in source
    assert '"USB_BLOCKER=hid-first-byte"' in source
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
                "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
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
                "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
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
                "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
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
                "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
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
                "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
                "[cohesix:root-task] Cohesix boot: root-task online",
                "usb: linked_runtime command-probe result=enable-slot-ok",
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
                *_driver_task_boot_affinity_lines(),
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
                *_driver_task_boot_affinity_lines(),
                "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 mcs=0 task_count=9 failed_count=0 live_tcb_count=9 root_authority=admission-descriptor-diagnostics-only hardware_owner=linked-runtime fault_endpoint_ready=yes revoke_ready=yes broad_caps_leaked=0 sched=yes affinity=per-driver affinity_configured=9 affinity_applied=9 vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes owner_state=driver-owned live_hot_paths=yes",
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
                *_driver_task_boot_affinity_lines(),
                "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 task_count=9 failed_count=0 live_tcb_count=9 root_authority=admission-descriptor-diagnostics-only hardware_owner=linked-runtime fault_endpoint_ready=yes revoke_ready=yes broad_caps_leaked=0 sched=yes affinity=per-driver affinity_configured=9 affinity_applied=9 vspace=isolated ipc_abi=callback-pointer pointer_free_ipc=no owner_state=driver-owned live_hot_paths=yes",
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
                *_driver_task_boot_affinity_lines(),
                "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 task_count=9 failed_count=0 live_tcb_count=9 root_authority=admission-descriptor-diagnostics-only hardware_owner=linked-runtime fault_endpoint_ready=yes revoke_ready=yes broad_caps_leaked=0 sched=yes affinity=per-driver affinity_configured=9 affinity_applied=9 vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes owner_state=linked-runtime-owner-state-missing live_hot_paths=yes",
                "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=18",
                "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=22",
                "SCHED_CONTRACT contract=hdmi-text isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=44",
                "SCHED_CONTRACT contract=cyw43455 isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=91",
                "SCHED_CONTRACT contract=genet isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=73",
                "SCHED_CONTRACT contract=sdio-host isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=31",
                "SCHED_CONTRACT contract=pcie-root isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated observed_service_us=36",
                "DRIVER_TASK_ACCEPTANCE dedicated_ready=no reason=driver-task-owner-state-not-proven substrate=active capset=pass fault=pass revoke=pass sched=pass affinity=pass vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes owner_state=linked-runtime-owner-state-missing required=7 dedicated=7 compatibility=0",
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
    assert "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF=yes" in result.stdout
    assert "DRIVER_TASK_DEDICATED_READY=yes" in result.stdout
    assert "DRIVER_TASK_SDIO_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_DMA_BLOCKER=none" in result.stdout
    assert "PI4_RUNTIME_DMA_PROOF=fresh-pi" in result.stdout
    assert "PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified" in result.stdout


def test_gate_proof_rejects_pre_seal_driver_task_proof(
    tmp_path: pathlib.Path,
) -> None:
    """Strict driver proof must reject stale descriptor-present evidence."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(_strip_runtime_descriptor_seal_fields(_strong_driver_task_proof_lines())),
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
    assert "DRIVER_TASK_OWNER_STATE_PROOF=yes" in result.stdout
    assert "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF=no" in result.stdout
    assert (
        "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF expected yes got no"
        in result.stderr
    )
    assert "PI4_RUNTIME_DMA_PROOF=diagnostic" in result.stdout


def test_gate_proof_rejects_v7_runtime_descriptor_proof(
    tmp_path: pathlib.Path,
) -> None:
    """Strict proof rejects sealed ABI v7 lines after the v8 authority change."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    stale_lines = [
        line.replace("descriptor_version=8", "descriptor_version=7")
        for line in _strong_driver_task_proof_lines()
    ]
    log_path = tmp_path / "pi4-serial-v7.log"
    log_path.write_text("\n".join(stale_lines), encoding="utf-8")

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
    assert "DRIVER_TASK_OWNER_STATE_PROOF=yes" in result.stdout
    assert "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF=no" in result.stdout
    assert (
        "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF expected yes got no"
        in result.stderr
    )
    assert "PI4_RUNTIME_DMA_PROOF=diagnostic" in result.stdout


def test_gate_proof_driver_task_proof_does_not_require_usb_first_byte(
    tmp_path: pathlib.Path,
) -> None:
    """Owner-state proof is separate from local-seat HID first-byte proof."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    lines = [
        line
        for line in _strong_driver_task_proof_lines()
        if not line.startswith(
            (
                "usb: runtime_gate",
                "[local-seat] usb keyboard command-ready",
            )
        )
    ]
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
    assert "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF=yes" in result.stdout
    assert "USB_GATE=10" not in result.stdout
    assert "first_byte=yes" not in "\n".join(lines)


def test_gate_proof_accepts_wired_driver_task_proof_without_sdio(
    tmp_path: pathlib.Path,
) -> None:
    """Wired closure must not require the inactive SDIO/CYW43 path."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(_strong_wired_driver_task_proof_lines()),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--require-driver-task-proof",
            "--require-wired-ready",
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
    assert "DRIVER_TASK_ACTIVE_NET=genet" in result.stdout
    assert "DRIVER_TASK_OWNER_STATE_PROOF=yes" in result.stdout
    assert "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF=yes" in result.stdout
    assert "DRIVER_TASK_SDIO_DEDICATED=no" in result.stdout
    assert "DRIVER_TASK_DMA_PROOFS=5" in result.stdout
    assert "PI4_RUNTIME_DMA_PROOF=fresh-pi" in result.stdout


def test_gate_proof_accepts_wifi_selected_driver_task_proof_without_genet(
    tmp_path: pathlib.Path,
) -> None:
    """WiFi closure requires CYW43 plus SDIO, not inactive GENET proof."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(_strong_wifi_selected_driver_task_proof_lines()),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--require-driver-task-proof",
            "--expect",
            "DRIVER_TASK_ACTIVE_NET=cyw43",
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
    assert "DRIVER_TASK_ACTIVE_NET=cyw43" in result.stdout
    assert "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF=yes" in result.stdout
    assert "DRIVER_TASK_SDIO_DEDICATED=yes" in result.stdout
    assert "DRIVER_TASK_DMA_PROOFS=6" in result.stdout
    assert "PI4_RUNTIME_DMA_PROOF=fresh-pi" in result.stdout


def test_gate_proof_accepts_functional_usb_without_oldgood_receipt(
    tmp_path: pathlib.Path,
) -> None:
    """Current USB function does not depend on the dormant old-good receipt."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(_strong_driver_task_proof_lines()),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
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

    assert result.returncode == 0
    assert "USB_OLDGOOD_REPLAY=no" in result.stdout
    assert "USB_RUNTIME_QUEUE_VALID=yes" in result.stdout
    assert "USB_RUNTIME_QUEUED_REPORTS=1" in result.stdout
    assert "USB_PHYSICAL_INPUT_PROOF=yes" in result.stdout


def test_gate_proof_accepts_identity_bound_usb_oldgood_retained_pair(
    tmp_path: pathlib.Path,
) -> None:
    """The current adjacent runtime receipt closes only the old-good gate."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
                [
                    *_strong_driver_task_proof_lines(),
                    *_oldgood_usb_replay_lines()[-4:],
                    *_retained_usb_oldgood_lines(),
                ]
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
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

    assert result.returncode == 0
    assert "USB_OLDGOOD_REPLAY=yes" in result.stdout
    assert "USB_OLDGOOD_MISSING=none" in result.stdout


def test_gate_proof_ignores_uncommitted_dormant_usb_oldgood_receipt(
    tmp_path: pathlib.Path,
) -> None:
    """A torn passive receipt cannot override current functional USB proof."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    lines = [
        *_strong_driver_task_proof_lines(),
        *_oldgood_usb_replay_lines()[-4:],
        *(
            line.replace("commit=14", "commit=13")
            for line in _retained_usb_oldgood_lines()
        ),
    ]
    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text("\n".join(lines), encoding="utf-8")

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
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

    assert result.returncode == 0
    assert "USB_OLDGOOD_REPLAY=no" in result.stdout
    assert "USB_OLDGOOD_MISSING=retained-receipt-uncommitted" in result.stdout
    assert "USB_RUNTIME_QUEUE_VALID=yes" in result.stdout


def test_gate_proof_requires_usb_first_report_and_byte_for_ready(
    tmp_path: pathlib.Path,
) -> None:
    """Full USB readiness cannot substitute command admission for HID input."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    lines = [
        *(
            line.replace("first_report=yes", "first_report=no")
            .replace("first_byte=yes", "first_byte=no")
            .replace("first_byte_source=linked-runtime-hid", "first_byte_source=none")
            for line in _strong_driver_task_proof_lines()
            if not line.startswith("[local-seat] usb keyboard command-ready")
        ),
    ]
    log_path.write_text("\n".join(lines), encoding="utf-8")

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
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
    assert "USB_FIRST_REPORT_READY expected yes got no" in result.stderr


def test_gate_proof_accepts_ready_with_oldgood_replay_contracts(
    tmp_path: pathlib.Path,
) -> None:
    """Full ready proof requires both gates and linked old-good replay."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    lines = [
        *_strong_driver_task_proof_lines(),
        *_oldgood_usb_replay_lines(),
        *_oldgood_wifi_replay_lines(),
    ]
    log_path.write_text("\n".join(lines), encoding="utf-8")

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--require-ready",
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
    assert "USB_OLDGOOD_REPLAY=yes" in result.stdout
    assert "WIFI_OLDGOOD_REPLAY=yes" in result.stdout
    assert "WIFI_FIRMWARE_IDENTITY_PROOF=yes" in result.stdout
    assert "WIFI_CLM_READY_PROOF=yes" in result.stdout
    assert "WIFI_FIRMWARE_VERSION_PROOF=yes" in result.stdout
    assert "WIFI_CLM_VERSION_PROOF=yes" in result.stdout
    assert "SDIO_IRQ158_INBAND_PROOF=yes" in result.stdout
    assert "WIFI_DPC_PROOF=yes" in result.stdout
    assert "WIFI_GATE7_COMPLETE=yes" in result.stdout
    assert "WIFI_GATE7_SEEN=7a>7b>7c>7d>7e" in result.stdout
    assert "WIFI_GATE7_LAST=7e" in result.stdout
    assert "WIFI_GATE7_MISSING=none" in result.stdout


def test_gate_proof_rejects_wifi_ready_without_bootstrap_supervisor(
    tmp_path: pathlib.Path,
) -> None:
    """A Gate-10 replay cannot substitute for the boot supervisor terminal."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    lines = [
        line
        for line in [
            *_strong_driver_task_proof_lines(),
            *_oldgood_wifi_replay_lines(),
        ]
        if not line.startswith("CYW43_BOOTSTRAP_SUPERVISOR ")
    ]
    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text("\n".join(lines), encoding="utf-8")

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--require-wifi-ready",
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
    assert "CYW43_BOOTSTRAP_SUPERVISOR_SEEN=no" in result.stdout
    assert (
        "CYW43_BOOTSTRAP_SUPERVISOR_SEEN expected yes got no"
        in result.stderr
    )


def test_gate_proof_rejects_wifi_ready_with_incomplete_gate7_handshake(
    tmp_path: pathlib.Path,
) -> None:
    """Latest-subgate telemetry cannot hide a missing Gate 7 handshake step."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    lines = [
        line
        for line in [
            *_strong_driver_task_proof_lines(),
            *_oldgood_wifi_replay_lines(),
        ]
        if "kind=gtk" not in line
    ]
    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text("\n".join(lines), encoding="utf-8")

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--require-wifi-ready",
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
    assert "WIFI_SUBGATE=8h-data-admission" in result.stdout
    assert "WIFI_GATE7_COMPLETE=no" in result.stdout
    assert "WIFI_GATE7_SEEN=7a>7b>7c" in result.stdout
    assert "WIFI_GATE7_MISSING=7d" in result.stdout
    assert "WIFI_GATE7_COMPLETE expected yes got no" in result.stderr


def test_gate_proof_rejects_wifi_ready_without_dpc_proof(
    tmp_path: pathlib.Path,
) -> None:
    """Current WiFi acceptance must include exact healthy DPC accounting."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    log_path = tmp_path / "pi4-serial.log"
    lines = [
        line
        for line in [
            *_strong_driver_task_proof_lines(),
            *_oldgood_wifi_replay_lines(),
        ]
        if not _is_wifi_dpc_proof_line(line)
    ]
    log_path.write_text("\n".join(lines), encoding="utf-8")

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--require-wifi-ready",
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
    assert "WIFI_DPC_PROOF=no" in result.stdout
    assert "WIFI_DPC_REASON=missing" in result.stdout
    assert "WIFI_DPC_PROOF expected yes got no reason=missing" in result.stderr


def test_gate_proof_rejects_wifi_ready_with_zero_dpc_activity(
    tmp_path: pathlib.Path,
) -> None:
    """An exact but idle DPC line is not evidence that IRQ service worked."""

    venv_dir = REPO_ROOT / ".venv"
    if not (venv_dir / "bin" / "python").is_file():
        pytest.skip("current Python is not inside a venv-like directory")

    lines = [
        line
        for line in [
            *_strong_driver_task_proof_lines(),
            *_oldgood_wifi_replay_lines(),
        ]
        if not _is_wifi_dpc_proof_line(line)
    ]
    lines.append(
        "CYW43_SDIO_DPC generation=9 captures=0 published=0 consumed=0 "
        "rearms=0 overruns=0 epoch_errors=0 sequence_errors=0 "
        "ack_failures=0 owner_active=yes poisoned=no masked=no"
    )
    lines.extend(
        [
            "CYW43_SDIO_DPC_SCOPE captures=event-attempts published=ring-events "
            "poisoned=aggregate-client-or-ring source=card-int-or-source-probe "
            "physical_card_irq=not-exported",
            "CYW43_SDIO_DPC_TRUTH generation=9 owner_active=yes "
            "ring_poisoned=no client_sample_stale=no ring_consumer=0 "
            "sample_consumer=0 sample_reason=current authority=live-ring "
            "action=none",
        ]
    )
    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text("\n".join(lines), encoding="utf-8")

    result = subprocess.run(
        [
            str(SCRIPT_PATH),
            "--normalize-only",
            "--require-wifi-ready",
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
    assert "WIFI_DPC_PROOF=no" in result.stdout
    assert "WIFI_DPC_REASON=no-activity" in result.stdout
    assert "WIFI_DPC_PROOF expected yes got no reason=no-activity" in result.stderr


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
                "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
                "[cohesix:root-task] Cohesix boot: root-task online",
                "usb: linked_runtime command-probe result=enable-slot-ok",
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
                "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
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
