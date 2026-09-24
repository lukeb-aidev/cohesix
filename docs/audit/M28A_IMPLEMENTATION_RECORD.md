<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record M28a registered CUDA implementation, focused checks and exact-source completion evidence. -->
<!-- Author: Lukas Bower -->

# Milestone 28a implementation record

Milestone 28a is **complete** under `m28a-approved-user-workloads` and
`m28a-cuda-operations-and-recovery`. The exact-source systemd and Docker owner
cases below are admitted M28a target results at their stated proof classes.

## Task record

```text
Title/ID: m28a-approved-user-workloads; m28a-cuda-operations-and-recovery
Milestone: 28a / Useful CUDA Workloads and Reliable GPU Operations
Goal: Admit reviewed user CUDA packages and retain exact native outcome, output and recovery identity.
Inputs: accepted M28 selected jobs and standing ledger; 27c recipes; selected Jetson CUDA 13.2 profile; generated provider/recipe contracts.
Changes:
  - apps/gpu-bridge-host/src/{registered,workload,reference}.rs + cuda/batch_edges.cu — private digest-pinned enrollment, typed request, fresh device/headroom check, fixed child ABI, bounded output and independent native identity.
  - apps/host-ticket-agent/src/executors/workload.rs + apps/coh/src/{doctor,recipe,main}.rs + crates/coh-cli/src/lib.rs — preserve existing ticket/lease dispatch and recovery while exposing enrollment and diagnostics.
  - tools/cohesix-py/cohesix/workload.py + examples/cuda_batch_edges.py + apps/swarmui/src/workbench.rs — independent pixel verifier, user adaptation and host-owned workbench views.
  - configs/cuda_recipe.toml + tools/coh-rtc/src/recipe.rs + generated projections + provider/test catalogs — bounded versioned contract and selected live cases.
Commands: focused Rust and Python commands and selected systemd and Docker live commands in the evidence ledger.
Checks: reject changed registration/package/input, unsafe paths, stale inventory and typed-bound violations before native dispatch; verify exact GPU/output and settled original outcome in each advertised lane.
Deliverables: versioned enrollment, native diagnostics, reference and adaptation, recovery guidance, and exact-source systemd and Docker reports.
```

The implementation uses AI assistance for source, tests and this task record.
The reviewed executable remains privileged configuration, and its package,
registration, input, native device, target ticket and output digests have
separate checks. The v1 fixed reference remains decodable. The v2 request
contains no executable path, shell command or secret value; parameters are
typed data to the fixed `run` entrypoint. The existing bridge WAL and standing
ledger retain execution and reservation state separately. Failed or interrupted
effects are reconciled by original identity; a new effect requires a new
admission.

The host compatibility review covered `coh` and `cohsh` CLI boundaries, the
gateway/agent/bridge transport, SwarmUI workbench, `tools/cohesix-py` and the
generated provider inventory. The existing Python `gpu_workload_ticket` call
still submits the same target ticket; the new verifier is additive. The fixed
version 1 native provider conformance runner still constructs its original
request schema. `scripts/rest_perf_harness.py` maps the unchanged GPU action
codes and does not parse registered request bytes, so this change adds no
benchmark parser or claimed performance result.

The focused recovery review found that standing selected jobs admit GPU submit
and systemd restart, while GPU cancel is already an authenticated target ticket.
The live runner now writes one distinct `host-ticket/v2` cancellation through
that route, correlates its target terminal and native object, and never replays
an uncertain cancellation. The original selected submit remains in the standing
ledger until its result and capacity settle.

## Focused evidence so far

