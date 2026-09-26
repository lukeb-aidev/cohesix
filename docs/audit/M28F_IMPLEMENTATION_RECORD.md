<!-- Author: Lukas Bower -->
<!-- Purpose: Record the selected M28f NeMo kit, live outcomes, adverse evidence and completion boundary. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# M28f implementation record — NeMo Agent Toolkit adoption kit

## Selected kit and topology

The selected client is a Linux AArch64 Jetson Orin Nano running NeMo Agent
Toolkit 1.9.0, `nvidia-nat-mcp` and `nvidia-nat-a2a` 1.9.0, MCP SDK 1.29.1 and
A2A SDK 0.3.26. The `cohesix-nemo-kit` 0.1.0 wheel SHA-256 is
`f9bda287604ee085f470195288ba7fefa88d507659430b06f11298ee4b83caad`.
The version-pinned Linux dependency-lock file SHA-256 is
`2d98723f2a4e539ef91048922b25230cb23cd73c76d390915b0781014181c2d8`.
The installer verified both digests, created a fresh non-editable venv,
installed the pinned dependencies and wheel, and passed `pip check`. The
portable [install summary](evidence/M28F_INSTALL_SUMMARY.json) identifies the
installed versions and private discovery-record digests. This is a tested
version lock and selected wheel digest, not a claim that every upstream wheel
has its own pinned distribution hash.

The kit uses Toolkit's own streamable-HTTP MCP client and per-user A2A client
and task helpers. Its small A2A plugin adds the gateway's authenticated Agent
Card request and structured job data part required by the installed Toolkit
release. Both clients use separate per-subject private `0600` settings and
gateway credential references. The verified delegated ticket, not Toolkit's
`user_id` or model output, decides scope. The selected MCP tools are
`available_selected_jobs`, `preflight_selected_job`, `submit_selected_job`,
`inspect_job` and `recover_job`; the A2A card advertises the selected CUDA
and PEFT skills. The package exposes separate MCP and A2A agent processes.
A combined protected process was not selected or claimed.

The gateway and a dedicated agent connected to an exact-source QEMU KVM Queen
on the Jetson. Target source commit
`7bb8080f10be98a3ee2436ff3b9a7512e9390fd9` used the selected manifest
SHA-256 `91e305af3b758474941822978dca331fb40b4b692086f59f82843c9e7b42653c`.
The live PID and `[BUILD]` identity were checked with rootserver SHA-256
`6fc5e4f3837acc66d69a8b0f3e5bf73ea165b047e6de3d2ff988e864a727749d`.
The pinned model endpoint was a loopback-forwarded local MLX server with
`mlx-community/Qwen3-4B-Instruct-2507-4bit`, revision
`50d427756c6b1b2fe0c0a10f67fbda1fc8e82c1b`. It supplied planner text
only; the Jetson CUDA provider and Cohesix signed outcomes remained separate.

## Live workflows and refusals

The native MCP client preflighted and submitted real systemd CUDA ticket
`m28f-reference-systemd-02`. Its first response was uncertain, so the client
recovered the same admission ID without another effect. The standing record
was confirmed and acknowledged, result SHA-256
`41007ca1717b795f26e0f169e79024429f9bb06a60b4de4fee2be1bc5bbecb91`;
the independent CUDA byte verifier passed. An earlier ticket ending `-01`
failed because its short inventory decision expired; that trial remains
adverse diagnostic evidence.

The native A2A client delegated HF PEFT task `m28f-a2a-peft-01`. The provider
ran validation, training, evaluation, scan, stage, load, canary and promotion;
the serving generation advanced from 8 to 9. The A2A task reported completed
with `provider_verified=false`. Independently, `coh peft release verify`
accepted signed graph SHA-256
`006becded8c2dd33ca7c9eae1b5b4a8a60a8d40fabc4349ed7bb5110d9fcd393`.
MCP recovered that original PEFT ID and A2A looked up the original CUDA ID,
showing one shared operation identity across both protocols. The initial
verifier preparation needed a corrected signed request and a fresh trust
observation; rejected diagnostic attempts remain private and were not
counted as success.

