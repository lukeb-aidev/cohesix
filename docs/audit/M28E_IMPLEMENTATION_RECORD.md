<!-- Author: Lukas Bower -->
<!-- Purpose: Record selected M28e A2A implementation, validation and exact evidence boundaries. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# M28e implementation record — selected durable A2A jobs

```text
Title/ID: m28e-a2a-durable-jobs
Milestone: 28e / m28e-a2a-durable-jobs
Goal: Delegate and recover selected CUDA and PEFT jobs through A2A without a second scheduler or authority.
Inputs: Completed M28 shared jobs, M28a CUDA, M28b PEFT, selected manifest and provider registry; A2A 0.3.0 and the installed a2a-sdk 0.3.26/NeMo 1.9.0 client.
Changes: coh-rtc generated skills and false-default control; authenticated Hive Gateway JSON-RPC/SSE projection; Root deadletter reconciliation; selected live matrix/runner; host/API/security/Python documentation and focused tests.
Commands: cargo test --locked -p coh-rtc; cargo test --locked -p hive-gateway --bin hive-gateway; cargo test --locked -p hive-gateway --test agent_protocol_controls --test a2a_protocol --test a2a_jobs; focused Python provider-matrix and release tests; scripts/check-generated.sh; selected m28e-a2a-live case.
Checks: Scoped Agent Card, bounded malformed/auth/refusal handling, original admission-to-task identity, native outcome mapping, pending cancellation, restart/reconnect, SDK/NeMo compatibility and exact-source CUDA/PEFT provider results.
Deliverables: Selected generated catalogue, service and peer contract, lifecycle guide, independent m28e-a2a-live record and Complete status at the selected scope.
```

## Selected contract and repair

A2A JSON-RPC 0.3.0 is pinned to `a2a-sdk==0.3.26`. The compiler selects
CUDA and PEFT skills only when their standing actions exist. The selected QEMU
host profile enables A2A; Pi keeps it disabled. Task IDs remain the original
host ticket and admission IDs. Creation enters the shared preflight and submit
path; lookup and bounded SSE reread the standing ledger and Root's original
ticket result. Cancellation records a request under existing authority. No
A2A queue, task store, model-byte artifact or verifier was added.

M28e live testing discovered that the completed M28
`m28-selected-job-contract` reconciliation read Root status but omitted its
deadletter terminal. The scoped restoration reads both for the same exact
ticket/admission identity and retains their line digests. The A2A projection
also returns an explicit original-ID error for an uncertain Root write and
maps a reservation with an expired decision to `unknown`; it never reports
that an uncertain write was submitted successfully. These are the discovery
task `m28e-a2a-durable-jobs` and restoration owner
`m28-selected-job-contract` for the shared result path. Task completion
reports a native terminal with `providerVerified=false`; the independent
CUDA output check or shared PEFT verifier owns provider acceptance.

## Exact-source live evidence

The selected source was `7bb8080f10be98a3ee2436ff3b9a7512e9390fd9`.
The resolved QEMU manifest SHA-256 was
`91e305af3b758474941822978dca331fb40b4b692086f59f82843c9e7b42653c`.
The Mac seL4 profile validation and selected image build passed. Its
rootserver SHA-256 was
`6fc5e4f3837acc66d69a8b0f3e5bf73ea165b047e6de3d2ff988e864a727749d`;
the CPIO SHA-256 was
`cc55f2ee3f232eee091ae732ff53c64077832b72481cb7c56385a097b5cba395`.
Pinned QEMU 10.1.0 KVM on the Jetson Orin Nano booted that image with
`[BUILD] 7bb8080f10be-dirty` and `root-console.start.ok`. The dirty suffix
records private selected-manifest projections in the isolated build tree;
the runner checked the exact committed source, live PID, loader paths, image
hashes, authenticated Queen manifest and source marker.

The standard SDK created CUDA task `m28e-reference-systemd-01` through A2A.
After gateway and agent restart, SDK resubscription returned `completed` for
the same ID. Root and the standing ledger retained
`claimed -> running -> succeeded`, confirmed execution, acknowledged delivery
and result SHA-256
`c5e62a95944a2b6fa426b2c1ab0b15b36fac4dcba87213fc04a49ae8a3c30a28`.
The independent M28a byte-level CUDA output verifier passed on the original
input and native output. A completed-task cancellation request did not change
that outcome; it was not used as proof of a pending cancellation.

A2A PEFT task `m28e-a2a-peft-01` used request SHA-256
`2a8e16dc2d12fa549af05a069199af529ec6b4824fa0de30c29c5d79ff9c13b7`.
Native validation, training, evaluation, scan, stage, load, canary and promote
all succeeded against the current accepted baseline. The native serving
generation advanced to 8. After a second gateway and agent restart, SDK
resubscription returned the same completed task. Root and the ledger retained
`claimed -> running -> succeeded`, confirmed execution, acknowledged delivery
and result SHA-256
`b65b948bc56f65bf3f9fc7b9bf10ec99756fd8a7764a5701c763059016323b9c`.
The shared `coh peft release verify` accepted its independently signed graph
SHA-256
`8717047ed80900f8dbb0c0c2ca5514abdd3c344232576903bc37ef29f541c16c`.
The source-matched Python view agreed on both admissions and the PEFT verified
outcome without submitting either effect again.

NeMo Agent Toolkit 1.9.0's native A2A client read the scoped card, looked up
the completed CUDA task and issued its cancellation method. A different
subject could not read that task, and exhausted-budget and revoked scopes
refused new A2A requests without a target job. The selected
`m28e-a2a-live` runner passed with `proof_class=live_target`; its reference
and summary are under
`/mnt/nvme/cohesix-dev/m28e-final-20260926/source/out/private/m28e-session/`.
Private tickets, request credentials, native records and full SDK captures
remain there, outside the repository commit.

## Focused validation and limits

The following passed on the final source: `cargo test --locked -p coh-rtc`;
`cargo test --locked -p hive-gateway --bin hive-gateway` (100 tests);
`cargo test --locked -p hive-gateway --test agent_protocol_controls --test
a2a_protocol --test a2a_jobs` (4 tests);
`out/toolchain/sel4-profile-venv/bin/python -m pytest -q
tests/test_provider_matrix.py` (4 tests) and the same interpreter against
`tools/cohesix-py/tests/test_model_release.py` (22 tests);
`scripts/check-generated.sh`; and
`scripts/ci/provider_conformance_run.sh --matrix
configs/provider_conformance.toml --case m28e-a2a-live` with the retained
private reference and `jetson-orin-nano-jp7` profile. The full suite was not
run.

Earlier private trials with a missing LoRA Worker, uncertain Root writes and
an incompatible native PEFT store remain diagnostic failures, not acceptance
evidence. This component result covers the selected Linux CUDA/PEFT and KVM
path. The mixed MLX/CUDA, weight-distribution and vMLX composition conditions
were not selected; Pi A2A remains disabled. It does not qualify physical Pi
behavior or assembled Release B.

AI assistance contributed implementation and test code under the named task.
Source, generated outputs, focused tests and native evidence remain the
reviewable authority for each claim.
