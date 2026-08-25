<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document the as-built Cohesix Python package, backends, and public client API. -->
<!-- Author: Lukas Bower -->
# Cohesix Python Support

The `cohesix` Python package is a host-side, non-authoritative client for the
existing Cohesix file and console contracts. It validates inputs, applies
generated bounds, and offers typed helpers; it does not add protocol verbs,
roles, paths, or authority.

The package source is under [`tools/cohesix-py/`](../tools/cohesix-py/). REST
semantics are defined in [API_GUIDELINES.md](API_GUIDELINES.md), control schemas
in [INTERFACES.md](INTERFACES.md), and live topology in
[HOST_TOOLS.md](HOST_TOOLS.md).

See the [Glossary](GLOSSARY.md) for Cohesix-specific role, namespace, and
evidence terms.

Python support consumes the compiler-owned
[`host-integration-dependency/v1`](../configs/generated/host_integration_dependency.json)
graph. Its `python-sdk-projection` row is release-required, while each external
systemd, Docker, Kubernetes, GPU, or PEFT provider remains an independent row.
The generated [support table](snippets/host_integration_dependency.md) is the
shared vocabulary: a Python object, successful dry run, mock probe, or package
install cannot create Worker READY, live-provider status, or use-case
acceptance.

## Requirements and installation

Python 3.11 or later is required. Use an isolated environment:

```bash
# Created by either canonical host installer:
# ./toolchain/setup_macos_arm64.sh
# ./toolchain/setup_linux_arm64.sh
source .venv/bin/activate
```

The installers populate `.venv` from the hash-locked host-test requirements and
install `tools/cohesix-py` in editable mode without optional integrations. To
create only that environment with an already installed Python 3.11 or later,
run `./toolchain/setup_repo_venv.sh --python python3`.

The editable path above is for a source checkout. From a release-bundle root,
the runtime setup creates `.venv` and installs the bundle's exact wheel without
resolving optional dependencies:

```bash
./scripts/setup_environment.sh
source .venv/bin/activate
```

Optional dependency groups are explicit:

```bash
# Docker, Kubernetes, and NVML probes
python3 -m pip install -e 'tools/cohesix-py[integrations]'

# PEFT and Transformers probes
python3 -m pip install -e 'tools/cohesix-py[ml]'

# Package tests
python3 -m pip install -e 'tools/cohesix-py[dev]'
```

The Python package version is defined in
[`tools/cohesix-py/pyproject.toml`](../tools/cohesix-py/pyproject.toml). It is
independent of a Cohesix release-bundle label.

## Backends

| Backend | Connection | Intended use | Authority and concurrency |
| --- | --- | --- | --- |
| `RestBackend` | `hive-gateway` HTTP API | Concurrent live clients | Inherits the gateway's upstream role/ticket; request auth protects writes |
| `TcpBackend` | Target TCP console | One direct live client | Performs `AUTH` and `ATTACH`; owns the single console connection |
| `FilesystemBackend` | Existing `coh mount` tree | File-oriented integrations | Inherits the authority of the process that created the mount |
| `MockBackend` | Local deterministic directory tree | Unit tests, examples, dry runs | Persistent at the selected root; processes using the same root can share it; never live-system evidence |

Use REST when Python must coexist with `cohsh`, SwarmUI, or continuous
publishers. Do not open a `TcpBackend` while another direct client owns the target
console.

### Explicit backend construction

Use explicit construction when application configuration already selected one
topology. These are the minimal constructors for all four public backends:

```python
import os

from cohesix import FilesystemBackend, MockBackend, RestBackend, TcpBackend

mock = MockBackend(root="out/examples/mockfs")
mounted = FilesystemBackend("/absolute/path/to/cohesix-mount")
rest = RestBackend(
    base_url=os.environ["COH_REST_URL"],
    request_auth_token=os.environ.get("HIVE_GATEWAY_REQUEST_AUTH_TOKEN"),
)
tcp = TcpBackend(
    host=os.environ.get("COH_TCP_HOST", "127.0.0.1"),
    port=int(os.environ.get("COH_TCP_PORT", "31337")),
    auth_token=os.environ["COH_AUTH_TOKEN"],
    role="queen",
    ticket=os.environ.get("COH_TICKET"),
)
try:
    print(tcp.list_dir("/"))
finally:
    tcp.close()
```

