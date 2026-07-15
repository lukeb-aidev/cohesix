<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Provide verified task-oriented Cohesix operator recipes beyond the canonical live walkthrough. -->
<!-- Author: Lukas Bower -->
# Cohesix Operator Recipes

This document contains advanced procedures that would obscure the single
end-to-end journey in
[OPERATOR_WALKTHROUGH.md](OPERATOR_WALKTHROUGH.md). Complete that walkthrough
first: these recipes assume a healthy target, one `hive-gateway` as the sole
TCP console owner, `COH_REST_URL` set to its loopback URL, and the required
request-auth token loaded without printing it.

Commands use a source checkout from the repository root. Release-bundle users
should replace `cargo run -p <package> --` with the corresponding `./bin/<tool>`
and use the bundle's `QUICKSTART.md`. These procedures never relax manifest,
role, ticket, lifecycle, policy, or request-size checks.

## Evidence packs, CI, and SIEM

### Capture an evidence pack

Capture while the relevant retained windows still contain the event under
investigation. Telemetry is opt-in because it can materially increase the
pack's size:

```bash
run_id="incident-$(date -u +%Y%m%dT%H%M%SZ)"
pack="out/evidence/$run_id"

cargo run -p coh -- evidence pack \
  --rest-url "$COH_REST_URL" \
  --out "$pack" \
  --with-telemetry

cargo run -p coh -- evidence timeline --input "$pack"
```

`evidence timeline` is offline: after a successful pack export it needs no
target, gateway, or credential. It writes `timeline.ndjson` for machines and
`timeline.md` for review. Events are ordered deterministically by available
audit sequence, lease sequence, correlation key, and event kind. Host-ticket
events carry federation fields when those fields were present, allowing the
same `id` and `idempotency_key` to be followed across hives.

### Pack contract

The exporter creates this deterministic layout. A profile-disabled or absent
optional path is represented in `summary.json`, not silently omitted from the
inventory.

| Path relative to the pack | Contract |
| --- | --- |
| `meta.json` | `cohesix-evidence-pack/meta-v1`; generated manifest and policy fingerprints, ticket-redaction mode, and telemetry-selection flag. |
| `bounds.json` | Gateway `/v1/meta/bounds` response, or the equivalent compiled local bounds for a non-REST export. It describes client/gateway policy and is not proof of the target image by itself. |
| `summary.json` | `cohesix-evidence-pack/summary-v1`; sorted item inventory with `captured`, `missing`, or `error` status and aggregate counts. |
| `proc/boot` | Target boot/profile and manifest evidence when exposed. |
| `proc/schedule/{summary,queue}` | Scheduler snapshots when enabled by `bounds.json`. |
| `proc/lease/{summary,active,preemptions}` | Lease snapshots when enabled by `bounds.json`. |
| `log/queen.log` | Bounded retained Queen log snapshot; the current exporter budgets for the 2,048-line retained ring rather than the interactive tail default. |
| `audit/{export,journal,decisions}` | Audit metadata and redacted JSONL when audit is exposed. |
| `host/tickets/{spec,status,deadletter}` | Redacted host-ticket request and receipt JSONL when host tickets are exposed. |
| `replay/status` | Replay state when replay is exposed. |
| `telemetry/` | Pulled Queen telemetry only when `--with-telemetry` is set. |
| `timeline.ndjson`, `timeline.md` | Offline correlation products created by the separate timeline command. |

The two manifest fingerprints have different provenance: `meta.json` and
`bounds.json` use the host tool's compiled/generated policy, while `proc/boot`
is target evidence. Preserve and compare both before claiming parity.

### Redaction and failure rules

- Audit `ticket` values other than `none` are replaced with
  `sha256:<hex-digest>`. This preserves correlation without exporting a raw
  capability ticket.
- Audit and host-ticket JSON are recursively redacted when a key denotes a
  token, authorization value, authentication reference, secret, password,
  signing key, or API key. The replacement value is `<redacted>`.
- Invalid UTF-8 or malformed JSONL on a surface that must be sanitized fails
  closed; the exporter does not copy the unsanitized line.
- A missing or disabled namespace path is non-fatal and appears as
  `status: "missing"` with `detail: "not-found"` in `summary.json`.
- Optional capture failures can appear as `status: "error"`. A non-missing
  failure on a core capture can terminate the command before a final summary is
  committed. Treat a non-zero command exit or a pack without all three top-level
  JSON files as a partial, non-publishable capture.
- Redaction is deliberately narrow. Review the resulting pack under the
  deployment's data-classification policy before sharing it; telemetry,
  application logs, and non-secret payload fields can still contain sensitive
  operational data.

### CI validation

The checked-in validator is deterministic and exits `0` only when its contract
passes; it exits `2` for a failed pack:

