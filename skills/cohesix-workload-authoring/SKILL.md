---
name: cohesix-workload-authoring
description: Prepare and enroll a reviewed Cohesix CUDA batch workload with pinned inputs, finite bounds and an independent output verifier. Use when adapting the shipped batch example or registering a new workload on the executor; existing jobs use GPU operations.
license: Apache-2.0
---
<!-- Author: Lukas Bower -->
<!-- Purpose: Move useful user workloads from native preparation to bounded registration and independently checked outcomes. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Bring a useful batch job

Start with a finite input batch, a defined output and a check independent of
the executable being enrolled. A first image task can use the shipped Sobel
edge extraction example; a defect detector, embedding pipeline or video
service needs its own reviewed package and verifier. The example processes
raw grayscale frames; it does not implement those applications.

Read the selected revision's [registered workload contract](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/HOST_TOOLS.md#registered-cuda-workloads),
[GPU nodes](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/GPU_NODES.md#admitted-gpu-workload-transport)
and [batch preparation example](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/tools/cohesix-py/examples/cuda_batch_edges.py).
Replace `main` with the selected source commit/tag or use matching local files.
Run preparation/enrollment on the actual Linux AArch64 NVIDIA executor.
A Mac controller can later submit the governed request through its gateway.
Use `cohesix-ai-host-setup` for a missing compatible CUDA environment.

## Prepare before enrollment

Inspect the selected executor configuration, physical CUDA UUID, publication,
helper and provider graph identities, package/tool versions and private state
ownership. Keep the existing bridge's systemd or digest-pinned Docker lane;
registration does not configure a new native execution owner. The maintained
`scripts/build-gpu-batch-edges.sh` checks the selected Orin Nano CUDA profile;
other compatible NVIDIA hosts require their own build/profile qualification.

For the shipped example, use that script with a fresh output directory on its
supported host. Review the resulting package and source hash record. Run the
matching Python `cuda_batch_edges.py prepare --help`, then prepare a private
input CAS, registration and canonical v2 request using observed device/helper/
topology/graph identities and the chosen original ticket ID. Freeze the
expected output hash before submission. Raw pixels must match the declared
width, height and frame count; a PNG/JPEG file is not that input format.
The selected example supports 1–4 frames, dimensions 3–256 and at most
262144 input/output bytes; these are example bounds, not arbitrary CUDA support.

For a new package, use `cohesix-cuda-registration/v1`: exact package SHA-256,
fixed `run` entrypoint, input CAS root, selected device, typed integer/choice
parameters, secret references and finite input/output/memory/disk/deadline
bounds. Review package filesystem access as well as declared outputs; its disk
guard is not a filesystem quota for writes elsewhere. Requests select a
registered digest and typed parameters, never an executable, shell command,
arbitrary input path or secret value. The task-specific independent verifier
must check meaningful output, not only process exit or a reported digest.

## Inspect, enroll and hand off

Use an absolute `COH_BIN` from the matching executor-host installation.
Inspection does not enroll or execute a package:

```bash
: "${COH_BIN:?set the verified executor-host binary directory}"
: "${CUDA_REGISTRATION:?set the prepared registration JSON path}"
"$COH_BIN/coh" workload inspect --registration "$CUDA_REGISTRATION"
```

Enrollment writes the private registry and requires the bridge owner's
filesystem permission and the user's authorization to enroll this package:

```bash
: "${CUDA_STATE_ROOT:?set the existing private bridge state root}"
"$COH_BIN/coh" workload register \
  --registration "$CUDA_REGISTRATION" --state-root "$CUDA_STATE_ROOT"
```

Retain the unchanged registration and request digests. Place the request only
in the configured agent request CAS as `<request_sha256>.json` through the
deployment's documented preparation path; enrollment alone starts no job.
Hand the complete typed host ticket and request binding to
`cohesix-gpu-operations`, or `cohesix-agent-delegation` for a scoped agent.
Require fresh resource reservation and control lease at the same READY Worker.

After native completion, run the example's `verify` mode or the new independent
verifier on retained output, and correlate the signed job/Worker result.
Return package, input, registration, request and output identities, limits,
original job ID and native verification. A lost reply goes to original-ID
reconciliation. The shipped batch recipe has no checkpoint resume; a new run
after a confirmed failure needs a separate authorized operation.
