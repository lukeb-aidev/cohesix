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

## Requirements and installation

Python 3.11 or later is required. Use an isolated environment:

```bash
python3 -m venv .venv
source .venv/bin/activate
python3 -m pip install --upgrade pip
python3 -m pip install -e tools/cohesix-py
```

The editable path above is for a source checkout. From a release-bundle root,
install the bundled, non-editable package instead:

```bash
python3 -m pip install ./python/cohesix-py
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
reuse prior state at the selected root. The REST and TCP constructors perform
live operations and must follow the ownership rules in
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
| `ScheduleRequest` | Validated `/queen/schedule/ctl` record |
| `LeaseRequest` | Validated `/queen/lease/ctl` record |
| `ExportRequest` | Validated `/queen/export/ctl` record |
| `HostTicketRequest`, `K8sRbacIntent` | Manifest-bounded host ticket records and Kubernetes intent conversion |
| `CohesixAudit` | Bounded client-side acknowledgement and breadcrumb collection |
| `CohesixError` | Package error type for validation, transport, and server refusals |

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

Generated defaults provide offline and non-REST bounds. A REST client can read
the gateway's `/v1/meta/bounds`; evidence should retain its `manifest_sha256`.
That response describes the gateway's compiled generated policy, not a manifest
queried from the target. Match it to `/proc/boot` or equivalent image build
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
```

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
