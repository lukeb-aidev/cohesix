<!-- Author: Lukas Bower -->
<!-- Purpose: Operate the confined native HF adapter release recipe with exact provenance, evaluation, serving and recovery authority. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Private LoRA release

`coh peft release` composes an admitted `peft.release` ticket with the existing
host phase journal and signed verifier. Native import and native training
converge on validation, evaluation, scan, stage, load, canary and promotion.
Training inserts one native HF Trainer operation after validation. The Linux
CUDA host owns every weight, dataset, checkpoint, evaluator and inference process.
The selected QEMU profile declares the WorkerLora receipt action. Other profiles
must explicitly select it before use; a Python projection never enables it.

The older `coh peft export/import/activate/rollback` commands keep their existing
file-registry and GPU snapshot contracts. Their pointer commits do not establish
native inference. Use the release recipe when native serving verification is
required. This reference adds no root console command or SwarmUI console verb.

## Qualified reference configuration

The reference uses Linux AArch64, an NVIDIA CUDA-capable host, Python 3.10,
PyTorch **2.9.1+cu126**, CUDA **12.6**, Transformers **4.56.2**, PEFT **0.17.1**,
Accelerate **1.10.1**, safetensors **0.6.2**, huggingface-hub **0.34.4** and
tokenizers **0.22.0**. The serving process is the upstream **Transformers serve**
command with its OpenAI-compatible streaming API. Its pinned dependencies are
openai **1.106.1**, FastAPI **0.116.1**, Uvicorn **0.35.0**, Pydantic **2.11.7**;
cryptography **45.0.7** verifies Ed25519 source attestations. No OpenAI service or
API key is used. Keep this stack in a dedicated environment.

The licensable reference base is `HuggingFaceTB/SmolLM2-135M`, revision
`93efa2f097d58c2a74874c7e644dbc9b0cee75a2`, under Apache-2.0. Its
`model.safetensors` SHA-256 is
`80521b40281d6ce74e35c9282c22539e75aa0ac8578892b2a59955ef78d55da1`.
Download the pinned model before execution and record every file digest in the
base manifest. Native execution is offline and disables remote model code.
The explicit tokenizer template concatenates message content for plain text
continuation; it is a qualified preprocessing transform, not a vendor chat template.

The reference selects 64 distinct training paragraphs and 16 separate held-out
paragraphs from licensed Cohesix documentation, seed 41, batch size 1, maximum
length 128, rank 4, 32 steps and learning rate 0.0005. HF Trainer emits held-out
`eval_loss`; lower is better. Predeclared bounds require 16 samples, loss at most
8.0, **zero permitted regression** against the accepted deployment and at most
one-hour-old evaluation evidence. Four fixed continuation canaries each generate
at most eight tokens, match the evaluator's greedy output and finish within
30 seconds. This small reference establishes its stated workload contract; it
does not qualify arbitrary models, data or production quality.

## Prepare the native host

Use [private_lora_release.py](../tools/cohesix-py/examples/private_lora_release.py)
with a fresh private absolute root, the pinned base, its manifest and licensed
documents. The base manifest has `model`, `revision`, `license_ref` and `files`
(file-name to SHA-256 map). The example writes native/agent configuration, CAS
objects, the requested base baseline, a systemd user unit and a prepared request.
It makes no admission or execution claim. Review the profile and install its
unit in the owning user's systemd configuration, then start that unit. The
native evaluator verifies the initial runtime before comparing candidates.

The example creates an explicitly local source-attestation key. This is a
reference source custodian, not a vendor or device attestation. Production source
keys belong in the independently administered profile. Keep private keys private;
never put them, raw tickets or datasets in evidence exports or commits.

Configure the existing agent with `--peft-release-config /absolute/agent.json`.
That file selects the absolute Python executable, pinned helper path and hash,
and absolute native configuration path. The native configuration selects one
private root, one profile digest, one allowlisted systemd user unit and one
loopback serving port. Ticket arguments cannot supply commands, URLs or paths.
An absent configuration returns `not_enabled`; macOS native execution returns
`not_supported`. macOS can still control the remote admitted workflow.

An import input contains the profile, source and attestation references,
license references, `checkpoint: null`, adapter bundle reference, exact base and
tokenizer digests, pinned versions, training dataset digest and resolved training
settings. A training input contains the profile/source/attestation/license
references and `checkpoint: null`. Each source attestation signs the exact
`cohesix-peft-source-attestation/v1` payload, including the adapter bundle for
import. Adapter identity hashes the canonical file-name/digest map of both native
safetensors and PEFT configuration, so changing configuration cannot reuse an
evaluated weight identity. An import requires genuine native safetensors plus PEFT configuration;
it needs no invented Cohesix training job. Missing provenance, changed hashes,
pickle files, symlinks, traversal, invalid tensors or incompatible settings fail.

[private_lora_request.py](../tools/cohesix-py/examples/private_lora_request.py)
prepares subsequent import/training requests from reviewed input and the current
accepted baseline. It retains the reference's predeclared bounds. It neither
signs provenance nor manufactures metrics. Any changed policy requires a new
reviewed request and admission before evaluation.

## Admit, execute and verify

