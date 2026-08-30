<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Provide the shortest safe path from a current Cohesix source checkout to a useful mock or live QEMU session. -->
<!-- Author: Lukas Bower -->

# Quickstart — Current Source Tree

Cohesix gives an operator one bounded, scriptable view of an seL4 Queen,
isolated services, and Workers. This quickstart gets you to a useful result in
two stages:

1. a five-minute host-only session that teaches the tools and produces a real
   evidence pack; and
2. a live QEMU session that inspects the current four-core SMP+MCS target.

You do not need a Raspberry Pi or NVIDIA GPU for the first stage. Mock mode is
clearly labelled and never counts as target proof.

This page exercises the current checkout. A versioned release bundle is an
immutable snapshot with its own `QUICKSTART.md`; use the instructions and
binaries inside that bundle instead of mixing them with current-tree
configuration.

Cohesix is a pre-production research OS. Check [Cohesix Status](STATUS.md) for
the current evidence boundary and the [Glossary](GLOSSARY.md) whenever a term
is unfamiliar.

## Choose your first result

| Time | Goal | Start here | What it proves |
| --- | --- | --- | --- |
| About 5 minutes | Learn the shell, inspect bounded state, and export evidence | [Host-only orientation](#1-get-a-useful-result-without-a-target) | Host tools and mock contracts only |
| About 30 minutes after toolchain setup | Boot seL4, inspect MCS state, and run a repeatable target check | [Live QEMU](#2-boot-and-inspect-a-live-qemu-queen) | Only the exact QEMU image and commands exercised |
| After QEMU is healthy | Share one target safely with shell, REST, Python, and UI clients | [REST gateway](#3-share-the-live-target-through-one-gateway) | Gateway and client behavior against that target session |
| Hardware-dependent | Boot the supported Pi 4 image | [Hardware bring-up](HARDWARE_BRINGUP.md) | Only the exact flashed image and fresh Pi evidence collected |

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

## 1. Get a useful result without a target

Start with the deterministic preflight. It validates policy, ticket, and
runtime contracts without probing unavailable target hardware:

```bash
cargo run -p coh -- doctor --mock
```

Every line should begin with `OK DOCTOR`. A skipped mount, GPU, or QEMU check is
expected in mock mode and is reported explicitly.

Open the shell:

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
`out/evidence/quickstart-mock/summary.json`. You have exercised three durable
Cohesix ideas: a bounded namespace, one command grammar for people and scripts,
and a portable evidence artifact. The content is simulated, but the pack
layout, redaction path, and offline timeline workflow are real host-tool
contracts.

## 2. Boot and inspect a live QEMU Queen

The selected seL4 16.0.0 output directory must already contain the kernel,
elfloader, generated headers, and configuration for the profile. The canonical
operational input is `out/sel4/profile-v2/qemu-smp-production`; preserved
`seL4/*` trees are archived diagnostic inputs only. See
[Toolchain setup](TOOLCHAIN_MAC_ARM64.md) if the profile does not exist.

In terminal 1, select that input explicitly and launch the authenticated TCP
console profile:

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

Leave QEMU running. At the serial `cohesix>` prompt, inspect the kernel-facing
state that makes the 26e target distinct from an ordinary application VM:

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

For an interactive session, omit `--script` and add `--role queen`. Useful
first reads are:

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

The canonical Worker namespace is `/shard`; do not require the legacy
`/worker` alias. An admitted request, a configured Worker, and a READY Worker
are different states. Use the exact target evidence rules in
[Test Plan](TEST_PLAN.md) before making a Worker-execution claim.

The direct TCP console is authenticated but not encrypted. Keep it on loopback
or carry it through an authenticated encrypted tunnel. Only one direct console
owner may attach at a time.

## 3. Share the live target through one gateway

Use `hive-gateway` when you want several clients—`cohsh`, `coh`, Python,
SwarmUI, or `curl`—to observe one target session. Make sure the direct shell has
exited first.

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

## Why come back to Cohesix

- The same `.coh` check can be syntax-checked offline and then run against a
  selected QEMU or Pi target without becoming an unrestricted shell script.
- Shell, REST, Python, FUSE, and SwarmUI project the same bounded namespace
  rather than inventing separate control protocols.
- Evidence packs preserve what was present, absent, or unavailable instead of
  turning a successful command into a broader system claim.
- seL4 capability and MCS state remain visible as generated, kernel, and live
  runtime facts, which makes isolation failures diagnosable.
- GPU and AI workloads remain host-side while Cohesix keeps authority,
  lifecycle, telemetry, and receipts bounded at the control-plane boundary.

## Choose the next guide

- Follow the [Operator Walkthrough](OPERATOR_WALKTHROUGH.md) for one complete
  live session, including gateway, script, Python, UI, and evidence steps.
- Use [Operator Recipes](OPERATOR_RECIPES.md) for reusable health checks,
  incident packs, fleet reads, FUSE, AArch64 NVIDIA GPU inventory, Pi
  comparison, and maintenance.
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
