---
name: cohesix-model-rollout
description: Choose and verify a model rollout on a selected Mac MLX host, Linux CUDA host or both. Use for model comparison, optional transfer, canary, promotion or rollback; not for training an individual PEFT adapter.
license: Apache-2.0
---
<!-- Author: Lukas Bower -->
<!-- Purpose: Keep platform-specific model choice, optional byte transfer and serving generation distinct and verifiable. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Roll out a selected model

Use a matching installed release and the deployment's selected host profile:
Apple Silicon macOS with admitted MLX, Linux AArch64 NVIDIA with admitted CUDA,
or both. A Mac or Linux controller can coordinate a remote selected host.
Read the same-version [host operation guide](../../docs/HOST_TOOLS.md#swarmui),
[private release guide](../../docs/PRIVATE_LORA_RELEASE.md) and
[Release B host/client contract](../../docs/BUILD_PLAN.md#release-b).
The Mac's local MLX workbench is useful for exploration; its local observation
does not itself admit a Cohesix job. A Linux CUDA result must come from its
selected native executor. Use only the stages and transfer path advertised by
the installed profile.
Inspect each candidate host's OS, accelerator, model format, runtime and
capacity before comparing or activating. If a combination is unsupported,
explain the exact incompatibility and offer a compatible selected host or a
separately qualified format/runtime change; do not assume Mac and Linux
installations can load the same artifact.

## Make the choice reproducible

Before work starts, name the application task, held-out input set, quality
measure, budget, selected base/model/adapter digests, runtime versions and
provider/host for each stage. Compare actual outputs and resource use on each
selected host. For a single-host rollout, compare candidates on that host;
for a mixed rollout, keep Mac Metal and Linux CUDA measures separate. Report
unsupported formats, stale capacity or incomparable evaluation as blockers,
not as a winner. A vMLX model response can be a serving observation; its MCP
client role and any A2A peer using it require separate configured compatibility.

Write a decision record with the chosen candidate and why it beat the
incumbent. Preserve stage/job identities and original admission IDs. A
single-host rollout has no cross-host transfer step. When the selected
operation needs to move model bytes, first establish that the installed
release and host pair actually advertise verified distribution.
A Queen reference or FUSE listing is only a reference. Record the authorised
source, destination and payload route; require the full destination digest
and receipt before a **separate** activation decision. Do not infer transfer
from a client timeout, URL or chunk acknowledgement.

## Prove the generation users reached

Preflight the destination's model/runtime compatibility and available
capacity. Admit the selected release, observe the native load, then send a
real application canary to the exact candidate generation. Promote only after
its predeclared quality and service checks pass. On failure, use the original
job lineage to inspect or recover, then observe a client reaching the known
good incumbent after any rollback. A successful control action, a `g1`
diagnostic label and a running server process are not serving proof.

Return a compact flight record: candidates and measures, chosen host at each
stage, source and destination hashes if bytes moved, admission IDs, canary
generation and output, promotion or rollback result, shared-verifier outcome
and unresolved questions. Route adapter-specific training/import and held-out
release mechanics to the [private adapter skill](../cohesix-private-adapter-release/SKILL.md);
route contested evidence to the [evidence skill](../cohesix-evidence/SKILL.md).
