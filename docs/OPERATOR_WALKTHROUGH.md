<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Guide a first-time operator through an evidence-backed decision about changing an edge AI node. -->
<!-- Author: Lukas Bower -->

# Operator Walkthrough — Is This Edge AI Node Safe to Change?

A team wants to update a vision model or restart an AI service on an edge node.
The node appears healthy, but “the dashboard is green” is not enough. Before
approving the change, the operator needs to establish:

- the exact seL4 Queen and policy being consulted;
- whether control, scheduling, and lease state are healthy;
- whether the real CUDA host is the expected machine;
- whether several tools are seeing the same session; and
- whether the decision can be reconstructed after the live system is gone.

This walkthrough produces that go/no-go case. It uses a live QEMU Queen as the
control plane and can include a Jetson or another compatible Linux AArch64
NVIDIA CUDA host as the external AI machine. The same sequence applies to a Pi
Queen only after its independent physical acceptance gates pass.

The walkthrough stops before changing a model or service. In current 26e,
target control state, host GPU discovery, host-side AI execution, and physical
target evidence remain separate proof classes. A deployment-specific executor
must still prove the external change and return its result.

Complete the [Quickstart](QUICKSTART.md) first if the host tools are not built
or the current QEMU profile has not booted. See the [Glossary](GLOSSARY.md) for
Cohesix-specific terms.

## The operational picture you are building

```text
QEMU serial --------------------------> independent boot/MCS truth

QEMU TCP console <---- hive-gateway <---- operator shell
             one owner       |       <---- automation / Python
                             |       <---- evidence exporter / SwarmUI
                             `-------<---- bounded Jetson GPU publication

Jetson or CUDA host -----------------> model runtime remains host-side
```

The target exposes one authenticated TCP console. `hive-gateway` owns that
connection and multiplexes bounded REST requests. It does not create another
in-target listener or expand the role and ticket used for its upstream
attachment.

The examples assume:

- the repository root is the current directory;
- QEMU is running the selected TCP-console profile on `127.0.0.1:31337`;
- the Queen console secret is available without being committed or printed;
- the gateway will bind only to `127.0.0.1:8080`; and
- no other direct TCP client is attached when the gateway starts.

Release-bundle users should use the matching executable under `bin/` and the
paths in that bundle's `QUICKSTART.md`.

## 1. Identify the control plane before trusting it

Keep the QEMU terminal open. At `cohesix>`, run:

```text
ping
bi
caps mcs
smp mcs
mem
netstats
```

Look for these outcomes:

- `ping` responds and the console remains interactive;
- `bi` identifies the selected kernel and generated profile;
- `caps mcs` reports bounded MCS authority and object counts;
- `smp mcs` keeps generated admission, kernel state, and live registry state
  source-labelled; and
- `netstats` reports either a ready TCP path or a specific blocker.

Do not collapse an unavailable live field into a generated value. If the image
identity is unexpected or a required live field is unavailable, the rollout is
already a no-go. This separation keeps configuration, kernel truth, and runtime
observation distinguishable during diagnosis.

## 2. Save the before-change baseline

Before starting the gateway, use the direct console for one foreground script.
In terminal 2:

```bash
: "${COHSH_AUTH_TOKEN:?set the Queen console authentication token}"

mkdir -p out/operator
out/cohesix/host-tools/cohsh \
  --transport tcp \
  --tcp-host 127.0.0.1 \
  --tcp-port 31337 \
  --script scripts/cohsh/smp_parity.coh \
  > out/operator/qemu-smp-parity.txt
```

The script attaches, pings, reads `/proc/boot`, schedule and lease summaries,
lists `/proc`, then performs a clean quit. A zero exit means its exact
assertions passed; it does not mean every 26e test or physical target passed.

This transcript is the before-state for the proposed change. Retain it with the
change or incident identifier, then repeat the same script after the operation.
The value is not the text file by itself; it is the ability to compare the same
bounded assertions against the same identified target. More useful custom
scripts are in
[Operator Recipes](OPERATOR_RECIPES.md#make-a-repeatable-health-check).

## 3. Give the review team one safe session owner

The direct script has exited, so the TCP console is free. In terminal 2, load
the console secret and a distinct REST write secret, then start the gateway:

```bash
: "${COH_AUTH_TOKEN:?set the Queen console authentication token}"
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?set a distinct REST write token}"
export COH_REST_URL="http://127.0.0.1:8080"

out/cohesix/host-tools/hive-gateway \
  --bind 127.0.0.1:8080 \
  --tcp-host 127.0.0.1 \
  --tcp-port 31337
