<!-- Author: Lukas Bower -->
<!-- Purpose: Track Milestone 26c host/VM NineDoor semantic parity claims and evidence. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C NineDoor Parity Matrix

Status: `SESSION-MODEL-PARITY-CLOSED / LIVE-WORKER-EXECUTION-REOPENED`

This matrix closes QEMU host/root session-model parity and records the 26c Pi 4
TCP/REST projection proof. It does not prove a live Worker task, endpoint-cap
delivery, notification handling, or applied Worker scheduling.

| Surface | Host Evidence | VM/Root Evidence | Status | Notes |
| --- | --- | --- | --- | --- |
| 9P version | `out/audit/gate/20260628T015332Z/workspace-tests.log` includes NineDoor integration tests passing; `out/audit/gate/20260628T015332Z/secure9p-codec-tests.log` PASS | Generated `SECURE9P_LIMITS` keep `msize = 8192`; QEMU regression batch PASS | PASS-QEMU | 9P2000.L remains mandatory. |
| Path validation | `cargo test -p secure9p-codec` PASS in saved gate log | QEMU regression batch and root generated limits stayed aligned | PASS-QEMU | Codec rejects invalid paths, truncated frames, trailing request bytes, and trailing response bytes. |
| Attach ticket validation | NineDoor integration covers worker command rejection and GPU worker flows; `cargo test -p tests` covers `worker_attach_without_ticket_is_rejected` | Root event handling enforces Worker-role tickets; `worker_authority.rs` reports every target role non-executable and endpoint authority disabled | PASS-QEMU-SESSION | Ticket acceptance binds a namespace session only; it does not start or attach a Worker TCB. |
| Worker namespace listing | NineDoor integration covers spawn/emit/kill, GPU worker job flow, and session-scoped bind behavior | Generated Worker role records are all non-executable | PASS-QEMU-MODEL | Listing and model lifecycle parity are not live Worker execution evidence. |
| Telemetry append bounds | Secure9P session/bounds tests and NineDoor integration passed in saved gate logs | Root-task lib tests and QEMU regression batch passed in saved gate logs | PASS-QEMU | Grammar and output shape remain protected by the saved QEMU gate evidence. |
| Schedule queue | NineDoor integration and QEMU regression batch passed in saved gate logs | Generated `non-mcs` priority/domain/service-turn values are metadata only | PASS-QEMU-MODEL | No Worker TCB exists on which to apply the record. |
| Lease queue | NineDoor integration covers GPU lease expiry/revocation behavior | Root/host models retain lease state; Worker endpoint authority and lifecycle notifications are disabled | PASS-QEMU-MODEL | Lease model parity is not endpoint-cap authority. |
| Worker notification lifecycle | Worker helper code exposes receipt-only GPU/LoRA event handling without host GPU/training execution | Generated notification requirements are disabled | NOT-ACTIVE | Helper events do not prove notification-object creation, cap delivery, or handling by a live task. |
| Error mapping | NineDoor integration, Secure9P codec tests, and QEMU regression batch passed in saved gate logs | Root-task and console grammar tests passed in saved gate logs | PASS-QEMU | ACK/ERR/END and NineDoor error semantics still cannot drift. |
| Pi TCP projection | `out/test-plan/m26c-pi4-live/cohsh-tcp-proof-genet-latest.txt`; Stage 03 PASS in `out/test-plan/m26c-pi4-live` | GENET console listener accepted authenticated `cohsh` at `192.168.10.50:31337`; runtime/DMA proof remained fresh-pi/counter-qualified | PASS-PI4 | This is console/TCP proof only, not a new in-VM 9P/TCP listener. |
| Pi REST projection | Stage 04 PASS through `hive-gateway` at `http://127.0.0.1:48080` with `m26c-pi4-rest-token` | Gateway projected documented console/NineDoor semantics over the authenticated Pi TCP console | PASS-PI4 | REST remains a host-side projection and does not add VM authority. |

## Existing QEMU Closure Evidence

- `out/test-plan/m26c-qemu/stage_01.done` through `stage_05.done`
- `out/test-plan/m26c-qemu/logs/stage-05-due-diligence.log` - `PASS due-diligence-gate`
- `out/audit/gate/20260628T015332Z/secure9p-codec-tests.log` - PASS
- `out/audit/gate/20260628T015332Z/integration-tests.log` - PASS
- `out/audit/gate/20260628T015332Z/workspace-tests.log` - PASS
- `out/audit/gate/20260628T015332Z/regression-batch.log` - PASS

## Pi 4 Closure Evidence

- `/Users/lukasbower/pi4-serial-20260629-135454.log`
- `/Users/lukasbower/tcpdump-usb-eth-20260629-135504.pcap`
- `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env`
- `out/test-plan/m26c-pi4-live/cohsh-tcp-proof-genet-latest.txt`
- `out/test-plan/m26c-pi4-live/stage_03.done`
- `out/test-plan/m26c-pi4-live/stage_04.done`
- `out/test-plan/m26c-pi4-live/stage_05.done`
- `out/audit/gate/20260629T061204Z/workspace-tests.log` - PASS

## Remaining Open Evidence

- Any future claim of full cap-bundle authority needs its own generated
  manifest, tests, and acceptance evidence.
