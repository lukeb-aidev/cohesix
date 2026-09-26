---
name: cohesix-gpu-operations
description: Register, run or diagnose an approved Cohesix CUDA workload and establish its outcome across Queen, executor and evidence. Use for governed GPU jobs, not adapter promotion, CUDA installation or unrestricted shell access.
license: Apache-2.0
---
<!-- Author: Lukas Bower -->
<!-- Purpose: Bind operational GPU guidance to existing admission and independently verified outcomes. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Operate a governed GPU workload

Use Cohesix when the job needs existing admission, leases, bounded external
execution and verifiable results. If the user only needs local model training or
CUDA setup, route to that host's native tooling. Cohesix is not a training engine,
CUDA installer or general remote command runner.

## Establish fit before acting

Use one matching host-tool installation; `COH_BIN` is its absolute binary
directory and `COH_REST_URL` is the operator-approved existing gateway. Run from
the installation root. Check `"$COH_BIN/coh" --help` and
`"$COH_BIN/coh" gpu --help`. These instructions describe source interfaces;
check the installed bundle's VERSION.txt, manifest and bundled help first.
For copied skills, the web references remain usable; pin `main` to the selected
commit/tag or read the same document in that checkout.

For non-public reads, the operator must securely load
`HIVE_GATEWAY_REQUEST_AUTH_TOKEN` and `COH_REST_TICKET` with the required read
scope and finite quota. These do not authorize writes. Start by inspecting the
Queen identity, gateway ownership and read-only state
using [host tools](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/HOST_TOOLS.md). Separate the controller, QEMU/Pi Queen
and external executor. Run the matching `coh` host tools on macOS or Linux;
neither controller needs local CUDA when it submits to a remote executor.
The native CUDA executor itself must be a supported Linux AArch64 NVIDIA host.
Inspect its actual OS, architecture, device, driver, CUDA runtime and selected
provider before submission. If any requirement is missing, report the specific
incompatibility and offer the [AI host setup](../cohesix-ai-host-setup/SKILL.md)
path or a different compatible executor; do not treat a Mac controller or a
GPU listing as a CUDA substitute.

```bash
: "${COH_REST_URL:?set the existing gateway base URL}"
"$COH_BIN/coh" gpu --rest-url "$COH_REST_URL" list
```

Select `GPU_ID` from the actual result, never from an example or mock:

```bash
: "${GPU_ID:?select the published GPU identity}"
"$COH_BIN/coh" gpu --rest-url "$COH_REST_URL" status --gpu "$GPU_ID"
```

Inventory alone is insufficient. The [GPU contract](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/GPU_NODES.md)
requires a compatible prepared Linux AArch64 NVIDIA host, native device identity
and fresh publication, pinned helper/input artifacts, running bounded bridge and
ticket agent, current ready Worker and lease, exact provider graph and writer
epoch, and independently enrolled evidence custodians for signed proof. Inspect
actual availability and expiry; do not start or reconfigure these services as a
side effect of inspection. Missing setup is an operator prerequisite.

Jetson is one reference host, not the only possible Linux AArch64 CUDA host.
Check its actual driver/runtime/device and package compatibility; do not replace
board-managed drivers. MIG is not supported by the recorded Orin reference.
Allocation admission is not a hard GPU memory partition. See the
[foundation evidence](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/audit/M27B_IMPLEMENTATION_RECORD.md) for qualified
native paths and limits, not proof that this deployment has executed anything.

## Submit only the existing approved operation

Before mutation, establish the requested action, exact target/device, inputs,
limits, expiry, stable ticket/idempotency identity and scope of authorization.
The operator must supply a validated request file and deployed executor/trust
configuration conforming to [GPU nodes](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/GPU_NODES.md) and
[authority](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/M27A_AUTHORITY.md). Do not invent request identities,
credentials, fixture keys, admission fields or trust to fill missing prerequisites.
A documented command is not permission to execute it.