```

In terminal 3, set the URL and confirm both host policy and live target state:

```bash
export COH_REST_URL="http://127.0.0.1:8080"

curl --fail-with-body --silent --show-error \
  "$COH_REST_URL/v1/meta/status"

curl --fail-with-body --silent --show-error \
  "$COH_REST_URL/v1/meta/bounds"

curl --fail-with-body --silent --show-error --get \
  --data-urlencode 'path=/proc/boot' \
  --data-urlencode 'max_bytes=1024' \
  "$COH_REST_URL/v1/fs/cat"
```

Continue only when:

- status reports `connected: true`;
- bounds reports the expected gateway generated-policy fingerprint;
- `/proc/boot` identifies the intended target profile and manifest; and
- the gateway and target fingerprints have been compared as separate facts.

`/v1/meta/bounds` is compiled host policy. `/proc/boot` is target-reported
state. A match supports parity for this session; either value alone does not.

## 4. Answer the control-plane go/no-go questions

Open a REST-backed shell in terminal 3:

```bash
out/cohesix/host-tools/cohsh \
  --transport rest \
  --rest-url "$COH_REST_URL" \
  --role queen
```

At `coh>`, run the read-only tour:

```text
ping
ls /
ls /proc
ls /shard
cat /proc/boot
cat /proc/root/reachable
cat /proc/schedule/summary
cat /proc/lease/summary
tail /log/queen.log 32
test --mode quick --no-mutate
```

These surfaces answer the questions that matter before an edge change:

| Decision question | Surface | Stop condition |
| --- | --- | --- |
| Is this the intended target? | `/proc/boot` | Image, profile, or manifest differs from the change record. |
| Is bounded root control reachable? | `/proc/root/reachable` | A required root or service is unreachable. |
| Is scheduler ingress progressing? | `/proc/schedule/summary` | Required progress is absent or a typed overload persists. |
| Will this collide with current work? | `/proc/lease/summary` | An unexplained active lease owns the resource. |
| Are the expected Worker roles exposed? | `/shard` | The selected manifest and visible role layout disagree. |
| Is there a recent warning relevant to the change? | `/log/queen.log` | A retained fault or refusal has not been routed. |

The selected QEMU and Pi profiles declare 256 Worker slots: one Heartbeat, 127
GPU, and 128 LoRA. A namespace entry or admitted control write is not by itself
a READY or execution result. Keep configured, admitted, READY, provider
completion, target execution, and release acceptance as separate states.

Use `quit` to close `cohsh`. The gateway remains available to other clients.

## 5. Give deployment automation the same bounded facts

Install the target-neutral Python package into the repository virtual
environment:

```bash
python3 -m pip install -e tools/cohesix-py
```

Then perform two bounded read-only operations through the same gateway:

```bash
python3 - <<'PY'
import os

from cohesix import RestBackend

backend = RestBackend(os.environ["COH_REST_URL"])
print("root entries:", backend.list_dir("/"))
print(backend.read_file("/proc/root/reachable", 64).decode("utf-8"))
PY
```

This is deliberately small. The Python package mirrors existing namespace and
control semantics; it is not a second authority path. A deployment gate can
now refuse a rollout on the same target facts the operator reviewed, without
scraping terminal output or acquiring a shell. The supported backends and typed
APIs are in [Python Support](PYTHON_SUPPORT.md).

## 6. Verify the actual accelerator as a separate boundary

The Queen can be healthy while the external AI host is wrong, missing its
driver, or exposing a different accelerator. On any compatible Linux AArch64
NVIDIA CUDA host—including Jetson Orin, AWS G5g, NVIDIA DGX Spark, and similar
systems—inspect local GPU discovery without changing the target:

```bash
cargo run -p coh -- doctor
cargo run -p gpu-bridge-host -- --list
```

NVML is preferred where complete; feature-limited NVML implementations such as
some Jetson profiles fall back to CUDA discovery. `--list` is local inventory
only. To publish one production-mode snapshot through the running gateway:

```bash
cargo run -p gpu-bridge-host -- \
  --publish \
  --rest-url "$COH_REST_URL"
```

If you maintain a real model registry, add `--registry "$COH_GPU_REGISTRY"`
after setting that path explicitly. Without a registry, publication reports
empty model state rather than inventing models or an active selection.

Verify the projected state with REST-backed `cohsh`:

```text
ls /gpu
cat /gpu/bridge/status
```

GPU discovery and publication do not execute a workload, isolate a CUDA
context, prove a model runtime, or prove PEFT training. In 26e, CUDA/NVML and AI
execution remain host-side; Cohesix projects bounded inventory and control
state. A rollout that requires CUDA is a no-go until its separate runtime probe
and deployment-specific executor also succeed.

## 7. Freeze the decision record before retained windows wrap

While the gateway is still connected:

```bash
run_id="operator-$(date -u +%Y%m%dT%H%M%SZ)"
pack="out/evidence/$run_id"

