<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record M28b private PEFT lifecycle implementation, focused checks and exact-source component evidence. -->
<!-- Author: Lukas Bower -->

# Milestone 28b implementation record

Milestone 28b is **complete** under `m28b-configurable-peft-training-and-import`
and `m28b-evaluate-canary-promote-rollback`. The selected reference is a private
SmolLM2-135M LoRA workflow on Linux AArch64 NVIDIA with a KVM Queen. Completion
here is a component claim. It does not extend to a Pi 4 image, integrated Release
B, general model formats, or external inference routing.

## Task record

```text
Title/ID: m28b-configurable-peft-training-and-import; m28b-evaluate-canary-promote-rollback
Milestone: 28b / Deeper PEFT Lifecycle and Verified Serving
Goal: Complete pinned training and independent import, then measure and reversibly serve a real adapter under the original ticket and signed outcome.
Inputs: accepted M27d phase journal and verifier; M28/M28a authority; configs/cuda_recipe.toml; selected private KVM manifest; local base and disjoint data on Merlin2.
Changes:
  - tools/cohesix-py/cohesix/hf_native.py and examples/private_lora_{release,import,client}.py — v2 pinned selection, safe independent import, complete Trainer-state checkpoints, native evaluation and application-facing serving.
  - apps/coh/src/peft and apps/host-ticket-agent/src/executors — preserve the existing release transaction, enforce current generation and bind native phases to the admitted operation.
  - tools/coh-rtc/src/recipe.rs, configs/cuda_recipe.toml and generated projections — declare the qualified LoRA, safe format, checkpoint and serving capabilities; QLoRA remains disabled.
  - tools/cohesix-py/cohesix/model_release.py, SwarmUI projection, provider matrix and operator docs — expose bounded Python plan/submit/inspect/recover, signed verification, comparison and recovery state across the selected surfaces.
Commands: Focused commands and exact live case invocations below.
Checks: Separate genuine train, independent import and full-state resume; fixed held-out refusal; signed canary/promotion and observed application generation; interrupted transition with incumbent restoration.
Deliverables: Versioned reference and selection examples, supported native endpoint, focused tests, m28b-peft-live and m28b-serving-live reports, this record and Build Plan/Status completion.
```

AI assistance contributed source, focused tests, live probes and this record.
The v2 capability is limited to the selected local base, tokenizer, data,
Transformers/PEFT runtime, LoRA rank and safe adapter format. A full resume
retains adapter, optimizer, scheduler, RNG and Trainer position in immutable CAS
objects; an adapter-only restart cannot satisfy its checkpoint contract. The
independent imported adapter records **unknown training provenance**. Source and
format validation do not certify its licence or safety. The controller continues
to use the accepted M27d phase journal and shared signed verifier; Python and
SwarmUI project the same operation instead of creating a second evaluator or
outcome ledger. `verified_success` becomes true only after the shared CLI
verification succeeds for the requested outcome.

The compatibility review covered `coh` and `cohsh`, Hive Gateway and
`host-ticket-agent`, generated provider/recipe inventories, SwarmUI, the Python
SDK, and `scripts/rest_perf_harness.py`. The M28b request preserves the existing
`peft.release` host ticket, WorkerLora receipt and bounded phase ABI. The
benchmark harness neither parses native adapter bytes nor supplies a serving
result; no benchmark figure is claimed. No code under `releases/` changed.

## Selected source and host boundary

Implementation source `8c0147bad1c7dc893bac84d791da0166ec3d2c48` built
the `qemu_smp_kvm_production` KVM rootserver SHA-256
`65bdf95b8fcb8def209a4da11104361f168a0b1cf04890a1a47ce3d5076bad51`.
The selected generated manifest SHA-256 was
`7e7a27ee02720e147fbf7f8386a689c1c6cb678c11377d574423a9157213bd52`;
the payload CPIO SHA-256 was
`ffb92871abebf865c75ed0121146c26d27ac2deee313dbf3fd63273cb674f6e0`.
The live runner checked the KVM process, embedded source marker, loaded
elfloader/rootserver/CPIO, authenticated target boot, selected generated graph,
host binary digests, original WorkerLora receipt and independently enrolled
gateway/native/Worker signatures. Merlin2 ran the selected Orin GPU UUID
`8cc9c407-2cc0-587a-8d93-b7e2754559cb`, driver `595.78`, and pinned
qualified Python/HF packages.

The subsequent closure commit adds this documentation path to the compiler's
source-surface inventory and regenerates its graph hash. It does not modify the
selected M28b product logic or the `root_task_resolved.json` runtime manifest.
The live claims in this record remain bound to implementation source `8c0147b`
and its stated image/graph, rather than being relabelled as a new-image run.

