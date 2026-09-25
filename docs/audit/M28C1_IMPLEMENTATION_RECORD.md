<!-- Author: Lukas Bower -->
<!-- Purpose: Retain exact-source selected Mac MLX release and governed vMLX component evidence with its proof limits. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# M28c1 implementation record — governed Mac MLX release and vMLX

```text
Title/ID: m28c1-admitted-mlx-release
Milestone: 28c1 / m28c1-admitted-mlx-release
Goal: Admit local Mac MLX training and release through the durable PEFT ticket and shared signed verifier.
Inputs: M28c local Metal selection; M28b release controller, ticket, WorkerLora and evidence contracts; Apple M4/macOS 27; qemu_smp_production seL4 profile.
Changes:
  - apps/host-ticket-agent/src/executors/peft_release.rs — launchd phase custody, original-ticket recovery and bounded quiescence for the selected Mac native helper.
  - tools/cohesix-py/cohesix/{mlx_release,mlx_service}.py — pinned Metal phases, held-out comparison, digest-checked staging, direct serving, canary, accepted generation and rollback.
  - scripts/ci/provider_m28c1_live.py and selected matrix/action entries — exact-source Mac HVF image and signed train/rollback acceptance.
  - docs/HOST_TOOLS.md, docs/PRIVATE_LORA_RELEASE.md and tools/cohesix-py/README.md — as-built operator and Python contracts.
Commands: Pinned profile validate/build and Queen launch; focused owner tests; m28c1-mlx-live train and rollback cases described below.
Checks: Signed successful train and recovered-failure rollback reconcile their original operations; held-out quality, canary, accepted generation and native phase observations agree with the shared verifier.
Deliverables: Selected admitted generation 1, retained private signed/native reports, focused tests, this record and Complete status.

Title/ID: m28c1-vmlx-governed-serving
Milestone: 28c1 / m28c1-vmlx-governed-serving
Goal: Serve only the accepted generation through pinned vMLX and observe the verified incumbent after rollback.
Inputs: Signed train and rollback graphs above; installed signed vMLX 1.6.65 engine; content-bound fused model copy and four frozen prompts.
Changes:
  - tools/cohesix-py/cohesix/vmlx_governed.py — accepted-state fence, engine/process custody, response and resource checks, and orphan recovery.
  - apps/swarmui/frontend/workbench/mlx.js and index.html — signed release/rollback graph and generation projection.
  - scripts/ci/provider_m28c1_live.py — separate signed-graph, fused-source and real-engine selected-host case.
  - docs/SWARMUI.md and docs/PRIVATE_LORA_RELEASE.md — governed serving and proof-class guidance.
Commands: Focused vMLX and frontend tests; m28c1-vmlx-live selected-host case described below.
Checks: Four pinned engine responses pass frozen hashes and bounds; changed generation refuses; the restored accepted incumbent matches both signed graphs.
Deliverables: Governed optional vMLX serving evidence and Complete status for the selected Mac component.
```

## Exact selected source and target

The accepted runtime source is commit `560a54cb3a100b99197711c19e690d5c0e9cdf6f`.
The selected private manifest resolved to SHA-256
`d2439a7c02c580c4c482b9e472da7ca952297b66cb939bbc27f99d646a81331f`.
The local `qemu_smp_production` seL4 source/build validation passed with
`--require-source --require-artifacts --for-runtime`. The pinned
`~/cohesix/qemu/qemu-10.1.0-hvf-gic-sync/install/bin/qemu-system-aarch64`
has SHA-256 `a0471828f464116c51c1d29ebae12a2a0fc713b4edec5c52e81bd5388040135a`.
The live HVF PID was `10550`; its rootserver, elfloader and CPIO SHA-256 values
were respectively `f3994e2c9f91e215f8d47b80319681e532fae89e830dd64098f4848498b236bb`,
`192f48f588fb44ac40fcf1f1c88c59995f8cf3adaffee48ec617363a70150ec5`
and `3dce9c392c006ac6af66b22eca4ec603add8b10bb99e09eb996189be55103a10`.
The rootserver contains one `[BUILD] 560a54cb3a10-dirty` marker; `dirty`
denotes selected private compiler-generated derivatives. The live runner
allowed only those derivatives and refused other source changes. The pinned
QEMU boot reached `root-console.start.ok`, and the authenticated gateway
connected to its TCP console before admission.

The private Mac selection used the pinned model tree SHA-256
`8b40b6d325ea432fda5d9d9612813d203d44bde7c232b465b0af32454b1d4791`
and held-out data tree SHA-256
`6b2ef5031d278330c0e4db1a40f5de3ddcc2647df74e2fa7a1c2d0f3229670b7`.
The frozen 48-step rank-8 seed-42 native Metal train produced adapter
`a9b837751e207b3c2c52d166f4fb341921cd8d5082197a82915d88f1ea498850`.
The selected vMLX fused source SHA-256
`b75efc50e56aec7c671357ee6eec7d4df55b8c82896a4308d941e0b86e629777`
was bound through the retained fusion provenance to that base and adapter.