| Check | Result | Proof limit |
| --- | --- | --- |
| `cargo test --locked -p gpu-bridge-host --lib` | PASS, 29 tests | Source contract and bounded fault injection; no admitted target operation. |
| `cargo test --locked -p coh-cli --lib`; `cargo test --locked -p coh --lib doctor::`; `cargo test --locked -p coh --test recipe --test run` | PASS, 2 + 2 + 8 + 3 tests | Local CLI, diagnostic and recipe contract. |
| `cargo test --locked -p host-ticket-agent --lib executors::workload`; `cargo test --locked -p swarmui --test workbench` | PASS, 4 + 9 tests | Host dispatch and workbench contract, not a packaged desktop run. |
| `cargo test --locked -p coh-rtc recipe::`; `.venv/bin/python -m pytest -q tools/cohesix-py/tests/test_workload.py tests/test_provider_m28a_live.py tests/test_provider_matrix.py` | PASS, 1 + 10 tests | Generated recipe grammar, Python verifier and live-runner refusal logic. |
| `scripts/check-generated.sh`; `git diff --check` | PASS, repeated in a clean publication worktree after private KVM derivation | Generated source and Test Plan catalog consistency; no unrelated Rust suite. |
| Direct Merlin2 CUDA 13.2 batch-edge adaptation, 16×16 frame, 2 iterations | PASS: all 256 output bytes matched the independent CPU verifier, tolerance zero | Direct native smoke only; no root admission, signed terminal or native owner conformance. |
| `m28a-workloads-live`, reference and adaptation on the systemd owner | PASS: two original selected admissions, target terminals, native objects and zero-error output verification under a no-swap service | Exact-source KVM Queen and Merlin2 GPU, systemd lane only. |
| `m28a-recovery-live`, separate target cancellation | PASS: lost response reconciled without resubmit, native child reaped, unrelated sentinel alive and reservation settled under that service | Exact-source KVM Queen and Merlin2 GPU, systemd lane only. |
| Selected bridge restart and 2,000 ms native deadline | PASS: original admissions confirmed, interrupted or timed-out native children reaped, no effect replay, unrelated sentinels alive and reservations settled | Separate R4 scope on the same exact source and image. |
| `m28a-workloads-live`, reference and adaptation on the Docker owner | PASS: two original selected admissions, target terminals, native Docker cgroup identity and zero-error output verification | Exact-source KVM Queen and Merlin2 GPU, Docker lane only. |

After the final recovery-runner correction, `.venv/bin/python -m pytest -q
tests/test_provider_m28a_live.py` passed five focused cases. The bounded OOM
mapping test is included in the 29 `gpu-bridge-host` library tests; it injects
an isolated native failure and does not exhaust the shared GPU host.

The direct adaptation input SHA-256 was
`2a1c5fbee26c37c84c76b4f56a9860047a7fdffcd510dfa8d9f510594ff891ee`;
expected and observed output SHA-256 were both
`03662cb12492a2f3bb2df3e6d870db7ce120922c2d4e42cee9772fb082ea45ae`.
The selected Orin UUID was `8cc9c4072cc0587a8d93b7e2754559cb` and the
native runtime/driver API version was 13020. Scratch bytes remain under
ignored `out/m28a-direct-smoke/` locally and a separate private Merlin2
build directory; neither is an immutable acceptance artifact.

## Exact-source selected owner evidence

Source `4ea30b3bf0af601fa65c0f244f9f3bccf4fd5ef2` produced the selected
`qemu_smp_kvm_production` rootserver SHA-256
`5c9dba70ce22fb5a09f067cf5d52e0244956b888538e3b0302a13627528a3073`
and generated manifest SHA-256
`0ae5af3ea9640fde312dcf8945a5946e40adde1f14a49d5305206b5065a3cc3a`.
The live runner checked the loaded KVM image, authenticated `/proc/boot`,
gateway/agent/bridge binary digests and selected provider graph. Merlin2 ran
Linux AArch64, NVIDIA driver 595.78 and CUDA runtime/driver API 13020 on device
UUID `8cc9c4072cc0587a8d93b7e2754559cb`. The helper and enrolled package
SHA-256 values were
`931cea560a81a0a6eac86e7e1624d66d4b2f4e170793b2c6bdb4f1bb0cefbe96`
and `eca3b49092740fd959718fe7cf4a8bd9a78ddce870774d5c3450a12e910d6b15`.
The selected KVM derivation uses private manifest credentials; generated
derivatives from that selection are not source changes.

