<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record M28a registered CUDA implementation, focused checks and open exact-source evidence obligations. -->
<!-- Author: Lukas Bower -->

# Milestone 28a implementation record

Milestone 28a is **in progress** under `m28a-approved-user-workloads` and
`m28a-cuda-operations-and-recovery`. This record does not promote a direct
native smoke or an inherited M28 result into an admitted M28a target result.

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
Commands: focused Rust and Python commands in the evidence table; selected live commands remain open.
Checks: reject changed registration/package/input, unsafe paths, stale inventory and typed-bound violations before native dispatch; verify exact GPU/output and settled original outcome in each advertised lane.
Deliverables: implementation and operator guidance now; exact-source live case reports and milestone completion remain open.
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
| `scripts/check-generated.sh`; `git diff --check` | PASS | Generated source and Test Plan catalog consistency. |
| Direct Merlin2 CUDA 13.2 batch-edge adaptation, 16×16 frame, 2 iterations | PASS: all 256 output bytes matched the independent CPU verifier, tolerance zero | Direct native smoke only; no root admission, signed terminal or native owner conformance. |

The direct adaptation input SHA-256 was
`2a1c5fbee26c37c84c76b4f56a9860047a7fdffcd510dfa8d9f510594ff891ee`;
expected and observed output SHA-256 were both
`03662cb12492a2f3bb2df3e6d870db7ce120922c2d4e42cee9772fb082ea45ae`.
The selected Orin UUID was `8cc9c4072cc0587a8d93b7e2754559cb` and the
native runtime/driver API version was 13020. Scratch bytes remain under
ignored `out/m28a-direct-smoke/` locally and a separate private Merlin2
build directory; neither is an immutable acceptance artifact.

## Completion gates still open

- Build a fresh KVM image and host tools from one committed M28a source, with
  selected manifest, loaded rootserver, authenticated Queen and GPU identities
  bound as the live runner requires.
- Run `m28a-workloads-live` for the reference and independently configured
  adaptation under both advertised `systemd` and Docker native owners. Retain
  each original selected job, signed target terminal, native object and
  task-specific output verifier.
- Run `m28a-recovery-live` with a separate target-admitted cancellation, controller
  response loss, reaped native child, unrelated sentinel survival and standing
  reservation settlement. Retain timeout/revoke, runner restart and capped OOM
  evidence at their actual proof classes.
- Review exact installed Mac/Linux client behavior and source compatibility;
  update BUILD_PLAN and STATUS only after the applicable live evidence passes.

The old M28 KVM process and its accepted native results remain historical.
The current Merlin2 `wizard` session has no access to the host Docker socket;
that owner lane has not been run. The local checkout also contains unrelated
concurrent M28g planning edits, which are outside this task's commit.
