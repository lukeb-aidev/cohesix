<!-- Author: Lukas Bower -->
<!-- Purpose: Track Milestone 26c host/VM NineDoor semantic parity claims and evidence. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C NineDoor Parity Matrix

Status: `QEMU-CLOSED / PI4-PROOF-OPEN`

This matrix closes QEMU host/VM semantic parity only. It does not close live
Pi 4 hardware proof, isolated Pi runtime/DMA proof, or future cap-bundle
authority beyond the current generated endpoint-badge checks.

| Surface | Host Evidence | VM/Root Evidence | Status | Notes |
| --- | --- | --- | --- | --- |
| 9P version | `out/audit/gate/20260628T015332Z/workspace-tests.log` includes NineDoor integration tests passing; `out/audit/gate/20260628T015332Z/secure9p-codec-tests.log` PASS | Generated `SECURE9P_LIMITS` keep `msize = 8192`; QEMU regression batch PASS | PASS-QEMU | 9P2000.L remains mandatory. |
| Path validation | `cargo test -p secure9p-codec` PASS in saved gate log | QEMU regression batch and root generated limits stayed aligned | PASS-QEMU | Codec rejects invalid paths, truncated frames, trailing request bytes, and trailing response bytes. |
| Attach ticket validation | NineDoor integration covers worker command rejection and GPU worker flows; `cargo test -p tests` covers `worker_attach_without_ticket_is_rejected` | `apps/root-task/src/event/mod.rs` still enforces worker tickets; `apps/root-task/src/worker_authority.rs` requires generated endpoint badges for implemented worker roles | PASS-QEMU | Current authority is endpoint-badge validation for implemented roles, not full future cap-bundle transfer authority. |
| Worker namespace listing | NineDoor integration covers spawn/emit/kill, GPU worker job flow, and session-scoped bind behavior | Generated worker roles mark heartbeat, GPU, and LoRA implemented; worker-bus remains deferred | PASS-QEMU | Placeholder-only semantics are no longer the QEMU state for implemented worker roles. |
| Telemetry append bounds | Secure9P session/bounds tests and NineDoor integration passed in saved gate logs | Root-task lib tests and QEMU regression batch passed in saved gate logs | PASS-QEMU | Grammar and output shape remain protected by the saved QEMU gate evidence. |
| Schedule queue | NineDoor integration and QEMU regression batch passed in saved gate logs | Generated scheduling evidence is profile-qualified as `non-mcs` with priority/domain/service-turn fallback | PASS-QEMU | This closes the QEMU non-MCS evidence item; it does not claim consumed MCS budget evidence. |
| Lease queue | NineDoor integration covers GPU lease expiry/revocation behavior | `worker_authority` models `LeaseRenewal` endpoint badges; worker loops handle lease notifications and expiry | PASS-QEMU | Lease authority is represented by generated endpoint-badge checks for implemented roles. |
| Worker notification lifecycle | Worker VM helper code exposes receipt-only GPU/LoRA loops without host GPU/training execution | Generated notification badges are event-specific; `worker_heart::worker_loop` handles revoke, shutdown, lease, and pressure events | PASS-QEMU | Full future cap-bundle notification isolation is not claimed. |
| Error mapping | NineDoor integration, Secure9P codec tests, and QEMU regression batch passed in saved gate logs | Root-task and console grammar tests passed in saved gate logs | PASS-QEMU | ACK/ERR/END and NineDoor error semantics still cannot drift. |

## Existing QEMU Closure Evidence

- `out/test-plan/m26c-qemu/stage_01.done` through `stage_05.done`
- `out/test-plan/m26c-qemu/logs/stage-05-due-diligence.log` - `PASS due-diligence-gate`
- `out/audit/gate/20260628T015332Z/secure9p-codec-tests.log` - PASS
- `out/audit/gate/20260628T015332Z/integration-tests.log` - PASS
- `out/audit/gate/20260628T015332Z/workspace-tests.log` - PASS
- `out/audit/gate/20260628T015332Z/regression-batch.log` - PASS

## Remaining Open Evidence

- Live Pi 4 hardware proof remains open.
- Isolated Pi runtime/DMA proof remains open.
- Any future claim of full cap-bundle authority needs its own generated
  manifest, tests, and acceptance evidence.
