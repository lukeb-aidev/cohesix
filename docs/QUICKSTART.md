<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Help a newcomer use Cohesix to make and preserve a trustworthy edge-operation decision. -->
<!-- Author: Lukas Bower -->

# Quickstart — Make a Trustworthy Edge Decision

Suppose a camera, robot, factory line, or private AI node is due for a model or
service change. Before touching it, you need better answers than “the process
is running”:

- Which exact control-plane image and policy are active?
- Is the node responsive, schedulable, and free of an unexpected lease?
- Can several people or tools inspect it without competing for its console?
- Can you preserve what you saw for a review, incident, or rollback decision?

Cohesix answers those questions through a small seL4 control plane and a
bounded host-tool interface. This quickstart first rehearses the decision flow
without hardware, then repeats it against a live four-core SMP+MCS QEMU Queen.
If you have a Jetson or another compatible Linux AArch64 NVIDIA CUDA host, an
optional final step adds its real accelerator inventory without moving CUDA or
model weights into the VM.

You do not need a Raspberry Pi or NVIDIA GPU to begin. Mock results are always
labelled and never count as target or hardware proof.

This page exercises the current checkout. A versioned release bundle is an
immutable snapshot with its own `QUICKSTART.md`; use the instructions and
binaries inside that bundle instead of mixing them with current-tree
configuration.

Cohesix is a pre-production research OS. Check [Cohesix Status](STATUS.md) for
the current evidence boundary and the [Glossary](GLOSSARY.md) whenever a term
is unfamiliar.

## Choose your first useful situation

