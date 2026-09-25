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
| `GET` | `/v1/meta/providers` | Full registry, including identity mappings; read scope `/proc/providers/registry` | Yes, plus read delegation |
| `GET` | `/v1/meta/status` | Connection, broker, queue, and cache state | Yes, plus read delegation |
| `GET` | `/v1/fs/ls?path=...` | `LS` | Visibility-dependent read delegation |
| `GET` | `/v1/fs/cat?path=...&max_bytes=...` | Bounded `CAT` | Visibility-dependent read delegation |
| `GET` | `/v1/fs/tail?path=...&max_bytes=...` | Bounded `TAIL`; optional `lines=1..256` | Visibility-dependent read delegation |
| `POST` | `/v1/identity/exchange` | [Mapped identity issuance](IDENTITY_MAPPING.md) | Yes; no existing ticket |
| `POST` | `/v1/fs/echo` | One bounded `ECHO` append | Yes, plus write delegation |
| `POST` | `/v1/fs/echo-batch` | Bounded ordered `ECHO` appends | Yes, plus write delegation |
| `POST` | `/v1/jobs` | Reserve and submit one exact selected host ticket | Yes, plus delegated `/host/tickets/spec` write |
| `GET` | `/v1/jobs/{admission_id}` | Read retained execution and result delivery | Yes, plus delegated `/host/tickets/status` read |
| `POST` | `/v1/jobs/{admission_id}/cancel` | Record a cancellation request; no termination claim | Yes, plus delegated `/host/tickets/spec` write |
| `POST` | `/v1/jobs/{admission_id}/reconcile` | Read the target result for the same identity | Yes, plus delegated `/host/tickets/status` read |
| `POST` | `/v1/standing/scopes/{scope_id}/inspect` | Inspect durable budget and revocation state | Yes, plus separate `/host/standing/admin` write |
| `POST` | `/v1/standing/scopes/{scope_id}/revoke` | Block new dispatch under the scope | Yes, plus separate `/host/standing/admin` write |
| `POST` | `/mcp` | Stateless MCP 2025-11-25 JSON-RPC over Streamable HTTP, when compiled on | Yes, plus delegated read/write for each selected operation |
| `GET` | `/.well-known/agent-card.json` | Subject-scoped A2A 0.3 Agent Card, when compiled on | Yes, plus delegated `/host/tickets/status` read |
| `POST` | `/a2a` | A2A 0.3 JSON-RPC task create, lookup, cancel and bounded SSE, when compiled on | Yes, plus delegated read/write for each selected operation |
| `GET` | `/v1/openapi.yaml` | Embedded OpenAPI 3.1 document | No |
| `GET` | `/docs` | Offline index linking the embedded OpenAPI and provider contract | No |

Both `CAT` and `TAIL` require `max_bytes`. Only `TAIL` accepts the optional
`lines` query. The gateway validates those bounds before contacting the target.

The selected manifest has versioned `[gateway.agent_protocols]`,
`[gateway.mcp]` and `[gateway.a2a]` enablement switches. Their schema defaults
are false; effective access requires both the master and protocol switch. The
selected QEMU profile enables MCP and A2A; the Pi profile keeps both disabled.
Each route is registered only when its effective switch is true. REST remains
available when either protocol is disabled, including authenticated reads used
to recover an existing job. Gateway launch variables and clients cannot raise
the compiled switches; an attempted protocol override is refused at startup.

The MCP endpoint accepts a single JSON-RPC request per POST with
`Accept: application/json, text/event-stream`, `Content-Type: application/json`
and the gateway request credential. After `initialize`, send
`MCP-Protocol-Version: 2025-11-25` on subsequent requests. A delegated
`x-cohesix-ticket` is required for selected discovery, preflight and job access.
The endpoint rejects invalid `Origin`, unsupported revisions, messages above
8,192 bytes and more than 16 concurrent requests; a tool call has a 30-second
gateway wait bound. `GET` and `DELETE` return 405 after authentication because
this stateless server has no SSE stream or server session to close. No client
credentials are forwarded to the target or native provider.

The compiler writes the selected tool, authority, lifecycle and evidence map
to [`mcp_catalogue.json`](../configs/generated/mcp_catalogue.json). The active
subject sees only tools backed by currently usable standing scopes. The
`cohesix.preflight_selected_job` tool observes a selected CUDA or PEFT request
and prepares a short-lived binding without an effect. Submit rechecks the facts
and standing budget. `cohesix.submit_selected_job` returns an admission or an
existing original identity, never a native completion certificate. Inspect and
recover use that original `admission_id`; cancellation requests do not prove
termination. The job resource `cohesix://jobs/{admission_id}` is subject-scoped
and carries bounded state, not model bytes. PEFT comparison, promotion and
rollback outcomes require the shared verifier and serving observation.

The generated [A2A catalogue](../configs/generated/a2a_catalogue.json)
selects JSON-RPC binding 0.3.0, compatible with `a2a-sdk==0.3.26` in the
installed NeMo Agent Toolkit 1.9.0 client. The Agent Card requires the gateway
request credential and delegated read ticket; it exposes only actions with a
currently usable scope for that subject. A peer sends one A2A `message/send`
or `message/stream` message with a single data part containing `skillId`, the
private selected `scopeId`, and the existing raw host ticket. The gateway
preflights fresh provider facts, then submits through the same durable REST
job path. A peer that already holds the exact preflight request may instead
send `skillId`, `binding`, and `ticket`; changed identity or facts refuse.
The original `ticket.id` is the A2A task ID and retained admission ID.

