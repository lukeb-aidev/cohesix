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
Changes: coh-rtc generated skills and false-default control, authenticated Hive Gateway JSON-RPC/SSE projection, selected live matrix/runner, host/API/security/Python documentation and focused tests.
Commands: cargo test --locked -p coh-rtc; cargo test --locked -p hive-gateway --test agent_protocol_controls --test a2a_protocol --test a2a_jobs; focused A2A task-state unit test; focused provider-matrix tests; scripts/check-generated.sh; m28e-a2a-live selected case.
Checks: Scoped Agent Card, bounded malformed/auth/refusal handling, original admission-to-task identity, native outcome mapping, pending cancellation, restart/reconnect, SDK/NeMo compatibility and exact-source CUDA/PEFT provider results.
Deliverables: Selected generated catalogue, service and peer contract, lifecycle guide, independent m28e-a2a-live record and Complete status after acceptance.
```

## Selected contract

A2A JSON-RPC 0.3.0 is pinned to `a2a-sdk==0.3.26` for the installed NeMo
Agent Toolkit 1.9.0 client. The compiler selects CUDA and PEFT skills only
when their standing actions exist. The QEMU host profile enables the A2A
route; the Pi profile remains disabled. The task ID is the original host ticket
and admission ID. Both task creation forms enter the existing preflight and
submit path. Task lookup and SSE reread the shared durable ledger and original
target result; task cancellation uses the existing cancellation authority.
No A2A-owned queue, task store, model-byte artifact or provider verifier was
added. A successful A2A task reports the observed native terminal but leaves
`providerVerified=false` until the independent shared verifier checks the
result. Missing target results and uncertain effects remain `unknown`.

## Focused validation and proof limits

The compiler, gateway protocol, task-mapping, provider-matrix, generated-file
and live-case checks are recorded with exact commands and results when the
selected candidate is qualified. This implementation record does not by
itself claim live provider work, physical Pi behavior or assembled Release B.

AI assistance contributed implementation and test code under the named task.
The source, generated outputs, tests and native evidence remain the reviewable
authority for each claim.