The original native profile SHA-256 was
`adbec83c81c5285e765be2d93581c61a3b262b7a64bbff0c2d46de6f46cf92b7`.
A second private deployment used profile SHA-256
`3a2ff4efb37316dab76e8837cba135c349b9e7378707ef37e539d964f99955c5`
for the positive checkpoint resume. Both profiles selected base
`80521b40281d6ce74e35c9282c22539e75aa0ac8578892b2a59955ef78d55da1`,
train set `3889f6bb44e744660eab6f5885752b48cd029035adb85403224ff760807c4dc5`,
disjoint held-out set
`95db14445cf85817f929b21673fb2c7db127508e1c2fabfa4a761f71fc077182`,
seed 41 and 32 maximum steps. The second profile has a fresh local source
attestation and generation-zero base serving state; it does not rewrite the
first deployment's incumbent or its negative comparison.

Base weights, private train/evaluation rows, prompts, outputs and adapters were
staged and executed on Merlin2. The Mac transferred source and KVM artifacts,
not those native payloads. The reference declares gated downloads and uploads
disabled; native base loading uses local files with remote-code loading disabled.
The selected processes were sampled for sockets every 0.5 seconds during each
case: 954 samples and 2,964 socket rows contained zero observed non-loopback
connections. This observation and declared local paths are narrower than a
network egress guarantee; no absolute no-transfer claim is made. Private input,
CAS and full native logs remain on Merlin2 under
`/mnt/nvme/cohesix-dev/m28b-live-20260924/` and
`/mnt/nvme/cohesix-dev/m28b-resume-20260924/`.

## Admitted lifecycle and requested outcomes

Each command below used a fresh operation and private reference. The retained
`out/scripts/m28b-admission.py` on Merlin2 prepared the immutable request,
enrolled separate producer keys, ran the canonical
`scripts/ci/provider_conformance_run.sh` matrix command with the case's
`reference.toml` and `live/` state directory, and retained both the agent and
runner results under `out/private/m28b/session/cases/<operation>/`. The selected
host profile was `jetson-orin-nano-jp7` throughout.

```bash
python out/scripts/m28b-admission.py train m28b-train-final
python out/scripts/m28b-admission.py import m28b-import-final
python out/scripts/m28b-admission.py promote m28b-promote-final
python out/scripts/m28b-admission.py reject m28b-reject-final
python out/scripts/m28b-admission.py rollback m28b-rollback-final
M28B_NATIVE_ROOT=/mnt/nvme/cohesix-dev/m28b-resume-20260924 \
  python out/scripts/m28b-admission.py resume m28b-resume-final-02
```

| Original operation | Signed native state and observation | Accepted generation | Signed graph SHA-256 | Live summary SHA-256 |
| --- | --- | ---: | --- | --- |
| `m28b-train-final` | `succeeded`; genuine 32-step training, held-out loss 4.92838860 | 4 | `39c6504199614c413803aba1f7fe5494d4e4a5b9d4f8271623205b1d4dddf737` | `2fcb55700e76fb95b9f7972def220d2f2e2fae268ccdf3bb76ad7db0c911ad99` |
| `m28b-import-final` | `succeeded`; compatible independent adapter, training provenance unknown | 5 | `94b77cc11b4f4e65c94ba157867028170f099e359d26029ea88956a994724185` | `db0feba2a6aa099bffff61dddebbfcfea169c0a868450c58f1bdb66b0ed395a7` |
| `m28b-promote-final` | `succeeded`; canary and separate application request saw adapter `6e32c5a1…` | 6 | `d59af576722be90605ecb612afd7bec1804f9ee75519136a147d034bd23752d3` | `ce452b0ac30cc939a418a76578c6bf42628820c3123588617c56874fb736e1d4` |
| `m28b-reject-final` | `failed` as specified; loss 4.93975401 exceeded incumbent 4.92838860; no load or promote | 6 | `5f08b40167e3bf9f2cc20bc777058ae1a12f8413ed188f0fa10bd62a13f4d029` | `28649181598f43d3a7fe14e98e1e25b84f5950275772464a448d54bd32eb99a8` |
| `m28b-rollback-final` | `recovered_failure`; interruption after load, rollback and another incumbent application request | 6 | `c843405b06d8f7ae645a31dff5d725d7a24d05e578a0b5b5737ed2ecc154beb1` | `e181678ca50b33f2e753f38bb794fb71b4dd76b68cc5a87f12c5f64df333a098` |
| `m28b-resume-final-02` | `succeeded`; restored source step 8, trained to step 32, loss 4.93017483 against fresh base 4.94147396 | 1 | `804bf56584895f60878b2db8d8aef7b1c587f07cea69996989deaf52b44c4154` | `ca1d6aafe37b9d0f112ed975ea6973cff25ae1b01e533753a3bcc5e59e38197e` |