For an explicitly authorized submission with those prerequisites satisfied:

```bash
: "${GPU_REQUEST:?set the approved workload request JSON path}"
: "${COH_TICKET_REF:?set env:NAME or file:/absolute/path for the delegated ticket}"
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?load the gateway write credential securely}"
"$COH_BIN/coh" --ticket-ref "$COH_TICKET_REF" gpu \
  --rest-url "$COH_REST_URL" workload \
  --action gpu.workload.submit --spec "$GPU_REQUEST"
```

The request uses the current typed ticket contract; `coh` writes
`/host/tickets/spec`. Native execution stays on the configured executor.
Keep the credential environment private and avoid shell tracing. The gateway
write credential and delegated ticket are distinct; production policy also
checks intent, current authority and fencing. Never fall back to unrestricted
`coh run -- PROGRAM` or Python `run_command` to bypass a refusal. Those run on
the caller's host, and mock mode does not prevent local process execution.

## Establish the outcome, including after failure

Read `/host/tickets/status.snapshot` and `/host/tickets/deadletter.snapshot` through the existing
REST-backed `cohsh` shell, correlating the original ticket/action/native job.
At its `coh>` prompt:

```text
cat /host/tickets/status.snapshot
cat /host/tickets/deadletter.snapshot
```

Use bounded reads; retained logs can be incomplete. A submit ACK proves neither
native start nor completion. A status row or successful process exit alone does
not prove the requested output. Review the original durable journal, native
result, output hashes and exact Worker receipt, then run the signed verifier:

```bash
: "${GRAPH:?set the producer graph JSON path}"
: "${TRUST:?set independently enrolled external trust JSON path}"
: "${CAS:?set the retained content-addressed object directory}"
"$COH_BIN/coh" evidence verify --input "$GRAPH" --trust "$TRUST" --cas "$CAS"
```

Check the returned terminal **outcome**, exact request/input/device binding and
output verification. A valid signed failure or cancellation is not successful
work; successful cancel/observe proves that action, not the original submit.
[Causal evidence](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/CAUSAL_EVIDENCE.md) defines custody and proof limits.

After a timeout, lost ACK, stale lease/epoch, bridge restart, Busy or ambiguous
result, stop resubmission. Preserve the original identity and reconcile native
state plus journal/evidence. Do not choose a new ID, reset journals, renew expired
authority implicitly or repeat a mutation blindly. Cancel, cleanup and recovery
need their own current exact authorization; cancellation is confirmed only after
native termination. Missing/conflicting evidence remains unknown and requires
the operator or custodian. Never manufacture a receipt to finish a workflow.

## Choose the neighboring workflow

- Python `CohesixClient.gpu_workload_ticket(spec)` uses the same admitted path;
  it needs the matching generated target profile and authority. Follow
  [Python support](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/PYTHON_SUPPORT.md); a client object is not a grant.
- `coh plan ID` and `coh explain ID` read registered workflow declarations;
  IDs come from `coh providers`. A listed domain playbook can still refuse
  execution with `not_enabled`. Mock/dry-run plans do not establish its use case.
- For native PEFT or MLX training, comparison and served-generation proof,
  use [private adapter release](../cohesix-private-adapter-release/SKILL.md).
  Registry activation or a package probe cannot stand in for its native
  canary and shared-verifier result.
- For an MCP or A2A caller, use
  [agent delegation](../cohesix-agent-delegation/SKILL.md) with the matching
  generated catalogue and effective protocol controls. That client sees the
  same job identity and budgets; it cannot mint a CUDA outcome.
- For choosing between Mac MLX and CUDA hosts, approved model transfer and
  exact serving generation, use [model rollout](../cohesix-model-rollout/SKILL.md).

Report admitted, ready, running, terminal, verified requested outcome and
acceptance separately. Historical QEMU/native proof is not new Pi qualification.
