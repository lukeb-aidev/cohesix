<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define the as-built Cohesix REST projection, authentication, and compatibility rules. -->
<!-- Author: Lukas Bower -->
# Cohesix API Guidelines

`hive-gateway` is a host-only HTTP projection of the existing Cohesix console
and file operations. It does not add an in-target HTTP server, a new control
protocol, or per-request target identities.

The machine-readable contract is
[`resources/openapi/hive-gateway.yaml`](../resources/openapi/hive-gateway.yaml).
The gateway embeds that file and serves it at `/v1/openapi.yaml`. Detailed path
and payload semantics remain canonical in [INTERFACES.md](INTERFACES.md);
[HOST_API.md](HOST_API.md) is the narrative HTTP reference and must agree with
the embedded OpenAPI document.

See the [Glossary](GLOSSARY.md) for Cohesix-specific role, namespace, and
authority terms.

## Design contract

- Authority remains in the target. HTTP operations project `LS`, `CAT`, `TAIL`,
  and `ECHO`; metadata endpoints describe the gateway and active bounds.
- `hive-gateway` is the sole target TCP-console client in gateway mode. Multiple
  HTTP clients may use the gateway concurrently.
- The gateway attaches upstream with one configured role and optional
  capability ticket. Every HTTP request inherits that upstream authority.
- Request authentication at the HTTP edge is separate from TCP authentication,
  role attachment, capability tickets, policy approvals, and lifecycle gates.
- Paths, payloads, reads, queues, and retries remain bounded. Clients must not
  infer support for a namespace that the selected manifest does not expose.
- Successful and refused target operations preserve the console `OK`/`ERR`
  model. HTTP status alone is not enough to determine target success.

## Deployment topology

The default bind is loopback-only (`127.0.0.1:8080`). Non-mock startup requires
a non-placeholder TCP console token and a non-placeholder gateway request-auth
token. A non-loopback bind requires explicit opt-in and an external secure
transport boundary such as an authenticated tunnel, VPN, or TLS reverse proxy.

The gateway does not terminate TLS. Do not expose it directly to an untrusted
network.

For startup and end-to-end validation, use
[OPERATOR_WALKTHROUGH.md](OPERATOR_WALKTHROUGH.md). For tool composition, use
[HOST_TOOLS.md](HOST_TOOLS.md).

## Authentication model

### Upstream console authority

The gateway connects to the target using:

- TCP endpoint: `COH_TCP_HOST` and `COH_TCP_PORT`, or CLI equivalents;
- console authentication: `COH_AUTH_TOKEN` or `COHSH_AUTH_TOKEN`;
- role: `COH_ROLE` or `--role`;
- optional capability ticket: `COH_TICKET` or `--ticket`.

This attachment determines the namespace and operations available to every
REST caller. REST has no endpoint for supplying a different role or ticket per
request.

### HTTP request authentication

The gateway request-auth token resolves from `--request-auth-token`,
`HIVE_GATEWAY_REQUEST_AUTH_TOKEN`, `COH_REST_AUTH_TOKEN`, or
`COHSH_REST_AUTH_TOKEN`. `POST /v1/fs/echo` accepts either:

```http
Authorization: Bearer <token>
```

or:

```http
x-cohesix-auth: <token>
```

The as-built GET endpoints do not require this token. Keep the gateway on
loopback or place it behind an authenticated boundary because those reads may
expose operational state. A valid request-auth token only admits the HTTP
write; the target can still refuse the operation for role, ticket, policy,
lifecycle, bounds, or schema reasons.

## Endpoint reference

| Method | Endpoint | Projection | Request auth |
| --- | --- | --- | --- |
| `GET` | `/v1/meta/bounds` | Gateway-compiled manifest fingerprint, protocol bounds, paths, and feature metadata | No |
| `GET` | `/v1/meta/status` | Gateway connection, broker, queue, and cache status | No |
| `GET` | `/v1/fs/ls?path=...` | `LS` | No |
| `GET` | `/v1/fs/cat?path=...&max_bytes=...` | `CAT` | No |
| `GET` | `/v1/fs/tail?path=...&max_bytes=...` | `TAIL`; optional `lines=1..256` | No |
| `POST` | `/v1/fs/echo` | One bounded `ECHO` append | Yes |
| `GET` | `/v1/openapi.yaml` | Embedded OpenAPI 3.1 document | No |
| `GET` | `/docs` | Swagger UI loading the embedded document | No |

`/docs` loads Swagger UI assets from a public CDN. In an air-gapped deployment,
consume `/v1/openapi.yaml` directly or provide locally managed UI assets outside
the gateway.

### Read examples

```bash
curl --fail-with-body --silent --show-error \
  http://127.0.0.1:8080/v1/meta/bounds

curl --fail-with-body --silent --show-error --get \
  --data-urlencode 'path=/proc/lifecycle/state' \
  --data-urlencode 'max_bytes=128' \
  http://127.0.0.1:8080/v1/fs/cat
```

### Write example

The following queues one scheduler record and is appropriate only where that
mutation is intended:

```bash
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?request-auth token is required}"

curl --fail-with-body --silent --show-error \
  -X POST http://127.0.0.1:8080/v1/fs/echo \
  -H "Authorization: Bearer ${HIVE_GATEWAY_REQUEST_AUTH_TOKEN}" \
  -H 'Content-Type: application/json' \
  --data-binary '{"path":"/queen/schedule/ctl","line":"{\"id\":\"api-check-1\",\"role\":\"worker-gpu\",\"priority\":2,\"ticks\":3,\"budget_ms\":120}"}'
```

Control records are single-line JSON where the target interface requires JSONL.
The JSON schema, identifier rules, and queue bounds are defined in
[INTERFACES.md](INTERFACES.md), not by the HTTP wrapper.

