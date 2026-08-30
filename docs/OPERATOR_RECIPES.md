<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Provide situation-based Cohesix recipes for edge operations, AI hosts, incidents, and controlled change. -->
<!-- Author: Lukas Bower -->

# Cohesix Operator Recipes — Real-World Jobs

Use these recipes when you have an operational question, not when you merely
want to tour a component. They cover the recurring jobs that make Cohesix
valuable around edge and AI systems: deciding whether a node is safe to
change, constraining an automated action, investigating degraded service,
rolling an adapter forward or back, comparing targets, and preserving a case
for someone who was not present.

Start with the [Quickstart](QUICKSTART.md), then complete the live
[Operator Walkthrough](OPERATOR_WALKTHROUGH.md) before using a mutating recipe.

Commands assume a source checkout from the repository root. Release users
should replace `cargo run -p <package> --` with the corresponding executable in
`bin/` and follow the bundle's `QUICKSTART.md` for paths.

The useful habit across every situation is simple:

1. identify the exact target and transport;
2. prefer a bounded read before a write;
3. retain the typed result instead of guessing from a timeout; and
4. say what the result proves—and what it does not.

## Situation map

| Real-world situation | Recipe | Result or mutation |
| --- | --- | --- |
| You are evaluating Cohesix for a traffic, factory, medical, logistics, or private-AI workflow | [Explore a deployment before connecting hardware](#explore-a-deployment-before-connecting-hardware) | Labelled mock/dry-run plan and artifacts |
| A model, service, or image change needs a dependable preflight | [Turn the go/no-go review into a checked script](#make-a-repeatable-health-check) | Read-only pass/refusal |
| An edge node is responsive but latency or progress looks wrong | [Locate an isolation or scheduling mismatch](#inspect-sel4-and-mcs-state) | Read-only source-labelled diagnosis |
| A customer, reviewer, or incident lead asks what happened | [Build an offline-reviewable case](#capture-and-validate-an-evidence-pack) | Read-only evidence export |
| CI, an inventory service, or a notebook needs the same facts | [Feed bounded state into existing automation](#read-the-same-state-with-rest-and-python) | Read-only integration |
| A small fleet needs one shift-level view | [Find the hive that needs attention](#read-a-small-fleet) | Read-only fan-in |
| Existing tools expect ordinary files | [Expose only the allowed namespace through FUSE](#mount-the-bounded-namespace) | Reads or policy-checked writes |
| A Jetson or another AArch64 NVIDIA node is about to receive AI work | [Check the real accelerator before scheduling](#inspect-and-publish-aarch64-nvidia-gpu-state) | Local read; optional bounded publication |
| A private adapter is ready to stage or roll back | [Stage and reverse a private adapter rollout](#stage-and-reverse-a-private-adapter-rollout) | Host registry and target publication writes |
| An agent or controller should request one action without receiving a shell | [Let automation request one bounded action](#host-tickets-and-federation) | Allowlisted host action; read-only first |
| A QEMU result needs physical Pi confirmation | [Repeat the same check without merging the evidence](#repeat-a-check-on-qemu-and-pi) | Read-only comparison |
| A node needs planned maintenance | [Cordon, drain, verify, and resume](#run-a-maintenance-window) | Lifecycle mutation |
| Something failed and retries would destroy the first clue | [Route the first failed boundary](#triage-the-first-failed-boundary) | Read-only triage |

## Explore a deployment before connecting hardware

The Python playbooks are useful design tools for discussing a real deployment
with application, security, and operations teams. List the checked-in
scenarios:

```bash
python3 -m pip install -e tools/cohesix-py
cohesix-playbook --list
```

The catalogue contains nine patterns. Start with one of these four; together
they show the clearest reasons to put Cohesix between an AI workload and the
systems it can change:

| Situation to explore | Playbook | Why Cohesix is useful |
| --- | --- | --- |
| A factory needs visual QA without starving a higher-priority safety detector | `jetson-manufacturing-safety` | Makes GPU lease, quota, priority, and preemption decisions explicit and reviewable. |
| A team trains or adapts centrally, then serves on a Jetson-class edge node | `mixed-closed-loop-ai-factory` | Links training and inference waves to an export window and bounded GPU lease instead of hiding the handoff in scripts. |
| Private data must produce LoRA adapters without giving the trainer control of the fleet | `mac-private-peft-grid` | Separates adapter work, approval, export, and activation authority while retaining a rollback-oriented trail. |
| A medical edge workflow needs narrow resource use and an explainable export boundary | `mixed-medical-edge-ai` | Combines a small lease quota, explicit export window, and evidence-oriented control flow for later review. |

Select an identifier from the table and render its schedule, lease, approval,
export, and host-probe plan without touching a live target:

```bash
playbook="mixed-closed-loop-ai-factory"

cohesix-playbook \
  --playbook "$playbook" \
  --dry-run \
  --mock
```

Use `cohesix-playbook --list` for the five additional traffic-safety,
critical-infrastructure, logistics, endpoint-compliance, and release-factory
patterns. They use the same command form and do not need duplicate runbooks
here.

Use the artifacts under `out/examples/playbooks/` to ask concrete questions:
Which GPU or service is scarce? Which action needs approval? What must be
observable before mutation? Which receipt would let an operator distinguish
success from an admitted request?

The playbook names express integration patterns. Mock and dry-run output is not
hardware, provider, target, sector, safety, or compliance acceptance. Its value
is exposing the required control and evidence relationships before expensive
integration work begins.

<a id="make-a-repeatable-health-check"></a>
## Before a rollout: turn the go/no-go review into a checked script

Use `.coh` when a sequence matters. It is intentionally smaller than a shell
language: there are no downloads, variables, loops, command substitution, or
hidden host commands.

Create the local output directory:

```bash
mkdir -p out/operator
```

Save this as `out/operator/health.coh`:

```text
# Read-only operator health check
ping
EXPECT OK
cat /proc/boot
EXPECT OK
EXPECT SUBSTR path=/proc/boot
cat /proc/root/reachable
EXPECT OK
EXPECT SUBSTR path=/proc/root/reachable
cat /proc/schedule/summary
EXPECT OK
tail /log/queen.log 16
EXPECT OK
```

Check the file without contacting a target:

```bash
cargo run -p cohsh -- --check out/operator/health.coh
```

Run it through the gateway already selected by `COH_REST_URL`:

```bash
cargo run -p cohsh -- \
  --transport rest \
  --role queen \
  --script out/operator/health.coh
```

Script mode exits non-zero on a transport failure, typed command error, or
failed assertion and reports the source line plus a bounded recent-response
history. This makes the same file useful before a deployment, after a change,
and in a support request.

Use the checked-in `scripts/cohsh/smp_parity.coh` for a small target identity,
scheduler, lease, and `/proc` baseline. Generated scripts such as
`scripts/cohsh/boot_v0.coh` must be regenerated from their owning manifest/IR;
do not edit them as personal runbooks.

**What it proves:** the listed reads and assertions completed against the
selected session. **What it does not prove:** all target tests, Worker
execution, performance, or Pi hardware acceptance.

<a id="inspect-sel4-and-mcs-state"></a>
## When an edge node degrades: locate the isolation or scheduling mismatch

Use this when a node still answers `ping`, but control latency, Worker progress,
or driver responsiveness has changed. The serial/local-seat root console and
the NineDoor operator namespace are independent views; together they help
separate a bad generated policy, missing seL4 object, runtime admission issue,
and ordinary host slowdown.

At the target `cohesix>` console:

```text
bi
caps mcs
smp mcs
smp
```

The useful distinction is in the labels:

- `source=generated` describes compiler admission;
- `source=kernel` describes the selected seL4 configuration or BootInfo; and
- `source=runtime` describes the copied live registry snapshot.

An unavailable runtime value cannot be replaced by a generated value. `smp`
reports bounded userspace activity and selected driver progress; it is not a
kernel CPU-utilization meter.

Through `cohsh`, inspect the operator-facing consequences:

```text
cat /proc/boot
cat /proc/root/reachable
cat /proc/schedule/summary
cat /proc/lease/summary
ls /shard
tail /log/queen.log 32
```

The selected QEMU and Pi manifests declare one Heartbeat, 127 GPU, and 128 LoRA
Worker slots. The `/shard` namespace is canonical. A declared slot, directory,
queued request, READY observation, host-provider completion, and target
execution proof are separate facts.

**Why this is useful:** a generic VM monitor can say that a process exists;
these views let you route a mismatch among compiler policy, seL4 objects, live
admission, scheduler progress, and bounded operator state.

<a id="capture-an-evidence-pack"></a>
<a id="capture-and-validate-an-evidence-pack"></a>
<a id="evidence-packs-ci-and-siem"></a>
## After a change or incident: build an offline-reviewable case

Use this when another engineer, a customer, or an auditor must understand the
event without access to the live node. Capture soon after the event, before
bounded logs and telemetry wrap:

```bash
run_id="case-$(date -u +%Y%m%dT%H%M%SZ)"
pack="out/evidence/$run_id"

cargo run -p coh -- evidence pack \
  --rest-url "$COH_REST_URL" \
  --out "$pack"

cargo run -p coh -- evidence timeline \
  --input "$pack"
```

Add `--with-telemetry` only when the extra data is relevant and its size and
sensitivity are acceptable.

### Read the pack in this order

| File | First question |
| --- | --- |
| `summary.json` | Which requested paths were captured, missing, or errored? |
| `meta.json` | Which exporter, policy fingerprint, and redaction mode created the pack? |
| `bounds.json` | Which generated host limits and feature gates were applied? |
| `proc/boot` | Which profile and manifest did the target report? |
| `proc/schedule/*`, `proc/lease/*` | What bounded scheduler and lease state was retained? |
| `log/queen.log` | What recent target events remain? |
| `timeline.md` | What ordered, human-readable story can be reconstructed offline? |
| `timeline.ndjson` | What can an incident or analytics pipeline ingest? |

`meta.json`/`bounds.json` and `proc/boot` have different provenance. Preserve
and compare them; do not call a host policy fingerprint target proof.

### Validate for CI or support

```bash
python3 tools/cohesix-py/examples/ci_evidence_pack.py \
  --pack "$pack" \
  --out "$pack/ci-summary.json"
```

The validator checks required files, generated bounds, enabled schedule/lease
surfaces, audit relationships, and raw ticket leakage. A structurally valid
pack still proves only the target/session and surfaces it actually captured.

For a stable offline SIEM projection:

```bash
python3 tools/cohesix-py/examples/siem_export_ndjson.py \
  --pack "$pack" \
  --out "$pack/siem.ndjson"
```

### Know the redaction boundary

- Capability tickets and secret-like JSON keys are redacted from supported
  audit and host-ticket inputs.
- Malformed data on a surface that must be sanitized fails closed.
- Optional disabled paths are recorded as missing rather than invented.
- Logs, telemetry, and ordinary payload fields can still contain sensitive
  deployment data. Review the pack before sharing it.
- A non-zero export, absent top-level metadata, or unexplained core error is a
  partial pack, not a publishable result.

<a id="read-the-same-state-with-rest-and-python"></a>
## For CI or inventory: feed bounded state into existing automation

With one healthy gateway, a shell script can read a bounded node without
parsing an interactive terminal:

```bash
curl --fail-with-body --silent --show-error --get \
  --data-urlencode 'path=/proc/root/reachable' \
  --data-urlencode 'max_bytes=64' \
  "$COH_REST_URL/v1/fs/cat"
```

The Python SDK exposes the same operation as a typed backend:

```bash
python3 -m pip install -e tools/cohesix-py

python3 - <<'PY'
import os

from cohesix import RestBackend

backend = RestBackend(os.environ["COH_REST_URL"])
for path in (
    "/proc/root/reachable",
    "/proc/schedule/summary",
    "/proc/lease/summary",
):
    value = backend.read_file(path, 256).decode("utf-8")
    print(f"{path}: {value}")
PY
```

Use REST or Python for health services, deployment checks, notebooks, and
incident collection. Writes still require gateway request authentication and
the gateway's upstream role/ticket, target lifecycle, policy, and schema all
remain authoritative.

<a id="read-a-small-fleet"></a>
## During a shift: find the hive that needs attention

For one gateway:

```bash
cargo run -p coh -- fleet \
  --rest-url "$COH_REST_URL" \
  status
```

For several gateways, give each one a stable operator name:

```bash
cargo run -p coh -- fleet \
  --hive qemu=http://127.0.0.1:8080 \
  --hive pi4=http://127.0.0.1:8081 \
  status

cargo run -p coh -- fleet \
  --hive qemu=http://127.0.0.1:8080 \
  --hive pi4=http://127.0.0.1:8081 \
  pressure
```

`fleet status`, `lease-summary`, and `pressure` are read-only fan-in commands.
Each output row names its hive and carries a bounded error when one source is
unavailable; inspect every row instead of treating process completion as proof
that every hive was healthy.

This is intentionally not a distributed authority system. Each gateway still
owns one target session, and two targets can have different manifests,
evidence status, and request-auth boundaries.

<a id="mount-the-bounded-namespace"></a>
<a id="mounted-namespace-with-fuse"></a>
## For existing file-based tools: expose only the allowed namespace

`coh mount` is useful when existing read-only tools expect files. The mount is
a foreground FUSE server and exposes only the generated mount root and
allowlist; it does not broaden the attached role or ticket.

Linux needs FUSE 3 and a usable `/dev/fuse`. macOS needs an approved MacFUSE
installation. Check the host first:

```bash
cargo build -p coh --features fuse
cargo run -p coh --features fuse -- doctor
```

Create an empty mount point and start a REST-backed mount in its own terminal:

```bash
mount_dir="$PWD/out/mount/cohesix"
mkdir -p "$mount_dir"

cargo run -p coh --features fuse -- mount \
  --rest-url "$COH_REST_URL" \
  --at "$mount_dir"
```

Use ordinary tools from another terminal:

```bash
find "$mount_dir/proc" -maxdepth 2 -type f
sed -n '1,40p' "$mount_dir/proc/boot"
```

Unmount cleanly and wait for the foreground process to return:

```bash
# Linux
fusermount3 -u "$mount_dir"

# macOS
umount "$mount_dir"
```

Do not kill a busy mount first. Stop filesystem users, unmount, confirm the
mount is gone, then remove the empty directory if desired. Exactly one REST
mount can hold the host-side lock for a given gateway URL.

The mount is a convenience projection. Current target, host, and profile
support still needs its own validation; a successful local mount is not target
Worker or hardware proof.

<a id="inspect-and-publish-aarch64-nvidia-gpu-state"></a>
## Before scheduling AI work: check the real AArch64 NVIDIA accelerator

A Linux AArch64 NVIDIA CUDA system is useful in the 26e topology as a host for
Cohesix tools, CUDA workloads, models, containers, and evidence. Jetson Orin,
AWS G5g, NVIDIA DGX Spark, and compatible partner or future systems share this
architectural role. None is the seL4 Queen in this topology.

On a supported Ubuntu release, use the Linux ARM64 setup and real host checks.
On another distribution, use a matching release bundle or satisfy the same
toolchain/runtime contract explicitly; do not force the Ubuntu installer past
its OS guard:

```bash
./toolchain/setup_linux_arm64.sh
source "$HOME/.cargo/env"
source .venv/bin/activate

cargo run -p coh -- doctor
cargo run -p gpu-bridge-host -- --list
```

`coh doctor` prefers NVML and uses CUDA discovery when NVML is feature-limited.
`gpu-bridge-host --list` prints local inventory only. Backend availability and
reported fields can differ by SoC, discrete GPU, driver, and CUDA release; the
typed result is authoritative for that host.

Use SSD/NVMe for build output, model registries, caches, containers, and
evidence when available. Supply the location rather than baking a device path
into scripts. For example:

```bash
: "${COHESIX_FAST_ROOT:?set a writable fast-storage directory}"
export CARGO_TARGET_DIR="$COHESIX_FAST_ROOT/cargo-target"
```

To publish one real GPU snapshot to a gateway reachable through the approved
deployment or encrypted-tunnel boundary:

```bash
: "${COH_REST_URL:?set the gateway URL}"
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?set REST write authentication}"

cargo run -p gpu-bridge-host -- \
  --publish \
  --rest-url "$COH_REST_URL"
```

Add `--registry "$COH_GPU_REGISTRY"` only for a real, validated registry. No
registry produces explicit empty model state. Do not use `--mock` in an
operational result.

**What it proves:** local GPU discovery and, if published, a bounded inventory
snapshot reached the selected Queen. **What it does not prove:** CUDA workload
execution, device isolation, inference, NeMo, PEFT training, or target Worker
execution. Those need independent host-runtime and target evidence.

<a id="stage-and-reverse-a-private-adapter-rollout"></a>
## When a private adapter is ready: stage it with a rollback path

This recipe fits a private LoRA workflow in which training data and adapter
bytes must remain on the AI host, while Cohesix retains bounded job, registry,
activation, and evidence state. Rehearse the lifecycle first:

```bash
python3 tools/cohesix-py/examples/peft_roundtrip.py --mock
```

For a live registry operation, require every deployment-specific location
explicitly. Put large exports, adapters, registries, and model caches on fast
SSD/NVMe when available:

```bash
: "${COH_REST_URL:?set the gateway URL}"
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?set REST write authentication}"
: "${COH_PEFT_JOB:?set the admitted LoRA job id}"
: "${COH_PEFT_MODEL:?set the model or adapter id}"
: "${COH_PEFT_EXPORT:?set the export directory}"
: "${COH_PEFT_ADAPTER:?set the trained adapter directory}"
: "${COH_GPU_REGISTRY:?set the validated registry directory}"

cargo run -p coh -- peft export \
  --rest-url "$COH_REST_URL" \
  --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" \
  --job "$COH_PEFT_JOB" \
  --out "$COH_PEFT_EXPORT"
```

Train and evaluate outside the Cohesix VM with the selected CUDA/PEFT runtime.
The import directory must contain the bounded Cohesix inputs
`adapter.safetensors`, `lora.json`, and `metrics.json`; validate their base
model, dataset provenance, license, metrics, format, and runtime compatibility
before import.

```bash
cargo run -p coh -- peft import \
  --rest-url "$COH_REST_URL" \
  --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" \
  --model "$COH_PEFT_MODEL" \
  --from "$COH_PEFT_ADAPTER" \
  --job "$COH_PEFT_JOB" \
  --export "$COH_PEFT_EXPORT" \
  --registry "$COH_GPU_REGISTRY" \
  --publish

cargo run -p coh -- peft activate \
  --rest-url "$COH_REST_URL" \
  --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" \
  --model "$COH_PEFT_MODEL" \
  --registry "$COH_GPU_REGISTRY"

cargo run -p gpu-bridge-host -- \
  --registry "$COH_GPU_REGISTRY" \
  --publish \
  --rest-url "$COH_REST_URL"
```

Verify the registry projection through REST-backed `cohsh`:

```text
ls /gpu/models/available
cat /gpu/models/active
```

The active identifier proves a bounded registry pointer was accepted. It does
not prove that an inference process reloaded the adapter, passed a canary, or
served a request. Keep the deployment runtime's reload and canary result as a
separate receipt.

If that result fails, roll the pointer back and republish the registry:

```bash
cargo run -p coh -- peft rollback \
  --rest-url "$COH_REST_URL" \
  --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" \
  --registry "$COH_GPU_REGISTRY"

cargo run -p gpu-bridge-host -- \
  --registry "$COH_GPU_REGISTRY" \
  --publish \
  --rest-url "$COH_REST_URL"
```

Capture before-import, after-activation, and after-rollback evidence when the
workflow is being evaluated for production use. Current host helpers do not by
themselves provide a crash-safe train/evaluate/scan/reload transaction.

<a id="host-tickets-and-federation"></a>
## When automation needs authority: request one bounded host action

This is the Agent Action Airlock pattern. An AI agent, controller, or support
tool proposes one generated action record instead of receiving a shell,
cluster credential, or unrestricted host API. The target admits or refuses the
record; `host-ticket-agent` independently validates and executes it; status or
dead-letter records preserve the result.

Start with the non-mutating `systemd.status-check` action against a unit that
exists on the host running `host-ticket-agent`. In a REST-backed `cohsh`
session, replace the example unit only with one allowed by the selected
deployment:

```text
cat /host/tickets/spec.snapshot
echo {"schema":"host-ticket/v1","id":"status-demo-1","idempotency_key":"status-demo-1","action":"systemd.status-check","target":"/host/systemd/cohesix-agent.service/status"} > /host/tickets/spec
quit
```

Run one agent pass with deployment-specific durable state files:

```bash
: "${COH_REST_URL:?set the gateway URL}"
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?set REST write authentication}"

agent_state="$PWD/out/host-ticket-agent/status-demo"
mkdir -p "$agent_state"

cargo run -p host-ticket-agent -- \
  --rest-url "$COH_REST_URL" \
  --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" \
  --cursor "$agent_state/cursor.json" \
  --execution-journal "$agent_state/execution-journal.json" \
  --agent-lock "$agent_state/agent.lock" \
  --run-once
```

Reconnect through REST and inspect both terminal destinations:

```text
cat /host/tickets/status.snapshot
cat /host/tickets/deadletter.snapshot
```

A succeeded status check demonstrates the request/admission/executor/result
path for that host unit. A failed or refused result is also useful: preserve
its typed reason rather than widening the allowlist or retrying. Move to
`systemd.restart`, Docker, Kubernetes, GPU-lease, or PEFT actions only after the
exact target, arguments, idempotency behavior, rollback, and failure policy
have been reviewed for that deployment.

Federation uses the same bounded record shape with named source and target
hives. Enable `--relay` only when the resolved manifest declares the peers,
actions, hop bounds, and local hive; give every relay its own WAL, cursor,
journal, lock, credentials, and evidence retention. Federation forwards a
request—it does not grant ambient fleet authority or prove exactly-once
external side effects.

<a id="repeat-a-check-on-qemu-and-pi"></a>
## Before claiming hardware success: repeat the same check on QEMU and Pi

Use the same read-only script to compare operator behavior, while retaining
different target identities and proof files.

Run against QEMU after ensuring no gateway owns its console:

```bash
mkdir -p out/operator/compare
: "${QEMU_AUTH_TOKEN:?set the QEMU Queen token}"

COHSH_AUTH_TOKEN="$QEMU_AUTH_TOKEN" \
  cargo run -p cohsh -- \
  --transport tcp \
  --tcp-host 127.0.0.1 \
  --tcp-port 31337 \
  --script scripts/cohsh/smp_parity.coh \
  > out/operator/compare/qemu.txt
```

Only after the Pi has passed the exact-image boot and selected-network gates in
[Hardware Bring-up](HARDWARE_BRINGUP.md), run it against the current proven
address:

```bash
: "${PI4_TARGET_IP:?set the proven Pi address}"
: "${PI4_AUTH_TOKEN:?set the Pi Queen token}"

COHSH_AUTH_TOKEN="$PI4_AUTH_TOKEN" \
  cargo run -p cohsh -- \
  --transport tcp \
  --tcp-host "$PI4_TARGET_IP" \
  --tcp-port 31337 \
  --script scripts/cohsh/smp_parity.coh \
  > out/operator/compare/pi4.txt
```

Compare the command outcomes, but expect `/proc/boot` identities and
target-specific state to differ. A passing script on both targets proves the
script's assertions twice; it does not make QEMU evidence Pi evidence or merge
GENET and Wi-Fi acceptance.

For published comparisons, retain source commit, selected manifest, image
hash, seL4 build identity, transcript, and each target's own evidence pack.

<a id="run-a-maintenance-window"></a>
## For planned maintenance: cordon, drain, verify, and resume

Lifecycle control mutates target state. Capture a pre-maintenance evidence pack
and inspect active leases first:

```text
cat /proc/lifecycle/state
cat /proc/lease/active
lifecycle cordon
cat /proc/lifecycle/state
```

After cordon, stop new submissions and continuous publishers. Wait for active
leases to finish through their owning control paths. Do not edit `/proc`
output. When the active set is empty:

```text
cat /proc/lease/active
lifecycle drain
cat /proc/lifecycle/state
```

The intended maintenance state is `QUIESCED`. Perform the external maintenance,
rerun the relevant health check, then resume admission:

```text
lifecycle resume
cat /proc/lifecycle/state
cat /proc/root/reachable
```

`quiesce` is an explicit shortcut only when there are no active leases.
`lifecycle reset` changes lifecycle state to BOOTING; it does not reboot the
platform. The authenticated `reboot` command is a separate operation.

Capture a post-maintenance pack so the before/after records remain comparable.

<a id="triage-the-first-failed-boundary"></a>
## When something fails: route the first failed boundary

| Observation | First boundary to inspect | Do not conclude |
| --- | --- | --- |
| `coh doctor --mock` passes, QEMU does not boot | selected seL4 tree, build inputs, staged image, serial output | that target code is healthy because host parsing passed |
| Serial `ping` works, direct TCP cannot attach | TCP readiness, sole owner, endpoint, and console authentication | that the whole target failed |
| Direct TCP works, gateway says disconnected | another TCP owner, gateway console token, target address | that REST policy is the first cause |
| Gateway is connected, one command returns `ERR` | exact role, ticket, lifecycle, path, bound, and schema in the error | that retrying or changing the payload is safe |
| Pi reaches DHCP but `cohsh` fails | same-boot packet capture and selected GENET or Wi-Fi ingress path | that link or DHCP equals TCP acceptance |
| AArch64 NVIDIA GPU listing works, `/gpu` is absent | whether publication ran, gateway write auth, and manifest path enablement | that local inventory automatically changed the Queen |
| Evidence summary says an optional path is missing | `bounds.json` feature gate and active manifest | that the exporter fabricated or lost the path |
| SwarmUI looks healthy but a target read fails | underlying gateway and target record | that presentation state overrides control-plane evidence |

Use [Failure Modes](FAILURE_MODES.md) for the full routing guide. Preserve the
first typed failure and the independent serial surface before changing code or
adding retries.

## Use the strongest claim each situation actually earns

| Result | Useful today for | Do not promote it to |
| --- | --- | --- |
| Checked `.coh` reads, target identity, scheduler/lease observations, and a complete evidence pack | Pre-change gates, incident comparison, support cases, and operator handoff | Whole-milestone, performance, or physical-hardware acceptance |
| Real AArch64 NVIDIA inventory and bounded `/gpu` publication | Verifying that the intended accelerator host is present and visible to the control plane | CUDA execution, isolation, inference, PEFT training, or NeMo acceptance |
| PEFT export/import/activate/rollback and WorkerLora receipt surfaces | Rehearsing and integrating a private adapter lifecycle with explicit rollback | Training provenance, evaluation, scan, inference reload, or successful canary |
| Host tickets and playbooks | Designing and testing an action airlock or sector workflow with explicit authority and receipts | Production provider behavior, sector certification, or safe autonomous operation |
| The same script on QEMU and Pi | Comparing exact operator contracts while retaining two proof files | Treating VM evidence as physical-board evidence |

Use [Cohesix Status](STATUS.md) for the current public capability boundary,
[Use Cases](USE_CASES.md) for maturity-labelled deployment patterns,
[Host Tools](HOST_TOOLS.md) for exact modes, and the
[Build Plan](BUILD_PLAN.md) for planned hardening. The practical value already
available in 26e is not that every integration is finished. It is that an
operator can make bounded decisions, expose only narrow authority, keep target
and host claims separate, and reconstruct what happened after the live system
is gone.
