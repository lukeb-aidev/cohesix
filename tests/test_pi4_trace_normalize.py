# Author: Lukas Bower
# Purpose: Unit tests for scripts/pi4_trace_normalize.py Pi 4 USB/WiFi log normalization.
# Copyright 2026 Lukas Bower

"""Tests for scripts/pi4_trace_normalize.py."""

import importlib.util
import io
import json
import pathlib
import sys
from collections.abc import Callable

import pytest

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
    "wsec_key=ptk+gtk key_order=m4-after-wsec carrier=yes"
)


def healthy_wifi_dpc_triplet(
    *, generation: int = 9, captures: int = 6, rearms: int | None = None
) -> list[str]:
    """Return one canonical healthy accounting/scope/live-truth sample."""

    rearm_count = captures if rearms is None else rearms
    return [
        f"CYW43_SDIO_DPC generation={generation} captures={captures} "
        f"published={captures} consumed={captures} rearms={rearm_count} "
        "overruns=0 epoch_errors=0 sequence_errors=0 ack_failures=0 "
        "owner_active=yes poisoned=no masked=no",
        normalizer.CYW43_SDIO_DPC_SCOPE_LINE,
        f"CYW43_SDIO_DPC_TRUTH generation={generation} owner_active=yes "
        "ring_poisoned=no client_sample_stale=no "
        f"ring_consumer={captures} sample_consumer={captures} "
        "sample_reason=current authority=live-ring action=none",
    ]


def descriptor_seal_suffix(hot_path: str) -> str:
    """Return current runtime descriptor seal proof fields for a hot path."""

    bus_link_seal = (
        "valid" if hot_path in {"usb-keyboard", "cyw43-wifi", "sdio-host"} else "none"
    )
    return (
        "descriptor_version=8 descriptor_seal=valid "
        f"artifact_hash=nonzero bus_link_seal={bus_link_seal}"
    )


def seal_driver_task_runtime_descriptor_lines(lines: list[str]) -> list[str]:
    """Add sealed runtime descriptor proof to synthetic green fixtures."""

    sealed_lines: list[str] = []
    for line in lines:
        if (
            (
                line.startswith("DRIVER_TASK_OWNER_STATE ")
                or line.startswith("DRIVER_TASK_DMA_PROOF ")
            )
            and "descriptor=present" in line
            and "descriptor_version=" not in line
        ):
            hot_path = None
            for field_match in normalizer.KEY_VALUE_RE.finditer(line):
                if field_match.group("key") == "hot_path":
                    hot_path = field_match.group("value")
                    break
            if hot_path is not None:
                line = f"{line} {descriptor_seal_suffix(hot_path)}"
        sealed_lines.append(line)
    return sealed_lines


def strip_driver_task_runtime_descriptor_seals(lines: list[str]) -> list[str]:
    """Remove seal proof fields from a current fixture to model stale logs."""

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


def oldgood_usb_replay_lines() -> list[str]:
    """Return a synthetic linked-runtime USB old-good replay trace."""

    return seal_driver_task_runtime_descriptor_lines([
        "DRIVER_TASK_OWNER_STATE contract=usb-local-seat hot_path=usb-keyboard "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=pcie-root hot_path=pcie-root "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
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
        "usb: runtime_queue queue_valid=yes queued_reports=1 "
        "doorbell_pending=no preserved_events=0 transfer_events=1 "
        "report_status=produced-byte",
        "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
        "read=1 ascii=0x74 key=0x17",
        "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
        "first_byte_source=linked-runtime-hid proof_gate=10 target_gate=10 blocker=none",
    ])


def strict_wired_boot_proof_lines() -> list[str]:
    """Return a synthetic boot slice that satisfies strict Pi 4 proof gates."""

    return seal_driver_task_runtime_descriptor_lines([
        "U-Boot 2026.01-dirty",
        "Starting kernel ...",
        "[cohesix:root-task] Cohesix boot: root-task online",
        "[timers] backend=arch-counter counter=vct timer_freq_hz=54000000",
        "[Cohesix] Root console ready (type 'help' for commands)",
        "Cohesix console ready",
        "cohesix> ",
        "SERIAL_ECHO result=ok serial_responsive=yes",
        "USB_BURST bytes=256 drops=0 max_latency_us=900",
        "HDMI_RESPONSIVE max_gap_ms=9 mirrored_bytes=256",
        "DRIVER_TASK_DEFAULT requested=dedicated required=yes "
        "substrate_active=yes live_hot_paths=yes",
        "DRIVER_TASK_SELECTED profile=pi4-hardware selection=wired "
        "active_net=genet required_roles=0x2f required_hot_paths=0x4f "
        "required_tasks=5",
        "DRIVER_TASK_BOOT contract=serial role=serial started=yes affinity_core=1",
        "DRIVER_TASK_BOOT contract=usb-local-seat role=usb started=yes affinity_core=1",
        "DRIVER_TASK_BOOT contract=hdmi-text role=display started=yes affinity_core=1",
        "DRIVER_TASK_BOOT contract=bcmgenet-v5 role=net started=yes affinity_core=1",
        "DRIVER_TASK_BOOT contract=pcie-root role=pcie started=yes affinity_core=2",
        "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-hardware "
        "task_count=5 failed_count=0 live_tcb_count=5 "
        "root_authority=admission-descriptor-diagnostics-only "
        "hardware_owner=linked-runtime fault_endpoint_ready=yes "
        "revoke_ready=yes broad_caps_leaked=0 sched=yes "
        "affinity=per-driver affinity_configured=5 affinity_applied=5 "
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
        "DRIVER_TASK_OWNER_STATE contract=pcie-root hot_path=pcie-root "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=18",
        "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=22",
        "SCHED_CONTRACT contract=hdmi-text isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=44",
        "SCHED_CONTRACT contract=bcmgenet-v5 isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=31",
        "SCHED_CONTRACT contract=pcie-root isolation=dedicated-sel4-task "
        "live_tcb=yes hot_path=dedicated observed_service_us=36",
        "DRIVER_TASK_DMA_PROOF contract=serial hot_path=serial-console "
        "status=ready profile=bounded-no-iommu descriptor=present "
        "root_pointer=no owner=linked-runtime mmio_pages=0 dma_pages=0 "
        "shared_pages=4 bus_address_policy=zero-dma "
        "cache_policy=uncached-plus-root-maintenance proof_effect=runtime-dma-proof-ready",
        "DRIVER_TASK_DMA_PROOF contract=usb-local-seat hot_path=usb-keyboard "
        "status=ready profile=bounded-no-iommu descriptor=present "
        "root_pointer=no owner=linked-runtime mmio_pages=0 dma_pages=128 "
        "shared_pages=32 bus_address_policy=hal-bounded-bus-address "
        "cache_policy=uncached-plus-root-maintenance proof_effect=runtime-dma-proof-ready",
        "DRIVER_TASK_DMA_PROOF contract=hdmi-text hot_path=hdmi-text "
        "status=ready profile=bounded-no-iommu descriptor=present "
        "root_pointer=no owner=linked-runtime mmio_pages=0 dma_pages=0 "
        "shared_pages=16 bus_address_policy=zero-dma "
        "cache_policy=uncached-plus-root-maintenance proof_effect=runtime-dma-proof-ready",
        "DRIVER_TASK_DMA_PROOF contract=bcmgenet-v5 hot_path=genet-nic "
        "status=ready profile=bounded-no-iommu descriptor=present "
        "root_pointer=no owner=linked-runtime mmio_pages=6 dma_pages=64 "
        "shared_pages=32 bus_address_policy=hal-bounded-bus-address "
        "cache_policy=uncached-plus-root-maintenance proof_effect=runtime-dma-proof-ready",
        "DRIVER_TASK_DMA_PROOF contract=pcie-root hot_path=pcie-root "
        "status=ready profile=bounded-no-iommu descriptor=present "
        "root_pointer=no owner=linked-runtime mmio_pages=11 dma_pages=0 "
        "shared_pages=16 bus_address_policy=zero-dma "
        "cache_policy=uncached-plus-root-maintenance proof_effect=runtime-dma-proof-ready",
        "DRIVER_TASK_COUNTER contract=usb-local-seat hot_path=usb-keyboard "
        "source=root-ring sequence=1 submitted=2 completed=2 idle=0 fault=0 "
        "budget=0 frame=1 desc=1 staged_bytes=64 clean_ops=1 clean_bytes=64 "
        "inv_ops=1 inv_bytes=64 sends=2 yields=0 busy=0 same_request=0 "
        "timeouts=0 keep_active=0 aborts=0 overruns=0 drops=0 rx_frames=1 "
        "rx_bytes=8 tx_frames=1 tx_bytes=8 role_aux0=0 role_aux1=0 "
        "role_aux2=0 role_aux3=0",
        "DRIVER_TASK_ACCEPTANCE dedicated_ready=yes reason=active-substrate "
        "substrate=active capset=pass fault=pass revoke=pass sched=pass "
        "affinity=pass vspace=isolated ipc_abi=shared-ring-command "
        "pointer_free_ipc=yes owner_state=driver-owned required=5 "
        "dedicated=5 compatibility=0 active_net=genet live_hot_paths=yes",
        *oldgood_usb_replay_lines(),
        "OK NETTEST detail=pass scope=serial-local",
        "netstats: rx_pkts=4 tx_pkts=9 rx_used=4 tx_used=9 polls=30",
        "netstats: mode=dhcp policy=wired active=wired standby=wifi "
        "addr_src=dhcp-lease ip=192.168.10.50 gateway=192.168.10.1 dhcp=bound",
        "netstatus: ip=192.168.10.50 gateway=192.168.10.1 "
        "src=dhcp-lease dhcp=bound tcp_ready=yes",
    ])


def oldgood_wifi_replay_lines() -> list[str]:
    """Return a synthetic linked-runtime CYW43 old-good replay trace."""

    return seal_driver_task_runtime_descriptor_lines([
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=begin backoff_ms=0 "
        "next_attempt_ms=100 serial=ready local_seat=enabled recovery=full "
        "console_seq=1 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes",
        "DRIVER_TASK_OWNER_STATE contract=cyw43455 hot_path=cyw43-wifi "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=sdio-host hot_path=sdio-host "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "SDIO_DRIVER_TASK_REPLAY_STATUS stage=engine-init blocker=ready detail=0x5500",
        "wifi: cyw43-transport-ready owner=linked-runtime",
        "wifi: firmware_contract fw=609309 nvram=1744 clm=2676 "
        f"fw_hash={normalizer.CYW43_CAPTURE_FIRMWARE_SHA256} "
        f"nvram_hash={normalizer.CYW43_CAPTURE_NVRAM_SHA256} "
        f"clm_hash={normalizer.CYW43_CAPTURE_CLM_SHA256} "
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
        *wifi_gate8_snapshot_lines(
            len(normalizer.WIFI_GATE8_SUBGATES),
            pair_epoch=1,
            generation=9,
        ),
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
        "netstats: generation=9 mode=dhcp policy=wifi active=wifi standby=wired "
        "addr_src=dhcp-lease ip=192.168.10.50 gateway=192.168.10.1 dhcp=bound",
        "netstats: wifi_assoc=1 wifi_link=1 eapol_rx=2 eapol_start=1 eapol_secure=1",
        "netstatus: generation=9 ip=192.168.10.50 gateway=192.168.10.1 "
        "src=dhcp-lease dhcp=bound tcp_ready=yes",
        *healthy_wifi_dpc_triplet(),
    ])


def oldgood_wifi_line_index(lines: list[str], needle: str) -> int:
    """Return the first synthetic WiFi replay line index containing `needle`."""

    return next(index for index, line in enumerate(lines) if needle in line)


def retained_wifi_oldgood_receipt_lines() -> list[str]:
    """Return the exact compact owner and retained-prefix serial transaction."""

    return [
        "DRIVER_TASK_OWNER_STATE contract=serial hot_path=serial-console "
        "owner_state=driver-owned descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=none "
        "root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=usb-local-seat hot_path=usb-keyboard "
        "owner_state=driver-owned descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=valid "
        "root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=hdmi-text hot_path=hdmi-text "
        "owner_state=driver-owned descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=none "
        "root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=pcie-root hot_path=pcie-root "
        "owner_state=driver-owned descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=none "
        "root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=cyw43455 hot_path=cyw43-wifi "
        "owner_state=driver-owned descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=valid "
        "root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=sdio-host hot_path=sdio-host "
        "owner_state=driver-owned descriptor=present descriptor_version=8 "
        "descriptor_seal=valid artifact_hash=nonzero bus_link_seal=valid "
        "root_pointer=no",
        "WIFI_OLDGOOD_RETAINED_BEGIN id=1 attempt=1 pair_epoch=1 "
        "generation=9 prefix_steps=26 fw=609309 nvram=1744 clm=2676",
        "WIFI_OLDGOOD_RETAINED_HASH id=1 artifact=firmware "
        f"sha256={normalizer.CYW43_CAPTURE_FIRMWARE_SHA256}",
        "WIFI_OLDGOOD_RETAINED_HASH id=1 artifact=nvram "
        f"sha256={normalizer.CYW43_CAPTURE_NVRAM_SHA256}",
        "WIFI_OLDGOOD_RETAINED_HASH id=1 artifact=clm "
        f"sha256={normalizer.CYW43_CAPTURE_CLM_SHA256}",
        "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host stage=engine-init "
        "blocker=ready detail=0x5500",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=net-engine-init status=ready",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-firmware status=ready",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-function2 status=ready",
        "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
        "stage=cyw43-control-txglomalign event=pre-tx-drain-ready poll=0",
        "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
        "stage=cyw43-control-txglomalign event=tx-complete result=16",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-control-txglomalign status=ready",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-control-ulp-sdioctrl status=unsupported",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-control-rxglom status=ready",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-control-mac status=ready",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-control-revinfo status=ready",
        "CYW43_DRIVER_TASK_CLM contract=cyw43455 "
        "stage=cyw43-control-clmload action=ready index=2 offset=2676 "
        "len=2676 flags=0x0000",
        "CYW43_DRIVER_TASK_TEXT_IOVAR contract=cyw43455 "
        "stage=cyw43-control-firmware-version name=ver printable_len=48",
        "CYW43_DRIVER_TASK_TEXT_IOVAR contract=cyw43455 "
        "stage=cyw43-control-clm-version name=clmver printable_len=16",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-control-up status=ready",
        "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
        "path=association-supervisor action=ready generation=9 ssid_len=7 "
        "result=0x00000000",
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
        "status=required associated=yes link_up=yes assoc_event=assoc "
        "assoc_poll=2 eapol_rx=0",
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 msg=m1 "
        "action=recv-m1 poll=3 len=99",
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 msg=m2 "
        "action=send-m2 poll=4 len=121",
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 msg=m3 "
        "action=recv-m3 poll=5 len=151",
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 msg=m4 "
        "action=send-m4 poll=6 len=99",
        "CYW43_DRIVER_TASK_HOST_EAPOL_KEY contract=cyw43455 kind=ptk "
        "stage=cyw43-host-eapol-ptk status=ready",
        "CYW43_DRIVER_TASK_HOST_EAPOL_KEY contract=cyw43455 kind=gtk "
        "stage=cyw43-host-eapol-gtk status=ready",
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
        "status=secure associated=yes link_up=yes eapol_rx=2",
        "[dhcp] start ready interface=wifi generation=9 xid=0x01020304 "
        "now_ms=1000",
        "[dhcp] lease bound generation=9 ip=192.168.86.154/24 "
        "gateway=192.168.86.1 server=192.168.86.1 lease_s=3600",
        "WIFI_OLDGOOD_RETAINED_END id=1 attempt=1 pair_epoch=1 "
        "generation=9 prefix_steps=26 status=complete",
    ]


def retained_wifi_oldgood_replay_lines() -> list[str]:
    """Return a full Gate-10 trace using the new retained old-good prefix."""

    return [
        *oldgood_wifi_resource_replay_lines(),
        *retained_wifi_oldgood_receipt_lines(),
        "netstats: rx_pkts=4 tx_pkts=9 rx_used=4 tx_used=9 polls=30",
        "netstats: generation=9 udp_rx=2 udp_tx=4 tcp_accepts=1 "
        "tcp_auth=1 tcp_rx_bytes=58 tcp_tx_bytes=6782",
        "netstats: generation=9 mode=dhcp policy=wifi active=wifi standby=wired "
        "addr_src=dhcp-lease ip=192.168.86.154 gateway=192.168.86.1 "
        "dhcp=bound",
        "netstats: wifi_assoc=1 wifi_link=1 eapol_rx=2 eapol_start=1 "
        "eapol_secure=1",
        "netstatus: generation=9 ip=192.168.86.154 "
        "gateway=192.168.86.1 src=dhcp-lease dhcp=bound tcp_ready=yes",
        "nettest: generation=9 run_generation=2 enabled=true running=false "
        "verdict=peer-assisted-pass tx_ok=true udp_echo_ok=false tcp_ok=false "
        "console_ok=true peer_assisted_ok=true",
        *healthy_wifi_dpc_triplet(generation=9),
    ]


def oldgood_usb_resource_replay_lines() -> list[str]:
    """Return linked USB old-good replay using resource-init breadcrumbs."""

    return [
        "DRIVER_TASK_OWNER_STATE contract=usb-local-seat hot_path=usb-keyboard "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=pcie-root hot_path=pcie-root "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
        "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat hot_path=usb-keyboard "
        "stage=usb-engine-init status=ready",
        "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat hot_path=usb-keyboard "
        "stage=usb-keyboard-enumeration status=command-ring-ready",
        "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat hot_path=usb-keyboard "
        "stage=usb-keyboard-enumeration status=root-port-connected",
        "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat hot_path=usb-keyboard "
        "stage=usb-keyboard-enumeration status=device-addressed",
        "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat hot_path=usb-keyboard "
        "stage=usb-keyboard-enumeration status=hub-set-configuration-done",
        "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat hot_path=usb-keyboard "
        "stage=usb-keyboard-enumeration status=hub-context-done",
        "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat hot_path=usb-keyboard "
        "stage=usb-keyboard-enumeration status=hub-port-power-done",
        "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat hot_path=usb-keyboard "
        "stage=usb-keyboard-enumeration status=hub-port-status-done",
        "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat hot_path=usb-keyboard "
        "stage=usb-keyboard-enumeration status=hub-port-ready",
        "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat hot_path=usb-keyboard "
        "stage=usb-keyboard-enumeration status=hub-child-probe-begin",
        "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat hot_path=usb-keyboard "
        "stage=usb-keyboard-enumeration status=hid-endpoint-ready",
        "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat hot_path=usb-keyboard "
        "stage=usb-keyboard-interrupt-in status=ready",
        "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat hot_path=usb-keyboard "
        "stage=usb-keyboard-first-report status=ready",
        "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no "
        "clean_polls=2 no_reply=0 recovery_pending=no",
        "[local-seat] runtime keyboard first-byte source=linked-runtime-hid read=1 "
        "ascii=0x74 detail=0x0501 result=0x00000001",
        "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
        "first_byte_source=linked-runtime-hid proof_gate=10 target_gate=10 blocker=none",
    ]


def retained_usb_oldgood_receipt_lines() -> list[str]:
    """Return one exact adjacent current USB runtime receipt pair."""

    return [
        "USB_OLDGOOD_RETAINED v=1 task=12 token=0xdeadbeef "
        "link_epoch=7 link_token=0xc001d00d epoch=3 seq=14 "
        "mask=0x00003fff topology=0x10230581 input_gen=9 commit=14 "
        "source=linked-runtime-hid",
        "USB_OLDGOOD_CURRENT contracts=usb-local-seat+pcie-root "
        "owners=driver-owned+driver-owned descriptors=sealed+sealed "
        "command_ready=yes proof_gate=14 blocker=none root_pointer=no",
    ]


def oldgood_wifi_resource_replay_lines() -> list[str]:
    """Return linked CYW43 old-good replay using resource-init breadcrumbs."""

    return seal_driver_task_runtime_descriptor_lines([
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=begin backoff_ms=0 "
        "next_attempt_ms=100 serial=ready local_seat=enabled recovery=full "
        "console_seq=1 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes",
        "DRIVER_TASK_OWNER_STATE contract=cyw43455 hot_path=cyw43-wifi "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=sdio-host hot_path=sdio-host "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host stage=engine-init blocker=ready detail=0x5500",
        "wifi: firmware_contract fw=609309 nvram=1744 clm=2676 "
        f"fw_hash={normalizer.CYW43_CAPTURE_FIRMWARE_SHA256} "
        f"nvram_hash={normalizer.CYW43_CAPTURE_NVRAM_SHA256} "
        f"clm_hash={normalizer.CYW43_CAPTURE_CLM_SHA256} "
        "board=raspberrypi,4-model-b rstvec=0xb83ef198 verified=yes "
        "armcr4_release=1 sr_kso=yes current_clock=41666666Hz preferred=41666666Hz",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=net-engine-init status=ready",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-firmware status=ready",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-function2 status=ready",
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
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-control-txglomalign status=ready",
        "CYW43_DRIVER_TASK_CONTROL_REPLY contract=cyw43455 "
        "stage=cyw43-control-txglomalign event=matched-reply "
        "status=0x00000000 reply_match=yes",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-control-ulp-sdioctrl status=unsupported",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-control-rxglom status=ready",
        "CYW43_DRIVER_TASK_CONTROL_REPLY contract=cyw43455 stage=cyw43-control-rxglom "
        "event=matched-reply status=0x00000000 reply_match=yes",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-control-mac status=ready",
        "CYW43_DRIVER_TASK_CONTROL_REPLY contract=cyw43455 "
        "stage=cyw43-control-mac event=matched-reply status=0x00000000 "
        "reply_match=yes",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-control-revinfo status=ready",
        "CYW43_DRIVER_TASK_CONTROL_REPLY contract=cyw43455 stage=cyw43-control-revinfo "
        "event=matched-reply status=0x00000000 reply_match=yes",
        "CYW43_DRIVER_TASK_CLM contract=cyw43455 stage=cyw43-control-clmload "
        "action=ready index=2 offset=2676 len=2676 flags=0x0000",
        "CYW43_DRIVER_TASK_TEXT_IOVAR contract=cyw43455 "
        "stage=cyw43-control-firmware-version name=ver printable_len=48",
        "CYW43_DRIVER_TASK_TEXT_IOVAR contract=cyw43455 "
        "stage=cyw43-control-clm-version name=clmver printable_len=16",
        "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
        "stage=cyw43-control-up status=ready",
        "CYW43_DRIVER_TASK_CONTROL_REPLY contract=cyw43455 stage=cyw43-control-up "
        "event=matched-reply status=0x00000000 reply_match=yes",
        "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
        "path=primary-bsscfg:join action=ready ssid_len=7 result=0x00000000",
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 status=required "
        "associated=yes link_up=yes assoc_event=link-up eapol_rx=0",
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 msg=m1 action=recv-m1 poll=12 len=121",
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 msg=m2 action=send-m2 poll=12 len=121",
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 msg=m3 action=recv-m3 poll=15 len=151",
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 msg=m4 action=send-m4 poll=15 len=95",
        "CYW43_DRIVER_TASK_HOST_EAPOL_KEY contract=cyw43455 kind=ptk "
        "stage=cyw43-host-eapol-ptk status=ready",
        "CYW43_DRIVER_TASK_HOST_EAPOL_KEY contract=cyw43455 kind=gtk "
        "stage=cyw43-host-eapol-gtk status=ready",
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 status=secure "
        "associated=yes link_up=yes eapol_rx=2",
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=stabilizing backoff_ms=0 "
        "next_attempt_ms=150 serial=ready local_seat=enabled recovery=full "
        "console_seq=2 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes",
        *wifi_gate8_snapshot_lines(
            len(normalizer.WIFI_GATE8_SUBGATES),
            pair_epoch=1,
            generation=9,
        ),
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
        *healthy_wifi_dpc_triplet(),
    ])


def linked_runtime_wifi_harness_replay_lines() -> list[str]:
    """Return the offline linked-runtime WiFi replay harness proof trace."""

    lines = oldgood_wifi_resource_replay_lines()
    join_index = next(
        index
        for index, line in enumerate(lines)
        if line.startswith("CYW43_DRIVER_TASK_JOIN_REQUEST")
    )
    return [
        *lines[: join_index + 1],
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
        "status=event-rx reason=host-eapol-required polls=1 starts=0 "
        "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 event_rx=1 "
        "control_rx=0 empty_polls=0 associated=no link_up=no assoc_event=none "
        "assoc_poll=0 post_assoc_polls=0 next_action=inspect-cyw43-join-event-state",
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
        "status=event-rx reason=host-eapol-required polls=2 starts=0 "
        "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 event_rx=2 "
        "control_rx=0 empty_polls=0 associated=yes link_up=yes assoc_event=link-up "
        "assoc_poll=2 post_assoc_polls=0 next_action=send-eapol-start-or-wait-m1",
        *lines[join_index + 1 :],
    ]


def pi4_hardware_wifi_gate7_to_10_capture_lines() -> list[str]:
    """Return the captured Pi 4 WiFi Gate 7-10 proof sequence."""

    lines = oldgood_wifi_resource_replay_lines()
    join_index = next(
        index
        for index, line in enumerate(lines)
        if line.startswith("CYW43_DRIVER_TASK_JOIN_REQUEST")
    )
    return [
        *lines[: join_index + 1],
        "[cyw43] event type=3 flags=0x0000 status=0x00000000 "
        "reason=0x00000000 auth=0x00000000 addr=f0-72-ea-4c-c7-a5 "
        "src=8a-a2-9e-66-59-10",
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
        "status=event-rx reason=host-eapol-required polls=704 starts=0 "
        "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 event_rx=1 "
        "control_rx=1 empty_polls=703 associated=no link_up=no assoc_event=none "
        "assoc_poll=0 post_assoc_polls=0 next_action=inspect-cyw43-join-event-state",
        "[cyw43] host-eapol ap-mac seed source=event-addr-global event=assoc "
        "mac=f0-72-ea-4c-c7-a5 event_addr=f0-72-ea-4c-c7-a5 "
        "event_src=8a-a2-9e-66-59-10",
        "[cyw43] event type=7 flags=0x0000 status=0x00000000 "
        "reason=0x00000000 auth=0x00000000 addr=f0-72-ea-4c-c7-a5 "
        "src=8a-a2-9e-66-59-10",
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
        "status=event-rx reason=host-eapol-required polls=705 starts=0 "
        "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 event_rx=2 "
        "control_rx=2 empty_polls=704 associated=yes link_up=no assoc_event=assoc "
        "assoc_poll=705 post_assoc_polls=0 next_action=inspect-cyw43-data-rx-path",
        "[cyw43] event type=16 flags=0x0001 status=0x00000000 "
        "reason=0x00000000 auth=0x00000000 addr=f0-72-ea-4c-c7-a5 "
        "src=8a-a2-9e-66-59-10",
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
        "status=event-rx reason=host-eapol-required polls=706 starts=0 "
        "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 event_rx=3 "
        "control_rx=3 empty_polls=704 associated=yes link_up=yes assoc_event=assoc "
        "assoc_poll=705 post_assoc_polls=1 next_action=send-eapol-start-or-wait-m1",
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
        "msg=m1 action=recv-m1 poll=710 len=113",
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
        "msg=m2 action=send-m2 poll=710 len=135",
        "[cyw43] event type=0 flags=0x0000 status=0x00000000 "
        "reason=0x00000000 auth=0x00000000 addr=f0-72-ea-4c-c7-a5 "
        "src=8a-a2-9e-66-59-10",
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
        "msg=m3 action=recv-m3 poll=715 len=169",
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
        "msg=m4 action=send-m4 poll=715 len=113",
        "CYW43_DRIVER_TASK_HOST_EAPOL_KEY contract=cyw43455 kind=ptk "
        "stage=cyw43-host-eapol-ptk status=ready",
        "CYW43_DRIVER_TASK_HOST_EAPOL_KEY contract=cyw43455 kind=gtk "
        "stage=cyw43-host-eapol-gtk status=ready",
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 status=secure "
        "reason=none polls=716 starts=0 tx_retries=0 data_rx=2 eapol_rx=2 "
        "non_eapol_rx=0 event_rx=4 control_rx=3 empty_polls=704 associated=yes "
        "link_up=yes assoc_event=assoc assoc_poll=705 post_assoc_polls=5 "
        "next_action=release-dhcp-data",
        JOIN_COMPLETE_HOST_EAPOL,
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=stabilizing backoff_ms=0 "
        "next_attempt_ms=150 serial=ready local_seat=enabled recovery=full "
        "console_seq=2 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes",
        *wifi_gate8_snapshot_lines(
            len(normalizer.WIFI_GATE8_SUBGATES),
            pair_epoch=1,
            generation=9,
        ),
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=ready backoff_ms=0 "
        "next_attempt_ms=200 serial=ready local_seat=enabled recovery=full "
        "console_seq=3 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes",
        "[dhcp] start ready interface=wifi now_ms=0",
        "[dhcp] tx queued kind=discover from=selecting to=selecting "
        "len=259 attempts=1 tx_packets=1",
        "[dhcp] rx transition from=selecting to=requesting action=send-queued "
        "len=300 attempts=0 rx_packets=1 invalid=0",
        "[dhcp] tx queued kind=request from=requesting to=requesting "
        "len=270 attempts=1 tx_packets=2",
        "[dhcp] rx ack ip=192.168.86.154 phase=bound len=300 rx_packets=2",
        "[dhcp] lease bound ip=192.168.86.154/24 gateway=192.168.86.1 "
        "server=192.168.86.1 lease_s=86400",
        "[net-selftest] result generation=9 run_generation=1 "
        "tx_ok=true udp_echo_ok=false "
        "tcp_ok=false console_ok=true peer_assisted_ok=true "
        "result=peer-assisted-pass",
        "OK NETTEST detail=started run_generation=1",
        "nettest: generation=9 run_generation=1 enabled=true running=false "
        "verdict=peer-assisted-pass tx_ok=true udp_echo_ok=false "
        "tcp_ok=false console_ok=true peer_assisted_ok=true",
        "netstats: rx_pkts=590 tx_pkts=141 rx_used=590 tx_used=141 polls=9831",
        "netstats: generation=9 udp_rx=2 udp_tx=27 tcp_accepts=4 tcp_auth=4 "
        "tcp_rx_bytes=320 tcp_tx_bytes=11287",
        "netstats: mode=dhcp policy=wifi active=wifi standby=none "
        "addr_src=dhcp-lease ip=192.168.86.154 gateway=192.168.86.1 dhcp=bound",
        "netstats: wifi_assoc=1 wifi_link=1 eapol_rx=2 eapol_start=0 eapol_secure=1",
        "netstatus: generation=9 ip=192.168.86.154 gateway=192.168.86.1 "
        "src=dhcp-lease dhcp=bound tcp_ready=yes",
        *healthy_wifi_dpc_triplet(captures=16),
    ]


def test_wifi_password_prompt_redacts_typed_secret() -> None:
    """Current and historical U-Boot prompts must not leak typed passwords."""

    current_prompt = (
        "Wi-Fi password (leave blank for an open network): "
        "correct-horse-battery-staple"
    )
    historical_prompt = "Wi-Fi PSK (blank for open network): legacy-secret"

    current_redacted = normalizer.redact_sensitive_line(current_prompt)
    historical_redacted = normalizer.redact_sensitive_line(historical_prompt)

    assert current_redacted == (
        "Wi-Fi password (leave blank for an open network): <redacted>"
    )
    assert "correct-horse-battery-staple" not in current_redacted
    assert historical_redacted == (
        "Wi-Fi PSK (blank for open network): <redacted>"
    )
    assert "legacy-secret" not in historical_redacted


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


def test_uboot_wifi_policy_missing_line_is_wifi_policy_evidence() -> None:
    event = normalizer.parse_line(
        "[cohesix] U-Boot policy missing: cohesix.env has no saved Wi-Fi credentials",
        21,
    )

    assert event is not None
    assert event.domain == "wifi"
    assert event.source == "uboot"
    assert event.message == (
        "U-Boot policy missing: cohesix.env has no saved Wi-Fi credentials"
    )


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


@pytest.mark.parametrize("diagnostic_version", [4, 5, 6])
def test_direct_genet_netstats_rows_are_preserved_as_driver_diagnostics(
    diagnostic_version: int,
) -> None:
    rows = [
        "netstats: genet_direct refresh=fresh snapshot=present "
        "phase=pre-idle-service generation=7 sequence=42",
        "netstats: genet_direct_flags flags=0x000001c3 initialized=yes "
        "active=yes faulted=no irq_pending=no rx_pending=no tx_pending=no",
        "netstats: genet_direct_before sequence=41 irq_wakes=8 irq_acks=8 "
        "raw=0x00000000 mask=0xffffffff active=0x00000000 rdma=5/5 tdma=3/3",
        "netstats: genet_direct_before_ring rx_cursor=12/12 tx_cursor=9/9 "
        "rx_packets=12 tx_packets=9 peer_wakes=4 peer_signals=6",
        "netstats: genet_direct_irq badge=0x00000400 wakes=9 acks=9 "
        "ack_failures=0 unmask_failures=0",
        "netstats: genet_direct_irq_source raw=0x00000000 mask=0xffffffff "
        "active=0x00000000 last=0x00000000",
        "netstats: genet_direct_notification receipts=13 rejected=1 "
        "badge_or=0x00000500",
        "netstats: genet_direct_dpc turns=9 budget_hits=0 final_rechecks=9 "
        "level_adoptions=0 mcs_quantum_high_us=731 mcs_reasons=0x00000000",
        "netstats: genet_direct_dma rdma_prod=5 rdma_cons=5 tdma_prod=3 "
        "tdma_cons=3 rx_packets=12 tx_packets=9",
        "netstats: genet_direct_ring rx_prod=12 rx_cons=12 tx_prod=9 "
        "tx_cons=9 rx_valid=yes tx_valid=yes state_changes=0",
        "netstats: genet_direct_peer wakes=4 signals=6 poison_rx=0/0 "
        "poison_tx=0/0",
    ]

    if diagnostic_version >= 5:
        rows.extend([
            'netstats: genet_direct_slice sample=present turn=9 direction=rx stages=0x0000003f flags=0x00000005 clock=cntvct timer_hz=54000000',
            'netstats: genet_direct_slice_begin began_ticks=10 source_ready_ticks=20 packet_done_ticks=30',
            'netstats: genet_direct_slice_end irq_done_ticks=35 finished_ticks=40 rx_publication_ticks=25',
            'netstats: genet_direct_slice_packet rx_cursor=12 tx_cursor=0 frame_len=60',
            'netstats: genet_direct_slice_tcp present=yes src=192.0.2.1:0 dst=198.51.100.2:31337 seq=0x12345678 ack=0x9abcdef0 flags=0x118',
        ])
    if diagnostic_version == 6:
        rows[11] = rows[11].replace("0x0000003f", "0x000001ff").replace(
            "0x00000005", "0x0000000d"
        )
        rows.append(
            "netstats: genet_direct_slice_rx notify_due=yes "
            "signal_enter_ticks=26 signal_return_ticks=27 retired_ticks=28"
        )
    events = normalizer.parse_events(rows)

    assert len(events) == {4: 11, 5: 16, 6: 17}[diagnostic_version]
    assert [event.raw for event in events] == rows
    assert all(event.domain == "driver" for event in events)
    assert all(event.source == "cohesix" for event in events)
    assert events[0].fields == {
        "refresh": "fresh",
        "snapshot": "present",
        "phase": "pre-idle-service",
        "generation": "7",
        "sequence": "42",
    }
    assert events[1].fields["active"] == "yes"
    assert events[10].fields["poison_tx"] == "0/0"
    if diagnostic_version >= 5:
        assert events[11].fields["clock"] == "cntvct"
        assert events[11].fields["timer_hz"] == "54000000"
        assert events[13].fields["rx_publication_ticks"] == "25"
        assert events[15].fields["src"] == "192.0.2.1:0"

    if diagnostic_version == 6:
        assert events[16].fields == {
            "notify_due": "yes",
            "signal_enter_ticks": "26",
            "signal_return_ticks": "27",
            "retired_ticks": "28",
        }


@pytest.mark.parametrize("include_truncated_slice", [False, True])
def test_direct_genet_rows_have_no_network_or_acceptance_authority(
    include_truncated_slice: bool,
) -> None:
    legacy_rows = [
        "netstats: generation=7 mode=dhcp policy=wired active=wired "
        "standby=none addr_src=dhcp-lease ip=192.168.10.50 "
        "gateway=192.168.10.1 dhcp=bound",
    ]
    incomplete_rows = [
        "netstats: genet_direct refresh=fresh snapshot=present "
        "phase=pre-idle-service generation=7 sequence=42",
        "netstats: genet_direct_flags flags=0x000001c3 active=yes [truncated]",
        "netstats: genet_direct_before sequence=41 irq_wakes=8",
    ]
    if include_truncated_slice:
        incomplete_rows.extend([
            "netstats: genet_direct_slice sample=present turn=9 direction=rx "
            "stages=0x0000003f flags=0x00000005 clock=cntvct timer_hz=54000000",
            "netstats: genet_direct_slice_begin began_ticks=10 [truncated]",
            "netstats: genet_direct_slice_tcp present=yes src=192.0.2.1:0 "
            "dst=198.51.100.2:31337 status=ready gate=10 [truncated]",
            "netstats: genet_direct_slice_rx notify_due=yes "
            "signal_enter_ticks=26 retired_ticks=28 [truncated]",
        ])
    legacy = normalizer.summarize_gates(
        normalizer.parse_events(legacy_rows)
    ).to_record()
    augmented_events = normalizer.parse_events([*legacy_rows, *incomplete_rows])
    augmented = normalizer.summarize_gates(augmented_events).to_record()

    assert all(event.domain == "driver" for event in augmented_events[1:])
    for key in (
        "NET_ACTIVE",
        "NET_ADDR_SRC",
        "NET_DHCP",
        "NET_TCP_READY",
        "NETTEST_PROOF",
        "COHSH_TCP_AUTH_PROOF",
        "WIFI_GATE",
        "WIFI_BLOCKER",
    ):
        assert augmented[key] == legacy[key]
    assert augmented["NET_ACTIVE"] == "wired"
    assert augmented["NET_TCP_READY"] == "no"
    assert augmented["NETTEST_PROOF"] == "no"
    assert normalizer.boot_evidence_blockers(augmented) == (
        normalizer.boot_evidence_blockers(legacy)
    )


def test_pair_handoff_rows_are_payload_not_boot_acceptance() -> None:
    """A complete-looking first-child trace cannot manufacture boot gates."""
    rows = [
        "wifi: pair_handoff v=1 scope=first-pre-fence role=cyw43 "
        "observed=no cause=unavailable-or-unstable",
        "wifi: pair_handoff v=1 scope=first-pre-fence role=sdio "
        "observed=yes request=eff090d9 parent=00000002 gen=43595301 "
        "route=31 stages=01ff detail=00000100 witness=00000001 "
        "ticks=00000036 revision=9",
    ]
    events = normalizer.parse_events(rows)
    assert [event.raw for event in events] == rows
    assert all(event.domain == "wifi" for event in events)
    assert all(event.fields["diagnostic"] == "passive-first-child" for event in events)
    assert normalizer.summarize_gates(events).to_record() == (
        normalizer.summarize_gates([]).to_record()
    )
    malformed = normalizer.parse_events([
        "wifi: pair_handoff v=99 observed=yes status=ready gate=10 detail=fault",
        "wifi: pair_handoff",
    ])
    assert normalizer.summarize_gates(malformed).to_record() == (
        normalizer.summarize_gates([]).to_record()
    )


def test_compact_cyw43_bus_episode_is_anchored_and_passive() -> None:
    line = (
        "CYW43_BUS_EPISODE p=ffffffff e=ffffffff lg=ffffffff pe=ffffffff "
        "pa=ffffffff/0007 c=3 f=fffffffffffffffe l=ffffffffffffffff "
        "ch=ffffffff/ffff/ffff/ffffffff hw=0003/0003 d=ffffffff "
        "o8=ffffffff r=ffffffff t=ffffffff q=0000001f "
        "er=4/ffff/ffffffff fl=0000003f"
    )

    parsed = normalizer.parse_cyw43_bus_episode(line)
    assert parsed is not None
    assert parsed["diagnostic"] == "cyw43-bus-episode"
    assert parsed["diagnostic_schema"] == "compact-v1"
    assert parsed["publication_sequence"] == "ffffffff"
    assert parsed["episode_sequence"] == "ffffffff"
    assert parsed["logical_generation"] == "ffffffff"
    assert parsed["physical_epoch"] == "ffffffff"
    assert parsed["parent_sequence"] == "ffffffff"
    assert parsed["parent_op"] == "0007"
    assert parsed["cause_code"] == "3"
    assert parsed["first_cntvct"] == "fffffffffffffffe"
    assert parsed["last_cntvct"] == "ffffffffffffffff"
    assert parsed["child_sequence"] == "ffffffff"
    assert parsed["child_code"] == "ffff"
    assert parsed["child_detail"] == "ffff"
    assert parsed["child_result"] == "ffffffff"
    assert parsed["child_engine"] == "0003"
    assert parsed["child_irq_contract"] == "0003"
    assert parsed["dpc_sequence"] == "ffffffff"
    assert parsed["op8_progress"] == "ffffffff"
    assert parsed["rx_progress"] == "ffffffff"
    assert parsed["tx_progress"] == "ffffffff"
    assert parsed["pending_mask"] == "0000001f"
    assert parsed["exit_reason"] == "4"
    assert parsed["exit_detail"] == "ffff"
    assert parsed["exit_result"] == "ffffffff"
    assert parsed["flags"] == "0000003f"
    malformed_line = f"{line} trailing=yes"
    assert normalizer.parse_cyw43_bus_episode(malformed_line) is None

    event = normalizer.parse_events([line])[0]
    assert event.domain == "wifi"
    assert event.stage == "cyw43-bus-episode"
    assert event.message == "bus-episode diagnostic=passive"
    assert event.fields["diagnostic"] == "cyw43-bus-episode"

    record = normalizer.summarize_gates([event]).to_record()
    empty = normalizer.summarize_gates([]).to_record()
    for key in (
        "WIFI_GATE",
        "WIFI_BLOCKER",
        "WIFI_DPC_PROOF",
        "WIFI_DPC_REASON",
        "WIFI_GATE8_COMPLETE",
    ):
        assert record[key] == empty[key]

    malformed_event = normalizer.parse_events([malformed_line])[0]
    malformed_record = normalizer.summarize_gates([malformed_event]).to_record()
    for key in (
        "WIFI_GATE",
        "WIFI_BLOCKER",
        "WIFI_DPC_PROOF",
        "WIFI_DPC_REASON",
        "WIFI_GATE8_COMPLETE",
    ):
        assert malformed_record[key] == empty[key]


def dpc_child_timing_lines() -> list[str]:
    return [
        (
            "CYW43_DPC_CHILD_TIMING v=1 pe=00000005 e=0000001d "
            "src=000003e8 q=00000578 qc=00000007 len=128 n=2 "
            "fl=00000001 s2q=0 max=0 ovf=0 unk=0 tail_us=60"
        ),
        (
            "CYW43_DPC_CHILD_TIMING_ENTRY i=0 seq=00000029 a=01 k=02 "
            "ph=03 eng=01 vf=1f "
            "ts=0000044c/00000460/0000047e/000004b0/000004c4 "
            "pre_us=100 p2n_us=20 n2i_us=30 i2t_us=50 t2a_us=20"
        ),
        (
            "CYW43_DPC_CHILD_TIMING_ENTRY i=1 seq=0000002a a=04 k=05 "
            "ph=04 eng=02 vf=1f "
            "ts=000004e2/000004ec/00000500/00000528/0000053c "
            "pre_us=30 p2n_us=10 n2i_us=20 i2t_us=40 t2a_us=20"
        ),
    ]


def test_cyw43_dpc_child_timing_is_anchored_typed_and_passive() -> None:
    lines = dpc_child_timing_lines()
    header = normalizer.parse_cyw43_dpc_child_timing(lines[0])
    first = normalizer.parse_cyw43_dpc_child_timing_entry(lines[1])
    assert header is not None
    assert header["diagnostic"] == "cyw43-dpc-child-timing"
    assert header["physical_epoch"] == "00000005"
    assert header["source_cntvct"] == "000003e8"
    assert header["source_to_queue_us"] == "0"
    assert first is not None
    assert first["diagnostic"] == "cyw43-dpc-child-timing-entry"
    assert first["intake_cntvct"] == "00000460"
    assert first["publish_to_intake_us"] == "20"
    assert first["intake_to_issue_us"] == "30"
    assert normalizer.parse_cyw43_dpc_child_timing(f"{lines[0]} trailing=yes") is None
    assert (
        normalizer.parse_cyw43_dpc_child_timing_entry(f"{lines[1]} trailing=yes")
        is None
    )

    events = normalizer.parse_events(lines)
    assert [event.stage for event in events] == [
        "cyw43-dpc-child-timing",
        "cyw43-dpc-child-timing-entry",
        "cyw43-dpc-child-timing-entry",
    ]
    timing = normalizer.summarize_cyw43_dpc_child_timing(events)
    assert timing.status == "complete"
    assert timing.reason == "none"
    assert timing.version == 1
    assert timing.physical_epoch == 5
    assert timing.event_sequence == 29
    assert timing.source_cntvct == "0x000003e8"
    assert timing.queue_cntvct == "0x00000578"
    assert timing.queue_commit_sequence == 7
    assert timing.data_len == 128
    assert timing.count == 2
    assert timing.observed_entries == 2
    assert timing.flags == "0x00000001"
    assert timing.s2q_us == 0
    assert timing.max_us == 0
    assert timing.source_to_publish_us == 100
    assert timing.publish_to_intake_us == 30
    assert timing.intake_to_issue_us == 50
    assert timing.issue_to_terminal_us == 90
    assert timing.terminal_to_accept_us == 40
    assert timing.between_child_us == 30
    assert timing.dominant_seam == "cyw43-source-to-publish"
    assert timing.tail_us == 60

    record = normalizer.summarize_gates(events).to_record()
    empty = normalizer.summarize_gates([]).to_record()
    assert record["WIFI_DPC_CHILD_TIMING_STATUS"] == "complete"
    assert record["WIFI_DPC_CHILD_TIMING_S2Q_US"] == 0
    assert record["WIFI_DPC_CHILD_TIMING_MAX_US"] == 0
    assert record["WIFI_DPC_CHILD_TIMING_SOURCE_TO_PUBLISH_US"] == 100
    assert record["WIFI_DPC_CHILD_TIMING_PUBLISH_TO_INTAKE_US"] == 30
    assert record["WIFI_DPC_CHILD_TIMING_INTAKE_TO_ISSUE_US"] == 50
    assert record["WIFI_DPC_CHILD_TIMING_ISSUE_TO_TERMINAL_US"] == 90
    assert record["WIFI_DPC_CHILD_TIMING_TERMINAL_TO_ACCEPT_US"] == 40
    assert record["WIFI_DPC_CHILD_TIMING_BETWEEN_CHILD_US"] == 30
    assert (
        record["WIFI_DPC_CHILD_TIMING_DOMINANT_SEAM"]
        == "cyw43-source-to-publish"
    )
    for key in (
        "WIFI_GATE",
        "WIFI_BLOCKER",
        "WIFI_DPC_PROOF",
        "WIFI_DPC_REASON",
        "WIFI_GATE8_COMPLETE",
    ):
        assert record[key] == empty[key]


def test_cyw43_dpc_child_timing_inexact_missing_or_mismatched_is_unknown() -> None:
    lines = dpc_child_timing_lines()
    variants = [
        [
            lines[0].replace("fl=00000001", "fl=00000003").replace(
                "ovf=0", "ovf=1"
            ),
            *lines[1:],
        ],
        lines[:2],
        [lines[0], lines[1], lines[2].replace("i=1", "i=0")],
        [lines[0], lines[1].replace("vf=1f", "vf=0f"), lines[2]],
        [lines[0].replace("max=0", "max=1"), *lines[1:]],
        [lines[0], lines[1].replace("k=02", "k=07"), lines[2]],
        [
            lines[0],
            lines[1].replace("0000044c/", "8000044c/"),
            lines[2],
        ],
    ]
    expected_reasons = (
        "inexact",
        "entry-count-mismatch",
        "entry-index-mismatch",
        "entry-incomplete",
        "maximum-mismatch",
        "entry-type-mismatch",
        "wrap-ambiguous",
    )
    for variant, expected_reason in zip(variants, expected_reasons, strict=True):
        events = normalizer.parse_events(variant)
        timing = normalizer.summarize_cyw43_dpc_child_timing(events)
        assert timing.status == "UNKNOWN"
        assert timing.reason == expected_reason
        assert timing.s2q_us == "UNKNOWN"
        assert timing.max_us == "UNKNOWN"
        assert timing.source_to_publish_us == "UNKNOWN"
        assert timing.publish_to_intake_us == "UNKNOWN"
        assert timing.intake_to_issue_us == "UNKNOWN"
        assert timing.issue_to_terminal_us == "UNKNOWN"
        assert timing.terminal_to_accept_us == "UNKNOWN"
        assert timing.between_child_us == "UNKNOWN"
        assert timing.dominant_seam == "UNKNOWN"
        assert timing.tail_us == "UNKNOWN"
        record = normalizer.summarize_gates(events).to_record()
        assert record["WIFI_DPC_CHILD_TIMING_STATUS"] == "UNKNOWN"
        assert record["WIFI_DPC_CHILD_TIMING_S2Q_US"] == "UNKNOWN"
        assert record["WIFI_DPC_CHILD_TIMING_MAX_US"] == "UNKNOWN"
        assert (
            record["WIFI_DPC_CHILD_TIMING_SOURCE_TO_PUBLISH_US"] == "UNKNOWN"
        )
        assert (
            record["WIFI_DPC_CHILD_TIMING_PUBLISH_TO_INTAKE_US"] == "UNKNOWN"
        )
        assert (
            record["WIFI_DPC_CHILD_TIMING_INTAKE_TO_ISSUE_US"] == "UNKNOWN"
        )
        assert (
            record["WIFI_DPC_CHILD_TIMING_ISSUE_TO_TERMINAL_US"] == "UNKNOWN"
        )
        assert (
            record["WIFI_DPC_CHILD_TIMING_TERMINAL_TO_ACCEPT_US"] == "UNKNOWN"
        )
        assert record["WIFI_DPC_CHILD_TIMING_BETWEEN_CHILD_US"] == "UNKNOWN"
        assert record["WIFI_DPC_CHILD_TIMING_DOMINANT_SEAM"] == "UNKNOWN"
        assert record["WIFI_DPC_CHILD_TIMING_TAIL_US"] == "UNKNOWN"


def test_host_annotation_tail_cannot_replay_target_gate_or_boot_evidence() -> None:
    lines = [
        "U-Boot 2026.01",
        "Bootstrapping kernel",
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=0 sta",
        "[host] drain post-root-prompt-settle-before-diagnostics duration_s=8.00",
        (
            "tus=preflight backoff_ms=250 next_attempt_ms=250 serial=blocked "
            "local_seat=enabled recovery=full console_seq=1 "
            "telemetry_sinks=serial+queen-log prompt_refresh=no"
        ),
        *oldgood_wifi_replay_lines(),
        (
            "[host] diagnostics timeout tail='U-Boot 2026.01\\r\\n"
            "Bootstrapping kernel\\r\\n"
            "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=stabilizing "
            "backoff_ms=0 next_attempt_ms=999 serial=ready local_seat=enabled "
            "recovery=full console_seq=99 "
            "telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes\\r\\n"
            "wifi: gate 8 subgate=8a-pair-generation status=pass "
            "pair_epoch=1 generation=9 blocker=none\\r\\n'"
        ),
    ]

    slices = normalizer.boot_slices(lines)
    assert len(slices) == 1
    events = normalizer.parse_events(normalizer.latest_boot_lines(lines))
    assert all(event.line != len(lines) for event in events)
    record = normalizer.summarize_gates(events).to_record()
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "yes"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == "none"
    assert record["WIFI_GATE8_COMPLETE"] == "yes"
    assert record["WIFI_GATE8_STATUS"] == "pass"


def test_pi4_wifi_mailbox_usb_power_lines_are_usb_platform_evidence() -> None:
    event = normalizer.parse_line(
        "[pi4-platform] mailbox vl805-usb-hcd-power action=begin module=0x00000003",
        31,
    )

    assert event is not None
    assert event.domain == "usb"
    assert event.source == "cohesix"
    assert event.fields["action"] == "begin"


def test_pi4_wifi_mailbox_module3_power_on_line_is_usb_platform_evidence() -> None:
    event = normalizer.parse_line(
        "[pi4-platform] mailbox power-on module=0x00000003",
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


def test_cli_gate_summary_labels_uboot_policy_missing(
    tmp_path: pathlib.Path, capsys
) -> None:
    """Serial U-Boot Wi-Fi recovery is policy evidence, not hardware failure."""

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "U-Boot 2026.01-dirty",
                "[cohesix] Cohesix boot options",
                "[cohesix] U-Boot policy missing: "
                "cohesix.env has no saved Wi-Fi credentials",
                "[cohesix] Serial Wi-Fi password entry is disabled "
                "because U-Boot echoes input",
                "[cohesix] Recovery is file-based or local-USB only; "
                "do not type PSK on serial",
                "  0. Exit to U-Boot prompt for file-based policy recovery",
            ]
        ),
        encoding="utf-8",
    )

    result = normalizer.main([str(log_path), "--gate-summary"])
    captured = capsys.readouterr()
    gate_lines = dict(line.split("=", 1) for line in captured.out.splitlines())

    assert result == 0
    assert gate_lines["WIFI_GATE"] == "1"
    assert gate_lines["WIFI_BLOCKER"] == "uboot-policy-missing"
    assert gate_lines["WIFI_EXACT"] == "uboot-policy-missing"
    assert gate_lines["WIFI_PHASE"] == "none"


def test_cli_gate_summary_labels_current_uboot_policy_missing(
    tmp_path: pathlib.Path, capsys
) -> None:
    """Current local-input failure is classified only when policy is absent."""

    unavailable = (
        "[cohesix] Wi-Fi password entry is unavailable over serial because "
        "U-Boot echoes typed input"
    )
    missing = (
        "[cohesix] No Wi-Fi network is configured and local USB input is unavailable"
    )
    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "U-Boot 2026.01-dirty",
                "[cohesix] Cohesix boot menu",
                "[cohesix] No Wi-Fi network is configured",
                unavailable,
                missing,
                "[cohesix] Connect a USB keyboard or create cohesix.env on the "
                "SD boot partition, then restart",
            ]
        ),
        encoding="utf-8",
    )

    result = normalizer.main([str(log_path), "--gate-summary"])
    captured = capsys.readouterr()
    gate_lines = dict(line.split("=", 1) for line in captured.out.splitlines())

    assert result == 0
    assert gate_lines["WIFI_GATE"] == "1"
    assert gate_lines["WIFI_BLOCKER"] == "uboot-policy-missing"
    assert gate_lines["WIFI_EXACT"] == "uboot-policy-missing"
    assert not normalizer.uboot_wifi_policy_missing_line(unavailable)
    assert not normalizer.uboot_wifi_policy_missing_line(
        "[cohesix] Existing Wi-Fi settings were not changed"
    )


def test_cli_boot_summary_scores_each_boot_slice(
    tmp_path: pathlib.Path, capsys
) -> None:
    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "U-Boot 2026.01-dirty",
                "[cohesix:root-task] Cohesix boot: root-task online",
                "Cohesix console ready",
                "cohesix> HDMI_FRAME_SUBMIT reason=keyboard-scrollback "
                "status=ready root_console_ready=yes attached=yes bytes=1",
                "SCHED_CONTRACT contract=cyw43455 isolation=dedicated-sel4-task "
                "max_service_us=1000 observed_service_us=3281157",
                *strict_wired_boot_proof_lines(),
            ]
        ),
        encoding="utf-8",
    )

    result = normalizer.main([str(log_path), "--boot-summary"])
    captured = capsys.readouterr()
    summary = json.loads(captured.out)

    assert result == 0
    assert len(summary["boots"]) == 2
    assert summary["boots"][0]["score"] == "fail"
    assert "driver-task-budget-overrun" in summary["boots"][0]["blockers"]
    assert "serial-unclean" in summary["boots"][0]["blockers"]
    assert summary["boots"][1]["score"] == "pass"


def test_cli_boot_summary_skips_uboot_menu_save_reset(
    tmp_path: pathlib.Path, capsys
) -> None:
    """A U-Boot policy save/reset slice is not a failed Cohesix boot."""

    log_path = tmp_path / "pi4-serial.log"
    log_path.write_text(
        "\n".join(
            [
                "U-Boot 2026.01-dirty",
                "[cohesix] Cohesix boot options",
                "[cohesix] saved settings to cohesix.env",
                "resetting ...",
                *strict_wired_boot_proof_lines(),
            ]
        ),
        encoding="utf-8",
    )

    result = normalizer.main([str(log_path), "--boot-summary"])
    captured = capsys.readouterr()
    summary = json.loads(captured.out)

    assert result == 0
    assert summary["boots"][0]["score"] == "skip"
    assert summary["boots"][0]["kind"] == "uboot-menu-save-reset"
    assert summary["boots"][1]["kind"] == "cohesix-boot"


def test_boot_summary_rejects_console_only_boot_without_network_proof() -> None:
    """A prompt plus USB readiness is not a perfect Pi boot proof."""

    summaries = normalizer.summarize_boot_slices(
        [
            "U-Boot 2026.01-dirty",
            "Starting kernel ...",
            "[cohesix:root-task] Cohesix boot: root-task online",
            "Cohesix console ready",
            "cohesix> ",
            "SERIAL_ECHO result=ok serial_responsive=yes",
            *oldgood_usb_replay_lines(),
            "USB_BURST bytes=256 drops=0 max_latency_us=900",
            "HDMI_RESPONSIVE max_gap_ms=9 mirrored_bytes=256",
        ]
    )

    assert summaries[0]["score"] == "fail"
    assert "network-active-missing" in summaries[0]["blockers"]
    assert "network-tcp-proof-missing" in summaries[0]["blockers"]
    assert "driver-task-dedicated-not-ready" in summaries[0]["blockers"]


@pytest.mark.parametrize(
    ("field", "value", "expected_blocker"),
    [
        ("HDMI_RESPONSIVE_PROOF", "no", "hdmi-responsive-proof-missing"),
        ("USB_BLOCKER", "post-ready-busy", "local-seat-usb-blocked"),
        (
            "USB_COMMAND_READY",
            "no",
            "local-seat-usb-command-ready-missing",
        ),
        (
            "USB_FIRST_REPORT_READY",
            "no",
            "local-seat-usb-first-report-missing",
        ),
        (
            "USB_FIRST_BYTE_READY",
            "no",
            "local-seat-usb-first-byte-missing",
        ),
        ("USB_LOCAL_SEAT_STATE", "unknown", "local-seat-usb-state-not-ready"),
        ("USB_BUSY_AFTER_READY", "unknown", "local-seat-usb-busy-proof-missing"),
        ("USB_BURST_PROOF", "no", "local-seat-usb-burst-proof-missing"),
        ("USB_BURST_DROPS", 1, "local-seat-usb-burst-drops"),
        (
            "USB_POST_FIRST_BYTE_BLOCKER",
            "usb-post-first-byte-queue-collapse",
            "local-seat-usb-post-first-byte-usb-post-first-byte-queue-collapse",
        ),
    ],
)
def test_boot_summary_requires_complete_operator_liveness_proof(
    field: str,
    value: object,
    expected_blocker: str,
) -> None:
    """A network-ready boot cannot hide a missing local-operator gate."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(strict_wired_boot_proof_lines())
    ).to_record()
    assert normalizer.boot_evidence_blockers(record) == []

    record[field] = value

    assert expected_blocker in normalizer.boot_evidence_blockers(record)


def test_boot_summary_treats_usb_oldgood_receipt_as_diagnostic_only() -> None:
    """A dormant USB old-good receipt cannot reject current functional proof."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(strict_wired_boot_proof_lines())
    ).to_record()
    assert normalizer.boot_evidence_blockers(record) == []

    record["USB_OLDGOOD_REPLAY"] = "no"
    record["USB_OLDGOOD_LAST"] = "none"
    record["USB_OLDGOOD_MISSING"] = "not-run"

    assert normalizer.boot_evidence_blockers(record) == []


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
        "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
        "Starting kernel ...",
        "wifi: boot_failure source=live exact=new-failure",
    ]

    events = normalizer.parse_events(normalizer.latest_boot_lines(lines))
    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["USB_BOOTLOADER_HANDOFF_SEEN"] == "no"
    assert gates.to_record()["USB_COLD_BOOT_SEEN"] == "yes"
    assert gates.to_record()["WIFI_EXACT"] == "new-failure"


def test_boot_slices_ignore_uboot_menu_text() -> None:
    lines = [
        "U-Boot 2026.01-dirty",
        "  0. Exit to U-Boot prompt",
        "Starting kernel ...",
        "[cohesix:root-task] Cohesix boot: root-task online",
        "U-Boot 2026.01-dirty",
        "  0. Exit to U-Boot prompt",
        "Starting kernel ...",
        "[cohesix:root-task] Cohesix boot: root-task online",
    ]

    slices = normalizer.boot_slices(lines)

    assert [offset for offset, _ in slices] == [0, 4]


def test_boot_summary_skips_uboot_save_reset_menu_slice() -> None:
    lines = [
        "U-Boot 2026.01-dirty",
        "[cohesix] Cohesix boot options",
        "[cohesix] mode=dhcp",
        "[cohesix] saved settings to cohesix.env",
        "resetting ...",
        *strict_wired_boot_proof_lines(),
    ]

    summaries = normalizer.summarize_boot_slices(lines)
    line_offset, latest_lines = normalizer.latest_boot_slice(lines)

    assert summaries[0]["score"] == "skip"
    assert summaries[0]["kind"] == "uboot-menu-save-reset"
    assert summaries[1]["kind"] == "cohesix-boot"
    assert summaries[1]["score"] == "pass"
    assert line_offset == 5
    assert latest_lines[0] == "U-Boot 2026.01-dirty"


def test_boot_summary_skips_current_verified_save_restart_slice() -> None:
    """Current verified-save wording is not scored as a failed Cohesix boot."""

    lines = [
        "U-Boot 2026.01-dirty",
        "[cohesix] Cohesix boot menu",
        "[cohesix] saved and verified settings in cohesix.env",
        "[cohesix] Saved settings verified; restarting",
        "resetting ...",
        *strict_wired_boot_proof_lines(),
    ]

    summaries = normalizer.summarize_boot_slices(lines)
    line_offset, latest_lines = normalizer.latest_boot_slice(lines)

    assert summaries[0]["score"] == "skip"
    assert summaries[0]["kind"] == "uboot-menu-save-reset"
    assert summaries[1]["kind"] == "cohesix-boot"
    assert summaries[1]["score"] == "pass"
    assert line_offset == 5
    assert latest_lines[0] == "U-Boot 2026.01-dirty"


def test_latest_boot_slice_ignores_trailing_uboot_save_reset_menu() -> None:
    lines = [
        "U-Boot 2026.01-dirty",
        "Starting kernel ...",
        "[cohesix:root-task] Cohesix boot: root-task online",
        "Cohesix console ready",
        "cohesix> ",
        "U-Boot 2026.01-dirty",
        "[cohesix] saved settings to cohesix.env",
        "resetting ...",
    ]

    line_offset, latest_lines = normalizer.latest_boot_slice(lines)

    assert line_offset == 0
    assert latest_lines[0] == "U-Boot 2026.01-dirty"
    assert "Starting kernel ..." in latest_lines


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
        "WIFI_SUBGATE": "none",
        "WIFI_SUBGATE_NAME": "none",
        "WIFI_SUBGATE_SOURCE": "none",
        "WIFI_SUBGATE_STATUS": "none",
        "WIFI_SUBGATE_REASON": "none",
        "WIFI_SUBGATE_LINE": 0,
        "WIFI_GATE7_COMPLETE": "no",
        "WIFI_GATE7_SEEN": "none",
        "WIFI_GATE7_LAST": "none",
        "WIFI_GATE7_MISSING": "7a",
        "WIFI_GATE8_COMPLETE": "no",
        "WIFI_GATE8_SEEN": "none",
        "WIFI_GATE8_LAST": "none",
        "WIFI_GATE8_MISSING": "8a-pair-generation",
        "WIFI_GATE8_STATUS": "none",
        "WIFI_GATE8_GENERATION": 0,
        "WIFI_GATE8_PAIR_EPOCH": 0,
        "WIFI_GATE8_BLOCKER": "none",
        "WIFI_GATE8_LINE": 0,
        "WIFI_GATE8_LATEST_SEEN": "none",
        "WIFI_GATE8_LATEST_LAST": "none",
        "WIFI_GATE8_LATEST_MISSING": "8a-pair-generation",
        "WIFI_GATE8_LATEST_STATUS": "none",
        "WIFI_GATE8_LATEST_PAIR_EPOCH": 0,
        "WIFI_GATE8_LATEST_GENERATION": 0,
        "WIFI_GATE8_LATEST_BLOCKER": "none",
        "WIFI_GATE8_LATEST_LINE": 0,
        "WIFI_GATE8_LATEST_ATTEMPT": 0,
        "USB_OLDGOOD_REPLAY": "no",
        "USB_OLDGOOD_LAST": "none",
        "USB_OLDGOOD_MISSING": "cold-boot-unseeded",
        "WIFI_OLDGOOD_REPLAY": "no",
        "WIFI_OLDGOOD_LAST": "none",
        "WIFI_OLDGOOD_MISSING": "cyw43-owner-state",
        "WIFI_EXACT": "cyw43-ht-clock-timeout-before-function2",
        "WIFI_PHASE": "cyw43-load-firmware-fail",
        "WIFI_BLOCKER_LINE": 8,
        "WIFI_DIAG_DETAIL": "unknown",
        "WIFI_DIAG_SCOPE": "unknown",
        "WIFI_DIAG_CAUSE": "none",
        "WIFI_DIAG_TRIGGER": "none",
        "WIFI_DIAG_RETAINED": "none",
        "WIFI_DIAG_BODY_LINES": 0,
        "WIFI_DIAG_BODY_BYTES": 0,
        "SERIAL_CLEAN": "yes",
        "BOOT_HALTED": "no",
        "TIMER_IRQ27_SEEN": "no",
        "TIMER_BACKEND": "unknown",
        "TIMER_CLOCK_HZ": 0,
        "TIMER_EL0_COUNTER": "none",
        "DUMMY_TIMER_SEEN": "no",
        "BOOT_HALT_REASON": "none",
        "PANIC_SEEN": "no",
        "PANIC_REASON": "none",
        "SDIO_IRQ158_SEEN": "no",
        "SDIO_IRQ158_BOUND": "no",
        "SDIO_IRQ158_INBAND_PROOF": "no",
        "SDIO_IRQ158_LINE": 0,
        "USB_BOOTLOADER_HANDOFF_SEEN": "no",
        "USB_COLD_BOOT_SEEN": "no",
        "USB_STALE_UEFI_HINT_SEEN": "no",
        "USB_EVENT_RING_ALIVE": "no",
        "USB_PSC_DRAIN_COUNT": 0,
        "USB_PSC_DRAIN_MASK": "0x00000000",
        "ROOT_CONSOLE_READY": "no",
        "ROOT_PROMPT_SEEN": "no",
        "NET_ACTIVE": "unknown",
        "NET_ADDR_SRC": "unknown",
        "NET_DHCP": "unknown",
            "NET_TCP_READY": "no",
            "NETTEST_PROOF": "no",
            "COHSH_TCP_AUTH_PROOF": "no",
            "TCP_ACCEPTS": 0,
            "TCP_AUTH_SESSIONS": 0,
            "TCP_RX_BYTES": 0,
        "WIFI_DATA_PATH_TX": 0,
        "WIFI_DATA_PATH_RX_PRESERVED": 0,
        "WIFI_DATA_PATH_RX_DELIVERED": 0,
        "WIFI_DATA_PATH_RX_DROPPED": 0,
        "WIFI_DATA_PATH_LAST": "none",
        "WIFI_SERVICE_OP": 0,
        "WIFI_SERVICE_REASON": 0,
        "WIFI_SERVICE_PROGRESS": "0x00000000",
        "WIFI_SERVICE_SEQ": 0,
        "WIFI_SERVICE_CREDIT": 0,
        "WIFI_SERVICE_CHANNEL": 0,
        "WIFI_SERVICE_RFRAME": 0,
        "WIFI_SERVICE_EAPOL_M1": 0,
        "WIFI_SERVICE_EAPOL_M2": 0,
        "WIFI_SERVICE_EAPOL_M3": 0,
        "WIFI_SERVICE_EAPOL_M4": 0,
        "WIFI_PRIORITY_EPISODE_SCOPE": "none",
        "WIFI_PRIORITY_EPISODE_PHASE": "none",
        "WIFI_PRIORITY_EPISODE_PAIR_EPOCH": 0,
        "WIFI_PRIORITY_EPISODE_MASK": "0x00",
        "WIFI_PRIORITY_EPISODE_COUNTS_SCOPE": "none",
        "WIFI_PRIORITY_EPISODE_FAULTS_SCOPE": "none",
        "WIFI_PRIORITY_EPISODE_OPENS": 0,
        "WIFI_PRIORITY_EPISODE_CLOSES": 0,
        "WIFI_PRIORITY_EPISODE_RESTORES": 0,
        "WIFI_PRIORITY_EPISODE_AMORTIZED_REQUESTS": 0,
        "WIFI_PRIORITY_EPISODE_FAILURES": 0,
        "WIFI_PRIORITY_EPISODE_RECOVERY_REVOCATIONS": 0,
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_SCOPE": "none",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_CAUSE": "unavailable",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_OUTER_PHASE": "none",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_OUTER_PAIR_EPOCH": 0,
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_OUTER_MASK": "0x00",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_ACTIVE": "unknown",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_PHASE": "none",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_MASK": "0x00",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_REQUEST": 0,
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_GENERATION": 0,
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_COMMAND_SEQUENCE": 0,
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_DOORBELL_ISSUED": "unknown",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_PUBLICATION_LATCHED": "unknown",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_SIGNAL_RETURNED": "unknown",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_PARENT_DEADLINE_EXPIRED": "unknown",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_CHILD_TERMINAL": "unknown",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_CHILD_WAIT_RECEIPT": "unknown",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_CHILD_BUS_EPISODE": "unknown",
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_BUS_PARENT_SEQUENCE": 0,
        "WIFI_DEFERRED_RECOVERY_SCHEDULER_BUS_PARENT_OP": "0x0000",
        "WIFI_DEFERRED_RECOVERY_RUNTIME_SOURCE_LINE": 0,
        "WIFI_CAUSAL_FRONTIER": "none",
        "WIFI_RX_IRQ_PRESERVE_COUNT": 0,
        "WIFI_RX_IRQ_PRESERVE_REASON": "none",
        "WIFI_RX_IRQ_PRESERVE_INT": "0x00000000",
        "WIFI_RX_IRQ_PRESERVE_ACK": "0x00000000",
        "WIFI_RX_IRQ_EPISODE_PRESERVES": 0,
        "WIFI_RX_IRQ_EPISODE_REARMS": 0,
        "WIFI_RXTRACE_SEQ": 0,
        "WIFI_RXTRACE_START_TICKS": "0x00000000",
        "WIFI_RXTRACE_PRE_SAMPLE_DELTA_TICKS": 0,
        "WIFI_RXTRACE_TRANSFER_DELTA_TICKS": 0,
        "WIFI_RXTRACE_POST_SAMPLE_DELTA_TICKS": 0,
        "WIFI_DPC_PROOF": "no",
        "WIFI_DPC_REASON": "missing",
        "WIFI_DPC_GENERATION": 0,
        "WIFI_DPC_CAPTURES": 0,
        "WIFI_DPC_PUBLISHED": 0,
        "WIFI_DPC_CONSUMED": 0,
        "WIFI_DPC_REARMS": 0,
        "WIFI_DPC_OVERRUNS": 0,
        "WIFI_DPC_EPOCH_ERRORS": 0,
        "WIFI_DPC_SEQUENCE_ERRORS": 0,
        "WIFI_DPC_ACK_FAILURES": 0,
        "WIFI_DPC_OWNER_ACTIVE": "unknown",
        "WIFI_DPC_POISONED": "unknown",
        "WIFI_DPC_RING_POISONED": "unknown",
        "WIFI_DPC_CLIENT_SAMPLE_STALE": "unknown",
        "WIFI_DPC_TRUTH_AUTHORITY": "unknown",
        "WIFI_DPC_TRUTH_LINE": 0,
        "WIFI_DPC_MASKED": "unknown",
        "WIFI_DPC_LINE": 0,
        "WIFI_DPC_CHILD_TIMING_STATUS": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_REASON": "missing",
        "WIFI_DPC_CHILD_TIMING_VERSION": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_PHYSICAL_EPOCH": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_EVENT_SEQUENCE": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_SOURCE_CNTVCT": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_QUEUE_CNTVCT": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_QUEUE_COMMIT_SEQUENCE": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_DATA_LEN": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_COUNT": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_OBSERVED_ENTRIES": 0,
        "WIFI_DPC_CHILD_TIMING_FLAGS": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_S2Q_US": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_MAX_US": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_SOURCE_TO_PUBLISH_US": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_PUBLISH_TO_INTAKE_US": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_INTAKE_TO_ISSUE_US": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_ISSUE_TO_TERMINAL_US": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_TERMINAL_TO_ACCEPT_US": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_BETWEEN_CHILD_US": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_DOMINANT_SEAM": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_OVERFLOW_COUNT": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_UNKNOWN_COUNT": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_TAIL_US": "UNKNOWN",
        "WIFI_DPC_CHILD_TIMING_LINE": 0,
        "CYW43_BOOTSTRAP_SUPERVISOR_SEEN": "no",
        "CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT": 0,
        "CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES": 0,
        "CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES": 0,
        "CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS": "none",
        "CYW43_BOOTSTRAP_SUPERVISOR_READY": "no",
        "CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER": "none",
        "WIFI_FIRMWARE_IDENTITY_PROOF": "no",
        "WIFI_FIRMWARE_IDENTITY_BLOCKER": "nvram-len",
        "WIFI_CLM_READY_PROOF": "no",
        "WIFI_FIRMWARE_VERSION_PROOF": "no",
        "WIFI_CLM_VERSION_PROOF": "no",
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
        "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF": "no",
        "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOFS": 0,
        "DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_BLOCKER": "none",
        "DRIVER_TASK_ACTIVE_NET": "unknown",
        "DRIVER_TASK_BUDGET_OVERRUNS": 0,
        "DRIVER_TASK_LATENCY_PROOFS": 0,
        "DRIVER_TASK_RING_CALL_BEGIN": 0,
        "DRIVER_TASK_RING_CALL_RETURN": 0,
        "DRIVER_TASK_RING_CALL_OUTSTANDING": 0,
        "DRIVER_TASK_RING_CALL_TIMEOUT": 0,
        "DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT": 0,
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
        "DRIVER_TASK_COUNTER_OVERRUNS": 0,
        "DRIVER_TASK_COUNTER_DROPS": 0,
        "DRIVER_TASK_DMA_PROOFS": 0,
        "DRIVER_TASK_DMA_BLOCKER": "missing:cyw43-wifi",
        "PI4_RUNTIME_DMA_PROOF": "absent",
        "PI4_RUNTIME_DMA_PROOF_REASON": "no-runtime-proof-lines",
        "PI4_RUNTIME_DMA_COUNTER_PROOF": "absent",
        "SERIAL_OUTPUT_TX_PENDING": "unknown",
        "SERIAL_OUTPUT_INTERACTIVE": "unknown",
        "SERIAL_OUTPUT_DEFERRED": 0,
        "SERIAL_OUTPUT_FLUSHED": 0,
        "SERIAL_OUTPUT_BACKPRESSURE": 0,
        "HDMI_DISPLAY_PENDING_BYTES": 0,
        "HDMI_DISPLAY_PENDING_REDRAW": "unknown",
        "HDMI_DISPLAY_SUBMITTED": 0,
        "HDMI_DISPLAY_DEFERRED": 0,
        "HDMI_DISPLAY_BUSY": 0,
        "HDMI_DISPLAY_NO_REPLY": 0,
        "HDMI_DISPLAY_COALESCED": 0,
        "HDMI_DISPLAY_BACKPRESSURE_BYTES": 0,
        "HDMI_DISPLAY_SUPERSEDED_BYTES": 0,
        "HDMI_STATUS_STATE": "unknown",
        "HDMI_STATUS_BLOCKER": "not-run",
        "HDMI_STATUS_RECEIPT": "none",
        "HDMI_DRIVER_OUTSTANDING": 0,
        "USB_KEYBOARD_NO_REPLIES": 0,
        "USB_KEYBOARD_POLL_COOLDOWN": 0,
        "USB_KEYBOARD_COOLDOWN_SKIPS": 0,
        "USB_RUNTIME_QUEUED_REPORTS": 0,
        "USB_RUNTIME_TRANSFER_EVENTS": 0,
        "USB_RUNTIME_REPORT_STATUS": "unknown",
        "USB_RUNTIME_QUEUE_VALID": "unknown",
        "USB_RUNTIME_CADENCE_PREVIOUS": "unknown",
        "USB_RUNTIME_CADENCE_GAP_TICKS": "UNKNOWN",
        "USB_RUNTIME_CADENCE_RUN_TICKS": "UNKNOWN",
        "USB_RUNTIME_CADENCE_LINE": 0,
        "USB_RUNTIME_DOORBELL_PENDING": "unknown",
        "USB_RUNTIME_RECOVERY_DIAG_VALID": "unknown",
        "USB_RUNTIME_ENDPOINT_RECOVERIES": 0,
        "USB_RUNTIME_ENDPOINT_RECOVERY_FAILURES": 0,
        "USB_RUNTIME_QUEUE_COLLAPSE_RECOVERIES": 0,
        "USB_RUNTIME_RECOVERY_STAGE": "unknown",
        "USB_RUNTIME_RECOVERY_REASON": "unknown",
        "USB_RUNTIME_COMMAND_COMPLETION_BLOCKED": 0,
        "USB_RUNTIME_DRIVER_ACTIVE": "unknown",
        "USB_RUNTIME_DRIVER_OUTSTANDING": 0,
        "USB_RUNTIME_DRIVER_ACTIVE_NO_PROGRESS": 0,
        "USB_RUNTIME_DRIVER_SAME_REQUEST": 0,
        "USB_RUNTIME_DRIVER_KEEP_ACTIVE": 0,
        "USB_RUNTIME_DRIVER_ABORTS": 0,
        "USB_EVENT_LOOP_RUNTIME_SKIPPED": 0,
        "USB_DIAG_LIVENESS_GENERATION": 0,
        "USB_DIAG_LIVENESS_STATUS": "not-run",
        "USB_DIAG_LIVENESS_BACKEND_DELTA": 0,
        "USB_DIAG_LIVENESS_ACCEPTED_DELTA": 0,
        "USB_DIAG_LIVENESS_DRAINED_DELTA": 0,
        "USB_DIAG_LIVENESS_ECHOED_DELTA": 0,
        "USB_DIAG_LIVENESS_DROP_DELTA": 0,
        "USB_GATE_SCOPE": "startup",
        "USB_CURRENT_LIVENESS": "unproven",
        "USB_CURRENT_LIVENESS_REASON": "diagnostic-not-run",
        "USB_PHYSICAL_INPUT_PROOF": "no",
        "USB_POST_FIRST_BYTE_BLOCKER": "none",
        "USB_STARTUP_BLOCKER_SEEN": "no",
        "USB_ACTIVE_BLOCKER_SEEN": "no",
        "USB_RECOVERED_FROM_BLOCKER": "no",
        "USB_RECOVERY_STATE": "unknown",
        "USB_LOCAL_SEAT_STATE": "blocked",
        "USB_LOCAL_SEAT_REASON": "cmd-poll-only-timeout",
        "USB_COMMAND_READY": "no",
        "USB_FIRST_REPORT_READY": "no",
        "USB_FIRST_BYTE_READY": "no",
        "USB_BUSY_AFTER_READY": "no",
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
    assert record["NET_TCP_READY"] == "no"
    assert record["NETTEST_PROOF"] == "no"
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


def test_latest_complete_smp_driver_proof_supersedes_provisional_roles() -> None:
    """The latest atomic activity aggregate owns live runtime role counts."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_SUMMARY contracts=6 dedicated=1 compatibility=5",
            "DRIVER_TASK role=serial contract=serial "
            "isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated",
            "[smp] activity driver-proof contracts=6 requested_dedicated=6 "
            "dedicated=5 compat=1 substrate=yes configured=6 live=5 failed=1 "
            "hot_mask=0x1f compat_mask=0x20",
            "[smp] activity driver-proof contracts=6 requested_dedicated=6 "
            "dedicated=6 compat=0 substrate=yes configured=6 live=6 failed=0 "
            "hot_mask=0x3f compat_mask=0x0",
            "[smp] activity selected profile=pi4-hardware net=wifi "
            "active_contracts=selected-only",
            "[smp] activity driver-proof contracts=6 requested_dedicated=6 "
            "dedicated=0 compat=6 substrate=yes configured=6 live=6 failed=0 "
            "hot_mask=0x0",
            "[smp] activity driver-proof contracts=6 requested_dedicated=6 "
            "dedicated=1 compat=5 substrate=yes configured=6 live=6 failed=0 "
            "hot_mask=0x3f compat_mask=0x0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["DRIVER_TASK_CONTRACTS"] == 6
    assert record["DRIVER_TASK_DEDICATED"] == 6
    assert record["DRIVER_TASK_COMPATIBILITY"] == 0
    assert record["DRIVER_TASK_LIVE_HOT_PATHS"] == "yes"
    assert record["DRIVER_TASK_SERIAL_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_USB_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_DISPLAY_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_NET_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_SDIO_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_PCIE_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_DEDICATED_READY"] == "no"
    assert record["DRIVER_TASK_OWNER_STATE_PROOF"] == "no"
    assert record["DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF"] == "no"


def test_latest_valid_partial_failure_supersedes_stale_healthy_aggregate() -> None:
    """Configured handles exclude separately counted failed creations."""

    events = normalizer.parse_events(
        [
            "[smp] activity driver-proof contracts=6 requested_dedicated=6 "
            "dedicated=6 compat=0 substrate=yes configured=6 live=6 failed=0 "
            "hot_mask=0x3f compat_mask=0x0",
            "[smp] activity selected profile=pi4-hardware net=wifi "
            "active_contracts=selected-only",
            "[smp] activity driver-proof contracts=6 requested_dedicated=6 "
            "dedicated=6 compat=0 substrate=yes configured=5 live=5 failed=1 "
            "hot_mask=0x1f compat_mask=0x0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["DRIVER_TASK_CONTRACTS"] == 6
    assert record["DRIVER_TASK_DEDICATED"] == 6
    assert record["DRIVER_TASK_COMPATIBILITY"] == 0
    assert record["DRIVER_TASK_LIVE_HOT_PATHS"] == "no"
    assert record["DRIVER_TASK_SERIAL_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_USB_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_DISPLAY_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_NET_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_SDIO_DEDICATED"] == "yes"
    assert record["DRIVER_TASK_PCIE_DEDICATED"] == "no"
    assert record["DRIVER_TASK_DEDICATED_READY"] == "no"


def test_gate_summary_tracks_smp_activity_net_state() -> None:
    """Operator activity summaries must carry Genet readiness into gate proof."""

    events = normalizer.parse_events(
        [
            "[smp] activity net attached=yes backend=bcmgenet-v5 mode=dhcp "
            "active=wired standby=wifi src=dhcp-lease dhcp=bound "
            "contract=bcmgenet-v5",
            "[smp] activity net-link link=yes last_poll_ms=70950 tx_drops=0 "
            "ip=192.168.10.50 gw=192.168.10.1",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["NET_ACTIVE"] == "wired"
    assert record["NET_ADDR_SRC"] == "dhcp-lease"
    assert record["NET_DHCP"] == "bound"
    assert record["NET_TCP_READY"] == "no"
    assert record["NETTEST_PROOF"] == "no"


def test_gate_summary_accepts_compact_smp_operator_liveness_snapshot() -> None:
    """SMP snapshots close USB state but not end-to-end serial responsiveness."""

    events = normalizer.parse_events(
        [
            "cohesix> [local-seat] hdmi prompt pending "
            "reason=keyboard-command-pending action=wait-for-prompt-ready "
            "console_seq=126 telemetry_sinks=serial prompt_refresh=no",
            "[local-seat] keyboard unavailable detail=backend-unavailable",
            "cohesix> wifi: debug subcommand=probe-ht action=begin "
            "profile=bounded mode=one-shot",
            "cohesix> [smp] activity begin source=userspace benchmark=off "
            "hdmi=high-impact-only",
            "[smp] activity pump now_ms=336580 input=local-seat lines=3 ok=1 "
            "denied=1 ticks=66748 serial_rx_drop=0 serial_tx_drop=0 "
            "utf8_drop=0 serial_budget_overruns=0 "
            "serial_rx_backpressure=0 serial_tx_backpressure=0 "
            "serial_pressure_source=uart-output",
            "[smp] activity local-seat runtime=present attached=yes "
            "keyboard_device=usb-kbd0 display=hdmi0 backend_poll=yes "
            "keyboard_ready=yes command_ready=yes first_report=yes "
            "first_byte=yes",
            "[smp] activity local-seat-input backend_polls=329652 "
            "backend_bytes=47 queued=0 arming=0 accepted=47 drained=47 "
            "echoed=47 drop=0 no_reply=0 cooldown=0 cooldown_skips=0 "
            "hdmi_drop=0",
            "cohesix> [smp] scheduler dump unavailable after linked UART "
            "cutover use=smp-activity",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["SERIAL_CLEAN"] == "yes"
    assert record["SERIAL_RESPONSIVE_PROOF"] == "no"
    assert record["USB_GATE"] == 10
    assert record["USB_BLOCKER"] == "none"
    assert record["USB_LOCAL_SEAT_STATE"] == "ready"
    assert record["USB_COMMAND_READY"] == "yes"
    assert record["USB_FIRST_REPORT_READY"] == "yes"
    assert record["USB_FIRST_BYTE_READY"] == "yes"


def test_gate_summary_tracks_netstatus_tcp_ready_proof() -> None:
    events = normalizer.parse_events(
        [
            "netstatus: ip=192.168.10.50 gateway=192.168.10.1 "
            "src=dhcp-lease dhcp=bound tcp_ready=yes",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["NET_ADDR_SRC"] == "dhcp-lease"
    assert record["NET_DHCP"] == "bound"
    assert record["NET_TCP_READY"] == "yes"
    assert record["NETTEST_PROOF"] == "no"


def test_gate_summary_keeps_selected_network_after_passive_component_state() -> None:
    events = normalizer.parse_events(
        [
            "netstats: generation=1 mode=dhcp policy=wifi active=wifi "
            "standby=none addr_src=dhcp-lease ip=192.168.86.154 "
            "gateway=192.168.86.1 dhcp=bound",
            "netstats: cyw43_priority_lease state=inactive pair_epoch=0 "
            "mask=0x00 active=no close_pending=no",
            "netstats: mcs_idle schema=v1 before=200 after=0 timer_reject=0 "
            "clear=0/0 last_cut=0 mask=0x0001",
            "netstats: mcs_idle_fences schema=v1 base=0 counts=200,0,0,0",
            "netstats: mcs_idle_fences schema=v1 base=4 counts=0,0,0,0",
            "netstats: mcs_idle_fences schema=v1 base=8 counts=0,0,0,0",
            "netstats: mcs_idle_fences schema=v1 base=12 counts=0,0,0,0",
            "netstats: mcs_session schema=v1 generation=1 conn=11 before=20 "
            "after=0 timer_reject=0 clear=0/0",
            "netstats: mcs_session_fences schema=v1 base=0 counts=0,0,0,20",
            "netstats: mcs_session_fences schema=v1 base=4 counts=0,0,0,0",
            "netstats: mcs_session_fences schema=v1 base=8 counts=0,0,0,0",
            "netstats: mcs_session_fences schema=v1 base=12 counts=0,0,0,0",
            "netstats: mcs_session_operator schema=v1 serial_rx=0 "
            "serial_line=20 local_line=0 local_chunk=0 usb_bytes=0 usb_service=0",
            "netstats: mcs_session_yield schema=v1 samples=20 "
            "total_us=190000 max_us=10000 invalid=0",
            "netstats: mcs_session_yield_cut schema=v1 cause=NO_PRODUCTIVE_SUCCESSOR "
            "pending=8 phase=3 pub=2 cmd=37 stage=37 drain=36 ticks=100/200",
            "netstats: wifi_ack_admission schema=v1 gen=1 dequeued=60 "
            "staged=60 completed=59 runtime_gen=1 ingress_seq=121 consumed=no",
            "netstats: wifi_ack_last schema=v1 gen=1 src=c0a85666:49152 "
            "dst=c0a8569a:31337 seq=11223344 ack=55667788",
            "netstats: wifi_ack_fin schema=v1 gen=1 src=c0a85666:49152 "
            "dst=c0a8569a:31337 seq=11223344 ack=55667788",
            "netstats: wifi_ack_before_fin schema=v1 seq=11223344 "
            "ack=55667787 runtime_gen=1 ingress_seq=120 consumed=yes",
            "netstats: wifi_rx_dequeue_slow gen=1 src=c0a85666:49152 "
            "dst=c0a8569a:31337 seq=11223344 ack=55667788 flags=010",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["NET_ACTIVE"] == "wifi"
    assert record["NET_ADDR_SRC"] == "dhcp-lease"
    assert record["NET_DHCP"] == "bound"
    assert record["NET_TCP_READY"] == "no"
    assert record["NETTEST_PROOF"] == "no"


def test_gate_summary_late_tcp_not_ready_clears_current_tcp_state() -> None:
    events = normalizer.parse_events(
        [
            "OK NETTEST detail=pass scope=serial-local",
            "netstats: mode=dhcp policy=wifi active=wifi standby=wired "
            "addr_src=dhcp-lease ip=192.168.86.154 gateway=192.168.86.1 "
            "dhcp=bound tcp_ready=yes",
            "netstats: mode=dhcp policy=wifi active=wifi standby=wired "
            "addr_src=wifi-tx-terminal-fault ip=192.168.86.154 "
            "gateway=192.168.86.1 dhcp=tx-terminal-fault tcp_ready=no",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["NET_ACTIVE"] == "wifi"
    assert record["NET_ADDR_SRC"] == "wifi-tx-terminal-fault"
    assert record["NET_TCP_READY"] == "no"
    assert record["NETTEST_PROOF"] == "yes"


def test_gate_summary_host_cohsh_proof_survives_stale_tcp_ready_no() -> None:
    events = normalizer.parse_events(
        [
            "[cohsh-net][auth] auth OK, session established (conn_id=1)",
            "netstats: mode=dhcp policy=wired active=wired standby=wifi "
            "addr_src=dhcp-lease ip=192.168.10.50 gateway=192.168.10.1 "
            "dhcp=bound tcp_ready=no",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["NET_ACTIVE"] == "wired"
    assert record["NET_ADDR_SRC"] == "dhcp-lease"
    assert record["NET_TCP_READY"] == "yes"
    assert record["NETTEST_PROOF"] == "no"


def test_gate_summary_nettest_error_clears_stale_success_proof() -> None:
    """A current net-disabled result must not inherit earlier NETTEST proof."""

    events = normalizer.parse_events(
        [
            "OK NETTEST detail=pass scope=serial-local",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43-command driver-task runtime init failed",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["NET_TCP_READY"] == "no"
    assert record["NETTEST_PROOF"] == "no"


def test_gate_summary_classifies_fresh_pi_runtime_dma_proof() -> None:
    """Runtime/DMA proof needs live Pi owner-state and counter-qualified timing."""

    events = normalizer.parse_events(
        seal_driver_task_runtime_descriptor_lines([
            "U-Boot 2026.01",
            "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
            "cohesix> driver proof",
            "[timers] backend=arch-counter counter=vct timer_freq_hz=54000000",
            "DRIVER_TASK_DEFAULT requested=dedicated required=yes live_hot_paths=yes",
            "DRIVER_TASK_SELECTED profile=pi4-hardware selection=wifi "
            "active_net=cyw43 required_roles=0x3f required_hot_paths=0x7f "
            "required_tasks=6",
            "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 "
            "task_count=7 failed_count=0 live_tcb_count=7 "
            "fault_endpoint_ready=yes revoke_ready=yes broad_caps_leaked=0 "
            "sched=yes affinity=per-driver affinity_configured=7 "
            "affinity_applied=7 vspace=isolated ipc_abi=shared-ring-command "
            "pointer_free_ipc=yes owner_state=driver-owned live_hot_paths=yes",
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
            "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=18",
            "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=22",
            "SCHED_CONTRACT contract=hdmi-text isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=44",
            "SCHED_CONTRACT contract=cyw43455 isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=91",
            "SCHED_CONTRACT contract=sdio-host isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=31",
            "SCHED_CONTRACT contract=pcie-root isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=36",
            "DRIVER_TASK_DMA_PROOF contract=serial hot_path=serial-console "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=0 dma_pages=0 "
            "shared_pages=4 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=0 "
            "cache_clean_bytes=0 cache_invalidate_ops=0 cache_invalidate_bytes=0 "
            "proof_effect=runtime-dma-proof-ready",
            "DRIVER_TASK_DMA_PROOF contract=usb-local-seat hot_path=usb-keyboard "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=0 dma_pages=128 "
            "shared_pages=32 bus_address_policy=hal-bounded-bus-address "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=1 "
            "cache_clean_bytes=64 cache_invalidate_ops=1 cache_invalidate_bytes=64 "
            "proof_effect=runtime-dma-proof-ready",
            "DRIVER_TASK_DMA_PROOF contract=hdmi-text hot_path=hdmi-text "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=0 dma_pages=0 "
            "shared_pages=16 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=0 "
            "cache_clean_bytes=0 cache_invalidate_ops=0 cache_invalidate_bytes=0 "
            "proof_effect=runtime-dma-proof-ready",
            "DRIVER_TASK_DMA_PROOF contract=bcmgenet-v5 hot_path=genet-nic "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=6 dma_pages=64 "
            "shared_pages=32 bus_address_policy=hal-bounded-bus-address "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=1 "
            "cache_clean_bytes=64 cache_invalidate_ops=1 cache_invalidate_bytes=64 "
            "proof_effect=runtime-dma-proof-ready",
            "DRIVER_TASK_DMA_PROOF contract=cyw43455 hot_path=cyw43-wifi "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=0 dma_pages=0 "
            "shared_pages=64 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=0 "
            "cache_clean_bytes=0 cache_invalidate_ops=0 cache_invalidate_bytes=0 "
            "proof_effect=runtime-dma-proof-ready",
            "DRIVER_TASK_DMA_PROOF contract=sdio-host hot_path=sdio-host "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=1 dma_pages=0 "
            "shared_pages=32 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=0 "
            "cache_clean_bytes=0 cache_invalidate_ops=0 cache_invalidate_bytes=0 "
            "proof_effect=runtime-dma-proof-ready",
            "DRIVER_TASK_DMA_PROOF contract=pcie-root hot_path=pcie-root "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=11 dma_pages=0 "
            "shared_pages=16 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=0 "
            "cache_clean_bytes=0 cache_invalidate_ops=0 cache_invalidate_bytes=0 "
            "proof_effect=runtime-dma-proof-ready",
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
            "rx_bytes=64 tx_frames=1 tx_bytes=64",
            "DRIVER_TASK_COUNTER contract=sdio-host hot_path=sdio-host "
            "source=root-ring sequence=2 submitted=2 completed=2 idle=0 fault=0 "
            "budget=0 frame=1 desc=1 staged_bytes=64 clean_ops=0 clean_bytes=0 "
            "inv_ops=0 inv_bytes=0 sends=2 yields=0 busy=0 same_request=0 "
            "timeouts=0 keep_active=0 aborts=0 overruns=0 drops=0 rx_frames=1 "
            "rx_bytes=64 tx_frames=1 tx_bytes=64",
            "DRIVER_TASK_ACCEPTANCE dedicated_ready=yes reason=active-substrate "
            "substrate=active capset=pass fault=pass revoke=pass sched=pass "
            "affinity=pass vspace=isolated ipc_abi=shared-ring-command "
            "pointer_free_ipc=yes owner_state=driver-owned required=7 "
            "dedicated=7 compatibility=0 active_net=cyw43",
        ])
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_DMA_PROOFS"] == 7
    assert record["DRIVER_TASK_DMA_BLOCKER"] == "none"
    assert record["DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF"] == "yes"
    assert record["DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOFS"] == 6
    assert record["DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_BLOCKER"] == "none"
    assert record["PI4_RUNTIME_DMA_PROOF"] == "fresh-pi"
    assert record["PI4_RUNTIME_DMA_PROOF_REASON"] == "live-pi-owner-state"
    assert record["PI4_RUNTIME_DMA_COUNTER_PROOF"] == "counter-qualified"

    missing_sdio_counter = [
        event
        for event in events
        if not (
            event.raw.startswith("DRIVER_TASK_COUNTER ")
            and event.fields.get("contract") == "sdio-host"
            and event.fields.get("hot_path") == "sdio-host"
        )
    ]
    incomplete_record = normalizer.summarize_gates(
        missing_sdio_counter
    ).to_record()
    assert incomplete_record["PI4_RUNTIME_DMA_PROOF"] == "fresh-pi"
    assert incomplete_record["PI4_RUNTIME_DMA_COUNTER_PROOF"] == "diagnostic"


def test_gate_summary_rejects_pre_seal_runtime_dma_as_fresh_pi() -> None:
    """Old descriptor-present proof is not current sealed runtime proof."""

    events = normalizer.parse_events(
        strip_driver_task_runtime_descriptor_seals(strict_wired_boot_proof_lines())
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_OWNER_STATE_PROOF"] == "yes"
    assert record["DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF"] == "no"
    assert record["DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOFS"] == 0
    assert record["DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_BLOCKER"].endswith(
        ":descriptor-version-missing"
    )
    assert record["PI4_RUNTIME_DMA_PROOF"] == "diagnostic"
    assert "driver-task-runtime-descriptor-seal-missing" in (
        normalizer.boot_evidence_blockers(record)
    )


def test_gate_summary_rejects_v7_runtime_descriptor_as_stale() -> None:
    """ABI v7 owner proof remains historical and cannot satisfy v8 closure."""

    stale_lines = [
        line.replace("descriptor_version=8", "descriptor_version=7")
        for line in strict_wired_boot_proof_lines()
    ]
    record = normalizer.summarize_gates(
        normalizer.parse_events(stale_lines)
    ).to_record()

    assert record["DRIVER_TASK_OWNER_STATE_PROOF"] == "yes"
    assert record["DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF"] == "no"
    assert record["DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_BLOCKER"].endswith(
        ":descriptor-version-missing"
    )
    assert record["PI4_RUNTIME_DMA_PROOF"] == "diagnostic"


def test_gate_summary_accepts_late_wired_owner_state_refresh() -> None:
    """Resolved Pi owner-state replay should close the wired runtime/DMA proof."""

    events = normalizer.parse_events(
        seal_driver_task_runtime_descriptor_lines([
            "U-Boot 2026.01",
            "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
            "usb: platform_reset policy=full-reset-start "
            "origin=live-runtime-default handoff=none seed=none run=run-cold",
            "[Cohesix] Root console ready (type 'help' for commands)",
            "cohesix> driver proof",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=serial tcb=0x064e "
            "runtime_descriptor=yes reason=root-shell-before-first-service-proof",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=usb-local-seat tcb=0x08f2 "
            "runtime_descriptor=yes reason=root-shell-before-first-service-proof",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=pcie-root tcb=0x0713 "
            "runtime_descriptor=yes reason=root-shell-before-first-service-proof",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-owner-state "
            "status=blocked-first-report detail=0x0500 result=0 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=serial hot_path=serial-console "
            "stage=runtime-descriptor-replay status=ready detail=0x0501 "
            "result=1 frame_len=0",
            "DRIVER_TASK_RUNTIME_INIT_DEFERRED contract=serial "
            "hot_path=serial-console status=resumed owner=linked-runtime "
            "root_action=descriptor-replay action=steady-service-enabled",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=runtime-descriptor-replay "
            "status=ready detail=0x0501 result=1 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-owner-state "
            "status=ready detail=0x0501 result=1 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=pcie-root hot_path=pcie-root "
            "stage=runtime-descriptor-replay status=ready detail=0x0501 "
            "result=1 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=pcie-root hot_path=pcie-root "
            "stage=pcie-owner-state status=ready detail=0x0501 "
            "result=1 frame_len=0",
            "[timers] backend=arch-counter counter=vct timer_freq_hz=54000000",
            "DRIVER_TASK_DEFAULT requested=dedicated required=yes "
            "substrate_active=yes live_hot_paths=yes",
            "DRIVER_TASK_SELECTED profile=pi4-uboot-aarch64 selection=wired "
            "active_net=genet required_roles=0x2f required_hot_paths=0x2f "
            "required_tasks=5",
            "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 "
            "task_count=7 failed_count=0 live_tcb_count=7 "
            "root_authority=admission-descriptor-diagnostics-only "
            "hardware_owner=linked-runtime fault_endpoint_ready=yes "
            "revoke_ready=yes broad_caps_leaked=0 sched=yes "
            "affinity=per-driver affinity_configured=7 affinity_applied=7 "
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
            "DRIVER_TASK_OWNER_STATE contract=pcie-root hot_path=pcie-root "
            "owner_state=driver-owned descriptor=present root_pointer=no",
            "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=18",
            "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=22",
            "SCHED_CONTRACT contract=hdmi-text isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=44",
            "SCHED_CONTRACT contract=bcmgenet-v5 isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=31",
            "SCHED_CONTRACT contract=pcie-root isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=36",
            "DRIVER_TASK_DMA_PROOF contract=serial hot_path=serial-console "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=0 dma_pages=0 "
            "shared_pages=4 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=0 "
            "cache_clean_bytes=0 cache_invalidate_ops=0 cache_invalidate_bytes=0 "
            "proof_effect=runtime-dma-proof-ready",
            "DRIVER_TASK_DMA_PROOF contract=usb-local-seat hot_path=usb-keyboard "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=0 dma_pages=128 "
            "shared_pages=32 bus_address_policy=hal-bounded-bus-address "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=1 "
            "cache_clean_bytes=64 cache_invalidate_ops=1 cache_invalidate_bytes=64 "
            "proof_effect=runtime-dma-proof-ready",
            "DRIVER_TASK_DMA_PROOF contract=hdmi-text hot_path=hdmi-text "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=0 dma_pages=0 "
            "shared_pages=16 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=0 "
            "cache_clean_bytes=0 cache_invalidate_ops=0 cache_invalidate_bytes=0 "
            "proof_effect=runtime-dma-proof-ready",
            "DRIVER_TASK_DMA_PROOF contract=bcmgenet-v5 hot_path=genet-nic "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=6 dma_pages=64 "
            "shared_pages=32 bus_address_policy=hal-bounded-bus-address "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=1 "
            "cache_clean_bytes=64 cache_invalidate_ops=1 cache_invalidate_bytes=64 "
            "proof_effect=runtime-dma-proof-ready",
            "DRIVER_TASK_DMA_PROOF contract=pcie-root hot_path=pcie-root "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=11 dma_pages=0 "
            "shared_pages=16 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=0 "
            "cache_clean_bytes=0 cache_invalidate_ops=0 cache_invalidate_bytes=0 "
            "proof_effect=runtime-dma-proof-ready",
            "DRIVER_TASK_COUNTER contract=usb-local-seat hot_path=usb-keyboard "
            "source=root-ring sequence=1 submitted=2 completed=2 idle=0 fault=0 "
            "budget=0 frame=1 desc=1 staged_bytes=64 clean_ops=1 clean_bytes=64 "
            "inv_ops=1 inv_bytes=64 sends=2 yields=0 busy=0 same_request=0 "
            "timeouts=0 keep_active=0 aborts=0 overruns=0 drops=0 rx_frames=1 "
            "rx_bytes=8 tx_frames=1 tx_bytes=8 role_aux0=0 role_aux1=0 "
            "role_aux2=0 role_aux3=0",
            "DRIVER_TASK_ACCEPTANCE dedicated_ready=yes reason=active-substrate "
            "substrate=active capset=pass fault=pass revoke=pass sched=pass "
            "affinity=pass vspace=isolated ipc_abi=shared-ring-command "
            "pointer_free_ipc=yes owner_state=driver-owned required=5 "
            "dedicated=5 compatibility=0 active_net=genet live_hot_paths=yes",
        ])
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_BOOTSTRAP_DEFERRED"] == 0
    assert (
        record["DRIVER_TASK_RESOURCE_BLOCKER"]
        == "usb-keyboard:usb-owner-state:blocked-first-report"
    )
    assert record["DRIVER_TASK_RESOURCE_CURRENT_BLOCKER"] == "none"
    assert record["DRIVER_TASK_OWNER_STATE_PROOF"] == "yes"
    assert record["DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF"] == "yes"
    assert record["DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOFS"] == 5
    assert record["DRIVER_TASK_DMA_PROOFS"] == 5
    assert record["DRIVER_TASK_DMA_BLOCKER"] == "none"
    assert record["PI4_RUNTIME_DMA_PROOF"] == "fresh-pi"
    assert record["PI4_RUNTIME_DMA_PROOF_REASON"] == "live-pi-owner-state"
    assert record["PI4_RUNTIME_DMA_COUNTER_PROOF"] == "counter-qualified"


def test_gate_summary_replaces_superseded_dma_blockers() -> None:
    """Later ready DMA proof should replace early owner-state-missing output."""

    events = normalizer.parse_events(
        seal_driver_task_runtime_descriptor_lines([
            "U-Boot 2026.01",
            "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
            "usb: platform_reset policy=full-reset-start "
            "origin=live-runtime-default handoff=none seed=none run=run-cold",
            "[Cohesix] Root console ready (type 'help' for commands)",
            "cohesix>",
            "[timers] backend=arch-counter counter=vct timer_freq_hz=54000000",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=serial tcb=0x05bd "
            "runtime_descriptor=yes reason=root-shell-before-first-service-proof",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=usb-local-seat tcb=0x07a7 "
            "runtime_descriptor=yes reason=root-shell-before-first-service-proof",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=pcie-root tcb=0x06a2 "
            "runtime_descriptor=yes reason=root-shell-before-first-service-proof",
            "DRIVER_TASK_DEFAULT requested=dedicated required=yes "
            "substrate_active=yes live_hot_paths=no",
            "DRIVER_TASK_SELECTED profile=pi4-hardware selection=wired "
            "active_net=genet required_roles=0x2f required_hot_paths=0x4f "
            "required_tasks=5",
            "DRIVER_TASK_SUBSTRATE active=yes profile=pi4-uboot-aarch64 "
            "task_count=5 failed_count=0 live_tcb_count=5 "
            "root_authority=admission-descriptor-diagnostics-only "
            "hardware_owner=linked-runtime fault_endpoint_ready=yes "
            "revoke_ready=yes broad_caps_leaked=0 sched=yes "
            "affinity=per-driver affinity_configured=5 affinity_applied=5 "
            "vspace=isolated ipc_abi=shared-ring-command pointer_free_ipc=yes",
            "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=root-task-compatibility",
            "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=root-task-compatibility",
            "SCHED_CONTRACT contract=hdmi-text isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=44",
            "SCHED_CONTRACT contract=bcmgenet-v5 isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=31",
            "SCHED_CONTRACT contract=pcie-root isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=root-task-compatibility",
            "DRIVER_TASK_OWNER_STATE contract=serial hot_path=serial-console "
            "owner_state=missing descriptor=missing root_pointer=unknown",
            "DRIVER_TASK_DMA_PROOF contract=serial hot_path=serial-console "
            "status=owner-state-missing profile=bounded-no-iommu descriptor=present "
            "root_pointer=unknown owner=unproven mmio_pages=1 dma_pages=0 "
            "shared_pages=4 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance",
            "DRIVER_TASK_OWNER_STATE contract=usb-local-seat hot_path=usb-keyboard "
            "owner_state=missing descriptor=missing root_pointer=unknown",
            "DRIVER_TASK_DMA_PROOF contract=usb-local-seat hot_path=usb-keyboard "
            "status=owner-state-missing profile=bounded-no-iommu descriptor=present "
            "root_pointer=unknown owner=unproven mmio_pages=16 dma_pages=128 "
            "shared_pages=32 bus_address_policy=hal-bounded-bus-address "
            "cache_policy=uncached-plus-root-maintenance",
            "DRIVER_TASK_OWNER_STATE contract=pcie-root hot_path=pcie-root "
            "owner_state=missing descriptor=missing root_pointer=unknown",
            "DRIVER_TASK_DMA_PROOF contract=pcie-root hot_path=pcie-root "
            "status=owner-state-missing profile=bounded-no-iommu descriptor=present "
            "root_pointer=unknown owner=unproven mmio_pages=10 dma_pages=0 "
            "shared_pages=16 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance",
            "DRIVER_TASK_DEFAULT requested=dedicated required=yes "
            "substrate_active=yes live_hot_paths=yes",
            "DRIVER_TASK_SELECTED profile=pi4-hardware selection=wired "
            "active_net=genet required_roles=0x2f required_hot_paths=0x4f "
            "required_tasks=5",
            "DRIVER_TASK_OWNER_STATE contract=serial hot_path=serial-console "
            "owner_state=driver-owned descriptor=present root_pointer=no",
            "DRIVER_TASK_DMA_PROOF contract=serial hot_path=serial-console "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=1 dma_pages=0 "
            "shared_pages=4 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance",
            "DRIVER_TASK_OWNER_STATE contract=usb-local-seat hot_path=usb-keyboard "
            "owner_state=driver-owned descriptor=present root_pointer=no",
            "DRIVER_TASK_DMA_PROOF contract=usb-local-seat hot_path=usb-keyboard "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=16 dma_pages=128 "
            "shared_pages=32 bus_address_policy=hal-bounded-bus-address "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=1 "
            "cache_clean_bytes=64 cache_invalidate_ops=1 cache_invalidate_bytes=64",
            "DRIVER_TASK_OWNER_STATE contract=hdmi-text hot_path=hdmi-text "
            "owner_state=driver-owned descriptor=present root_pointer=no",
            "DRIVER_TASK_DMA_PROOF contract=hdmi-text hot_path=hdmi-text "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=0 dma_pages=0 "
            "shared_pages=16 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance",
            "DRIVER_TASK_OWNER_STATE contract=bcmgenet-v5 hot_path=genet-nic "
            "owner_state=driver-owned descriptor=present root_pointer=no",
            "DRIVER_TASK_DMA_PROOF contract=bcmgenet-v5 hot_path=genet-nic "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=6 dma_pages=64 "
            "shared_pages=32 bus_address_policy=hal-bounded-bus-address "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=1 "
            "cache_clean_bytes=64 cache_invalidate_ops=1 cache_invalidate_bytes=64",
            "DRIVER_TASK_OWNER_STATE contract=pcie-root hot_path=pcie-root "
            "owner_state=driver-owned descriptor=present root_pointer=no",
            "DRIVER_TASK_DMA_PROOF contract=pcie-root hot_path=pcie-root "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=11 dma_pages=0 "
            "shared_pages=16 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance",
            "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=18",
            "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=22",
            "SCHED_CONTRACT contract=pcie-root isolation=dedicated-sel4-task "
            "live_tcb=yes hot_path=dedicated observed_service_us=36",
            "DRIVER_TASK_COUNTER contract=usb-local-seat hot_path=usb-keyboard "
            "source=root-ring sequence=1 submitted=2 completed=2 idle=0 fault=0 "
            "budget=0 frame=1 desc=1 staged_bytes=64 clean_ops=1 clean_bytes=64 "
            "inv_ops=1 inv_bytes=64 sends=2 yields=0 busy=0 same_request=0 "
            "timeouts=0 keep_active=0 aborts=0 overruns=0 drops=0 rx_frames=1 "
            "rx_bytes=8 tx_frames=1 tx_bytes=8 role_aux0=0 role_aux1=0 "
            "role_aux2=0 role_aux3=0",
            "DRIVER_TASK_ACCEPTANCE dedicated_ready=yes "
            "reason=dedicated-sel4-substrate-active active_net=genet "
            "substrate=active capset=pass fault=pass revoke=pass sched=pass "
            "affinity=pass vspace=isolated ipc_abi=shared-ring-command "
            "pointer_free_ipc=yes owner_state=driver-owned required=5 "
            "dedicated=5 compatibility=0 live_hot_paths=yes",
        ])
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["DRIVER_TASK_BOOTSTRAP_DEFERRED"] == 0
    assert record["DRIVER_TASK_OWNER_STATE_PROOF"] == "yes"
    assert record["DRIVER_TASK_RUNTIME_DESCRIPTOR_SEAL_PROOF"] == "yes"
    assert record["DRIVER_TASK_DMA_PROOFS"] == 5
    assert record["DRIVER_TASK_DMA_BLOCKER"] == "none"
    assert record["PI4_RUNTIME_DMA_PROOF"] == "fresh-pi"
    assert record["PI4_RUNTIME_DMA_PROOF_REASON"] == "live-pi-owner-state"
    assert record["PI4_RUNTIME_DMA_COUNTER_PROOF"] == "counter-qualified"


def test_gate_summary_splits_glued_serial_trace_segment() -> None:
    """UART prompt capture can glue typed input before SERIAL_INPUT_TRACE."""

    events = normalizer.parse_events(
        [
            "cohesix> usb statusSERIAL_INPUT_TRACE stage=consume-line "
            "route=bcm2711-mini-uart line_len=10 rx_depth=0 partial_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["SERIAL_RESPONSIVE_PROOF"] == "yes"


def test_gate_summary_rejects_zero_smp_root_queue_counters_as_serial_proof() -> None:
    """Root queue health alone cannot prove isolated-runtime or UART delivery."""

    events = normalizer.parse_events(
        [
            "[smp] activity pump now_ms=336580 input=local-seat lines=3 ok=1 "
            "denied=1 ticks=66748 serial_rx_drop=0 serial_tx_drop=0 "
            "utf8_drop=0 serial_budget_overruns=0 "
            "serial_rx_backpressure=0 serial_tx_backpressure=0 "
            "serial_pressure_source=uart-output",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["SERIAL_CLEAN"] == "yes"
    assert record["SERIAL_RESPONSIVE_PROOF"] == "no"


def test_usb_command_ready_ignores_later_wifi_blockers() -> None:
    """CYW43 blocker fields must not poison recovered USB command readiness."""

    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready action=enable-command-input "
            "clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x68 detail=0x0000 result=0x00000001",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "command_ready=yes proof_gate=10 target_gate=10 blocker=none",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "wifi: evidence boundary failure_domain=cyw43-control-tx-not-submitted "
            "blocker=cyw43-control-tx-not-submitted detail=0x5103",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_LOCAL_SEAT_STATE"] == "ready"
    assert record["USB_LOCAL_SEAT_REASON"] == "command-ready"
    assert record["USB_BUSY_AFTER_READY"] == "no"


def test_gate_summary_marks_prompt_prefixed_trace_as_unclean() -> None:
    """Prompt/log interleaving must not look like clean operator evidence."""

    events = normalizer.parse_events(
        [
            "cohesix> HDMI_FRAME_SUBMIT reason=keyboard-scrollback status=ready "
            "root_console_ready=yes attached=yes bytes=220",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.to_record()["HDMI_RESPONSIVE_PROOF"] == "yes"
    assert gates.to_record()["SERIAL_CLEAN"] == "no"
    assert any(
        event.fields.get("serial_error") == "prompt-prefixed-trace"
        for event in events
    )


def test_gate_summary_clears_recovery_request_after_sustained_input() -> None:
    """Later sustained input progress closes earlier post-first-byte recovery noise."""

    events = normalizer.parse_events(
        [
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x65",
            "usb: recovery_request action=no-reply aux0=0x55534252 "
            "no_reply=27 streak=9 cooldown=2 recovery_aux_requests=1 "
            "recovery_aux_pending=yes queue_empty=yes accepted=12 drained=11 "
            "echoed=10 detail=0x0501 result=0x00000220 queued_reports=1 "
            "report_status=none report_status_code=0",
            "usb: sustained_input queue_valid=yes detail=0x0501 "
            "result=0x64000020 queued_reports=1 transfer_events=100 "
            "report_status=none accepted=14 drained=14 echoed=14 "
            "no_reply=27 no_reply_streak=0 recovery_aux_requests=1 "
            "recovery_aux_pending=no blocker=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_POST_FIRST_BYTE_BLOCKER"] == "none"
    assert record["USB_BURST_PROOF"] == "yes"
    assert record["USB_BURST_DROPS"] == 0


def test_gate_summary_tracks_cyw43_data_path_trace_counts() -> None:
    """Gate summaries surface CYW43 DHCP/ARP TX and RX handoff telemetry."""

    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_DATA_PATH contract=cyw43455 event=tx-result "
            "action=submitted attempt=1 len=286 ethertype=0x0800 ip_proto=17 "
            "udp_src=68 udp_dst=67 dhcp=discover arp=none tx_total_len=328 "
            "tx_request_len=328 cmd53_mode=block block_size=64 block_count=6 "
            "completion_code=1 completion_detail=0x0148 completion_result=0x00000148 "
            "completion_flags=0x0000 completion_len=0 pending_before=no pending_after=no",
            "CYW43_DRIVER_TASK_DATA_PATH contract=cyw43455 event=rx-preserve "
            "action=pre-poll attempt=0 len=286 ethertype=0x0800 ip_proto=17 "
            "udp_src=67 udp_dst=68 dhcp=offer arp=none tx_total_len=328 "
            "tx_request_len=328 cmd53_mode=block block_size=64 block_count=6 "
            "completion_code=2 completion_detail=0x0000 completion_result=0x0000011e "
            "completion_flags=0x0001 completion_len=286 pending_before=no pending_after=yes",
            "CYW43_DRIVER_TASK_DATA_PATH contract=cyw43455 event=rx-deliver "
            "action=pending attempt=0 len=42 ethertype=0x0806 ip_proto=0 "
            "udp_src=0 udp_dst=0 dhcp=none arp=reply tx_total_len=84 "
            "tx_request_len=84 cmd53_mode=byte block_size=84 block_count=0 "
            "completion_code=0 completion_detail=0x0000 completion_result=0x00000000 "
            "completion_flags=0x0000 completion_len=0 pending_before=yes pending_after=no",
            "CYW43_DRIVER_TASK_DATA_PATH contract=cyw43455 event=rx-preserve-drop "
            "action=pre-poll attempt=0 len=286 ethertype=0x0800 ip_proto=17 "
            "udp_src=67 udp_dst=68 dhcp=ack arp=none tx_total_len=328 "
            "tx_request_len=328 cmd53_mode=block block_size=64 block_count=6 "
            "completion_code=2 completion_detail=0x0000 completion_result=0x0000011e "
            "completion_flags=0x0001 completion_len=286 pending_before=yes pending_after=yes",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DATA_PATH_TX"] == 1
    assert record["WIFI_DATA_PATH_RX_PRESERVED"] == 1
    assert record["WIFI_DATA_PATH_RX_DELIVERED"] == 1
    assert record["WIFI_DATA_PATH_RX_DROPPED"] == 1
    assert record["WIFI_DATA_PATH_LAST"] == "rx-preserve-drop:pre-poll:dhcp-ack"


def test_gate_summary_surfaces_priority_episode_and_first_recovery_scheduler() -> None:
    """Scheduling diagnostics retain exact scopes without changing gate truth."""

    lines = [
        "wifi: priority_episode scope=current phase=open pair_epoch=7 mask=0x03",
        "wifi: priority_episode scope=current phase=open pair_epoch=4294967296 mask=0x04",
        "wifi: priority_episode_counts scope=boot-cumulative opens=2 closes=1 "
        "restores=1 amortized_requests=9",
        "wifi: priority_episode_faults scope=boot-cumulative failures=3 "
        "recovery_revocations=4",
        "wifi: deferred_recovery scheduler scope=first-pre-fence "
        "cause=persistent-parent-stable-invalid "
        "outer=open/7/0x03 root=yes/issued/0x03/64/11 "
        "command_sequence=64",
        "wifi: deferred_recovery scheduler_edge publication_latched=yes "
        "signal_returned=yes parent_deadline_expired=yes "
        "child_terminal=no child_wait_receipt=no child_bus_episode=no "
        "bus_parent=0/0x0000 rsl=39579 "
        "evidence=exact-only",
        "wifi: deferred_recovery scheduler scope=first-pre-fence "
        "outer=closing/8/0x03 root=malformed command_sequence=65 "
        "doorbell_issued=no",
        "wifi: deferred_recovery scheduler scope=first-pre-fence "
        "outer=closing/4294967296/0x04 "
        "root=yes/not-a-phase/0x04/4294967296/4294967296 "
        "command_sequence=4294967296 doorbell_issued=no",
        "wifi: root grant state=issued active=yes phase=issued mask=0x03 "
        "request=64 generation=11 command_sequence=64 sequence_published=yes "
        "doorbell_issued=yes",
        "wifi: root grant_ids notify_bound=yes producer=9 shared=9 consumed=8 exact=yes",
    ]
    events = normalizer.parse_events(lines)
    record = normalizer.summarize_gates(events).to_record()
    baseline = normalizer.summarize_gates([]).to_record()

    assert [event.stage for event in events] == [
        "priority-episode",
        "priority-episode",
        "priority-episode-counts",
        "priority-episode-faults",
        "deferred-recovery-scheduler",
        "deferred-recovery-scheduler-edge",
        "deferred-recovery-scheduler",
        "deferred-recovery-scheduler",
        "root-grant",
        "root-grant-ids",
    ]
    assert events[4].fields["outer"] == "open/7/0x03"
    assert events[4].fields["root"] == "yes/issued/0x03/64/11"
    assert events[4].fields["cause"] == "persistent-parent-stable-invalid"
    assert events[5].fields["publication_latched"] == "yes"
    assert events[5].fields["signal_returned"] == "yes"
    assert events[5].fields["parent_deadline_expired"] == "yes"
    assert events[5].fields["child_terminal"] == "no"
    assert events[5].fields["child_wait_receipt"] == "no"
    assert events[5].fields["child_bus_episode"] == "no"
    assert events[5].fields["bus_parent"] == "0/0x0000"
    assert events[5].fields["rsl"] == "39579"
    assert events[5].fields["evidence"] == "exact-only"
    assert events[8].fields["command_sequence"] == "64"
    assert events[8].fields["doorbell_issued"] == "yes"
    assert record["WIFI_PRIORITY_EPISODE_SCOPE"] == "current"
    assert record["WIFI_PRIORITY_EPISODE_PHASE"] == "open"
    assert record["WIFI_PRIORITY_EPISODE_PAIR_EPOCH"] == 7
    assert record["WIFI_PRIORITY_EPISODE_MASK"] == "0x03"
    assert record["WIFI_PRIORITY_EPISODE_COUNTS_SCOPE"] == "boot-cumulative"
    assert record["WIFI_PRIORITY_EPISODE_FAULTS_SCOPE"] == "boot-cumulative"
    assert record["WIFI_PRIORITY_EPISODE_OPENS"] == 2
    assert record["WIFI_PRIORITY_EPISODE_CLOSES"] == 1
    assert record["WIFI_PRIORITY_EPISODE_RESTORES"] == 1
    assert record["WIFI_PRIORITY_EPISODE_AMORTIZED_REQUESTS"] == 9
    assert record["WIFI_PRIORITY_EPISODE_FAILURES"] == 3
    assert record["WIFI_PRIORITY_EPISODE_RECOVERY_REVOCATIONS"] == 4
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_SCOPE"] == "first-pre-fence"
    assert (
        record["WIFI_DEFERRED_RECOVERY_SCHEDULER_CAUSE"]
        == "persistent-parent-stable-invalid"
    )
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_OUTER_PHASE"] == "open"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_OUTER_PAIR_EPOCH"] == 7
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_OUTER_MASK"] == "0x03"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_ACTIVE"] == "yes"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_PHASE"] == "issued"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_MASK"] == "0x03"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_REQUEST"] == 64
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_GENERATION"] == 11
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_COMMAND_SEQUENCE"] == 64
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_DOORBELL_ISSUED"] == "yes"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_PUBLICATION_LATCHED"] == "yes"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_SIGNAL_RETURNED"] == "yes"
    assert (
        record["WIFI_DEFERRED_RECOVERY_SCHEDULER_PARENT_DEADLINE_EXPIRED"]
        == "yes"
    )
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_CHILD_TERMINAL"] == "no"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_CHILD_WAIT_RECEIPT"] == "no"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_CHILD_BUS_EPISODE"] == "no"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_BUS_PARENT_SEQUENCE"] == 0
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_BUS_PARENT_OP"] == "0x0000"
    assert record["WIFI_DEFERRED_RECOVERY_RUNTIME_SOURCE_LINE"] == 39579
    assert record["WIFI_CAUSAL_FRONTIER"] == "root-signal-returned"
    for key in (
        "WIFI_GATE",
        "WIFI_BLOCKER",
        "WIFI_GATE8_COMPLETE",
        "WIFI_GATE8_STATUS",
        "WIFI_DPC_PROOF",
        "WIFI_DPC_REASON",
    ):
        assert record[key] == baseline[key]


def test_typed_first_recovery_cause_survives_generic_supervisor_failure() -> None:
    """The retained scheduler cause and recovery stage remain the exact fault."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            "wifi: deferred_recovery retained=yes refinement=exact-owner "
            "logical_terminal_observed=no cause=pair-signal "
            "subphase=cyw43-control-txglomalign gate=0 current=yes "
            "live_generation=0",
            "wifi: deferred_recovery scheduler scope=first-pre-fence "
            "cause=persistent-parent-stable-invalid "
            "outer=inactive/0/0x00 root=yes/issued/0x00/27/0 "
            "command_sequence=27",
            "wifi: deferred_recovery scheduler_edge publication_latched=yes "
            "signal_returned=yes parent_deadline_expired=no "
            "child_terminal=no child_wait_receipt=no child_bus_episode=no "
            "bus_parent=0/0x0000 evidence=exact-only",
            bootstrap_supervisor_line(
                1,
                "failed",
                0,
                normalizer.CYW43_BOOTSTRAP_NO_ATTEMPT_MS,
                2,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 7
    assert record["WIFI_BLOCKER"] == "supervisor-failed"
    assert record["WIFI_GATE8_BLOCKER"] == "supervisor-failed"
    assert record["WIFI_EXACT"] == "persistent-parent-stable-invalid"
    assert record["WIFI_PHASE"] == "cyw43-control-txglomalign"
    assert record["WIFI_CAUSAL_FRONTIER"] == "root-signal-returned"


def test_priority_episode_count_and_fault_scopes_are_independent() -> None:
    """Absent split records remain unobserved rather than fabricated zeros."""

    counts_only = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                "wifi: priority_episode_counts scope=boot-cumulative opens=2 "
                "closes=1 restores=1 amortized_requests=9"
            ]
        )
    ).to_record()
    faults_only = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                "wifi: priority_episode_faults scope=boot-cumulative failures=3 "
                "recovery_revocations=4"
            ]
        )
    ).to_record()

    assert counts_only["WIFI_PRIORITY_EPISODE_COUNTS_SCOPE"] == "boot-cumulative"
    assert counts_only["WIFI_PRIORITY_EPISODE_FAULTS_SCOPE"] == "none"
    assert counts_only["WIFI_PRIORITY_EPISODE_FAILURES"] == 0
    assert faults_only["WIFI_PRIORITY_EPISODE_COUNTS_SCOPE"] == "none"
    assert faults_only["WIFI_PRIORITY_EPISODE_FAULTS_SCOPE"] == "boot-cumulative"
    assert faults_only["WIFI_PRIORITY_EPISODE_OPENS"] == 0


def test_unavailable_recovery_scheduler_is_parsed_but_not_promoted_to_proof() -> None:
    """A missing immutable tuple stays visible without fabricating its scope."""

    events = normalizer.parse_events(
        [
            "wifi: deferred_recovery scheduler scope=unavailable "
            "outer=unavailable/0/0x00 root=no/unavailable/0x00/0/0 "
            "command_sequence=0 doorbell_issued=no"
        ]
    )
    record = normalizer.summarize_gates(events).to_record()

    assert [event.stage for event in events] == ["deferred-recovery-scheduler"]
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_SCOPE"] == "none"


def test_recovery_scheduler_keeps_legacy_doorbell_compatibility() -> None:
    """Legacy doorbell telemetry maps only to the publication latch fact."""

    events = normalizer.parse_events(
        [
            "wifi: deferred_recovery scheduler scope=first-pre-fence "
            "outer=open/7/0x03 root=yes/issued/0x03/64/11 "
            "command_sequence=64 doorbell_issued=yes"
        ]
    )
    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_ROOT_DOORBELL_ISSUED"] == "yes"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_PUBLICATION_LATCHED"] == "yes"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_SIGNAL_RETURNED"] == "unknown"
    assert (
        record["WIFI_DEFERRED_RECOVERY_SCHEDULER_PARENT_DEADLINE_EXPIRED"]
        == "unknown"
    )
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_CHILD_TERMINAL"] == "unknown"
    assert (
        record["WIFI_DEFERRED_RECOVERY_SCHEDULER_CHILD_WAIT_RECEIPT"]
        == "unknown"
    )
    assert (
        record["WIFI_DEFERRED_RECOVERY_SCHEDULER_CHILD_BUS_EPISODE"]
        == "unknown"
    )
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_BUS_PARENT_SEQUENCE"] == 0
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_BUS_PARENT_OP"] == "0x0000"
    assert record["WIFI_CAUSAL_FRONTIER"] == "root-issued-phase"


def test_recovery_scheduler_edge_requires_adjacent_retained_tuple() -> None:
    """A delayed edge cannot attach child facts to an older scheduler tuple."""

    events = normalizer.parse_events(
        [
            "wifi: deferred_recovery scheduler scope=first-pre-fence "
            "outer=open/7/0x03 root=yes/issued/0x03/64/11 "
            "command_sequence=64 doorbell_issued=yes",
            "wifi: priority_episode scope=current phase=poisoned "
            "pair_epoch=7 mask=0x00",
            "wifi: deferred_recovery scheduler_edge publication_latched=yes "
            "signal_returned=yes parent_deadline_expired=yes "
            "child_terminal=no child_wait_receipt=no child_bus_episode=no "
            "bus_parent=0/0x0000 evidence=exact-only",
        ]
    )
    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_PUBLICATION_LATCHED"] == "yes"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_SIGNAL_RETURNED"] == "unknown"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_CHILD_TERMINAL"] == "unknown"
    assert record["WIFI_DEFERRED_RECOVERY_SCHEDULER_BUS_PARENT_SEQUENCE"] == 0
    assert record["WIFI_CAUSAL_FRONTIER"] == "root-issued-phase"


@pytest.mark.parametrize(
    ("action", "active_descriptor", "active_op"),
    [
        ("no-completion-active-tx", "eth-tx", "0x0007"),
        ("no-completion-active-control-frame", "control-frame", "0x0006"),
        ("no-completion-active-control-exchange", "control-exchange", "0x000b"),
        ("no-completion-active-other", "other", "0xffff"),
    ],
)
def test_gate_summary_preserves_cyw43_active_no_completion_actions(
    action: str, active_descriptor: str, active_op: str
) -> None:
    """Active-descriptor TX stalls must survive normalization for WiFi proof."""

    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_DATA_PATH contract=cyw43455 event=tx-result "
            f"action={action} attempt=1 len=42 channel=none "
            f"active_descriptor={active_descriptor} active_op={active_op} "
            "ethertype=0x0806 ip_proto=0 udp_src=0 udp_dst=0 dhcp=none "
            "arp=request tx_total_len=84 tx_request_len=84 cmd53_mode=byte "
            "block_size=84 block_count=0 completion_code=0 completion_detail=0x0000 "
            "completion_result=0x00000000 completion_flags=0x0000 completion_len=0 "
            "pending_before=no pending_after=no",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DATA_PATH_TX"] == 1
    assert record["WIFI_DATA_PATH_LAST"] == f"tx-result:{action}:arp-request"


def test_gate_summary_accepts_sustained_input_usb_burst_field() -> None:
    """The root-task sustained-input line may carry USB burst proof directly."""

    events = normalizer.parse_events(
        [
            "usb: sustained_input queued_reports=1 transfer_events=128 "
            "report_status=produced-byte accepted=512 drained=512 echoed=512 "
            "no_reply=0 runtime_skipped=8 blocker=none usb_burst=yes drops=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_BURST_PROOF"] == "yes"
    assert record["USB_BURST_DROPS"] == 0
    assert record["USB_POST_FIRST_BYTE_BLOCKER"] == "none"
    assert record["USB_RUNTIME_QUEUED_REPORTS"] == 1
    assert record["USB_RUNTIME_REPORT_STATUS"] == "produced-byte"
    assert record["USB_EVENT_LOOP_RUNTIME_SKIPPED"] == 8


def test_gate_summary_tracks_driver_task_counter_snapshots() -> None:
    """Counter snapshots are diagnostic evidence and must be activity-bearing."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_COUNTER contract=cyw43455 hot_path=cyw43-wifi "
            "source=root-ring sequence=12 submitted=3 completed=2 idle=1 "
            "fault=0 budget=0 frame=1 desc=1 staged_bytes=256 clean_ops=4 "
            "clean_bytes=512 inv_ops=3 inv_bytes=60 sends=8 yields=8 busy=1 "
            "same_request=2 timeouts=3 keep_active=2 aborts=1 overruns=6 drops=7 "
            "rx_frames=5 "
            "rx_bytes=1500 tx_frames=4 tx_bytes=1200",
            "DRIVER_TASK_COUNTER contract=usb-local-seat hot_path=usb-keyboard "
            "source=root-ring sequence=0 submitted=0 completed=0 idle=0 "
            "fault=0 budget=0 frame=0 desc=0 staged_bytes=0 clean_ops=0 "
            "clean_bytes=0 inv_ops=0 inv_bytes=0 sends=0 yields=0 busy=0 "
            "same_request=0 timeouts=0 keep_active=0 aborts=0 overruns=0 drops=0 "
            "rx_frames=0 "
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
    assert record["DRIVER_TASK_COUNTER_OVERRUNS"] == 6
    assert record["DRIVER_TASK_COUNTER_DROPS"] == 7
    assert record["DRIVER_TASK_COUNTER_STAGED_BYTES"] == 256
    assert record["DRIVER_TASK_COUNTER_CACHE_OPS"] == 7
    assert record["DRIVER_TASK_COUNTER_CACHE_BYTES"] == 572
    assert record["DRIVER_TASK_COUNTER_RX_FRAMES"] == 5
    assert record["DRIVER_TASK_COUNTER_TX_FRAMES"] == 4
    assert record["DRIVER_TASK_COUNTER_RX_BYTES"] == 1500
    assert record["DRIVER_TASK_COUNTER_TX_BYTES"] == 1200
    assert record["DRIVER_TASK_DEDICATED_READY"] == "no"


def test_gate_summary_splits_serial_and_hdmi_output_pressure() -> None:
    events = normalizer.parse_events(
        [
            "usb: output_pressure serial_tx_pending=yes serial_interactive=no "
            "deferred=11 flushed=7 backpressure=3 hdmi_pending_bytes=144 "
            "hdmi_pending_redraw=yes hdmi_submitted=5 hdmi_deferred=4 "
            "hdmi_busy=2 hdmi_no_reply=1 hdmi_coalesced=8 "
            "hdmi_backpressure_bytes=13 hdmi_superseded_bytes=21",
            "[smp] activity local-seat-display pending_bytes=12 "
            "pending_redraw=no submitted=6 deferred=5 busy=3 no_reply=2 "
            "coalesced=9 backpressure_bytes=14 superseded_bytes=22",
            "usb: keyboard_trace source=linked-runtime polls=100 "
            "backend_bytes=4 queued=0 accepted=4 drained=4 echoed=4 "
            "dropped=0 overruns=2 no_reply=3 cooldown=4 cooldown_skips=5",
            "[smp] activity local-seat runtime=present attached=yes "
            "keyboard_device=usb-kbd0 display=hdmi0 backend_poll=yes "
            "backend_polls=120 backend_bytes=4 keyboard_ready=yes "
            "first_report=yes first_byte=yes queued=0 accepted=4 drained=4 "
            "echoed=4 drop=0 no_reply=6 cooldown=7 cooldown_skips=8 "
            "hdmi_drop=0",
            "[smp] activity local-seat-display pending_bytes=4096 "
            "pending_redraw=no submitted=8 deferred=9 busy=0 no_reply=1280 "
            "coalesced=10 backpressure_bytes=64 superseded_bytes=0",
            "cohesix> HDMI_FRAME_QUEUE reason=keyboard-scrollback "
            "chunk_bytes=512 chunk_redraw=yes generation=15 pending_bytes=128 "
            "redraw_bytes=64 pending_redraw=yes scrollback=0 open_line=yes "
            "submitted=9 deferred=10 busy=1 no_reply=11 cooldown=2",
            "HDMI_FRAME_COUNTERS reason=keyboard-scrollback coalesced=12 "
            "backpressure_bytes=15 superseded_bytes=33 "
            "redraw_no_reply_streak=2 stale_after_retry=yes",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["SERIAL_OUTPUT_TX_PENDING"] == "yes"
    assert record["SERIAL_OUTPUT_INTERACTIVE"] == "no"
    assert record["SERIAL_OUTPUT_DEFERRED"] == 11
    assert record["SERIAL_OUTPUT_FLUSHED"] == 7
    assert record["SERIAL_OUTPUT_BACKPRESSURE"] == 3
    assert record["HDMI_DISPLAY_PENDING_BYTES"] == 128
    assert record["HDMI_DISPLAY_PENDING_REDRAW"] == "yes"
    assert record["HDMI_DISPLAY_SUBMITTED"] == 9
    assert record["HDMI_DISPLAY_DEFERRED"] == 10
    assert record["HDMI_DISPLAY_BUSY"] == 1
    assert record["HDMI_DISPLAY_NO_REPLY"] == 11
    assert record["HDMI_DISPLAY_COALESCED"] == 12
    assert record["HDMI_DISPLAY_BACKPRESSURE_BYTES"] == 15
    assert record["HDMI_DISPLAY_SUPERSEDED_BYTES"] == 33
    assert record["USB_KEYBOARD_NO_REPLIES"] == 6
    assert record["USB_KEYBOARD_POLL_COOLDOWN"] == 7
    assert record["USB_KEYBOARD_COOLDOWN_SKIPS"] == 8


def test_gate_summary_treats_post_prompt_hdmi_frame_as_responsive_proof() -> None:
    events = normalizer.parse_events(
        [
            "cohesix> HDMI_FRAME_SUBMIT reason=keyboard-scrollback status=ready "
            "root_console_ready=yes attached=yes failed=no fatal=no redraw=yes "
            "bytes=220 chunk_index=0 chunk_count=1 payload_sig=0xc13cd357 "
            "completion_sequence=29 code=1 detail=0 result=220 frame_len=0",
            "HDMI_FRAME_QUEUE reason=keyboard-scrollback chunk_bytes=220 "
            "chunk_redraw=yes generation=5 pending_bytes=0 redraw_bytes=0 "
            "pending_redraw=no scrollback=0 open_line=no submitted=0 "
            "deferred=0 busy=0 no_reply=0 cooldown=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["HDMI_RESPONSIVE_PROOF"] == "yes"
    assert record["HDMI_DISPLAY_PENDING_BYTES"] == 0
    assert record["HDMI_DISPLAY_PENDING_REDRAW"] == "no"
    assert record["HDMI_DISPLAY_NO_REPLY"] == 0


def test_gate_summary_treats_in_progress_hdmi_queue_as_responsive_proof() -> None:
    events = normalizer.parse_events(
        [
            "HDMI_FRAME_QUEUE reason=keyboard-scrollback chunk_bytes=512 "
            "chunk_redraw=yes generation=5 pending_bytes=200 redraw_bytes=0 "
            "pending_redraw=no scrollback=0 open_line=no submitted=4 "
            "deferred=0 busy=0 no_reply=0 cooldown=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["HDMI_RESPONSIVE_PROOF"] == "yes"
    assert record["HDMI_DISPLAY_PENDING_BYTES"] == 200
    assert record["HDMI_DISPLAY_PENDING_REDRAW"] == "no"
    assert record["HDMI_DISPLAY_BUSY"] == 0
    assert record["HDMI_DISPLAY_NO_REPLY"] == 0


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
            "DRIVER_TASK_BOOT contract=hdmi-text role=display started=yes affinity_core=1",
            "DRIVER_TASK_BOOT contract=pcie-root role=pcie started=yes affinity_core=2",
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract=sdio-host tcb=0x0713 runtime_descriptor=yes",
            "DRIVER_TASK_BOOT contract=sdio-host role=sdio started=yes affinity_core=3",
            "DRIVER_TASK_BOOT contract=bcmgenet-v5 role=net started=yes affinity_core=1",
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
            "DRIVER_TASK_BOOT contract=hdmi-text role=display started=yes affinity_core=1",
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
            "DRIVER_TASK_BOOT contract=hdmi-text role=display started=yes affinity_core=1",
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
            "DRIVER_TASK_BOOT contract=hdmi-text role=display started=yes affinity_core=1",
            "DRIVER_TASK_BOOT contract=pcie-root role=pcie started=yes affinity_core=2",
            "DRIVER_TASK_BOOT contract=bcmgenet-v5 role=net started=yes affinity_core=1",
            "[smp] activity selected profile=pi4-hardware net=wired active_contracts=selected-only",
        ]
    )

    wired_record = normalizer.summarize_gates(wired_events).to_record()
    assert wired_record["DRIVER_TASK_AFFINITY_MANIFEST_PROOF"] == "yes"
    assert wired_record["DRIVER_TASK_AFFINITY_MANIFEST_MATCHES"] == 5
    assert wired_record["DRIVER_TASK_AFFINITY_MANIFEST_MISSING"] == 0
    assert wired_record["DRIVER_TASK_AFFINITY_MANIFEST_MISMATCHES"] == 0


def test_gate_summary_owner_state_follows_selected_pi4_network_profile() -> None:
    """Owner-state proof must require the selected NIC, not every NIC."""

    base_owner_state = [
        "DRIVER_TASK_OWNER_STATE contract=serial hot_path=serial-console "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=usb-local-seat hot_path=usb-keyboard "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=hdmi-text hot_path=hdmi-text "
        "owner_state=driver-owned descriptor=present root_pointer=no",
        "DRIVER_TASK_OWNER_STATE contract=pcie-root hot_path=pcie-root "
        "owner_state=driver-owned descriptor=present root_pointer=no",
    ]
    wifi_events = normalizer.parse_events(
        [
            "DRIVER_TASK_SELECTED profile=pi4-hardware selection=wifi active_net=cyw43",
            *base_owner_state,
            "DRIVER_TASK_OWNER_STATE contract=cyw43455 hot_path=cyw43-wifi "
            "owner_state=driver-owned descriptor=present root_pointer=no",
            "DRIVER_TASK_OWNER_STATE contract=sdio-host hot_path=sdio-host "
            "owner_state=driver-owned descriptor=present root_pointer=no",
        ]
    )
    wifi_record = normalizer.summarize_gates(wifi_events).to_record()
    assert wifi_record["DRIVER_TASK_OWNER_STATE_PROOF"] == "yes"
    assert wifi_record["DRIVER_TASK_ACTIVE_NET"] == "cyw43"

    missing_sdio_events = normalizer.parse_events(
        [
            "DRIVER_TASK_SELECTED profile=pi4-hardware selection=wifi active_net=cyw43",
            *base_owner_state,
            "DRIVER_TASK_OWNER_STATE contract=cyw43455 hot_path=cyw43-wifi "
            "owner_state=driver-owned descriptor=present root_pointer=no",
        ]
    )
    missing_sdio_record = normalizer.summarize_gates(missing_sdio_events).to_record()
    assert missing_sdio_record["DRIVER_TASK_OWNER_STATE_PROOF"] == "no"
    assert missing_sdio_record["DRIVER_TASK_ACTIVE_NET"] == "cyw43"

    wired_events = normalizer.parse_events(
        [
            "DRIVER_TASK_SELECTED profile=pi4-hardware selection=wired active_net=genet",
            *base_owner_state,
            "DRIVER_TASK_OWNER_STATE contract=bcmgenet-v5 hot_path=genet-nic "
            "owner_state=driver-owned descriptor=present root_pointer=no",
        ]
    )
    wired_record = normalizer.summarize_gates(wired_events).to_record()
    assert wired_record["DRIVER_TASK_OWNER_STATE_PROOF"] == "yes"
    assert wired_record["DRIVER_TASK_ACTIVE_NET"] == "genet"


def test_gate_summary_masks_wifi_gates_when_genet_is_selected() -> None:
    """A wired DHCP failure must not be scored as a WiFi bring-up failure."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_SELECTED profile=pi4-hardware selection=wired active_net=genet",
            "[net-console] root console bounded-wait reason=net-not-ready active=wired action=wait-for-net",
            "[dhcp] start backend=bcmgenet-v5 mode=dhcp interface=wired",
            "[dhcp] failed reason=timeout-exhausted discovers=4 active=wired",
            "netstats: mode=dhcp policy=wired active=wired standby=none "
            "addr_src=dhcp-failed ip=0.0.0.0 gateway=0.0.0.0 dhcp=failed",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_ACTIVE_NET"] == "genet"
    assert record["NET_ACTIVE"] == "wired"
    assert record["WIFI_GATE"] == 0
    assert record["WIFI_BLOCKER"] == "not-selected"
    assert record["WIFI_EXACT"] == "none"
    assert record["WIFI_OLDGOOD_MISSING"] == "not-selected"


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


def test_gate_summary_deduplicates_driver_task_summary_and_role_replay() -> None:
    """Aggregate counts and role-only replay facts must not mint contracts."""

    events = normalizer.parse_events(
        [
            "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task",
            "DRIVER_TASK role=serial contract=serial "
            "isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated",
            "SCHED_CONTRACT contract=cyw43455 isolation=dedicated-sel4-task",
            "DRIVER_TASK role=net contract=cyw43455 "
            "isolation=dedicated-sel4-task live_tcb=yes hot_path=dedicated",
            "DRIVER_TASK_SUMMARY contracts=2 dedicated=2 compatibility=0",
            "DRIVER_TASK_BUS_LINK contract=usb-keyboard owner=pcie-root "
            "channel=usb-pcie",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "events=1 blocker=none",
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host selected=yes "
            "events=1 blocker=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["DRIVER_TASK_CONTRACTS"] == 2
    assert record["DRIVER_TASK_DEDICATED"] == 2
    assert record["DRIVER_TASK_COMPATIBILITY"] == 0


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


def test_gate_summary_counts_observed_service_budget_overruns() -> None:
    """Observed service time above max_service_us must fail closed."""

    events = normalizer.parse_events(
        [
            "SCHED_CONTRACT contract=cyw43455 isolation=dedicated-sel4-task "
            "max_service_us=1000 observed_service_us=3281157",
            "SCHED_CONTRACT contract=usb-local-seat isolation=dedicated-sel4-task "
            "service_max_us=250 service_us=9278",
            "SCHED_CONTRACT contract=serial isolation=dedicated-sel4-task "
            "max_service_us=250 observed_service_us=18",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_BUDGET_OVERRUNS"] == 2
    assert record["DRIVER_TASK_LATENCY_PROOFS"] == 3


def test_gate_summary_ignores_zero_driver_task_budget_overrun_fields() -> None:
    """Zero-valued pressure fields must not count as budget-overrun evidence."""

    events = normalizer.parse_events(
        [
            "usb: local-seat drops keyboard_drop=0 driver_task_budget_overruns=0 "
            "driver_task_no_replies=3122 poll_cooldown=1 cooldown_skips=221894",
            "[smp] activity pump now_ms=120915 input=local-seat lines=6 ok=5 "
            "denied=0 ticks=22318 serial_rx_drop=0 serial_tx_drop=0 "
            "utf8_drop=0 serial_budget_overruns=0 serial_rx_backpressure=0 "
            "serial_tx_backpressure=0 serial_pressure_source=uart-output",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_BUDGET_OVERRUNS"] == 0


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
    assert record["SERIAL_CLEAN"] == "no"


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
    assert record["DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT"] == 1


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
    assert record["DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT"] == 1
    assert record["DRIVER_TASK_RING_CALL_KEEP_ACTIVE"] == 1
    assert record["DRIVER_TASK_RING_CALL_ABORT"] == 0
    assert record["DRIVER_TASK_RING_CALL_OUTSTANDING"] == 1


def test_gate_summary_closes_driver_task_timeout_after_return() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RING_CALL_TIMEOUT contract=cyw43455 endpoint=0x192d "
            "request=93 mode=nonblocking attempts=1048576 opcode=1 arg0=5 "
            "aux0=0x43595734 frame_len=28 owner=linked-runtime "
            "marker_valid=yes marker_sequence=93 marker_phase=142 "
            "marker_phase_name=cyw43-sdio-owner-wait-begin "
            "marker_aux0=0x43595734 blocker=cyw43-sdio-owner-wait-begin "
            "next_action=check-keep-active",
            "DRIVER_TASK_RING_CALL_KEEP_ACTIVE contract=cyw43455 endpoint=0x192d "
            "request=93 mode=nonblocking timeout_count=0 keep_limit=512 "
            "progress_advanced=yes opcode=1 arg0=5 aux0=0x43595734 "
            "frame_len=28 owner=linked-runtime marker_valid=yes "
            "marker_sequence=93 marker_phase=142 "
            "marker_phase_name=cyw43-sdio-owner-wait-begin "
            "marker_aux0=0x43595734 blocker=cyw43-sdio-owner-wait-begin "
            "next_action=poll-same-request",
            "DRIVER_TASK_RING_CALL_RETURN contract=cyw43455 endpoint=0x192d "
            "request=93 sequence=93 code=5 detail=21290 result=80",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["DRIVER_TASK_RING_CALL_TIMEOUT"] == 1
    assert record["DRIVER_TASK_RING_CALL_KEEP_ACTIVE"] == 1
    assert record["DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT"] == 0


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
    assert record["DRIVER_TASK_RING_CALL_UNRESOLVED_TIMEOUT"] == 1
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


@pytest.mark.parametrize(
    "exact",
    [
        "driver-runtime-sdio-dma-mmio-pre-admission-missing",
        "driver-runtime-sdio-dma-mmio-not-covered",
        "driver-runtime-sdio-owner-handle-missing",
        "driver-task-bootstrap-failed",
    ],
)
def test_gate_summary_tracks_wifi_runtime_resource_admission_prerequisite(
    exact: str,
) -> None:
    events = normalizer.parse_events(
        [
            "wifi: prerequisite name=runtime-resource-admission status=fail "
            f"contract=sdio-host fault_detail={exact} next=runtime-power-reset",
            "wifi: gate 1 name=runtime-power-reset status=blocked "
            "evidence=power=unknown reset=unknown dependency=runtime-resource-admission "
            "next=sdio-card-select",
            "wifi: evidence boundary proof=gate-frontier direct_proof_gate=0 "
            "inferred_frontier_gate=0 proof_gate=0 frontier_gate=0 "
            f"failing_gate=1 target_gate=10 failure_domain={exact}",
            "wifi: next_action=repair-sdio-runtime-resource-admission "
            f"blocker={exact} proof_gate=0 target_gate=10 source=debug-handle-unavailable",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 0
    assert gates.wifi_blocker == exact
    assert gates.wifi_exact == exact
    assert gates.wifi_phase == "runtime-resource-admission"
    assert gates.wifi_blocker_line == 1


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


def test_gate_summary_preserves_firmware_frontier_after_nettest_disabled() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-chunk op=2 flags=0x0000 "
            "target=0x00222000 payload_off=4096 payload_len=8192 "
            "total_len=609309 detail=21289 "
            "reason=cyw43-firmware-retry-exhausted result=83888128",
            "wifi: gate 6 name=firmware-upload status=fail "
            "evidence=uploaded=no fault_detail=0x5329 next=function2-ready",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43-command driver-task runtime init failed",
            "wifi: gate 9 name=tcp-nettest status=blocked "
            "dependency=not-reached-due-to-gate-6",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 5
    assert gates.wifi_blocker == "cyw43-firmware-retry-exhausted"
    assert gates.wifi_exact == "cyw43-firmware-retry-exhausted"
    assert gates.wifi_phase == "cyw43-firmware-chunk"


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


@pytest.mark.parametrize(
    ("phase", "phase_name", "blocker"),
    [
        (447, "cyw43-backplane-alp-request", "cyw43-backplane-alp-request-no-reply"),
        (448, "cyw43-backplane-alp-poll", "cyw43-backplane-alp-poll-no-reply"),
        (449, "cyw43-backplane-force-alp", "cyw43-backplane-force-alp-no-reply"),
        (
            450,
            "cyw43-backplane-force-alp-settle",
            "cyw43-backplane-force-alp-settle-no-reply",
        ),
        (
            451,
            "cyw43-backplane-pullup-clear",
            "cyw43-backplane-pullup-clear-no-reply",
        ),
        (
            452,
            "cyw43-backplane-pullup-fault-contained",
            "cyw43-backplane-pullup-fault-contained",
        ),
        (
            453,
            "cyw43-backplane-chipcommon-read",
            "cyw43-backplane-chipcommon-read-no-reply",
        ),
        (454, "cyw43-backplane-window-low", "cyw43-backplane-window-low-no-reply"),
        (455, "cyw43-backplane-window-mid", "cyw43-backplane-window-mid-no-reply"),
        (
            456,
            "cyw43-backplane-window-high",
            "cyw43-backplane-window-high-no-reply",
        ),
        (
            457,
            "cyw43-backplane-pullup-skipped",
            "cyw43-backplane-pullup-skipped-no-reply",
        ),
    ],
)
def test_gate_summary_preserves_exact_linked_backplane_progress(
    phase: int, phase_name: str, blocker: str
) -> None:
    events = normalizer.parse_events(
        [
            "wifi: cyw43 linked_runtime_progress marker_valid=yes sequence=99 "
            f"phase={phase} phase_name={phase_name} aux0=0x43595734 "
            f"gate=5 blocker={blocker} next_action=inspect-exact-backplane-action",
            "wifi: gate 5 name=backplane-window status=fail "
            f"evidence=phase={phase} phase_name={phase_name} next=firmware-upload",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_GATE"] == 4
    assert record["WIFI_BLOCKER"] == blocker
    assert record["WIFI_EXACT"] == blocker
    assert record["WIFI_PHASE"] == blocker
    assert record["WIFI_EXACT"] != "wifi-driver-task-runtime-unproved"


@pytest.mark.parametrize(
    ("phase", "phase_name", "blocker"),
    [
        (447, "cyw43-backplane-alp-request", "cyw43-backplane-alp-request-no-reply"),
        (448, "cyw43-backplane-alp-poll", "cyw43-backplane-alp-poll-no-reply"),
        (449, "cyw43-backplane-force-alp", "cyw43-backplane-force-alp-no-reply"),
        (
            450,
            "cyw43-backplane-force-alp-settle",
            "cyw43-backplane-force-alp-settle-no-reply",
        ),
        (
            451,
            "cyw43-backplane-pullup-clear",
            "cyw43-backplane-pullup-clear-no-reply",
        ),
        (
            452,
            "cyw43-backplane-pullup-fault-contained",
            "cyw43-backplane-pullup-fault-contained",
        ),
        (
            453,
            "cyw43-backplane-chipcommon-read",
            "cyw43-backplane-chipcommon-read-no-reply",
        ),
        (454, "cyw43-backplane-window-low", "cyw43-backplane-window-low-no-reply"),
        (455, "cyw43-backplane-window-mid", "cyw43-backplane-window-mid-no-reply"),
        (
            456,
            "cyw43-backplane-window-high",
            "cyw43-backplane-window-high-no-reply",
        ),
        (
            457,
            "cyw43-backplane-pullup-skipped",
            "cyw43-backplane-pullup-skipped-no-reply",
        ),
    ],
)
def test_gate_summary_derives_exact_backplane_frontier_from_raw_progress(
    phase: int, phase_name: str, blocker: str
) -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-firmware blocker=no-reply",
            "DRIVER_TASK_RING_PROGRESS contract=cyw43455 request=99 "
            "expected_aux0=0x43595734 marker_valid=yes marker_sequence=99 "
            f"marker_phase={phase} marker_phase_name={phase_name} "
            "marker_aux0=0x43595734",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_EXACT"] == blocker
    assert record["WIFI_PHASE"] == blocker
    assert record["WIFI_EXACT"] != "wifi-driver-task-runtime-unproved"


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


def test_gate_summary_tracks_cyw43_post_release_function2_ready_fault() -> None:
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
            "detail=21291 reason=cyw43-post-release-function2-ready "
            "result=0x00000600",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-firmware-release status=fault acceptance=no "
            "code=5 detail=21291 result=0x00000600 frame_len=0",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43-command driver-task runtime init failed",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-post-release-function2-ready"
    assert gates.wifi_exact == "cyw43-post-release-function2-ready"
    assert gates.wifi_phase == "cyw43-firmware-release"


def test_terminal_release_fault_outranks_later_passive_probe_diagnostics() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-release op=5 detail=21290 "
            "reason=cyw43-post-release-ht-clock result=82",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-firmware-recover "
            "status=generation-reset-ready",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-release op=5 detail=21290 "
            "reason=cyw43-post-release-ht-clock result=82",
            "wifi: gate 7 name=function2-ready status=fail "
            "evidence=f2_enabled=no f2_ready=no",
            "wifi: gate 9 name=dhcp-bound status=blocked "
            "dependency=not-reached-due-to-gate-7",
            "wifi: debug subcommand=probe-ht action=complete result=err "
            "source=linked-runtime-replay-failure",
            "ERR WIFI reason=policy detail=subcommand=probe-ht "
            "error=pi4-wifi-driver-task-runtime-required",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 7
    assert record["WIFI_BLOCKER"] == "cyw43-post-release-ht-clock"
    assert record["WIFI_EXACT"] == "cyw43-post-release-ht-clock"
    assert record["WIFI_PHASE"] == "cyw43-firmware-release"
    assert record["WIFI_BLOCKER_LINE"] == 3
    assert record["WIFI_SUBGATE"] == "none"
    assert record["WIFI_SUBGATE_NAME"] == "none"
    assert record["WIFI_SUBGATE_SOURCE"] == "none"


def test_generic_release_fault_uses_durable_phase_and_beats_load_fw_policy_error() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-release op=5 detail=21253 "
            "reason=cyw43-release result=208",
            "wifi: gate 8 name=firmware-channel status=fail evidence=exact=none",
            "ERR WIFI reason=policy detail=subcommand=load-fw "
            "error=pi4-wifi-driver-task-runtime-required",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 6
    assert record["WIFI_BLOCKER"] == "cyw43-release-armcr4-reset"
    assert record["WIFI_EXACT"] == "cyw43-release-armcr4-reset"
    assert record["WIFI_PHASE"] == "cyw43-firmware-release"
    assert record["WIFI_BLOCKER_LINE"] == 1


def test_untyped_release_fault_still_beats_later_passive_diagnostics() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-release op=5 detail=21253 "
            "reason=cyw43-release result=0",
            "ERR WIFI reason=policy detail=subcommand=load-fw "
            "error=pi4-wifi-driver-task-runtime-required",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 6
    assert record["WIFI_BLOCKER"] == "cyw43-release"
    assert record["WIFI_EXACT"] == "cyw43-release"
    assert record["WIFI_PHASE"] == "cyw43-firmware-release"


def test_gate_summary_decodes_exact_armcr4_reset_edge_at_gate_six() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-release op=5 detail=21279 "
            "reason=cyw43-backplane-armcr4-reset result=6",
            "wifi: gate 8 name=firmware-channel status=fail evidence=exact=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 6
    assert record["WIFI_BLOCKER"] == "cyw43-armcr4-clear-write"
    assert record["WIFI_EXACT"] == "cyw43-armcr4-clear-write"
    assert record["WIFI_PHASE"] == "cyw43-firmware-release"


def test_gate_summary_decodes_armcr4_prereset_edge_during_firmware_prep() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-prep op=15 detail=21279 "
            "reason=cyw43-backplane-armcr4-reset result=1",
            "wifi: gate 6 name=firmware-upload status=fail "
            "evidence=uploaded=no fault_detail=0x531f next=function2-ready",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 6
    assert record["WIFI_BLOCKER"] == "cyw43-armcr4-prereset-write"
    assert record["WIFI_EXACT"] == "cyw43-armcr4-prereset-write"
    assert record["WIFI_PHASE"] == "cyw43-firmware-prep"
    assert record["WIFI_BLOCKER_LINE"] == 1


def test_gate_summary_keeps_wifi_blackbox_fault_over_later_prompt_replay() -> None:
    events = normalizer.parse_events(
        [
            "wifi: gate 5 name=backplane-window status=pass evidence=programmed=0x00198000 next=firmware-upload",
            "wifi: gate 6 name=firmware-upload status=fail evidence=uploaded=no fault_detail=0x5103 next=function2-ready",
            "wifi: evidence cyw43 stage=cyw43-firmware-chunk op=2 flags=0x0001 target=0x00199c00 payload_len=1024 total_len=609309 detail=0x5103 reason=sdio-descriptor-transfer-failed result=0x05000100",
            "wifi: evidence sdio_cmd53 func=1 addr=0x00199c00 len=1024 "
            "cmd53_count=512 desc_blkcnt=0 host_blkcnt=1 increment=yes "
            "block_mode=byte-retry op=2 source=owner-terminal",
            "wifi: evidence sdio_status descriptor_status=descriptor-transfer-failed "
            "transfer_stage=response transfer_status=0x000100 "
            "transfer_reason=sdio-r5-response r5=0x0100 retry=byte512-fallback "
            "host=0x06 host_mode=4bit+high-speed clock=0x5007 "
            "clock_state=internal+stable+card",
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


def test_gate_summary_keeps_primary_gate_six_fault_over_preserved_recovery() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-firmware blocker=failed",
            "wifi: gate 5 name=backplane-window status=inferred "
            "evidence=programmed=n/a next=firmware-upload",
            "wifi: gate 6 name=firmware-upload status=fail "
            "evidence=uploaded=no verified=no fault_detail=0x5103 "
            "next=function2-ready",
            "wifi: gate 9 name=dhcp-bound status=blocked "
            "dependency=not-reached-due-to-gate-6",
            "wifi: cyw43 fault stage=cyw43-firmware-chunk op=2 "
            "detail=0x5103 reason=",
            "wifi: recovery fault stage=cyw43-transport-init op=1 "
            "detail=0x5313 reason=cyw43-transport-f1-block-size "
            "causal_preserved=yes",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43-command driver-task runtime init failed",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 5
    assert record["WIFI_BLOCKER"] == "cyw43-sdio-descriptor-transfer-failed"
    assert record["WIFI_EXACT"] == "cyw43-sdio-descriptor-transfer-failed"
    assert record["WIFI_PHASE"] == "cyw43-firmware-chunk"
    assert record["WIFI_BLOCKER_LINE"] == 5


def test_gate_summary_uses_explicit_gate_six_boundary_over_generic_later_gate() -> None:
    events = normalizer.parse_events(
        [
            "wifi: gate 5 name=backplane-window status=inferred "
            "evidence=programmed=n/a next=firmware-upload",
            "wifi: gate 6 name=firmware-upload status=fail "
            "evidence=uploaded=no verified=no fault_detail=0x5103 "
            "next=function2-ready",
            "wifi: gate 7 name=function2-ready status=blocked "
            "dependency=not-reached-due-to-gate-6 next=firmware-channel",
            "wifi: evidence sdio_status "
            "descriptor_status=sdio-descriptor-transfer-failed "
            "transfer_stage=command transfer_status=0x0c8000 "
            "transfer_reason=sdhci-command retry=none",
            "wifi: evidence boundary console_client=root-net-console "
            "hal=admission-descriptor-diagnostics-only "
            "linked_runtime_owner=cyw43+sdio "
            "failure_domain=sdio-descriptor-transfer-failed "
            "direct_proof_gate=0 proof_gate=5 frontier_gate=5 "
            "failing_gate=6 target_gate=10",
            "wifi: cyw43 fault stage=cyw43-transport-init op=1 "
            "detail=0x5103 reason=",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 5
    assert record["WIFI_BLOCKER"] == "cyw43-sdio-descriptor-transfer-failed"
    assert record["WIFI_EXACT"] == "cyw43-sdio-descriptor-transfer-failed"
    assert record["WIFI_PHASE"] == "cyw43-transport-init"


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


def test_wifi_diagnostic_wrapper_does_not_replace_runtime_power_exact() -> None:
    events = normalizer.parse_events(
        [
            "wifi: gate 1 name=runtime-power-reset status=fail "
            "evidence=power=unknown reset=unknown pwrseq_status=unknown "
            "pwrseq_phase=none source=hal-runtime-required next=sdio-card-select",
            "wifi: next_action=verify-linked-runtime-power-reset-resources "
            "blocker=wifi-power-reset proof_gate=0 target_gate=10 "
            "source=hal-runtime-required",
            "ERR WIFI reason=policy detail=subcommand=probe-ht "
            "error=pi4-wifi-driver-task-runtime-required "
            "source=linked-runtime-replay-failure",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_BLOCKER"] == "runtime-power-reset"
    assert record["WIFI_EXACT"] == "runtime-power-reset"


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


def test_gate_summary_clears_current_resource_blocker_after_ready() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-keyboard-enumeration-retry "
            "status=not-enumerated detail=0x0200 result=0 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-owner-state "
            "status=blocked-first-report detail=0x0500 result=0 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-owner-state "
            "status=ready detail=0x0501 result=1 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert (
        record["DRIVER_TASK_RESOURCE_BLOCKER"]
        == "usb-keyboard:usb-keyboard-enumeration-retry:not-enumerated"
    )
    assert record["DRIVER_TASK_RESOURCE_CURRENT_BLOCKER"] == "none"


def test_gate_summary_clears_current_resource_blocker_after_owner_state() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
            "hot_path=cyw43-wifi stage=cyw43-host-eapol "
            "status=secure detail=0 result=0 frame_len=0",
            "DRIVER_TASK_OWNER_STATE contract=cyw43455 hot_path=cyw43-wifi "
            "owner_state=driver-owned hardware_owner=linked-runtime "
            "descriptor=present root_pointer=no proof_effect=owner-state-proven",
            "DRIVER_TASK_DMA_PROOF contract=cyw43455 hot_path=cyw43-wifi "
            "status=ready profile=bounded-no-iommu descriptor=present "
            "root_pointer=no owner=linked-runtime mmio_pages=0 dma_pages=0 "
            "shared_pages=64 bus_address_policy=zero-dma "
            "cache_policy=uncached-plus-root-maintenance cache_clean_ops=1 "
            "cache_clean_bytes=64 cache_invalidate_ops=1 cache_invalidate_bytes=64 "
            "proof_effect=runtime-dma-proof-ready",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert (
        record["DRIVER_TASK_RESOURCE_BLOCKER"]
        == "cyw43-wifi:cyw43-host-eapol:secure"
    )
    assert record["DRIVER_TASK_RESOURCE_CURRENT_BLOCKER"] == "none"


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


def test_gate_summary_labels_idle_hid_report_as_awaiting_physical_key() -> None:
    events = normalizer.parse_events(
        [
            "usb: runtime_queue queue_valid=yes detail=0x0501 result=0x01000480 "
            "queued_reports=1 doorbell_pending=no preserved_events=0 "
            "transfer_events=1 report_status=idle-report",
            "usb: acceptance xhci=yes hid_keyboard=yes first_report=yes "
            "first_byte=no usable=no prompt_polling=yes "
            "input_observation=idle-report-no-key-byte death_proof=no",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "first_byte_source=none proof_gate=9 target_gate=10 "
            "next=press-key-for-first-byte blocker=awaiting-physical-key "
            "detail=0x0501 result=0x01000480 progress_gate=7 "
            "progress_phase=427 progress_phase_name=usb-hub-port-status-payload-read",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["USB_GATE"] == 9
    assert record["USB_BLOCKER"] == "awaiting-physical-key"


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


def test_gate_summary_preserves_sdio_wl_on_progress_blocker() -> None:
    events = normalizer.parse_events(
        [
            "wifi: sdio linked_runtime_progress marker_valid=yes sequence=2 "
            "phase=435 phase_name=sdio-wifi-pwrseq-low-done "
            "aux0=0x454e474e gate=1 "
            "blocker=sdio-wl-on-low-host-reset-no-reply "
            "next_action=inspect-sdhci-all-reset-after-wl-on-low",
            "wifi: gate 1 name=hal-power-reset status=inferred "
            "evidence=power=unknown reset=unknown pwrseq_status=no-reply "
            "pwrseq_phase=sdio-wifi-pwrseq-low-done source=hal-runtime-required "
            "next=sdio-card-select",
            "wifi: gate 2 name=sdio-card-select status=fail "
            "evidence=stage=engine-init status=no-reply phase=435 "
            "phase_name=sdio-wifi-pwrseq-low-done marker_valid=yes "
            "source=linked-runtime next=cccr-fbr-ready",
            "wifi: next_action=inspect-sdhci-all-reset-after-wl-on-low "
            "blocker=sdio-wl-on-low-host-reset-no-reply proof_gate=0 "
            "target_gate=10 source=hal-runtime-required",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_GATE"] == 1
    assert record["WIFI_BLOCKER"] == "sdio-wl-on-low-host-reset-no-reply"
    assert record["WIFI_EXACT"] == "sdio-wl-on-low-host-reset-no-reply"
    assert record["WIFI_PHASE"] == "sdio-wl-on-low-host-reset-no-reply"


def test_gate_summary_reports_wifi_pwrseq_engine_init_failure_at_gate_one() -> None:
    for blocker in (
        "wifi-pwrseq-failed",
        "wifi-pwrseq-get-config-failed",
        "wifi-pwrseq-set-config-failed",
        "wifi-pwrseq-assert-low-failed",
        "wifi-pwrseq-release-high-failed",
    ):
        events = normalizer.parse_events(
            [
                "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host "
                "selected=wifi-owner-link attempted=yes stage=engine-init "
                f"blocker={blocker}",
            ]
        )

        record = normalizer.summarize_gates(events).to_record()
        assert record["WIFI_GATE"] == 1
        assert record["WIFI_BLOCKER"] == blocker
        assert record["WIFI_EXACT"] == blocker
        assert record["WIFI_PHASE"] == "engine-init"


def test_gate_summary_keeps_pwrseq_exact_after_generic_passive_diagnostics() -> None:
    events = normalizer.parse_events(
        [
            "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host "
            "selected=wifi-owner-link attempted=yes stage=engine-init "
            "blocker=wifi-pwrseq-set-config-failed",
            "[net-console] deferred failed detail=sdio-host-linked-runtime "
            "driver-task runtime init failed",
            "wifi: driver-task replay failure detail=net-disabled "
            "cause=sdio-host-linked-runtime driver-task runtime init failed",
            "wifi: gate 1 name=runtime-power-reset status=inferred "
            "evidence=power=unknown reset=unknown "
            "pwrseq_status=wifi-pwrseq-set-config-failed "
            "pwrseq_phase=runtime-poll-ready source=hal-runtime-required "
            "next=sdio-card-select",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    assert record["WIFI_GATE"] == 1
    assert record["WIFI_BLOCKER"] == "wifi-pwrseq-set-config-failed"
    assert record["WIFI_EXACT"] == "wifi-pwrseq-set-config-failed"
    assert record["WIFI_PHASE"] == "engine-init"
    assert record["WIFI_BLOCKER_LINE"] == 1


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


def test_gate_summary_classifies_markerless_cyw43_engine_init_replay_exhaustion() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes policy=wifi "
            "attempted=yes stage=engine-init blocker=begin",
            "DRIVER_TASK_RING_CALL_BEGIN contract=cyw43455 endpoint=0x1b70 "
            "request=2 opcode=1 flags=0x2000 arg0=5 arg1=8 aux0=0x454e474e "
            "aux1=0 frame_len=0",
            "DRIVER_TASK_RING_CALL_RETURN contract=cyw43455 endpoint=0x1b70 "
            "request=2 sequence=2 code=5 detail=1 result=0",
            "DRIVER_TASK_RING_PROGRESS contract=cyw43455 request=2 "
            "expected_aux0=0x454e474e marker_valid=yes marker_sequence=0 "
            "marker_phase=202 marker_phase_name=runtime-poll-ready marker_aux0=0x00000004",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes policy=wifi "
            "attempted=yes stage=engine-init blocker=stale-admission-retry",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=net-engine-init status=stale-admission-retry acceptance=no "
            "code=5 detail=1 result=0",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes policy=wifi "
            "attempted=yes stage=engine-init-replay blocker=begin",
            "DRIVER_TASK_RING_CALL_RETURN contract=cyw43455 endpoint=0x1b70 "
            "request=3 sequence=3 code=5 detail=1 result=0",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes policy=wifi "
            "attempted=yes stage=engine-init-replay blocker=stale-admission-exhausted",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=net-engine-init-replay status=stale-admission-exhausted "
            "acceptance=no code=5 detail=1 result=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 1
    assert record["WIFI_BLOCKER"] == "cyw43-engine-init-stale-admission-exhausted"
    assert record["WIFI_EXACT"] == "cyw43-engine-init-stale-admission-exhausted"
    assert record["WIFI_PHASE"] == "net-engine-init-replay"
    assert (
        record["NET_DRIVER_TASK_REPLAY_BLOCKER"]
        == "cyw43-wifi:engine-init-replay:stale-admission-exhausted"
    )
    assert (
        record["DRIVER_TASK_RESOURCE_CURRENT_BLOCKER"]
        == "cyw43-wifi:net-engine-init-replay:stale-admission-exhausted"
    )


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


def test_gate_summary_marks_hard_guard_as_terminal_boot_failure() -> None:
    events = normalizer.parse_events(
        [
            "[cohesix] Network: Wi-Fi",
            "[HARD_GUARD] tag=bootstrap_ep "
            "v=EPIdentifyInvalid{ident=4}",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.boot_halted is True
    assert gates.to_record()["BOOT_HALTED"] == "yes"
    assert gates.to_record()["PANIC_SEEN"] == "no"
    assert (
        gates.to_record()["BOOT_HALT_REASON"]
        == "hard-guard-bootstrap-ep-epidentifyinvalid"
    )
    assert gates.to_record()["WIFI_GATE"] == 0
    assert gates.to_record()["WIFI_BLOCKER"] == "boot-halted-before-wifi"
    assert gates.to_record()["WIFI_PHASE"] == "root-bootstrap"
    assert normalizer.boot_evidence_blockers(gates.to_record())[0] == "boot-halted"


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


def test_gate_summary_accepts_canonical_linked_runtime_sdio_irq_topology() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_IRQ_TOPOLOGY contract=sdio-host irq=158 badge=159 "
            "handler_slot=4 notification_slot=3 trigger=level status=bound "
            "proof_effect=notification-dpc-ready",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert events[0].domain == "driver"
    assert gates.sdio_irq158_seen
    assert gates.sdio_irq158_bound
    assert gates.sdio_irq158_line == 1
    assert gates.to_record()["SDIO_IRQ158_SEEN"] == "yes"
    assert gates.to_record()["SDIO_IRQ158_BOUND"] == "yes"
    assert gates.to_record()["SDIO_IRQ158_INBAND_PROOF"] == "no"


def test_gate_summary_rejects_nonbound_linked_runtime_sdio_irq_topology() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_IRQ_TOPOLOGY contract=sdio-host irq=158 badge=159 "
            "handler_slot=4 notification_slot=3 trigger=level status=failed",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.sdio_irq158_seen
    assert not gates.sdio_irq158_bound
    assert gates.to_record()["SDIO_IRQ158_BOUND"] == "no"


def test_gate_summary_reports_arch_counter_timer_backend() -> None:
    events = normalizer.parse_events(
        [
            "[timers] init: timer_freq_hz=54000000 Hz",
            "[timers] summary backend=arch-counter counter=virtual "
            "timer_freq_hz=54000000 period_cycles=270000",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["TIMER_BACKEND"] == "arch-counter"
    assert record["TIMER_CLOCK_HZ"] == 54_000_000
    assert record["TIMER_EL0_COUNTER"] == "vct"
    assert record["DUMMY_TIMER_SEEN"] == "no"


def test_gate_summary_reports_dummy_timer_backend() -> None:
    events = normalizer.parse_events(
        [
            "[timers] init: using dummy software counter; snapshots will not "
            "read CNT registers",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["TIMER_BACKEND"] == "dummy"
    assert record["TIMER_EL0_COUNTER"] == "none"
    assert record["DUMMY_TIMER_SEEN"] == "yes"


def test_gate_summary_derives_sdio_irq158_bound_from_hal_init_when_irq_breadcrumbs_suppressed() -> None:
    events = normalizer.parse_events(
        [
            "[pi4-wifi] hal init: clock=41666666Hz bus_width=4 ioex=0x06 "
            "iordy=0x06 irq_bound=true",
            "[local-seat] pi4 keyboard runtime proof result=online gate=10 "
            "source=linked-runtime-hid",
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
            "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
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
        "cyw43-association-not-associated": "cyw43-association-not-associated",
        "association-not-associated": "cyw43-association-not-associated",
        "cyw43-association-event-missing": "cyw43-association-event-missing",
        "dhcp-pending": "dhcp-pending",
        "dhcp-failed": "dhcp-failed",
        "0x5330": "cyw43-post-release-dpc-activate",
        "0x5331": "cyw43-probe-cardctrl-read",
        "21298": "cyw43-probe-cardctrl-write",
        "0x5333": "cyw43-probe-pmucontrol-read",
        "21300": "cyw43-probe-pmucontrol-write",
        "0x5335": "cyw43-probe-function2-disable-read",
        "21302": "cyw43-probe-function2-disable-write",
        "0x5337": "cyw43-probe-sdonly-clock",
        "0x5338": "cyw43-release-intstatus-clear",
        "0x5339": "cyw43-post-release-ienx-admission",
        "21305": "cyw43-post-release-ienx-admission",
        "0x5324": "cyw43-transport-card-cmd0",
        "21285": "cyw43-transport-card-cmd5-ocr",
        "0x5326": "cyw43-transport-card-cmd5-ready",
        "21287": "cyw43-transport-card-cmd3-rca",
        "0x5328": "cyw43-transport-card-cmd7-select",
        "not-ready:ipc-buffer": "net-not-ready-ipc-buffer",
        "policy-disabled": "nettest-policy-disabled",
        "selftest-disabled": "nettest-selftest-disabled",
        "unsupported": "nettest-unsupported",
        "nettest timeout": "nettest-failed",
        "unknown-wifi-edge": "unknown-wifi-edge",
    }

    for raw, expected in cases.items():
        assert normalizer.normalize_wifi_blocker(raw) == expected

    assert normalizer.normalize_wifi_exact("0x5337") == "cyw43-probe-sdonly-clock"
    assert normalizer.normalize_wifi_exact("21303") == "cyw43-probe-sdonly-clock"


def test_post_release_dpc_activation_fault_is_a_gate_seven_hardware_frontier() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-release op=5 detail=21296 "
            "reason=cyw43-post-release-dpc-activate result=327681",
            "wifi: gate 9 name=dhcp-bound status=blocked "
            "dependency=not-reached-due-to-gate-7",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 7
    assert record["WIFI_BLOCKER"] == "cyw43-post-release-dpc-activate"
    assert record["WIFI_EXACT"] == "cyw43-post-release-dpc-activate"
    assert record["WIFI_PHASE"] == "cyw43-firmware-release"


def test_probe_attach_fault_is_a_gate_five_hardware_frontier() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-prep op=1 detail=21301 "
            "reason=cyw43-probe-function2-disable-read result=2",
            "wifi: gate 6 name=firmware-upload status=blocked "
            "dependency=not-reached-due-to-gate-5",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 5
    assert record["WIFI_BLOCKER"] == "cyw43-probe-function2-disable-read"
    assert record["WIFI_EXACT"] == "cyw43-probe-function2-disable-read"
    assert record["WIFI_PHASE"] == "cyw43-firmware-prep"


def test_failed_generation_retry_preserves_causal_release_fault() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-firmware blocker=begin",
            "CYW43_DRIVER_TASK_COMMAND_FAULT stage=cyw43-firmware-release "
            "op=5 detail=21290 reason=cyw43-post-release-ht-clock result=82",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-firmware-recover status=generation-reset-ready",
            "CYW43_DRIVER_TASK_COMMAND_FAULT stage=cyw43-transport-init "
            "op=1 detail=21285 reason=cyw43-transport-card-cmd5-ocr "
            "result=83951616",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-post-release-ht-clock"


def test_failed_generation_retry_preserves_causal_armcr4_flush_fault() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_COMMAND_FAULT stage=cyw43-firmware-release "
            "op=5 detail=21279 reason=cyw43-armcr4-prereset-flush result=2",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-firmware-recover status=generation-reset-ready",
            "CYW43_DRIVER_TASK_COMMAND_FAULT stage=cyw43-transport-init "
            "op=1 detail=21285 reason=cyw43-transport-card-cmd5-ocr "
            "result=84082688",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 6
    assert gates.wifi_blocker == "cyw43-armcr4-prereset-flush"
    assert gates.wifi_exact == "cyw43-armcr4-prereset-flush"
    assert gates.wifi_phase == "cyw43-firmware-release"


def test_retained_cmd5_ocr_fault_outranks_generic_firmware_replay_failure() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-transport-init op=1 detail=21285 "
            "reason=cyw43-transport-card-cmd5-ocr result=33652736",
            "CYW43_SDIO_OWNER_FAULT contract=cyw43455 "
            "stage=cyw43-transport-init op=1 cmd=5 arg=0x00000000 "
            "detail=0x5325 reason=cyw43-transport-card-cmd5-ocr "
            "xfer_stage=command xfer_status=0x018000",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-firmware blocker=failed",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 2
    assert gates.wifi_blocker == "cyw43-transport-card-cmd5-ocr"
    assert gates.wifi_exact == "cyw43-transport-card-cmd5-ocr"
    assert gates.wifi_phase == "cyw43-transport-init"
    assert gates.wifi_blocker_line == 1


def test_transport_ready_clears_retained_cmd5_ocr_fault() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-transport-init op=1 detail=21285 "
            "reason=cyw43-transport-card-cmd5-ocr result=33652736",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-transport-init status=ready code=1 detail=21512 result=1",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-firmware-release op=5 detail=21290 "
            "reason=cyw43-post-release-ht-clock result=82",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-post-release-ht-clock"


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


def test_gate_summary_tracks_usb_post_prompt_attach_retry_exhaustion() -> None:
    events = normalizer.parse_events(
        [
            "usb: boot_blocker stage=post-prompt-local-seat-attach "
            "status=retry-exhausted attempts=32 keyboard=backend-unavailable "
            "command_ready=no first_report=no first_byte=no "
            "detail=0x0216 result=0x0f000001 next=usb-probe-kbd",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate >= 8
    assert gates.usb_blocker == "post-prompt-attach-retry-exhausted"


def test_gate_summary_ignores_nonterminal_usb_post_prompt_attach_status() -> None:
    events = normalizer.parse_events(
        [
            "usb: boot_blocker stage=post-prompt-local-seat-attach "
            "status=pending attempts=7 keyboard=backend-unavailable "
            "command_ready=no first_report=no first_byte=no "
            "detail=0x0216 result=0x0f000001 next=usb-probe-kbd",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_blocker != "post-prompt-attach-retry-exhausted"


def test_gate_summary_rejects_unsourced_usb_keyboard_report_and_first_byte() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb hid keyboard ready slot=1 iface=0 ep=0x81 "
            "source=direct layout=boot subclass=0x01 protocol=0x01",
            "[local-seat] usb hid first report shift=0 keys=04,00,00,00,00,00",
            "[local-seat] runtime keyboard first-byte read=1",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 8
    assert gates.usb_blocker == "hid-first-report"


def test_gate_summary_tracks_linked_usb_keyboard_report_and_first_byte() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb hid keyboard ready slot=1 iface=0 ep=0x81 "
            "source=linked-runtime layout=boot subclass=0x01 protocol=0x01",
            "[local-seat] usb hid first report source=linked-runtime-hid "
            "shift=0 keys=04,00,00,00,00,00",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid read=1",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 9
    assert gates.usb_blocker == "command-input-ready"


def test_gate_summary_tracks_prompt_prefixed_linked_usb_first_byte() -> None:
    events = normalizer.parse_events(
        [
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "first_byte_source=none proof_gate=9 target_gate=10 "
            "blocker=keyboard-first-byte",
            "usb: gate 10 name=first-console-byte status=fail "
            "evidence=first_byte=no first_byte_source=none parser_ingress=no "
            "backend_bytes=0 accepted=0 echoed=0 next=acceptance-complete",
            "cohesix> [local-seat] usb hid first report contract=usb-local-seat "
            "source=linked-runtime-hid tag=usb-hid-report-event len=1 accepted=1 "
            "detail=0x0000 result=0x00000001 transfer_event=yes",
            "cohesix> [local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "cohesix> [local-seat] runtime keyboard first-byte "
            "source=linked-runtime-hid read=1 ascii=0x54 detail=0x0000 "
            "result=0x00000001",
            "usb: next_action=inspect-hid-report-to-console-byte-path "
            "blocker=keyboard-first-byte proof_gate=9 target_gate=10",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 10
    assert gates.usb_blocker == "none"


def test_gate_summary_keeps_linked_gate10_over_later_stale_hub_blocker() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb hid first report source=linked-runtime-hid "
            "shift=0 keys=04,00,00,00,00,00",
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x61",
            "usb: linked_runtime_progress marker_valid=yes sequence=8 "
            "phase_name=usb-hub-port-status-disconnected aux0=0x55534245 "
            "gate=7 blocker=hub-port-disconnected "
            "next_action=inspect-hub-port-status",
            "usb: next_action=inspect-usb-keyboard-enumeration "
            "blocker=usb-keyboard-enumeration-no-reply proof_gate=4",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-keyboard-enumeration "
            "status=progress detail=0x0501 result=0 frame_len=0",
            "DRIVER_TASK_RING_CALL_TIMEOUT contract=usb-local-seat "
            "endpoint=0x01 request=2 mode=nonblocking attempts=1 "
            "opcode=1 arg0=2 aux0=0 frame_len=0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 10
    assert gates.usb_blocker == "none"


def test_gate_summary_replaces_stale_usb_first_report_frontier_after_ready() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-keyboard-first-report "
            "status=blocked-first-report detail=0x0500 result=0 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-keyboard-first-report "
            "status=ready detail=0x0501 result=1 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-owner-state "
            "status=ready detail=0x0501 result=1 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_BLOCKER"] == "none"
    assert record["USB_DRIVER_TASK_FRONTIER"] == "usb-owner-state-ready"
    assert record["DRIVER_TASK_RESOURCE_CURRENT_BLOCKER"] == "none"


def test_gate_summary_keeps_usb_owner_state_separate_from_first_byte() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-engine-init "
            "status=ready detail=0x0201 result=1 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-owner-state "
            "status=ready detail=0x0201 result=1 frame_len=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_DRIVER_TASK_FRONTIER"] == "usb-owner-state-ready"
    assert record["DRIVER_TASK_RESOURCE_CURRENT_BLOCKER"] == "none"
    assert record["USB_GATE"] < 10
    assert record["USB_BLOCKER"] != "none"


def test_gate_summary_uses_resource_init_first_report_despite_stale_progress_marker() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-keyboard-enumeration "
            "status=ready detail=0x0202 result=0x9f000301 frame_len=24 "
            "progress_marker_valid=yes progress_phase_name=usb-hub-port-status-disconnected",
            "DRIVER_TASK_RING_CALL_TIMEOUT contract=usb-local-seat endpoint=0x07aa "
            "request=8 mode=nonblocking attempts=16384 opcode=1 arg0=2 "
            "aux0=0x55534245 frame_len=0 owner=linked-runtime marker_valid=yes "
            "marker_sequence=8 marker_phase=428 "
            "marker_phase_name=usb-hub-port-status-disconnected "
            "marker_aux0=0x55534245 blocker=usb-keyboard-enumeration-no-reply",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-keyboard-first-report "
            "status=ready detail=0x0501 result=0x01000220 frame_len=0 "
            "progress_marker_valid=yes progress_phase_name=usb-hub-port-status-disconnected",
            "DRIVER_TASK_RESOURCE_INIT contract=usb-local-seat "
            "hot_path=usb-keyboard stage=usb-owner-state "
            "status=ready detail=0x0501 result=0x01000220 frame_len=0 "
            "progress_marker_valid=yes progress_phase_name=usb-hub-port-status-disconnected",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_BLOCKER"] == "none"
    assert record["USB_DRIVER_TASK_FRONTIER"] == "usb-owner-state-ready"


def test_gate_summary_treats_usb_keyboard_runtime_online_as_ready() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] pi4 keyboard runtime proof result=online gate=10 "
            "source=linked-runtime-hid",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 10
    assert gates.usb_blocker == "none"


def test_gate_summary_tracks_usb_runtime_gate_contract() -> None:
    events = normalizer.parse_events(
        [
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "proof_gate=8 target_gate=10 next=hid-first-report "
            "blocker=hid-first-report",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "proof_gate=9 target_gate=10 next=command-input-ready "
            "blocker=command-input-ready",
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "first_byte_source=linked-runtime-hid proof_gate=10 target_gate=10 "
            "next=none blocker=none",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 10
    assert gates.usb_blocker == "none"


def test_gate_summary_accepts_exact_healthy_wifi_dpc_proof() -> None:
    events = normalizer.parse_events(healthy_wifi_dpc_triplet())

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DPC_PROOF"] == "yes"
    assert record["WIFI_DPC_REASON"] == "none"
    assert record["WIFI_DPC_GENERATION"] == 9
    assert record["WIFI_DPC_CAPTURES"] == 6
    assert record["WIFI_DPC_PUBLISHED"] == 6
    assert record["WIFI_DPC_CONSUMED"] == 6
    assert record["WIFI_DPC_REARMS"] == 6
    assert record["WIFI_DPC_OWNER_ACTIVE"] == "yes"
    assert record["WIFI_DPC_POISONED"] == "no"
    assert record["WIFI_DPC_RING_POISONED"] == "no"
    assert record["WIFI_DPC_CLIENT_SAMPLE_STALE"] == "no"
    assert record["WIFI_DPC_TRUTH_AUTHORITY"] == "live-ring"
    assert record["WIFI_DPC_MASKED"] == "no"


def test_gate_summary_rejects_wifi_dpc_accounting_without_truth_triplet() -> None:
    record = normalizer.summarize_gates(
        normalizer.parse_events([healthy_wifi_dpc_triplet()[0]])
    ).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "truth-sequence-mismatch"
    assert record["WIFI_DPC_GENERATION"] == 9
    assert record["WIFI_DPC_CAPTURES"] == 6
    assert record["WIFI_DPC_TRUTH_LINE"] == 0


def test_gate_summary_rejects_malformed_wifi_dpc_scope() -> None:
    triplet = healthy_wifi_dpc_triplet()
    triplet[1] = (
        "CYW43_SDIO_DPC_SCOPE captures=event-attempts "
        "published=ring-events poisoned=aggregate-client-or-ring "
        "source=card-int-or-source-probe"
    )

    record = normalizer.summarize_gates(
        normalizer.parse_events(triplet)
    ).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "truth-sequence-mismatch"
    assert record["WIFI_DPC_GENERATION"] == 9
    assert record["WIFI_DPC_TRUTH_LINE"] == 0


def test_gate_summary_rejects_latest_malformed_wifi_dpc_accounting() -> None:
    """A malformed latest accounting row revokes an older healthy triplet."""

    lines = [
        *healthy_wifi_dpc_triplet(),
        "CYW43_SDIO_DPC generation=9 captures=7 published=7",
    ]

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "malformed-line"
    assert record["WIFI_DPC_LINE"] == len(lines)


def test_gate_summary_rejects_latest_orphan_malformed_wifi_dpc_scope() -> None:
    """A malformed latest scope row revokes an older healthy triplet."""

    lines = [
        *healthy_wifi_dpc_triplet(),
        "CYW43_SDIO_DPC_SCOPE captures=event-attempts truncated=yes",
    ]

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "truth-sequence-mismatch"


@pytest.mark.parametrize(
    ("reserved", "reason"),
    [
        ("CYW43_SDIO_DPC", "malformed-line"),
        ("CYW43_SDIO_DPC_SCOPE", "truth-sequence-mismatch"),
        ("CYW43_SDIO_DPC_TRUTH", "truth-sequence-mismatch"),
    ],
)
def test_bare_reserved_wifi_dpc_row_revokes_older_triplet(
    reserved: str,
    reason: str,
) -> None:
    """A token-only capture fragment is a malformed latest DPC record."""

    record = normalizer.summarize_gates(
        normalizer.parse_events([*healthy_wifi_dpc_triplet(), reserved])
    ).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == reason


def test_later_healthy_wifi_dpc_triplet_supersedes_transient_stale_sample() -> None:
    """The prescribed bounded rerun can replace a stale quiescence sample."""

    stale = healthy_wifi_dpc_triplet()
    stale[0] = stale[0].replace("poisoned=no", "poisoned=yes")
    stale[2] = (
        stale[2]
        .replace("client_sample_stale=no", "client_sample_stale=yes")
        .replace("sample_consumer=6", "sample_consumer=5")
        .replace("sample_reason=current", "sample_reason=ring-consumer-mismatch")
        .replace("action=none", "action=rerun-proof")
    )
    stale_record = normalizer.summarize_gates(
        normalizer.parse_events(stale)
    ).to_record()
    rerun_record = normalizer.summarize_gates(
        normalizer.parse_events([*stale, *healthy_wifi_dpc_triplet()])
    ).to_record()

    assert stale_record["WIFI_DPC_PROOF"] == "no"
    assert stale_record["WIFI_DPC_REASON"] == "client-sample-stale"
    assert rerun_record["WIFI_DPC_PROOF"] == "yes"
    assert rerun_record["WIFI_DPC_REASON"] == "none"


@pytest.mark.parametrize(
    "transient",
    [
        "CYW43_SDIO_DPC generation=9 captures=7 published=7",
        "CYW43_SDIO_DPC_SCOPE",
        "CYW43_SDIO_DPC_TRUTH",
    ],
)
def test_later_healthy_wifi_dpc_triplet_supersedes_transient_structure_failure(
    transient: str,
) -> None:
    """A complete later triplet clears earlier incomplete sample structure."""

    record = normalizer.summarize_gates(
        normalizer.parse_events([transient, *healthy_wifi_dpc_triplet()])
    ).to_record()

    assert record["WIFI_DPC_PROOF"] == "yes"
    assert record["WIFI_DPC_REASON"] == "none"


def test_wifi_dpc_counter_fault_remains_sticky_within_generation() -> None:
    """A later sample cannot erase a cumulative same-generation HW fault."""

    faulty = healthy_wifi_dpc_triplet()
    faulty[0] = faulty[0].replace("overruns=0", "overruns=1")
    record = normalizer.summarize_gates(
        normalizer.parse_events([*faulty, *healthy_wifi_dpc_triplet()])
    ).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "overrun"


def test_gate_summary_rejects_wifi_dpc_u32_overflow() -> None:
    """Every live-ring accounting field is bounded by its u32 ABI type."""

    accounting, scope, truth = healthy_wifi_dpc_triplet()
    accounting = accounting.replace("captures=6", "captures=4294967296")

    record = normalizer.summarize_gates(
        normalizer.parse_events([accounting, scope, truth])
    ).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "numeric-field-invalid"


@pytest.mark.parametrize(
    "truth",
    [
        (
            "CYW43_SDIO_DPC_TRUTH generation=9 owner_active=yes "
            "ring_poisoned=no client_sample_stale=no ring_consumer=6 "
            "sample_consumer=6 sample_reason=current authority=live-ring "
            "action=none extra=yes"
        ),
        (
            "CYW43_SDIO_DPC_TRUTH owner_active=yes generation=9 "
            "ring_poisoned=no client_sample_stale=no ring_consumer=6 "
            "sample_consumer=6 sample_reason=current authority=live-ring "
            "action=none"
        ),
        (
            "CYW43_SDIO_DPC_TRUTH generation=0x9 owner_active=yes "
            "ring_poisoned=no client_sample_stale=no ring_consumer=6 "
            "sample_consumer=6 sample_reason=current authority=live-ring "
            "action=none"
        ),
    ],
)
def test_gate_summary_requires_exact_wifi_dpc_truth_grammar(truth: str) -> None:
    accounting, scope, _ = healthy_wifi_dpc_triplet()

    record = normalizer.summarize_gates(
        normalizer.parse_events([accounting, scope, truth])
    ).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "truth-sequence-mismatch"
    assert record["WIFI_DPC_GENERATION"] == 9
    assert record["WIFI_DPC_TRUTH_LINE"] == 0


def test_gate_summary_requires_authoritative_unmasked_wifi_dpc_state() -> None:
    accounting, scope, truth = healthy_wifi_dpc_triplet()
    accounting = accounting.removesuffix(" masked=no")

    record = normalizer.summarize_gates(
        normalizer.parse_events([accounting, scope, truth])
    ).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "masked-unproven"
    assert record["WIFI_DPC_MASKED"] == "unknown"


def test_gate_summary_rejects_zero_wifi_dpc_generation() -> None:
    record = normalizer.summarize_gates(
        normalizer.parse_events(healthy_wifi_dpc_triplet(generation=0))
    ).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "generation-zero"
    assert record["WIFI_DPC_GENERATION"] == 0


def test_gate_summary_rejects_stale_client_sample_with_clean_live_ring() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_SDIO_DPC generation=9 captures=6 published=6 consumed=6 "
            "rearms=5 overruns=0 epoch_errors=0 sequence_errors=0 "
            "ack_failures=0 owner_active=yes poisoned=yes masked=no",
            normalizer.CYW43_SDIO_DPC_SCOPE_LINE,
            "CYW43_SDIO_DPC_TRUTH generation=9 owner_active=yes "
            "ring_poisoned=no client_sample_stale=yes ring_consumer=6 "
            "sample_consumer=5 sample_reason=ring-consumer-mismatch "
            "authority=live-ring action=rerun-proof",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "client-sample-stale"
    assert record["WIFI_DPC_POISONED"] == "yes"
    assert record["WIFI_DPC_RING_POISONED"] == "no"
    assert record["WIFI_DPC_CLIENT_SAMPLE_STALE"] == "yes"
    assert record["WIFI_DPC_TRUTH_AUTHORITY"] == "live-ring"
    assert record["WIFI_DPC_TRUTH_LINE"] == 3
    assert record["WIFI_DPC_REARMS"] == 5


@pytest.mark.parametrize(
    ("truth", "reason"),
    [
        (
            "generation=10 owner_active=yes ring_poisoned=no "
            "client_sample_stale=yes ring_consumer=6 sample_consumer=5 "
            "sample_reason=ring-consumer-mismatch authority=live-ring "
            "action=rerun-proof",
            "truth-generation-mismatch",
        ),
        (
            "generation=9 owner_active=no ring_poisoned=no "
            "client_sample_stale=yes ring_consumer=6 sample_consumer=5 "
            "sample_reason=ring-consumer-mismatch authority=live-ring "
            "action=rerun-proof",
            "truth-owner-mismatch",
        ),
        (
            "generation=9 owner_active=yes ring_poisoned=no "
            "client_sample_stale=yes ring_consumer=5 sample_consumer=5 "
            "sample_reason=ring-consumer-mismatch authority=live-ring "
            "action=rerun-proof",
            "truth-consumer-mismatch",
        ),
        (
            "generation=9 owner_active=yes ring_poisoned=no "
            "client_sample_stale=yes ring_consumer=6 sample_consumer=5 "
            "sample_reason=ring-consumer-mismatch authority=cached-client "
            "action=rerun-proof",
            "truth-authority-invalid",
        ),
        (
            "generation=9 owner_active=yes ring_poisoned=no "
            "client_sample_stale=yes ring_consumer=6 sample_consumer=5 "
            "sample_reason=current authority=live-ring action=rerun-proof",
            "truth-reason-mismatch",
        ),
        (
            "generation=9 owner_active=yes ring_poisoned=no "
            "client_sample_stale=yes ring_consumer=6 sample_consumer=5 "
            "sample_reason=ring-consumer-mismatch authority=live-ring "
            "action=none",
            "truth-action-mismatch",
        ),
        (
            "generation=9 owner_active=yes ring_poisoned=no "
            "client_sample_stale=no ring_consumer=6 sample_consumer=5 "
            "sample_reason=current authority=live-ring action=none",
            "truth-stale-state-mismatch",
        ),
    ],
)
def test_gate_summary_rejects_mismatched_wifi_dpc_truth(
    truth: str, reason: str
) -> None:
    events = normalizer.parse_events(
        [
            "CYW43_SDIO_DPC generation=9 captures=6 published=6 consumed=6 "
            "rearms=5 overruns=0 epoch_errors=0 sequence_errors=0 "
            "ack_failures=0 owner_active=yes poisoned=yes masked=no",
            normalizer.CYW43_SDIO_DPC_SCOPE_LINE,
            f"CYW43_SDIO_DPC_TRUTH {truth}",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == reason


def test_gate_summary_rejects_live_ring_poison_truth() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_SDIO_DPC generation=9 captures=6 published=6 consumed=6 "
            "rearms=5 overruns=0 epoch_errors=0 sequence_errors=0 "
            "ack_failures=0 owner_active=yes poisoned=yes masked=no",
            normalizer.CYW43_SDIO_DPC_SCOPE_LINE,
            "CYW43_SDIO_DPC_TRUTH generation=9 owner_active=yes "
            "ring_poisoned=yes client_sample_stale=no ring_consumer=6 "
            "sample_consumer=6 sample_reason=ring-poisoned "
            "authority=live-ring action=restart-pair",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "poisoned"
    assert record["WIFI_DPC_RING_POISONED"] == "yes"


def test_gate_summary_rejects_unpaired_wifi_dpc_truth() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_SDIO_DPC generation=9 captures=6 published=6 consumed=6 "
            "rearms=5 overruns=0 epoch_errors=0 sequence_errors=0 "
            "ack_failures=0 owner_active=yes poisoned=yes masked=no",
            "CYW43_SDIO_DPC_TRUTH generation=9 owner_active=yes "
            "ring_poisoned=no client_sample_stale=yes ring_consumer=6 "
            "sample_consumer=5 sample_reason=ring-consumer-mismatch "
            "authority=live-ring action=rerun-proof",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "truth-sequence-mismatch"


def test_gate_summary_rejects_orphan_live_poison_after_healthy_truth() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_SDIO_DPC generation=9 captures=6 published=6 consumed=6 "
            "rearms=5 overruns=0 epoch_errors=0 sequence_errors=0 "
            "ack_failures=0 owner_active=yes poisoned=no masked=no",
            normalizer.CYW43_SDIO_DPC_SCOPE_LINE,
            "CYW43_SDIO_DPC_TRUTH generation=9 owner_active=yes "
            "ring_poisoned=no client_sample_stale=no ring_consumer=6 "
            "sample_consumer=6 sample_reason=current authority=live-ring "
            "action=none",
            "CYW43_SDIO_DPC_TRUTH generation=9 owner_active=yes "
            "ring_poisoned=yes client_sample_stale=no ring_consumer=6 "
            "sample_consumer=6 sample_reason=ring-poisoned "
            "authority=live-ring action=restart-pair",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "truth-sequence-mismatch"
    assert record["WIFI_DPC_RING_POISONED"] == "no"


def test_gate_summary_rejects_cross_generation_orphan_dpc_truth() -> None:
    events = normalizer.parse_events(
        [
            *healthy_wifi_dpc_triplet(generation=9),
            "CYW43_SDIO_DPC_TRUTH generation=10 owner_active=yes "
            "ring_poisoned=yes client_sample_stale=no ring_consumer=7 "
            "sample_consumer=7 sample_reason=ring-poisoned "
            "authority=live-ring action=restart-pair",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "truth-sequence-mismatch"
    assert record["WIFI_DPC_GENERATION"] == 9
    assert record["WIFI_DPC_RING_POISONED"] == "no"


def test_gate_summary_treats_rearm_count_as_unmasked_telemetry() -> None:
    events = normalizer.parse_events(healthy_wifi_dpc_triplet(rearms=5))

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DPC_PROOF"] == "yes"
    assert record["WIFI_DPC_REASON"] == "none"
    assert record["WIFI_DPC_REARMS"] == 5


def test_gate_summary_rejects_legacy_dpc_line_without_owner_active_proof() -> None:
    """Historical grammar remains readable but cannot prove live activation."""

    events = normalizer.parse_events(
        [
            "CYW43_SDIO_DPC generation=9 captures=6 published=6 consumed=6 "
            "rearms=6 overruns=0 epoch_errors=0 sequence_errors=0 "
            "ack_failures=0 poisoned=no masked=no",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "truth-sequence-mismatch"
    assert record["WIFI_DPC_OWNER_ACTIVE"] == "unknown"


def test_gate_summary_rejects_exact_zero_activity_wifi_dpc_proof() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_SDIO_DPC generation=10 captures=0 published=0 consumed=0 "
            "rearms=0 overruns=0 epoch_errors=0 sequence_errors=0 "
            "ack_failures=0 owner_active=yes poisoned=no masked=no",
            normalizer.CYW43_SDIO_DPC_SCOPE_LINE,
            "CYW43_SDIO_DPC_TRUTH generation=10 owner_active=yes "
            "ring_poisoned=no client_sample_stale=no ring_consumer=0 "
            "sample_consumer=0 sample_reason=current authority=live-ring "
            "action=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "no-activity"
    assert record["WIFI_DPC_GENERATION"] == 10
    assert record["WIFI_DPC_CAPTURES"] == 0
    assert record["WIFI_DPC_PUBLISHED"] == 0
    assert record["WIFI_DPC_MASKED"] == "no"


@pytest.mark.parametrize(
    ("fields", "reason"),
    [
        ({"poisoned": "yes"}, "poisoned"),
        ({"overruns": 1}, "overrun"),
        ({"epoch_errors": 1}, "epoch-error"),
        ({"sequence_errors": 1}, "sequence-error"),
        ({"ack_failures": 1}, "ack-failure"),
        ({"owner_active": "no"}, "owner-inactive"),
        (
            {
                "captures": 0,
                "published": 0,
                "consumed": 0,
                "rearms": 0,
                "masked": "no",
            },
            "no-activity",
        ),
        ({"published": 5}, "capture-publish-mismatch"),
        ({"consumed": 5}, "consume-publish-mismatch"),
        ({"masked": "yes"}, "masked"),
    ],
)
def test_gate_summary_rejects_invalid_wifi_dpc_proof(
    fields: dict[str, int | str], reason: str
) -> None:
    values: dict[str, int | str] = {
        "generation": 9,
        "captures": 6,
        "published": 6,
        "consumed": 6,
        "rearms": 6,
        "overruns": 0,
        "epoch_errors": 0,
        "sequence_errors": 0,
        "ack_failures": 0,
        "owner_active": "yes",
        "poisoned": "no",
        "masked": "no",
    }
    values.update(fields)
    if values["epoch_errors"] != 0:
        values["poisoned"] = "yes"
    ring_poisoned = (
        "yes"
        if values["poisoned"] == "yes" and values["epoch_errors"] == 0
        else "no"
    )
    if ring_poisoned == "yes":
        sample_reason, action = "ring-poisoned", "restart-pair"
    elif values["owner_active"] == "no":
        sample_reason, action = "owner-inactive", "activate-owner"
    elif values["masked"] == "yes":
        sample_reason, action = "owner-rearm-pending", "service-sdio-owner"
    else:
        sample_reason, action = "current", "none"
    events = normalizer.parse_events(
        [
            "CYW43_SDIO_DPC "
            f"generation={values['generation']} captures={values['captures']} "
            f"published={values['published']} consumed={values['consumed']} "
            f"rearms={values['rearms']} overruns={values['overruns']} "
            f"epoch_errors={values['epoch_errors']} "
            f"sequence_errors={values['sequence_errors']} "
            f"ack_failures={values['ack_failures']} "
            f"owner_active={values['owner_active']} "
            f"poisoned={values['poisoned']} masked={values['masked']}",
            normalizer.CYW43_SDIO_DPC_SCOPE_LINE,
            f"CYW43_SDIO_DPC_TRUTH generation={values['generation']} "
            f"owner_active={values['owner_active']} "
            f"ring_poisoned={ring_poisoned} client_sample_stale=no "
            f"ring_consumer={values['consumed']} "
            f"sample_consumer={values['consumed']} "
            f"sample_reason={sample_reason} authority=live-ring "
            f"action={action}",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == reason


def test_wifi_dpc_proof_requires_exact_complete_line() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_SDIO_DPC generation=9 captures=6 published=6 consumed=6 "
            "rearms=6 overruns=0 epoch_errors=0 sequence_errors=0 poisoned=no",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "malformed-line"


def test_wifi_dpc_proof_does_not_promote_rejected_supervisor_retry() -> None:
    """Attempt-two DPC health cannot rescue a forbidden outer retry."""

    events = normalizer.parse_events(
        [
            "wifi: preserved_failure source=live "
            "stage=cyw43-load-firmware-fail "
            "exact=cyw43-device-on-timeout-before-ht",
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            "CYW43_SDIO_DPC generation=1 captures=6 published=5 consumed=5 "
            "rearms=5 overruns=0 epoch_errors=0 sequence_errors=0 "
            "ack_failures=0 owner_active=yes poisoned=no masked=no",
            bootstrap_supervisor_line(1, "backoff", 1_000, 1_150, 2),
            bootstrap_supervisor_line(2, "begin", 0, 1_150, 3),
            *historical_bootstrap_gate8_ready_tail(
                2,
                generation=2,
                pair_epoch=2,
                stabilizing_ms=1_200,
                ready_ms=1_300,
                console_seq=4,
            ),
            "CYW43_SDIO_DPC generation=2 captures=8 published=8 consumed=8 "
            "rearms=8 overruns=0 epoch_errors=0 sequence_errors=0 "
            "ack_failures=0 owner_active=yes poisoned=no masked=no",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 2
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 1
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "outer-backoff-forbidden"
    )
    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "gate8-ready-missing"


def test_wifi_dpc_proof_rerun_can_close_transient_counter_skew() -> None:
    """A later quiescent sample may close an earlier in-flight count skew."""

    events = normalizer.parse_events(
        [
            "CYW43_SDIO_DPC generation=9 captures=6 published=5 consumed=5 "
            "rearms=5 overruns=0 epoch_errors=0 sequence_errors=0 "
            "ack_failures=0 owner_active=yes poisoned=no masked=no",
            normalizer.CYW43_SDIO_DPC_SCOPE_LINE,
            "CYW43_SDIO_DPC_TRUTH generation=9 owner_active=yes "
            "ring_poisoned=no client_sample_stale=no ring_consumer=5 "
            "sample_consumer=5 sample_reason=current authority=live-ring "
            "action=none",
            *healthy_wifi_dpc_triplet(captures=8),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_DPC_PROOF"] == "yes"
    assert record["WIFI_DPC_REASON"] == "none"
    assert record["WIFI_DPC_CAPTURES"] == 8


def test_boot_acceptance_requires_wifi_dpc_but_wired_history_does_not() -> None:
    wifi_record = {
        "NET_ACTIVE": "wifi",
        "WIFI_DPC_PROOF": "no",
        "WIFI_DPC_REASON": "missing",
    }
    wired_record = {
        "NET_ACTIVE": "wired",
        "WIFI_DPC_PROOF": "no",
        "WIFI_DPC_REASON": "missing",
    }

    assert "wifi-dpc-proof-missing" in normalizer.boot_evidence_blockers(wifi_record)
    assert "wifi-dpc-proof-missing" not in normalizer.boot_evidence_blockers(wired_record)


def test_gate_summary_degrades_command_ready_without_first_report() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_gate keyboard=yes first_report=no first_byte=no "
            "proof_gate=10 target_gate=10 next=none blocker=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    blockers = normalizer.boot_evidence_blockers(record)

    assert record["USB_GATE"] == 8
    assert record["USB_BLOCKER"] == "usb-hid-interrupt-no-completion"
    assert record["USB_LOCAL_SEAT_STATE"] == "blocked"
    assert record["USB_LOCAL_SEAT_REASON"] == "usb-hid-interrupt-no-completion"
    assert record["USB_COMMAND_READY"] == "no"
    assert record["USB_FIRST_REPORT_READY"] == "no"
    assert record["USB_FIRST_BYTE_READY"] == "no"
    assert "local-seat-usb-gate-incomplete" in blockers
    assert "local-seat-usb-first-byte-missing" in blockers


def test_gate_summary_accepts_clean_command_ready_without_runtime_gate() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_OWNER_STATE contract=usb-local-seat hot_path=usb-keyboard "
            "owner_state=missing hardware_owner=unproven descriptor=missing",
            "[local-seat] hdmi prompt enabled reason=usb-console-command-ready "
            "action=show-prompt",
            "[local-seat] usb keyboard command-ready action=enable-command-input "
            "clean_polls=2 arming_bytes=0 queued=0 accepted=0 drained=0 echoed=0 "
            "no_reply=0 recovery_pending=no hdmi_pending=0 hdmi_submitted=0",
            "HDMI_FRAME_SUBMIT reason=keyboard-scrollback status=ready "
            "root_console_ready=yes attached=yes failed=no fatal=no redraw=yes",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_GATE"] == 10
    assert record["USB_BLOCKER"] == "none"
    assert record["USB_COMMAND_READY"] == "yes"
    assert record["USB_FIRST_REPORT_READY"] == "yes"
    assert record["USB_LOCAL_SEAT_STATE"] == "ready"
    assert record["USB_BUSY_AFTER_READY"] == "no"


def test_gate_summary_tracks_usb_startup_churn_without_marking_recovered() -> None:
    events = normalizer.parse_events(
        [
            "usb: runtime_queue queue_valid=no queued_reports=0 "
            "doorbell_pending=no preserved_events=0 transfer_events=0 "
            "report_status=none",
            "usb: recovery_request action=no-reply queued_reports=0 "
            "transfer_events=0 report_status=none",
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "proof_gate=10 target_gate=10 next=none blocker=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_GATE"] == 10
    assert record["USB_BLOCKER"] == "none"
    assert record["USB_STARTUP_BLOCKER_SEEN"] == "yes"
    assert record["USB_ACTIVE_BLOCKER_SEEN"] == "no"
    assert record["USB_RECOVERED_FROM_BLOCKER"] == "no"
    assert record["USB_LOCAL_SEAT_STATE"] == "ready"
    assert record["USB_LOCAL_SEAT_REASON"] == "command-ready"
    assert "local-seat-usb-startup-blocker" in normalizer.boot_evidence_blockers(record)


def test_gate_summary_quarantines_invalid_usb_queue_enumeration_snapshot() -> None:
    """Enumeration result bytes must not become HID queue counters."""

    events = normalizer.parse_events(
        [
            "usb: runtime_queue queue_valid=no detail=0x0205 "
            "result=0x0f000001 queued_reports=1 doorbell_pending=no "
            "preserved_events=0 transfer_events=15 report_status=none",
            "usb: stall_telemetry queue_valid=no queued_reports=1 doorbell=no "
            "preserved=0 transfer_events=15 report_status=none",
            "usb: sustained_input queue_valid=no detail=0x0205 "
            "result=0x0f000001 queued_reports=1 transfer_events=15 "
            "report_status=none arming=0 accepted=0 drained=0 echoed=0",
            "usb: runtime_gate keyboard=no first_report=no first_byte=no "
            "command_ready=no proof_gate=7 target_gate=10 "
            "blocker=hub-descriptor-transfer-failed",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_RUNTIME_QUEUE_VALID"] == "no"
    assert record["USB_RUNTIME_QUEUED_REPORTS"] == 0
    assert record["USB_RUNTIME_TRANSFER_EVENTS"] == 0
    assert record["USB_GATE"] == 7
    assert record["USB_BLOCKER"] == "hub-descriptor-transfer-failed"
    assert record["USB_STARTUP_BLOCKER_SEEN"] == "yes"
    assert "local-seat-usb-one-deep-proof-missing" in (
        normalizer.boot_evidence_blockers(record)
    )


def test_gate_summary_accepts_typed_unknown_invalid_usb_queue_companions() -> None:
    """Current invalid queue output stays untyped and fail-closed."""

    events = normalizer.parse_events(
        [
            "usb: runtime_queue queue_valid=no detail=0x0205 "
            "result=0x0f000001 queued_reports=unknown "
            "doorbell_pending=unknown preserved_events=unknown "
            "transfer_events=unknown report_status=unknown",
            "usb: stall_telemetry queue_valid=no queued_reports=unknown "
            "doorbell=unknown preserved=unknown transfer_events=unknown "
            "report_status=unknown",
            "usb: runtime_gate keyboard=no first_report=no first_byte=no "
            "command_ready=no proof_gate=7 target_gate=10 "
            "blocker=hub-descriptor-transfer-failed",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_RUNTIME_QUEUE_VALID"] == "no"
    assert record["USB_RUNTIME_QUEUED_REPORTS"] == 0
    assert record["USB_RUNTIME_TRANSFER_EVENTS"] == 0
    assert record["USB_GATE"] == 7
    assert record["USB_BLOCKER"] == "hub-descriptor-transfer-failed"


def test_gate_summary_names_usb_hid_interrupt_no_completion() -> None:
    events = normalizer.parse_events(
        [
            "usb: runtime_queue queue_valid=yes queued_reports=1 "
            "doorbell_pending=no preserved_events=0 transfer_events=0 "
            "report_status=none pre_first_report_no_completion=yes debt=yes",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_GATE"] == 8
    assert record["USB_BLOCKER"] == "usb-hid-interrupt-no-completion"
    assert record["USB_STARTUP_BLOCKER_SEEN"] == "yes"
    assert record["USB_ACTIVE_BLOCKER_SEEN"] == "no"


def test_gate_summary_downgrades_command_ready_hid_no_completion() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready action=enable-command-input "
            "clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_queue queue_valid=yes queued_reports=1 "
            "doorbell_pending=no preserved_events=0 transfer_events=0 "
            "report_status=decoded-empty pre_first_report_no_completion=yes debt=yes",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_GATE"] == 8
    assert record["USB_BLOCKER"] == "usb-hid-interrupt-no-completion"
    assert record["USB_COMMAND_READY"] == "no"
    assert record["USB_LOCAL_SEAT_STATE"] == "blocked"
    assert record["USB_LOCAL_SEAT_REASON"] == "usb-hid-interrupt-no-completion"
    assert record["USB_BUSY_AFTER_READY"] == "yes"


def test_gate_summary_rejects_non_one_deep_queue_before_first_byte() -> None:
    """Command readiness cannot authorize an invalid interrupt-IN depth."""

    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready action=enable-command-input "
            "clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_queue queue_valid=yes queued_reports=2 "
            "doorbell_pending=no preserved_events=0 transfer_events=1 "
            "report_status=idle-report",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "command_ready=yes proof_gate=10 target_gate=10 blocker=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_RUNTIME_QUEUED_REPORTS"] == 2
    assert record["USB_GATE"] == 8
    assert record["USB_BLOCKER"] == "usb-hid-interrupt-queue-depth-invalid"
    assert record["USB_COMMAND_READY"] == "no"
    assert record["USB_LOCAL_SEAT_STATE"] == "blocked"
    assert record["USB_BUSY_AFTER_READY"] == "yes"


def test_gate_summary_keeps_invalid_queue_depth_after_later_first_byte() -> None:
    """A later byte cannot clear an invalid depth without a one-deep receipt."""

    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready action=enable-command-input "
            "clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_queue queue_valid=yes queued_reports=2 "
            "doorbell_pending=no preserved_events=0 transfer_events=1 "
            "report_status=idle-report",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x61",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "first_byte_source=linked-runtime-hid command_ready=yes "
            "proof_gate=10 target_gate=10 blocker=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_GATE"] == 10
    assert record["USB_BLOCKER"] == "none"
    assert record["USB_RUNTIME_QUEUED_REPORTS"] == 2
    assert record["USB_ACTIVE_BLOCKER_SEEN"] == "yes"
    assert record["USB_LOCAL_SEAT_STATE"] == "degraded"
    assert record["USB_LOCAL_SEAT_REASON"] == "usb-hid-interrupt-queue-depth-invalid"
    assert record["USB_BUSY_AFTER_READY"] == "yes"


def test_boot_evidence_requires_current_one_deep_usb_queue() -> None:
    """Gate 10 text alone cannot replace the current one-deep queue receipt."""

    ready_lines = [
        "[local-seat] usb keyboard command-ready action=enable-command-input "
        "clean_polls=2 no_reply=0 recovery_pending=no",
        "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
        "read=1 ascii=0x61",
        "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
        "first_byte_source=linked-runtime-hid command_ready=yes "
        "proof_gate=10 target_gate=10 blocker=none",
    ]

    missing_record = normalizer.summarize_gates(
        normalizer.parse_events(ready_lines)
    ).to_record()
    assert "local-seat-usb-one-deep-proof-missing" in (
        normalizer.boot_evidence_blockers(missing_record)
    )

    current_record = normalizer.summarize_gates(
        normalizer.parse_events(
            ready_lines
            + [
                "usb: runtime_queue queue_valid=yes queued_reports=1 "
                "doorbell_pending=no preserved_events=0 transfer_events=2 "
                "report_status=idle-report"
            ]
        )
    ).to_record()
    assert "local-seat-usb-one-deep-proof-missing" not in (
        normalizer.boot_evidence_blockers(current_record)
    )


def test_gate_summary_keeps_late_command_ready_bad_after_hid_no_completion() -> None:
    events = normalizer.parse_events(
        [
            "usb: recovery_request action=no-reply aux0=0x00000003 no_reply=128 "
            "streak=128 cooldown=2 recovery_aux_requests=2 recovery_aux_pending=yes "
            "queue_empty=yes accepted=0 drained=0 echoed=0 detail=0x0501 "
            "result=0x00000020 queued_reports=1 report_status=none "
            "report_status_code=0 stale_runtime_queue=no full_idle_queue=yes "
            "pre_first_report_no_completion=yes debt=yes",
            "[local-seat] usb keyboard command-ready action=enable-command-input "
            "clean_polls=2 no_reply=0 recovery_pending=no",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_GATE"] == 8
    assert record["USB_BLOCKER"] == "usb-hid-interrupt-no-completion"
    assert record["USB_COMMAND_READY"] == "no"
    assert record["USB_LOCAL_SEAT_STATE"] == "blocked"
    assert record["USB_LOCAL_SEAT_REASON"] == "usb-hid-interrupt-no-completion"


def test_gate_summary_treats_post_ready_cumulative_usb_counters_as_historical() -> None:
    events = normalizer.parse_events(
        [
            "usb: runtime_queue queue_valid=no queued_reports=0 "
            "doorbell_pending=no preserved_events=0 transfer_events=0 "
            "report_status=none",
            "[local-seat] usb keyboard command-deferred "
            "reason=keyboard-poll-cooldown action=log-recovery-deferred",
            "[local-seat] usb keyboard command-ready action=enable-command-input clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_queue queue_valid=yes detail=0x0501 result=0x01000420 "
            "queued_reports=1 doorbell_pending=no preserved_events=0 "
            "transfer_events=1 report_status=idle-report",
            "usb: sustained_input queue_valid=yes detail=0x0501 result=0x01000420 "
            "queued_reports=1 transfer_events=1 report_status=idle-report "
            "arming=0 accepted=0 drained=0 echoed=0 no_reply=889 no_reply_streak=0 "
            "recovery_aux_requests=0 recovery_aux_pending=no runtime_skipped=902 "
            "blocker=none usb_burst=no drops=0",
            "usb: acceptance xhci=yes hid_keyboard=yes first_report=yes first_byte=no "
            "command_ready=yes usable=yes prompt_polling=yes "
            "input_observation=idle-report-no-key-byte",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "proof_gate=10 target_gate=10 next=command-input-ready blocker=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    blockers = normalizer.boot_evidence_blockers(record)

    assert record["USB_GATE"] == 10
    assert record["USB_BLOCKER"] == "none"
    assert record["USB_STARTUP_BLOCKER_SEEN"] == "yes"
    assert record["USB_ACTIVE_BLOCKER_SEEN"] == "no"
    assert record["USB_LOCAL_SEAT_STATE"] == "ready"
    assert record["USB_BUSY_AFTER_READY"] == "no"
    assert "local-seat-usb-degraded" not in blockers
    assert "local-seat-usb-first-byte-missing" in blockers
    assert "local-seat-usb-burst-proof-missing" in blockers
    assert "local-seat-usb-startup-blocker" in blockers


def test_gate_summary_treats_no_idle_report_as_usb_ready() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready "
            "action=enable-command-input clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_queue queue_valid=yes detail=0x0501 result=0x00001801 "
            "queued_reports=1 doorbell_pending=no preserved_events=0 "
            "transfer_events=0 report_status=no-idle-report",
            "usb: runtime_recovery diag_valid=no recoveries=0 failures=0 "
            "queue_collapse=0 stage=none stage_code=0 reason=none reason_code=0 "
            "command_completion_blocked=0",
            "usb: acceptance xhci=yes hid_keyboard=yes first_report=yes first_byte=no "
            "command_ready=yes usable=yes prompt_polling=yes "
            "input_observation=endpoint-armed-no-idle-report",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "proof_gate=10 target_gate=10 next=command-input-ready blocker=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    blockers = normalizer.boot_evidence_blockers(record)

    assert record["USB_GATE"] == 10
    assert record["USB_BLOCKER"] == "none"
    assert record["USB_COMMAND_READY"] == "yes"
    assert record["USB_RUNTIME_RECOVERY_DIAG_VALID"] == "no"
    assert record["USB_LOCAL_SEAT_STATE"] == "ready"
    assert record["USB_BUSY_AFTER_READY"] == "no"
    assert "local-seat-usb-degraded" not in blockers
    assert "local-seat-usb-first-byte-missing" in blockers


def test_gate_summary_treats_missing_usb_recovery_diag_as_telemetry_for_idle_report() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready action=enable-command-input clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_queue queue_valid=yes detail=0x0501 result=0x01000420 "
            "queued_reports=1 doorbell_pending=no preserved_events=0 "
            "transfer_events=1 report_status=idle-report",
            "usb: runtime_recovery diag_valid=no recoveries=0 failures=0 "
            "queue_collapse=0 stage=none stage_code=0 reason=none reason_code=0 "
            "command_completion_blocked=0",
            "usb: acceptance xhci=yes hid_keyboard=yes first_report=yes first_byte=no "
            "command_ready=yes usable=yes prompt_polling=yes "
            "input_observation=idle-report-no-key-byte",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "proof_gate=10 target_gate=10 next=command-input-ready blocker=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    blockers = normalizer.boot_evidence_blockers(record)

    assert record["USB_GATE"] == 10
    assert record["USB_BLOCKER"] == "none"
    assert record["USB_RUNTIME_RECOVERY_DIAG_VALID"] == "no"
    assert record["USB_LOCAL_SEAT_STATE"] == "ready"
    assert record["USB_LOCAL_SEAT_REASON"] == "command-ready"
    assert "local-seat-usb-degraded" not in blockers


def test_gate_summary_keeps_missing_usb_recovery_diag_degraded_for_recovery_fault() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready action=enable-command-input clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_queue queue_valid=yes detail=0x0501 result=0x37000420 "
            "queued_reports=32 doorbell_pending=no preserved_events=0 "
            "transfer_events=55 report_status=queue-collapse",
            "usb: runtime_recovery diag_valid=no recoveries=1 failures=1 "
            "queue_collapse=1 stage=ready stage_code=9 "
            "reason=queue-collapse reason_code=4 command_completion_blocked=1",
            "usb: acceptance xhci=yes hid_keyboard=yes first_report=yes first_byte=no "
            "command_ready=yes usable=yes prompt_polling=yes "
            "input_observation=idle-report-no-key-byte",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "proof_gate=10 target_gate=10 next=command-input-ready blocker=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    blockers = normalizer.boot_evidence_blockers(record)

    assert record["USB_GATE"] == 8
    assert record["USB_BLOCKER"] == "usb-hid-interrupt-queue-depth-invalid"
    assert record["USB_RUNTIME_RECOVERY_DIAG_VALID"] == "no"
    assert record["USB_LOCAL_SEAT_STATE"] == "blocked"
    assert record["USB_LOCAL_SEAT_REASON"] == "usb-hid-interrupt-queue-depth-invalid"
    assert "local-seat-usb-gate-incomplete" in blockers


def test_gate_summary_marks_usb_degraded_after_post_ready_blocker() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_queue queue_valid=no queued_reports=0 "
            "doorbell_pending=no preserved_events=0 transfer_events=0 "
            "report_status=none",
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "proof_gate=10 target_gate=10 next=none blocker=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    blockers = normalizer.boot_evidence_blockers(record)

    assert record["USB_GATE"] == 8
    assert record["USB_BLOCKER"] == "usb-hid-interrupt-queue-depth-invalid"
    assert record["USB_STARTUP_BLOCKER_SEEN"] == "no"
    assert record["USB_ACTIVE_BLOCKER_SEEN"] == "yes"
    assert record["USB_RECOVERED_FROM_BLOCKER"] == "yes"
    assert record["USB_LOCAL_SEAT_STATE"] == "blocked"
    assert record["USB_LOCAL_SEAT_REASON"] == "usb-hid-interrupt-queue-depth-invalid"
    assert "local-seat-usb-gate-incomplete" in blockers
    assert "local-seat-usb-active-blocker" in blockers


def test_gate_summary_marks_usb_degraded_when_busy_reappears_after_ready() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "proof_gate=10 target_gate=10 next=none blocker=none",
            "[local-seat] usb keyboard command-deferred "
            "reason=keyboard-poll-cooldown action=log-recovery-deferred",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    blockers = normalizer.boot_evidence_blockers(record)

    assert record["USB_GATE"] == 10
    assert record["USB_BLOCKER"] == "none"
    assert record["USB_LOCAL_SEAT_STATE"] == "degraded"
    assert record["USB_LOCAL_SEAT_REASON"] == "usb-post-ready-busy"
    assert record["USB_BUSY_AFTER_READY"] == "yes"
    assert "local-seat-usb-degraded" in blockers
    assert "local-seat-usb-busy-after-ready" in blockers


def test_gate_summary_treats_transient_post_ready_busy_as_superseded() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "command_ready=yes proof_gate=10 target_gate=10 next=none blocker=none",
            "[local-seat] usb keyboard command-deferred "
            "reason=keyboard-poll-no-reply action=log-recovery-deferred",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x61",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "command_ready=yes proof_gate=10 target_gate=10 next=none blocker=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    blockers = normalizer.boot_evidence_blockers(record)

    assert record["USB_GATE"] == 10
    assert record["USB_BLOCKER"] == "none"
    assert record["USB_LOCAL_SEAT_STATE"] == "ready"
    assert record["USB_LOCAL_SEAT_REASON"] == "command-ready"
    assert record["USB_BUSY_AFTER_READY"] == "no"
    assert "local-seat-usb-degraded" not in blockers
    assert "local-seat-usb-busy-after-ready" not in blockers


def test_gate_summary_treats_sustained_input_as_post_busy_progress() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "command_ready=yes proof_gate=10 target_gate=10 next=none blocker=none",
            "[local-seat] usb keyboard command-deferred "
            "reason=keyboard-poll-no-reply action=log-recovery-deferred",
            "usb: sustained_input queue_valid=yes detail=0x0501 "
            "result=0x64000020 queued_reports=1 transfer_events=100 "
            "report_status=none accepted=14 drained=14 echoed=14 "
            "no_reply=27 no_reply_streak=0 recovery_aux_requests=1 "
            "recovery_aux_pending=no blocker=none usb_burst=yes drops=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    blockers = normalizer.boot_evidence_blockers(record)

    assert record["USB_GATE"] == 10
    assert record["USB_LOCAL_SEAT_STATE"] == "ready"
    assert record["USB_BUSY_AFTER_READY"] == "no"
    assert record["USB_BURST_PROOF"] == "yes"
    assert "local-seat-usb-degraded" not in blockers
    assert "local-seat-usb-busy-after-ready" not in blockers


def test_gate_summary_reports_post_first_byte_unmatched_transfer() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x61",
            "usb: runtime_queue queue_valid=yes queued_reports=73 "
            "doorbell_pending=no preserved_events=0 transfer_events=55 "
            "report_status=unmatched-transfer",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_GATE"] == 10
    assert record["USB_BLOCKER"] == "none"
    assert (
        record["USB_POST_FIRST_BYTE_BLOCKER"]
        == "usb-post-first-byte-unmatched-transfer"
    )


def test_gate_summary_rejects_latched_ready_for_retained_usb_request() -> None:
    """A startup Gate 10 cannot hide one live request with no terminal receipt."""

    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready "
            "source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte "
            "source=linked-runtime-hid read=1 ascii=0x75",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "command_ready=yes proof_gate=10 target_gate=10 next=none blocker=none",
            "[smp] activity local-seat runtime=present attached=yes "
            "backend_polls=23896 backend_bytes=1 queued=0 arming=1 accepted=1 "
            "drained=1 echoed=1 dropped=0 no_reply=0 cooldown=0 cooldown_skips=0",
            "usb: stall_counter domain=usb-runtime contract=usb-local-seat "
            "submitted=10669 completed=10668 busy=0 same=92438 timeouts=3546 "
            "keep_active=3546 aborts=0 fault=0 budget=0 rx=12/12 tx=0/0",
            "[smp] activity local-seat runtime=present attached=yes "
            "backend_polls=165948 backend_bytes=12 queued=0 arming=1 accepted=12 "
            "drained=12 echoed=12 dropped=0 no_reply=0 cooldown=0 cooldown_skips=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_GATE"] == 10
    assert record["USB_GATE_SCOPE"] == "startup"
    assert record["USB_RUNTIME_DRIVER_ACTIVE"] == "yes"
    assert record["USB_RUNTIME_DRIVER_OUTSTANDING"] == 1
    assert record["USB_RUNTIME_DRIVER_ACTIVE_NO_PROGRESS"] == 3546
    assert record["USB_RUNTIME_DRIVER_SAME_REQUEST"] == 92438
    assert record["USB_RUNTIME_DRIVER_KEEP_ACTIVE"] == 3546
    assert record["USB_POST_FIRST_BYTE_BLOCKER"] == "usb-retained-request-no-terminal"
    assert record["USB_CURRENT_LIVENESS"] == "unproven"
    assert record["USB_CURRENT_LIVENESS_REASON"] == "usb-retained-request-no-terminal"
    assert record["USB_PHYSICAL_INPUT_PROOF"] == "no"
    assert record["USB_ACTIVE_BLOCKER_SEEN"] == "yes"
    assert record["USB_RECOVERED_FROM_BLOCKER"] == "no"
    assert record["USB_RECOVERY_STATE"] == "degraded-active"
    assert record["USB_LOCAL_SEAT_STATE"] == "degraded"
    assert record["USB_LOCAL_SEAT_REASON"] == "usb-retained-request-no-terminal"


def test_gate_summary_reports_post_first_byte_queue_collapse() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x61",
            "usb: runtime_queue queue_valid=yes queued_reports=4 "
            "doorbell_pending=no preserved_events=0 transfer_events=255 "
            "report_status=queue-collapse",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_GATE"] == 10
    assert record["USB_BLOCKER"] == "none"
    assert (
        record["USB_POST_FIRST_BYTE_BLOCKER"]
        == "usb-post-first-byte-queue-collapse"
    )
    assert record["USB_RUNTIME_QUEUED_REPORTS"] == 4
    assert record["USB_RUNTIME_TRANSFER_EVENTS"] == 255
    assert record["USB_RUNTIME_REPORT_STATUS"] == "queue-collapse"


def test_gate_summary_reports_usb_runtime_recovery_diagnostics() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x61",
            "usb: runtime_queue queue_valid=yes queued_reports=32 "
            "doorbell_pending=no preserved_events=0 transfer_events=0 "
            "report_status=none",
            "usb: runtime_recovery diag_valid=yes recoveries=1 failures=0 "
            "queue_collapse=0 stage=ready stage_code=9 "
            "reason=full-queue-no-event reason_code=3 "
            "command_completion_blocked=2",
            "usb: runtime_recovery diag_valid=unknown recoveries=0 failures=0 "
            "queue_collapse=0 stage=unknown stage_code=0 "
            "reason=unknown reason_code=0 command_completion_blocked=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_RUNTIME_RECOVERY_DIAG_VALID"] == "yes"
    assert record["USB_RUNTIME_ENDPOINT_RECOVERIES"] == 1
    assert record["USB_RUNTIME_ENDPOINT_RECOVERY_FAILURES"] == 0
    assert record["USB_RUNTIME_QUEUE_COLLAPSE_RECOVERIES"] == 0
    assert record["USB_RUNTIME_RECOVERY_STAGE"] == "ready"
    assert record["USB_RUNTIME_RECOVERY_REASON"] == "full-queue-no-event"
    assert record["USB_RUNTIME_COMMAND_COMPLETION_BLOCKED"] == 2


def test_gate_summary_reports_post_first_byte_recovery_request_timeout() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x65",
            "DRIVER_TASK_RING_CALL_ABORT contract=usb-local-seat "
            "endpoint=0x07a3 request=171845 mode=prompt-slice "
            "reason=timeout-resume-limit timeout_count=256 opcode=1 "
            "arg0=2 aux0=0x55534252 frame_len=0 owner=linked-runtime "
            "marker_valid=yes marker_sequence=171845 marker_phase=268 "
            "marker_phase_name=usb-hid-interrupt-queue-begin "
            "marker_aux0=0x55534252 blocker=usb-hid-interrupt-queue-begin",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert (
        record["USB_POST_FIRST_BYTE_BLOCKER"]
        == "usb-post-first-byte-recovery-request-timeout"
    )


def test_gate_summary_reports_post_first_byte_recovery_request_no_reply() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x65",
            "usb: recovery_request action=no-reply aux0=0x55534252 "
            "no_reply=27 streak=9 cooldown=2 recovery_aux_requests=1 "
            "recovery_aux_pending=yes queue_empty=yes accepted=12 drained=11 "
            "echoed=10 detail=0x0501 result=0x00000220 queued_reports=1 "
            "report_status=none report_status_code=0 stale_runtime_queue=no "
            "full_idle_queue=no",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert (
        record["USB_POST_FIRST_BYTE_BLOCKER"]
        == "usb-post-first-byte-recovery-request-no-reply"
    )
    assert record["USB_KEYBOARD_NO_REPLIES"] == 27
    assert record["USB_RUNTIME_QUEUED_REPORTS"] == 1
    assert record["USB_RUNTIME_REPORT_STATUS"] == "none"


def test_gate_summary_reports_post_first_byte_recovery_pending_without_diag() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x65",
            "usb: sustained_input queue_valid=no detail=0x0501 result=0x00000000 "
            "queued_reports=0 transfer_events=0 report_status=none "
            "accepted=5 drained=5 echoed=5 no_reply=273197 no_reply_streak=9 "
            "recovery_aux_requests=4040 recovery_aux_pending=yes "
            "runtime_skipped=0 blocker=none usb_burst=yes drops=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert (
        record["USB_POST_FIRST_BYTE_BLOCKER"]
        == "usb-post-first-byte-recovery-pending-no-diag"
    )


def test_gate_summary_reports_post_first_byte_recovery_failed() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x61",
            "usb: runtime_queue queue_valid=yes queued_reports=4 "
            "doorbell_pending=no preserved_events=0 transfer_events=255 "
            "report_status=recovery-failed",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert (
        record["USB_POST_FIRST_BYTE_BLOCKER"]
        == "usb-post-first-byte-recovery-failed"
    )


def test_gate_summary_reports_post_first_byte_queue_collapse_risk() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x61",
            "usb: runtime_queue queue_valid=yes queued_reports=2 "
            "doorbell_pending=no preserved_events=0 transfer_events=255 "
            "report_status=produced-byte",
            "usb: sustained_verdict blocker=usb-post-first-byte-queue-collapse-risk "
            "usb_burst=no drops=0",
            "usb: event_loop keyboard_priority=97 runtime_skipped=97 "
            "serial_dispatch_yielded=308 post_runtime_keyboard=211 "
            "output_keyboard_polls=435 hdmi_pump=308",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert (
        record["USB_POST_FIRST_BYTE_BLOCKER"]
        == "usb-post-first-byte-queue-collapse-risk"
    )
    assert record["USB_RUNTIME_QUEUED_REPORTS"] == 2
    assert record["USB_RUNTIME_TRANSFER_EVENTS"] == 255
    assert record["USB_RUNTIME_REPORT_STATUS"] == "produced-byte"
    assert record["USB_EVENT_LOOP_RUNTIME_SKIPPED"] == 97


def test_gate_summary_does_not_infer_first_byte_from_usb_gate10() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid "
            "clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=no "
            "first_byte_source=none proof_gate=10 target_gate=10 "
            "next=command-input-ready blocker=none",
            "usb: runtime_queue queue_valid=yes queued_reports=1 "
            "doorbell_pending=no preserved_events=0 transfer_events=255 "
            "report_status=idle-report",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_GATE"] == 10
    assert record["USB_FIRST_BYTE_READY"] == "no"
    assert record["USB_POST_FIRST_BYTE_BLOCKER"] == "none"
    assert record["USB_LOCAL_SEAT_STATE"] == "ready"


def test_gate_summary_uses_linked_runtime_first_byte_for_post_input_health() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid "
            "clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "first_byte_source=linked-runtime-hid proof_gate=10 target_gate=10 "
            "next=command-input-ready blocker=none",
            "usb: runtime_queue queue_valid=yes queued_reports=1 "
            "doorbell_pending=no preserved_events=0 transfer_events=255 "
            "report_status=idle-report",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_POST_FIRST_BYTE_BLOCKER"] == "none"
    assert record["USB_LOCAL_SEAT_STATE"] == "ready"


def test_gate_summary_uses_current_active_no_progress_not_cumulative_keep_active() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid "
            "clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "first_byte_source=linked-runtime-hid command_ready=yes "
            "proof_gate=10 target_gate=10 blocker=none",
            "usb: stall_counter domain=usb-runtime contract=usb-local-seat "
            "active=yes outstanding=11 active_no_progress=0 submitted=81843 "
            "completed=81832 busy=0 same=658976 timeouts=2148 "
            "keep_active=2143 aborts=5 fault=0 budget=0 rx=9/9 tx=0/0",
            "usb: runtime_queue queue_valid=yes queued_reports=1 "
            "doorbell_pending=no preserved_events=0 transfer_events=255 "
            "report_status=idle-report",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_RUNTIME_DRIVER_ACTIVE"] == "yes"
    assert record["USB_RUNTIME_DRIVER_ACTIVE_NO_PROGRESS"] == 0
    assert record["USB_RUNTIME_DRIVER_KEEP_ACTIVE"] == 2143
    assert record["USB_POST_FIRST_BYTE_BLOCKER"] == "none"
    assert record["USB_LOCAL_SEAT_STATE"] == "ready"


def test_gate_summary_prefers_sustained_input_blocker() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x61",
            "usb: sustained_input queued_reports=4 transfer_events=255 "
            "report_status=recovery-failed accepted=416 drained=416 echoed=416 "
            "no_reply=6 runtime_skipped=97 "
            "blocker=usb-post-first-byte-recovery-failed",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert (
        record["USB_POST_FIRST_BYTE_BLOCKER"]
        == "usb-post-first-byte-recovery-failed"
    )
    assert record["USB_RUNTIME_REPORT_STATUS"] == "recovery-failed"
    assert record["USB_KEYBOARD_NO_REPLIES"] == 6
    assert record["USB_EVENT_LOOP_RUNTIME_SKIPPED"] == 97


def test_gate_summary_uses_latest_cumulative_usb_counter_snapshot() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x61",
            "DRIVER_TASK_COUNTER contract=usb-local-seat hot_path=usb-keyboard "
            "source=root-ring sequence=320019 submitted=320019 completed=296283 "
            "idle=0 fault=0 budget=0 frame=27 desc=27 staged_bytes=0 "
            "clean_ops=0 clean_bytes=0 inv_ops=0 inv_bytes=0 sends=0 yields=0 "
            "busy=0 same_request=0 timeouts=23756 keep_active=20 aborts=0 "
            "rx_frames=27 rx_bytes=27 tx_frames=0 tx_bytes=0",
            "[smp] activity local-seat runtime=present attached=yes "
            "keyboard_device=usb-kbd0 display=hdmi0 backend_poll=yes "
            "backend_polls=320009 backend_bytes=27 keyboard_ready=yes "
            "first_report=yes first_byte=yes queued=0 accepted=27 drained=27 "
            "echoed=27 drop=0 no_reply=0 cooldown=0 cooldown_skips=0 hdmi_drop=0",
            "DRIVER_TASK_COUNTER contract=usb-local-seat hot_path=usb-keyboard "
            "source=root-ring sequence=321655 submitted=321655 completed=296499 "
            "idle=0 fault=0 budget=0 frame=27 desc=27 staged_bytes=0 "
            "clean_ops=0 clean_bytes=0 inv_ops=0 inv_bytes=0 sends=0 yields=0 "
            "busy=0 same_request=0 timeouts=25176 keep_active=20 aborts=0 "
            "rx_frames=27 rx_bytes=27 tx_frames=0 tx_bytes=0",
            "[smp] activity local-seat runtime=present attached=yes "
            "keyboard_device=usb-kbd0 display=hdmi0 backend_poll=yes "
            "backend_polls=321645 backend_bytes=27 keyboard_ready=yes "
            "first_report=yes first_byte=yes queued=0 accepted=27 drained=27 "
            "echoed=27 drop=0 no_reply=0 cooldown=0 cooldown_skips=0 hdmi_drop=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_POST_FIRST_BYTE_BLOCKER"] == "none"
    assert record["DRIVER_TASK_COUNTER_SNAPSHOTS"] == 1
    assert record["DRIVER_TASK_COUNTER_TIMEOUTS"] == 25176
    assert record["USB_KEYBOARD_NO_REPLIES"] == 0


def test_latest_counter_deduplication_preserves_prior_invalid_snapshot() -> None:
    """A later valid cumulative sample must not erase malformed telemetry."""

    events = normalizer.parse_events(
        [
            "DRIVER_TASK_COUNTER contract=cyw43455 hot_path=cyw43-wifi "
            "source=root-ring sequence=1 submitted=1 completed=1 idle=0 "
            "fault=0 budget=0 frame=1 desc=1 staged_bytes=64 clean_ops=0 "
            "clean_bytes=0 inv_ops=0 inv_bytes=0 sends=1 yields=0 busy=0 "
            "same_request=0 timeouts=3 keep_active=0 aborts=0 overruns=0 "
            "drops=0 rx_frames=1 rx_bytes=64 tx_frames=1 tx_bytes=64",
            "DRIVER_TASK_COUNTER contract=cyw43455 hot_path=cyw43-wifi "
            "source=root-ring sequence=0 submitted=0 completed=0 idle=0 "
            "fault=0 budget=0 frame=0 desc=0 staged_bytes=0 clean_ops=0 "
            "clean_bytes=0 inv_ops=0 inv_bytes=0 sends=0 yields=0 busy=0 "
            "same_request=0 timeouts=0 keep_active=0 aborts=0 overruns=0 "
            "drops=0 rx_frames=0 rx_bytes=0 tx_frames=0 tx_bytes=0",
            "DRIVER_TASK_COUNTER contract=cyw43455 hot_path=cyw43-wifi "
            "source=root-ring sequence=3 submitted=3 completed=3 idle=0 "
            "fault=0 budget=0 frame=2 desc=2 staged_bytes=192 clean_ops=0 "
            "clean_bytes=0 inv_ops=0 inv_bytes=0 sends=3 yields=0 busy=0 "
            "same_request=0 timeouts=7 keep_active=0 aborts=0 overruns=0 "
            "drops=0 rx_frames=2 rx_bytes=128 tx_frames=2 tx_bytes=128",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["DRIVER_TASK_COUNTER_SNAPSHOTS"] == 1
    assert record["DRIVER_TASK_COUNTER_INVALID"] == 1
    assert record["DRIVER_TASK_COUNTER_TIMEOUTS"] == 7


def test_gate_summary_reports_active_post_first_byte_no_reply_delta() -> None:
    events = normalizer.parse_events(
        [
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid "
            "read=1 ascii=0x61",
            "[smp] activity local-seat runtime=present attached=yes "
            "keyboard_device=usb-kbd0 display=hdmi0 backend_poll=yes "
            "backend_polls=320009 backend_bytes=27 keyboard_ready=yes "
            "first_report=yes first_byte=yes queued=0 accepted=27 drained=27 "
            "echoed=27 drop=0 no_reply=6 cooldown=0 cooldown_skips=0 hdmi_drop=0",
            "[smp] activity local-seat runtime=present attached=yes "
            "keyboard_device=usb-kbd0 display=hdmi0 backend_poll=yes "
            "backend_polls=321645 backend_bytes=27 keyboard_ready=yes "
            "first_report=yes first_byte=yes queued=0 accepted=27 drained=27 "
            "echoed=27 drop=0 no_reply=8 cooldown=0 cooldown_skips=0 hdmi_drop=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_POST_FIRST_BYTE_BLOCKER"] == "usb-post-first-byte-no-progress"
    assert record["USB_KEYBOARD_NO_REPLIES"] == 8


def test_gate_summary_requires_command_ready_for_runtime_gate10_status() -> None:
    events = normalizer.parse_events(
        [
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "proof_gate=10 target_gate=10 next=none blocker=none",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.usb_gate == 9
    assert gates.usb_blocker == "command-input-ready"


def test_gate_summary_accepts_oldgood_usb_replay_contract() -> None:
    events = normalizer.parse_events(oldgood_usb_replay_lines())

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert gates.usb_gate == 10
    assert gates.usb_blocker == "none"
    assert record["USB_OLDGOOD_REPLAY"] == "yes"
    assert record["USB_OLDGOOD_LAST"] == "runtime-gate10"
    assert record["USB_OLDGOOD_MISSING"] == "none"


def test_gate_summary_accepts_oldgood_usb_resource_replay_contract() -> None:
    events = normalizer.parse_events(oldgood_usb_resource_replay_lines())

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_OLDGOOD_REPLAY"] == "yes"
    assert record["USB_OLDGOOD_LAST"] == "runtime-gate10"
    assert record["USB_OLDGOOD_MISSING"] == "none"


def test_gate_summary_accepts_identity_bound_usb_oldgood_retained_pair() -> None:
    """The adjacent complete runtime/current pair is an additive replay proof."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(retained_usb_oldgood_receipt_lines())
    ).to_record()

    assert record["USB_OLDGOOD_REPLAY"] == "yes"
    assert record["USB_OLDGOOD_LAST"] == "runtime-gate10"
    assert record["USB_OLDGOOD_MISSING"] == "none"


@pytest.mark.parametrize(
    ("old", "new", "missing"),
    [
        ("v=1", "v=2", "retained-receipt-version"),
        ("task=12", "task=0", "retained-receipt-identity"),
        ("task=12", "task=4294967296", "retained-receipt-numeric-invalid"),
        ("token=0xdeadbeef", "token=0x00000000", "retained-receipt-identity"),
        ("mask=0x00003fff", "mask=0x00001fff", "retained-receipt-steps"),
        ("mask=0x00003fff", "mask=0x80003fff", "retained-receipt-steps"),
        ("commit=14", "commit=13", "retained-receipt-uncommitted"),
        ("topology=0x10230581", "topology=0x10230501", "retained-receipt-topology"),
        ("input_gen=9", "input_gen=0", "retained-receipt-identity"),
        (
            "source=linked-runtime-hid",
            "source=none",
            "retained-receipt-source",
        ),
        ("owners=driver-owned+driver-owned", "owners=missing+driver-owned", "usb-owner-state"),
        ("owners=driver-owned+driver-owned", "owners=driver-owned+missing", "pcie-owner-state"),
        ("descriptors=sealed+sealed", "descriptors=missing+sealed", "usb-descriptor-seal"),
        ("descriptors=sealed+sealed", "descriptors=sealed+missing", "pcie-descriptor-seal"),
        ("command_ready=yes", "command_ready=no", "command-ready"),
        ("proof_gate=14", "proof_gate=0", "runtime-gate10"),
        ("blocker=none", "blocker=receipt-missing", "runtime-gate10"),
    ],
)
def test_usb_oldgood_retained_pair_rejects_incomplete_or_noncurrent_proof(
    old: str,
    new: str,
    missing: str,
) -> None:
    """Every runtime identity and current-root condition remains fail-closed."""

    lines = [
        line.replace(old, new, 1) if old in line else line
        for line in retained_usb_oldgood_receipt_lines()
    ]
    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["USB_OLDGOOD_REPLAY"] == "no"
    assert record["USB_OLDGOOD_MISSING"] == missing


@pytest.mark.parametrize(
    "mutation",
    ("gap", "missing-current", "malformed-current", "trailing-reserved"),
)
def test_usb_oldgood_retained_pair_requires_latest_physical_adjacency(
    mutation: str,
) -> None:
    """A clipped, spliced, or superseded pair cannot reuse older evidence."""

    lines = retained_usb_oldgood_receipt_lines()
    if mutation == "gap":
        lines.insert(1, "unclassified physical serial gap")
    elif mutation == "missing-current":
        lines.pop()
    elif mutation == "malformed-current":
        lines[-1] = "USB_OLDGOOD_CURRENT clipped"
    else:
        lines.append("USB_OLDGOOD_TRUNCATED")

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["USB_OLDGOOD_REPLAY"] == "no"
    assert record["USB_OLDGOOD_MISSING"] != "none"


def test_newer_invalid_usb_oldgood_pair_revokes_older_complete_pair() -> None:
    """Only the latest passive USB receipt transaction can carry authority."""

    lines = [
        *retained_usb_oldgood_receipt_lines(),
        retained_usb_oldgood_receipt_lines()[0],
        retained_usb_oldgood_receipt_lines()[1].replace(
            "command_ready=yes", "command_ready=no"
        ),
    ]
    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["USB_OLDGOOD_REPLAY"] == "no"
    assert record["USB_OLDGOOD_MISSING"] == "command-ready"


def test_gate_summary_rejects_usb_oldgood_hid_endpoint_not_ready() -> None:
    """A not-ready HID endpoint breadcrumb is a blocker, not endpoint proof."""

    lines = [
        line.replace("status=hid-endpoint-ready", "status=hid-endpoint-not-ready")
        for line in oldgood_usb_resource_replay_lines()
        if "stage=usb-keyboard-interrupt-in" not in line
        and "stage=usb-keyboard-first-report" not in line
        and "runtime keyboard first-byte" not in line
        and "usb: runtime_gate" not in line
    ]

    events = normalizer.parse_events(
        [
            *lines,
            "[local-seat] usb hid first report contract=usb-local-seat "
            "source=linked-runtime-hid tag=usb-hid-report-event len=8 accepted=8 "
            "detail=0x0501 result=0x00000001 transfer_event=yes",
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid read=1 "
            "ascii=0x74 detail=0x0501 result=0x00000001",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "first_byte_source=linked-runtime-hid proof_gate=10 target_gate=10 blocker=none",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_OLDGOOD_REPLAY"] == "no"
    assert record["USB_OLDGOOD_LAST"] == "hub-child-probe"
    assert record["USB_OLDGOOD_MISSING"] == "hid-endpoint"


def test_gate_summary_rejects_usb_gate10_without_oldgood_replay() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_OWNER_STATE contract=usb-local-seat hot_path=usb-keyboard "
            "owner_state=driver-owned descriptor=present root_pointer=no",
            "DRIVER_TASK_OWNER_STATE contract=pcie-root hot_path=pcie-root "
            "owner_state=driver-owned descriptor=present root_pointer=no",
            "[cohesix] WARNING: usb stop failed or was inactive before Cohesix boot; xHCI trust tokens cleared before Cohesix cold boot",
            "[local-seat] usb keyboard command-ready source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
            "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
            "first_byte_source=linked-runtime-hid proof_gate=10 target_gate=10 "
            "next=none blocker=none",
        ]
    )

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert gates.usb_gate == 10
    assert gates.usb_blocker == "none"
    assert record["USB_OLDGOOD_REPLAY"] == "no"
    assert record["USB_OLDGOOD_MISSING"] == "xhci-controller-ready"


def test_gate_summary_does_not_credit_uboot_usb_handoff_as_replay() -> None:
    events = normalizer.parse_events(
        [
            "[cohesix:usb-trace] stage=handoff-usb-reset-begin input=0",
            *oldgood_usb_replay_lines(),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["USB_BOOTLOADER_HANDOFF_SEEN"] == "yes"
    assert record["USB_OLDGOOD_REPLAY"] == "no"
    assert record["USB_OLDGOOD_MISSING"] == "forbidden-bootloader-handoff"


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
        "usb-hub-port-status-ack-done": "hub-port-status-payload-no-reply",
        "usb-hub-port-status-payload-read": "hub-port-reset-no-reply",
        "usb-hub-port-status-disconnected": "hub-port-disconnected",
        "usb-hub-port-status-reset-active": "hub-port-reset-still-active",
        "usb-hub-port-status-enable-missing": "hub-port-enable-missing",
        "usb-hub-port-clear-changes-begin": "hub-port-clear-changes-no-reply",
        "usb-hub-port-clear-changes-done": "hub-port-reset-no-reply",
        "usb-hub-port-clear-changes-failed": "hub-port-clear-changes-failed",
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


def test_gate_summary_names_quiet_preassoc_firstread_as_association_missing() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-event-missing polls=24576 starts=0 "
            "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 event_rx=0 "
            "control_rx=0 empty_polls=24576 associated=no link_up=no "
            "assoc_event=none assoc_poll=0 post_assoc_polls=0 "
            "firstread_class=preassoc-cadence-empty "
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
    assert gates.wifi_blocker == "cyw43-association-event-missing"
    assert gates.wifi_exact == "cyw43-association-event-missing"
    assert gates.wifi_phase == "association"


def test_gate_summary_names_source_asserted_570a_as_asserted_empty() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-event-missing polls=24576 starts=0 "
            "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 event_rx=0 "
            "control_rx=0 empty_polls=24576 associated=no link_up=no "
            "assoc_event=none assoc_poll=0 post_assoc_polls=0 "
            "firstread_class=source-asserted-empty "
            "rx_firstread_attempts=10 rx_firstread_empty=10 "
            "rx_firstread_invalid=0 rx_firstread_failed=0 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "control_rx_firstread_attempts=10 control_rx_firstread_empty=10 "
            "control_rx_firstread_failed=0 last_rx_idle_detail=0x570a "
            "last_rx_idle_result=0xab070040 last_control_rx_idle_detail=0x570a "
            "last_control_rx_idle_result=0xab070040 "
            "rxsrc_mode=owner-card-sampled rxsrc_probe_len=64 rxsrc_ien=0x07 "
            "rxsrc_frame_ind=no rxsrc_host_int=yes rxsrc_card_int=no "
            "rxsrc_f2_ready=yes control_rxsrc_mode=owner-card-sampled "
            "control_rxsrc_probe_len=64 control_rxsrc_ien=0x07 "
            "control_rxsrc_frame_ind=no control_rxsrc_host_int=yes "
            "control_rxsrc_card_int=no control_rxsrc_f2_ready=yes "
            "last_flags=0x0000 last_len=0 last_ethertype=0x0000 "
            "last_ethertype_valid=no next_action=inspect-cyw43-rx-source-latch",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-firstread-source-asserted-empty"
    assert gates.wifi_exact == "cyw43-data-rx-firstread-source-asserted-empty"
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_names_ptk_install_over_prior_firstread_empty() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=host-eapol-required polls=32 starts=0 "
            "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 event_rx=0 "
            "control_rx=0 empty_polls=32 associated=no link_up=no "
            "assoc_event=none assoc_poll=0 post_assoc_polls=0 "
            "firstread_class=source-asserted-empty rx_firstread_attempts=10 "
            "rx_firstread_empty=10 rx_firstread_invalid=0 rx_firstread_failed=0 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "control_rx_firstread_attempts=10 control_rx_firstread_empty=10 "
            "control_rx_firstread_failed=0 last_rx_idle_detail=0x570a "
            "last_rx_idle_result=0xab070040 last_control_rx_idle_detail=0x570a "
            "last_control_rx_idle_result=0xab070040 "
            "next_action=inspect-cyw43-rx-source-latch",
            "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
            "msg=m1 action=recv-m1 poll=7 len=121",
            "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
            "msg=m2 action=send-m2 poll=7 len=129",
            "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
            "msg=m3 action=recv-m3 poll=8 len=177",
            "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
            "msg=m4 action=send-m4 poll=8 len=113",
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-host-eapol-ptk event=wsec-key-commandless-stale "
            "poll=1 flags=0x0000 seq=39 code=fault detail=0x530b "
            "result=0xfffffffe cmd=263 cmd_hex=0x00000107 id=39 "
            "header=extended expected_response_len=0 iovar=wsec_key "
            "nonmatching=1 malformed=0",
            "CYW43_DRIVER_TASK_HOST_EAPOL_KEY contract=cyw43455 "
            "kind=ptk stage=cyw43-host-eapol-ptk status=failed",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=host-eapol-ptk-install polls=64 starts=0 "
            "tx_retries=0 data_rx=0 eapol_rx=4 non_eapol_rx=0 event_rx=1 "
            "control_rx=0 empty_polls=16 associated=yes link_up=yes "
            "assoc_event=link-up assoc_poll=4 post_assoc_polls=2 "
            "firstread_class=source-asserted-empty rx_firstread_attempts=10 "
            "rx_firstread_empty=10 rx_firstread_invalid=0 rx_firstread_failed=0 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "control_rx_firstread_attempts=10 control_rx_firstread_empty=10 "
            "control_rx_firstread_failed=0 last_rx_idle_detail=0x570a "
            "last_rx_idle_result=0xab070040 last_control_rx_idle_detail=0x570a "
            "last_control_rx_idle_result=0xab070040 next_action=install-wsec-key",
            "ERR NETTEST reason=policy detail=wifi-host-eapol-required",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "host-eapol-ptk-install"
    assert gates.wifi_exact == "host-eapol-ptk-install"
    assert gates.wifi_phase == "join-security"


def test_gate_summary_refines_host_eapol_ptk_tx_no_reply_progress() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
            "msg=m3 action=recv-m3 poll=24 len=169",
            "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
            "msg=m4 action=send-m4 poll=24 len=113",
            "CYW43_DRIVER_TASK_HOST_EAPOL_DRAIN contract=cyw43455 "
            "stage=m4-before-wsec result=timeout tx_result=0x0000008f "
            "polls=69380 observed_control=0",
            "CYW43_DRIVER_TASK_CONTROL_REQUEST contract=cyw43455 "
            "stage=cyw43-host-eapol-ptk cmd=263 cmd_hex=0x00000107 "
            "id=39 runtime_flags=0x0002 bcdc_flags=0x0002 payload_len=189 "
            "response_len=0 iovar=wsec_key header_mode=extended",
            "CYW43_DRIVER_TASK_COMMAND_NO_REPLY contract=cyw43455 "
            "stage=cyw43-host-eapol-ptk op=6 flags=0x0002 "
            "target=0x00000000 payload_off=284 payload_len=209 total_len=209 "
            "iovar=wsec_key reason=cyw43-runtime-command-no-reply request=1590 "
            "resumes=0 progress_marker_valid=yes progress_sequence=1590 "
            "progress_phase=142 progress_phase_name=cyw43-sdio-owner-wait-begin "
            "progress_aux0=0x43595734 progress_request_match=yes",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-host-eapol-ptk status=tx-no-reply acceptance=no "
            "blocker=cyw43-host-eapol-ptk-tx-no-reply "
            "active_request_valid=yes active_request=1590 "
            "progress_marker_valid=yes progress_sequence=1590 "
            "progress_phase=142 progress_phase_name=cyw43-sdio-owner-wait-begin "
            "progress_aux0=0x43595734 progress_request_match=yes",
            "CYW43_DRIVER_TASK_HOST_EAPOL_KEY contract=cyw43455 "
            "kind=ptk stage=cyw43-host-eapol-ptk status=failed",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=host-eapol-ptk-install polls=24576 starts=0 "
            "tx_retries=0 data_rx=0 eapol_rx=4 non_eapol_rx=0 event_rx=4 "
            "control_rx=0 empty_polls=24576 associated=yes link_up=yes "
            "assoc_event=link-up assoc_poll=24 post_assoc_polls=4 "
            "next_action=inspect-host-eapol-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-host-eapol-ptk-tx-no-reply"
    assert gates.wifi_exact == "cyw43-host-eapol-ptk-tx-no-reply"
    assert gates.wifi_phase == "cyw43-sdio-owner-wait-begin"


def test_gate_summary_refines_host_eapol_ptk_poll_timeout_after_split_tx() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_DRAIN contract=cyw43455 "
            "stage=m4-before-wsec result=credit-observed tx_result=0x0000008f "
            "polls=1 observed_control=0",
            "CYW43_DRIVER_TASK_CONTROL_REQUEST contract=cyw43455 "
            "stage=cyw43-host-eapol-ptk cmd=263 cmd_hex=0x00000107 "
            "id=39 runtime_flags=0x000a bcdc_flags=0x0002 payload_len=189 "
            "response_len=0 iovar=wsec_key header_mode=extended",
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-host-eapol-ptk event=tx-complete poll=0 "
            "flags=0x000a code=2 detail=0x00d1 result=0x000000d1 "
            "cmd=263 id=39 header=extended response_len=0 iovar=wsec_key "
            "nonmatching_frames=0 malformed_frames=0",
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-host-eapol-ptk event=wsec-key-commandless-stale "
            "poll=1 flags=0x0008 code=2 detail=0x00d1 result=0xfffffffe "
            "cmd=263 id=39 header=extended response_len=0 iovar=wsec_key "
            "nonmatching_frames=1 malformed_frames=0",
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-host-eapol-ptk event=cyw43-control-reply-nonmatching "
            "poll=10970 flags=0x000a code=3 detail=0x570e result=0x03005030 "
            "cmd=263 id=39 header=extended response_len=0 iovar=wsec_key "
            "nonmatching_frames=1 malformed_frames=0",
            "CYW43_DRIVER_TASK_COMMAND_NO_REPLY contract=cyw43455 "
            "stage=cyw43-host-eapol-ptk op=11 flags=0x000a "
            "target=0x00000000 payload_off=0 payload_len=209 total_len=209 "
            "iovar=wsec_key reason=cyw43-runtime-command-no-reply request=1590 "
            "progress_phase=142 progress_phase_name=cyw43-sdio-owner-wait-begin",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-host-eapol-ptk status=poll-timeout acceptance=no "
            "blocker=cyw43-host-eapol-ptk-poll-timeout "
            "progress_phase=142 progress_phase_name=cyw43-sdio-owner-wait-begin",
            "CYW43_DRIVER_TASK_HOST_EAPOL_KEY contract=cyw43455 "
            "kind=ptk stage=cyw43-host-eapol-ptk status=failed",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=host-eapol-ptk-install polls=24576 starts=0 "
            "tx_retries=0 data_rx=3 eapol_rx=3 non_eapol_rx=0 event_rx=3 "
            "control_rx=0 empty_polls=24576 associated=yes link_up=no "
            "next_action=inspect-host-eapol-error",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-host-eapol-ptk-poll-timeout"
    assert gates.wifi_exact == "cyw43-host-eapol-ptk-poll-timeout"
    assert gates.wifi_phase == "cyw43-sdio-owner-wait-begin"


def test_gate_summary_clears_key_timeout_after_retry_matched_reply() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-host-eapol-post-secure-gtk status=poll-timeout "
            "acceptance=no "
            "blocker=cyw43-host-eapol-post-secure-gtk-poll-timeout "
            "progress_phase=142 "
            "progress_phase_name=cyw43-sdio-owner-wait-begin",
            "CYW43_DRIVER_TASK_HOST_EAPOL_KEY_RETRY contract=cyw43455 "
            "stage=cyw43-host-eapol-post-secure-gtk iovar=wsec_key "
            "action=resubmit reason=cyw43-control-exchange",
            "CYW43_DRIVER_TASK_CONTROL_REPLY contract=cyw43455 "
            "stage=cyw43-host-eapol-post-secure-gtk event=matched-reply "
            "poll=1 flags=0x0004 cmd=263 id=50 status=0x00000000 "
            "response_len=173 iovar=wsec_key reply_match=yes",
            "CYW43_DRIVER_TASK_HOST_EAPOL_KEY contract=cyw43455 kind=gtk "
            "stage=cyw43-host-eapol-post-secure-gtk status=ready",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=secure reason=none polls=8197 data_rx=6 eapol_rx=6 "
            "non_eapol_rx=0 event_rx=2 control_rx=0 associated=yes "
            "link_up=yes next_action=release-dhcp-data",
            "[dhcp] tx queued xid=0x12345678",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 8
    assert gates.wifi_blocker == "dhcp-pending"
    assert gates.wifi_exact != "cyw43-host-eapol-post-secure-gtk-poll-timeout"


def test_gate_summary_maps_cyw43_deauth_reason_to_link_down() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=secure reason=none polls=8197 data_rx=6 eapol_rx=6 "
            "non_eapol_rx=0 event_rx=2 control_rx=0 associated=yes "
            "link_up=yes next_action=release-dhcp-data",
            "CYW43_DRIVER_TASK_EVENT_RX contract=cyw43455 stage=data-path "
            "flags=0x6301 len=80 event_type=6 status=0x00000000 "
            "reason=0x00000002 auth=0x00000000 label=deauth-ind retained=yes",
            "netstats: mode=dhcp policy=wifi active=wifi standby=none "
            "addr_src=wifi-link-down ip=0.0.0.0 gateway=0.0.0.0 "
            "dhcp=link-down",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 8
    assert gates.wifi_blocker == "wifi-link-down"
    assert gates.wifi_exact == "wifi-link-down"


def test_gate_summary_clears_key_timeout_after_key_ready_marker() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-host-eapol-gtk status=poll-timeout acceptance=no "
            "blocker=cyw43-host-eapol-gtk-poll-timeout",
            "CYW43_DRIVER_TASK_HOST_EAPOL_KEY_RETRY contract=cyw43455 "
            "stage=cyw43-host-eapol-gtk iovar=wsec_key action=accept-after-tx",
            "CYW43_DRIVER_TASK_HOST_EAPOL_KEY contract=cyw43455 kind=gtk "
            "stage=cyw43-host-eapol-gtk status=ready",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=secure reason=none polls=8197 data_rx=6 eapol_rx=6 "
            "non_eapol_rx=0 event_rx=2 control_rx=0 associated=yes "
            "link_up=yes next_action=release-dhcp-data",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 8
    assert gates.wifi_blocker == "none"
    assert gates.wifi_exact == "none"


def test_normalize_wifi_zero_status_is_not_a_blocker() -> None:
    for value in ("0", "0x0", "0x00", "0x0000", "0x00000000"):
        assert normalizer.normalize_wifi_blocker(value) == "none"
        assert normalizer.normalize_wifi_exact(value) == "none"


def test_gate_summary_names_firstread_source_asserted_empty() -> None:
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
            "control_rx_firstread_failed=0 last_rx_idle_detail=0x570e "
            "last_rx_idle_result=0xab070200 last_control_rx_idle_detail=0x570e "
            "last_control_rx_idle_result=0xab070200 "
            "rxsrc_mode=owner-card-sampled rxsrc_probe_len=512 rxsrc_ien=0x07 "
            "rxsrc_frame_ind=yes rxsrc_host_int=yes rxsrc_card_int=no "
            "rxsrc_f2_ready=yes control_rxsrc_mode=owner-card-sampled "
            "control_rxsrc_probe_len=512 control_rxsrc_ien=0x07 "
            "control_rxsrc_frame_ind=yes control_rxsrc_host_int=yes "
            "control_rxsrc_card_int=no control_rxsrc_f2_ready=yes "
            "last_flags=0x0000 last_len=0 last_ethertype=0x0000 "
            "last_ethertype_valid=no next_action=inspect-cyw43-rx-source-latch",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-firstread-source-asserted-empty"
    assert gates.wifi_exact == "cyw43-data-rx-firstread-source-asserted-empty"
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_preserves_source_asserted_empty_over_association_gap() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-event-missing polls=24576 "
            "starts=0 tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 "
            "event_rx=0 control_rx=0 empty_polls=24576 associated=no "
            "link_up=no assoc_event=none assoc_poll=0 post_assoc_polls=0 "
            "assoc_set_ssid_rescue=no rx_firstread_attempts=24576 "
            "rx_firstread_empty=24576 rx_firstread_invalid=0 "
            "rx_firstread_failed=0 rx_firstread_remainder_failed=0 "
            "rx_firstread_decode_miss=0 control_rx_firstread_attempts=24576 "
            "control_rx_firstread_empty=24576 control_rx_firstread_failed=0 "
            "last_rx_idle_detail=0x570e last_rx_idle_result=0xab070200 "
            "last_control_rx_idle_detail=0x570e "
            "last_control_rx_idle_result=0xab070200 "
            "rxsrc_mode=owner-card-sampled rxsrc_probe_len=512 rxsrc_ien=0x07 "
            "rxsrc_frame_ind=yes rxsrc_host_int=yes rxsrc_card_int=no "
            "rxsrc_f2_ready=yes control_rxsrc_mode=owner-card-sampled "
            "control_rxsrc_probe_len=512 control_rxsrc_ien=0x07 "
            "control_rxsrc_frame_ind=yes control_rxsrc_host_int=yes "
            "control_rxsrc_card_int=no control_rxsrc_f2_ready=yes "
            "last_flags=0x0000 last_len=0 last_ethertype=0x0000 "
            "last_ethertype_valid=no "
            "next_action=inspect-cyw43-association-event-or-join-policy",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-firstread-source-asserted-empty"
    assert gates.wifi_exact == "cyw43-data-rx-firstread-source-asserted-empty"
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_names_association_event_missing_without_rx_source_proof() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-event-missing polls=24576 "
            "starts=0 tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 "
            "event_rx=0 control_rx=0 empty_polls=24576 associated=no "
            "link_up=no assoc_event=none assoc_poll=0 post_assoc_polls=0 "
            "assoc_set_ssid_rescue=no rx_firstread_attempts=0 "
            "rx_firstread_empty=0 rx_firstread_invalid=0 rx_firstread_failed=0 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "control_rx_firstread_attempts=0 control_rx_firstread_empty=0 "
            "control_rx_firstread_failed=0 last_rx_idle_detail=0x0000 "
            "last_rx_idle_result=0x00000000 last_control_rx_idle_detail=0x0000 "
            "last_control_rx_idle_result=0x00000000 "
            "last_flags=0x0000 last_len=0 last_ethertype=0x0000 "
            "last_ethertype_valid=no "
            "next_action=inspect-cyw43-association-event-or-join-policy",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-association-event-missing"
    assert gates.wifi_exact == "cyw43-association-event-missing"
    assert gates.wifi_phase == "association"


def test_gate_summary_names_host_eapol_retransmit_ack_timeout() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-event-missing polls=24576 "
            "starts=0 tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 "
            "event_rx=0 control_rx=0 empty_polls=24576 associated=no "
            "link_up=no assoc_event=none assoc_poll=0 post_assoc_polls=0 "
            "assoc_set_ssid_rescue=no rx_firstread_attempts=9 "
            "rx_firstread_empty=0 rx_firstread_invalid=0 rx_firstread_failed=9 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "control_rx_firstread_attempts=9 control_rx_firstread_empty=1 "
            "control_rx_firstread_failed=8 last_rx_idle_detail=0x5709 "
            "last_rx_idle_result=0x00000000 last_control_rx_idle_detail=0x5709 "
            "last_control_rx_idle_result=0x00000000 "
            "rxsrc_mode=unreported rxsrc_probe_len=0 rxsrc_ien=0x00 "
            "rxsrc_frame_ind=no rxsrc_host_int=no rxsrc_card_int=no "
            "rxsrc_f2_ready=no control_rxsrc_mode=unreported "
            "control_rxsrc_probe_len=0 control_rxsrc_ien=0x00 "
            "control_rxsrc_frame_ind=no control_rxsrc_host_int=no "
            "control_rxsrc_card_int=no control_rxsrc_f2_ready=no "
            "rxtrace_valid=yes rxtrace_flags=0x0010 rxtrace_detail=0x5709 "
            "rxtrace_probe_len=0 rxtrace_source=0x00000000 "
            "rxtrace_prefix=0x00000000 rxtrace_digest=0x00000000 "
            "rxtrace_rframe=0x0000 rxtrace_firstread_reads=0 "
            "rxtrace_block_reads=0 rxtrace_rframe_reads=1 "
            "rxtrace_request_len=0 rxtrace_block_size=0 rxtrace_block_count=0 "
            "control_rxtrace_valid=yes control_rxtrace_flags=0x0010 "
            "control_rxtrace_detail=0x5709 control_rxtrace_probe_len=0 "
            "control_rxtrace_source=0x00000000 control_rxtrace_prefix=0x00000000 "
            "control_rxtrace_digest=0x00000000 control_rxtrace_rframe=0x0000 "
            "control_rxtrace_firstread_reads=0 control_rxtrace_block_reads=0 "
            "control_rxtrace_rframe_reads=1 control_rxtrace_request_len=0 "
            "control_rxtrace_block_size=0 control_rxtrace_block_count=0 "
            "last_flags=0x0000 last_len=0 last_ethertype=0x0000 "
            "last_ethertype_valid=no "
            "next_action=inspect-cyw43-data-rx-cmd53-firstread",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-retransmit-ack-timeout"
    assert gates.wifi_exact == "cyw43-data-rx-retransmit-ack-timeout"
    assert gates.wifi_phase == "runtime-rx"


def test_host_eapol_rxtrace_blocker_names_queue_and_copy_failures() -> None:
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker({"rxtrace_flags": "0x0400"})
        == "cyw43-data-rx-ring-copy-failed"
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker({"rxtrace_flags": "0x00c0"})
        == "cyw43-data-rx-queue-full"
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker(
            {"control_rxtrace_flags": "0x0240"}
        )
        == "cyw43-control-rx-queue-invalid-flags"
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker({"rxtrace_flags": "0x0010"})
        == "cyw43-data-rx-retransmit-ack-timeout"
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker({"rxtrace_flags": "0x1830"})
        is None
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker({"rxtrace_flags": "0xd010"})
        is None
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker(
            {
                "rxtrace_flags": "0x0010",
                "rxtrace_retx_sample": "0x0121",
                "rxtrace_retx_action": "block",
            }
        )
        == "cyw43-data-rx-retransmit-live-source-rframe-unavailable"
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker(
            {
                "control_rxtrace_flags": "0x0010",
                "control_rxtrace_retx_sample": "0x0112",
                "control_rxtrace_retx_action": "clear-stale",
            }
        )
        is None
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker(
            {
                "rxtrace_flags": "0x0010",
                "rxtrace_retx_sample": "0x0125",
                "rxtrace_retx_action": "read-source-asserted",
            }
        )
        is None
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker(
            {
                "rxtrace_flags": "0x0010",
                "rxtrace_retx_sample": "0x0125",
            }
        )
        is None
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker(
            {
                "rxtrace_detail": "0x5709",
                "rxtrace_request_len": "64",
                "rxtrace_cmd53_arg": "0x11000040",
                "rxtrace_cmd53_fn": "1",
                "rxtrace_cmd53_addr": "0x08000",
                "rxtrace_cmd53_write": "no",
                "rxtrace_cmd53_mode": "byte",
                "rxtrace_cmd53_count": "64",
            }
        )
        == "cyw43-data-rx-cmd53-function-mismatch"
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker(
            {
                "rxtrace_detail": "0x5709",
                "rxtrace_request_len": "64",
                "rxtrace_cmd53_arg": "0x21000020",
                "rxtrace_cmd53_fn": "2",
                "rxtrace_cmd53_addr": "0x08000",
                "rxtrace_cmd53_write": "no",
                "rxtrace_cmd53_mode": "byte",
                "rxtrace_cmd53_count": "32",
            }
        )
        == "cyw43-data-rx-firstread-cmd53-count-mismatch"
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker(
            {
                "control_rxtrace_detail": "0x5709",
                "control_rxtrace_request_len": "64",
                "control_rxtrace_cmd53_arg": "0x29000001",
                "control_rxtrace_cmd53_fn": "2",
                "control_rxtrace_cmd53_addr": "0x08000",
                "control_rxtrace_cmd53_write": "no",
                "control_rxtrace_cmd53_mode": "block",
                "control_rxtrace_cmd53_count": "1",
            }
        )
        == "cyw43-control-rx-firstread-cmd53-block-mode"
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker(
            {
                "rxtrace_detail": "0x5709",
                "rxtrace_request_len": "64",
                "rxtrace_cmd53_arg": "0x21000040",
                "rxtrace_cmd53_fn": "2",
                "rxtrace_cmd53_addr": "0x08000",
                "rxtrace_cmd53_write": "no",
                "rxtrace_cmd53_mode": "byte",
                "rxtrace_cmd53_count": "64",
                "rxtrace_transfer_result": "32",
            }
        )
        == "cyw43-data-rx-firstread-short-read"
    )
    assert (
        normalizer.cyw43_host_eapol_rxtrace_blocker(
            {
                "rxtrace_detail": "0x5709",
                "rxtrace_request_len": "64",
                "rxtrace_cmd53_arg": "0x21000040",
                "rxtrace_cmd53_fn": "2",
                "rxtrace_cmd53_addr": "0x08000",
                "rxtrace_cmd53_write": "no",
                "rxtrace_cmd53_mode": "byte",
                "rxtrace_cmd53_count": "64",
                "rxtrace_transfer_result": "0",
                "rxtrace_payload_after": "0x00000000",
            }
        )
        == "cyw43-data-rx-firstread-transfer-no-result"
    )


def test_gate_summary_names_host_eapol_rx_cmd53_shape_failure() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-event-missing "
            "polls=24576 starts=0 data_rx=0 eapol_rx=0 event_rx=0 "
            "associated=no link_up=no assoc_event=none "
            "last_rx_idle_detail=0x5709 rxtrace_valid=yes "
            "rxtrace_flags=0x0000 rxtrace_detail=0x5709 "
            "rxtrace_request_len=64 rxtrace_cmd53_arg=0x21000040 "
            "rxtrace_cmd53_fn=2 rxtrace_cmd53_addr=0x08000 "
            "rxtrace_cmd53_write=no rxtrace_cmd53_mode=byte "
            "rxtrace_cmd53_inc=no rxtrace_cmd53_count=64 "
            "rxtrace_transfer_result=0x00000000 "
            "rxtrace_payload_after=0x00000000 "
            "control_rxtrace_valid=yes control_rxtrace_flags=0x0000 "
            "next_action=inspect-cyw43-data-rx-cmd53-firstread",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-firstread-transfer-no-result"
    assert gates.wifi_exact == "cyw43-data-rx-firstread-transfer-no-result"
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_names_host_eapol_rx_sdio_owner_fault() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_SDIO_OWNER_FAULT contract=cyw43455 "
            "stage=cyw43-host-eapol-rx op=8 cmd=53 arg=0x21000040 "
            "fn=2 win=0x08000 target=0x00000000 effective=0x00000000 "
            "chunk_off=0 payload_off=0 inc=no write=no mode=byte "
            "len=64 blksz=64 blkcnt=0 tm=0x0010 host=0x06 power=0x0f "
            "clock=0x5007 present=0x01ff0506 int=0x00000010 "
            "resp0=0x00000800 blkreg=0x00000040 detail=0x5103 "
            "reason=sdio-descriptor-transfer-failed xfer_stage=response "
            "xfer_status=0x000800 xfer_reason=sdio-r5-response "
            "r5=0x0800 owner_window=function2-fifo retry=byte",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-sdio-owner-sdio-r5-response"
    assert gates.wifi_exact == "cyw43-data-rx-sdio-owner-sdio-r5-response"
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_names_retransmit_sample_blocker() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-event-missing "
            "polls=24576 starts=0 data_rx=0 eapol_rx=0 event_rx=0 "
            "associated=no link_up=no assoc_event=none "
            "last_rx_idle_detail=0x5709 rxtrace_valid=yes "
            "rxtrace_flags=0x0010 rxtrace_retx_sample=0x0121 "
            "rxtrace_retx_action=block control_rxtrace_valid=yes "
            "control_rxtrace_flags=0x0000 control_rxtrace_retx_sample=0x0000 "
            "control_rxtrace_retx_action=none "
            "next_action=inspect-cyw43-data-rx-cmd53-firstread",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert (
        gates.wifi_blocker
        == "cyw43-data-rx-retransmit-live-source-rframe-unavailable"
    )
    assert (
        gates.wifi_exact
        == "cyw43-data-rx-retransmit-live-source-rframe-unavailable"
    )
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_names_host_eapol_rx_queue_full() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-event-missing "
            "polls=24576 starts=0 data_rx=0 eapol_rx=0 event_rx=0 "
            "associated=no link_up=no assoc_event=none "
            "last_rx_idle_detail=0x5709 rxtrace_valid=yes "
            "rxtrace_flags=0x00c0 control_rxtrace_valid=yes "
            "control_rxtrace_flags=0x0000 "
            "next_action=inspect-cyw43-data-rx-cmd53-firstread",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-queue-full"
    assert gates.wifi_exact == "cyw43-data-rx-queue-full"
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_keeps_asserted_zero_retry_firstread_blocker() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-event-missing "
            "polls=24576 starts=0 data_rx=0 eapol_rx=0 event_rx=0 "
            "associated=no link_up=no assoc_event=none "
            "last_rx_idle_detail=0x570e rxtrace_valid=yes "
            "rxtrace_flags=0xd010 control_rxtrace_valid=yes "
            "control_rxtrace_flags=0xd010 "
            "next_action=inspect-cyw43-assoc-event-rx-or-sdio-owner-ienx-snapshot",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-firstread-source-asserted-empty"
    assert gates.wifi_exact == "cyw43-data-rx-firstread-source-asserted-empty"
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_preserves_v3_rxtrace_source_asserted_from_detail_line() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-not-associated "
            "polls=24576 starts=0 data_rx=0 eapol_rx=0 event_rx=0 "
            "associated=no link_up=no assoc_event=none assoc_probe=not-associated "
            "assoc_probe_result=0xffffffef assoc_set_ssid_rescue=yes "
            "firstread_class=preassoc-cadence-empty rx_firstread_attempts=4 "
            "rx_firstread_empty=4 last_rx_idle_detail=0x570a "
            "last_rx_idle_result=0xa8070040 next_action=inspect-cyw43-association-event-after-set-ssid-rescue",
            "CYW43_DRIVER_TASK_HOST_EAPOL_RXTRACE contract=cyw43455 lane=data "
            "source_flags=0x0042 pre_source=0xa8070040 post_source=0x88070040 "
            "pre_fresh=yes pre_asserted=yes pre_failed=no post_fresh=yes "
            "post_asserted=no post_failed=no source_asserted_ever=yes "
            "pre_int=0x00000000 post_int=0x00000000 pre_sdhci=0x00000000 "
            "post_sdhci=0x00000000 first_nonzero=none first_nonzero_off=65535 "
            "first_nonzero_byte=0x00 fifo_window_req=0x18000000 "
            "fifo_window_programmed=0x18000000 fifo_window_readback=0x18000000 "
            "fifo_window_flags=0x0007 fifo_window_ok=yes source_empty_polls=19",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-firstread-source-asserted-empty"
    assert gates.wifi_exact == "cyw43-data-rx-firstread-source-asserted-empty"
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_preserves_v3_rxtrace_shape_from_detail_line() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-not-associated "
            "polls=24576 starts=0 data_rx=0 eapol_rx=0 event_rx=0 "
            "associated=no link_up=no assoc_event=none assoc_probe=not-associated "
            "assoc_probe_result=0xffffffef assoc_set_ssid_rescue=yes "
            "firstread_class=preassoc-cadence-empty rx_firstread_attempts=4 "
            "rx_firstread_empty=4 last_rx_idle_detail=0x5709 "
            "last_rx_idle_result=0x00000000 next_action=inspect-cyw43-association-event-after-set-ssid-rescue",
            "CYW43_DRIVER_TASK_HOST_EAPOL_RXTRACE contract=cyw43455 lane=data "
            "flags=0x0000 detail=0x5709 request_len=64 "
            "cmd53_arg=0x21000040 cmd53_fn=2 cmd53_addr=0x08000 "
            "cmd53_write=no cmd53_mode=byte cmd53_inc=no cmd53_count=64 "
            "transfer_result=0x00000000 payload_after=0x00000000 "
            "irq_preserve_count=2 irq_preserve_reason=2 "
            "irq_preserve_int=0x20000040 irq_preserve_ack=0x20000000 "
            "irq_episode_preserves=1 irq_episode_rearms=4 "
            "trace_seq=17 start_ticks_lo=0x12345678 "
            "pre_sample_delta_ticks=12 transfer_delta_ticks=345 "
            "post_sample_delta_ticks=456",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-firstread-transfer-no-result"
    assert gates.wifi_exact == "cyw43-data-rx-firstread-transfer-no-result"
    assert gates.wifi_phase == "runtime-rx"
    assert gates.wifi_rx_irq_preserve_count == 2
    assert gates.wifi_rx_irq_preserve_reason == "rframe-pending"
    assert gates.wifi_rx_irq_episode_preserves == 1
    assert gates.wifi_rx_irq_episode_rearms == 4
    assert gates.wifi_rxtrace_seq == 17
    assert gates.wifi_rxtrace_start_ticks == 0x12345678
    assert gates.wifi_rxtrace_pre_sample_delta_ticks == 12
    assert gates.wifi_rxtrace_transfer_delta_ticks == 345
    assert gates.wifi_rxtrace_post_sample_delta_ticks == 456


def test_gate_summary_labels_deprecated_cyw43_rx_irq_source_asserted_preserve_reason() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_RXTRACE contract=cyw43455 lane=data "
            "flags=0x0000 detail=0x5709 request_len=64 "
            "cmd53_arg=0x21000040 cmd53_fn=2 cmd53_addr=0x08000 "
            "cmd53_write=no cmd53_mode=byte cmd53_inc=no cmd53_count=64 "
            "transfer_result=0x00000000 payload_after=0x00000000 "
            "irq_preserve_count=1 irq_preserve_reason=5 "
            "irq_preserve_int=0x20000040 irq_preserve_ack=0x20000000",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_rx_irq_preserve_count == 1
    assert gates.wifi_rx_irq_preserve_reason == "deprecated-source-asserted"


def test_cli_suppresses_broken_pipe(monkeypatch) -> None:
    """Piped consumers such as head must not turn valid output into a traceback."""

    class BrokenPipeStdout:
        closed = False

        def write(self, _value: str) -> int:
            raise BrokenPipeError()

        def flush(self) -> None:
            pass

        def close(self) -> None:
            self.closed = True

    stdout = BrokenPipeStdout()
    monkeypatch.setattr(normalizer.sys, "stdin", io.StringIO("U-Boot 2026.01\n"))
    monkeypatch.setattr(normalizer.sys, "stdout", stdout)

    assert normalizer.run_cli(["-", "--boot-summary"]) == 0
    assert stdout.closed


def test_gate_summary_names_bssid_probe_tx_submit_fail_over_firstread_idle() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-host-eapol-bssid-probe status=tx-submit-fail "
            "acceptance=no code=5 detail=20739 result=83888128 "
            "frame_len=0 blocker=cyw43-host-eapol-bssid-probe-tx-submit-fail",
            "CYW43_DRIVER_TASK_HOST_EAPOL_ASSOC_PROBE contract=cyw43455 "
            "poll=20481 attempt=4 status=failed bssid=00:00:00:00:00:00 "
            "reason=control-error-limit result=0x05000800",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=host-eapol-required polls=20481 starts=0 "
            "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 event_rx=0 "
            "control_rx=0 empty_polls=20481 associated=no link_up=no "
            "assoc_event=none assoc_poll=0 post_assoc_polls=0 "
            "assoc_set_ssid_rescue=no rx_firstread_attempts=9397 "
            "rx_firstread_empty=0 rx_firstread_invalid=0 rx_firstread_failed=9397 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "control_rx_firstread_attempts=9396 "
            "control_rx_firstread_empty=0 control_rx_firstread_failed=9396 "
            "last_rx_idle_detail=0x5706 last_rx_idle_result=0xab070200 "
            "last_control_rx_idle_detail=0x5706 "
            "last_control_rx_idle_result=0xab070200 "
            "last_flags=0x0000 last_len=0 last_ethertype=0x0000 "
            "last_ethertype_valid=no "
            "next_action=inspect-cyw43-data-rx-cmd53-firstread",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-host-eapol-bssid-probe-tx-submit-fail"
    assert gates.wifi_exact == "cyw43-host-eapol-bssid-probe-tx-submit-fail"
    assert gates.wifi_phase == "control-tx"


def test_gate_summary_names_post_rescue_association_gap_over_firstread_empty() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_ASSOC_RESCUE contract=cyw43455 "
            "poll=20481 attempt=4 status=ready "
            "reason=firmware-not-associated-limit action=set-ssid result=0x00000000",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=host-eapol-required polls=32768 starts=0 "
            "tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 event_rx=0 "
            "control_rx=0 empty_polls=32768 associated=no link_up=no "
            "assoc_event=none assoc_poll=0 post_assoc_polls=0 "
            "assoc_set_ssid_rescue=yes rx_firstread_attempts=7902 "
            "rx_firstread_empty=7902 rx_firstread_invalid=0 rx_firstread_failed=0 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "control_rx_firstread_attempts=32762 control_rx_firstread_empty=32762 "
            "control_rx_firstread_failed=0 last_rx_idle_detail=0x570a "
            "last_rx_idle_result=0xab070200 last_control_rx_idle_detail=0x570a "
            "last_control_rx_idle_result=0xab070200 "
            "rxsrc_mode=owner-card-sampled-cached rxsrc_probe_len=512 "
            "rxsrc_ien=0x07 rxsrc_frame_ind=yes rxsrc_host_int=yes "
            "rxsrc_card_int=no rxsrc_f2_ready=yes "
            "control_rxsrc_mode=owner-card-sampled-cached "
            "control_rxsrc_probe_len=512 control_rxsrc_ien=0x07 "
            "control_rxsrc_frame_ind=yes control_rxsrc_host_int=yes "
            "control_rxsrc_card_int=no control_rxsrc_f2_ready=yes "
            "last_flags=0x0000 last_len=0 last_ethertype=0x0000 "
            "last_ethertype_valid=no "
            "next_action=inspect-cyw43-association-event-after-set-ssid-rescue",
            "netstatus: ip=0.0.0.0 gateway=0.0.0.0 "
            "src=wifi-host-eapol-required dhcp=host-eapol-required",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-association-not-associated"
    assert gates.wifi_exact == "cyw43-association-not-associated"
    assert gates.wifi_phase == "association"


def test_gate_summary_names_post_rescue_source_asserted_empty() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_ASSOC_RESCUE contract=cyw43455 "
            "poll=20481 attempt=4 status=ready "
            "reason=firmware-not-associated-limit action=set-ssid "
            "result=0x00000000",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-not-associated "
            "polls=45056 starts=0 tx_retries=0 data_rx=0 eapol_rx=0 "
            "non_eapol_rx=0 event_rx=0 control_rx=0 empty_polls=45056 "
            "associated=no link_up=no assoc_event=none assoc_poll=0 "
            "post_assoc_polls=0 assoc_set_ssid_rescue=yes "
            "rx_firstread_attempts=240 rx_firstread_empty=240 "
            "rx_firstread_invalid=0 rx_firstread_failed=0 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "control_rx_firstread_attempts=45050 "
            "control_rx_firstread_empty=45050 control_rx_firstread_failed=0 "
            "last_rx_idle_detail=0x570e last_rx_idle_result=0x8b070200 "
            "last_control_rx_idle_detail=0x570e "
            "last_control_rx_idle_result=0x8b070200 "
            "rxsrc_mode=owner-card-sampled rxsrc_probe_len=512 "
            "rxsrc_ien=0x07 rxsrc_frame_ind=yes rxsrc_host_int=yes "
            "rxsrc_card_int=no rxsrc_f2_ready=yes "
            "control_rxsrc_mode=owner-card-sampled "
            "control_rxsrc_probe_len=512 control_rxsrc_ien=0x07 "
            "control_rxsrc_frame_ind=yes control_rxsrc_host_int=yes "
            "control_rxsrc_card_int=no control_rxsrc_f2_ready=yes "
            "last_flags=0x0000 last_len=0 last_ethertype=0x0000 "
            "last_ethertype_valid=no "
            "next_action=inspect-cyw43-association-event-after-set-ssid-rescue",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-data-rx-firstread-source-asserted-empty"
    assert gates.wifi_exact == "cyw43-data-rx-firstread-source-asserted-empty"
    assert gates.wifi_phase == "runtime-rx"


def test_gate_summary_preserves_not_associated_probe_over_firstread_empty() -> None:
    events = normalizer.parse_events(
        [
            "wifi: sdio linked_runtime_progress marker_valid=yes sequence=0 "
            "phase=202 phase_name=runtime-poll-ready aux0=0x00000007 "
            "gate=0 blocker=sdio-linked-runtime-progress-no-reply "
            "next_action=inspect-linked-sdio-runtime-progress",
            "wifi: gate 8 name=host-eapol status=fail "
            "evidence=exact=wifi-host-eapol-pending control_stage=host-eapol "
            "dependency=ready-for-direct-evidence next=dhcp-bound",
            "CYW43_DRIVER_TASK_HOST_EAPOL_ASSOC_PROBE contract=cyw43455 "
            "poll=24576 attempt=4 status=failed bssid=00:00:00:00:00:00 "
            "reason=firmware-not-associated-limit result=0xffffffef",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-not-associated polls=24576 "
            "starts=0 tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 "
            "event_rx=0 control_rx=0 empty_polls=24576 associated=no "
            "link_up=no assoc_event=none assoc_poll=0 post_assoc_polls=0 "
            "rx_firstread_attempts=13494 rx_firstread_empty=13494 "
            "rx_firstread_invalid=0 rx_firstread_failed=0 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "control_rx_firstread_attempts=24576 control_rx_firstread_empty=24576 "
            "control_rx_firstread_failed=0 last_rx_idle_detail=0x570a "
            "last_rx_idle_result=0xab070200 last_control_rx_idle_detail=0x570a "
            "last_control_rx_idle_result=0xab070200 rxsrc_mode=owner-card-sampled-cached "
            "rxsrc_probe_len=512 rxsrc_ien=0x07 rxsrc_frame_ind=yes "
            "rxsrc_host_int=yes rxsrc_card_int=no rxsrc_f2_ready=yes "
            "control_rxsrc_mode=owner-card-sampled-cached control_rxsrc_probe_len=512 "
            "control_rxsrc_ien=0x07 control_rxsrc_frame_ind=yes "
            "control_rxsrc_host_int=yes control_rxsrc_card_int=no "
            "control_rxsrc_f2_ready=yes last_flags=0x0000 last_len=0 "
            "last_ethertype=0x0000 last_ethertype_valid=no "
            "next_action=inspect-cyw43-association-event-or-join-policy",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-association-not-associated"
    assert gates.wifi_exact == "cyw43-association-not-associated"
    assert gates.wifi_phase == "association"


def test_gate_summary_names_not_associated_from_probe_status_fields() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-not-associated polls=8192 "
            "starts=0 tx_retries=0 data_rx=0 eapol_rx=0 non_eapol_rx=0 "
            "event_rx=0 control_rx=0 empty_polls=8192 associated=no "
            "link_up=no assoc_event=none assoc_poll=0 post_assoc_polls=0 "
            "assoc_probe=not-associated assoc_probe_result=0xffffffef "
            "assoc_set_ssid_rescue=no firstread_class=preassoc-cadence-empty "
            "rx_probe_poll=8192 rx_probe_flags=0x0004 "
            "control_rx_probe_poll=8192 control_rx_probe_flags=0x0004 "
            "rx_firstread_attempts=10 rx_firstread_empty=10 "
            "rx_firstread_invalid=0 rx_firstread_failed=0 "
            "rx_firstread_remainder_failed=0 rx_firstread_decode_miss=0 "
            "control_rx_firstread_attempts=10 control_rx_firstread_empty=10 "
            "control_rx_firstread_failed=0 last_rx_idle_detail=0x570a "
            "last_rx_idle_result=0xa8070040 last_control_rx_idle_detail=0x570a "
            "last_control_rx_idle_result=0xa8070040 "
            "rxsrc_mode=owner-card-sampled-cached rxsrc_probe_len=64 "
            "rxsrc_ien=0x07 rxsrc_frame_ind=no rxsrc_host_int=no "
            "rxsrc_card_int=no rxsrc_f2_ready=yes "
            "control_rxsrc_mode=owner-card-sampled-cached "
            "control_rxsrc_probe_len=64 control_rxsrc_ien=0x07 "
            "control_rxsrc_frame_ind=no control_rxsrc_host_int=no "
            "control_rxsrc_card_int=no control_rxsrc_f2_ready=yes "
            "last_flags=0x0000 last_len=0 last_ethertype=0x0000 "
            "last_ethertype_valid=no "
            "next_action=inspect-cyw43-association-event-or-join-policy",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-association-not-associated"
    assert gates.wifi_exact == "cyw43-association-not-associated"
    assert gates.wifi_phase == "association"


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


def test_gate_summary_keeps_terminal_passive_no_rframe_over_stale_progress() -> None:
    """The retained op11 result identifies the failing Gate 8 edge."""

    events = normalizer.parse_events(
        [
            "wifi: gate 8 name=firmware-channel status=fail "
            "evidence=exact=none dependency=ready-for-direct-evidence",
            "wifi: evidence cyw43 detail=0x530b "
            "reason=cyw43-control-exchange result=0x43030000 "
            "stage=cyw43-control-txglomalign op=11",
            "wifi: sdio progress_action sequence=2 "
            "blocker=sdio-linked-runtime-progress-no-reply "
            "next_action=inspect-linked-sdio-runtime-progress",
            "wifi: evidence cyw43 detail=0x530b "
            "reason=cyw43-control-exchange result=0x43030000 "
            "stage=cyw43-control-txglomalign op=11",
        ]
    )

    gates = normalizer.summarize_gates(events)

    # WIFI_GATE is the last proven gate; this exact timeout fails Gate 8.
    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "control-plane-reply-idle-loop"
    assert gates.wifi_exact == "cyw43-control-rx-no-rframe"
    assert gates.wifi_phase == "cyw43-control-txglomalign"
    assert gates.wifi_blocker_line == 4


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
    assert gates.wifi_blocker == "cyw43-runtime-command-no-reply"
    assert gates.wifi_exact == "cyw43-runtime-command-no-reply"
    assert gates.wifi_phase == "cyw43-control-exchange"
    assert gates.wifi_subgate == "none"
    assert gates.wifi_subgate_name == "none"


def test_gate_summary_keeps_causal_parent_no_reply_over_secondary_sdio_progress() -> None:
    """The explicit Gate 8 boundary outranks superseded recovery telemetry."""

    events = normalizer.parse_events(
        [
            "wifi: gate 8 name=firmware-channel status=fail "
            "evidence=exact=none dependency=ready-for-direct-evidence",
            "wifi: evidence cyw43 detail=0x0000 "
            "reason=cyw43-runtime-command-no-reply result=0x00000000 "
            "stage=cyw43-control-txglomalign op=11",
            "wifi: evidence boundary "
            "failure_domain=cyw43-runtime-command-no-reply "
            "direct_proof_gate=7 proof_gate=7 frontier_gate=7 "
            "failing_gate=8 target_gate=10",
            "wifi: cyw43 last_progress marker_valid=yes sequence=180 "
            "phase=144 phase_name=cyw43-sdio-owner-reply "
            "aux0=0x43595734 superseded=yes",
            "wifi: sdio progress_action sequence=180 "
            "blocker=sdio-linked-runtime-progress-no-reply "
            "next_action=inspect-linked-sdio-runtime-progress",
            "wifi: recovery fault detail=network-quarantined "
            "causal_preserved=yes",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-runtime-command-no-reply"
    assert gates.wifi_exact == "cyw43-runtime-command-no-reply"
    assert gates.wifi_phase == "cyw43-control-txglomalign"
    assert gates.wifi_blocker_line == 2
    assert gates.wifi_subgate == "none"
    assert gates.wifi_subgate_name == "none"


def test_gate_summary_does_not_mislabel_sdio_identity_fault_as_gate7_join() -> None:
    """A Gate 8 linked-SDIO fault is below the host-EAPOL sub-gate taxonomy."""

    events = normalizer.parse_events(
        [
            "wifi: gate 7 name=function2-ready status=pass "
            "evidence=f2_enabled=no dependency=ready-for-direct-evidence",
            "wifi: gate 8 name=control-plane-exact-error status=fail "
            "evidence=exact=sdio-request-identity-invalid "
            "control_stage=firmware-channel dependency=ready-for-direct-evidence",
            "wifi: evidence boundary proof=gate-frontier direct_proof_gate=7 "
            "proof_gate=7 frontier_gate=7 failing_gate=8 target_gate=10 "
            "failure_domain=sdio-request-identity-invalid",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "sdio-request-identity-invalid"
    assert gates.wifi_exact == "sdio-request-identity-invalid"
    assert gates.wifi_subgate == "none"
    assert gates.wifi_subgate_name == "none"
    assert gates.wifi_subgate_reason == "sdio-request-identity-invalid"


def test_gate_summary_reads_nested_gate_exact_over_later_recovery_progress() -> None:
    """The Gate 8 evidence field preserves the live association blocker."""

    events = normalizer.parse_events(
        [
            "wifi: gate 8 name=host-eapol status=fail "
            "evidence=exact=wifi-host-eapol-pending "
            "control_stage=host-eapol dependency=ready-for-direct-evidence",
            "wifi: evidence boundary proof=gate-frontier "
            "direct_proof_gate=7 inferred_frontier_gate=7 proof_gate=7 "
            "frontier_gate=7 failing_gate=8 target_gate=10 "
            "failure_domain=wifi-host-eapol-pending",
            "wifi: sdio linked_runtime_progress marker_valid=yes sequence=0 "
            "phase=202 phase_name=runtime-poll-ready aux0=0x00000007 "
            "gate=0 blocker=sdio-linked-runtime-progress-no-reply "
            "next_action=inspect-linked-sdio-runtime-progress",
            "[net-console] deferred failed detail=cyw43-pair-recovery-limit "
            "driver-task runtime init failed",
            "CYW43_BOOTSTRAP_SUPERVISOR attempt=5 status=exhausted "
            "backoff_ms=0 recovery=full",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "wifi-host-eapol-pending"
    assert gates.wifi_exact == "wifi-host-eapol-pending"
    assert gates.wifi_phase == "host-eapol"
    assert gates.wifi_blocker_line == 1


def test_gate_summary_keeps_control_tx_no_reply_below_tcp_gate() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-plane status=begin acceptance=no code=none "
            "detail=none result=none frame_len=0 owner=linked-runtime "
            "root_action=submit-turn blocker=cyw43-control-plane-begin",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=begin",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=begin acceptance=no "
            "blocker=cyw43-control-txglomalign-begin "
            "progress_marker_valid=yes progress_sequence=0 "
            "progress_phase=202 progress_phase_name=runtime-poll-ready "
            "progress_aux0=0x00000004",
            "CYW43_DRIVER_TASK_CONTROL_REQUEST contract=cyw43455 "
            "stage=cyw43-control-txglomalign cmd=263 cmd_hex=0x00000107 "
            "id=1 runtime_flags=0x0008 bcdc_flags=0x0002 payload_len=36 "
            "response_len=0 iovar=bus:txglomalign value=0x00000008",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=tx-no-reply "
            "acceptance=no blocker=cyw43-control-txglomalign-tx-no-reply "
            "active_request_valid=yes active_request=180 "
            "progress_marker_valid=yes progress_sequence=180 "
            "progress_phase=144 progress_phase_name=cyw43-sdio-owner-reply "
            "progress_aux0=0x43595734 progress_request_match=yes",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-plane status=failed acceptance=no "
            "blocker=cyw43-control-plane-failed "
            "progress_marker_valid=yes progress_sequence=180 "
            "progress_phase=144 progress_phase_name=cyw43-sdio-owner-reply "
            "progress_aux0=0x43595734 progress_request_match=yes",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "[net-console] deferred failed detail=cyw43-command-completion "
            "driver-task runtime init failed",
            "ERR NETSTATS reason=policy detail=net-disabled "
            "cause=cyw43-command-completion driver-task runtime init failed",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43-command-completion driver-task runtime init failed",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-control-tx-no-reply"
    assert gates.wifi_exact == "cyw43-control-tx-no-reply"
    assert gates.wifi_phase == "cyw43-control-txglomalign"
    assert gates.wifi_subgate == "none"
    assert gates.wifi_subgate_name == "none"


def test_gate_summary_keeps_control_tx_retry_no_reply_exact() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=begin acceptance=no",
            "CYW43_DRIVER_TASK_CONTROL_REQUEST contract=cyw43455 "
            "stage=cyw43-control-txglomalign cmd=263 cmd_hex=0x00000107 "
            "id=1 runtime_flags=0x0008 bcdc_flags=0x0002 payload_len=36 "
            "response_len=0 iovar=bus:txglomalign value=0x00000008",
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-control-txglomalign event=tx-complete "
            "poll=0 flags=0x0008 sequence=180 code=5 detail=0x5103 "
            "result=0x05000800 frame_off=768 frame_len=56 "
            "frame_flags=0x0000 expected_cmd=263 expected_cmd_hex=0x00000107 "
            "expected_id=1 header_mode=plain expected_response_len=0 "
            "iovar=bus:txglomalign nonmatching_frames=0 malformed_frames=0",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=tx-fault-sdio-owner-recover-ready "
            "acceptance=no code=5 detail=20739 result=83888128 frame_len=56",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=tx-retry-no-reply "
            "acceptance=no blocker=cyw43-control-txglomalign-tx-retry-no-reply "
            "active_request_valid=yes active_request=182 "
            "progress_marker_valid=yes progress_sequence=182 "
            "progress_phase=142 progress_phase_name=cyw43-sdio-owner-wait-begin "
            "progress_aux0=0x43595734 progress_request_match=yes",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-plane status=failed acceptance=no "
            "blocker=cyw43-control-plane-failed "
            "progress_marker_valid=yes progress_sequence=182 "
            "progress_phase=142 progress_phase_name=cyw43-sdio-owner-wait-begin "
            "progress_aux0=0x43595734 progress_request_match=yes",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "wifi: evidence boundary failure_domain=cyw43-control-tx-retry-no-reply "
            "direct_proof_gate=0 proof_gate=7 frontier_gate=7 failing_gate=8 "
            "target_gate=10",
            "wifi: next_action=inspect-cyw43-runtime-fault-stage "
            "blocker=cyw43-control-tx-retry-no-reply proof_gate=7 target_gate=10 "
            "source=hal-runtime-required",
            "ERR NETSTATS reason=policy detail=net-disabled "
            "cause=cyw43-command-completion driver-task runtime init failed",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43-command-completion driver-task runtime init failed",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-control-tx-retry-no-reply"
    assert gates.wifi_exact == "cyw43-control-tx-retry-no-reply"
    assert gates.wifi_phase == "cyw43-control-txglomalign"
    assert gates.wifi_subgate == "none"
    assert gates.wifi_subgate_name == "none"


def test_gate_summary_honors_wifi_diag_control_tx_failure_domain() -> None:
    events = normalizer.parse_events(
        [
            "wifi: driver-task replay failure detail=net-disabled "
            "cause=cyw43-command-completion driver-task runtime init failed",
            "wifi: cyw43 linked_runtime_progress marker_valid=yes sequence=180 "
            "phase=144 phase_name=cyw43-sdio-owner-reply aux0=0x43595734 "
            "gate=2 blocker=cyw43-sdio-owner-replied "
            "next_action=continue-cyw43-card-adoption",
            "wifi: gate 2 name=sdio-card-select status=inferred "
            "evidence=stage=cyw43-control-txglomalign detail=0x0000 "
            "result=0x00000000 next=cccr-fbr-ready",
            "wifi: evidence boundary failure_domain=cyw43-control-tx-no-reply "
            "direct_proof_gate=0 proof_gate=7 frontier_gate=7 failing_gate=8 "
            "target_gate=10",
            "wifi: next_action=inspect-cyw43-runtime-fault-stage "
            "blocker=cyw43-control-tx-no-reply proof_gate=7 target_gate=10 "
            "source=hal-runtime-required",
            "ERR NETTEST reason=policy detail=net-disabled "
            "cause=cyw43-command-completion driver-task runtime init failed",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-control-tx-no-reply"
    assert gates.wifi_exact == "cyw43-control-tx-no-reply"
    assert gates.wifi_phase == "none"


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


def test_cyw43_split_control_key_install_nonmatching_is_stage_specific() -> None:
    event = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-host-eapol-ptk "
            "event=cyw43-control-reply-nonmatching poll=10970 flags=0x000a "
            "sequence=42 code=3 detail=0x570e result=0x03005030 "
            "frame_off=0 frame_len=0 frame_flags=0x0000 "
            "expected_cmd=263 expected_cmd_hex=0x00000107 expected_id=39 "
            "header_mode=extended expected_response_len=0 iovar=wsec_key "
            "nonmatching_frames=1 malformed_frames=0",
        ]
    )[0]

    assert (
        normalizer.cyw43_control_split_event_exact(event)
        == "cyw43-host-eapol-ptk-reply-nonmatching"
    )


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
    assert gates.wifi_blocker == "cyw43-control-tx-not-submitted"
    assert gates.wifi_exact == "cyw43-control-tx-not-submitted"
    assert gates.wifi_phase == "cyw43-control-txglomalign"


def test_gate_summary_refines_cyw43_split_control_tx_sdio_descriptor_fault() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-control-firmware-version event=tx-complete "
            "poll=0 flags=0x0000 sequence=205 code=5 detail=0x5103 "
            "result=0x04208000 frame_off=768 frame_len=56 "
            "frame_flags=0x0000 expected_cmd=262 expected_cmd_hex=0x00000106 "
            "expected_id=12 header_mode=extended expected_response_len=128 "
            "iovar=ver nonmatching_frames=0 malformed_frames=0",
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-control-firmware-version event=tx-retry-complete "
            "poll=1 flags=0x0000 sequence=206 code=5 detail=0x5103 "
            "result=0x04208000 frame_off=768 frame_len=56 "
            "frame_flags=0x0000 expected_cmd=262 expected_cmd_hex=0x00000106 "
            "expected_id=12 header_mode=extended expected_response_len=128 "
            "iovar=ver nonmatching_frames=0 malformed_frames=0",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-sdio-descriptor-transfer-failed"
    assert gates.wifi_exact == "cyw43-sdio-descriptor-transfer-failed"
    assert gates.wifi_phase == "cyw43-control-firmware-version"


def test_gate_summary_keeps_cyw43_split_control_fault_after_nettest_error() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-control-firmware-version event=tx-retry-complete "
            "poll=1 flags=0x0000 sequence=206 code=5 detail=0x5103 "
            "result=0x04208000 frame_off=768 frame_len=56 "
            "frame_flags=0x0000 expected_cmd=262 expected_cmd_hex=0x00000106 "
            "expected_id=12 header_mode=extended expected_response_len=128 "
            "iovar=ver nonmatching_frames=0 malformed_frames=0",
            "ERR NETTEST detail=unsupported reason=not-ready",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-sdio-descriptor-transfer-failed"
    assert gates.wifi_exact == "cyw43-sdio-descriptor-transfer-failed"
    assert gates.wifi_phase == "cyw43-control-firmware-version"


def test_gate_summary_keeps_live_txglomalign_descriptor_fault_after_deferred_error() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=begin",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=begin acceptance=no",
            "CYW43_DRIVER_TASK_CONTROL_REQUEST contract=cyw43455 "
            "stage=cyw43-control-txglomalign cmd=263 id=1 "
            "iovar=bus:txglomalign value=0x00000008 header_mode=plain",
            "CYW43_DRIVER_TASK_CONTROL_SPLIT contract=cyw43455 "
            "stage=cyw43-control-txglomalign event=tx-complete "
            "poll=0 flags=0x0000 sequence=180 code=5 detail=0x5103 "
            "result=0x05000800 frame_off=768 frame_len=56 "
            "frame_flags=0x0000 expected_cmd=263 expected_cmd_hex=0x00000107 "
            "expected_id=1 header_mode=plain expected_response_len=0 "
            "iovar=bus:txglomalign nonmatching_frames=0 malformed_frames=0",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=tx-submit-fail "
            "acceptance=no code=5 detail=20739 result=83888128 frame_len=56 "
            "owner=linked-runtime blocker=cyw43-control-txglomalign-tx-submit-fail",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-plane status=failed blocker=cyw43-control-plane-failed",
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "wifi: gate 6 name=firmware-upload status=fail "
            "evidence=uploaded=no verified=no fault_detail=0x5103 "
            "next=function2-ready",
            "wifi: gate 9 name=dhcp-bound status=blocked "
            "evidence=active=none tcp_ready=no "
            "dependency=not-reached-due-to-gate-6",
            "[net-console] deferred failed detail=cyw43-command "
            "driver-task runtime init failed",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-sdio-descriptor-transfer-failed"
    assert gates.wifi_exact == "cyw43-sdio-descriptor-transfer-failed"
    assert gates.wifi_phase == "cyw43-control-txglomalign"
    assert gates.wifi_subgate == "none"


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


def test_gate_summary_refines_cyw43_txglomalign_commandless_badarg() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=begin acceptance=no",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-control-txglomalign op=11 flags=0x0008 "
            "target=0x00000000 payload_off=284 payload_len=36 total_len=36 "
            "detail=21259 reason=cyw43-control-exchange result=0xfffffffe",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=fail acceptance=no "
            "code=5 detail=21259 result=0xfffffffe frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-plane status=failed acceptance=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-control-txglomalign-badarg"
    assert gates.wifi_exact == "cyw43-control-txglomalign-badarg"
    assert gates.wifi_phase == "cyw43-control-txglomalign"


def test_gate_summary_refines_cyw43_txglomalign_commandless_unsupported() -> None:
    events = normalizer.parse_events(
        [
            "NET_DRIVER_TASK_REPLAY_STATUS role=cyw43-wifi selected=yes "
            "policy=wifi attempted=yes stage=cyw43-control-plane blocker=failed",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=begin acceptance=no",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-control-txglomalign op=11 flags=0x0008 "
            "target=0x00000000 payload_off=284 payload_len=36 total_len=36 "
            "detail=21259 reason=cyw43-control-exchange result=0xffffffe9",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=fail acceptance=no "
            "code=5 detail=21259 result=0xffffffe9 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-plane status=failed acceptance=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-control-txglomalign-unsupported"
    assert gates.wifi_exact == "cyw43-control-txglomalign-unsupported"
    assert gates.wifi_phase == "cyw43-control-txglomalign"


def test_unrelated_ready_cannot_resolve_cyw43_txglomalign_reject() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=begin acceptance=no",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-control-txglomalign op=11 flags=0x0008 "
            "target=0x00000000 payload_off=0 payload_len=36 total_len=36 "
            "detail=21259 reason=cyw43-control-exchange result=0xfffffffe",
            "DRIVER_TASK_RESOURCE_INIT contract=other hot_path=usb-hid "
            "stage=cyw43-control-txglomalign status=ready acceptance=no",
        ]
    )

    assert normalizer.summarize_cyw43_control_txglomalign_reject(events) == (
        "cyw43-control-txglomalign-badarg",
        "cyw43-control-txglomalign",
        2,
    )


def test_gate_summary_rejects_legacy_txglomalign_fallback() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=begin acceptance=no",
            "CYW43_DRIVER_TASK_COMMAND_FAULT contract=cyw43455 "
            "stage=cyw43-control-txglomalign op=11 flags=0x0008 "
            "target=0x00000000 payload_off=0 payload_len=36 total_len=36 "
            "detail=21259 reason=cyw43-control-exchange result=0xfffffffe",
            "CYW43_DRIVER_TASK_TXGLOMALIGN contract=cyw43455 "
            "stage=cyw43-control-txglomalign action=optional-badarg "
            "value=8 code=5 detail=0x530b result=0xfffffffe",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign status=optional-badarg "
            "acceptance=no code=5 detail=21259 result=0xfffffffe frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign-fallback4 status=begin acceptance=no",
            "CYW43_DRIVER_TASK_TXGLOMALIGN contract=cyw43455 "
            "stage=cyw43-control-txglomalign-fallback4 action=ready "
            "value=4 code=2 detail=0x0000 result=0x00000000",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign-fallback4 status=ready "
            "acceptance=no code=2 detail=0 result=0 frame_len=0",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-plane status=failed acceptance=no",
        ]
    )

    assert normalizer.summarize_cyw43_control_txglomalign_reject(events) == (
        "cyw43-control-txglomalign-legacy-value4",
        "cyw43-control-txglomalign-fallback4",
        7,
    )
    gates = normalizer.summarize_gates(events)
    assert gates.wifi_blocker == "cyw43-control-txglomalign-legacy-value4"
    assert gates.wifi_exact == "cyw43-control-txglomalign-legacy-value4"
    assert gates.wifi_phase == "cyw43-control-txglomalign-fallback4"


def test_gate_summary_rejects_fallback_only_txglomalign_value4() -> None:
    events = normalizer.parse_events(
        [
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign-fallback status=begin acceptance=no",
            "CYW43_DRIVER_TASK_TXGLOMALIGN contract=cyw43455 "
            "stage=cyw43-control-txglomalign-fallback action=ready "
            "value=4 code=2 detail=0 result=0",
            "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 hot_path=cyw43-wifi "
            "stage=cyw43-control-txglomalign-fallback status=ready acceptance=no",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_blocker == "cyw43-control-txglomalign-legacy-value4"
    assert gates.wifi_exact == "cyw43-control-txglomalign-legacy-value4"
    assert gates.wifi_phase == "cyw43-control-txglomalign-fallback"


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
            "source=linked-runtime-hid",
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


def test_gate_summary_keeps_dhcp_bound_net_ready_short_of_tcp_proof() -> None:
    """DHCP plus root-console handoff is not remote TCP/cohsh proof."""

    events = normalizer.parse_events(
        [
            "[INFO root_task::net::stack] [dhcp] start ready "
            "interface=wifi now_ms=45335",
            "[INFO root_task::net::stack] [dhcp] lease bound "
            "ip=192.168.86.154/24 gateway=192.168.86.1 "
            "server=192.168.86.1 lease_s=86400",
            "[net-console] root console wait complete reason=net-ready "
            "action=start-serial-root-console elapsed_ms=46725 polls=4",
        ]
    )

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert gates.wifi_gate == 9
    assert gates.wifi_blocker == "tcp-proof-missing"
    assert record["NET_ACTIVE"] == "wifi"
    assert record["NET_ADDR_SRC"] == "dhcp-lease"
    assert record["NET_DHCP"] == "bound"


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


def test_gate_summary_reports_explicit_wifi_gate7_subgate() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
            "path=primary-bsscfg:join action=ready ssid_len=12 result=0x00000000",
            "CYW43_DRIVER_TASK_WIFI_GATE7 contract=cyw43455 "
            "source=join-request subgate=7a name=join-submit status=ready "
            "reason=primary-bsscfg:join polls=0 associated=no link_up=no "
            "event_rx=0 eapol_rx=0 data_rx=0 result=0x00000000",
            "CYW43_DRIVER_TASK_WIFI_GATE7 contract=cyw43455 "
            "source=host-eapol-status subgate=7b name=wrong-producer-name "
            "status=required reason=cyw43-association-not-associated "
            "polls=8193 associated=no link_up=no event_rx=0 eapol_rx=0 "
            "data_rx=0 result=0xffffffef",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-not-associated "
            "polls=8193 data_rx=0 eapol_rx=0 event_rx=0 empty_polls=8193 "
            "associated=no link_up=no assoc_event=none "
            "assoc_probe=not-associated assoc_probe_result=0xffffffef "
            "assoc_set_ssid_rescue=yes firstread_class=preassoc-cadence-empty "
            "rx_firstread_empty=1 control_rx_firstread_empty=0",
        ]
    )

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert gates.wifi_gate == 7
    assert gates.wifi_blocker == "cyw43-association-not-associated"
    assert record["WIFI_SUBGATE"] == "7b"
    assert record["WIFI_SUBGATE_NAME"] == "association"
    assert record["WIFI_SUBGATE_SOURCE"] == "host-eapol-status"
    assert record["WIFI_SUBGATE_STATUS"] == "required"
    assert record["WIFI_SUBGATE_REASON"] == "cyw43-association-not-associated"
    assert record["WIFI_SUBGATE_LINE"] == 4


def test_gate_summary_tracks_oldgood_prejoin_probe_and_pmk_evidence() -> None:
    lines = [
        "CYW43_DRIVER_TASK_FIRMWARE_SUPPLICANT contract=cyw43455 "
        "path=primary-plain status=unsupported action=try-bsscfg-wrapper "
        "reason=known-good-cyw43-fwsup-shape eapver=0xffffffff "
        "timeout_ms=2500 result=0xffffffe2",
        "CYW43_DRIVER_TASK_FIRMWARE_SUPPLICANT contract=cyw43455 "
        "path=bsscfg-wrapper status=unsupported "
        "action=continue-host-eapol-required reason=firmware-offload-unavailable "
        "eapver=0xffffffff timeout_ms=2500 result=0xffffffe2",
        "CYW43_DRIVER_TASK_HOST_EAPOL_PMK contract=cyw43455 "
        "status=ready kind=passphrase ssid_len=12 psk_len=12 "
        "action=derive-host-ptk-on-m1",
        "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
        "path=primary-bsscfg:join action=ready ssid_len=12 result=0x00000000",
        "CYW43_DRIVER_TASK_WIFI_GATE7 contract=cyw43455 "
        "source=join-request subgate=7a name=join-submit status=ready "
        "reason=primary-bsscfg:join polls=0 associated=no link_up=no "
        "event_rx=0 eapol_rx=0 data_rx=0 result=0x00000000",
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
        "status=required reason=cyw43-association-not-associated "
        "polls=8193 data_rx=0 eapol_rx=0 event_rx=0 empty_polls=8193 "
        "associated=no link_up=no assoc_event=none "
        "assoc_probe=not-associated assoc_probe_result=0xffffffef "
        "assoc_set_ssid_rescue=yes firstread_class=preassoc-cadence-empty "
        "rx_firstread_empty=1 control_rx_firstread_empty=0",
        "CYW43_DRIVER_TASK_WIFI_GATE7 contract=cyw43455 "
        "source=host-eapol-status subgate=7b name=association "
        "status=required reason=cyw43-association-not-associated "
        "polls=8193 associated=no link_up=no event_rx=0 eapol_rx=0 "
        "data_rx=0 result=0xffffffef",
    ]

    primary_probe = next(i for i, line in enumerate(lines) if "path=primary-plain" in line)
    wrapper_probe = next(i for i, line in enumerate(lines) if "path=bsscfg-wrapper" in line)
    pmk_ready = next(i for i, line in enumerate(lines) if "HOST_EAPOL_PMK" in line)
    join_request = next(i for i, line in enumerate(lines) if "JOIN_REQUEST" in line)

    assert primary_probe < wrapper_probe < pmk_ready < join_request

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_GATE"] == 7
    assert record["WIFI_BLOCKER"] == "cyw43-association-not-associated"
    assert record["WIFI_SUBGATE"] == "7b"
    assert record["WIFI_SUBGATE_NAME"] == "association"
    assert record["WIFI_SUBGATE_SOURCE"] == "host-eapol-status"
    assert record["WIFI_SUBGATE_STATUS"] == "required"
    assert record["WIFI_SUBGATE_REASON"] == "cyw43-association-not-associated"
    assert record["WIFI_SUBGATE_LINE"] == 7


def test_gate_summary_accepts_explicit_wifi_gate7_7b_plus_frontiers() -> None:
    for subgate, name in [
        ("7b", "association"),
        ("7c", "eapol-rx"),
        ("7d", "eapol-handshake"),
        ("7e", "secure-release"),
    ]:
        events = normalizer.parse_events(
            [
                "CYW43_DRIVER_TASK_WIFI_GATE7 contract=cyw43455 "
                f"source=host-eapol-status subgate={subgate} name={name} "
                "status=pending reason=test-frontier polls=1 "
                "associated=yes link_up=yes event_rx=1 eapol_rx=1 data_rx=1 "
                "result=0x00000000",
            ]
        )

        assert normalizer.summarize_wifi_gate7_subgate(
            events, 7, "wifi-host-eapol-pending"
        ) == (subgate, name)


def test_gate_summary_infers_wifi_gate7_7b_plus_from_host_eapol_status() -> None:
    cases = [
        (
            "status=required reason=cyw43-association-not-associated "
            "polls=8193 associated=no link_up=no event_rx=0 eapol_rx=0 data_rx=0",
            "7b",
            "association",
        ),
        (
            "status=event-rx reason=none polls=12 associated=yes link_up=yes "
            "event_rx=1 eapol_rx=0 data_rx=0",
            "7c",
            "eapol-rx",
        ),
        (
            "status=eapol-rx reason=none polls=13 associated=yes link_up=yes "
            "event_rx=1 eapol_rx=1 data_rx=1 next_action=inspect-host-eapol-handshake-state",
            "7d",
            "eapol-handshake",
        ),
        (
            "status=secure reason=none polls=14 associated=yes link_up=yes "
            "event_rx=1 eapol_rx=2 data_rx=2 next_action=release-dhcp-data",
            "7e",
            "secure-release",
        ),
    ]

    for status_line, subgate, name in cases:
        events = normalizer.parse_events(
            [f"CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 {status_line}"]
        )
        record = normalizer.summarize_gates(events).to_record()

        assert record["WIFI_SUBGATE"] == subgate
        assert record["WIFI_SUBGATE_NAME"] == name
        assert record["WIFI_SUBGATE_SOURCE"] == "host-eapol-status"
        assert record["WIFI_SUBGATE_LINE"] == 1


def test_gate_summary_infers_wifi_gate7_subgate_for_old_association_logs() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=cyw43-association-not-associated "
            "polls=24576 data_rx=0 eapol_rx=0 event_rx=0 empty_polls=24576 "
            "associated=no link_up=no assoc_event=none "
            "assoc_probe=not-associated assoc_probe_result=0xffffffef "
            "assoc_set_ssid_rescue=yes firstread_class=preassoc-cadence-empty "
            "rx_firstread_empty=9 control_rx_firstread_empty=0",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 7
    assert record["WIFI_BLOCKER"] == "cyw43-association-not-associated"
    assert record["WIFI_SUBGATE"] == "7b"
    assert record["WIFI_SUBGATE_NAME"] == "association"


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
            "netstats: wifi_service_turn op=54 reason=3 progress=0x00000015 "
            "seq=4 credit=9 credit_obs=8 channel=2 rframe=512 "
            "src_flags=0x0042 pre_src=0x43520058 post_src=0x43520059 "
            "eapol_m1=1 eapol_m2=1 eapol_m3=1 eapol_m4=1 ptk=1 gtk=1",
        ]
    )

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert gates.wifi_gate == 9
    assert gates.wifi_blocker == "wifi-gate7-7a-missing"
    assert record["NET_TCP_READY"] == "no"
    assert record["NETTEST_PROOF"] == "yes"
    assert record["COHSH_TCP_AUTH_PROOF"] == "no"
    assert record["WIFI_SERVICE_OP"] == 54
    assert record["WIFI_SERVICE_REASON"] == 3
    assert record["WIFI_SERVICE_PROGRESS"] == "0x00000015"
    assert record["WIFI_SERVICE_SEQ"] == 4
    assert record["WIFI_SERVICE_CREDIT"] == 9
    assert record["WIFI_SERVICE_CHANNEL"] == 2
    assert record["WIFI_SERVICE_RFRAME"] == 512
    assert record["WIFI_SERVICE_EAPOL_M1"] == 1
    assert record["WIFI_SERVICE_EAPOL_M2"] == 1
    assert record["WIFI_SERVICE_EAPOL_M3"] == 1
    assert record["WIFI_SERVICE_EAPOL_M4"] == 1


def test_gate_summary_classifies_missing_host_eapol_m3() -> None:
    events = normalizer.parse_events(
        [
            "netstats: mode=dhcp policy=wifi active=wifi standby=wired "
            "addr_src=host-eapol-m3-missing ip=0.0.0.0 gateway=0.0.0.0 "
            "dhcp=host-eapol-m3-missing",
            "netstats: wifi_service_turn op=54 reason=3 progress=0x00000001 "
            "seq=4 credit=9 credit_obs=8 channel=65535 rframe=0 "
            "src_flags=0x0000 pre_src=0x00000000 post_src=0x00000000 "
            "eapol_m1=1 eapol_m2=1 eapol_m3=0 eapol_m4=0 ptk=0 gtk=0",
        ]
    )

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert gates.wifi_gate == 9
    assert gates.wifi_blocker == "host-eapol-m3-missing"
    assert gates.wifi_exact == "host-eapol-m3-missing"
    assert gates.wifi_phase == "join-security"
    assert record["WIFI_SERVICE_EAPOL_M1"] == 1
    assert record["WIFI_SERVICE_EAPOL_M2"] == 1
    assert record["WIFI_SERVICE_EAPOL_M3"] == 0


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


def test_gate_summary_keeps_dhcp_bound_root_console_net_ready_at_gate_nine() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=secure reason=none polls=8197 associated=yes link_up=yes "
            "eapol_rx=7 next_action=release-dhcp-data",
            "[INFO root_task::net::stack] [dhcp] start ready interface=wifi "
            "now_ms=45335",
            "[INFO root_task::net::stack] [dhcp] tx queued kind=discover "
            "from=selecting to=selecting len=300 attempts=1 tx_packets=1",
            "CYW43_DRIVER_TASK_DATA_PATH contract=cyw43455 event=rx-deliver "
            "action=pending dhcp=offer",
            "[INFO root_task::net::stack] [dhcp] rx ack ip=192.168.86.154 "
            "phase=bound len=300 rx_packets=2",
            "[INFO root_task::net::stack] [dhcp] lease bound "
            "ip=192.168.86.154/24 gateway=192.168.86.1 "
            "server=192.168.86.1 lease_s=86400",
            "[net-console] root console wait complete reason=net-ready "
            "action=start-serial-root-console elapsed_ms=46725 polls=4",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 9
    assert record["WIFI_BLOCKER"] == "tcp-proof-missing"
    assert record["WIFI_EXACT"] == "none"
    assert record["NET_ACTIVE"] == "wifi"
    assert record["NET_ADDR_SRC"] == "dhcp-lease"
    assert record["NET_DHCP"] == "bound"


def test_gate_summary_treats_peer_assisted_nettest_as_ready_for_netstats() -> None:
    events = normalizer.parse_events(
        [
            "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
            "server=192.168.10.1 lease_s=3600",
            "[net-selftest] result tx_ok=true udp_echo_ok=false tcp_ok=false "
            "console_ok=true peer_assisted_ok=true result=peer-assisted-pass",
            "OK NETTEST detail=started",
        ]
    )

    gates = normalizer.summarize_gates(events)

    assert gates.wifi_gate == 9
    assert gates.wifi_blocker == "netstats-missing"


def test_gate_summary_does_not_credit_started_nettest_as_gate_ten() -> None:
    """Command admission is not terminal NETTEST success evidence."""

    events = normalizer.parse_events(
        [
            "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
            "server=192.168.10.1 lease_s=3600",
            "OK NETTEST detail=started",
            "netstats: rx_pkts=4 tx_pkts=9 rx_used=4 tx_used=9 polls=30",
            "netstats: mode=dhcp policy=wifi active=wifi standby=none "
            "addr_src=dhcp-lease ip=192.168.10.50 gateway=192.168.10.1 "
            "dhcp=bound",
            "netstats: wifi_assoc=1 wifi_link=1 eapol_rx=2 "
            "eapol_start=1 eapol_secure=1",
        ]
    )

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert gates.wifi_gate == 9
    assert gates.wifi_blocker != "none"
    assert record["NETTEST_PROOF"] == "no"


def test_gate_summary_keeps_async_peer_assisted_result_unproved_after_netstats() -> None:
    events = normalizer.parse_events(
        [
            "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
            "server=192.168.10.1 lease_s=3600",
            "[net-selftest] result tx_ok=true udp_echo_ok=false tcp_ok=false "
            "console_ok=true peer_assisted_ok=true result=peer-assisted-pass",
            "OK NETTEST detail=started",
            "netstats: rx_pkts=4 tx_pkts=9 rx_used=4 tx_used=9 polls=30",
            "netstats: mode=dhcp policy=wifi active=wifi standby=none "
            "addr_src=dhcp-lease ip=192.168.10.50 gateway=192.168.10.1 dhcp=bound",
            "netstats: wifi_assoc=1 wifi_link=1 eapol_rx=2 eapol_start=1 eapol_secure=1",
        ]
    )

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert gates.wifi_gate == 9
    assert gates.wifi_blocker == "netstats-missing"
    assert record["NETTEST_PROOF"] == "no"


def test_gate_summary_does_not_downgrade_remote_cohsh_after_peer_echo_missing() -> None:
    events = normalizer.parse_events(
        [
            "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
            "server=192.168.10.1 lease_s=3600",
            "[cohsh-net][auth] auth OK, session established (conn_id=1)",
            "[net-selftest] result tx_ok=true udp_echo_ok=false tcp_ok=false "
            "console_ok=false result=peer-assisted-pass",
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


def test_gate_summary_promotes_dhcp_pending_after_secure_release() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=required reason=host-eapol-required polls=8193 "
            "associated=yes link_up=no event_rx=1 eapol_rx=1 data_rx=1 "
            "firstread_class=source-asserted-empty",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=secure reason=none polls=8194 associated=yes link_up=yes "
            "event_rx=1 eapol_rx=4 data_rx=4 next_action=release-dhcp-data",
            "CYW43_DRIVER_TASK_HOST_EAPOL_KEY contract=cyw43455 "
            "kind=scb stage=cyw43-host-eapol-scb-authorize status=deferred",
            "CYW43_DRIVER_TASK_WIFI_GATE7 contract=cyw43455 "
            "source=host-eapol-status subgate=7e name=secure-release "
            "status=secure reason=passed polls=8194 associated=yes link_up=yes "
            "event_rx=1 eapol_rx=4 data_rx=4 result=0x00000000",
            "ERR NETTEST reason=policy detail=dhcp-pending",
            "netstats: mode=dhcp policy=wifi active=wifi standby=none "
            "addr_src=dhcp-pending ip=0.0.0.0 gateway=0.0.0.0 dhcp=selecting",
            "netstats: wifi_assoc=1 wifi_link=1 eapol_rx=4 eapol_start=0 eapol_secure=1",
        ]
    )

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert record["WIFI_GATE"] == 9
    assert record["WIFI_BLOCKER"] == "dhcp-pending"
    assert record["WIFI_EXACT"] == "dhcp-pending"
    assert record["WIFI_PHASE"] == "dhcp"
    assert record["WIFI_SUBGATE"] == "7e"
    assert record["WIFI_SUBGATE_NAME"] == "secure-release"


def test_gate_summary_reports_dhcp_not_started_after_secure_release() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=secure reason=none polls=8194 associated=yes link_up=yes "
            "event_rx=1 eapol_rx=4 data_rx=4 next_action=release-dhcp-data",
            "CYW43_DRIVER_TASK_WIFI_GATE7 contract=cyw43455 "
            "source=host-eapol-status subgate=7e name=secure-release "
            "status=secure reason=passed polls=8194 associated=yes link_up=yes "
            "event_rx=1 eapol_rx=4 data_rx=4 result=0x00000000",
            "wifi: gate 9 name=dhcp-bound status=fail "
            "evidence=active=wifi address_source=dhcp-pending "
            "dhcp_phase=disabled ip=0.0.0.0 "
            "dependency=ready-for-direct-evidence next=nettest-netstats-cohsh",
        ]
    )

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert record["WIFI_GATE"] == 9
    assert record["WIFI_BLOCKER"] == "dhcp-not-started"
    assert record["WIFI_EXACT"] == "dhcp-not-started"
    assert record["WIFI_PHASE"] == "dhcp"
    assert record["WIFI_BLOCKER_LINE"] == 3
    assert record["WIFI_SUBGATE"] == "7e"
    assert record["WIFI_SUBGATE_NAME"] == "secure-release"


def test_gate_summary_reports_rx_admission_blocked_after_secure_release() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=secure reason=none polls=8194 associated=yes link_up=yes "
            "event_rx=1 eapol_rx=4 data_rx=4 next_action=release-dhcp-data",
            "CYW43_DRIVER_TASK_HOST_EAPOL_RX_ADMISSION contract=cyw43455 "
            "action=restore-after-secure status=error allmulti=1 promisc=1 "
            "data=allowed-after-keys",
            "netstats: mode=dhcp policy=wifi active=wifi standby=none "
            "addr_src=wifi-data-rx-admission-blocked ip=0.0.0.0 "
            "gateway=0.0.0.0 dhcp=rx-admission-blocked",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE"] == 9
    assert record["WIFI_BLOCKER"] == "wifi-data-rx-admission-blocked"
    assert record["WIFI_EXACT"] == "wifi-data-rx-admission-blocked"
    assert record["WIFI_PHASE"] == "dhcp"
    assert record["WIFI_BLOCKER_LINE"] == 3
    assert record["WIFI_SUBGATE"] == "7e"
    assert record["WIFI_SUBGATE_NAME"] == "secure-release"


def test_gate_summary_treats_repair_rx_admission_as_non_blocking() -> None:
    for restore_status in ("repair-pending", "repair-deferred"):
        events = normalizer.parse_events(
            [
                "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
                "status=secure reason=none polls=8194 associated=yes link_up=yes "
                "event_rx=1 eapol_rx=4 data_rx=4 next_action=release-dhcp-data",
                "CYW43_DRIVER_TASK_HOST_EAPOL_RX_ADMISSION contract=cyw43455 "
                f"action=restore-after-secure status={restore_status} "
                "allmulti=1 promisc=1 data=allowed-after-keys",
                "[INFO root_task::net::stack] [dhcp] start ready "
                "interface=wifi now_ms=45335",
                "[INFO root_task::net::stack] [dhcp] lease bound "
                "ip=192.168.86.154/24 gateway=192.168.86.1 "
                "server=192.168.86.1 lease_s=86400",
                "[net-console] root console wait complete reason=net-ready "
                "action=start-serial-root-console elapsed_ms=46725 polls=4",
            ]
        )

        record = normalizer.summarize_gates(events).to_record()

        assert record["WIFI_GATE"] == 9
        assert record["WIFI_BLOCKER"] == "tcp-proof-missing"
        assert record["WIFI_EXACT"] == "none"
        assert record["NET_ACTIVE"] == "wifi"
        assert record["NET_ADDR_SRC"] == "dhcp-lease"
        assert record["NET_DHCP"] == "bound"


def test_gate_summary_reports_cyw43_data_path_status_blockers() -> None:
    cases = [
        ("wifi-rx-overflow", "rx-overflow", "runtime-rx"),
        ("wifi-rx-starvation", "rx-starvation", "runtime-rx"),
        ("wifi-tx-terminal-fault", "tx-terminal-fault", "data-tx"),
    ]
    for addr_src, dhcp, phase in cases:
        events = normalizer.parse_events(
            [
                "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
                "status=secure reason=none polls=8194 associated=yes link_up=yes "
                "event_rx=1 eapol_rx=4 data_rx=4 next_action=release-dhcp-data",
                "netstats: mode=dhcp policy=wifi active=wifi standby=none "
                f"addr_src={addr_src} ip=192.168.86.154 "
                f"gateway=192.168.86.1 dhcp={dhcp} tcp_ready=no",
            ]
        )

        record = normalizer.summarize_gates(events).to_record()

        assert record["NET_ACTIVE"] == "wifi"
        assert record["NET_ADDR_SRC"] == addr_src
        assert record["NET_DHCP"] == dhcp
        assert record["WIFI_GATE"] == 9
        assert record["WIFI_BLOCKER"] == addr_src
        assert record["WIFI_EXACT"] == addr_src
        assert record["WIFI_PHASE"] == phase
        assert record["WIFI_BLOCKER_LINE"] == 2
        assert record["WIFI_SUBGATE"] == "7e"
        assert record["WIFI_SUBGATE_NAME"] == "secure-release"


def test_gate_summary_accepts_oldgood_wifi_replay_contract() -> None:
    events = normalizer.parse_events(oldgood_wifi_replay_lines())

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert gates.wifi_gate == 10
    assert gates.wifi_blocker == "none"
    assert record["WIFI_OLDGOOD_REPLAY"] == "yes"
    assert record["WIFI_OLDGOOD_LAST"] == "dpc-healthy-after-tcp"
    assert record["WIFI_OLDGOOD_MISSING"] == "none"
    assert record["WIFI_FIRMWARE_IDENTITY_PROOF"] == "yes"
    assert record["WIFI_CLM_READY_PROOF"] == "yes"
    assert record["WIFI_FIRMWARE_VERSION_PROOF"] == "yes"
    assert record["WIFI_CLM_VERSION_PROOF"] == "yes"
    assert record["SDIO_IRQ158_INBAND_PROOF"] == "yes"
    assert record["WIFI_SUBGATE"] == "8h-data-admission"
    assert record["WIFI_SUBGATE_NAME"] == "8h-data-admission"


def test_gate_summary_accepts_identity_bound_wifi_oldgood_retained_prefix() -> None:
    """The compact replay prefix must close only with fresh live tail proof."""

    lines = [
        line
        for line in retained_wifi_oldgood_replay_lines()
        if not line.startswith(("wifi: firmware_contract", "wifi: firmware_release"))
    ]
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "yes"
    assert record["WIFI_OLDGOOD_LAST"] == "dpc-healthy-after-tcp"
    assert record["WIFI_OLDGOOD_MISSING"] == "none"
    assert record["WIFI_FIRMWARE_IDENTITY_PROOF"] == "yes"


def test_wifi_oldgood_retained_prefix_rejects_noncontiguous_or_reordered_rows() -> None:
    """Filtered or reordered physical rows cannot reconstruct an atomic receipt."""

    for mutation in ("gap", "swap"):
        lines = retained_wifi_oldgood_replay_lines()
        begin = oldgood_wifi_line_index(lines, "WIFI_OLDGOOD_RETAINED_BEGIN")
        if mutation == "gap":
            lines.insert(begin + 2, "unclassified physical serial gap")
        else:
            lines[begin + 1], lines[begin + 2] = (
                lines[begin + 2],
                lines[begin + 1],
            )

        record = normalizer.summarize_gates(
            normalizer.parse_events(lines)
        ).to_record()

        assert record["WIFI_OLDGOOD_REPLAY"] == "no"
        assert record["WIFI_OLDGOOD_MISSING"] != "none"


def test_wifi_oldgood_retained_prefix_rejects_wrong_owner_or_identity() -> None:
    """Current owner seals and pair/generation identity are mandatory."""

    for old, new in (
        ("contract=serial ", "contract=serial-console "),
        ("generation=9 prefix_steps=26", "generation=10 prefix_steps=26"),
    ):
        lines = retained_wifi_oldgood_replay_lines()
        receipt_start = oldgood_wifi_line_index(
            lines, "DRIVER_TASK_OWNER_STATE contract=serial "
        )
        receipt_end = oldgood_wifi_line_index(lines, "WIFI_OLDGOOD_RETAINED_END")
        changed = False
        for index in range(receipt_start, receipt_end + 1):
            if old in lines[index]:
                lines[index] = lines[index].replace(old, new, 1)
                changed = True
                break
        assert changed

        record = normalizer.summarize_gates(
            normalizer.parse_events(lines)
        ).to_record()

        assert record["WIFI_OLDGOOD_REPLAY"] == "no"


def test_newer_incomplete_wifi_oldgood_retained_prefix_revokes_older_complete() -> None:
    """A clipped later receipt cannot leave an older transaction authoritative."""

    lines = [
        *retained_wifi_oldgood_replay_lines(),
        "WIFI_OLDGOOD_RETAINED_BEGIN",
    ]
    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_MISSING"] == "retained-prefix-begin-malformed"


@pytest.mark.parametrize(
    "boundary",
    [
        "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
        "path=association-supervisor action=ready generation=10 ssid_len=7 "
        "result=0x00000000",
        "CYW43_GATE8_RECOVERY",
    ],
)
def test_wifi_oldgood_retained_prefix_is_revoked_by_later_join_or_recovery(
    boundary: str,
) -> None:
    """A later association lifecycle edge invalidates the retained generation."""

    record = normalizer.summarize_gates(
        normalizer.parse_events([*retained_wifi_oldgood_replay_lines(), boundary])
    ).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_MISSING"] == "retained-prefix-invalidated"


def test_wifi_oldgood_retained_prefix_requires_fresh_tcp_and_dpc_tail() -> None:
    """The passive prefix cannot reuse older TCP or DPC evidence."""

    for removed in ("tcp_accepts=1", "CYW43_SDIO_DPC generation=9"):
        lines = [
            line
            for line in retained_wifi_oldgood_replay_lines()
            if removed not in line
        ]
        record = normalizer.summarize_gates(
            normalizer.parse_events(lines)
        ).to_record()

        assert record["WIFI_OLDGOOD_REPLAY"] == "no"
        assert record["WIFI_OLDGOOD_MISSING"] != "none"


def test_wifi_oldgood_retained_prefix_rejects_cross_generation_network_tail() -> None:
    """Association-generation TCP/nettest rows cannot be stitched across boots."""

    lines = retained_wifi_oldgood_replay_lines()
    end = oldgood_wifi_line_index(lines, "WIFI_OLDGOOD_RETAINED_END")
    for index in range(end + 1, len(lines)):
        lines[index] = lines[index].replace("generation=9", "generation=10")
    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_MISSING"] in {
        "tcp-authenticated",
        "gate8-identity-current",
    }


def test_truncated_wifi_oldgood_retained_prefix_cannot_supply_live_gate7() -> None:
    """Passive replay rows remain non-authoritative even when the tail is clipped."""

    lines = retained_wifi_oldgood_receipt_lines()
    secure = oldgood_wifi_line_index(
        lines,
        "status=secure associated=yes link_up=yes",
    )
    events = normalizer.parse_events(lines[: secure + 1])
    proof = normalizer.summarize_wifi_gate7_proof(
        normalizer.wifi_oldgood_authority_events(events)
    )

    assert not proof.complete


def test_malformed_wifi_oldgood_retained_prefix_revokes_older_live_authority() -> None:
    """A clipped passive lifecycle row cannot hide behind an older Gate 7/8."""

    lines = retained_wifi_oldgood_replay_lines()
    begin = oldgood_wifi_line_index(lines, "WIFI_OLDGOOD_RETAINED_BEGIN")
    end = oldgood_wifi_line_index(lines, "WIFI_OLDGOOD_RETAINED_END")
    for index in range(begin, end + 1):
        if "msg=m1" in lines[index]:
            lines[index] = "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE clipped"
            break
    else:
        raise AssertionError("missing retained M1 row")

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_MISSING"] == "host-eapol-m1"
    assert record["WIFI_GATE7_COMPLETE"] == "no"
    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert int(record["WIFI_GATE"]) < 10


@pytest.mark.parametrize(
    ("old", "new"),
    [
        ("index=2 ", "index=65536 "),
        ("assoc_poll=2 ", "assoc_poll=4294967296 "),
        ("eapol_rx=2", "eapol_rx=4294967296"),
    ],
)
def test_wifi_oldgood_retained_prefix_rejects_impossible_producer_widths(
    old: str,
    new: str,
) -> None:
    """The parser must enforce the Rust producer's u16/u32 field widths."""

    lines = retained_wifi_oldgood_replay_lines()
    begin = oldgood_wifi_line_index(lines, "WIFI_OLDGOOD_RETAINED_BEGIN")
    end = oldgood_wifi_line_index(lines, "WIFI_OLDGOOD_RETAINED_END")
    for index in range(begin, end + 1):
        if old in lines[index]:
            lines[index] = lines[index].replace(old, new, 1)
            break
    else:
        raise AssertionError(f"missing retained field {old!r}")

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"


def test_oldgood_wifi_replay_requires_authenticated_tcp_counters() -> None:
    lines = [line for line in oldgood_wifi_replay_lines() if "tcp_accepts=" not in line]

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_MISSING"] == "tcp-authenticated"


def test_oldgood_wifi_replay_requires_explicit_tcp_ready() -> None:
    lines = [line for line in oldgood_wifi_replay_lines() if "tcp_ready=yes" not in line]

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_MISSING"] == "tcp-ready"


def test_gate_summary_accepts_oldgood_wifi_resource_replay_contract() -> None:
    events = normalizer.parse_events(oldgood_wifi_resource_replay_lines())

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "yes"
    assert record["WIFI_OLDGOOD_LAST"] == "dpc-healthy-after-tcp"
    assert record["WIFI_OLDGOOD_MISSING"] == "none"
    assert record["WIFI_SUBGATE"] == "8h-data-admission"
    assert record["WIFI_SUBGATE_NAME"] == "8h-data-admission"
    assert record["WIFI_GATE7_COMPLETE"] == "yes"
    assert record["WIFI_GATE7_SEEN"] == "7a>7b>7c>7d>7e"
    assert record["WIFI_GATE7_LAST"] == "7e"
    assert record["WIFI_GATE7_MISSING"] == "none"


@pytest.mark.parametrize(
    ("removed", "seen", "last", "missing"),
    [
        ("CYW43_DRIVER_TASK_JOIN_REQUEST", "none", "none", "7a"),
        ("association-proof", "7a", "7a", "7b"),
        ("msg=m1", "7a>7b", "7b", "7c"),
        ("msg=m2", "7a>7b>7c", "7c", "7d"),
        ("msg=m3", "7a>7b>7c", "7c", "7d"),
        ("msg=m4", "7a>7b>7c", "7c", "7d"),
        ("kind=ptk", "7a>7b>7c", "7c", "7d"),
        ("kind=gtk", "7a>7b>7c", "7c", "7d"),
        ("status=secure", "7a>7b>7c>7d", "7d", "7e"),
    ],
)
def test_gate7_ordered_proof_fails_closed_at_every_host_eapol_cut(
    removed: str,
    seen: str,
    last: str,
    missing: str,
) -> None:
    lines = oldgood_wifi_resource_replay_lines()
    if removed == "association-proof":
        lines = [
            line
            for line in lines
            if "associated=yes link_up=yes" not in line
            and "wifi_assoc=1 wifi_link=1" not in line
        ]
    else:
        lines = [line for line in lines if removed not in line]

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_GATE7_COMPLETE"] == "no"
    assert record["WIFI_GATE7_SEEN"] == seen
    assert record["WIFI_GATE7_LAST"] == last
    assert record["WIFI_GATE7_MISSING"] == missing
    assert record["WIFI_GATE"] == 9


def test_wifi_gate10_is_revoked_when_gate7_proof_is_forbidden() -> None:
    """Canonical WiFi Gate 10 cannot survive an incomplete Gate 7 proof."""

    lines = [
        *oldgood_wifi_resource_replay_lines(),
        "wifi: evidence root_pointer=yes source=forbidden-shortcut",
    ]

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE7_COMPLETE"] == "no"
    assert record["WIFI_GATE7_MISSING"] == "forbidden-shortcut"
    assert record["WIFI_GATE"] == 9
    assert (
        record["WIFI_BLOCKER"]
        == "wifi-gate7-forbidden-shortcut-missing"
    )


def test_gate7_ordered_proof_rejects_reordered_handshake_steps() -> None:
    lines = oldgood_wifi_resource_replay_lines()
    m2_index = oldgood_wifi_line_index(lines, "msg=m2")
    m3_index = oldgood_wifi_line_index(lines, "msg=m3")
    lines[m2_index], lines[m3_index] = lines[m3_index], lines[m2_index]

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_GATE7_COMPLETE"] == "no"
    assert record["WIFI_GATE7_SEEN"] == "7a>7b>7c"
    assert record["WIFI_GATE7_LAST"] == "7c"
    assert record["WIFI_GATE7_MISSING"] == "7d"


def test_gate7_ordered_proof_does_not_stitch_across_join_attempts() -> None:
    lines = oldgood_wifi_resource_replay_lines()
    m2_index = oldgood_wifi_line_index(lines, "msg=m2")
    lines.insert(
        m2_index,
        "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
        "path=primary-bsscfg:join action=ready ssid_len=7 result=0x00000000",
    )

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_GATE7_COMPLETE"] == "no"
    assert record["WIFI_GATE7_SEEN"] == "7a>7b"
    assert record["WIFI_GATE7_LAST"] == "7b"
    assert record["WIFI_GATE7_MISSING"] == "7c"


def test_gate7_ordered_proof_rejects_firmware_supplicant_shortcut() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
            "path=primary-bsscfg:join action=ready result=0x00000000",
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS status=required "
            "associated=yes link_up=yes eapol_rx=0",
            JOIN_COMPLETE_SECURE,
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS status=secure "
            "associated=yes link_up=yes eapol_rx=2",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE7_COMPLETE"] == "no"
    assert record["WIFI_GATE7_SEEN"] == "none"
    assert record["WIFI_GATE7_MISSING"] == "forbidden-shortcut"


def test_boot_acceptance_requires_complete_ordered_wifi_gate7_proof() -> None:
    record = normalizer.summarize_gates(
        normalizer.parse_events(oldgood_wifi_resource_replay_lines())
    ).to_record()
    record["WIFI_GATE7_COMPLETE"] = "no"
    record["WIFI_GATE7_MISSING"] = "7d"

    blockers = normalizer.boot_evidence_blockers(record)

    assert "wifi-gate7-subgates-incomplete" in blockers
    assert "wifi-gate7-7d-missing" in blockers


def test_retained_gate7_requires_one_exact_current_diag_transaction() -> None:
    """Retained Gate 7 and all Gate 8 rows bind one id, pair, and generation."""

    snapshot_sequence = 17
    pair_epoch = 23
    generation = 29
    lines = [
        wifi_diag_begin_line(snapshot_sequence, pair_epoch, generation),
        wifi_gate7_retained_line(snapshot_sequence, pair_epoch, generation),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=pair_epoch,
                generation=generation,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        wifi_diag_complete_line(snapshot_sequence, pair_epoch, generation),
    ]
    events = normalizer.parse_events(lines)

    gate7 = normalizer.summarize_wifi_gate7_proof(events)
    gate8 = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )

    assert gate7.complete
    assert gate7.retained_current
    assert gate7.snapshot_sequence == snapshot_sequence
    assert gate7.pair_epoch == pair_epoch
    assert gate7.generation == generation
    assert gate8.complete
    assert gate8.pair_epoch == pair_epoch
    assert gate8.generation == generation


def test_retained_gate7_accepts_exact_clipped_gate8_tail_recovery() -> None:
    """An exact detail=no terminal may recover a contiguous same-id tail cut."""

    snapshot_sequence = 17
    pair_epoch = 23
    generation = 29
    lines = [
        wifi_diag_begin_line(snapshot_sequence, pair_epoch, generation),
        wifi_gate7_retained_line(snapshot_sequence, pair_epoch, generation),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=pair_epoch,
                generation=generation,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES[:-1]
        ],
        wifi_diag_complete_line(
            snapshot_sequence,
            pair_epoch,
            generation,
            detail="no",
        ),
    ]
    events = normalizer.parse_events(lines)

    gate7 = normalizer.summarize_wifi_gate7_proof(events)
    gate8 = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )

    assert gate7.complete
    assert gate7.retained_current
    assert gate7.snapshot_sequence == snapshot_sequence
    assert gate8.complete
    assert gate8.pair_epoch == pair_epoch
    assert gate8.generation == generation


@pytest.mark.parametrize(
    "trailing_kind",
    [
        "valid-begin",
        "malformed-begin",
        "bare-begin",
        "retained",
        "bare-retained",
        "context",
        "bare-context",
    ],
)
def test_newer_incomplete_wifi_diag_revokes_older_current_snapshot(
    trailing_kind: str,
) -> None:
    """A captured prefix of a newer diagnostic cannot retain an old pass."""

    trailing = {
        "valid-begin": wifi_diag_begin_line(18, 31, 37),
        "malformed-begin": "wifi: diag_begin id=truncated",
        "bare-begin": "wifi: diag_begin",
        "retained": wifi_gate7_retained_line(18, 31, 37),
        "bare-retained": "wifi: gate7_retained",
        "context": (
            "wifi: diag_context id=18 retained=none cause=none trigger=none"
        ),
        "bare-context": "wifi: diag_context",
    }[trailing_kind]
    lines = [
        wifi_diag_begin_line(17, 23, 29),
        wifi_gate7_retained_line(17, 23, 29),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=23,
                generation=29,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        wifi_diag_complete_line(17, 23, 29),
        trailing,
    ]
    events = normalizer.parse_events(lines)

    gate7 = normalizer.summarize_wifi_gate7_proof(events)
    gate8 = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )

    assert not gate7.complete
    assert gate7.missing == "retained-diag-incomplete"
    assert not gate8.complete
    assert gate8.blocker == "diag-transaction-incomplete"


def test_newer_partial_gate8_is_not_overwritten_by_older_diag_pass() -> None:
    """A later Gate 8 frontier remains authoritative over an old terminal."""

    lines = [
        wifi_diag_begin_line(17, 23, 29),
        wifi_gate7_retained_line(17, 23, 29),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=23,
                generation=29,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        wifi_diag_complete_line(17, 23, 29),
        wifi_gate8_subgate_line(
            "8a-pair-generation",
            pair_epoch=31,
            generation=37,
        ),
    ]
    events = normalizer.parse_events(lines)

    gate8 = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )

    assert not gate8.complete
    assert gate8.pair_epoch == 31
    assert gate8.generation == 37
    assert gate8.blocker == "telemetry-truncated"


def test_gate8_recovery_after_diag_terminal_revokes_older_pass() -> None:
    """A later recovery boundary cannot be erased by an old diag summary."""

    lines = [
        *oldgood_wifi_resource_replay_lines(),
        wifi_diag_begin_line(17, 1, 9),
        wifi_gate7_retained_line(17, 1, 9),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=1,
                generation=9,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        wifi_diag_complete_line(17, 1, 9),
        (
            "CYW43_GATE8_RECOVERY attempt=1 generation=9 "
            "blocker=carrier-lost deadline_ms=250 action=pair-restart"
        ),
    ]

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE"] < 10
    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_BLOCKER"] == "gate8-recovery"


@pytest.mark.parametrize(
    "boundary",
    [
        "CYW43_GATE8_RECOVERY",
        "CYW43_GATE8_READY_RETRACTED",
        "CYW43_GATE8_READY_TRANSACTION",
        "CYW43_GATE8_SNAPSHOT_COMMIT",
        "CYW43_GATE8_COMMIT",
        "CYW43_RUNTIME_RECOVERY",
        "CYW43_BOOTSTRAP_SUPERVISOR",
    ],
)
def test_bare_gate8_authority_boundary_revokes_older_pass(
    boundary: str,
) -> None:
    """Every reserved Gate 8 lifecycle token fails closed when truncated."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(
            [*oldgood_wifi_resource_replay_lines(), boundary]
        )
    ).to_record()

    assert record["WIFI_GATE"] < 10
    assert record["WIFI_GATE8_COMPLETE"] == "no"


def test_bare_gate8_subgate_fails_inside_current_diag_transaction() -> None:
    """A malformed subgate is red when bracketed as current diagnostic truth."""

    lines = [
        wifi_diag_begin_line(17, 23, 29),
        wifi_gate7_retained_line(17, 23, 29),
        "wifi: gate 8 subgate",
        wifi_diag_complete_line(17, 23, 29),
    ]
    events = normalizer.parse_events(lines)

    gate7 = normalizer.summarize_wifi_gate7_proof(events)
    gate8 = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )

    assert not gate7.complete
    assert not gate8.complete


@pytest.mark.parametrize(
    "boundary",
    [
        (
            "CYW43_GATE8_RECOVERY attempt=1 generation=29 "
            "blocker=carrier-lost deadline_ms=250 action=pair-restart"
        ),
        "CYW43_GATE8_READY_RETRACTED",
        "CYW43_GATE8_READY_TRANSACTION",
        "CYW43_GATE8_SNAPSHOT_COMMIT",
        "CYW43_GATE8_COMMIT",
        "CYW43_RUNTIME_RECOVERY",
        "CYW43_BOOTSTRAP_SUPERVISOR",
    ],
)
def test_gate8_lifecycle_boundary_inside_diag_window_revokes_pass(
    boundary: str,
) -> None:
    """A lifecycle edge cannot be hidden behind the old terminal summary."""

    lines = [
        wifi_diag_begin_line(17, 23, 29),
        wifi_gate7_retained_line(17, 23, 29),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=23,
                generation=29,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        boundary,
        wifi_diag_complete_line(17, 23, 29),
    ]
    events = normalizer.parse_events(lines)

    gate8 = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )
    gate7 = normalizer.summarize_wifi_gate7_proof(events)

    assert not gate8.complete
    assert gate8.blocker == "diag-transaction-boundary"
    assert not gate7.complete


def test_additional_gate8_row_inside_diag_window_revokes_pass() -> None:
    """A second snapshot frontier cannot splice into an old terminal."""

    lines = [
        wifi_diag_begin_line(17, 23, 29),
        wifi_gate7_retained_line(17, 23, 29),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=23,
                generation=29,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        wifi_gate8_subgate_line(
            "8a-pair-generation",
            pair_epoch=31,
            generation=37,
        ),
        wifi_diag_complete_line(17, 23, 29),
    ]
    events = normalizer.parse_events(lines)

    gate8 = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )

    assert not gate8.complete
    assert gate8.blocker == "diag-transaction-boundary"


def test_different_gate8_frontier_after_clipped_prefix_revokes_recovery() -> None:
    """A second 8a cannot masquerade as the clipped 8h tail of an old pair."""

    lines = [
        wifi_diag_begin_line(17, 23, 29),
        wifi_gate7_retained_line(17, 23, 29),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=23,
                generation=29,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES[:-1]
        ],
        wifi_gate8_subgate_line(
            "8a-pair-generation",
            pair_epoch=31,
            generation=37,
        ),
        wifi_diag_complete_line(17, 23, 29, detail="no"),
    ]
    events = normalizer.parse_events(lines)

    gate7 = normalizer.summarize_wifi_gate7_proof(events)
    gate8 = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )

    assert not gate7.complete
    assert not gate8.complete
    assert gate8.blocker == "diag-transaction-boundary"


def test_retained_gate7_identity_cannot_splice_into_newer_gate8() -> None:
    """Gate 10 binds retained host-EAPOL truth to the exact Gate 8 pair."""

    old_pair_epoch = 2
    old_generation = 3
    current_pair_epoch = 4
    current_generation = 5
    lines = oldgood_wifi_resource_replay_lines()
    stabilizing = next(
        index
        for index, line in enumerate(lines)
        if line.startswith("CYW43_BOOTSTRAP_SUPERVISOR")
        and "status=stabilizing" in line
    )
    old_snapshot = [
        wifi_diag_begin_line(17, old_pair_epoch, old_generation),
        wifi_gate7_retained_line(17, old_pair_epoch, old_generation),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=old_pair_epoch,
                generation=old_generation,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        wifi_diag_complete_line(17, old_pair_epoch, old_generation),
    ]
    lines[stabilizing:stabilizing] = old_snapshot
    lines = [
        line.replace("pair_epoch=1 generation=9", "pair_epoch=4 generation=5")
        for line in lines
    ]
    lines.extend(wifi_generation_gate10_lines(current_generation))

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "yes"
    assert record["WIFI_GATE8_PAIR_EPOCH"] == current_pair_epoch
    assert record["WIFI_GATE8_GENERATION"] == current_generation
    assert record["WIFI_GATE7_COMPLETE"] == "no"
    assert record["WIFI_GATE7_MISSING"] == "retained-generation-mismatch"
    assert record["WIFI_GATE"] == 9


def test_latest_complete_wifi_diag_supersedes_older_snapshot() -> None:
    """A fully terminated newer transaction replaces an earlier pass."""

    lines: list[str] = []
    for snapshot_sequence, pair_epoch, generation in (
        (17, 23, 29),
        (18, 31, 37),
    ):
        lines.extend(
            [
                wifi_diag_begin_line(
                    snapshot_sequence,
                    pair_epoch,
                    generation,
                ),
                wifi_gate7_retained_line(
                    snapshot_sequence,
                    pair_epoch,
                    generation,
                ),
                *[
                    wifi_gate8_subgate_line(
                        subgate,
                        pair_epoch=pair_epoch,
                        generation=generation,
                    )
                    for subgate in normalizer.WIFI_GATE8_SUBGATES
                ],
                wifi_diag_complete_line(
                    snapshot_sequence,
                    pair_epoch,
                    generation,
                ),
            ]
        )
    events = normalizer.parse_events(lines)

    gate7 = normalizer.summarize_wifi_gate7_proof(events)
    gate8 = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )

    assert gate7.complete
    assert gate7.snapshot_sequence == 18
    assert gate7.pair_epoch == 31
    assert gate7.generation == 37
    assert gate8.complete
    assert gate8.pair_epoch == 31
    assert gate8.generation == 37


@pytest.mark.parametrize(
    "later_attempt",
    [
        (
            "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
            "path=association-supervisor action=ready generation=37 "
            "ssid_len=7 result=0x00000000"
        ),
        (
            "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
            "msg=m1 action=recv-m1 poll=12 len=121"
        ),
    ],
)
def test_new_host_eapol_attempt_revokes_retained_gate7(
    later_attempt: str,
) -> None:
    """A new Join or explicit M1 cuts an older retained handshake receipt."""

    lines = [
        wifi_diag_begin_line(17, 23, 29),
        wifi_gate7_retained_line(17, 23, 29),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=23,
                generation=29,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        wifi_diag_complete_line(17, 23, 29),
        later_attempt,
    ]

    gate7 = normalizer.summarize_wifi_gate7_proof(
        normalizer.parse_events(lines)
    )

    assert not gate7.complete
    assert gate7.missing == "retained-new-attempt"


@pytest.mark.parametrize(
    "later_attempt",
    [
        "CYW43_DRIVER_TASK_JOIN_REQUEST",
        (
            "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
            "path=association-supervisor action=ready generation=oops "
            "ssid_len=7 result=0x00000000"
        ),
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE",
    ],
)
def test_malformed_attempt_boundary_revokes_retained_gate7(
    later_attempt: str,
) -> None:
    """A truncated current attempt cannot preserve an old retained receipt."""

    lines = [
        wifi_diag_begin_line(17, 23, 29),
        wifi_gate7_retained_line(17, 23, 29),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=23,
                generation=29,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        wifi_diag_complete_line(17, 23, 29),
        later_attempt,
    ]

    gate7 = normalizer.summarize_wifi_gate7_proof(
        normalizer.parse_events(lines)
    )

    assert not gate7.complete
    assert gate7.missing == "retained-new-attempt-unproven"


@pytest.mark.parametrize(
    "attempt_boundary",
    [
        (
            "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
            "path=association-supervisor action=ready generation=37 "
            "ssid_len=7 result=0x00000000"
        ),
        (
            "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
            "msg=m1 action=recv-m1 poll=12 len=121"
        ),
        "CYW43_DRIVER_TASK_JOIN_REQUEST",
    ],
)
def test_attempt_boundary_inside_diag_window_revokes_retained_gate7(
    attempt_boundary: str,
) -> None:
    """A handshake transition during snapshot emission invalidates Gate 7."""

    lines = [
        wifi_diag_begin_line(17, 23, 29),
        wifi_gate7_retained_line(17, 23, 29),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=23,
                generation=29,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        attempt_boundary,
        wifi_diag_complete_line(17, 23, 29),
    ]

    events = normalizer.parse_events(lines)
    gate7 = normalizer.summarize_wifi_gate7_proof(events)
    gate8 = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )

    assert not gate7.complete
    assert not gate8.complete
    assert gate8.blocker == "diag-transaction-boundary"


@pytest.mark.parametrize("with_diag", [False, True])
@pytest.mark.parametrize(
    "attempt_boundary",
    [
        (
            "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
            "path=association-supervisor action=ready generation=37 "
            "ssid_len=7 result=0x00000000"
        ),
        (
            "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
            "path=primary-bsscfg:join action=ready ssid_len=7 "
            "result=0x00000000"
        ),
        "CYW43_DRIVER_TASK_JOIN_REQUEST",
        (
            "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
            "msg=m1 action=recv-m1 poll=99 len=121"
        ),
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE",
    ],
)
def test_attempt_after_ready_revokes_gate8_authority(
    attempt_boundary: str,
    with_diag: bool,
) -> None:
    """A new association attempt cuts the prior Gate 8 Ready generation."""

    lines = oldgood_wifi_resource_replay_lines()
    if with_diag:
        lines.extend(
            [
                wifi_diag_begin_line(17, 1, 9),
                wifi_gate7_retained_line(17, 1, 9),
                *[
                    wifi_gate8_subgate_line(
                        subgate,
                        pair_epoch=1,
                        generation=9,
                    )
                    for subgate in normalizer.WIFI_GATE8_SUBGATES
                ],
                wifi_diag_complete_line(17, 1, 9),
            ]
        )
    lines.append(attempt_boundary)

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE"] < 10
    assert record["WIFI_GATE7_COMPLETE"] == "no"
    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_BLOCKER"] == "new-association-attempt"


def test_post_secure_m1_does_not_revoke_gate8_without_new_join() -> None:
    """A legal post-secure rekey cuts Gate 7 only, not Gate 8 lifecycle."""

    lines = [
        *oldgood_wifi_resource_replay_lines(),
        (
            "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract=cyw43455 "
            "msg=m1 action=post-secure-recv-m1 poll=99 len=121"
        ),
    ]
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE"] == 9
    assert record["WIFI_GATE7_COMPLETE"] == "no"
    assert record["WIFI_GATE8_COMPLETE"] == "yes"
    assert record["WIFI_DPC_PROOF"] == "yes"


@pytest.mark.parametrize(
    ("current_dpc", "reason"),
    [
        ([], "missing"),
        (["CYW43_SDIO_DPC"], "malformed-line"),
    ],
)
def test_current_wifi_diag_does_not_reuse_an_older_dpc_sample(
    current_dpc: list[str],
    reason: str,
) -> None:
    """The current passive diagnostic owns its exact DPC sample window."""

    lines = [
        *oldgood_wifi_resource_replay_lines(),
        normalizer.WIFI_DIAG_COMMAND_BEGIN_LINE,
        *current_dpc,
        *wifi_current_gate7_diag_lines(
            pair_epoch=1,
            generation=9,
            snapshot_sequence=23,
        ),
    ]
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE"] < 10
    assert record["WIFI_GATE7_COMPLETE"] == "yes"
    assert record["WIFI_GATE8_COMPLETE"] == "yes"
    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == reason


@pytest.mark.parametrize(
    "command_begin",
    [
        "wifi: debug subcommand=diag",
        "wifi: debug subcommand=diag action=begin",
        (
            "wifi: debug subcommand=diag action=begin "
            "profile=bounded"
        ),
        (
            "wifi: debug subcommand=diag action=invalid "
            "profile=bounded mode=one-shot"
        ),
    ],
)
def test_malformed_current_wifi_diag_command_cannot_reuse_dpc(
    command_begin: str,
) -> None:
    """A clipped reserved command marker cuts older DPC authority."""

    lines = [
        *oldgood_wifi_resource_replay_lines(),
        command_begin,
        *wifi_current_gate7_diag_lines(
            pair_epoch=1,
            generation=9,
            snapshot_sequence=23,
        ),
    ]
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE"] < 10
    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "missing"


def test_current_wifi_diag_accepts_only_its_fresh_dpc_triplet() -> None:
    """A fresh exact triplet closes the latest diagnostic DPC receipt."""

    lines = [
        *oldgood_wifi_resource_replay_lines(),
        normalizer.WIFI_DIAG_COMMAND_BEGIN_LINE,
        *healthy_wifi_dpc_triplet(generation=11, captures=8),
        *wifi_current_gate7_diag_lines(
            pair_epoch=1,
            generation=9,
            snapshot_sequence=23,
        ),
        (
            "wifi: debug subcommand=diag action=complete "
            "profile=bounded mode=one-shot result=ok "
            "source=linked-runtime-retained-state"
        ),
    ]
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE"] == 10
    assert record["WIFI_DPC_PROOF"] == "yes"
    assert record["WIFI_DPC_GENERATION"] == 11
    assert record["WIFI_DPC_CAPTURES"] == 8


def test_current_wifi_dump_state_accepts_only_its_fresh_dpc_triplet() -> None:
    """The verbose command owns the current DPC acceptance receipt."""

    lines = [
        *oldgood_wifi_resource_replay_lines(),
        (
            "wifi: debug subcommand=dump-state action=begin "
            "profile=verbose mode=one-shot"
        ),
        *healthy_wifi_dpc_triplet(generation=12, captures=9),
        (
            "wifi: debug subcommand=dump-state action=complete "
            "profile=verbose mode=one-shot result=ok"
        ),
    ]
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE"] == 10
    assert record["WIFI_DPC_PROOF"] == "yes"
    assert record["WIFI_DPC_GENERATION"] == 12
    assert record["WIFI_DPC_CAPTURES"] == 9


def test_incomplete_wifi_dump_state_cannot_reuse_an_older_dpc_sample() -> None:
    """A later incomplete dump-state revokes older DPC sample authority."""

    lines = [
        *oldgood_wifi_resource_replay_lines(),
        (
            "wifi: debug subcommand=dump-state action=begin "
            "profile=verbose mode=one-shot"
        ),
    ]
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE"] < 10
    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "missing"


def test_current_wifi_diag_complete_without_begin_cannot_reuse_dpc() -> None:
    """A terminal-only captured command cannot borrow an older DPC sample."""

    lines = [
        *oldgood_wifi_resource_replay_lines(),
        *wifi_current_gate7_diag_lines(
            pair_epoch=1,
            generation=9,
            snapshot_sequence=23,
        ),
        (
            "wifi: debug subcommand=diag action=complete "
            "profile=bounded mode=one-shot result=ok "
            "source=linked-runtime-retained-state"
        ),
    ]
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE"] < 10
    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "missing"


def test_current_wifi_diag_complete_cannot_stitch_to_prior_command() -> None:
    """Each retained transaction requires its immediately preceding command begin."""

    command_complete = (
        "wifi: debug subcommand=diag action=complete "
        "profile=bounded mode=one-shot result=ok "
        "source=linked-runtime-retained-state"
    )
    lines = [
        *oldgood_wifi_resource_replay_lines(),
        normalizer.WIFI_DIAG_COMMAND_BEGIN_LINE,
        *healthy_wifi_dpc_triplet(generation=11, captures=8),
        *wifi_current_gate7_diag_lines(
            pair_epoch=1,
            generation=9,
            snapshot_sequence=23,
        ),
        command_complete,
        *wifi_current_gate7_diag_lines(
            pair_epoch=1,
            generation=9,
            snapshot_sequence=24,
        ),
        command_complete,
    ]
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE"] < 10
    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "missing"


@pytest.mark.parametrize("gate", [0, 2])
@pytest.mark.parametrize(
    "ring",
    [
        "eff090d9:0001:2000:00000000>eff090d9:0005:5104",
        "eff090d9:0001:2000:00000000>eff090d9:0005:5104:09000004",
    ],
)
def test_schema_v2_wifi_diag_reports_bounded_causal_frontier(
    gate: int,
    ring: str,
) -> None:
    """Compact causal triage is validated without inventing Gate 7/8 proof."""

    body = [
        (
            "wifi: diag_begin id=7 schema=v2 "
            "snapshot=best-effort-multi-record pair=0 gen=0 "
            "source=causal-triage"
        ),
        (
            f"wifi: causal_frontier id=7 gate={gate} status=no-reply "
            "blocker=sdio-engine-init downstream=not-reached"
        ),
        (
            "wifi: causal_progress id=7 cyw43=0/0/unavailable "
            "sdio=0/0/unavailable replay=engine-init/no-reply "
            f"ring={ring}"
        ),
        "wifi: causal_episode id=7 state=unavailable",
        "CYW43_DPC_CHILD_TIMING_ENTRY state=unavailable",
        "wifi: causal_grant id=7 state=unavailable",
        "wifi: causal_fault id=7 state=none",
    ]
    prefix_bytes = sum(len(line.encode("utf-8")) + 2 for line in body)
    body.append(
        "wifi: diag_transport id=7 "
        f"body_lines=7 body_bytes={prefix_bytes} "
        "max_lines=8 max_bytes=2048 backlog_before=0 "
        "wake=bound/badge/polls/hits:yes/8/4/1"
    )
    body_bytes = sum(len(line.encode("utf-8")) + 2 for line in body)
    lines = [
        (
            "wifi: debug subcommand=diag action=begin "
            "profile=bounded-causal mode=one-shot"
        ),
        *body,
        (
            "wifi: diag_complete id=7 causal=yes detail=yes schema=v2 "
            f"gate={gate} status=no-reply blocker=sdio-engine-init "
            f"body_lines=8 body_bytes={body_bytes}"
        ),
        (
            "wifi: debug subcommand=diag action=complete "
            "profile=bounded-causal mode=one-shot result=ok "
            "source=causal-triage"
        ),
    ]

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_DIAG_DETAIL"] == "yes"
    assert record["WIFI_DIAG_SCOPE"] == "best-effort-multi-record"
    assert record["WIFI_DIAG_BODY_LINES"] == 8
    assert record["WIFI_DIAG_BODY_BYTES"] == body_bytes
    assert (
        record["WIFI_CAUSAL_FRONTIER"]
        == f"gate-{gate}/no-reply/sdio-engine-init"
    )
    assert record["WIFI_GATE7_COMPLETE"] == "no"
    assert record["WIFI_GATE8_COMPLETE"] == "no"


@pytest.mark.parametrize(
    "ring",
    [
        "u",
        "eff090d9:0001:2000:00000000>eff090d9:0005:5104",
        "eff090d9:0001:2000:00000000>eff090d9:0005:5104:09000004",
    ],
)
def test_schema_v2_wifi_diag_accepts_exact_historical_and_current_rings(
    ring: str,
) -> None:
    """Schema v2 retains its exact legacy tuple alongside current evidence."""

    line = (
        "wifi: causal_progress id=7 cyw43=0/0/unavailable "
        "sdio=0/0/unavailable replay=engine-init/no-reply "
        f"ring={ring}"
    )

    assert normalizer.WIFI_CAUSAL_DIAG_PROGRESS_RE.fullmatch(line) is not None


def test_schema_v2_wifi_diag_accepts_exact_historical_progress_without_ring() -> None:
    """Early schema-v2 logs remain parseable before the ring field existed."""

    line = (
        "wifi: causal_progress id=7 cyw43=0/0/unavailable "
        "sdio=0/0/unavailable replay=engine-init/no-reply"
    )

    assert normalizer.WIFI_CAUSAL_DIAG_PROGRESS_RE.fullmatch(line) is not None


@pytest.mark.parametrize(
    "ring",
    [
        "eff090d9:0001:2000:00000000>eff090d9:0005:5104:9000004",
        "eff090d9:0001:2000:00000000>eff090d9:0005:5104:09000004:0000",
        "EFF090D9:0001:2000:00000000>eff090d9:0005:5104:09000004",
    ],
)
def test_schema_v2_wifi_diag_rejects_inexact_sdio_ring_result(ring: str) -> None:
    """Neither historical nor current schema-v2 tuples admit malformed data."""

    line = (
        "wifi: causal_progress id=7 cyw43=0/0/unavailable "
        "sdio=0/0/unavailable replay=engine-init/no-reply "
        f"ring={ring}"
    )

    assert normalizer.WIFI_CAUSAL_DIAG_PROGRESS_RE.fullmatch(line) is None


def schema_v2_containment_diag_lines(
    ring: str | None,
    episode: str,
    blocker: str = "sdio-host-config-containment-host-reset1-timeout",
    fault: str = (
        "wifi: causal_fault id=7 stage=cyw43-transport-init op=0001 "
        "flags=0000 target=00000000 payload=0/0 total=0 detail=5310 "
        "reason=cyw43-transport-bus-link-missing result=06000000"
    ),
) -> list[str]:
    """Build one byte-exact bounded causal transaction for host validation."""

    progress = (
        "wifi: causal_progress id=7 cyw43=3/446/"
        "cyw43-sdio-pair-restart-required "
        "sdio=3/1/command-observed replay=unavailable/unavailable"
    )
    if ring is not None:
        progress = f"{progress} ring={ring}"
    body = [
        (
            "wifi: diag_begin id=7 schema=v2 "
            "snapshot=best-effort-multi-record pair=1 gen=0 "
            "source=causal-triage"
        ),
        (
            f"wifi: causal_frontier id=7 gate=2 status=fail "
            f"blocker={blocker} downstream=not-reached"
        ),
        progress,
        episode,
        "CYW43_DPC_CHILD_TIMING_ENTRY state=unavailable",
        "wifi: causal_grant id=7 state=unavailable",
        fault,
    ]
    prefix_bytes = sum(len(line.encode("utf-8")) + 2 for line in body)
    body.append(
        "wifi: diag_transport id=7 "
        f"body_lines=7 body_bytes={prefix_bytes} "
        "max_lines=8 max_bytes=2048 backlog_before=0 "
        "wake=bound/badge/polls/hits:yes/8/4/1"
    )
    body_bytes = sum(len(line.encode("utf-8")) + 2 for line in body)
    return [
        (
            "wifi: debug subcommand=diag action=begin "
            "profile=bounded-causal mode=one-shot"
        ),
        *body,
        (
            "wifi: diag_complete id=7 causal=yes detail=yes schema=v2 "
            f"gate=2 status=fail blocker={blocker} "
            f"body_lines=8 body_bytes={body_bytes}"
        ),
        (
            "wifi: debug subcommand=diag action=complete "
            "profile=bounded-causal mode=one-shot result=ok "
            "source=causal-triage"
        ),
    ]


CURRENT_CONTAINMENT_RING = (
    "eff090d9:0006:2000:43595301>eff090d9:0005:5104:09000004"
)
CURRENT_CONTAINMENT_EPISODE = (
    "wifi: causal_episode id=7 pub=1 episode=1 phys=1129927425 "
    "logical=0 parent=1414664193/0001 "
    "child=4025520345/0005/5104/09000004 "
    "exit=4/5310/06000000 pending=00030324"
)


def test_schema_v2_containment_refinement_requires_correlated_current_evidence() -> None:
    """A current result, physical epoch, and episode admit exact refinement."""

    lines = schema_v2_containment_diag_lines(
        CURRENT_CONTAINMENT_RING,
        CURRENT_CONTAINMENT_EPISODE,
    )
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_CAUSAL_FRONTIER"] == (
        "gate-2/fail/sdio-host-config-containment-host-reset1-timeout"
    )


@pytest.mark.parametrize(
    ("phase", "blocker"),
    [
        (1, "sdio-host-config-containment-dma-abort-timeout"),
        (2, "sdio-host-config-containment-dma-verify"),
        (3, "sdio-host-config-containment-host-reset1-injected"),
        (4, "sdio-host-config-containment-host-reset1-timeout"),
        (5, "sdio-host-config-containment-clock-settle1-timeout"),
        (6, "sdio-host-config-containment-host-reset2-injected"),
        (7, "sdio-host-config-containment-host-reset2-timeout"),
        (8, "sdio-host-config-containment-clock-settle2-timeout"),
        (9, "sdio-host-config-containment-final-inhibit-timeout"),
        (10, "sdio-host-config-containment-final-host-not-quiescent"),
        (11, "sdio-host-config-containment-final-dma-not-quiescent"),
        (12, "sdio-host-config-containment-unclassified"),
        (13, "sdio-host-config-containment-unknown"),
    ],
)
def test_schema_v2_containment_phase_mapping_is_exact(
    phase: int,
    blocker: str,
) -> None:
    """Every typed stage-9 containment result maps to one literal blocker."""

    result = 0x0900_0000 | phase
    ring = (
        "eff090d9:0006:2000:43595301>"
        f"eff090d9:0005:5104:{result:08x}"
    )
    progress = (
        "wifi: causal_progress id=7 cyw43=3/446/"
        "cyw43-sdio-pair-restart-required "
        "sdio=3/1/command-observed replay=unavailable/unavailable "
        f"ring={ring}"
    )
    episode = CURRENT_CONTAINMENT_EPISODE.replace(
        "child=4025520345/0005/5104/09000004",
        f"child=4025520345/0005/5104/{result:08x}",
    )
    fault = (
        "wifi: causal_fault id=7 stage=cyw43-transport-init op=0001 "
        "flags=0000 target=00000000 payload=0/0 total=0 detail=5310 "
        "reason=cyw43-transport-bus-link-missing result=06000000"
    )

    assert normalizer.wifi_causal_containment_claim_is_correlated(
        blocker,
        progress,
        episode,
        fault,
    )


@pytest.mark.parametrize(
    "ring",
    [
        None,
        "eff090d9:0006:2000:43595301>eff090d9:0005:5104",
    ],
)
def test_schema_v2_containment_refinement_rejects_historical_ring_forms(
    ring: str | None,
) -> None:
    """Historical compatibility cannot be promoted into containment proof."""

    lines = schema_v2_containment_diag_lines(
        ring,
        CURRENT_CONTAINMENT_EPISODE,
    )
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_CAUSAL_FRONTIER"] == "gate-0/fail/transaction-invalid"


@pytest.mark.parametrize(
    "ring,episode",
    [
        (
            "eff090d9:0006:2000:43595302>eff090d9:0005:5104:09000004",
            CURRENT_CONTAINMENT_EPISODE,
        ),
        (
            CURRENT_CONTAINMENT_RING,
            CURRENT_CONTAINMENT_EPISODE.replace(
                "child=4025520345/",
                "child=4025520346/",
            ),
        ),
        (
            CURRENT_CONTAINMENT_RING,
            CURRENT_CONTAINMENT_EPISODE.replace(
                "exit=4/5310/06000000",
                "exit=4/5310/06000001",
            ),
        ),
    ],
)
def test_schema_v2_containment_refinement_rejects_cross_record_drift(
    ring: str,
    episode: str,
) -> None:
    """Epoch, child, and parent-exit drift invalidate the whole transaction."""

    lines = schema_v2_containment_diag_lines(ring, episode)
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_CAUSAL_FRONTIER"] == "gate-0/fail/transaction-invalid"


@pytest.mark.parametrize(
    "ring,episode",
    [
        (
            "00000000:0006:2000:43595301>00000000:0005:5104:09000004",
            CURRENT_CONTAINMENT_EPISODE,
        ),
        (
            "eff090d9:0006:2000:00000000>eff090d9:0005:5104:09000004",
            CURRENT_CONTAINMENT_EPISODE,
        ),
        (
            CURRENT_CONTAINMENT_RING,
            CURRENT_CONTAINMENT_EPISODE.replace("pub=1 ", "pub=0 "),
        ),
        (
            CURRENT_CONTAINMENT_RING,
            CURRENT_CONTAINMENT_EPISODE.replace("episode=1 ", "episode=0 "),
        ),
        (
            CURRENT_CONTAINMENT_RING,
            CURRENT_CONTAINMENT_EPISODE.replace("phys=1129927425 ", "phys=0 "),
        ),
        (
            CURRENT_CONTAINMENT_RING,
            CURRENT_CONTAINMENT_EPISODE.replace(
                "parent=1414664193/",
                "parent=0/",
            ),
        ),
        (
            CURRENT_CONTAINMENT_RING,
            CURRENT_CONTAINMENT_EPISODE.replace(
                "pub=1 ",
                "pub=4294967296 ",
            ),
        ),
        (
            CURRENT_CONTAINMENT_RING,
            CURRENT_CONTAINMENT_EPISODE.replace(
                "episode=1 ",
                "episode=4294967296 ",
            ),
        ),
        (
            CURRENT_CONTAINMENT_RING,
            CURRENT_CONTAINMENT_EPISODE.replace(
                "phys=1129927425 ",
                "phys=4294967296 ",
            ),
        ),
        (
            CURRENT_CONTAINMENT_RING,
            CURRENT_CONTAINMENT_EPISODE.replace(
                "logical=0 ",
                "logical=4294967296 ",
            ),
        ),
        (
            CURRENT_CONTAINMENT_RING,
            CURRENT_CONTAINMENT_EPISODE.replace(
                "parent=1414664193/",
                "parent=4294967296/",
            ),
        ),
        (
            CURRENT_CONTAINMENT_RING,
            CURRENT_CONTAINMENT_EPISODE.replace(
                "child=4025520345/",
                "child=4294967296/",
            ),
        ),
    ],
)
def test_schema_v2_containment_refinement_rejects_zero_or_overflow_identity(
    ring: str,
    episode: str,
) -> None:
    """Every correlated publication and transaction identity is a nonzero u32."""

    lines = schema_v2_containment_diag_lines(ring, episode)
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_CAUSAL_FRONTIER"] == "gate-0/fail/transaction-invalid"


@pytest.mark.parametrize(
    "fault",
    [
        "wifi: causal_fault id=7 state=none",
        (
            "wifi: causal_fault id=7 stage=cyw43-transport-init op=0001 "
            "flags=0000 target=00000000 payload=0/0 total=0 detail=5310 "
            "reason=cyw43-transport-bus-link-missing result=06000001"
        ),
        (
            "wifi: causal_fault id=7 stage=cyw43-firmware-prep op=0002 "
            "flags=0000 target=00000000 payload=0/0 total=0 detail=5310 "
            "reason=cyw43-transport-bus-link-missing result=06000000"
        ),
    ],
)
def test_schema_v2_containment_refinement_requires_exact_parent_fault(
    fault: str,
) -> None:
    """Absent or contradictory causal-parent rows invalidate refinement."""

    lines = schema_v2_containment_diag_lines(
        CURRENT_CONTAINMENT_RING,
        CURRENT_CONTAINMENT_EPISODE,
        fault=fault,
    )
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_CAUSAL_FRONTIER"] == "gate-0/fail/transaction-invalid"


def test_schema_v2_wifi_diag_rejects_clipped_causal_body() -> None:
    """A terminal marker cannot validate a clipped compact diagnostic body."""

    lines = [
        (
            "wifi: debug subcommand=diag action=begin "
            "profile=bounded-causal mode=one-shot"
        ),
        (
            "wifi: diag_begin id=7 schema=v2 "
            "snapshot=best-effort-multi-record pair=0 gen=0 "
            "source=causal-triage"
        ),
        (
            "wifi: causal_frontier id=7 gate=2 status=no-reply "
            "blocker=sdio-engine-init downstream=not-reached"
        ),
        (
            "wifi: diag_transport id=7 body_lines=7 body_bytes=300 "
            "max_lines=8 max_bytes=2048 backlog_before=0 "
            "wake=bound/badge/polls/hits:yes/8/4/1"
        ),
        (
            "wifi: diag_complete id=7 causal=yes detail=yes schema=v2 "
            "gate=2 status=no-reply blocker=sdio-engine-init "
            "body_lines=8 body_bytes=450"
        ),
        (
            "wifi: debug subcommand=diag action=complete "
            "profile=bounded-causal mode=one-shot result=ok "
            "source=causal-triage"
        ),
    ]

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_CAUSAL_FRONTIER"] == "gate-0/fail/transaction-invalid"
    assert record["WIFI_DIAG_BODY_LINES"] == 8
    assert record["WIFI_DIAG_BODY_BYTES"] == 450


def test_schema_v2_usb_cadence_splits_gap_from_run_duration() -> None:
    """The normalizer must not reinterpret runtime duration as entry gap."""

    lines = [
        (
            "PI4_CADENCE schema=v2 c=usb-local-seat q=2 entry=200 "
            "prev=valid gap=100 run=20 p=7/usb-dma-zero-progress "
            "w=100/500 e=progress f=7"
        )
    ]

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["USB_RUNTIME_CADENCE_PREVIOUS"] == "valid"
    assert record["USB_RUNTIME_CADENCE_GAP_TICKS"] == 0x100
    assert record["USB_RUNTIME_CADENCE_RUN_TICKS"] == 0x20
    assert record["USB_RUNTIME_CADENCE_LINE"] == 1


@pytest.mark.parametrize("complete_position", [1, -1])
def test_current_wifi_diag_rejects_complete_before_transaction_terminal(
    complete_position: int,
) -> None:
    """The command completion cannot precede its retained proof terminal."""

    command_complete = (
        "wifi: debug subcommand=diag action=complete "
        "profile=bounded mode=one-shot result=ok "
        "source=linked-runtime-retained-state"
    )
    transaction = wifi_current_gate7_diag_lines(
        pair_epoch=1,
        generation=9,
        snapshot_sequence=23,
    )
    transaction.insert(complete_position, command_complete)
    lines = [
        *oldgood_wifi_resource_replay_lines(),
        normalizer.WIFI_DIAG_COMMAND_BEGIN_LINE,
        *healthy_wifi_dpc_triplet(generation=11, captures=8),
        *transaction,
    ]
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE"] < 10
    assert record["WIFI_DPC_PROOF"] == "no"
    assert record["WIFI_DPC_REASON"] == "missing"


@pytest.mark.parametrize(
    "later_attempt",
    [
        "CYW43_DRIVER_TASK_JOIN_REQUEST",
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE",
    ],
)
def test_malformed_attempt_boundary_revokes_legacy_gate7(
    later_attempt: str,
) -> None:
    """Legacy ordered proof also fails closed at a malformed new attempt."""

    gate7 = normalizer.summarize_wifi_gate7_proof(
        normalizer.parse_events(
            [*oldgood_wifi_resource_replay_lines(), later_attempt]
        )
    )

    assert not gate7.complete
    assert gate7.missing == "7a"


def test_current_association_supervisor_join_requires_bounded_generation() -> None:
    """The current production Join grammar carries an exact nonzero u32 epoch."""

    def matches(generation: int) -> bool:
        event = normalizer.parse_events(
            [
                "CYW43_DRIVER_TASK_JOIN_REQUEST contract=cyw43455 "
                "path=association-supervisor action=ready "
                f"generation={generation} ssid_len=7 result=0x00000000"
            ]
        )[0]
        return normalizer.wifi_join_request_step(event)

    assert matches(37)
    assert not matches(0)
    assert not matches(normalizer.U32_MAX + 1)


def test_retained_gate7_rejects_standalone_or_prior_truncated_row() -> None:
    """A retained row cannot float into a later diagnostic transaction."""

    standalone = normalizer.summarize_wifi_gate7_proof(
        normalizer.parse_events([wifi_gate7_retained_line(1, 2, 3)])
    )
    assert not standalone.complete
    assert standalone.missing == "retained-diag-complete"

    lines = [
        wifi_diag_begin_line(1, 2, 3),
        wifi_gate7_retained_line(1, 2, 3),
        wifi_diag_begin_line(2, 2, 3),
        *[
            wifi_gate8_subgate_line(subgate, pair_epoch=2, generation=3)
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        wifi_diag_complete_line(2, 2, 3),
    ]
    proof = normalizer.summarize_wifi_gate7_proof(
        normalizer.parse_events(lines)
    )

    assert not proof.complete
    assert proof.missing == "retained-current"


def test_legacy_diag_summary_keeps_explicit_gate7_history_compatible() -> None:
    """A pre-id summary does not suppress canonical transient Gate 7 proof."""

    lines = [
        *oldgood_wifi_resource_replay_lines(),
        "wifi: diag_complete causal=yes detail=yes scope=current "
        "frontier=complete status=pass blocker=none",
    ]

    proof = normalizer.summarize_wifi_gate7_proof(
        normalizer.parse_events(lines)
    )

    assert proof.complete
    assert not proof.retained_current
    assert proof.seen == "7a>7b>7c>7d>7e"


@pytest.mark.parametrize(
    "mutation",
    [
        ("data=yes", "data=no"),
        ("eapol=yes", "eapol=no"),
        ("m3=yes", "m3=no"),
        ("pair=23", "pair=24"),
        ("snapshot=current", "snapshot=current extra=yes"),
    ],
)
def test_retained_gate7_full_match_fails_closed(
    mutation: tuple[str, str],
) -> None:
    """Impossible, mismatched, or extended retained grammars cannot pass."""

    retained = wifi_gate7_retained_line(17, 23, 29).replace(*mutation)
    lines = [
        wifi_diag_begin_line(17, 23, 29),
        retained,
        *[
            wifi_gate8_subgate_line(subgate, pair_epoch=23, generation=29)
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        wifi_diag_complete_line(17, 23, 29),
    ]

    proof = normalizer.summarize_wifi_gate7_proof(
        normalizer.parse_events(lines)
    )

    assert not proof.complete
    assert proof.missing == "retained-current-invalid"


def wifi_gate8_subgate_line(
    subgate: str,
    *,
    status: str = "pass",
    pair_epoch: int = 0,
    generation: int = 0,
    blocker: str = "none",
) -> str:
    """Return one canonical Gate 8 stabilization record."""

    return (
        f"wifi: gate 8 subgate={subgate} status={status} "
        f"pair_epoch={pair_epoch} "
        f"generation={generation} blocker={blocker}"
    )


def wifi_diag_begin_line(
    snapshot_sequence: int, pair_epoch: int, generation: int
) -> str:
    """Return one exact current WiFi diagnostic transaction opener."""

    return (
        f"wifi: diag_begin id={snapshot_sequence} pair_epoch={pair_epoch} "
        f"generation={generation} snapshot=current"
    )


def wifi_gate7_retained_line(
    snapshot_sequence: int,
    pair_epoch: int,
    generation: int,
) -> str:
    """Return one exact compact retained host-EAPOL receipt."""

    return (
        f"wifi: gate7_retained id={snapshot_sequence} src=sm status=pass "
        f"h=7a>7b>7c>7d>7e pair={pair_epoch} gen={generation} "
        "assoc=yes link=yes eapol=yes data=yes m1=yes m2=yes m3=yes "
        "m4=yes ptk=yes gtk=yes keys=yes secure=yes snapshot=current"
    )


def wifi_diag_complete_line(
    snapshot_sequence: int,
    pair_epoch: int,
    generation: int,
    *,
    detail: str = "yes",
    scope: str = "current",
    frontier: str = "complete",
    status: str = "pass",
    blocker: str = "none",
) -> str:
    """Return one exact current WiFi diagnostic terminal summary."""

    return (
        f"wifi: diag_complete id={snapshot_sequence} causal=yes detail={detail} "
        f"scope={scope} snapshot=current pair={pair_epoch} gen={generation} "
        f"front={frontier} status={status} block={blocker}"
    )


def wifi_gate8_snapshot_lines(
    passed: int,
    *,
    pair_epoch: int = 0,
    generation: int = 0,
    status: str = "pending",
    blocker: str = "stabilization-pending",
) -> list[str]:
    """Return one complete ordered Gate 8 diagnostic snapshot."""

    lines: list[str] = []
    for index, subgate in enumerate(normalizer.WIFI_GATE8_SUBGATES):
        if index < passed:
            lines.append(
                wifi_gate8_subgate_line(
                    subgate,
                    pair_epoch=pair_epoch,
                    generation=generation,
                )
            )
        elif index == passed:
            lines.append(
                wifi_gate8_subgate_line(
                    subgate,
                    status=status,
                    pair_epoch=pair_epoch,
                    generation=generation,
                    blocker=blocker,
                )
            )
        else:
            lines.append(
                wifi_gate8_subgate_line(
                    subgate,
                    status="pending",
                    pair_epoch=pair_epoch,
                    generation=generation,
                    blocker=normalizer.WIFI_GATE8_SUBGATES[index - 1],
                )
            )
    return lines


def wifi_gate8_transaction_lines(
    *,
    pair_epoch: int = 0,
    generation: int = 0,
    attempt: int = 1,
) -> list[str]:
    """Return one exact Begin/Stabilizing/8a-8h/Ready transaction."""

    return [
        bootstrap_supervisor_line(attempt, "begin", 0, 100, 1),
        bootstrap_supervisor_line(attempt, "stabilizing", 0, 150, 2),
        *wifi_gate8_snapshot_lines(
            len(normalizer.WIFI_GATE8_SUBGATES),
            pair_epoch=pair_epoch,
            generation=generation,
        ),
        bootstrap_supervisor_line(attempt, "ready", 0, 200, 3),
    ]


def wifi_current_gate7_diag_lines(
    *,
    pair_epoch: int,
    generation: int,
    snapshot_sequence: int = 1,
) -> list[str]:
    """Return one exact retained Gate 7 plus current Gate 8 diagnostic."""

    return [
        wifi_diag_begin_line(snapshot_sequence, pair_epoch, generation),
        wifi_gate7_retained_line(snapshot_sequence, pair_epoch, generation),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=pair_epoch,
                generation=generation,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        wifi_diag_complete_line(snapshot_sequence, pair_epoch, generation),
    ]


def wifi_gate8_with_current_gate7_lines(
    *,
    pair_epoch: int,
    generation: int,
) -> list[str]:
    """Return a Ready transaction followed by its current retained proof."""

    return [
        *wifi_gate8_transaction_lines(
            pair_epoch=pair_epoch,
            generation=generation,
        ),
        *wifi_current_gate7_diag_lines(
            pair_epoch=pair_epoch,
            generation=generation,
        ),
    ]


def wifi_generation_gate10_lines(
    generation: int,
    *,
    run_generation: int = 1,
    selftest_result: str = "peer-assisted-pass",
    tcp_counters: bool = True,
    auth_generation: int | None = None,
    dpc_generation: int | None = None,
) -> list[str]:
    """Return pair-generation evidence plus the independent DPC link epoch."""

    tcp_accepts = 1 if tcp_counters else 0
    tcp_auth = 1 if tcp_counters else 0
    tcp_rx_bytes = 58 if tcp_counters else 0
    resolved_dpc_generation = (
        dpc_generation if dpc_generation is not None else 0x4359_5301
    )
    lines = [
        "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
        "server=192.168.10.1 lease_s=3600",
    ]
    if selftest_result == "started-only":
        lines.extend(
            [
                "OK NETTEST detail=started "
                f"run_generation={run_generation}",
                f"nettest: generation={generation} "
                f"run_generation={run_generation} enabled=true running=true "
                "verdict=running tx_ok=na udp_echo_ok=na tcp_ok=na "
                "console_ok=na peer_assisted_ok=na",
            ]
        )
    else:
        lines.extend(
            [
                f"[net-selftest] result generation={generation} "
                f"run_generation={run_generation} tx_ok=true "
                "udp_echo_ok=false tcp_ok=false console_ok=true "
                f"peer_assisted_ok=true result={selftest_result}",
                "OK NETTEST detail=started "
                f"run_generation={run_generation}",
                f"nettest: generation={generation} "
                f"run_generation={run_generation} enabled=true running=false "
                f"verdict={selftest_result} tx_ok=true udp_echo_ok=false "
                "tcp_ok=false console_ok=true peer_assisted_ok=true",
            ]
        )
    lines.extend(
        [
            "netstats: rx_pkts=4 tx_pkts=9 rx_used=4 tx_used=9 polls=30",
            f"netstats: generation={generation} udp_rx=2 udp_tx=4 "
            f"tcp_accepts={tcp_accepts} tcp_auth={tcp_auth} "
            f"tcp_rx_bytes={tcp_rx_bytes} tcp_tx_bytes=6782",
            "netstats: mode=dhcp policy=wifi active=wifi standby=wired "
            "addr_src=dhcp-lease ip=192.168.10.50 gateway=192.168.10.1 "
            "dhcp=bound",
            "netstats: wifi_assoc=1 wifi_link=1 eapol_rx=2 "
            "eapol_start=1 eapol_secure=1",
            f"netstatus: generation={generation} ip=192.168.10.50 "
            "gateway=192.168.10.1 src=dhcp-lease dhcp=bound tcp_ready=yes",
            "[cohsh-net][auth] auth OK, session established "
            f"(generation={auth_generation if auth_generation is not None else generation} "
            "conn_id=1)",
        ]
    )
    lines.extend(healthy_wifi_dpc_triplet(generation=resolved_dpc_generation))
    return lines


def test_gate8_ordered_proof_accepts_all_subgates_in_generation_zero() -> None:
    """The initial CYW43 epoch is valid when all eight stages pass in order."""

    lines = wifi_gate8_transaction_lines()

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "yes"
    assert record["WIFI_GATE8_SEEN"] == ">".join(
        normalizer.WIFI_GATE8_SUBGATES
    )
    assert record["WIFI_GATE8_LAST"] == "8h-data-admission"
    assert record["WIFI_GATE8_MISSING"] == "none"
    assert record["WIFI_GATE8_STATUS"] == "pass"
    assert record["WIFI_GATE8_GENERATION"] == 0
    assert record["WIFI_GATE8_BLOCKER"] == "none"
    assert record["WIFI_GATE8_LINE"] == 10
    assert record["WIFI_GATE"] == 8
    assert record["WIFI_BLOCKER"] == "none"
    assert record["WIFI_SUBGATE"] == "8h-data-admission"
    assert record["WIFI_SUBGATE_SOURCE"] == "gate8-stability"


def test_gate8_proof_keeps_pair_epoch_distinct_from_connection_generation() -> None:
    """One pair/control epoch may legally cover a later Join generation."""

    lines = wifi_gate8_transaction_lines(
        pair_epoch=7,
        generation=23,
    )

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "yes"
    assert record["WIFI_GATE8_PAIR_EPOCH"] == 7
    assert record["WIFI_GATE8_GENERATION"] == 23


def test_gate8_transaction_rejects_nonatomic_snapshot_and_nonadjacent_ready() -> None:
    """Neither interleaved sub-gates nor a detached Ready record are authority."""

    nonatomic = wifi_gate8_transaction_lines(generation=9)
    nonatomic.insert(4, "wifi: snapshot source=live stage=interleaved")
    nonatomic_record = normalizer.summarize_gates(
        normalizer.parse_events(nonatomic)
    ).to_record()

    detached_ready = wifi_gate8_transaction_lines(generation=9)
    detached_ready.insert(-1, "wifi: snapshot source=live stage=interleaved")
    detached_record = normalizer.summarize_gates(
        normalizer.parse_events(detached_ready)
    ).to_record()

    assert nonatomic_record["WIFI_GATE8_COMPLETE"] == "no"
    assert nonatomic_record["WIFI_GATE8_BLOCKER"] == "snapshot-not-atomic"
    assert detached_record["WIFI_GATE8_COMPLETE"] == "no"
    assert detached_record["WIFI_GATE8_BLOCKER"] == "gate8-ready-not-adjacent"


@pytest.mark.parametrize(
    ("boundary", "expected_blocker"),
    [
        (
            "CYW43_GATE8_RECOVERY attempt=1 generation=9 "
            "blocker=carrier-lost deadline_ms=250 action=pair-restart",
            "gate8-recovery",
        ),
        (
            "CYW43_GATE8_READY_RETRACTED attempt=1 generation=9 "
            "deadline_ms=250 action=fresh-proof",
            "gate8-ready-retracted",
        ),
        (
            "CYW43_GATE8_READY_TRANSACTION status=failed "
            "action=retract-and-retry-fresh-snapshot",
            "gate8-publication-rejected",
        ),
        (
            "CYW43_GATE8_SNAPSHOT_COMMIT status=rejected "
            "action=retry-fresh-snapshot",
            "gate8-publication-rejected",
        ),
    ],
)
def test_gate8_authority_boundaries_revoke_ready(
    boundary: str,
    expected_blocker: str,
) -> None:
    """Recovery, retraction, and publication failure revoke accepted Ready."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(
            [*wifi_gate8_transaction_lines(generation=9), boundary]
        )
    ).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_BLOCKER"] == expected_blocker


def test_wifi_gate10_requires_post_ready_matching_generation() -> None:
    """Pre-Ready and stale-generation network proof cannot satisfy Gate 9/10."""

    current = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                *wifi_gate8_with_current_gate7_lines(
                    pair_epoch=1,
                    generation=9,
                ),
                *wifi_generation_gate10_lines(9),
            ]
        )
    ).to_record()
    pre_ready = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                *wifi_generation_gate10_lines(9),
                *wifi_gate8_with_current_gate7_lines(
                    pair_epoch=1,
                    generation=9,
                ),
            ]
        )
    ).to_record()
    stale = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                *wifi_gate8_with_current_gate7_lines(
                    pair_epoch=1,
                    generation=9,
                ),
                *wifi_generation_gate10_lines(8),
            ]
        )
    ).to_record()

    assert current["WIFI_GATE"] == 10
    assert current["NETTEST_PROOF"] == "yes"
    assert current["COHSH_TCP_AUTH_PROOF"] == "yes"
    assert current["WIFI_DPC_PROOF"] == "yes"
    assert pre_ready["WIFI_GATE"] == 8
    assert pre_ready["WIFI_BLOCKER"] == "dhcp-generation-proof-missing"
    assert stale["WIFI_GATE"] == 8
    assert stale["WIFI_BLOCKER"] == "dhcp-generation-proof-missing"


def test_wifi_gate10_rejects_started_only_nettest_and_stale_auth() -> None:
    """Started-only nettest and stale TCP-auth generations remain Gate-9 red."""

    started = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                *wifi_gate8_with_current_gate7_lines(
                    pair_epoch=1,
                    generation=9,
                ),
                *wifi_generation_gate10_lines(9, selftest_result="started-only"),
            ]
        )
    ).to_record()
    stale_auth = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                *wifi_gate8_with_current_gate7_lines(
                    pair_epoch=1,
                    generation=9,
                ),
                *wifi_generation_gate10_lines(
                    9,
                    tcp_counters=False,
                    auth_generation=8,
                ),
            ]
        )
    ).to_record()
    independent_dpc_epoch = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                *wifi_gate8_with_current_gate7_lines(
                    pair_epoch=1,
                    generation=9,
                ),
                *wifi_generation_gate10_lines(9, dpc_generation=8),
            ]
        )
    ).to_record()

    assert started["WIFI_GATE"] == 9
    assert started["NETTEST_PROOF"] == "no"
    assert stale_auth["WIFI_GATE"] == 9
    assert stale_auth["COHSH_TCP_AUTH_PROOF"] == "no"
    assert independent_dpc_epoch["WIFI_GATE"] == 10
    assert independent_dpc_epoch["WIFI_DPC_PROOF"] == "yes"
    assert independent_dpc_epoch["WIFI_DPC_GENERATION"] == 8


def test_wifi_gate10_rejects_internal_async_result_without_compact_status() -> None:
    """Internal self-test completion cannot replace target-visible final status."""

    async_only = [
        line
        for line in wifi_generation_gate10_lines(9)
        if not line.startswith("nettest:")
    ]
    record = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                *wifi_gate8_transaction_lines(generation=9),
                *async_only,
            ]
        )
    ).to_record()

    assert record["WIFI_GATE"] != 10
    assert record["NETTEST_PROOF"] == "no"


def test_nettest_compact_status_is_target_visible_terminal_proof() -> None:
    """The final untruncated ``netstats`` verdict does not require debug logs."""

    successful = normalizer.parse_events(
        [
            "nettest: generation=9 run_generation=3 enabled=true "
            "running=false verdict=peer-assisted-pass "
            "tx_ok=true udp_echo_ok=false "
            "tcp_ok=false console_ok=true peer_assisted_ok=true"
        ]
    )
    running = normalizer.parse_events(
        [
            "nettest: generation=9 run_generation=3 enabled=true "
            "running=true verdict=running "
            "tx_ok=na udp_echo_ok=na tcp_ok=na "
            "console_ok=na peer_assisted_ok=na"
        ]
    )
    disabled = normalizer.parse_events(
        [
            "nettest: generation=0 run_generation=0 enabled=false "
            "running=false verdict=none "
            "tx_ok=na udp_echo_ok=na tcp_ok=na "
            "console_ok=na peer_assisted_ok=na"
        ]
    )
    enabled_quiescent = normalizer.parse_events(
        [
            "nettest: generation=0 run_generation=0 enabled=true "
            "running=false verdict=none "
            "tx_ok=na udp_echo_ok=na tcp_ok=na "
            "console_ok=na peer_assisted_ok=na"
        ]
    )

    assert normalizer.summarize_net_state(successful)[4]
    assert not normalizer.summarize_net_state(running)[4]
    assert normalizer.parse_wifi_nettest_status(disabled[0]) is not None
    assert not normalizer.summarize_net_state(disabled)[4]
    assert (
        normalizer.parse_wifi_nettest_status(enabled_quiescent[0]) is not None
    )
    assert not normalizer.summarize_net_state(enabled_quiescent)[4]


@pytest.mark.parametrize(
    "line",
    (
        (
            "nettest: generation=9 run_generation=3 enabled=true "
            "running=false verdict=peer-assisted-pass tx_ok=true [truncated]"
        ),
        (
            "nettest: generation=9 run_generation=3 enabled=true "
            "running=false verdict=peer-assisted-pass tx_ok=true "
            "udp_echo_ok=false tcp_ok=false console_ok=true"
        ),
        (
            "nettest: generation=9 run_generation=3 enabled=true "
            "running=false verdict=pass tx_ok=true udp_echo_ok=false "
            "tcp_ok=true console_ok=true peer_assisted_ok=false"
        ),
        (
            "nettest: generation=9 run_generation=3 enabled=false "
            "running=false verdict=pass tx_ok=true udp_echo_ok=true "
            "tcp_ok=true console_ok=true peer_assisted_ok=false"
        ),
        (
            "nettest: generation=9 enabled=true running=false "
            "verdict=pass tx_ok=true udp_echo_ok=true tcp_ok=true "
            "console_ok=true peer_assisted_ok=false"
        ),
        (
            "nettest: generation=9 run_generation=0 enabled=true "
            "running=false verdict=pass tx_ok=true udp_echo_ok=true "
            "tcp_ok=true console_ok=true peer_assisted_ok=false"
        ),
    ),
)
def test_nettest_compact_status_rejects_invalid_proof(line: str) -> None:
    """Malformed or internally inconsistent compact status fails closed."""

    events = normalizer.parse_events([line])

    assert len(events) == 1
    assert normalizer.parse_wifi_nettest_status(events[0]) is None
    assert not normalizer.summarize_net_state(events)[4]


def test_gate8_proof_rejects_pair_epoch_stitching_inside_one_snapshot() -> None:
    """All 8a-8h records must name one immutable linked-pair epoch."""

    lines = wifi_gate8_snapshot_lines(
        len(normalizer.WIFI_GATE8_SUBGATES),
        pair_epoch=4,
        generation=9,
    )
    lines[5] = wifi_gate8_subgate_line(
        normalizer.WIFI_GATE8_SUBGATES[5],
        pair_epoch=5,
        generation=9,
    )

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_STATUS"] == "fail"
    assert record["WIFI_GATE8_BLOCKER"] == "pair-epoch-mismatch"
    assert record["WIFI_GATE8_LINE"] == 6


def test_gate8_ordered_proof_reports_pending_current_frontier() -> None:
    """Pending work is telemetry, not a completed or terminal Gate 8 proof."""

    lines = wifi_gate8_snapshot_lines(
        2,
        generation=14,
        blocker="join-owner-pending",
    )

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert (
        record["WIFI_GATE8_SEEN"]
        == "8a-pair-generation>8b-control-program"
    )
    assert record["WIFI_GATE8_LAST"] == "8b-control-program"
    assert record["WIFI_GATE8_MISSING"] == "8c-join-terminal"
    assert record["WIFI_GATE8_STATUS"] == "pending"
    assert record["WIFI_GATE8_GENERATION"] == 14
    assert record["WIFI_GATE8_BLOCKER"] == "join-owner-pending"
    assert record["WIFI_GATE8_LINE"] == 3
    assert record["WIFI_GATE"] == 7
    assert record["WIFI_BLOCKER"] == "join-owner-pending"
    assert record["WIFI_EXACT"] == "join-owner-pending"
    assert record["WIFI_PHASE"] == "gate8-stabilizing"
    assert record["WIFI_SUBGATE"] == "8c-join-terminal"
    assert record["WIFI_SUBGATE_STATUS"] == "pending"


def test_gate8_ordered_proof_reports_terminal_current_frontier() -> None:
    """A terminal sub-gate failure remains distinct from pending work."""

    lines = wifi_gate8_snapshot_lines(
        4,
        status="fail",
        generation=22,
        blocker="bssid-owner-terminal-failure",
    )

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_MISSING"] == "8e-bssid-refresh"
    assert record["WIFI_GATE8_STATUS"] == "fail"
    assert record["WIFI_GATE8_GENERATION"] == 22
    assert (
        record["WIFI_GATE8_BLOCKER"]
        == "bssid-owner-terminal-failure"
    )
    assert record["WIFI_GATE8_LINE"] == 5


def test_gate8_ordered_proof_never_stitches_recovery_generations() -> None:
    """A higher recovery epoch resets every earlier Gate 8 pass."""

    lines = [
        *wifi_gate8_snapshot_lines(
            4,
            generation=0,
            blocker="bssid-refresh-pending",
        ),
        *wifi_gate8_snapshot_lines(
            1,
            generation=1,
            blocker="control-program-pending",
        ),
    ]

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_SEEN"] == "8a-pair-generation"
    assert record["WIFI_GATE8_LAST"] == "8a-pair-generation"
    assert record["WIFI_GATE8_MISSING"] == "8b-control-program"
    assert record["WIFI_GATE8_GENERATION"] == 1
    assert record["WIFI_GATE8_BLOCKER"] == "control-program-pending"


def test_gate8_ordered_proof_rejects_mixed_snapshot_generations() -> None:
    """Every record in one Gate 8 snapshot must carry the same epoch."""

    lines = wifi_gate8_snapshot_lines(
        len(normalizer.WIFI_GATE8_SUBGATES),
        generation=7,
    )
    lines[4] = lines[4].replace("generation=7", "generation=8")

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_MISSING"] == "8e-bssid-refresh"
    assert record["WIFI_GATE8_STATUS"] == "fail"
    assert record["WIFI_GATE8_BLOCKER"] == "generation-mismatch"
    assert record["WIFI_GATE8_LINE"] == 5


def test_gate8_ordered_proof_rejects_subgate_order_gap() -> None:
    """A later pass cannot conceal a missing same-generation predecessor."""

    lines = wifi_gate8_snapshot_lines(
        len(normalizer.WIFI_GATE8_SUBGATES),
        generation=7,
    )
    lines[1], lines[2] = lines[2], lines[1]

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_SEEN"] == "8a-pair-generation"
    assert record["WIFI_GATE8_MISSING"] == "8b-control-program"
    assert record["WIFI_GATE8_STATUS"] == "fail"
    assert record["WIFI_GATE8_BLOCKER"] == "subgate-order-gap"
    assert record["WIFI_GATE"] == 7
    assert record["WIFI_BLOCKER"] == "subgate-order-gap"


def test_boot_acceptance_requires_complete_ordered_wifi_gate8_proof() -> None:
    """Observed Gate 8 telemetry must close all same-generation sub-gates."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(oldgood_wifi_resource_replay_lines())
    ).to_record()
    record["WIFI_GATE8_COMPLETE"] = "no"
    record["WIFI_GATE8_STATUS"] = "pending"
    record["WIFI_GATE8_MISSING"] = "8f-eapol-keys"

    blockers = normalizer.boot_evidence_blockers(record)

    assert "wifi-gate8-subgates-incomplete" in blockers
    assert "wifi-gate8-8f-eapol-keys-missing" in blockers


def test_boot_acceptance_requires_wifi_bootstrap_supervisor_terminal() -> None:
    """Production WiFi evidence must include a ready bootstrap supervisor."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(oldgood_wifi_resource_replay_lines())
    ).to_record()

    assert record["NET_ACTIVE"] == "wifi"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "yes"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == "none"
    clean_blockers = normalizer.boot_evidence_blockers(record)
    assert "cyw43-bootstrap-supervisor-missing" not in clean_blockers
    assert "cyw43-bootstrap-supervisor-not-first-attempt" not in clean_blockers
    assert "cyw43-bootstrap-supervisor-transient-retries" not in clean_blockers
    assert "cyw43-bootstrap-supervisor-recovery" not in clean_blockers

    missing_counters = dict(record)
    missing_counters.pop("CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES")
    missing_counters.pop("CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES")
    missing_counter_blockers = normalizer.boot_evidence_blockers(
        missing_counters
    )
    assert (
        "cyw43-bootstrap-supervisor-transient-retries-missing"
        in missing_counter_blockers
    )
    assert (
        "cyw43-bootstrap-supervisor-recoveries-missing"
        in missing_counter_blockers
    )

    record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] = 2
    record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] = 1
    record["CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES"] = 1
    retry_blockers = normalizer.boot_evidence_blockers(record)
    assert "cyw43-bootstrap-supervisor-not-first-attempt" in retry_blockers
    assert "cyw43-bootstrap-supervisor-transient-retries" in retry_blockers
    assert "cyw43-bootstrap-supervisor-recovery" in retry_blockers

    record["CYW43_BOOTSTRAP_SUPERVISOR_SEEN"] = "no"
    assert "cyw43-bootstrap-supervisor-missing" in (
        normalizer.boot_evidence_blockers(record)
    )

    record["CYW43_BOOTSTRAP_SUPERVISOR_SEEN"] = "yes"
    record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] = "no"
    record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] = "none"
    assert "cyw43-bootstrap-supervisor-ready-missing" in (
        normalizer.boot_evidence_blockers(record)
    )


def test_gate_summary_accepts_linked_runtime_wifi_harness_replay_contract() -> None:
    events = normalizer.parse_events(linked_runtime_wifi_harness_replay_lines())

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert gates.wifi_gate == 10
    assert gates.wifi_blocker == "none"
    assert record["WIFI_OLDGOOD_REPLAY"] == "yes"
    assert record["WIFI_OLDGOOD_LAST"] == "dpc-healthy-after-tcp"
    assert record["WIFI_OLDGOOD_MISSING"] == "none"


def test_gate_summary_accepts_pi4_hardware_wifi_gate7_to_10_capture_contract() -> None:
    events = normalizer.parse_events(pi4_hardware_wifi_gate7_to_10_capture_lines())

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert gates.wifi_gate == 10
    assert gates.wifi_blocker == "none"
    assert record["WIFI_OLDGOOD_REPLAY"] == "yes"
    assert record["WIFI_OLDGOOD_LAST"] == "dpc-healthy-after-tcp"
    assert record["WIFI_OLDGOOD_MISSING"] == "none"


def test_wifi_oldgood_replay_does_not_stitch_across_supervisor_recovery() -> None:
    """A later retry cannot consume ordered steps from an earlier attempt."""

    lines = oldgood_wifi_resource_replay_lines()
    stabilizing_index = oldgood_wifi_line_index(
        lines,
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=stabilizing",
    )
    final_ready_index = oldgood_wifi_line_index(
        lines,
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=ready",
    )
    lines[stabilizing_index] = (
        lines[stabilizing_index]
        .replace("attempt=1", "attempt=2", 1)
        .replace("next_attempt_ms=150", "next_attempt_ms=1200", 1)
        .replace("console_seq=2", "console_seq=6", 1)
    )
    lines[final_ready_index] = (
        lines[final_ready_index]
        .replace("attempt=1", "attempt=2", 1)
        .replace("next_attempt_ms=200", "next_attempt_ms=1300", 1)
        .replace("console_seq=3", "console_seq=7", 1)
    )
    join_index = oldgood_wifi_line_index(lines, "CYW43_DRIVER_TASK_JOIN_REQUEST")
    lines[join_index:join_index] = [
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=stabilizing backoff_ms=0 "
        "next_attempt_ms=150 serial=ready local_seat=enabled recovery=full "
        "console_seq=2 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes",
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=recovery backoff_ms=0 "
        "next_attempt_ms=160 serial=ready local_seat=enabled recovery=full "
        "console_seq=3 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes",
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=backoff backoff_ms=1000 "
        "next_attempt_ms=1160 serial=ready local_seat=enabled recovery=full "
        "console_seq=4 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes",
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=2 status=recovery backoff_ms=0 "
        "next_attempt_ms=1160 serial=ready local_seat=enabled recovery=full "
        "console_seq=5 telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes",
    ]

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "outer-backoff-forbidden"
    )
    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_LAST"] == "none"
    assert record["WIFI_OLDGOOD_MISSING"] == "sdio-engine-ready"


def test_gate_summary_requires_oldgood_txglomalign_control_step() -> None:
    lines = [
        line
        for line in oldgood_wifi_replay_lines()
        if "cyw43-control-txglomalign" not in line
    ]

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_LAST"] == "function2-ready"
    assert record["WIFI_OLDGOOD_MISSING"] == "control-txglom-pre-tx-drain-ready"


def test_gate_summary_requires_oldgood_txglom_tx_after_pre_tx_drain() -> None:
    lines = [
        line
        for line in oldgood_wifi_replay_lines()
        if "event=tx-complete" not in line
    ]

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_LAST"] == "control-txglom-pre-tx-drain-ready"
    assert record["WIFI_OLDGOOD_MISSING"] == "control-txglom-tx-complete"


def test_gate_summary_requires_oldgood_ulp_sdioctrl_control_step() -> None:
    lines = [
        line
        for line in oldgood_wifi_replay_lines()
        if "cyw43-control-ulp-sdioctrl" not in line
    ]

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_LAST"] == "control-txglomalign"
    assert record["WIFI_OLDGOOD_MISSING"] == "control-ulp-sdioctrl"


def test_gate_summary_requires_oldgood_cur_etheraddr_control_step() -> None:
    lines = [
        line
        for line in oldgood_wifi_replay_lines()
        if "cyw43-control-cur-etheraddr" not in line
    ]

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_LAST"] == "control-rxglom"
    assert record["WIFI_OLDGOOD_MISSING"] == "control-cur-etheraddr"


def test_gate_summary_rejects_legacy_only_function2_ready_as_oldgood() -> None:
    lines = [
        line
        for line in oldgood_wifi_replay_lines()
        if "stage=cyw43-function2 status=ready" not in line
    ]

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_LAST"] == "firmware-ready"
    assert record["WIFI_OLDGOOD_MISSING"] == "function2-ready"


def test_gate_summary_accepts_normalized_nvram_upload_identity() -> None:
    """Firmware identity uses normalized NVRAM upload length plus raw file hash."""

    events = normalizer.parse_events(
        [
            "wifi: firmware_contract fw=609309 nvram=1744 clm=2676 "
            f"fw_hash={normalizer.CYW43_CAPTURE_FIRMWARE_SHA256} "
            f"nvram_hash={normalizer.CYW43_CAPTURE_NVRAM_SHA256} "
            f"clm_hash={normalizer.CYW43_CAPTURE_CLM_SHA256} "
            "board=raspberrypi,4-model-b rstvec=0xb83ef198 verified=yes "
            "armcr4_release=1 sr_kso=yes current_clock=41666666Hz "
            "preferred=41666666Hz",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_FIRMWARE_IDENTITY_PROOF"] == "yes"
    assert record["WIFI_FIRMWARE_IDENTITY_BLOCKER"] == "none"


def test_gate_summary_rejects_raw_nvram_len_as_upload_identity() -> None:
    """The contract's nvram field reports normalized upload bytes, not raw size."""

    events = normalizer.parse_events(
        [
            "wifi: firmware_contract fw=609309 nvram=2074 clm=2676 "
            f"fw_hash={normalizer.CYW43_CAPTURE_FIRMWARE_SHA256} "
            f"nvram_hash={normalizer.CYW43_CAPTURE_NVRAM_SHA256} "
            f"clm_hash={normalizer.CYW43_CAPTURE_CLM_SHA256}",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_FIRMWARE_IDENTITY_PROOF"] == "no"
    assert record["WIFI_FIRMWARE_IDENTITY_BLOCKER"] == "nvram-upload-len"


def test_gate_summary_rejects_streamed_firmware_upload_without_identity_hashes() -> None:
    """A complete firmware stream still needs exact bundle identity fields."""

    events = normalizer.parse_events(
        seal_driver_task_runtime_descriptor_lines(
            [
                "DRIVER_TASK_OWNER_STATE contract=cyw43455 hot_path=cyw43-wifi "
                "owner_state=driver-owned descriptor=present root_pointer=no",
                "DRIVER_TASK_OWNER_STATE contract=sdio-host hot_path=sdio-host "
                "owner_state=driver-owned descriptor=present root_pointer=no",
                "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host stage=engine-init "
                "blocker=ready detail=0x5500",
                "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
                "hot_path=cyw43-wifi stage=net-engine-init status=ready",
                "CYW43_DRIVER_TASK_STREAM_PROGRESS contract=cyw43455 "
                "stage=cyw43-firmware-chunk uploaded=557056 total_len=609309 "
                "target=0x0021e000 chunk_len=8192",
                "CYW43_DRIVER_TASK_STREAM_PROGRESS contract=cyw43455 "
                "stage=cyw43-firmware-chunk uploaded=609309 total_len=609309 "
                "target=0x0022c000 chunk_len=3101",
                "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
                "hot_path=cyw43-wifi stage=cyw43-firmware-release status=ready",
                "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
                "hot_path=cyw43-wifi stage=cyw43-firmware status=ready",
                "DRIVER_TASK_RESOURCE_INIT contract=cyw43455 "
                "hot_path=cyw43-wifi stage=cyw43-function2 status=ready",
            ]
        )
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_FIRMWARE_IDENTITY_PROOF"] == "no"
    assert record["WIFI_FIRMWARE_IDENTITY_BLOCKER"] == "firmware-identity-line-missing"
    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_LAST"] == "none"
    assert record["WIFI_OLDGOOD_MISSING"] == "firmware-identity"


def test_gate_summary_rejects_incomplete_streamed_firmware_upload_identity() -> None:
    """Partial firmware streams must not satisfy the identity prerequisite."""

    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_STREAM_PROGRESS contract=cyw43455 "
            "stage=cyw43-firmware-chunk uploaded=557056 total_len=609309 "
            "target=0x0021e000 chunk_len=8192",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_FIRMWARE_IDENTITY_PROOF"] == "no"
    assert record["WIFI_FIRMWARE_IDENTITY_BLOCKER"] == "firmware-upload-incomplete"


def test_gate_summary_rejects_failed_function2_as_wifi_oldgood_replay() -> None:
    lines = [
        line.replace(
            "stage=cyw43-function2 status=ready",
            "stage=cyw43-function2 status=inferred f2_ready=no",
        )
        for line in oldgood_wifi_resource_replay_lines()
    ]
    lines.append("wifi: gate 7 name=function2-ready status=inferred f2_ready=no")
    events = normalizer.parse_events(lines)

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_LAST"] == "firmware-ready"
    assert record["WIFI_OLDGOOD_MISSING"] == "function2-ready"


def test_gate_summary_rejects_nonzero_join_result_as_wifi_oldgood_replay() -> None:
    lines = [
        line.replace("result=0x00000000", "result=0xffffffff")
        if "CYW43_DRIVER_TASK_JOIN_REQUEST" in line
        else line
        for line in oldgood_wifi_resource_replay_lines()
    ]
    events = normalizer.parse_events(lines)

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_LAST"] == "control-up"
    assert record["WIFI_OLDGOOD_MISSING"] == "join-request"


def test_gate_summary_rejects_generic_eapol_message_as_wifi_oldgood_replay() -> None:
    lines = [
        "wifi: rx msg=m1 ethertype=0x888e" if "msg=m1" in line else line
        for line in oldgood_wifi_resource_replay_lines()
    ]
    events = normalizer.parse_events(lines)

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_LAST"] == "association-link"
    assert record["WIFI_OLDGOOD_MISSING"] == "host-eapol-m1"


def test_gate_summary_rejects_nonpass_nettest_as_wifi_oldgood_replay() -> None:
    lines = [
        line.replace("OK NETTEST detail=pass", "OK NETTEST detail=started").replace(
            "result=peer-assisted-pass", "result=incomplete"
        )
        for line in oldgood_wifi_resource_replay_lines()
    ]
    events = normalizer.parse_events(lines)

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_LAST"] == "dhcp-bound"
    assert record["WIFI_OLDGOOD_MISSING"] == "nettest"


def test_gate_summary_rejects_condensed_host_eapol_as_oldgood_replay() -> None:
    base_lines = oldgood_wifi_replay_lines()
    join_end = oldgood_wifi_line_index(base_lines, "CYW43_DRIVER_TASK_JOIN_REQUEST") + 1
    lines = base_lines[:join_end]
    lines.extend(
        [
            JOIN_COMPLETE_HOST_EAPOL,
            *bootstrap_gate8_ready_tail(
                generation=9,
                pair_epoch=1,
                stabilizing_ms=150,
                ready_ms=200,
                console_seq=2,
            ),
            "[dhcp] start ready interface=wifi",
            "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
            "server=192.168.10.1 lease_s=3600",
            "OK NETTEST detail=pass scope=serial-local generation=9",
            "netstats: rx_pkts=4 tx_pkts=9 rx_used=4 tx_used=9 polls=30",
            "netstats: generation=9 udp_rx=1 udp_tx=1 tcp_accepts=0 "
            "tcp_auth=0 tcp_rx_bytes=0",
            "netstats: mode=dhcp policy=wifi active=wifi standby=wired "
            "addr_src=dhcp-lease ip=192.168.10.50 gateway=192.168.10.1 dhcp=bound",
            "netstats: wifi_assoc=1 wifi_link=1 eapol_rx=2 "
            "eapol_start=1 eapol_secure=1",
            "netstatus: generation=9 ip=192.168.10.50 "
            "gateway=192.168.10.1 src=dhcp-lease dhcp=bound tcp_ready=no",
        ]
    )
    events = normalizer.parse_events(lines)

    gates = normalizer.summarize_gates(events)
    record = gates.to_record()

    assert gates.wifi_gate == 9
    assert gates.wifi_blocker == "wifi-gate7-7c-missing"
    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_MISSING"] == "host-eapol-m1"


def test_gate_summary_rejects_wifi_dhcp_before_secure_replay() -> None:
    base_lines = oldgood_wifi_replay_lines()
    association_index = oldgood_wifi_line_index(
        base_lines,
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS status=required",
    )
    secure_index = oldgood_wifi_line_index(
        base_lines,
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS status=secure",
    )
    dhcp_start_index = oldgood_wifi_line_index(base_lines, "[dhcp] start ready")
    dhcp_bound_index = oldgood_wifi_line_index(base_lines, "[dhcp] lease bound")
    assert secure_index < dhcp_start_index < dhcp_bound_index
    lines = base_lines[:association_index]
    lines.extend(base_lines[secure_index:dhcp_bound_index])
    lines.extend(base_lines[association_index:secure_index])
    lines.extend(base_lines[dhcp_bound_index:])
    events = normalizer.parse_events(lines)

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_OLDGOOD_REPLAY"] == "no"
    assert record["WIFI_OLDGOOD_LAST"] == "gtk-install"
    assert record["WIFI_OLDGOOD_MISSING"] == "secure-release"


def test_gate_summary_keeps_dhcp_before_secure_below_release_subgate() -> None:
    events = normalizer.parse_events(
        [
            "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract=cyw43455 "
            "status=eapol-rx reason=none polls=13 associated=yes link_up=yes "
            "event_rx=1 eapol_rx=1 data_rx=1 "
            "next_action=inspect-host-eapol-handshake-state",
            "[dhcp] start ready interface=wifi",
            "[dhcp] lease bound ip=192.168.10.50/24 gateway=192.168.10.1 "
            "server=192.168.10.1 lease_s=3600",
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_SUBGATE"] == "7d"
    assert record["WIFI_SUBGATE_NAME"] == "eapol-handshake"
    assert record["WIFI_SUBGATE_REASON"] == "inspect-host-eapol-handshake-state"


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


def bootstrap_supervisor_line(
    attempt: int,
    status: str,
    backoff_ms: int,
    next_attempt_ms: int,
    console_seq: int = 17,
    *,
    serial: str = "ready",
) -> str:
    """Return one full-fidelity persistent-bootstrap supervisor record."""

    return (
        f"CYW43_BOOTSTRAP_SUPERVISOR attempt={attempt} status={status} "
        f"backoff_ms={backoff_ms} next_attempt_ms={next_attempt_ms} "
        f"serial={serial} local_seat=enabled recovery=full "
        f"console_seq={console_seq} "
        "telemetry_sinks=serial+qlog+hdmi prompt_refresh=yes"
    )


def historical_bootstrap_gate8_ready_tail(
    attempt: int,
    *,
    generation: int,
    pair_epoch: int,
    stabilizing_ms: int,
    ready_ms: int,
    console_seq: int,
) -> list[str]:
    """Return the historical pre-Commit Gate-8 publication sequence."""

    return [
        bootstrap_supervisor_line(
            attempt,
            "stabilizing",
            0,
            stabilizing_ms,
            console_seq,
        ),
        *wifi_gate8_snapshot_lines(
            len(normalizer.WIFI_GATE8_SUBGATES),
            pair_epoch=pair_epoch,
            generation=generation,
        ),
        bootstrap_supervisor_line(
            attempt,
            "ready",
            0,
            ready_ms,
            console_seq + 1,
        ),
    ]


def gate8_commit_line(
    *,
    generation: int,
    pair_epoch: int,
    deadline_ms: int,
    console_seq: int,
) -> str:
    """Return one exact production Gate 8 nonterminal commit record."""

    return (
        "CYW43_GATE8_COMMIT attempt=1 status=ready "
        f"pair_epoch={pair_epoch} generation={generation} "
        f"deadline_ms={deadline_ms} console_seq={console_seq} "
        "telemetry_sinks=serial+qlog+hdmi consumer=data"
    )


def bootstrap_gate8_ready_tail(
    *,
    generation: int,
    pair_epoch: int,
    stabilizing_ms: int,
    ready_ms: int,
    console_seq: int,
) -> list[str]:
    """Return the current Stabilizing, snapshot, Commit, and bootstrap Ready."""

    return [
        bootstrap_supervisor_line(
            1,
            "stabilizing",
            0,
            stabilizing_ms,
            console_seq,
        ),
        *wifi_gate8_snapshot_lines(
            len(normalizer.WIFI_GATE8_SUBGATES),
            pair_epoch=pair_epoch,
            generation=generation,
        ),
        gate8_commit_line(
            pair_epoch=pair_epoch,
            generation=generation,
            deadline_ms=stabilizing_ms,
            console_seq=console_seq + 1,
        ),
        bootstrap_supervisor_line(
            1,
            "ready",
            0,
            ready_ms,
            console_seq + 2,
        ),
    ]


def runtime_recovery_ready_line(*, generation: int, console_seq: int) -> str:
    """Return one exact-generation runtime service-restoration record."""

    return (
        "CYW43_RUNTIME_RECOVERY status=ready "
        f"generation={generation} console_seq={console_seq} "
        "telemetry_sinks=serial+qlog+hdmi"
    )


def bootstrap_gate8_exhaustion_lines() -> list[str]:
    """Return a historical five-attempt Gate 8 recovery/exhaustion trace."""

    def recovery_line(attempt: int, generation: int, deadline_ms: int) -> str:
        return (
            f"CYW43_GATE8_RECOVERY attempt={attempt} "
            f"generation={generation} "
            "blocker=gate8-stabilization-deadline "
            f"deadline_ms={deadline_ms} action=pair-restart"
        )

    return [
        bootstrap_supervisor_line(
            0,
            "preflight",
            normalizer.CYW43_BOOTSTRAP_SERIAL_RETRY_MS,
            normalizer.CYW43_BOOTSTRAP_SERIAL_RETRY_MS,
            1,
            serial="blocked",
        ),
        bootstrap_supervisor_line(0, "preflight", 0, 0, 2),
        bootstrap_supervisor_line(1, "begin", 0, 1_850, 3),
        bootstrap_supervisor_line(1, "stabilizing", 0, 117_200, 4),
        *wifi_gate8_snapshot_lines(
            4,
            pair_epoch=0,
            generation=1,
            blocker="bssid-owner-terminal-pending",
        ),
        recovery_line(1, 1, 117_200),
        bootstrap_supervisor_line(1, "backoff", 1_000, 118_200, 5),
        bootstrap_supervisor_line(2, "recovery", 0, 118_200, 6),
        bootstrap_supervisor_line(2, "stabilizing", 0, 270_650, 7),
        *wifi_gate8_snapshot_lines(
            4,
            pair_epoch=1,
            generation=3,
            blocker="bssid-owner-terminal-pending",
        ),
        recovery_line(2, 3, 270_650),
        bootstrap_supervisor_line(2, "backoff", 2_000, 272_650, 8),
        bootstrap_supervisor_line(3, "recovery", 0, 272_650, 9),
        bootstrap_supervisor_line(3, "backoff", 4_000, 276_650, 10),
        bootstrap_supervisor_line(4, "recovery", 0, 276_650, 11),
        bootstrap_supervisor_line(4, "stabilizing", 0, 429_090, 12),
        *wifi_gate8_snapshot_lines(
            4,
            pair_epoch=2,
            generation=6,
            blocker="bssid-owner-terminal-pending",
        ),
        recovery_line(4, 6, 429_090),
        bootstrap_supervisor_line(4, "backoff", 8_000, 437_090, 13),
        bootstrap_supervisor_line(5, "recovery", 0, 437_090, 14),
        bootstrap_supervisor_line(
            5,
            "exhausted",
            0,
            normalizer.CYW43_BOOTSTRAP_NO_ATTEMPT_MS,
            15,
        ),
    ]


def bootstrap_gate8_advancing_exhaustion_lines() -> list[str]:
    """Return a historical trace whose later attempts advance to 8h."""

    def recovery_line(
        attempt: int,
        generation: int,
        blocker: str,
        deadline_ms: int,
    ) -> str:
        return (
            f"CYW43_GATE8_RECOVERY attempt={attempt} "
            f"generation={generation} blocker={blocker} "
            f"deadline_ms={deadline_ms} action=pair-restart"
        )

    lines = [
        bootstrap_supervisor_line(1, "begin", 0, 1_850, 1),
        bootstrap_supervisor_line(1, "stabilizing", 0, 117_240, 2),
        *wifi_gate8_snapshot_lines(
            2,
            pair_epoch=0,
            generation=1,
            status="fail",
            blocker="association-terminal-failure",
        ),
        recovery_line(
            1,
            1,
            "association-terminal-failure",
            117_240,
        ),
        bootstrap_supervisor_line(1, "backoff", 1_000, 32_830, 3),
        bootstrap_supervisor_line(2, "recovery", 0, 32_830, 4),
    ]
    attempt_details = (
        (2, 1, 3, 184_545, 2_000, 110_730),
        (3, 2, 5, 263_150, 4_000, 191_585),
        (4, 3, 7, 344_230, 8_000, 276_365),
        (5, 4, 9, 429_010, 0, normalizer.CYW43_BOOTSTRAP_NO_ATTEMPT_MS),
    )
    console_seq = 5
    for attempt, pair_epoch, generation, deadline_ms, backoff_ms, next_ms in (
        attempt_details
    ):
        lines.extend(
            [
                bootstrap_supervisor_line(
                    attempt,
                    "stabilizing",
                    0,
                    deadline_ms,
                    console_seq,
                ),
                *wifi_gate8_snapshot_lines(
                    7,
                    pair_epoch=pair_epoch,
                    generation=generation,
                    status="fail",
                    blocker="root-rx-drop-since-generation",
                ),
                recovery_line(
                    attempt,
                    generation,
                    "root-rx-drop-since-generation",
                    deadline_ms,
                ),
            ]
        )
        console_seq += 1
        if attempt < 5:
            lines.extend(
                [
                    bootstrap_supervisor_line(
                        attempt,
                        "backoff",
                        backoff_ms,
                        next_ms,
                        console_seq,
                    ),
                    bootstrap_supervisor_line(
                        attempt + 1,
                        "recovery",
                        0,
                        next_ms,
                        console_seq + 1,
                    ),
                ]
            )
            console_seq += 2
        else:
            lines.append(
                bootstrap_supervisor_line(
                    attempt,
                    "exhausted",
                    0,
                    next_ms,
                    console_seq,
                )
            )
    return lines


def test_bootstrap_supervisor_absence_preserves_historical_scoring() -> None:
    """Logs from before the supervisor do not acquire a new blocker."""

    record = normalizer.summarize_gates([]).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_SEEN"] == "no"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "none"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == "none"
    assert not any(
        blocker.startswith("cyw43-bootstrap-supervisor-")
        for blocker in normalizer.boot_evidence_blockers(record)
    )


def test_bootstrap_supervisor_rejects_ready_without_stabilizing_transaction() -> None:
    """A bare Begin-to-Ready edge cannot publish Gate-8 authority."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 500, 1),
            bootstrap_supervisor_line(1, "ready", 0, 500, 2),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_SEEN"] == "yes"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 1
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "ready"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "invalid-status-sequence"
    )
    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_BLOCKER"] == "supervisor-stabilizing-missing"


def test_bootstrap_supervisor_accepts_stabilizing_then_gate8_ready() -> None:
    """The unique Ready follows Stabilizing, Gate 8 snapshot, and Commit."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 500, 1),
            *bootstrap_gate8_ready_tail(
                generation=0,
                pair_epoch=1,
                stabilizing_ms=550,
                ready_ms=600,
                console_seq=2,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "ready"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "yes"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == "none"
    assert record["WIFI_GATE8_COMPLETE"] == "yes"


def test_bootstrap_supervisor_accepts_failed_as_terminal_red() -> None:
    """Attempt one may fail terminally without scheduling another attempt."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(
                1,
                "failed",
                0,
                normalizer.CYW43_BOOTSTRAP_NO_ATTEMPT_MS,
                2,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 1
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "failed"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == "failed-status"
    assert record["WIFI_GATE8_BLOCKER"] == "supervisor-failed"


def test_bootstrap_supervisor_rejects_failed_without_terminal_sentinel() -> None:
    """A failed attempt cannot advertise a future retry timestamp."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "failed", 0, 200, 2),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "malformed-failed-sentinel"
    )


def test_bootstrap_supervisor_rejects_attempt_two_without_backoff() -> None:
    """Attempt two is forbidden even when no outer backoff line was emitted."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(2, "begin", 0, 200, 2),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 2
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == "attempt-overflow"


def test_bootstrap_supervisor_rejects_pre_ready_recovery() -> None:
    """Recovery is a runtime edge and cannot rescue initial bootstrap."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "recovery", 0, 150, 2),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 1
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES"] == 1
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "pre-ready-recovery-forbidden"
    )


def test_bootstrap_supervisor_rejects_second_post_ready_recovery() -> None:
    """One runtime recovery cannot rearm the same lifetime's recovery budget."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            *bootstrap_gate8_ready_tail(
                generation=17,
                pair_epoch=3,
                stabilizing_ms=150,
                ready_ms=200,
                console_seq=2,
            ),
            bootstrap_supervisor_line(1, "recovery", 0, 250, 5),
            bootstrap_supervisor_line(1, "stabilizing", 0, 300, 6),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                pair_epoch=4,
                generation=18,
            ),
            gate8_commit_line(
                pair_epoch=4,
                generation=18,
                deadline_ms=300,
                console_seq=7,
            ),
            runtime_recovery_ready_line(generation=18, console_seq=8),
            bootstrap_supervisor_line(1, "recovery", 0, 350, 9),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES"] == 2
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "recovery-limit-exceeded"
    )


def test_bootstrap_supervisor_rejects_legacy_exhausted_terminal() -> None:
    """Current production emits Failed or Permanent, never Exhausted."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(
                1,
                "exhausted",
                0,
                normalizer.CYW43_BOOTSTRAP_NO_ATTEMPT_MS,
                2,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 1
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "legacy-exhausted-status"
    )


def test_bootstrap_supervisor_counts_same_attempt_pair_recovery_as_red() -> None:
    """A restored runtime remains usable evidence but fails repeatability."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            *bootstrap_gate8_ready_tail(
                generation=17,
                pair_epoch=3,
                stabilizing_ms=150,
                ready_ms=200,
                console_seq=2,
            ),
            bootstrap_supervisor_line(1, "recovery", 0, 250, 5),
            bootstrap_supervisor_line(1, "stabilizing", 0, 300, 6),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                pair_epoch=4,
                generation=18,
            ),
            gate8_commit_line(
                pair_epoch=4,
                generation=18,
                deadline_ms=300,
                console_seq=7,
            ),
            runtime_recovery_ready_line(generation=18, console_seq=8),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 1
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES"] == 1
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "in-attempt-recovery-used"
    )
    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_BLOCKER"] == (
        "supervisor-in-attempt-recovery-used"
    )
    assert record["WIFI_GATE8_GENERATION"] == 18
    assert record["WIFI_GATE8_LATEST_STATUS"] == "pass"
    record["NET_ACTIVE"] = "wifi"
    blockers = normalizer.boot_evidence_blockers(record)
    assert "cyw43-bootstrap-supervisor-in-attempt-recovery-used" in blockers
    assert "cyw43-bootstrap-supervisor-recovery" in blockers


def test_bootstrap_supervisor_recovery_invalidates_earlier_gate8_snapshot() -> None:
    """A later recovery edge cannot reuse an earlier generation's 8a-8h proof."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            *bootstrap_gate8_ready_tail(
                generation=0,
                pair_epoch=1,
                stabilizing_ms=150,
                ready_ms=200,
                console_seq=2,
            ),
            bootstrap_supervisor_line(1, "recovery", 0, 250, 5),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_MISSING"] == "ready"
    assert record["WIFI_GATE8_STATUS"] == "pending"
    assert record["WIFI_GATE8_BLOCKER"] == "runtime-recovery-pending"
    assert record["WIFI_GATE8_LATEST_STATUS"] == "pass"
    assert record["WIFI_GATE8_LATEST_GENERATION"] == 0


def test_bootstrap_supervisor_rejects_stabilizing_after_ready_without_recovery(
) -> None:
    """Stabilizing cannot silently retract the unique bootstrap Ready."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            *bootstrap_gate8_ready_tail(
                generation=7,
                pair_epoch=1,
                stabilizing_ms=150,
                ready_ms=200,
                console_seq=2,
            ),
            bootstrap_supervisor_line(1, "stabilizing", 0, 250, 5),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_MISSING"] == "ready"
    assert record["WIFI_GATE8_STATUS"] == "fail"
    assert record["WIFI_GATE8_BLOCKER"] == (
        "stabilizing-after-ready-without-recovery"
    )
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "stabilizing"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "stabilizing-after-ready-without-recovery"
    )


def test_bootstrap_supervisor_rejects_duplicate_ready_without_recovery() -> None:
    """A second bootstrap Ready is invalid even without an intervening edge."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            *bootstrap_gate8_ready_tail(
                generation=7,
                pair_epoch=1,
                stabilizing_ms=150,
                ready_ms=200,
                console_seq=2,
            ),
            bootstrap_supervisor_line(1, "ready", 0, 300, 5),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "ready"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "bootstrap-ready-duplicate"
    )
    assert record["WIFI_GATE8_BLOCKER"] == "bootstrap-ready-duplicate"


def test_bootstrap_supervisor_rejects_duplicate_ready_after_fresh_recovery_proof(
) -> None:
    """Fresh runtime proof closes only through CYW43_RUNTIME_RECOVERY."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            *bootstrap_gate8_ready_tail(
                generation=7,
                pair_epoch=1,
                stabilizing_ms=150,
                ready_ms=200,
                console_seq=2,
            ),
            bootstrap_supervisor_line(1, "recovery", 0, 250, 5),
            bootstrap_supervisor_line(1, "stabilizing", 0, 300, 6),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                pair_epoch=2,
                generation=8,
            ),
            gate8_commit_line(
                pair_epoch=2,
                generation=8,
                deadline_ms=300,
                console_seq=7,
            ),
            bootstrap_supervisor_line(1, "ready", 0, 350, 8),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_BLOCKER"] == "bootstrap-ready-duplicate"
    assert record["WIFI_GATE8_LATEST_GENERATION"] == 8
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "ready"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == (
        "bootstrap-ready-duplicate"
    )


def test_bootstrap_supervisor_pre_ready_recovery_cannot_authorize_gate8_proof(
) -> None:
    """A pre-ready Recovery cannot authorize a later complete Gate 8 proof."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "stabilizing", 0, 150, 2),
            bootstrap_supervisor_line(1, "recovery", 0, 200, 3),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                pair_epoch=1,
                generation=7,
            ),
            gate8_commit_line(
                pair_epoch=1,
                generation=7,
                deadline_ms=200,
                console_seq=4,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE"] == 7
    assert record["WIFI_GATE8_BLOCKER"] == "pre-service-recovery-forbidden"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == (
        "pre-ready-recovery-forbidden"
    )


def test_bootstrap_supervisor_recovery_requires_new_gate8_generation() -> None:
    """A same-generation post-recovery snapshot is stale, even when 8/8 pass."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            *bootstrap_gate8_ready_tail(
                generation=7,
                pair_epoch=1,
                stabilizing_ms=150,
                ready_ms=200,
                console_seq=2,
            ),
            bootstrap_supervisor_line(1, "recovery", 0, 250, 5),
            bootstrap_supervisor_line(1, "stabilizing", 0, 300, 6),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                pair_epoch=2,
                generation=7,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_MISSING"] == "8a-pair-generation"
    assert record["WIFI_GATE8_BLOCKER"] == (
        "generation-not-advanced-after-runtime-recovery"
    )
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"


def test_bootstrap_supervisor_gate8_generation_wrap_is_forward() -> None:
    """A repair may advance through u32 wrap, while remaining acceptance-red."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            *bootstrap_gate8_ready_tail(
                generation=normalizer.U32_MAX,
                pair_epoch=3,
                stabilizing_ms=150,
                ready_ms=200,
                console_seq=2,
            ),
            bootstrap_supervisor_line(1, "recovery", 0, 250, 5),
            bootstrap_supervisor_line(1, "stabilizing", 0, 300, 6),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                pair_epoch=4,
                generation=0,
            ),
            gate8_commit_line(
                pair_epoch=4,
                generation=0,
                deadline_ms=300,
                console_seq=7,
            ),
            runtime_recovery_ready_line(generation=0, console_seq=8),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_BLOCKER"] == (
        "supervisor-in-attempt-recovery-used"
    )
    assert record["WIFI_GATE8_GENERATION"] == 0
    assert record["WIFI_GATE8_LATEST_STATUS"] == "pass"
    assert record["WIFI_GATE8_LATEST_GENERATION"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES"] == 1
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "in-attempt-recovery-used"
    )


def test_gate8_ordered_proof_rejects_later_generation_regression() -> None:
    """A stale later diagnostic cannot replace a newer generation's proof."""

    events = normalizer.parse_events(
        [
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                generation=12,
            ),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                generation=11,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_MISSING"] == "8a-pair-generation"
    assert record["WIFI_GATE8_BLOCKER"] == "generation-regression"


def test_historical_bootstrap_begin_resets_gate8_generation_history() -> None:
    """Historical pre-Commit evidence may restart from boot-reset generation."""

    events = normalizer.parse_events(
        [
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                generation=12,
            ),
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "stabilizing", 0, 150, 2),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                generation=0,
            ),
            bootstrap_supervisor_line(1, "ready", 0, 200, 3),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "yes"
    assert record["WIFI_GATE8_GENERATION"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "yes"


def test_bootstrap_supervisor_stabilizing_without_ready_is_not_terminal() -> None:
    """An attached stack cannot satisfy the supervisor's Ready contract."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "stabilizing", 0, 150, 2),
            *wifi_gate8_snapshot_lines(
                0,
                generation=0,
                blocker="pair-generation-pending",
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "stabilizing"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "stabilizing-not-terminal"
    )


def test_bootstrap_supervisor_ready_rejects_partial_gate8_telemetry() -> None:
    """A Ready line cannot outrank a current incomplete Gate 8 sequence."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "stabilizing", 0, 150, 2),
            *wifi_gate8_snapshot_lines(
                1,
                generation=0,
                blocker="control-program-pending",
            ),
            bootstrap_supervisor_line(1, "ready", 0, 200, 3),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "ready"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "gate8-subgates-incomplete"
    )


def test_bootstrap_supervisor_ready_requires_gate8_after_stabilizing() -> None:
    """The new stabilizing protocol cannot publish unsupported Ready proof."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "stabilizing", 0, 150, 2),
            bootstrap_supervisor_line(1, "ready", 0, 200, 3),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_STATUS"] == "fail"
    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_BLOCKER"] == "gate8-subgates-incomplete"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "ready"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "gate8-subgates-incomplete"
    )


def test_gate8_commit_is_nonterminal_until_later_bootstrap_service_ready() -> None:
    """Commit opens data, while delayed DHCP/listener Ready closes bootstrap."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "stabilizing", 0, 90_100, 2),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                pair_epoch=3,
                generation=17,
            ),
            gate8_commit_line(
                pair_epoch=3,
                generation=17,
                deadline_ms=90_100,
                console_seq=3,
            ),
            "[dhcp] start ready interface=wifi",
            "[dhcp] lease bound ip=192.168.86.154/24 "
            "gateway=192.168.86.1 server=192.168.86.1 lease_s=3600",
            "[net-console] listener admitted for current WiFi generation",
            bootstrap_supervisor_line(1, "ready", 0, 12_500, 4),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "yes"
    assert record["WIFI_GATE8_PAIR_EPOCH"] == 3
    assert record["WIFI_GATE8_GENERATION"] == 17
    assert record["WIFI_GATE8_BLOCKER"] == "none"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "ready"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "yes"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == "none"


def test_gate8_commit_without_service_ready_remains_nonterminal() -> None:
    """An exact Gate 8 commit cannot itself satisfy bootstrap readiness."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "stabilizing", 0, 90_100, 2),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                pair_epoch=3,
                generation=17,
            ),
            gate8_commit_line(
                pair_epoch=3,
                generation=17,
                deadline_ms=90_100,
                console_seq=3,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_MISSING"] == "ready"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "stabilizing-not-terminal"
    )


@pytest.mark.parametrize(
    ("mutation", "expected_blocker"),
    [
        ("pair_epoch=3", "gate8-commit-identity-mismatch"),
        ("generation=17", "gate8-commit-identity-mismatch"),
    ],
)
def test_gate8_commit_rejects_mixed_snapshot_identity(
    mutation: str, expected_blocker: str
) -> None:
    """The commit cannot stitch a different pair or generation to 8a-8h."""

    replacement = "pair_epoch=4" if mutation.startswith("pair") else "generation=18"
    commit = gate8_commit_line(
        pair_epoch=3,
        generation=17,
        deadline_ms=90_100,
        console_seq=3,
    ).replace(mutation, replacement, 1)
    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "stabilizing", 0, 90_100, 2),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                pair_epoch=3,
                generation=17,
            ),
            commit,
            bootstrap_supervisor_line(1, "ready", 0, 12_500, 4),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_BLOCKER"] == expected_blocker
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"


def test_gate8_commit_must_be_atomic_ninth_record() -> None:
    """No unrelated record may bisect the ordered 8a-8h plus Commit batch."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "stabilizing", 0, 90_100, 2),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                pair_epoch=3,
                generation=17,
            ),
            "[dhcp] start ready interface=wifi",
            gate8_commit_line(
                pair_epoch=3,
                generation=17,
                deadline_ms=90_100,
                console_seq=3,
            ),
            bootstrap_supervisor_line(1, "ready", 0, 12_500, 4),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_BLOCKER"] == "gate8-commit-not-adjacent"


def test_runtime_ready_cannot_replace_bootstrap_ready() -> None:
    """A runtime restoration marker is never an initial bootstrap terminal."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "stabilizing", 0, 90_100, 2),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                pair_epoch=3,
                generation=17,
            ),
            gate8_commit_line(
                pair_epoch=3,
                generation=17,
                deadline_ms=90_100,
                console_seq=3,
            ),
            runtime_recovery_ready_line(generation=17, console_seq=4),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_BLOCKER"] == "runtime-recovery-before-bootstrap-ready"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"


def test_runtime_recovery_restoration_is_not_zero_recovery_boot_proof() -> None:
    """Runtime may restore service, but the boot remains repeatability-red."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "stabilizing", 0, 90_100, 2),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                pair_epoch=3,
                generation=17,
            ),
            gate8_commit_line(
                pair_epoch=3,
                generation=17,
                deadline_ms=90_100,
                console_seq=3,
            ),
            bootstrap_supervisor_line(1, "ready", 0, 12_500, 4),
            bootstrap_supervisor_line(1, "recovery", 0, 130_000, 5),
            bootstrap_supervisor_line(1, "stabilizing", 0, 220_000, 6),
            *wifi_gate8_snapshot_lines(
                len(normalizer.WIFI_GATE8_SUBGATES),
                pair_epoch=4,
                generation=18,
            ),
            gate8_commit_line(
                pair_epoch=4,
                generation=18,
                deadline_ms=220_000,
                console_seq=7,
            ),
            runtime_recovery_ready_line(generation=18, console_seq=8),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_BLOCKER"] == (
        "supervisor-in-attempt-recovery-used"
    )
    assert record["WIFI_GATE8_PAIR_EPOCH"] == 4
    assert record["WIFI_GATE8_GENERATION"] == 18
    assert record["WIFI_GATE8_LATEST_STATUS"] == "pass"
    assert record["WIFI_GATE8_LATEST_PAIR_EPOCH"] == 4
    assert record["WIFI_GATE8_LATEST_GENERATION"] == 18
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "stabilizing"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES"] == 1
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == (
        "in-attempt-recovery-used"
    )
    record["NET_ACTIVE"] = "wifi"
    blockers = normalizer.boot_evidence_blockers(record)
    assert "cyw43-bootstrap-supervisor-in-attempt-recovery-used" in blockers
    assert "cyw43-bootstrap-supervisor-recovery" in blockers


def test_runtime_recovery_rejects_duplicate_bootstrap_ready() -> None:
    """A restored runtime generation cannot publish bootstrap Ready again."""

    lines = [
        bootstrap_supervisor_line(1, "begin", 0, 100, 1),
        bootstrap_supervisor_line(1, "stabilizing", 0, 90_100, 2),
        *wifi_gate8_snapshot_lines(
            len(normalizer.WIFI_GATE8_SUBGATES),
            pair_epoch=3,
            generation=17,
        ),
        gate8_commit_line(
            pair_epoch=3,
            generation=17,
            deadline_ms=90_100,
            console_seq=3,
        ),
        bootstrap_supervisor_line(1, "ready", 0, 12_500, 4),
        bootstrap_supervisor_line(1, "recovery", 0, 130_000, 5),
        bootstrap_supervisor_line(1, "stabilizing", 0, 220_000, 6),
        *wifi_gate8_snapshot_lines(
            len(normalizer.WIFI_GATE8_SUBGATES),
            pair_epoch=4,
            generation=18,
        ),
        gate8_commit_line(
            pair_epoch=4,
            generation=18,
            deadline_ms=220_000,
            console_seq=7,
        ),
        bootstrap_supervisor_line(1, "ready", 0, 180_000, 8),
    ]
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_BLOCKER"] == "bootstrap-ready-duplicate"


def test_bootstrap_supervisor_accepts_production_raw_uart_suffix() -> None:
    """The raw UART ordering suffix must not invalidate supervisor proof."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 500, 1),
            *bootstrap_gate8_ready_tail(
                generation=0,
                pair_epoch=1,
                stabilizing_ms=550,
                ready_ms=600,
                console_seq=2,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_SEEN"] == "yes"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "yes"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == "none"


def test_bootstrap_supervisor_rejects_obsolete_local_seat_ready_value() -> None:
    """Production local-seat state is boolean, never the obsolete Ready alias."""

    begin = bootstrap_supervisor_line(1, "begin", 0, 500, 1).replace(
        "local_seat=enabled",
        "local_seat=ready",
        1,
    )
    events = normalizer.parse_events(
        [
            begin,
            *bootstrap_gate8_ready_tail(
                generation=0,
                pair_epoch=1,
                stabilizing_ms=550,
                ready_ms=600,
                console_seq=2,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == (
        "production-suffix-incomplete"
    )


@pytest.mark.parametrize(
    "suffix",
    [
        "",
        " serial=ready",
        " serial=ready local_seat=enabled",
        (
            " serial=ready local_seat=enabled "
            "recovery=full"
        ),
        (
            " serial=ready local_seat=enabled "
            "recovery=full console_seq=17"
        ),
        (
            " serial=ready local_seat=enabled "
            "recovery=full console_seq=17 "
            "telemetry_sinks=serial+qlog+hdmi"
        ),
        (
            " serial=ready local_seat=enabled "
            "recovery=full console_seq=17 "
            "telemetry_sinks=serial+qlog+hdmi prompt_refresh="
        ),
        (
            " serial=ready local_seat=enabled "
            "recovery=full console_seq=17 "
            "telemetry_sinks=serial+qlog+hdmi prompt_refresh=no"
        ),
    ],
)
def test_bootstrap_supervisor_truncated_production_suffix_is_diagnostic_only(
    suffix: str,
) -> None:
    """Every production-suffix truncation boundary must fail readiness."""

    ready_prefix = (
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=ready "
        "backoff_ms=0 next_attempt_ms=500"
    )
    record = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                bootstrap_supervisor_line(1, "begin", 0, 500, 1),
                ready_prefix + suffix,
            ]
        )
    ).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_SEEN"] == "yes"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 1
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "ready"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "production-suffix-incomplete"
    )
    assert (
        "cyw43-bootstrap-supervisor-production-suffix-incomplete"
        in normalizer.boot_evidence_blockers(record)
    )


@pytest.mark.parametrize(
    ("old", "new"),
    [
        ("attempt=1", "attempt=01"),
        ("backoff_ms=0", "backoff_ms=00"),
        (
            "next_attempt_ms=500",
            "next_attempt_ms=18446744073709551616",
        ),
        ("console_seq=2", "console_seq=18446744073709551616"),
    ],
)
def test_bootstrap_supervisor_rejects_noncanonical_or_oversized_u64_fields(
    old: str, new: str
) -> None:
    """Evidence numbers must be canonical values representable by production."""

    lines = [
        bootstrap_supervisor_line(1, "begin", 0, 500, 1),
        bootstrap_supervisor_line(1, "ready", 0, 500, 2).replace(old, new, 1),
    ]
    if old == "attempt=1":
        lines[0] = lines[0].replace(old, new, 1)
    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == "numeric-field-invalid"


def test_bootstrap_supervisor_normalizes_later_ready_but_rejects_production_retry() -> None:
    """Later diagnostic readiness cannot promote warm-up retries."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(
                1, "backoff", 1_000, 1_200, 2
            ),
            bootstrap_supervisor_line(2, "begin", 0, 1_200, 3),
            bootstrap_supervisor_line(
                2, "backoff", 2_000, 3_300, 4
            ),
            bootstrap_supervisor_line(3, "begin", 0, 3_300, 5),
            *historical_bootstrap_gate8_ready_tail(
                3,
                generation=0,
                pair_epoch=1,
                stabilizing_ms=3_325,
                ready_ms=3_350,
                console_seq=6,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()
    record["NET_ACTIVE"] = "wifi"
    blockers = normalizer.boot_evidence_blockers(record)

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 3
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 2
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "ready"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "outer-backoff-forbidden"
    )
    assert "cyw43-bootstrap-supervisor-not-first-attempt" in blockers
    assert "cyw43-bootstrap-supervisor-transient-retries" in blockers


def test_bootstrap_supervisor_preflight_does_not_consume_an_attempt() -> None:
    """A recovered serial preflight precedes, but does not poison, attempt one."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(
                0, "preflight", 250, 350, 1, serial="blocked"
            ),
            bootstrap_supervisor_line(
                0, "preflight", 0, 400, 2, serial="ready"
            ),
            bootstrap_supervisor_line(1, "begin", 0, 400, 3),
            *bootstrap_gate8_ready_tail(
                generation=0,
                pair_epoch=1,
                stabilizing_ms=450,
                ready_ms=500,
                console_seq=4,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 1
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "ready"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "yes"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == "none"


def test_bootstrap_supervisor_preflight_does_not_rescue_legacy_retries() -> None:
    """Valid serial cutover cannot rescue a historical outer retry loop."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(
                0, "preflight", 250, 250, 1, serial="blocked"
            ),
            bootstrap_supervisor_line(
                0, "preflight", 0, 0, 2, serial="ready"
            ),
            bootstrap_supervisor_line(1, "begin", 0, 1_850, 3),
            bootstrap_supervisor_line(1, "backoff", 1_000, 29_135, 4),
            bootstrap_supervisor_line(2, "begin", 0, 29_135, 5),
            bootstrap_supervisor_line(2, "backoff", 2_000, 86_565, 6),
            bootstrap_supervisor_line(3, "begin", 0, 86_565, 7),
            bootstrap_supervisor_line(3, "backoff", 4_000, 151_895, 8),
            bootstrap_supervisor_line(4, "begin", 0, 151_895, 9),
            bootstrap_supervisor_line(4, "backoff", 8_000, 221_205, 10),
            bootstrap_supervisor_line(5, "begin", 0, 221_205, 11),
            bootstrap_supervisor_line(
                5,
                "exhausted",
                0,
                normalizer.CYW43_BOOTSTRAP_NO_ATTEMPT_MS,
                12,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 5
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 4
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "exhausted"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "outer-backoff-forbidden"
    )


def test_bootstrap_supervisor_blocked_preflight_remains_acceptance_red() -> None:
    """A serial-blocked preflight without a later episode is diagnostic only."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                bootstrap_supervisor_line(
                    0, "preflight", 250, 350, 1, serial="blocked"
                )
            ]
        )
    ).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == "serial-blocked"


def test_bootstrap_supervisor_accepts_serial_cutover_preflight_suffix() -> None:
    """The pre-cutover queen-log record is a legal attempt-zero snapshot."""

    line = (
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=0 status=preflight "
        "backoff_ms=250 next_attempt_ms=250 serial=blocked "
        "local_seat=enabled recovery=full console_seq=1 "
        "telemetry_sinks=serial+queen-log prompt_refresh=no"
    )
    record = normalizer.summarize_gates(
        normalizer.parse_events([line])
    ).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == "serial-blocked"


def test_bootstrap_supervisor_rejects_preflight_suffix_after_attempt_zero() -> None:
    """All active attempts still require the full operator-facing suffix."""

    line = (
        "CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=begin "
        "backoff_ms=0 next_attempt_ms=250 serial=blocked "
        "local_seat=enabled recovery=full console_seq=1 "
        "telemetry_sinks=serial+queen-log prompt_refresh=no"
    )
    record = normalizer.summarize_gates(
        normalizer.parse_events([line])
    ).to_record()

    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "production-suffix-incomplete"
    )


@pytest.mark.parametrize(
    ("lines", "expected_blocker"),
    [
        (
            [bootstrap_supervisor_line(0, "preflight", 1, 350, 1)],
            "malformed-preflight-timing",
        ),
        (
            [
                bootstrap_supervisor_line(
                    0, "preflight", 0, 350, 1, serial="blocked"
                )
            ],
            "malformed-preflight-timing",
        ),
        (
            [
                bootstrap_supervisor_line(
                    0, "preflight", 250, 249, 1, serial="blocked"
                )
            ],
            "malformed-preflight-timing",
        ),
        (
            [
                bootstrap_supervisor_line(
                    0, "preflight", 250, 500, 1, serial="blocked"
                ),
                bootstrap_supervisor_line(
                    0, "preflight", 250, 499, 2, serial="blocked"
                ),
            ],
            "malformed-preflight-timing",
        ),
    ],
)
def test_bootstrap_supervisor_rejects_malformed_preflight_timing(
    lines: list[str], expected_blocker: str
) -> None:
    """Preflight proof preserves the linked-serial 250 ms retry contract."""

    record = normalizer.summarize_gates(normalizer.parse_events(lines)).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == expected_blocker


def test_historical_bootstrap_ready_republication_is_acceptance_red() -> None:
    """Historical repeated bootstrap Ready records remain diagnostic only."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            *historical_bootstrap_gate8_ready_tail(
                1,
                generation=0,
                pair_epoch=1,
                stabilizing_ms=150,
                ready_ms=200,
                console_seq=2,
            ),
            bootstrap_supervisor_line(1, "recovery", 0, 500, 4),
            *historical_bootstrap_gate8_ready_tail(
                1,
                generation=1,
                pair_epoch=2,
                stabilizing_ms=550,
                ready_ms=600,
                console_seq=5,
            ),
            bootstrap_supervisor_line(1, "recovery", 0, 900, 7),
            *historical_bootstrap_gate8_ready_tail(
                1,
                generation=2,
                pair_epoch=3,
                stabilizing_ms=950,
                ready_ms=1_000,
                console_seq=8,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 1
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 0
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES"] == 2
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "ready"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "bootstrap-ready-duplicate"
    )
    assert record["WIFI_GATE8_LATEST_STATUS"] == "pass"
    assert record["WIFI_GATE8_LATEST_GENERATION"] == 2


def test_bootstrap_supervisor_rejects_historical_recovery_exhaustion() -> None:
    """Historical recovery and exhaustion remain diagnostic and acceptance-red."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(0, "preflight", 0, 0, 1),
            bootstrap_supervisor_line(1, "begin", 0, 100, 2),
            bootstrap_supervisor_line(1, "stabilizing", 0, 150, 3),
            bootstrap_supervisor_line(1, "ready", 0, 200, 4),
            bootstrap_supervisor_line(1, "recovery", 0, 300, 5),
            bootstrap_supervisor_line(1, "stabilizing", 0, 350, 6),
            bootstrap_supervisor_line(1, "ready", 0, 400, 7),
            bootstrap_supervisor_line(1, "recovery", 0, 500, 8),
            bootstrap_supervisor_line(1, "backoff", 1_000, 1_500, 9),
            bootstrap_supervisor_line(2, "recovery", 0, 1_500, 10),
            bootstrap_supervisor_line(2, "stabilizing", 0, 1_550, 11),
            bootstrap_supervisor_line(2, "ready", 0, 1_600, 12),
            bootstrap_supervisor_line(2, "recovery", 0, 1_700, 13),
            bootstrap_supervisor_line(2, "backoff", 2_000, 3_700, 14),
            bootstrap_supervisor_line(3, "recovery", 0, 3_700, 15),
            bootstrap_supervisor_line(3, "stabilizing", 0, 3_750, 16),
            bootstrap_supervisor_line(3, "ready", 0, 3_800, 17),
            bootstrap_supervisor_line(3, "recovery", 0, 3_900, 18),
            bootstrap_supervisor_line(3, "backoff", 4_000, 7_900, 19),
            bootstrap_supervisor_line(4, "recovery", 0, 7_900, 20),
            bootstrap_supervisor_line(4, "stabilizing", 0, 7_950, 21),
            bootstrap_supervisor_line(4, "ready", 0, 8_000, 22),
            bootstrap_supervisor_line(4, "recovery", 0, 8_100, 23),
            bootstrap_supervisor_line(4, "backoff", 8_000, 16_100, 24),
            bootstrap_supervisor_line(5, "recovery", 0, 16_100, 25),
            bootstrap_supervisor_line(5, "stabilizing", 0, 16_150, 26),
            bootstrap_supervisor_line(5, "ready", 0, 16_200, 27),
            bootstrap_supervisor_line(5, "recovery", 0, 16_300, 28),
            bootstrap_supervisor_line(
                5, "exhausted", 0, (1 << 64) - 1, 29
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 5
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 4
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_RECOVERIES"] == 10
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "exhausted"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "bootstrap-ready-duplicate"
    )


def test_bootstrap_supervisor_retains_first_gate8_failure_at_exhaustion() -> None:
    """Generic recovery lifecycle edges cannot erase the first atomic cause."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(bootstrap_gate8_exhaustion_lines())
    ).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 5
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 4
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "exhausted"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "outer-backoff-forbidden"
    )
    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_SEEN"] == ">".join(
        normalizer.WIFI_GATE8_SUBGATES[:4]
    )
    assert record["WIFI_GATE8_LAST"] == "8d-association-link"
    assert record["WIFI_GATE8_MISSING"] == "8e-bssid-refresh"
    assert record["WIFI_GATE8_STATUS"] == "fail"
    assert record["WIFI_GATE8_PAIR_EPOCH"] == 0
    assert record["WIFI_GATE8_GENERATION"] == 1
    assert record["WIFI_GATE8_BLOCKER"] == "bssid-owner-terminal-pending"


def test_gate8_reports_first_cause_and_latest_farthest_exhausted_frontier() -> None:
    """Later 8h progress stays visible without erasing attempt one's cause."""

    lines = bootstrap_gate8_advancing_exhaustion_lines()
    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()
    latest_line = next(
        index
        for index, line in enumerate(lines, start=1)
        if "subgate=8h-data-admission" in line and "generation=9" in line
    )

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 5
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 4
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "exhausted"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "outer-backoff-forbidden"
    )

    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_SEEN"] == ">".join(
        normalizer.WIFI_GATE8_SUBGATES[:2]
    )
    assert record["WIFI_GATE8_LAST"] == "8b-control-program"
    assert record["WIFI_GATE8_MISSING"] == "8c-join-terminal"
    assert record["WIFI_GATE8_STATUS"] == "fail"
    assert record["WIFI_GATE8_PAIR_EPOCH"] == 0
    assert record["WIFI_GATE8_GENERATION"] == 1
    assert record["WIFI_GATE8_BLOCKER"] == "association-terminal-failure"

    assert record["WIFI_GATE8_LATEST_SEEN"] == ">".join(
        normalizer.WIFI_GATE8_SUBGATES[:7]
    )
    assert record["WIFI_GATE8_LATEST_LAST"] == "8g-post-key-maintenance"
    assert record["WIFI_GATE8_LATEST_MISSING"] == "8h-data-admission"
    assert record["WIFI_GATE8_LATEST_STATUS"] == "fail"
    assert record["WIFI_GATE8_LATEST_PAIR_EPOCH"] == 4
    assert record["WIFI_GATE8_LATEST_GENERATION"] == 9
    assert (
        record["WIFI_GATE8_LATEST_BLOCKER"]
        == "root-rx-drop-since-generation"
    )
    assert record["WIFI_GATE8_LATEST_LINE"] == latest_line
    assert record["WIFI_GATE8_LATEST_ATTEMPT"] == 5


def test_gate8_postexhaust_passive_diag_cannot_mutate_retained_frontier() -> None:
    """Post-exhaustion `wifi diag` is passive and cannot alter boot history."""

    boot_lines = bootstrap_gate8_exhaustion_lines()
    baseline = normalizer.summarize_gates(
        normalizer.parse_events(boot_lines)
    ).to_record()
    with_postexhaust_diag = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                *boot_lines,
                *wifi_gate8_snapshot_lines(
                    0,
                    pair_epoch=2,
                    generation=7,
                    status="fail",
                    blocker="pair-recovery-required",
                ),
            ]
        )
    ).to_record()

    retained_keys = (
        "WIFI_GATE8_SEEN",
        "WIFI_GATE8_LAST",
        "WIFI_GATE8_MISSING",
        "WIFI_GATE8_STATUS",
        "WIFI_GATE8_PAIR_EPOCH",
        "WIFI_GATE8_GENERATION",
        "WIFI_GATE8_BLOCKER",
        "WIFI_GATE8_LINE",
        "WIFI_GATE8_LATEST_SEEN",
        "WIFI_GATE8_LATEST_LAST",
        "WIFI_GATE8_LATEST_MISSING",
        "WIFI_GATE8_LATEST_STATUS",
        "WIFI_GATE8_LATEST_PAIR_EPOCH",
        "WIFI_GATE8_LATEST_GENERATION",
        "WIFI_GATE8_LATEST_BLOCKER",
        "WIFI_GATE8_LATEST_LINE",
        "WIFI_GATE8_LATEST_ATTEMPT",
    )
    assert {
        key: with_postexhaust_diag[key] for key in retained_keys
    } == {key: baseline[key] for key in retained_keys}
    assert (
        with_postexhaust_diag["WIFI_GATE8_BLOCKER"]
        != "pair-epoch-not-advanced-after-recovery"
    )


def test_retained_gate8_labels_preserve_prefix_after_terminal_retirement() -> None:
    """Historical Gate 8 proof must not become a fresh Gate 4 failure."""

    lines = [
        "wifi: gate 1 name=runtime-power-reset status=pass "
        "evidence=power=proven-on-by-gate8-terminal "
        "reset=proven-deasserted-by-gate8-terminal pwrseq_status=unknown "
        "pwrseq_phase=none dependency=retained-exact-gate8-terminal "
        "source=driver-task next=sdio-card-select",
        "wifi: gate 2 name=sdio-card-select status=pass "
        "evidence=card=unknown rca=0x0000 ocr=0x00000000 "
        "next=cccr-fbr-ready",
        "wifi: gate 3 name=cccr-fbr-ready status=pass "
        "evidence=ioex=none iordy=none fbr1_blk=none fbr2_blk=none "
        "sequencer_proof=none next=ht-clock",
        "wifi: gate 4 name=ht-clock status=pass "
        "evidence=clock_snapshot=unavailable requested=unavailable "
        "effective=unavailable width=unavailable "
        "reason=post-scrub-retained-gate8-terminal-proves-prerequisite "
        "source=sdio-owner next=backplane-window",
        "wifi: gate 5 name=backplane-window status=pass "
        "evidence=programmed=unknown shadow=unknown fn=unknown "
        "sequencer_proof=none next=firmware-upload",
        "wifi: gate 6 name=firmware-upload status=pass "
        "evidence=uploaded=yes verified=yes fault_detail=0x0000 "
        "next=function2-ready",
        "wifi: gate 7 name=function2-ready status=pass "
        "evidence=f2_enabled=yes f2_ready=yes f2_state=post-release-ready "
        "dependency=none next=firmware-channel",
        "wifi: gate 8 name=firmware-channel status=fail "
        "evidence=exact=pair-recovery-required control_stage=none "
        "sdhci=unknown reply_mode=unknown dependency=pair-recovery-required "
        "next=dhcp-bound",
        *wifi_gate8_snapshot_lines(
            0,
            pair_epoch=2,
            generation=7,
            status="fail",
            blocker="pair-recovery-required",
        ),
        "wifi: next_action=run-pair-recovery-after-terminal-retirement "
        "blocker=pair-recovery-required proof_gate=7 target_gate=10",
    ]

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE"] == 7
    assert record["WIFI_BLOCKER"] == "pair-recovery-required"
    assert record["WIFI_GATE8_COMPLETE"] == "no"
    assert record["WIFI_GATE8_STATUS"] == "fail"
    assert record["WIFI_GATE8_BLOCKER"] == "pair-recovery-required"
    assert record["WIFI_BLOCKER"] != "ht-clock"
    assert "post-scrub" not in str(record["WIFI_BLOCKER"])


def test_bootstrap_supervisor_rejects_legacy_exhaustion_sequence() -> None:
    """A historical five-attempt exhaustion cannot satisfy current proof."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(1, "backoff", 1_000, 1_100, 2),
            bootstrap_supervisor_line(2, "begin", 0, 1_100, 3),
            bootstrap_supervisor_line(2, "backoff", 2_000, 3_100, 4),
            bootstrap_supervisor_line(3, "begin", 0, 3_100, 5),
            bootstrap_supervisor_line(3, "backoff", 4_000, 7_100, 6),
            bootstrap_supervisor_line(4, "begin", 0, 7_100, 7),
            bootstrap_supervisor_line(4, "backoff", 8_000, 15_100, 8),
            bootstrap_supervisor_line(5, "begin", 0, 15_100, 9),
            bootstrap_supervisor_line(
                5, "exhausted", 0, (1 << 64) - 1, 10
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_MAX_ATTEMPT"] == 5
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_TRANSIENT_RETRIES"] == 4
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_LAST_STATUS"] == "exhausted"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "outer-backoff-forbidden"
    )


@pytest.mark.parametrize(
    ("lines", "expected_blocker"),
    [
        ([bootstrap_supervisor_line(0, "begin", 0, 100)], "attempt-zero"),
        ([bootstrap_supervisor_line(1, "begin", 0, 100)], "begin-not-terminal"),
        (
            [
                bootstrap_supervisor_line(1, "begin", 0, 100, 1),
                bootstrap_supervisor_line(
                    1, "backoff", 1_000, 1_100, 2
                ),
            ],
            "outer-backoff-forbidden",
        ),
        (
            [
                bootstrap_supervisor_line(1, "begin", 0, 100, 1),
                bootstrap_supervisor_line(1, "stabilizing", 0, 150, 2),
                bootstrap_supervisor_line(
                    1, "backoff", 1_000, 1_150, 3
                ),
            ],
            "outer-backoff-forbidden",
        ),
        (
            [
                bootstrap_supervisor_line(1, "begin", 0, 100, 1),
                bootstrap_supervisor_line(
                    1, "backoff", 999, 1_100, 2
                ),
            ],
            "outer-backoff-forbidden",
        ),
        (
            [
                bootstrap_supervisor_line(1, "begin", 0, 100, 1),
                bootstrap_supervisor_line(
                    1, "backoff", 1_000, 1_100, 2
                ),
                bootstrap_supervisor_line(2, "begin", 0, 1_100, 3),
                bootstrap_supervisor_line(1, "ready", 0, 1_100, 4),
            ],
            "outer-backoff-forbidden",
        ),
        (
            [
                bootstrap_supervisor_line(1, "begin", 0, 100, 1),
                bootstrap_supervisor_line(1, "permanent", 0, 200, 2),
            ],
            "permanent-status",
        ),
        (
            [bootstrap_supervisor_line(1, "permanent", 0, 200, 1)],
            "permanent-status",
        ),
        (
            [
                bootstrap_supervisor_line(1, "begin", 0, 100, 2),
                bootstrap_supervisor_line(1, "ready", 0, 200, 1),
            ],
            "console-sequence-not-monotonic",
        ),
        (
            [
                "CYW43_BOOTSTRAP_SUPERVISOR status=begin attempt=1 "
                "backoff_ms=0 next_attempt_ms=100"
            ],
            "malformed-line",
        ),
    ],
)
def test_bootstrap_supervisor_rejects_incomplete_or_malformed_sequences(
    lines: list[str], expected_blocker: str
) -> None:
    """Incomplete, regressing, permanent, and malformed sequences fail closed."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["CYW43_BOOTSTRAP_SUPERVISOR_SEEN"] == "yes"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"] == expected_blocker
    assert (
        f"cyw43-bootstrap-supervisor-{expected_blocker}"
        in normalizer.boot_evidence_blockers(record)
    )


def test_exact_gate8_transaction_cannot_clear_forbidden_outer_retry() -> None:
    """A later exact 8a-8h publication cannot rescue an outer retry."""

    events = normalizer.parse_events(
        [
            bootstrap_supervisor_line(1, "begin", 0, 100, 1),
            bootstrap_supervisor_line(
                1, "backoff", 1_000, 1_100, 2
            ),
            bootstrap_supervisor_line(2, "begin", 0, 1_100, 3),
            *historical_bootstrap_gate8_ready_tail(
                2,
                generation=0,
                pair_epoch=1,
                stabilizing_ms=1_150,
                ready_ms=1_200,
                console_seq=4,
            ),
        ]
    )

    record = normalizer.summarize_gates(events).to_record()

    assert (
        record["CYW43_BOOTSTRAP_SUPERVISOR_BLOCKER"]
        == "outer-backoff-forbidden"
    )
    assert record["CYW43_BOOTSTRAP_SUPERVISOR_READY"] == "no"
    assert record["WIFI_BLOCKER"] == "supervisor-outer-backoff-forbidden"


def test_wifi_diag_complete_rejects_scrubbed_gate8_summary() -> None:
    """A retained or scrubbed summary cannot replace the current snapshot."""

    lines = [
        wifi_diag_begin_line(7, 1, 1),
        *[
            (
                f"wifi: gate 8 subgate={subgate} status={status} "
                "pair_epoch=1 generation=1 "
                f"blocker={blocker}"
            )
            for subgate, status, blocker in [
                ("8a-pair-generation", "fail", "pair-recovery-required"),
                ("8b-control-program", "pending", "8a-pair-generation"),
                ("8c-join-terminal", "pending", "8b-control-program"),
                ("8d-association-link", "pending", "8c-join-terminal"),
                ("8e-bssid-refresh", "pending", "8d-association-link"),
                ("8f-eapol-keys", "pending", "8e-bssid-refresh"),
                ("8g-post-key-maintenance", "pending", "8f-eapol-keys"),
            ]
            ],
    ]
    lines.append(
        "wifi: diag_context id=7 "
        "retained=8c-join-terminal/pending/join-owner-active "
        "cause=issued-owner-unknown trigger=rx-queue-poison"
    )
    lines.append(
        wifi_diag_complete_line(
            7,
            1,
            1,
            detail="no",
            scope="scrubbed",
            frontier="8a-pair-generation",
            status="fail",
            blocker="pair-recovery-required",
        )
    )

    record = normalizer.summarize_gates(
        normalizer.parse_events(lines)
    ).to_record()

    assert record["WIFI_GATE8_MISSING"] == "8a-pair-generation"
    assert record["WIFI_GATE8_STATUS"] == "fail"
    assert record["WIFI_GATE8_BLOCKER"] == "diag-summary-not-current"
    assert record["WIFI_DIAG_DETAIL"] == "no"
    assert record["WIFI_DIAG_SCOPE"] == "scrubbed"
    assert record["WIFI_DIAG_CAUSE"] == "issued-owner-unknown"
    assert record["WIFI_DIAG_TRIGGER"] == "rx-queue-poison"
    assert (
        record["WIFI_DIAG_RETAINED"]
        == "8c-join-terminal/pending/join-owner-active"
    )


def test_wifi_diag_complete_recovers_exact_current_clipped_gate8() -> None:
    """A clipped current snapshot recovers only from its exact terminal id."""

    pair_epoch = 41
    generation = 43
    lines = [
        wifi_diag_begin_line(37, pair_epoch, generation),
        *[
            wifi_gate8_subgate_line(
                subgate,
                pair_epoch=pair_epoch,
                generation=generation,
            )
            for subgate in normalizer.WIFI_GATE8_SUBGATES[:5]
        ],
        wifi_diag_complete_line(
            37,
            pair_epoch,
            generation,
            detail="no",
        ),
    ]
    events = normalizer.parse_events(lines)

    proof = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )

    assert proof.complete
    assert proof.pair_epoch == pair_epoch
    assert proof.generation == generation
    assert proof.line == len(lines)


def test_wifi_diag_complete_rejects_mismatched_clipped_gate8_identity() -> None:
    """A current summary cannot recover rows from another pair or command."""

    lines = [
        wifi_diag_begin_line(37, 41, 43),
        *[
            wifi_gate8_subgate_line(subgate, pair_epoch=41, generation=43)
            for subgate in normalizer.WIFI_GATE8_SUBGATES[:5]
        ],
        wifi_diag_complete_line(38, 41, 43, detail="no"),
    ]
    events = normalizer.parse_events(lines)

    proof = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )

    assert not proof.complete
    assert proof.blocker == "diag-snapshot-identity-invalid"


def test_wifi_diag_complete_revokes_old_gate8_on_latest_invalid_summary() -> None:
    """A malformed current terminal cannot leave an older Gate 8 accepted."""

    lines = [
        *[
            wifi_gate8_subgate_line(subgate, pair_epoch=41, generation=43)
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        wifi_diag_begin_line(37, 41, 43),
        *[
            wifi_gate8_subgate_line(subgate, pair_epoch=41, generation=43)
            for subgate in normalizer.WIFI_GATE8_SUBGATES[:5]
        ],
        wifi_diag_complete_line(
            37,
            41,
            43,
            detail="no",
            frontier="complete",
            status="pending",
            blocker="none",
        ),
    ]
    events = normalizer.parse_events(lines)

    proof = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )

    assert not proof.complete
    assert proof.blocker == "diag-summary-invalid"
    assert proof.line == len(lines)


def test_latest_malformed_wifi_diag_terminal_revokes_prior_snapshot() -> None:
    """The latest terminal prefix wins even when its causal field is absent."""

    lines = [
        wifi_diag_begin_line(37, 41, 43),
        wifi_gate7_retained_line(37, 41, 43),
        *[
            wifi_gate8_subgate_line(subgate, pair_epoch=41, generation=43)
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        wifi_diag_complete_line(37, 41, 43),
        "wifi: diag_complete id=37 detail=yes scope=current "
        "snapshot=current pair=41 gen=43 front=complete status=pass block=none",
    ]
    events = normalizer.parse_events(lines)

    gate8 = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )
    gate7 = normalizer.summarize_wifi_gate7_proof(events)

    assert not gate8.complete
    assert gate8.blocker == "diag-summary-invalid"
    assert not gate7.complete
    assert gate7.missing == "retained-diag-begin"
    assert normalizer.summarize_wifi_diag_complete(events) == (
        "unknown",
        "unknown",
        "none",
        "none",
        "none",
    )


@pytest.mark.parametrize(
    "mutation",
    [
        lambda line: f"{line} extra=yes",
        lambda line: line.replace(
            "causal=yes detail=yes", "detail=yes causal=yes"
        ),
        lambda line: line.replace("id=37", "id=0x25"),
        lambda line: line.replace("id=37", "id=38 id=37"),
        lambda _line: "wifi: diag_complete",
    ],
)
def test_compact_wifi_diag_terminal_requires_exact_emitted_grammar(
    mutation: Callable[[str], str],
) -> None:
    """Equivalent, extended, duplicate, or truncated terminals have no authority."""

    valid_terminal = wifi_diag_complete_line(37, 41, 43)
    lines = [
        wifi_diag_begin_line(37, 41, 43),
        wifi_gate7_retained_line(37, 41, 43),
        *[
            wifi_gate8_subgate_line(subgate, pair_epoch=41, generation=43)
            for subgate in normalizer.WIFI_GATE8_SUBGATES
        ],
        valid_terminal,
        mutation(valid_terminal),
    ]
    events = normalizer.parse_events(lines)

    gate8 = normalizer.refine_wifi_gate8_from_diag_complete(
        events, normalizer.summarize_wifi_gate8_proof(events)
    )
    gate7 = normalizer.summarize_wifi_gate7_proof(events)

    assert not gate8.complete
    assert gate8.blocker == "diag-summary-invalid"
    assert not gate7.complete
    assert gate7.missing == "retained-diag-begin"


def test_wifi_diag_context_requires_exact_matching_nonzero_id() -> None:
    """Non-authoritative context cannot splice into another terminal row."""

    context = (
        "wifi: diag_context id=37 retained=complete/pass/none "
        "cause=issued-owner-unknown trigger=rx-queue-poison"
    )
    detail, scope, cause, trigger, retained = (
        normalizer.summarize_wifi_diag_complete(
            normalizer.parse_events(
                [context, wifi_diag_complete_line(37, 41, 43)]
            )
        )
    )
    assert (detail, scope) == ("yes", "current")
    assert (cause, trigger, retained) == (
        "issued-owner-unknown",
        "rx-queue-poison",
        "complete/pass/none",
    )

    lines = [
        context.replace("id=37", "id=36"),
        wifi_diag_complete_line(37, 41, 43),
    ]

    detail, scope, cause, trigger, retained = (
        normalizer.summarize_wifi_diag_complete(
            normalizer.parse_events(lines)
        )
    )

    assert detail == "yes"
    assert scope == "current"
    assert cause == "none"
    assert trigger == "none"
    assert retained == "none"


def test_hdmi_passive_status_requires_driver_completion_receipt() -> None:
    """Queued display work becomes responsive only after a driver receipt."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                "hdmi: status mode=passive source=usb-status state=ready "
                "blocker=none receipt=driver-task-completion next_action=none",
                "hdmi: driver contract=hdmi-text counters=present active=no "
                "submitted=104 completed=103 outstanding=0 no_reply_streak=0 "
                "cooldown=0 stale=no",
            ]
        )
    ).to_record()

    assert record["HDMI_STATUS_STATE"] == "ready"
    assert record["HDMI_STATUS_BLOCKER"] == "none"
    assert record["HDMI_STATUS_RECEIPT"] == "driver-task-completion"
    assert record["HDMI_DRIVER_OUTSTANDING"] == 0
    assert record["HDMI_RESPONSIVE_PROOF"] == "yes"


def test_hdmi_passive_status_rejects_missing_or_inconsistent_driver_receipt() -> None:
    """Passive ready text cannot replace one exact current driver receipt."""

    status = (
        "hdmi: status mode=passive source=usb-status state=ready "
        "blocker=none receipt=driver-task-completion next_action=none"
    )
    invalid_driver_rows = (
        None,
        "hdmi: driver contract=hdmi-text counters=present active=yes "
        "submitted=104 completed=103 outstanding=0 no_reply_streak=0 "
        "cooldown=0 stale=no",
        "hdmi: driver contract=hdmi-text counters=absent active=no "
        "submitted=0 completed=0 outstanding=0 no_reply_streak=0 "
        "cooldown=0 stale=no",
        "hdmi: driver contract=hdmi-text counters=present active=no "
        "submitted=104 completed=103 outstanding=0 no_reply_streak=1 "
        "cooldown=0 stale=no",
        "hdmi: driver contract=hdmi-text counters=present active=no "
        "submitted=104 completed=103 outstanding=0 no_reply_streak=0 "
        "cooldown=0 stale=yes",
    )

    for driver_row in invalid_driver_rows:
        lines = [status]
        if driver_row is not None:
            lines.append(driver_row)
        record = normalizer.summarize_gates(
            normalizer.parse_events(lines)
        ).to_record()
        assert record["HDMI_RESPONSIVE_PROOF"] == "no"

    spliced_record = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                status,
                "this raw serial line is deliberately unclassified",
                "hdmi: driver contract=hdmi-text counters=present active=no "
                "submitted=104 completed=103 outstanding=0 no_reply_streak=0 "
                "cooldown=0 stale=no",
            ]
        )
    ).to_record()
    assert spliced_record["HDMI_RESPONSIVE_PROOF"] == "no"


def test_usb_diag_liveness_reports_real_post_command_input_delta() -> None:
    """USB diagnostic liveness is based on linked-runtime HID byte deltas."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                "usb: diag_liveness generation=4 status=pass "
                "proof=one-shot flow_delta=1/1/1/1 drop_delta=0 "
                "source=linked-runtime-hid next_action=none"
            ]
        )
    ).to_record()

    assert record["USB_DIAG_LIVENESS_GENERATION"] == 4
    assert record["USB_DIAG_LIVENESS_STATUS"] == "pass"
    assert record["USB_DIAG_LIVENESS_BACKEND_DELTA"] == 1
    assert record["USB_DIAG_LIVENESS_ACCEPTED_DELTA"] == 1
    assert record["USB_DIAG_LIVENESS_DRAINED_DELTA"] == 1
    assert record["USB_DIAG_LIVENESS_ECHOED_DELTA"] == 1
    assert record["USB_DIAG_LIVENESS_DROP_DELTA"] == 0
    assert record["USB_GATE_SCOPE"] == "startup"
    assert record["USB_CURRENT_LIVENESS"] == "pass"
    assert record["USB_CURRENT_LIVENESS_REASON"] == "fresh-key-path-complete"
    assert record["USB_PHYSICAL_INPUT_PROOF"] == "yes"


def test_usb_startup_byte_does_not_prove_current_keyboard_liveness() -> None:
    """Latched startup counters cannot hide a later keyboard-input death."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                "[local-seat] usb keyboard command-ready "
                "source=linked-runtime-hid clean_polls=2 no_reply=0 recovery_pending=no",
                "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
                "first_byte_source=linked-runtime-hid command_ready=yes "
                "proof_gate=10 blocker=none",
                "[smp] activity local-seat-input backend_polls=23896 "
                "backend_bytes=1 queued=0 arming=0 accepted=1 drained=1 "
                "echoed=1 drop=0 no_reply=0 cooldown=0 cooldown_skips=0",
            ]
        )
    ).to_record()

    assert record["USB_GATE"] == 10
    assert record["USB_GATE_SCOPE"] == "startup"
    assert record["USB_CURRENT_LIVENESS"] == "unproven"
    assert record["USB_CURRENT_LIVENESS_REASON"] == "diagnostic-not-run"
    assert record["USB_PHYSICAL_INPUT_PROOF"] == "no"


def test_usb_runtime_skip_after_first_byte_is_scheduler_telemetry() -> None:
    """Input-first runtime skips must not downgrade a working USB keyboard."""

    record = normalizer.summarize_gates(
        normalizer.parse_events(
            [
                "usb: runtime_gate keyboard=yes first_report=yes first_byte=yes "
                "first_byte_source=linked-runtime-hid proof_gate=10 blocker=none",
                "[local-seat] usb keyboard command-ready "
                "action=enable-command-input clean_polls=2 no_reply=0 "
                "recovery_pending=no",
                "[smp] activity local-seat-turns output_polls=0 hdmi_pump=6 "
                "net_mirror=0 net_suppressed=0 priority=1 skipped=1 "
                "serial_yield=1 post_runtime=0",
            ]
        )
    ).to_record()

    assert record["USB_EVENT_LOOP_RUNTIME_SKIPPED"] == 1
    assert record["USB_POST_FIRST_BYTE_BLOCKER"] == "none"
    assert record["USB_ACTIVE_BLOCKER_SEEN"] == "no"
    assert record["USB_BUSY_AFTER_READY"] == "no"