A second A2A PEFT candidate, `m28f-a2a-peft-reject-01`, changed the enrolled
evaluation-loss bound and failed policy-binding validation before training.
Its signed failed graph SHA-256 was
`8bc5a029267bf9361d93f196b0bec4efe27fa0b2c21b3f9e7f70a14c9fcc03a2`.
The shared verifier refused it, and accepted generation 9 remained healthy.
This establishes a failed-candidate refusal, not a measured inferior adapter.
An unauthorized MCP action was denied; a second subject could not inspect the
first subject's task. Neither refusal caused a target effect.

The one-slot `m28f-budget` scope admitted A2A CUDA ID
`m28f-reference-systemd-03`, then lost its Root write result. The original
reservation remained `reserved`, with no target result and no permitted effect
replay. An MCP attempt under `m28f-reference-systemd-04` was denied by the
same exhausted budget, including after gateway and agent restart. The A2A
cancellation helper returned an unknown outcome. The first job remains
**unresolved and reserved**; it is neither a confirmed success nor a confirmed
cancellation or released allocation. This adverse observation demonstrates
conservative cross-protocol budget enforcement, not recovery completion.

## Native Toolkit comparison and reproducibility

The kit's model-backed MCP agent invoked selected discovery, and its A2A
agent called the authenticated task helper and reported the original PEFT
task. Both ran from the pinned Toolkit environment. The effectful CUDA and
PEFT commands above used the same installed Toolkit native clients with
immutable private request files; planner text did not generate their tickets.

The fixed one-question dataset asked for the state and original ID of the
completed PEFT task. [Predeclared gates](../BENCHMARKS.md#nemo-agent-toolkit-190-comparison-m28f)
required the direct model to admit unknown state and the governed agent to
identify the correct completed task within 30 seconds, with `get_task` under
one second. Native `nat eval` and profiler recorded:

| Path | Answer and control | Whole workflow | Tool span |
| --- | --- | ---: | ---: |
| Direct pinned model | `Unknown`; no task tool or native receipt | 0.589 s | none |
| Governed per-user A2A | original ID and completed state via `get_task` | 12.778 s | 0.077 s |

The governed path spent more end-to-end time on model planning and client
setup; the measured A2A control span met the fixed budget. A preceding
diagnostic direct run falsely asserted task completion; it was retained as an
adverse model result. One task gives no quality distribution or tail-latency
claim. The direct path needs only the model credential but has no gateway
receipt or original-ID recovery. The governed path needs the private gateway
credential pair and a selected ticket; its signed verifier supplies useful
provider evidence. Native reconnect and original-ID lookup required no
resubmission. The unresolved budget attempt still requires operator
reconciliation. The model change from an inadequate 0.6B diagnostic model
to the selected 4B revision and the signed PEFT request correction were
manual setup work, not unattended recovery.

The [kit instructions](../../integrations/nemo-agent-toolkit/README.md)
give a second developer or agent the wheel-install, credential, request,
native workflow and evaluation commands. The evaluator for this record was
Codex using a new verified Jetson venv and the documented private reference;
there is no separate human reproduction claim. The sealed
[live summary](evidence/M28F_LIVE_SUMMARY.json) contains the exact IDs,
package/image/model identities and SHA-256 of each retained private evidence
file. Credentials, request bodies, model traces and native records remain in
the private Jetson test bed and are not published in this repository.

## Focused completion checks and boundary

The final checks were the eight focused `tests/test_nemo_agent_toolkit.py`
cases, four `tests/test_provider_matrix.py` cases, matrix validation, fresh
locked installation with `pip check`, native client discovery, model-backed
agents and direct/governed native Toolkit evaluation. The selected
`m28f-nemo-install` and `m28f-nemo-live` collectors both passed, the latter
with `proof_class=live_target` and independently verified provider outcomes.
The full test suite was not run. The component scope is Linux AArch64 Toolkit
clients, selected Jetson CUDA/PEFT providers and a KVM Queen. Physical Pi,
combined MCP/A2A process, mixed MLX/CUDA jobs, weight distribution, NeMo
Framework training, external NeMo catalog publication and assembled Release B
remain outside this completion claim.