cargo run -p coh -- evidence pack \
  --rest-url "$COH_REST_URL" \
  --out "$pack"

cargo run -p coh -- evidence timeline \
  --input "$pack"
```

Review these first:

- `summary.json` — every requested path is classified as captured, missing, or
  errored;
- `meta.json` and `bounds.json` — host-tool and generated-policy provenance;
- `proc/boot` — target-reported identity;
- `log/queen.log` — the retained log snapshot; and
- `timeline.md` — the offline, human-readable correlation view.

An optional path can be absent by design. A non-zero exporter exit, missing
top-level metadata, or unexplained error is not a publishable pack. The full
redaction and validation recipe is in
[Operator Recipes](OPERATOR_RECIPES.md#capture-and-validate-an-evidence-pack).

## 8. Review the case with another human in SwarmUI

With the gateway still running:

```bash
export SWARMUI_TRANSPORT="rest"
export SWARMUI_REST_URL="$COH_REST_URL"
cargo run -p swarmui
```

Use SwarmUI to browse status, telemetry, retained replay state, and the Live
Hive projection. This is the useful handoff point for a second operator: they
can see the same target and GPU projections, then inspect the evidence timeline
without inheriting an unrestricted host shell. SwarmUI's embedded console can
mutate the target and is subject to the same gateway role, ticket, policy, and
request authentication as `cohsh`. Visual state remains a presentation of
target or host records; it is not stronger proof than the records behind it.

## Make the go/no-go decision

The node is ready to enter a deployment-specific change procedure only when:

- serial and `/proc/boot` identify the intended target and profile;
- the checked baseline passes and required control/scheduler state is healthy;
- every existing lease is expected and compatible with the operation;
- the gateway is the sole shared console owner and reports the same session;
- a CUDA-dependent change has separate real-host inventory and runtime proof;
  and
- the evidence pack is complete enough to compare with the after-state.

Stop and route the first failed boundary when any item is false. Do not turn an
unknown GPU runtime into a target fault, a target refusal into a retry loop, or
a successful model-registry write into proof that an inference runtime
reloaded.

For a private adapter, continue with
[Stage and reverse a private adapter rollout](OPERATOR_RECIPES.md#stage-and-reverse-a-private-adapter-rollout).
For a constrained service, GPU, or model action proposed by automation, use
[Let automation request one bounded action](OPERATOR_RECIPES.md#host-tickets-and-federation).

## Move the decision workflow to Pi 4 or another CUDA host

For a Pi 4 Queen, keep the sequence but change the proof source:

1. build, flash, read back, and boot the exact image through
   [Hardware Bring-up](HARDWARE_BRINGUP.md);
2. retain serial as the recovery and independent evidence surface;
3. prove the selected GENET or Wi-Fi path for that same boot;
4. point the sole gateway at the proven target address; and
5. rerun the read-only tour and evidence export.

QEMU success is not Pi success. GENET and CYW43/Wi-Fi are also separate
physical evidence lanes.

A Linux AArch64 NVIDIA CUDA system—such as Jetson Orin, AWS G5g, NVIDIA DGX
Spark, or a compatible partner system—is a host for Cohesix tools, GPU
workloads, and model runtimes; it is not the seL4 target in this topology. Use
fast SSD/NVMe or equivalent high-performance storage for build outputs, model
registries, caches, containers, and evidence by supplying those locations
through the tools' normal arguments or environment. Do not infer target Worker
or AI-runtime acceptance from a successful host GPU inventory.

## Finish cleanly

1. Export any final evidence before stopping the gateway.
2. Exit `cohsh` and SwarmUI.
3. Stop one-shot or continuous host publishers.
4. Stop `hive-gateway` so it releases the target console.
5. Stop QEMU from its own terminal.
6. Unset session credentials and apply the deployment retention policy.

```bash
unset COH_AUTH_TOKEN COHSH_AUTH_TOKEN HIVE_GATEWAY_REQUEST_AUTH_TOKEN
```

You have completed the walkthrough when you can make a defensible go/no-go
decision from one exact target identity, one shared session, separate target
and GPU-host observations, and an offline-reviewable evidence pack. That is the
practical value of Cohesix before it performs any external action: it makes the
authority, preconditions, unknowns, and eventual result difficult to confuse.
