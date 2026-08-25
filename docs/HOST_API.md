<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Provide a concise operator reference for the Cohesix host-only REST gateway. -->
<!-- Author: Lukas Bower -->

# Cohesix Host REST API

`hive-gateway` is a host-only HTTP projection of existing Cohesix console and
file operations. It is not an in-target server and does not create a separate
authority path.

This page is an operator quick reference. It deliberately does not reproduce
the OpenAPI document.

## Canonical Sources

| Concern | Source |
| --- | --- |
| Machine-readable HTTP schema | [`resources/openapi/hive-gateway.yaml`](../resources/openapi/hive-gateway.yaml), also served at `/v1/openapi.yaml` |
| Authentication, compatibility, retry, and deployment rules | [API_GUIDELINES.md](API_GUIDELINES.md) |
| Target paths, payloads, and console semantics | [INTERFACES.md](INTERFACES.md) |
| Gateway startup and composition | [HOST_TOOLS.md](HOST_TOOLS.md) |

## Endpoint Summary

| Method | Endpoint | Operation | Request auth |
| --- | --- | --- | --- |
| `GET` | `/v1/meta/bounds` | Gateway-compiled bounds and manifest fingerprint | No |
| `GET` | `/v1/meta/status` | Connection, broker, queue, and cache state | No |
| `GET` | `/v1/fs/ls?path=...` | `LS` | No |
| `GET` | `/v1/fs/cat?path=...&max_bytes=...` | Bounded `CAT` | No |
| `GET` | `/v1/fs/tail?path=...&max_bytes=...` | Bounded `TAIL`; optional `lines=1..256` | No |
| `POST` | `/v1/fs/echo` | One bounded `ECHO` append | Yes |
| `GET` | `/v1/openapi.yaml` | Embedded OpenAPI 3.1 document | No |
| `GET` | `/docs` | Swagger UI backed by the embedded document | No |

Both `CAT` and `TAIL` require `max_bytes`. Only `TAIL` accepts the optional
`lines` query. The gateway validates those bounds before contacting the target.

## Worker Runtime Metadata

`GET /v1/meta/bounds` may include `worker_runtime`. Its absence means
`unknown`; clients must not reinterpret absence as `model-only`. The object is
declaration-only and has this additive shape:

The following example is the selected QEMU profile; Pi returns its separately
generated 64-Worker and eight-bit-shard values.

```json
{
  "roles": [
    {"role": "worker-heartbeat", "declaration": "executable", "executable_slots": 1}
  ],
  "task_abi_schema": "worker-task-abi/v2",
  "task_abi_version": 2,
  "worker_observation_schema": "cohesix-worker-observation/v1",
  "worker_integration_evidence_schema": "cohesix-worker-integration-evidence/v1",
  "maximum_live_tasks": 256,
  "canonical_telemetry_template": "/shard/<label>/worker/<id>/telemetry",
  "shard_bits": 6,
  "legacy_worker_alias": true
}
```

The role matrix and `maximum_live_tasks` are compiler bounds, not discovered
instances or READY state. Live lifecycle and receipts remain on canonical
`LS`, `CAT`, and `TAIL` projections below `/shard`; the legacy `/worker` alias
is usable only when the returned gate is true.

`GET /v1/meta/status` may add `backend_class` as `host-model`,
`console-projection`, or `unknown`. Connectivity and backend class never prove
target execution. The optional `worker_acceptance` summary is the only REST
projection of QEMU or fresh-Pi proof, and it exists only after the gateway has
validated a bounded local record with the shared `cohesix-worker-evidence`
parser.

Configure that import with an explicit trust root, the component record, and
the exact current target-session file:

```bash
hive-gateway \
  --worker-acceptance-root out/test-plan/m26e-worker-qemu \
  --worker-acceptance-evidence out/test-plan/m26e-worker-qemu/worker-task-evidence.json \
  --target-session out/test-plan/m26e-worker-qemu/target-session.json
```

Supply the normal bind, console, role, ticket, and request-auth options for the
selected deployment in the same invocation.

All three paths must be supplied together; the equivalent environment inputs
are `HIVE_GATEWAY_WORKER_ACCEPTANCE_ROOT`,
`HIVE_GATEWAY_WORKER_ACCEPTANCE_EVIDENCE`, and
`HIVE_GATEWAY_TARGET_SESSION`. The root must be a real directory, both files
must be canonical regular files below it, no traversed component may be a
symlink, and each input is capped at 256 KiB. The shared validator accepts only
a `target-component` record and requires its complete target session to equal
the supplied current-session bytes. The session's resolved-manifest hash must
also equal the manifest compiled into the gateway. A prior boot, a root-TCB or
full-system record, and a component copied beside a different session all fail
closed before load.