`FilesystemBackend` requires an already active `coh mount`; it neither creates
nor owns that mount. `MockBackend` seeds a deterministic local tree and may
reuse prior state at the selected root. When a `CohesixClient` binds a generated
profile, future mock Worker observations use that profile's exact shard width.
The REST and TCP constructors perform live operations and must follow the
ownership rules in
[HOST_TOOLS.md#choose-one-live-topology](HOST_TOOLS.md#choose-one-live-topology).

## Backend selection from the environment

`CohesixOrchestrator.from_env()` selects the first configured backend in this
order:

1. `COHESIX_MOCK=1`; optional root `COHESIX_MOCK_ROOT`.
2. `COHESIX_MOUNT_ROOT`.
3. `COH_REST_URL`, `HIVE_GATEWAY_URL`, or `COHESIX_REST_URL`.
4. Direct TCP using `COH_TCP_HOST`/`COHSH_TCP_HOST`,
   `COH_TCP_PORT`/`COHSH_TCP_PORT`, `COH_ROLE`/`COHSH_ROLE`, and optional
   `COH_TICKET`/`COHSH_TICKET`.

The common request timeout is `COHESIX_TIMEOUT_S`; direct TCP retry count is
`COHESIX_MAX_RETRIES`.

Example read-only inspection through a configured backend:

```python
from cohesix import CohesixOrchestrator

with CohesixOrchestrator.from_env() as cohesix:
    entries = cohesix.backend.list_dir("/")
    snapshot = cohesix.read_proc_snapshot()

print(entries)
print(snapshot.schedule_summary)
print(snapshot.lease_summary)
```

## Authentication

### REST

`RestBackend` accepts an explicit `request_auth_token`. Otherwise it resolves,
in order, `HIVE_GATEWAY_REQUEST_AUTH_TOKEN`, `COHSH_REST_AUTH_TOKEN`, then
`COH_REST_AUTH_TOKEN`. The backend sends both accepted gateway headers for
authenticated writes. Read endpoints do not currently require request auth,
but they still inherit the gateway's upstream target authority.

```python
import os

from cohesix import RestBackend

backend = RestBackend(
    base_url=os.environ["COH_REST_URL"],
    request_auth_token=os.environ.get("HIVE_GATEWAY_REQUEST_AUTH_TOKEN"),
)
print(backend.read_file("/proc/lifecycle/state", 128).decode("utf-8"))
```

### Direct TCP

`TcpBackend` requires a console auth token, role, and optional ticket:

```python
import os

from cohesix import TcpBackend

backend = TcpBackend(
    host=os.environ.get("COH_TCP_HOST", "127.0.0.1"),
    port=int(os.environ.get("COH_TCP_PORT", "31337")),
    auth_token=os.environ["COH_AUTH_TOKEN"],
    role="queen",
    ticket=os.environ.get("COH_TICKET"),
)
try:
    print(backend.list_dir("/"))
finally:
    backend.close()
```

For checkout-based development, `resolve_tcp_auth_token()` can read a Queen
ticket secret from an explicitly selected or default source manifest before it
falls back to `COH_AUTH_TOKEN` and `COHSH_AUTH_TOKEN`. This is a local
configuration convenience, not delegated identity. Production callers should
pass the intended secret explicitly through their secret-management boundary so
manifest discovery cannot select an unexpected credential. Missing, empty, and
placeholder tokens are rejected.

Never log tokens, embed them in source, or store populated credential files in
the repository.

## Public API

The package exports these primary surfaces from `cohesix`:

| Surface | Purpose |
| --- | --- |
| `CohesixClient` | GPU discovery and leases, telemetry push/pull, host-command receipts, evidence packs, and PEFT lifecycle helpers |
| `CohesixOrchestrator` | Typed approvals, scheduler records, leases, exports, host tickets, and `/proc` snapshots |
| `ControlPlan` | Declarative collection of approval, schedule, lease, and export writes |
| `ApprovalRequest` | Validated `/actions/queue` record |
| `ScheduleRequest`, `ScheduleDequeue` | Validated producer and exact FIFO-consumer `/queen/schedule/ctl` records |
| `LeaseRequest` | Validated `/queen/lease/ctl` record |
| `ExportRequest` | Validated `/queen/export/ctl` record |
| `HostTicketRequest`, `K8sRbacIntent` | Manifest-bounded host ticket records and Kubernetes intent conversion |
| `CohesixAudit` | Bounded client-side acknowledgement and breadcrumb collection |
| `TargetProfileContract`, `load_profile_contract` | Strict loader for one compiler-generated QEMU or Pi Python profile contract |
| `WorkerClient` and `CohesixClient.worker_*` | Existing `/queen/ctl` and canonical `/shard` Heartbeat, GPU, and LoRA lifecycle projection |
| `WorkerReceipt`, `CompatibilityReceipt`, `parse_receipt` | Version-1 compatibility and local-admitted version-2-correlated Worker receipt projection |
| `WorkerAcceptanceAxes` | Keeps admission, READY, provider, receipt, artifact, proof, release, and use-case state separate |
| `CohesixError` | Package error type for validation, transport, and server refusals |

## Target contracts and Worker API

The wheel is target-neutral. Its generated `DEFAULTS` object contains bounded
fallback expectations, `manifest_sha256=None`, and `execution_proof="none"`.
It cannot identify a running target. Live Worker calls require one explicit,
regular, non-symlink compiler output:

- `configs/generated/cohesix_python_qemu_smp_production.json` for
  `qemu_smp_production`;
- `configs/generated/cohesix_python_pi4_production.json` for
  `pi4_production`.

Both use `cohesix-python-profile/v1`, but each binds its own resolved-manifest
hash. They are generated independently; one target contract must never be
copied, renamed, or inferred from the other. A parsed in-memory mapping is
useful for validation tests but is marked `source="mapping"` and does not
establish target identity.

```python
from cohesix import CohesixClient, MockBackend, load_profile_contract

profile = load_profile_contract(
    "configs/generated/cohesix_python_qemu_smp_production.json",
    expected_target="qemu",
)
client = CohesixClient(
    MockBackend("out/examples/worker-model"),
    profile_contract=profile,
)

admission = client.worker_spawn("heartbeat", "heartbeat-1")
assert admission.lifecycle == "queued"  # the control write was admitted
ready = client.worker_wait_ready("heartbeat", "heartbeat-1")
assert ready.state.lifecycle == "ready"  # separately observed telemetry
client.worker_teardown("heartbeat", "heartbeat-1")
```

The mock in this example reports `execution_proof="host-model"`. It exercises
the API but is never QEMU or Pi proof. The three executable roles are
`worker-heartbeat`, `worker-gpu`, and `worker-lora`; their combined generated
maximum is 256 simultaneous tasks for QEMU (1 Heartbeat, 127 GPU, and 128 LoRA)
and 64 for Pi 4 (1 Heartbeat, 31 GPU, and 32 LoRA). `worker-bus` remains
model-only, and spawn or teardown returns a deterministic `CohesixError` before
any backend write.

Worker telemetry uses
`/shard/<label>/worker/<id>/telemetry`, where the selected QEMU contract keeps
the top six digest bits and the Pi contract keeps all eight. The legacy
`/worker/<id>/telemetry` path is returned only when the selected contract
enables `legacy_worker_alias`. No client should infer a role from an
instance-id prefix.

### Lifecycle, receipt, and proof boundaries

The API preserves these independent axes:

| Axis | Python meaning | Does not establish |
| --- | --- | --- |
| request admission | `/queen/ctl` append completed | Worker READY |
| lifecycle | newest bounded Worker observation | provider completion or target proof |
| provider completion | host provider reported terminal work | Worker receipt or artifact verification |
| receipt | confirmed, rejected, or stale generation-correlated projection | Python authority or execution proof |
| artifact | missing, verified, or mismatch | runtime release acceptance |
| execution proof | none, host-model, or a reference to accepted QEMU/fresh-Pi evidence | production use-case acceptance |
| Python projection compatibility | both shipped interpreters passed against the wheel/contract | target, provider, or runtime acceptance |
| runtime release / production use case | later evidence-graph promotions | inferred success from any Python object |

`cohesix-receipt-v1` remains a non-authoritative compatibility wrapper.
Receipt-bearing host-ticket work uses the accepted
`host-ticket/v2`/`host-ticket-result/v2` pair and the bounded
`worker-gpu-receipt/v1` or `worker-lora-receipt/v1` telemetry encoding. Python
requires the caller to classify such bytes as `source="local-admitted"`;
remote or unclassified version-2 data is rejected. Parsing still sets
`authoritative=False`: only root admission and matching target evidence can
establish authority outside the Python object.

The exact receipt actions are:

- GPU: `gpu.lease.grant`, `gpu.lease.renew`, `gpu.lease.release`;
- PEFT: `peft.export`, `peft.import`, `peft.activate`, `peft.rollback`.

The receipt identity is the full role, slot, lease epoch, supervisor
generation, and capability generation. A mismatch with the expected identity
or public instance is `stale`; it is never rebound to a newer Worker.

### Backend compatibility

All four backends use the same Worker payload validation and canonical paths.
Mock returns `host-model`; direct TCP reports `console-projection` as a backend
class but not target proof; filesystem is `unknown` unless separately bound;
REST treats absent optional `worker_runtime_bounds` or `backend_class` metadata
as `None`/`unknown`. Metadata is declaration-only. A connected gateway, mount,
or console is not READY and cannot create QEMU or Pi proof.

### Typed control example

The following performs a real scheduler write. Use it only where the mutation
is intended and where the gateway's upstream authority permits it:

```python
from cohesix import CohesixOrchestrator, ScheduleRequest

request = ScheduleRequest(
    request_id="python-schedule-1",
    role="worker-gpu",
    priority=2,
    ticks=3,
    budget_ms=120,
)

with CohesixOrchestrator.from_env() as cohesix:
    results = cohesix.enqueue_schedule([request])

for result in results:
    print(result.path, result.bytes_written)
```

If the target is policy-gated, queue an `ApprovalRequest` for the exact target
before the control record. Approvals do not broaden the gateway role or ticket
and are consumed according to the target policy contract.

### Typed Kubernetes intent example

`K8sRbacIntent` is a less obvious typed surface: it validates a Kubernetes
coexistence request, converts it to an allowlisted `host-ticket/v1` record, and
appends that record to `/host/tickets/spec`. It does not call Kubernetes
directly. The write below is real; run it only after reviewing the active host
ticket policy and intended node:

```python
from cohesix import CohesixOrchestrator, K8sRbacIntent

intent = K8sRbacIntent(
    intent_id="maint-node-1",
    subject="ops-user",
    namespace="edge-a",
    node="node-1",
    verb="cordon",
    reason="planned-maintenance",
    ttl_s=900,
)

with CohesixOrchestrator.from_env() as cohesix:
    results = cohesix.enqueue_k8s_rbac_tickets([intent])

for result in results:
    print(result.path, result.bytes_written)
```

Execution and lifecycle receipts belong to `host-ticket-agent`; request and
result schemas are owned by
[INTERFACES.md#host-tickets-and-federation](INTERFACES.md#host-tickets-and-federation).

## Bounds and generated defaults

The package imports generated client policy from
[`cohesix/generated.py`](../tools/cohesix-py/cohesix/generated.py). That file is
a `coh-rtc` output and must not be edited by hand. The default-profile summary
is in [snippets/cohesix_py_defaults.md](snippets/cohesix_py_defaults.md).

Generated defaults provide target-neutral offline fallback bounds. Worker APIs
instead require the explicit target-qualified contract described above. A REST
client can read the gateway's `/v1/meta/bounds`; evidence should retain its
`manifest_sha256`. That response describes the gateway's compiled generated
policy, not a manifest queried from the target. Optional Worker runtime bounds
are declarations only. Match all identities to `/proc/boot` and accepted target
evidence before claiming gateway-target manifest parity.

The package validates absolute paths, rejects `..`, enforces component and
payload limits, requires UTF-8 single-line writes, and preserves strict control
schemas. Client validation improves diagnostics but never replaces target-side
validation.

## REST retry behavior

`RestBackend` uses generated retry defaults unless overridden with
`max_attempts`, `backoff_ms`, and `backoff_ceiling_ms`. It retries HTTP `429`,
`500`, `502`, `503`, and `504`, plus selected transient URL failures.

For mutations, a transport loss can leave completion ambiguous. Verify the
read-only target state before manually retrying an append. Stable request IDs
help the target reject duplicates but do not make every operation idempotent. See
[FAILURE_MODES.md](FAILURE_MODES.md).

## Playbooks and integrations

`cohesix-playbook` executes checked-in, bounded scenario templates. The
templates are coverage and workflow assets; their worker counts are not claims
of live fleet capacity or benchmark evidence.

```bash
cohesix-playbook --list
cohesix-playbook --playbook mixed-closed-loop-ai-factory --dry-run --mock
```

Remove `--dry-run --mock` only after selecting a live backend, reviewing the
generated plan, and confirming the intended authority and side effects.

Optional probes cover systemd, Docker, Kubernetes, NVML, and PEFT package
state. They run on the host, validate and bound collected data, and do not grant
control authority. Evidence and receipt helpers write deterministic local
artifacts without creating a new target evidence channel.

Examples live in [`tools/cohesix-py/examples/`](../tools/cohesix-py/examples/).
Treat mock examples as functional demonstrations, not live acceptance proof.

## Testing

Run the package tests from the repository root:

```bash
python3 -m pytest tools/cohesix-py/tests
python3 -m pytest -k cohesix_parity tools/cohesix-py/tests/test_parity.py
python3 -m pip wheel --no-deps --wheel-dir out/python-wheels tools/cohesix-py
scripts/ci/python_compat_run.sh \
  --wheel-smoke \
  --wheel-dir out/python-wheels \
  --package-manifest out/python-compat/m26e-python-package.json \
  --state-dir out/python-compat/m26e-wheel
```

The wheel gate requires both CPython 3.11 and 3.13, installs the same wheel
without dependencies into isolated environments, checks the public entry
point, verifies the declared `integrations`, `ml`, and `dev` extras, and emits
`cohesix-python-package/v1`. That manifest binds the wheel hash and both target
contract hashes. `dev` remains test-only. Missing optional providers return a
typed skipped/degraded probe and never select `MockBackend` implicitly.

After a direct target runner has emitted an accepted `worker-control`,
`gpu-receipt-path`, or `peft-receipt-path` record, the matrix mode consumes that
record by reference and emits `python-sdk-projection.json`:

```bash
scripts/ci/python_compat_run.sh \
  --python-matrix 3.11,3.13 \
  --target qemu \
  --profile-contract configs/generated/cohesix_python_qemu_smp_production.json \
  --package-manifest out/python-compat/m26e-python-package.json \
  --wheel-dir out/python-wheels \
  --matrix configs/host_integration_acceptance.toml \
  --target-session out/host-integration/m26e-qemu/integration/worker-control.json \
  --state-dir out/python-compat/m26e-qemu
```

The result is release-required projection evidence. It references the direct
target session, exact wheel, graph, source matrix, manifest, contract, host, and
interpreter hashes; it does not replace target evidence or promote any external
CUDA/NVML, PEFT, FUSE, systemd, Docker, Kubernetes, federation, or use-case row.

Changes to a backend or public helper require tests for successful use,
validation failure, server refusal, and relevant retry/authentication behavior.
Changes to generated defaults must be made in manifest/IR inputs and refreshed
with the repository generation workflow.

## Related documentation

- [API_GUIDELINES.md](API_GUIDELINES.md) — REST contract and status handling.
- [HOST_TOOLS.md](HOST_TOOLS.md) — live topology and host-tool ownership.
- [INTERFACES.md](INTERFACES.md) — control and observability schemas.
- [OPERATOR_WALKTHROUGH.md](OPERATOR_WALKTHROUGH.md) — canonical live setup.
- [OPERATOR_RECIPES.md](OPERATOR_RECIPES.md) — evidence, mount, ticket, lifecycle, and PEFT procedures.
