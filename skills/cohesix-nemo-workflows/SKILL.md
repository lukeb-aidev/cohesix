---
name: cohesix-nemo-workflows
description: Connect the pinned Linux Cohesix NeMo Agent Toolkit kit, run or recover one MCP/A2A job, and compare direct versus governed agent workflows. Use for that kit's client, model and evaluation journey; other clients use agent delegation.
license: Apache-2.0
---
<!-- Author: Lukas Bower -->
<!-- Purpose: Give NeMo users a practical native-client and evaluation workflow with original-job recovery and independent verification. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Connect a NeMo workflow to permitted work

Use the matching [Cohesix NeMo kit guide](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/integrations/nemo-agent-toolkit/README.md)
and [agent protocol guide](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/HOST_TOOLS.md#selected-mcp-clients),
pinned to the selected source revision or read from the installed kit.
The 1.2.0 selection is Linux AArch64 with NeMo Agent Toolkit 1.9.0,
MCP SDK 1.29.1 and A2A SDK 0.3.26. Preserve its locked environment separately
from CUDA/PEFT. A newer upstream feature or a Mac NeMo installation does not
qualify this kit. For setup, load `cohesix-ai-host-setup`; for another agent
client, load `cohesix-agent-delegation`.

## Establish one subject and a working client

Verify the kit wheel/lock identities and `pip check`. Resolve `COH_NEMO` to
the absolute installed `cohesix-nemo` executable, including in non-login SSH
or service contexts. Use the existing HTTP gateway as the sole Queen TCP owner.
Give one client process a private `0600` settings file with `gateway_url`,
`subject`, `request_auth_ref` and `delegated_ticket_ref`. The subject label does
not grant authority; the verified delegated ticket does. Use HTTPS or protected
loopback HTTP. Keep credentials out of argv, model input and executor payloads.

```bash
: "${COH_NEMO:?set the absolute installed kit executable}"
: "${NEMO_SETTINGS:?set the private subject settings JSON path}"
"$COH_NEMO" doctor --settings "$NEMO_SETTINGS"
"$COH_NEMO" mcp --settings "$NEMO_SETTINGS"
"$COH_NEMO" a2a --settings "$NEMO_SETTINGS"
```

Use discovery to select the requested protocol and advertised action; do not
silently switch protocols. New admission needs live available jobs/skills,
fresh scope and cumulative standing budget. A client cannot enable a disabled
protocol or renew a spent scope. For two users, use separate processes,
settings and credentials; another user's task must remain inaccessible.

## Run once, then recover the same job

For authorized execution, prepare the kit's request JSON with `scope_id` and
the complete immutable `host-ticket/v2` from the selected CUDA/PEFT recipe.
Use a stable original ticket/idempotency identity and a fresh private output
path. Follow the guide's `mcp --request` or `a2a --request` command. First prove
the deterministic native client path; an optional model planner adds no scope.

After a timeout or disconnect, inspect the original ID using
`mcp --recover-id ORIGINAL_TICKET_ID` or
`a2a --get-task-id ORIGINAL_TICKET_ID`, retaining the same settings and a new
output filename. Output files are create-only. Recovery reads consume the
caller's finite read quota even when no new admission is needed.
Some kit paths require discovery before recovery: if a spent scope hides new
job tools/skills and recovery reports them unavailable, retain that error and
use the gateway's same-ID `cohesix.inspect_job`/`cohesix.recover_job` or native
A2A `tasks/get` under existing read authority. Load the delegation guide for
that client. Do not mint scope, submit again or use a new job ID to get a reply.
Missing recovery access leaves the result unresolved for the operator.

Run the independent CUDA byte check or exact `coh peft release` verifier and
serving canary before claiming the requested effect. A2A's
`provider_verified=false`, a cancel request and `recovered_failure` must keep
their actual meanings. Cancellation is native termination only after its
owner observes it.

## Add a planner or measure value when requested

Use the kit's `agent-mcp` or `agent-a2a` mode with separate protected model
settings, one fixed input file and an unused trace log. Pin the loaded model
ID and endpoint. The kit verifies the selected name through `/v1/models`;
only actual tool calls followed by independent job evidence establish work.

For an evaluation request, freeze 1–16 dataset rows, expected answers, model,
task IDs, quality criterion and budget. Use `eval-direct`, then the selected
`eval-mcp` or `eval-a2a` in unused private directories. Read the exact
[benchmark contract](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/BENCHMARKS.md#nemo-agent-toolkit-190-comparison-m28f).
Compare actual row completion, tool invocation, output quality, latency and
manual recovery. Check Toolkit's standardized CSV and native traces: zero
exit with no completed model rows is insufficient, and a completed row does
not independently verify an answer or effect. Return measured results,
original job identities, refusals, recovery limitations and native evidence;
do not infer savings or whole-client compatibility from discovery.