Status exposes hashes and bounded state only: component/session hashes,
target/proof class, topology hash, and each role's five-part identity, image
hash, READY/completion sequences, core, scheduling context, and object counts.
Those counts are the generated per-slot admission bundle associated with the
observed Worker identity; status does not describe them as a kernel allocation
or retype census.
Raw evidence, endpoint/fault badges, CPtrs, capability values, and secrets are
never returned. Missing or rejected input yields no proof and one typed
`worker_acceptance_diagnostic.code` such as `not-configured`,
`incomplete-configuration`, `outside-root`, `symlink-traversal`,
`record-too-large`, `invalid-target-session`, `target-session-mismatch`, or
`manifest-mismatch`.

This import is deliberately staged: a same-boot QEMU pre-pressure collector
may emit the component used to admit an executable workload, while the final
component/root/system collector runs only after the medium/high pressure
artifacts are immutable. The gateway must never require the final record that
its own pressure run is helping to produce.

## Authentication and Exposure

The gateway holds one upstream target-console session with a configured role
and optional capability ticket. All HTTP callers inherit that upstream
authority. The request-auth token on `POST /v1/fs/echo` authenticates the HTTP
write only; it is not a target identity or capability ticket.

The default bind is loopback. The gateway does not terminate TLS. Keep it on
loopback or place it behind an authenticated tunnel, VPN, or TLS reverse proxy.
The built-in Swagger UI loads public CDN assets; use `/v1/openapi.yaml` directly
in air-gapped environments.

## Examples

Read gateway status and a bounded target file:

```bash
curl --fail-with-body --silent --show-error \
  http://127.0.0.1:8080/v1/meta/status

curl --fail-with-body --silent --show-error --get \
  --data-urlencode 'path=/proc/schedule/queue' \
  --data-urlencode 'max_bytes=256' \
  http://127.0.0.1:8080/v1/fs/cat
```

Tail a bounded number of log lines:

```bash
curl --fail-with-body --silent --show-error --get \
  --data-urlencode 'path=/log/queen.log' \
  --data-urlencode 'max_bytes=512' \
  --data-urlencode 'lines=64' \
  http://127.0.0.1:8080/v1/fs/tail
```

For an intentional write, provide the request-auth token through the
environment rather than a command-line literal:

```bash
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?request-auth token is required}"

curl --fail-with-body --silent --show-error \
  -X POST http://127.0.0.1:8080/v1/fs/echo \
  -H "Authorization: Bearer ${HIVE_GATEWAY_REQUEST_AUTH_TOKEN}" \
  -H 'Content-Type: application/json' \
  --data-binary '{"path":"/queen/schedule/ctl","line":"{\"id\":\"api-check-1\",\"role\":\"worker-gpu\",\"priority\":2,\"ticks\":3,\"budget_ms\":120}"}'
```

After the Queen consumer accepts responsibility for the FIFO head, it removes
that pending record with a separately authenticated write:

```bash
curl --fail-with-body --silent --show-error \
  -X POST http://127.0.0.1:8080/v1/fs/echo \
  -H "Authorization: Bearer ${HIVE_GATEWAY_REQUEST_AUTH_TOKEN}" \
  -H 'Content-Type: application/json' \
  --data-binary '{"path":"/queen/schedule/ctl","line":"{\"op\":\"dequeue\",\"id\":\"api-check-1\"}"}'
```

The dequeue ID must match the exact queue head. It is consumer acceptance, not
Worker execution or completion evidence.

## Response Handling

Filesystem responses preserve target `OK` or `ERR` status and terminal `END`
semantics in JSON. A target refusal can therefore arrive with HTTP `200`.
Clients must inspect both the HTTP status and the JSON `status` field. Do not
blindly retry writes after an ambiguous transport failure; verify read-only
state first.

A successful control `ECHO` means the existing target path admitted the write.
It is not Worker READY, receipt confirmation, provider completion, target
acceptance, or execution proof. Discover those states independently through
canonical structured telemetry and validated acceptance evidence. The gateway
adds no Worker action endpoint or direct Worker RPC.

A successful queen telemetry segment-control `ECHO` may return the
provider-assigned segment ID as the sole `lines` entry. Validate it as one
bounded path component. When an older gateway omits the receipt, read the
device's `latest` file; do not replay an ambiguously completed creation write.

Use [FAILURE_MODES.md](FAILURE_MODES.md) for recovery and
[OPERATOR_WALKTHROUGH.md](OPERATOR_WALKTHROUGH.md) for an end-to-end validated
startup path.