`tasks/get` reads that original job and native result from the target status
and deadletter records, even after a gateway restart. `tasks/cancel` records
a request under the existing cancellation
authority; pending cancellation stays pending. `tasks/resubscribe` and
`message/stream` emit bounded SSE snapshots for at most 30 seconds or 64
events; after expiry the peer calls `tasks/get` or resubscribes under the same
ID. Streams have a separate 16-consumer bound and do not own execution.
An unavailable target read leaves a retained task at `unknown` when its native
terminal cannot be confirmed. `confirmed` plus a matching native
`succeeded`/`failed`/`recovered_failure`/cancellation state maps to
`completed`/`failed`/`canceled` as appropriate. A task artifact contains only
the original result URI and SHA-256 reference. `providerVerified=false`
means the A2A task never substitutes for independent CUDA output or shared
PEFT release verification. An uncertain Root write returns a JSON-RPC error
with the original task ID and `effectReplayAllowed=false`; an unadmitted
reservation becomes `unknown` when its decision expires. A lost send response
is recovered by `tasks/get` with the known ticket ID; no replacement identity
is submitted. Push
notification methods are unsupported.

Selected jobs accept at most 4,096 JSON bytes containing one `binding` and one
raw `ticket`. The versioned binding fixes subject, action, target, input hash,
policy graph hash, current state and resource generations, deadline, ticket id,
idempotency key, admission id, attempt and one budget unit. Only
`gpu.workload.submit`, `peft.release` and `systemd.restart` are admitted by the selected
standing profile. The gateway observes the target lease/GPU publication or
native systemd unit before reservation. The agent checks the same binding and
current state at its native dispatch boundary. A successful `POST` returns
HTTP 202 for a target write ACK; an exact existing admission returns 200. An
uncertain write returns 409 with its original admission id. Status and
reconciliation never submit the effect again.

The `record.execution` values are `reserved`, `dispatching`, `uncertain`,
`confirmed` and `refused_no_effect`; `record.delivery` is independently
`pending` or `acknowledged`. Cancellation first records
`cancel_requested=true`. If native dispatch has already begun, the flag does
not prove cancellation; a separate authorized native control and terminal
observation are required. Reconciliation reports `effect_replay_allowed=false`
and returns target results with their exact line SHA-256 digests only when the
target read succeeds. A digest matching `record.result_sha256` binds retained
execution to that target line. Its HTTP 503
means that the target result is unavailable, not that no effect occurred.

## Worker Runtime Metadata

`GET /v1/meta/bounds` may include `worker_runtime`. Its absence means
`unknown`; clients must not reinterpret absence as `model-only`. The object is
declaration-only and has this additive shape:

The following example is the selected QEMU profile; Pi returns its separately
generated 256-Worker and eight-bit-shard values.

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

`GET /v1/meta/status` requires `target_host` and `target_port`, the normalized
configured TCP backend endpoint. They do not identify the REST bind address and
do not by themselves prove target execution. Qualified Pi evidence uses them
only as one exact cross-check with its same-boot serial, packet, runtime, and
live gateway-continuity proof. The response may add `backend_class` as
`host-model`, `console-projection`, or `unknown`; connectivity and backend
class likewise never prove target execution. The optional `worker_acceptance`
summary remains the only REST projection of QEMU or fresh-Pi component proof,
and it exists only after the gateway has validated a bounded local record with
the shared `cohesix-worker-evidence` parser.

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
The built-in `/docs` index loads no CDN assets; use `/v1/openapi.yaml` directly
in air-gapped environments.

The admin-only `GET /v1/meta/providers` returns the exact compiled
`cohesix-provider-registry/v1` contract and stable integration graph/source
hashes. This describes requirements, without asserting live provider state.
Non-public `GET /v1/meta/status` and `/v1/fs/{ls,cat,tail}` require request auth
and `x-cohesix-ticket` carrying an explicit Read/ReadWrite scope. The generated
read classification defaults to admin-only and is checked before caches.
Scope refusals return bounded ERR/END responses with HTTP 403. Explicit
single-caller `--read-compatibility` covers gateway-Queen admin reads only.


Mutations require gateway request auth plus the delegated
`x-cohesix-ticket` header. A request-auth token alone does not authorize a write.
Rust `GatewayClient::with_delegated_ticket` and Python
`RestBackend(..., delegated_ticket=...)` bind the caller; `COH_REST_TICKET` is
the explicit process-level default. Delegation is enforced by the gateway;
the single upstream console retains its configured identity. `/v1/meta/status`
includes bounded authority cache, refusal, writer-epoch and audit-emission
counters. See [M27a authority](M27A_AUTHORITY.md) and the
[OpenAPI contract](../resources/openapi/hive-gateway.yaml).

## Examples

For non-public reads, use a private operator-provisioned header file containing
`Authorization: Bearer TOKEN` and `x-cohesix-ticket: TICKET` with actual scoped
read credentials. Set `COH_READ_HEADERS` to its absolute path; the file keeps
secret values out of command arguments. A token in an environment variable alone
does not make curl send it. Do not enable compatibility mode to bypass delegation.

Read gateway status and a bounded target file:

```bash
: "${COH_READ_HEADERS:?set the private read-credential header file}"
curl --header "@$COH_READ_HEADERS" --fail-with-body --silent --show-error \
  http://127.0.0.1:8080/v1/meta/status

curl --header "@$COH_READ_HEADERS" --fail-with-body --silent --show-error --get \
  --data-urlencode 'path=/proc/schedule/queue' \
  --data-urlencode 'max_bytes=256' \
  http://127.0.0.1:8080/v1/fs/cat
```

Tail a bounded number of log lines:

```bash
curl --header "@$COH_READ_HEADERS" --fail-with-body --silent --show-error --get \
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