The resume source was a genuinely interrupted Trainer run
`m28b-checkpoint-seed-01`: it retained full-state checkpoint SHA-256
`285d9571849a23472d1f46eafea515746e5a146d2a9b58c3a80e0cf75b1caf74`
at step 8 without a source `train.json`. The admitted resume retained a new
step-32 checkpoint SHA-256
`8f8e7a718b844a473eec1e8d60f56ebf78c87254161eccc69a6b01f5275c50e3`.
Its output weights differ from the fresh training adapter, as the documented
stochastic setting permits. The original profile also resumed step 8 directly
to evaluation, but its 4.93017483 loss was worse than the existing incumbent's
4.92838860; the predeclared zero-regression policy barred promotion. That
direct preflight is diagnostic, not an admitted target result.

The first fresh-profile admitted resume, `m28b-resume-final`, failed validation
because the scratch checkpoint seeding step had omitted its byte-exact source
request from private CAS. The checkpoint manifest already bound that request
digest. The same retained request bytes were added under that digest, the
checkpoint contract was rechecked, and a **new** target operation
`m28b-resume-final-02` completed. The failed identity was not replayed or
relabelled. The private scratch seeding script was corrected; tracked product
logic was unchanged by that staging repair.

The fixed evaluation used 16 held-out rows and the same tokenizer, base,
preprocessing, evaluator and zero-regression policy for each candidate and its
incumbent. The rejected adapter never reached load. The accepted serving
adapter at generation 6 was
`6e32c5a1ddfb0fd0ca5a740a9027382b4583332c91344bead8a4653f4b78f137`.
The promote application record binds operation, generation, adapter, native
invocation, latency, prompt digest and output digest. The rollback application
record independently observed the same accepted generation and adapter after
the interrupted forward transition. The original serving unit was restored
and observed healthy at generation 6 after the separate resume deployment was
stopped. The private KVM and gateway processes were then stopped; their exact
image/session logs and signed case graphs remain under the private evidence
root.

The 14 bounded session and case reports were copied without model, dataset,
prompt or output bytes into ignored `out/audit/m28b-source-8c0/`. The verified
bundle SHA-256 is
`b4d0d41caec6d1e7470e76833ca47bf1738b0c45dcb3e937d304f51180a60679`;
the original signed graphs, native journals and serial transcript remain on
Merlin2 under `out/private/m28b/session/`.

## Focused verification and limits

| Command or check | Result | Proof limit |
| --- | --- | --- |
| `cargo test --locked -p coh --test peft_import --test peft_release` | PASS | Pure CLI release and import contracts, not target execution. |
| `cargo test --locked -p host-ticket-agent --lib executors::peft` | PASS, five tests | Host PEFT executor bounds and file handling; the admitted cases cover live release phases. |
| `cargo test --locked -p host-ticket-agent --lib executors::peft_release` | PASS, zero tests selected by this filter | Compile check only; it does not add a test claim. |
| Qualified native `PYTHONPATH=tools/cohesix-py python -m unittest discover -s tools/cohesix-py/tests -p test_hf_native.py -v` | PASS, six tests with cryptography | Artifact confinement and source-attestation inputs; no CUDA execution in fixtures. |
| `.venv/bin/python -m pytest -q tools/cohesix-py/tests/test_model_release.py tools/cohesix-py/tests/test_playbooks.py tests/test_provider_m28b_live.py` | PASS, 35 tests | Python identity/recovery and live-runner refusal logic. |
| `cargo test --locked -p coh-rtc recipe::`; `cargo test --locked -p swarmui --test workbench` | PASS, one recipe and nine workbench tests | Generated capability and UI projection, not a packaged app. |
| `node --test tests/frontend_peft_projection.mjs`; `scripts/check-generated.sh`; `scripts/ci/check_test_plan.sh`; `git diff --check` | PASS | Frontend projection and canonical source/catalog consistency. |
| Selected `m28b-peft-live` and `m28b-serving-live` operations above | PASS at their specified positive or negative requested outcomes | Exact-source KVM/Orin component evidence only. |

The qualified native virtual environment has no `pytest` module, so its six
`unittest` cases were run directly in that environment. The local Python
environment skipped one cryptography-dependent input test; the qualified run
executed it. Full release, Pi hardware, pressure, repeatability and packaged
desktop gates were not run for this component closure. A workspace-wide
`cargo clippy --all-targets -- -D warnings` attempt exposed unrelated existing
lint failures in the standing ledger, GPU bridge and compiler; it supplied no
M28b acceptance and was not used to weaken a check.
