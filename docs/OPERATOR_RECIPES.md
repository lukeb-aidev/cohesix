<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Provide practical, evidence-aware Cohesix 26e recipes for new and returning operators. -->
<!-- Author: Lukas Bower -->

# Cohesix Operator Recipes

These recipes turn a healthy Cohesix session into repeatable work. Start with
the [Quickstart](QUICKSTART.md), then complete the live
[Operator Walkthrough](OPERATOR_WALKTHROUGH.md) before using a mutating recipe.

Commands assume a source checkout from the repository root. Release users
should replace `cargo run -p <package> --` with the corresponding executable in
`bin/` and follow the bundle's `QUICKSTART.md` for paths.

The useful habit across every recipe is simple:

1. identify the exact target and transport;
2. prefer a bounded read before a write;
3. retain the typed result instead of guessing from a timeout; and
4. say what the result proves—and what it does not.

## Recipe map

| If you want to... | Recipe | Mutation |
| --- | --- | --- |
| Turn a manual check into a dependable command | [Make a repeatable health check](#make-a-repeatable-health-check) | No |
| Inspect what 26e isolation is actually reporting | [Inspect seL4 and MCS state](#inspect-sel4-and-mcs-state) | No |
| Attach a useful artifact to a bug or change | [Capture and validate an evidence pack](#capture-and-validate-an-evidence-pack) | No |
| Integrate with an existing script or service | [Read the same state with REST and Python](#read-the-same-state-with-rest-and-python) | No |
| Check several Queens without opening several shells | [Read a small fleet](#read-a-small-fleet) | No |
| Use ordinary filesystem tools | [Mount the bounded namespace](#mount-the-bounded-namespace) | Reads or policy-checked writes |
| Use an AArch64 NVIDIA system as a Cohesix/GPU host | [Inspect and publish AArch64 NVIDIA GPU state](#inspect-and-publish-aarch64-nvidia-gpu-state) | Optional publication |
| Compare the QEMU and Pi operator experience | [Repeat a check on QEMU and Pi](#repeat-a-check-on-qemu-and-pi) | No |
| Prepare a node for planned maintenance | [Run a maintenance window](#run-a-maintenance-window) | Yes |
| Route a failure without random retries | [Triage the first failed boundary](#triage-the-first-failed-boundary) | No |

## Make a repeatable health check

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

## Inspect seL4 and MCS state

The serial/local-seat root console and the NineDoor operator namespace are
independent views. Use both when investigating temporal isolation.

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

## Capture and validate an evidence pack

Capture soon after the event, before bounded logs and telemetry wrap:

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

## Read the same state with REST and Python

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

## Read a small fleet

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

## Mount the bounded namespace

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

## Inspect and publish AArch64 NVIDIA GPU state

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

## Repeat a check on QEMU and Pi

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

## Run a maintenance window

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

## Triage the first failed boundary

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

## Know the current 26e boundary

Host tickets, federation, PEFT registry operations, built-in playbooks, and
host-side AI runtimes exist at different implementation and proof levels. They
are useful to developers, but they do not all constitute a production use-case
path today. Use [Cohesix Status](STATUS.md) for current public capability,
[Host Tools](HOST_TOOLS.md) for exact modes, and the
[Build Plan](BUILD_PLAN.md) for planned hardening rather than treating an old
demo or fixture as a current promise.

The strongest reasons to keep using Cohesix in 26e are already practical:
portable bounded checks, visible seL4/MCS authority, one consistent namespace
across several client styles, explicit target-versus-host boundaries, and
evidence that can be reviewed after the live system is gone.