The first R3 systemd run passed all three Test Plan cases but its transient
unit inherited the host's active swap allowance. Those summary hashes were
`a72ba5035e4798220f9807d6bc80c6fb6412181fb4898b9fff50c947ca35e6f5`,
`8bd5af9573df1cca898a3d8ee538ddc12133621a234dfa7fc4a3918e7cd35e64`
and `7e39e43032af4638db8f204ef89678522a913c6be4e2c66184a1d0b5bf3d7951`.
They remain diagnostic. R5 booted the same exact image and repeated all three
cases after starting a fresh systemd unit with `MemoryMax=1G`,
`MemorySwapMax=0`, `CPUQuota=200%`, `TasksMax=64` and `NoNewPrivileges=yes`.
The inspected unit properties SHA-256 was
`d9fa547be87ad8ed5c3262948d93192749e94d8ee2db3c956284f754924fa65b`.
The built macOS and Linux AArch64 `coh workload --help` surfaces matched the
documented inspect, register and diagnose commands. The Linux `coh workload
diagnose --executor-config out/private/m28a-r5/gpu-executor-systemd.json`
reported the exact device UUID, measured free memory, a separate 2 GiB
headroom estimate and a 64 MiB enforced request cap; its output SHA-256 was
`9a514d236deb1e8ff2892a580609997542ef7dee7a7e74fe45c89a91b99edbbb`.
The exact Test Plan runner commands were invoked on Merlin2 for each R5 case,
with its own reference and state directory:

```bash
scripts/ci/provider_conformance_run.sh --matrix configs/provider_conformance.toml \
  --case m28a-workloads-live --reference-config out/private/m28a-r5/reference-systemd-01/reference.toml \
  --host-profile jetson-orin-nano-jp7 --state-dir out/private/m28a-r5/reference-systemd-01/evidence
scripts/ci/provider_conformance_run.sh --matrix configs/provider_conformance.toml \
  --case m28a-workloads-live --reference-config out/private/m28a-r5/adaptation-systemd-01/reference.toml \
  --host-profile jetson-orin-nano-jp7 --state-dir out/private/m28a-r5/adaptation-systemd-01/evidence
scripts/ci/provider_conformance_run.sh --matrix configs/provider_conformance.toml \
  --case m28a-recovery-live --reference-config out/private/m28a-r5/adaptation-systemd-02/reference.toml \
  --host-profile jetson-orin-nano-jp7 --state-dir out/private/m28a-r5/adaptation-systemd-02/evidence
```

| Selected case | Result and boundary | Summary SHA-256 |
| --- | --- | --- |
| `reference-systemd-01` | PASS; 512 verified output bytes, exact expected digest, zero differing pixels | `4c5fec7e97a8516b206dd41ab8e0056e7af25720a33c9f9fd96ab7e6822dd611` |
| `adaptation-systemd-01` | PASS; independent 768-byte input pattern, 768 verified output bytes, zero differing pixels | `a11e1a2163764a5ea2713503e9c653bf088542d4869ae27f83128ea992e55780` |
| `adaptation-systemd-02` | PASS; lost controller response, one separate target-admitted cancel, original failed terminal and native `cancelled`, unrelated sentinel alive, standing reservation zero | `cff306cc3e654e5908310724933e0ad203ed9ad948c2ff95e1d0ccd233d2aa0f` |

The R3, R5 and R6 summaries, their native evidence objects and the R4 reports
were copied unchanged to `out/audit/m28a-source-4ea30b3bf/` in the isolated
checkout. A bounded projection of the Docker Engine inspection is retained
beside them. The original signed target streams, standing ledgers, agent journals,
GPU bridge WAL and serial transcripts remain under
`/home/wizard/cohesix-m28a-e9692c36d/out/private/m28a-r3/`, `m28a-r4/`,
`m28a-r5/` and `m28a-r6/` on Merlin2. These are component observations, not
Pi 4, pressure or overall release evidence.