```bash
python3 tools/cohesix-py/examples/ci_evidence_pack.py \
  --pack "$pack" \
  --out "$pack/ci-summary.json"
```

It requires `meta.json`, `bounds.json`, `summary.json`, and `log/queen.log`; it
also requires each schedule or lease file enabled by `bounds.json`, enforces
their advertised byte caps, requires journal and decisions when `audit/export`
exists, bounds optional `replay/status`, and rejects a raw
`cohesix-ticket-...` token in the audit journal. Its output schema is
`cohesix-ci-evidence-pack/v1`. This structural check complements, but does not
replace, target-specific boot, hardware, or test-plan evidence.

### SIEM export

Normalize the currently supported audit and active-lease sources to stable
NDJSON:

```bash
python3 tools/cohesix-py/examples/siem_export_ndjson.py \
  --pack "$pack" \
  --out "$pack/siem.ndjson"
```

The exporter is offline and deterministic. It emits
`cohesix-siem-event/v1` rows from `audit/journal`, `audit/decisions`, and
`proc/lease/active` when present. It does not currently normalize host-ticket
files; use `timeline.ndjson` when cross-hive ticket correlation is required.
Validate either output against the receiving system's ingestion limits before
shipping it.

## Mounted namespace with FUSE

### Prerequisites

`coh mount` is a foreground filesystem server. It exposes only the generated
mount root and allowlist; creating a mount does not broaden the attached role or
ticket.

- Linux needs a FUSE 3 runtime, a usable `/dev/fuse`, and `coh` built for Linux.
  Repository Linux and release builds include the FUSE backend.
- macOS needs MacFUSE installed, approved by the operator, and a device such as
  `/dev/macfuse0`. A direct source build must enable the optional `fuse` feature;
  `scripts/cohesix-build-run.sh` enables it for the staged macOS `coh` binary.
- The mount point must already exist and should be empty.

For a direct source invocation, build the correct feature and inspect the
generated doctor output. `doctor` also checks policy, ticket, GPU, and runtime
prerequisites, so evaluate its individual `check=mount` result if an unrelated
check fails:

```bash
cargo build -p coh --features fuse
cargo run -p coh --features fuse -- doctor
```

### REST-backed mount

Keep the gateway running and start the mount in its own terminal:

```bash
: "${COH_REST_URL:?set the gateway URL}"
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?set gateway request authentication}"
mount_dir="$PWD/out/mount/cohesix"
mkdir -p "$mount_dir"

cargo run -p coh --features fuse -- mount \
  --rest-url "$COH_REST_URL" \
  --at "$mount_dir"
```

Use a second terminal to read the mounted tree. Writes still pass through the
gateway and target policy. Exactly one REST mount can hold the host-side lock
for a given gateway URL; unmount the first before starting another.

### Direct mount

Stop `hive-gateway` first. The mount becomes the sole TCP console owner for its
lifetime:

```bash
: "${COH_AUTH_TOKEN:?set the target console token}"
mount_dir="$PWD/out/mount/cohesix"
mkdir -p "$mount_dir"

cargo run -p coh --features fuse -- mount \
  --host "${COH_TCP_HOST:-127.0.0.1}" \
  --port "${COH_TCP_PORT:-31337}" \
  --at "$mount_dir"
```

Do not start another direct client until the mount is gone.

### Unmount and verify cleanup

Unmount from a second terminal, then wait for the foreground `coh mount`
process to return:

```bash
# Linux
fusermount3 -u "$mount_dir"

# macOS
umount "$mount_dir"
```

Do not terminate a busy mount first. If unmount fails, stop filesystem users,
retry the host unmount command, and confirm the mount is absent before removing
the empty directory or restarting the gateway.

## Host tickets and federation

