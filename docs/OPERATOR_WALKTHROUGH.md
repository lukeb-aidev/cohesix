<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Guide a first-time operator through one useful, evidence-backed Cohesix 26e session. -->
<!-- Author: Lukas Bower -->

# Cohesix Operator Walkthrough

This walkthrough turns one live QEMU Queen into a small operator workspace. By
the end you will have:

- inspected the seL4 and MCS state on the independent serial surface;
- run a checked, repeatable `.coh` target probe;
- shared the target safely with shell, REST, and Python clients;
- viewed the same bounded state through SwarmUI; and
- exported a portable evidence pack and offline timeline.

The point is not another dashboard. The point is that every view refers to one
target session, one generated policy, and one file-shaped control plane whose
claims remain tied to evidence.

Complete the [Quickstart](QUICKSTART.md) first if the host tools are not built
or the current QEMU profile has not booted. See the [Glossary](GLOSSARY.md) for
Cohesix-specific terms.

## The session you are building

```text
QEMU serial -------------------------> independent boot/MCS diagnostics

QEMU TCP console <---- hive-gateway <---- cohsh
             one owner       |       <---- coh
                             |       <---- curl / Python
                             `------------ SwarmUI
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

## 1. Establish target truth at the serial console

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

Do not collapse an unavailable live field into a generated value. That
separation is one of the reasons to use Cohesix: configuration, kernel truth,
and runtime observation stay distinguishable during diagnosis.

## 2. Save a repeatable direct-target baseline

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

This transcript is your small before-state. It is easy to retain, diff, and
repeat after a configuration change. More useful custom scripts are in
[Operator Recipes](OPERATOR_RECIPES.md#make-a-repeatable-health-check).

## 3. Give concurrent tools one safe owner

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

## 4. Explore the 26e target through the operator shell

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

These surfaces answer different questions:

| Surface | Question answered |
| --- | --- |
| `/proc/boot` | Which target/profile says it is running? |
| `/proc/root/reachable` | Is the bounded root reachability view healthy? |
| `/proc/schedule/summary` | Is scheduler ingress making bounded progress? |
| `/proc/lease/summary` | What lease activity is retained? |
| `/shard` | Which canonical Worker namespace is exposed by this profile? |
| `/log/queen.log` | What recent bounded Queen events remain available? |

The selected QEMU and Pi profiles declare 256 Worker slots: one Heartbeat, 127
GPU, and 128 LoRA. A namespace entry or admitted control write is not by itself
a READY or execution result. Keep configured, admitted, READY, provider
completion, target execution, and release acceptance as separate states.

Use `quit` to close `cohsh`. The gateway remains available to other clients.

## 5. Use the same state from Python

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
control semantics; it is not a second authority path. Use it to integrate
Cohesix observations with a test harness, inventory system, notebook, or
incident workflow without scraping terminal output. The supported backends and
typed APIs are in [Python Support](PYTHON_SUPPORT.md).

## 6. Project real host GPU inventory when available

On any compatible Linux AArch64 NVIDIA CUDA host—including Jetson Orin, AWS
G5g, NVIDIA DGX Spark, and similar systems—inspect local GPU discovery without
changing the target:

```bash
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
state. Keep that boundary when reporting results.

## 7. Export the session before its retained windows wrap

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

## 8. See the same session in SwarmUI

With the gateway still running:

```bash
export SWARMUI_TRANSPORT="rest"
export SWARMUI_REST_URL="$COH_REST_URL"
cargo run -p swarmui
```

Use SwarmUI to browse status, telemetry, retained replay state, and the Live
Hive projection. Its embedded console can mutate the target and is subject to
the same gateway role, ticket, policy, and request authentication as `cohsh`.
Visual state remains a presentation of target or host records; it is not
stronger proof than the records behind it.

## Move the workflow to Pi 4 or an AArch64 CUDA host

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

You have completed the walkthrough when one exact target identity is visible
through serial and `/proc/boot`, the gateway is the only shared TCP owner, the
read-only checks pass, and the saved evidence pack can be reviewed offline.
That is a small result, but it is repeatable, inspectable, and difficult to
accidentally overstate—the foundation for every larger Cohesix workflow.