The separate R4 scope booted the same exact image. The bridge restart probe
observed an active native CUDA child and one reserved unit, changed the actual
systemd invocation, then reconciled the original admission to `confirmed`
execution and an `interrupted` native state. No effect replay was allowed;
the start marker was unchanged, an unrelated sentinel remained alive and the
reservation returned to zero. Its report SHA-256 is
`527c8b9c0908dc4bbf5dcdfda6ced84e1191f64ed255a6a079ce52c8d5a8d601`.
A 100,000-iteration job finished within its initial 30-second deadline, so it
is retained only as a successful diagnostic attempt. A separately admitted
2,000 ms deadline case then reached the CUDA start marker, failed with native
detail `timeout`, retained the exact failed target terminal, reaped its child,
kept the unrelated sentinel alive and settled its reservation to zero. Its
report SHA-256 is
`e795c4128b40b179c47299c17337e0ca228c16590e3d04bdc61b963a48a7cc4b`.
These probes directly exercise timeout and restart; the bounded OOM mapping
remains source-level fault-injection evidence.

R6 then booted the same exact KVM image with a separately provisioned scope.
The bridge ran as unprivileged UID 1000 inside Docker container
`6f163114336151728b20af49cebf2ae37f0f71317a865b9a6b4be03bd9139d14`
from pinned local image
`cohesix/jetson-ai@sha256:f98584bf0500d678d58ee89a693fee30c8f7305ea1bdd9177332183be90ffee1`.
The Engine reported that exact image ID, NVIDIA runtime, no network, a
read-only root filesystem, all capabilities dropped, `no-new-privileges`, host
cgroup namespace, 1 GiB memory with no swap addition, two CPUs and 64 tasks.
The bridge itself measured the matching `docker-<id>.scope` cgroup with
`memory.max=1073741824`, `cpu.max=200000 100000`, `pids.max=64` and
`NoNewPrivs=1` before and after native execution. The selected Engine owner
inspection SHA-256 was
`f5e1cb4dbdd0e22498a80ef051d0f5c14072b4f42ce0d0dc72be2a522218aaa5`.

```bash
scripts/ci/provider_conformance_run.sh --matrix configs/provider_conformance.toml \
  --case m28a-workloads-live --reference-config out/private/m28a-r6/reference-docker-01/reference.toml \
  --host-profile jetson-orin-nano-jp7 --state-dir out/private/m28a-r6/reference-docker-01/evidence
scripts/ci/provider_conformance_run.sh --matrix configs/provider_conformance.toml \
  --case m28a-workloads-live --reference-config out/private/m28a-r6/adaptation-docker-01/reference.toml \
  --host-profile jetson-orin-nano-jp7 --state-dir out/private/m28a-r6/adaptation-docker-01/evidence
```

| Selected Docker case | Result and original admission | Summary SHA-256 |
| --- | --- | --- |
| `reference-docker-01` | PASS; `m28a-reference-docker-01-admit` confirmed and acknowledged, 512 pixels verified at zero error, output SHA-256 `e4cd347ccc7288ca0670b7356d2e2cabbcd7dc280e4909d60062773c2e0938ea` | `05cb8e61f4f0f5c5ef0b47dcc8496b7bb93b6c4b1fe448c70379e87a8a7418f3` |
| `adaptation-docker-01` | PASS; `m28a-adaptation-docker-01-admit` confirmed and acknowledged, 768 pixels verified at zero error, output SHA-256 `6079e6984651bfd38998cfc799166819290e6f67ddc44f3d0dcfe906c4106aa3` | `003fc480871488b92b7b3b548a70297019b129157e4648126bd61bb5fded9065` |

Both target terminals identify the same admitted Worker and selected Orin UUID.
The separate Docker standing ledger had two attempts, two settled units, zero
active jobs and zero reserved units after the runs. Registration and dataset
bytes were distinct from Cohesix source; the adaptation changed the input
pattern and dimensions without a source edit. The registered batch recipe has
no checkpoint format: extra checkpoint fields are refused by its strict
request grammar, and operator guidance distinguishes original-outcome
reconciliation from a newly authorised restart.

## Acceptance boundary

The old M28 KVM process and its accepted results remain historical. M28a
completion covers only the selected Linux AArch64 Orin GPU, exact KVM Queen,
registered batch-edge recipe, and systemd and Docker owners above. It does
not qualify Pi 4 hardware, discrete GPUs, sustained pressure, Milestone 28b
or Release B. The full test suite was not run; the focused checks and selected
live cases above are the milestone's applicable completion evidence. The
original local checkout's concurrent M28g changes were left outside this
isolated publication.