The request, result, lifecycle, and federation fields are owned by
[INTERFACES.md#host-tickets-and-federation](INTERFACES.md#host-tickets-and-federation).
The examples below use `RestBackend` so the gateway remains the only direct TCP
client. The active resolved manifest must enable the host-ticket namespace and
allow the selected action. Install the package first as described in
[PYTHON_SUPPORT.md#requirements-and-installation](PYTHON_SUPPORT.md#requirements-and-installation).

### One local read-only host action

This Linux example queues a systemd status check. Replace the unit with one
that exists on the agent host; the agent executes `systemctl` locally and only
after claiming the manifest-authorized ticket:

```bash
python3 - <<'PY'
import os

from cohesix import CohesixOrchestrator, HostTicketRequest, RestBackend

backend = RestBackend(
    os.environ["COH_REST_URL"],
    request_auth_token=os.environ.get("HIVE_GATEWAY_REQUEST_AUTH_TOKEN"),
)
orchestrator = CohesixOrchestrator(backend)
request = HostTicketRequest(
    ticket_id="status-1",
    idempotency_key="status-1a",
    action="systemd.status-check",
    target="/systemd/cohesix-agent.service",
)
for result in orchestrator.enqueue_host_tickets([request]):
    print(result.path, result.bytes_written)
PY

export COHESIX_RESOLVED_MANIFEST="${COHESIX_RESOLVED_MANIFEST:-configs/generated/root_task_resolved.json}"
test -f "$COHESIX_RESOLVED_MANIFEST"

cargo run -p host-ticket-agent -- \
  --manifest "$COHESIX_RESOLVED_MANIFEST" \
  --rest-url "$COH_REST_URL" \
  --cursor out/host-ticket-agent/local-cursor.json \
  --run-once
```

Read both terminal and failure receipts; match the pair of stable identifiers,
not only the most recent line:

```bash
curl --fail-with-body --silent --show-error --get \
  --data-urlencode 'path=/host/tickets/status' \
  --data-urlencode 'max_bytes=4096' \
  "$COH_REST_URL/v1/fs/cat"

curl --fail-with-body --silent --show-error --get \
  --data-urlencode 'path=/host/tickets/deadletter' \
  --data-urlencode 'max_bytes=4096' \
  "$COH_REST_URL/v1/fs/cat"
```

Use a unique cursor file for every agent instance. Reusing `id` plus
`idempotency_key` deliberately deduplicates a terminal action; create a new pair
for a genuinely new operation.

### Federated relay

Federation requires independently generated manifests:

1. The source manifest names its own `local_hive`, the target as a peer, the
   peer REST URL, and an `auth_ref` environment-variable name.
2. The target manifest names the target as its `local_hive` and enables the
   same ticket action and result schemas.
3. The source agent process receives the target gateway token through the
   manifest's `auth_ref`; the token is not stored in the manifest or ticket.
4. Each source and target agent has a dedicated cursor. The relaying source also
   has a dedicated WAL.

With the checked-in default source profile, `hive-a` knows peer `hive-b` at
`http://127.0.0.1:8081` and resolves its request token from
`COHESIX_RELAY_HIVE_B_TOKEN`. A real target must be built from a corresponding
`hive-b` profile; do not run the same `local_hive = "hive-a"` manifest on both
sides.

Point each process at the resolved manifest generated for its own target. The
paths below are required inputs: do not let either process fall back to the
checked-in `hive-a` default when it is serving `hive-b`.

On the target host, run an ordinary processing agent against the target
gateway. On the source host, run a relaying agent against the source gateway:

```bash
# Target host: COH_REST_URL and request-auth token refer to hive-b.
export HIVE_B_MANIFEST="${HIVE_B_MANIFEST:?set the hive-b resolved manifest path}"
test -f "$HIVE_B_MANIFEST"
cargo run -p host-ticket-agent -- \
  --manifest "$HIVE_B_MANIFEST" \
  --rest-url "$COH_REST_URL" \
  --cursor out/host-ticket-agent/hive-b-cursor.json

# Source host: the standard token is for hive-a; auth_ref is for hive-b.
export HIVE_A_MANIFEST="${HIVE_A_MANIFEST:?set the hive-a resolved manifest path}"
test -f "$HIVE_A_MANIFEST"
export COHESIX_RELAY_HIVE_B_TOKEN="${HIVE_B_REQUEST_AUTH_TOKEN:?set hive-b request authentication}"
cargo run -p host-ticket-agent -- \
  --manifest "$HIVE_A_MANIFEST" \
  --rest-url "$COH_REST_URL" \
  --cursor out/host-ticket-agent/hive-a-cursor.json \
  --relay \
  --relay-wal out/host-ticket-agent/hive-a-relay-wal.json
```

Queue a compact, read-only federated request on the source. `ssh` is an example
systemd unit; choose a real target and confirm the serialized request stays
within `/v1/meta/bounds`:

```bash
python3 - <<'PY'
import os

from cohesix import CohesixOrchestrator, HostTicketRequest, RestBackend

backend = RestBackend(
    os.environ["COH_REST_URL"],
    request_auth_token=os.environ.get("HIVE_GATEWAY_REQUEST_AUTH_TOKEN"),
)
orchestrator = CohesixOrchestrator(backend)
request = HostTicketRequest(
    ticket_id="f",
    idempotency_key="k",
    action="systemd.status-check",
    target="/systemd/ssh",
)
orchestrator.enqueue_federated_host_tickets(
    source_hive="hive-a",
    target_hive="hive-b",
    requests=[request],
)
PY
```

Verify the target `status` or `deadletter` entry contains the same `id`,
`idempotency_key`, `source_hive`, and `target_hive`, plus the relay hop and
correlation identifier. Repeating the same federated key is a dedupe test, not
a second execution request. Capture a source and target evidence pack when the
relay itself is under test; `timeline.ndjson` retains the correlation fields.

## Lifecycle maintenance window

Lifecycle control is stateful. `drain` is valid only after `cordon` has moved an
ONLINE or DEGRADED target to DRAINING, and `drain`, `quiesce`, and `reset` are
refused while leases remain active.

In a REST-backed `cohsh` session:

```text
cat /proc/lifecycle/state
cat /proc/lease/active
lifecycle cordon
cat /proc/lifecycle/state
```

After cordon, stop new work submissions and continuous host publishers. Wait
for every active lease to finish through its documented control path; do not
edit `/proc` output. Then enter the maintenance state:

```text
cat /proc/lease/active
lifecycle drain
cat /proc/lifecycle/state
```

The expected state is `QUIESCED`. Capture pre-maintenance evidence, perform the
host or node maintenance, and rerun the relevant health checks. Resume only
when the node is ready to admit work:

```text
lifecycle resume
cat /proc/lifecycle/state
cat /proc/root/reachable
```

`quiesce` is an explicit shortcut from ONLINE, DEGRADED, or DRAINING when there
are no active leases; use it only when bypassing the drain observation step is
intentional. `reset` changes the lifecycle state to BOOTING. It does not reboot
the platform; the authenticated `reboot` command is a separate operation with
its own backend and authorization requirements.

## PEFT adapter lifecycle

PEFT artifacts and model runtimes remain host-side. Cohesix exports bounded job
inputs, builds a content-hashed registry entry, and publishes model descriptors
and an active-model pointer. Neither activation nor rollback reloads an
inference process.

### Export and import

Select stable identifiers and export the Queen job inputs:

```bash
job_id="job_8932"
model_id="edge-adapter-v1"
export_root="$PWD/out/peft/export"
adapter_dir="$PWD/out/peft/trained/$model_id"
registry="$PWD/out/model_registry"

cargo run -p coh -- peft \
  --rest-url "$COH_REST_URL" \
  export --job "$job_id" --out "$export_root"
```

The export must contain exactly `telemetry.cbor`, `base_model.ref`, and
`policy.toml` under `$export_root/$job_id`. Train or obtain the adapter through
the approved host-side ML workflow. The import directory must contain
`adapter.safetensors` and `lora.json`; `metrics.json` is optional.

Import is local and refuses to replace an existing model identifier:

```bash
cargo run -p coh -- peft import \
  --model "$model_id" \
  --from "$adapter_dir" \
  --job "$job_id" \
  --export "$export_root" \
  --registry "$registry"

sed -n '1,160p' "$registry/available/$model_id/manifest.toml"
```

Review the recorded base-model reference, job provenance, file sizes, and
SHA-256 values before publication. Publish the refreshed GPU/model descriptor
snapshot only when the host's real inventory and registry are ready:

```bash
cargo run -p gpu-bridge-host -- \
  --registry "$registry" \
  --publish \
  --rest-url "$COH_REST_URL"
```

### Activate, verify, and roll back

Activation uses atomic replacement for each host registry file, updates
`active` and `active_state.toml`, and then appends the model identifier to
`/gpu/models/active`:

```bash
cargo run -p coh -- peft \
  --rest-url "$COH_REST_URL" \
  activate --model "$model_id" --registry "$registry"

curl --fail-with-body --silent --show-error --get \
  --data-urlencode 'path=/gpu/models/active' \
  --data-urlencode 'max_bytes=128' \
  "$COH_REST_URL/v1/fs/cat"
```

At this point Cohesix records intent and state only. Use the inference
application's own authenticated deployment/reload mechanism, then verify that
runtime independently; no Cohesix command in this recipe performs that reload.

The host file replacements and target append are not one distributed
transaction. If activation reports a transport error after changing the local
pointer, compare the two host files with `/gpu/models/active` before deciding
whether to reconcile or retry. Do not blindly repeat the command.

If validation fails, restore the previous pointer and again apply the
application-specific runtime procedure:

```bash
cargo run -p coh -- peft \
  --rest-url "$COH_REST_URL" \
  rollback --registry "$registry"
```

Rollback is available only when `active_state.toml` names a previous imported
model. Preserve command acknowledgements, the registry manifest, runtime
deployment evidence, and an evidence pack as separate records.

## Related documentation

- [OPERATOR_WALKTHROUGH.md](OPERATOR_WALKTHROUGH.md) — canonical live topology and ordered first run.
- [HOST_TOOLS.md](HOST_TOOLS.md) — tool ownership, modes, and authentication layers.
- [USERLAND_AND_CLI.md](USERLAND_AND_CLI.md) — console, `cohsh`, and `.coh` grammar.
- [PYTHON_SUPPORT.md](PYTHON_SUPPORT.md) — Python backends and typed API.
- [INTERFACES.md](INTERFACES.md) — authoritative paths and record schemas.
- [FAILURE_MODES.md](FAILURE_MODES.md) — evidence-led diagnosis and retry discipline.