## Response contract

Filesystem endpoints return a `GatewayResponse` object:

```json
{
  "status": "OK",
  "verb": "CAT",
  "path": "/proc/lifecycle/state",
  "end": true,
  "lines": ["state=ONLINE"],
  "bytes": 12
}
```

On failure, `status` is `ERR`, `end` remains `true`, `lines` is empty, and
`error` contains the bounded failure text. A target-level `ERR`, including a
policy refusal, or the exact in-process host-model semantic-capacity equivalent
is returned with HTTP `200` so the refusal is preserved. Clients must therefore
check both the HTTP status and the JSON `status` field.

After a successful `ECHO /queen/telemetry/<device_id>/ctl`, `lines` may contain
the single provider-assigned segment ID already carried by the target's
successful `ECHO` acknowledgement. Clients must validate that receipt as one
bounded path component and fall back to reading
`/queen/telemetry/<device_id>/latest` when it is absent or invalid. The fallback
is compatibility resolution, not permission to replay the segment-creation
write.

| HTTP status | Meaning |
| --- | --- |
| `200` | Gateway completed the request; inspect JSON `status` for `OK` or semantic `ERR`. |
| `400` | Invalid path, bound, query, or request payload. |
| `401` | Missing or invalid request-auth token on `POST /v1/fs/echo`. |
| `429` | Bounded gateway broker queue is under backpressure. |
| `503` | Upstream transport or gateway session is unavailable. |
| `504` | Bounded broker response deadline expired. |

## Broker and client deadline composition

The gateway has independent bounded control and telemetry broker response
deadlines. A filesystem client must leave room for all legal gateway phases:

```text
operation_response_ms =
    5000
    + max(control_response_ms, telemetry_response_ms)
    + 5000
```

The first `5,000 ms` is the bounded broker-queue admission limit and the final
`5,000 ms` is HTTP response-delivery grace. The canonical gateway deadlines are
`120,000/120,000 ms`, producing a canonical client filesystem-operation window
of `130,000 ms`. An externally owned gateway must declare both broker values;
clients and test harnesses must not infer them from reachability or metadata.
An explicit client value below the composition is invalid.

The shared Rust client keeps metadata, name resolution, connection
establishment, and response-body transfer on short independent bounds. Its
filesystem-operation HTTP agent applies the composed window to request send,
request-body send, and response-header receipt because the HTTP library carries
the earlier send deadlines into response receipt. Request and response byte
bounds remain unchanged. `cohsh` exposes the composed window through
`--rest-response-timeout-ms` and `COHSH_REST_RESPONSE_TIMEOUT_MS`, including
for pooled transports.

This is a client/gateway liveness contract, not a new HTTP or target protocol
field. A gateway-generated `504` still means the server's broker response
deadline expired; a local client expiry remains a transport error and does not
prove whether a write committed. Neither condition authorizes an automatic
write retry. Endpoint schemas, target wire grammar, ACK/ERR/END meaning,
request concurrency, pool size, and retry policy are unchanged.

## Bounds and namespace discovery

REST clients should read `/v1/meta/bounds` when they connect and retain the
returned `manifest_sha256` with any evidence they produce. The gateway builds
this response from its compiled generated policy; it does not query the target
for a live manifest. Use the returned limits for requests sent through that
gateway, and compare the fingerprint with `/proc/boot` or equivalent image
build evidence before claiming that both sides use the same manifest.

Worker declarations and namespace bounds use the explicit
`--worker-runtime-profile` selection: `qemu-smp-production` is the default;
physical Pi gateways require `pi4-production`. These existing generated
profiles declare six and eight shard bits respectively. This selection does
not make the top-level host policy fingerprint a live target attestation.

Do not treat a successful `GET /v1/meta/bounds` as target-manifest proof or as
proof that every optional path exists. List the relevant parent and check the
profile-specific path. The canonical worker namespace begins at `/shard`;
`/worker` is only a generated legacy alias when enabled.

## Retries and idempotency

- Read requests may retry bounded transient `429`, `503`, or `504` responses
  with backoff.
- Do not blindly retry an append after a connection loss or client-side timeout;
  verify the target's read-only status first because the caller may not have
  received the completion.
- A returned target `ERR` has no side effects unless the interface contract says
  otherwise. Retry only after correcting the reported cause.
- Control records should carry stable application identifiers where their
  schema provides one. Duplicate identifiers and consumed approvals are
  intentionally rejected.
- Respect `Retry-After` when present and keep retry attempts within the
  generated client policy.

See [FAILURE_MODES.md](FAILURE_MODES.md) for symptom-specific recovery.

## Compatibility rules

The `/v1` surface is a projection, so compatibility includes both HTTP and target
contracts:

1. Do not add HTTP operations that cannot be expressed by the documented
   console/file semantics.
2. Update the OpenAPI source, gateway tests, client tests, and narrative docs in
   the same change when an endpoint or response changes.
3. Treat console grammar, NineDoor error semantics, control JSONL, `/proc`
   formats, and generated bounds as upstream compatibility constraints.
4. Reject unknown control fields and out-of-bound data rather than coercing it.
5. Preserve `OK`, `ERR`, and `END` meaning across REST, CLI, Python, and UI
   clients.
6. Never represent HTTP request authentication as a capability ticket or
   delegated target identity.

## Client guidance

- Shell users: [USERLAND_AND_CLI.md](USERLAND_AND_CLI.md)
- Rust host tools: [HOST_TOOLS.md](HOST_TOOLS.md)
- Python: [PYTHON_SUPPORT.md](PYTHON_SUPPORT.md)
- Interface schemas: [INTERFACES.md](INTERFACES.md)
- Security model: [SECURITY.md](SECURITY.md)