## Signed train, interruption and governed serving

The fresh `m28c1-mlx-train-live-final` operation completed validate, train,
evaluate, scan, stage, load, canary and promote. All eight native phase
observations succeeded. The 16-row held-out loss moved from
`5.796783924102783` to `0.8091069459915161`; the predeclared canary and
resource bounds passed. The shared verifier reported `succeeded` under graph
`3d1040cf44507d71fa44b194d595a1c91b543adfe786f2e424cbfddf29fc2ad0`.
Accepted generation 1 served the scanned adapter above. The case summary
SHA-256 is `2abfdb4e1deb250ae88ae94e0b963b74382f3a4112d23dea3cd361f9711b56af`.

The separate `m28c1-mlx-rollback-live-final` operation imported that exact
staged adapter, passed validate/evaluate/scan/stage/load and then lost its
owned serving process after the successful load observation. Its canary
refused `serving_process_missing`; native rollback restored the verified
incumbent and a healthy canary. The shared verifier reported
`recovered_failure` under the original operation and graph
`1fdb4cc1ba17cbd93da70786c6ba337f044d7eb3db02ecab5f0c9ba3a4dbbe79`.
Accepted generation 1 and the adapter digest stayed unchanged. The case
summary SHA-256 is `17af28148658896f88afca2c97392878bf7e5494e8c10d477a1c3e7ca3fdfc47`.
The successful train verified through `coh peft release verify`; the
recovered-failure request did not return a false successful verification.

The separate `m28c1-vmlx-live` case re-read both retained graphs through the
shared verifier. The installed signed vMLX 1.6.65 engine at commit
`22f9c77711fb580df32f4d40bbaea989c2d5421b` and SHA-256
`6ae7d9f0b5db2035b623fc0cacecc3f572bc46db18b69cc8fd611f71b0c2ac7d`
served four frozen response hashes:

```text
f11f82570a79fe6d3beaef5fe7ee651626cc8b68a19e3a4fd44bc470b264f017
d54caf7c83dd0f7f18a72bab8339bf62449e77e67ba63665021ca7d1b1da8f3b
d0bfb8e7a15424d7ec495a86cf1ce033d393e75b2544b37e184b62cbf9d39a16
5c2fd5cebdd2d9617082ec5ed0c14ec9315e6b16d618e7dd2ebb5f34dea42a97
```

Each result stayed within the selected 10-second and 4-GiB bounds; observed
latencies were 2,619–2,846 ms and maximum observed RSS was 430,768,128 bytes.
The runner refused a changed accepted generation and confirmed the rollback
incumbent canary. Its summary SHA-256 is
`58a9b716f3a5cdb6a990576108cec2e96d48384e70dfad643387b51df88bd276`.

The ignored private `out/private/m28c1/session/cases/` and
`out/private/m28c1/session/vmlx-live/` retain selected references, native
phase observations, signed graphs and bounded summaries. Earlier case
directories remain diagnostic failures; no failed or model-reported outcome
was promoted into the acceptance above. Source/profile/QEMU hashes appear in
each selected summary without retaining private credentials in this record.

## Focused validation and limits

The selected commands were the profile `configure`, `build` and `validate`
operations in `scripts/sel4_profile.py`, the pinned QEMU `--no-run` and
`--launch-existing --raw-qemu` paths in `scripts/cohesix-build-run.sh`, and
the `m28c1-mlx-live` and `m28c1-vmlx-live` cases in
`scripts/ci/provider_conformance_run.sh`. Private selections were assembled
under ignored `out/m28c1/` and `out/private/m28c1/`; the tracked conformance
runner checked the exact source, target image, manifest, binaries, native
configuration, signed graphs and bounded outputs before reporting success.

The focused MLX release/service, vMLX custody and live-reference/HVF
identity Python checks passed (`13 passed`); the SwarmUI frontend projection
checks passed (`4 passed`). The earlier focused host-ticket-agent tests for
the changed native executor passed. `scripts/ci/check_test_plan.sh`,
`scripts/check-generated.sh`, `cargo fmt --all -- --check` and
`git diff --check` passed after regenerating the canonical implementation
inventory and its dependent graph projections. The provider matrix selected
the three live cases above; no full suite was run.
The changed host-tool, Python and benchmark surfaces were reviewed: the
existing `coh` release controller and Linux HF executor remain on their
earlier contracts; Mac launchd and Python native behavior and SwarmUI signed
projection were updated together. No target hot-path or benchmark contract
changed. AI assistance contributed implementation, tests and this audit;
signed/native observations, not model text, establish the outcomes.

This is selected Mac component qualification on QEMU with a real local Metal
provider and installed vMLX engine. It does not claim fresh physical Pi 4
evidence, another Mac/MLX stack, an NVIDIA mixed workflow, an MCP client,
an assembled Release B bundle or the full test suite. Those claims retain
their own milestone gates.