| Situation | Time | Start here | Useful outcome |
| --- | --- | --- | --- |
| Rehearse an operational review without hardware | About 5 minutes | [Offline rehearsal](#1-rehearse-a-gono-go-check-without-a-target) | A reviewable mock evidence case and familiarity with the bounded workflow |
| Decide whether a live target is ready for change | About 30 minutes after toolchain setup | [Live before-state](#2-capture-a-live-before-change-baseline) | Identity, reachability, scheduling, lease, and log observations for one exact QEMU image |
| Let a team inspect one target safely | After QEMU is healthy | [Shared gateway](#3-let-several-tools-inspect-one-target-safely) | Shell, REST, Python, and UI access through one console owner |
| Include the real AI host in the review | Hardware-dependent | [CUDA host](#4-add-a-real-cuda-host-to-the-review) | Local GPU discovery and an optional bounded inventory publication—not inference proof |
| Qualify a physical Pi control plane | Hardware-dependent | [Hardware bring-up](HARDWARE_BRINGUP.md) | Only the exact flashed image and fresh Pi evidence collected |

## Prerequisites

Run the installer for your host from the repository root. Each installer pins
Rust 1.97.1 and creates the repository `.venv`.

macOS 26 or later on Apple Silicon is the primary, fully pinned seL4 build
host:

```bash
./toolchain/setup_macos_arm64.sh
```

Ubuntu 22.04, 24.04, or 26.04 on ARM64 supports Cohesix host-tool builds and
diagnostic QEMU/TCG runs. The host tools are intended to remain portable across
Linux AArch64 NVIDIA CUDA systems—including Jetson Orin, AWS G5g, NVIDIA DGX
Spark, and compatible future systems—when the selected OS, driver, CUDA/NVML,
and package prerequisites are present:

```bash
./toolchain/setup_linux_arm64.sh
```

Then activate the installed tools:

```bash
source "$HOME/.cargo/env"
source .venv/bin/activate
```

The Linux path does not create the pinned macOS seL4 compiler/profile inputs.
It can consume an explicitly supplied compatible seL4 output tree for a
diagnostic QEMU run, but that run is not release or physical-target acceptance.

## 1. Rehearse a go/no-go check without a target

Start with the deterministic preflight. Think of this as rehearsing the checks
you will use before a deployment or after an incident. It validates policy,
ticket, and runtime contracts without probing unavailable target hardware:

```bash
cargo run -p coh -- doctor --mock
```

Every line should begin with `OK DOCTOR`. A skipped mount, GPU, or QEMU check is
expected in mock mode and is reported explicitly.

Open the shell and ask the same first questions you would ask of a remote edge
node:

```bash
cargo run -p cohsh -- --transport mock --role queen
```

At the `coh>` prompt, inspect the same file-shaped surfaces used by live
targets:

```text
help
ls /
cat /proc/boot
cat /proc/schedule/summary
cat /proc/lease/summary
cat /proc/root/reachable
quit
```

Now create an offline-reviewable evidence sample:

```bash
cargo run -p coh -- evidence pack \
  --mock \
  --out out/evidence/quickstart-mock

cargo run -p coh -- evidence timeline \
  --input out/evidence/quickstart-mock
```

Open `out/evidence/quickstart-mock/timeline.md` and
`out/evidence/quickstart-mock/summary.json`. The correct decision from this run
is “workflow rehearsed; no live node assessed.” The content is simulated, but
the bounded namespace, pack layout, redaction path, and offline timeline are
the same host-tool contracts used for a live review.

## 2. Capture a live before-change baseline

The selected seL4 16.0.0 output directory must already contain the kernel,
elfloader, generated headers, and configuration for the profile. The canonical
operational input is `out/sel4/profile-v2/qemu-smp-production`; preserved
`seL4/*` trees are archived diagnostic inputs only. See
[Toolchain setup](TOOLCHAIN_MAC_ARM64.md) if the profile does not exist.

In terminal 1, select that input explicitly and launch the authenticated TCP
console profile. This is the control plane you will assess, not merely a
process to get running:

```bash
export SEL4_BUILD_DIR="$PWD/out/sel4/profile-v2/qemu-smp-production"
test -f "$SEL4_BUILD_DIR/kernel/autoconf/autoconf.h"

./scripts/cohesix-build-run.sh \
  --sel4-build "$SEL4_BUILD_DIR" \
  --out-dir out/cohesix \
  --profile release \
  --root-task-features release-qemu,bootstrap-trace \
  --cargo-target aarch64-unknown-none \
  --transport tcp
```

Leave QEMU running. At the serial `cohesix>` prompt, establish the independent
boot and isolation facts you would retain before approving an edge change:

```text
ping
bi
caps mcs
smp mcs
mem
netstats
```

`caps mcs` and `smp mcs` keep generated admission, kernel configuration, and
live runtime observations source-labelled. They are inspection records, not a
performance or Pi acceptance claim.

In terminal 2, read the Queen console secret from the selected deployment
without putting it in shell history:

```bash
read -r -s COHSH_AUTH_TOKEN
export COHSH_AUTH_TOKEN

out/cohesix/host-tools/cohsh \
  --transport tcp \
  --tcp-host 127.0.0.1 \
  --tcp-port 31337 \
  --script scripts/cohsh/smp_parity.coh
```

The checked-in script attaches, pings the target, reads boot, scheduling, and
lease state, lists `/proc`, and exits non-zero if an assertion fails. Validate
any `.coh` file locally before using it:

```bash
out/cohesix/host-tools/cohsh --check scripts/cohsh/smp_parity.coh
```

For an interactive session, omit `--script` and add `--role queen`. These reads
answer the minimum go/no-go questions:

```text
ls /
ls /shard
cat /proc/boot
cat /proc/root/reachable
cat /proc/schedule/summary
cat /proc/lease/summary
test --mode quick --no-mutate
quit
```

| Decision question | Evidence surface |
| --- | --- |
| Am I connected to the intended image and profile? | `/proc/boot` plus the independent serial `bi` output |
| Is the bounded control plane reachable? | `/proc/root/reachable` and the successful checked script |
| Is scheduling making progress? | `/proc/schedule/summary` and source-labelled `smp mcs` output |
| Is an unexpected lease still active? | `/proc/lease/summary` |
| Which Worker roles and instances are exposed? | `/shard`, interpreted with the selected manifest |

The canonical Worker namespace is `/shard`; do not require the legacy
`/worker` alias. An admitted request, a configured Worker, and a READY Worker
are different states. Use the exact target evidence rules in
[Test Plan](TEST_PLAN.md) before making a Worker-execution claim.

The direct TCP console is authenticated but not encrypted. Keep it on loopback
or carry it through an authenticated encrypted tunnel. Only one direct console
owner may attach at a time.

## 3. Let several tools inspect one target safely

During a rollout review or incident, an operator, an automation check, and a UI
may all need the same state. Use `hive-gateway` so `cohsh`, `coh`, Python,
SwarmUI, and `curl` share one target session instead of racing to own its
console. Make sure the direct shell has exited first.

In terminal 2, create a separate request-auth secret for REST writes and start
the gateway:

```bash
read -r -s COH_AUTH_TOKEN
export COH_AUTH_TOKEN
read -r -s HIVE_GATEWAY_REQUEST_AUTH_TOKEN
export HIVE_GATEWAY_REQUEST_AUTH_TOKEN
export COH_REST_URL="http://127.0.0.1:8080"

out/cohesix/host-tools/hive-gateway \
  --bind 127.0.0.1:8080 \
  --tcp-host 127.0.0.1 \
  --tcp-port 31337
```

In terminal 3, confirm that the gateway and target agree on the session you
intend to operate:

```bash
export COH_REST_URL="http://127.0.0.1:8080"

curl --fail-with-body --silent --show-error \
  "$COH_REST_URL/v1/meta/status"

curl --fail-with-body --silent --show-error --get \
  --data-urlencode 'path=/proc/boot' \
  --data-urlencode 'max_bytes=1024' \
  "$COH_REST_URL/v1/fs/cat"
```

Continue only when status reports `connected: true` and `/proc/boot` identifies
the expected target profile. Then open a REST-backed shell:

```bash
out/cohesix/host-tools/cohsh \
  --transport rest \
  --rest-url "$COH_REST_URL" \
  --role queen
```

The gateway is now the only TCP owner. REST clients inherit its upstream role
and optional ticket; a client-side `--role queen` does not create additional
target authority. Keep the gateway on loopback unless an authenticated TLS
reverse proxy and deployment policy provide the external boundary.

## 4. Add a real CUDA host to the review

If the proposed change uses an NVIDIA edge host, inspect that host separately.
On a compatible Linux AArch64 NVIDIA CUDA system such as Jetson Orin, AWS G5g,
or NVIDIA DGX Spark:

```bash
cargo run -p coh -- doctor
cargo run -p gpu-bridge-host -- --list
```

This can tell you that the expected accelerator, memory, driver/runtime, and
discovery path are present. It cannot tell you that a model loaded, inference
completed, or a lease isolated a CUDA context.

To add one bounded inventory snapshot to the running Queen review:

```bash
: "${COH_REST_URL:?set the gateway URL}"
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?set REST write authentication}"

cargo run -p gpu-bridge-host -- \
  --publish \
  --rest-url "$COH_REST_URL"
```

Then confirm `/gpu/bridge/status` through the REST-backed shell. Add
`--registry "$COH_GPU_REGISTRY"` only when that variable names a real,
validated model registry. Empty model state is more useful than a fabricated
demo catalogue.

## What you can use this result for

You now have the beginnings of a real operational case, not just a tour of
components:

- **Before a model or service rollout:** retain exact target identity,
  reachability, scheduler state, leases, and—when relevant—real GPU inventory.
- **After a failure:** compare the before-state with a new pack without relying
  on screenshots or memory.
- **During a shared investigation:** give tools and people one bounded gateway
  instead of sharing an unrestricted shell.
- **Across QEMU, Pi, and AI hosts:** keep each proof source separate while
  using the same operator grammar and evidence workflow.

The workflow deliberately stops short of claiming CUDA execution, inference,
PEFT training, or physical Pi acceptance. Cohesix is useful because it makes
those missing proofs visible instead of allowing a green command to stand in
for them.

## Choose the next guide

- Follow the [Operator Walkthrough](OPERATOR_WALKTHROUGH.md) to make a complete
  go/no-go decision for an edge-AI change using target, gateway, automation,
  GPU-host, UI, and evidence views.
- Use [Operator Recipes](OPERATOR_RECIPES.md) for incidents, deployment
  rehearsals, private adapter rollout, bounded action requests, fleet reads,
  AArch64 NVIDIA GPU checks, Pi comparison, and maintenance.
- Read [Failure Modes](FAILURE_MODES.md) when a gate fails; do not hide a
  typed error with retries.
- Use [Hardware Bring-up](HARDWARE_BRINGUP.md) for a Pi 4. Build, flash,
  readback, boot, network, and authenticated command proof are separate gates.
- Use [Userland and CLI](USERLAND_AND_CLI.md) for the complete console and
  `.coh` grammar.

When finished, stop clients before the gateway, stop the gateway before QEMU,
and unset the secrets used by the session:

```bash
unset COH_AUTH_TOKEN COHSH_AUTH_TOKEN HIVE_GATEWAY_REQUEST_AUTH_TOKEN
```
