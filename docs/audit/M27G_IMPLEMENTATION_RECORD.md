<!-- Author: Lukas Bower -->
<!-- Purpose: Retain integrated M27g qualification decisions, exact evidence boundaries and defect restoration records. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# M27g implementation and qualification record

Milestone 27g remains In Progress. The required successful physical two-hour
burn-in and subsequent complete staged Test Plan have not been established.
Historical component acceptance and earlier failed attempts retain their original
verdicts. The final release decision requires the named evidence and human review.

## Native CUDA refusal restoration

```text
Title/ID: m27g-native-cuda-refusal-restoration
Milestone: 27g / m27g-two-hour-pi4-operator-burn-in; restoration: 27b / m27b-provider-action-registry
Goal: Preserve a native executor refusal as a refusal and reconcile bound admission failures without dispatch or replay.
Inputs: Pi image 23513e1c955e, host source 683587f6a, selected Pi manifest e38454f6667d923fc6e3368ca2b9d44aba7ec5407fd9695e4aa526aec035dcaf, m27g-run-01 and native UI revalidation evidence.
Changes:
  - apps/gpu-bridge-host/src/workload.rs — persist stale-input and insufficient-ticket-time failures with the exact input before replying; authenticate and type explicit refusal replies.
  - apps/host-ticket-agent/src/executors/workload.rs — preserve known pre-dispatch refusals; retain ambiguous handling for transport, authentication, retained-output and idempotency errors.
  - docs/GPU_NODES.md and docs/HOST_TOOLS.md — document failure reconciliation and inventory freshness during desktop admission.
Commands:
  - cargo test -p gpu-bridge-host --lib workload::tests -- --nocapture
  - cargo test -p host-ticket-agent --lib executors::workload::tests -- --nocapture
  - cargo clippy -p gpu-bridge-host -p host-ticket-agent --all-targets -- -D warnings
  - scripts/check-generated.sh
  - scripts/ci/check_test_plan.sh
Checks: Deterministic stale input and short ticket failures retain exact bytes and terminal time without creating a native child; replay/recovery return the same failure. Unauthenticated or mismatched replies never establish a refusal. Fresh native/Worker checks and a new full timed attempt remain required.
Deliverables: Focused command logs, new exact host packages, native refusal and ordinary-work evidence, failed-attempt records and a new 120-minute burn-in before full release testing.
```

The original timed native UI case omitted the required planning step. Its
workload was never admitted, and its exact lease was subsequently released with
a confirmed Worker receipt. A separate UI revalidation correctly refused stale
inventory. The next case admitted successfully, but its harness observed the
first evidence record before the complete grant prefix. Its delayed execution
then exposed a product defect: an explicit native refusal was mapped to pending
provider state, requiring expiry reconciliation. Each original failure remains
retained under `out/burn-in/m27g-run-01`; correcting a harness does not relabel it
as a pass. The product recovery change is classified as major and requires a new
complete 120-minute attempt, as authorized by the operator.

Compatibility review covers coh/coh-status, cohsh, Hive Gateway, SwarmUI,
host-ticket-agent, host-sidecar-bridge, gpu-bridge-host, cas-tool, sidecar-bus,
`tools/cohesix-py`, and performance benchmark scripts. The existing authenticated
socket response and job/journal/evidence schemas, manifest policy, five-second
inventory lifetime, quotas and performance thresholds remain unchanged. Recipe
consumers already reconcile failed jobs and preserve failed requested outcomes.
No other implementation change is required; rebuild affected package dependencies
and test their native composition. These focused checks do not replace the full
post-burn staged target, pressure, due-diligence or release promotion gates.
