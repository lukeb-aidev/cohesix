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

## Response Handling

Filesystem responses preserve target `OK` or `ERR` status and terminal `END`
semantics in JSON. A target refusal can therefore arrive with HTTP `200`.
Clients must inspect both the HTTP status and the JSON `status` field. Do not
blindly retry writes after an ambiguous transport failure; verify read-only
state first.

A successful queen telemetry segment-control `ECHO` may return the
provider-assigned segment ID as the sole `lines` entry. Validate it as one
bounded path component. When an older gateway omits the receipt, read the
device's `latest` file; do not replay an ambiguously completed creation write.

Use [FAILURE_MODES.md](FAILURE_MODES.md) for recovery and
[OPERATOR_WALKTHROUGH.md](OPERATOR_WALKTHROUGH.md) for an end-to-end validated
startup path.