Create a `cohesix-peft-deployment/v1` file with an absolute private `journal`,
the complete native `request`, and one existing workflow `execution` object
containing `request`, `graph`, `trust` and `cas`. The workflow request is the
root-admitted `host-ticket/v2` caller request. Its action is `peft.release`,
arguments are `{"request_sha256":"<native-request-digest>"}`, `operation_id`
matches the native request and `subject_ref` matches its model. Pin the exact
READY WorkerLora id, role, supervisor and capability generation, current writer
epoch, finite expiry and original ticket/idempotency identity.

Approval grants exactly this release and its frozen rollback target. For first
deployment, human approval explicitly covers the qualified base baseline with
`adapter_sha256: null` and generation zero. The base is evaluated with the same
configuration and is retained as the rollback target. Empty baselines and zero
scores cannot manufacture an improvement claim. Gateway, native and Worker
custodians require separate enrollment; follow the
[signed evidence contract](CAUSAL_EVIDENCE.md).

```bash
coh peft release plan --deployment /absolute/deployment.json
coh --ticket-ref file:/private/operator.ticket peft release apply \
  --deployment /absolute/deployment.json --rest-url "$COH_REST_URL"
coh peft release watch --deployment /absolute/deployment.json
coh peft release verify --deployment /absolute/deployment.json
```

Use `COH_REST_AUTH_TOKEN` for gateway authentication. Direct TCP uses `--host`,
`--port` and the configured console authentication. Python's
`cohesix.playbooks.run_peft_release` calls these same commands and verifier;
credential arguments are references. `verify` succeeds only for a signed,
successfully promoted candidate. `watch`, `explain` and `recover` reconstruct
the same retained evidence. They do not resubmit an uncertain ticket or execute
compensation by themselves.

The host compares exact candidate/baseline artifacts, data/split, preprocessing,
base/tokenizer, evaluator/version, parameters/seed, runtime/resources and current
generation. Immediately before promotion it rechecks authority, evidence age
and generation. Registry lock ownership spans the transaction, and durable registry ownership
keeps unresolved operations blocking across an agent crash. A changed baseline
requires renewed evaluation and approval. The upstream service reloads the exact
candidate; its invocation identity and actual outputs establish serving behavior.
Only then can generation compare-and-swap commit the accepted deployment.

## Interruption, rollback and cleanup

Workflow position, native checkpoint and deployable adapter are separate records.
The selected reference saves deployable adapters and read-only training/end
observations; native optimizer/scheduler/RNG resume is **unqualified**. A supplied
checkpoint is refused with `native_resume_unqualified_use_new_authorized_attempt`.
Authorize a fresh operation if training must restart, preserving earlier evidence.
Verified immutable outputs within the original transaction are reused; native
phases with durable dispatch intent and unknown outcome are never issued twice.

Failed load/canary triggers compensation only under current authority. It restores
the previous runtime/artifact/generation, restarts the service and verifies health
and baseline behavior. The outcome remains `recovered_failure`; it never becomes
a successful candidate. Failed or unknown rollback blocks all new promotion in
that registry. Resolve current authority before proceeding.

When the original ticket expired or was revoked, obtain a **fresh approved ticket**
with the original request digest and `"recovery_only": true`. It has a new ticket
and idempotency identity, the same operation/model and current Worker binding.
This scope permits only fencing and quiescing original native invocations and
verified restoration of the frozen baseline. Delayed forward helpers are refused
after compensation. Cancellation before evaluation obtains baseline continuations
from the same native generation API and emits no candidate score. Separate recovery journals preserve the
original failed record. A successful restoration clears the registry blocker;
candidate release stays failed. At most four such attempts are retained. A newer,
unrelated accepted generation cannot be overwritten by compensation.

Bounds are one registry owner, 256 retained operations, four compensation attempts
per operation, 4,096 CAS objects and 2 GiB retained CAS bytes, 32 MiB per adapter
and 1 GiB per base weight file. Each phase runs in a systemd user unit with
3 GiB memory, 128 tasks, two CPU cores, 64 MiB maximum file size and a 300-second
deadline, additionally cut short by ticket expiry or revocation. The serving
unit has 2 GiB memory, 128 tasks and two CPU cores. CUDA allocation is limited to
40% per helper process; this is not a hard GPU partition. Measure available host
headroom before starting this combined workload.

Training scratch adapter files are removed only after verified CAS retention.
Per-phase kernel CPU usage and peak host RSS are retained separately from
serving memory; cumulative CPU totals include only observed completed phases.
Native commit also checks the absolute admitted expiry, comparison age and
accepted generation after canary. Phase process exit releases transient allocations; accepted serving capacity
remains in use until its owner stops the service. Retained artifacts and evidence
are bounded and are not silently deleted. Stop the reference service, verify it
inactive and remove its private enrollment keys when ending a disposable run.
Keep failed candidate evidence and the accepted/rollback artifacts needed for
recovery. Use [Adoption](ADOPTION.md) and [CI workflows](CI_WORKFLOWS.md) for installed
CLI/Python operation. Native UI and integrated release qualification remain separate;
these commands do not claim production ticket-to-bundle binding.
