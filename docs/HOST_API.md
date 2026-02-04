<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document the Cohesix host-only REST gateway API. -->
<!-- Author: Lukas Bower -->
# Cohesix Host REST API (Hive Gateway)

The **hive-gateway** is a host-only REST projection of Cohesix console/file semantics. It **does not introduce new control-plane behavior**. Every request is translated into existing `LS`, `CAT`, and `ECHO` flows with manifest-derived bounds enforced at the gateway and again inside the VM.

**Key properties**
- **Authority lives in the VM**: the gateway is a stateless proxy for `LS`/`CAT`/`ECHO` semantics.
- **Bounded**: payload sizes and `/proc` read limits are validated against manifest-derived limits.
- **No per-request auth**: the gateway attaches once using the configured role/ticket; protect the HTTP endpoint accordingly.

## Auth + role configuration
The gateway uses the fixed role/ticket configured in its environment (or CLI flags). Set these in `/etc/cohesix/hive-gateway.env` when running under systemd:
```
COH_TCP_HOST=127.0.0.1
COH_TCP_PORT=31337
COH_AUTH_TOKEN=changeme
COH_ROLE=queen
COH_TICKET=
HIVE_GATEWAY_BIND=127.0.0.1:8080
```

## Examples
**1) Inspect manifest bounds**
```
curl -sS http://127.0.0.1:8080/v1/meta/bounds | jq .
```

**2) Append a schedule entry**
```
curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo \
  -H 'Content-Type: application/json' \
  -d '{"path":"/queen/schedule/ctl","line":"{\"id\":\"job-1\",\"role\":\"worker-gpu\",\"priority\":2,\"ticks\":3,\"budget_ms\":120}"}'
```

**3) Read schedule queue**
```
curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/schedule/queue&max_bytes=256'
```

**4) Apply policy revision**
```
curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo \
  -H 'Content-Type: application/json' \
  -d '{"path":"/policy/ctl","line":"{\"op\":\"apply\",\"id\":\"rev-22\",\"sha256\":\"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"}"}'
```

## Swagger UI
The gateway serves Swagger UI at:
```
http://127.0.0.1:8080/docs
```
The UI loads assets from the public `swagger-ui` CDN; for air-gapped environments, use the OpenAPI spec directly.

## OpenAPI 3.1 spec
```yaml
openapi: 3.1.0
info:
  title: Cohesix Hive Gateway
  version: 0.1.0
  description: |-
    Host-only REST gateway that projects Cohesix console/file semantics into a
    JSON API. Responses mirror OK/ERR/END semantics and enforce manifest bounds.
servers:
  - url: http://127.0.0.1:8080
paths:
  /v1/meta/bounds:
    get:
      summary: Manifest-derived bounds and paths.
      responses:
        "200":
          description: Boundaries derived from the active manifest.
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/BoundsResponse"
  /v1/fs/ls:
    get:
      summary: List directory entries (LS).
      parameters:
        - in: query
          name: path
          required: true
          schema:
            type: string
      responses:
        "200":
          description: List operation response.
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/GatewayResponse"
        "400":
          description: Invalid request.
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/GatewayResponse"
  /v1/fs/cat:
    get:
      summary: Read file contents (CAT).
      parameters:
        - in: query
          name: path
          required: true
          schema:
            type: string
        - in: query
          name: max_bytes
          required: true
          schema:
            type: integer
            minimum: 1
      responses:
        "200":
          description: Read operation response.
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/GatewayResponse"
        "400":
          description: Invalid request.
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/GatewayResponse"
  /v1/fs/echo:
    post:
      summary: Append a single line (ECHO).
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: "#/components/schemas/EchoRequest"
      responses:
        "200":
          description: Echo operation response.
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/GatewayResponse"
        "400":
          description: Invalid request.
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/GatewayResponse"
  /v1/openapi.yaml:
    get:
      summary: OpenAPI 3.1 specification.
      responses:
        "200":
          description: OpenAPI specification (YAML).
          content:
            application/yaml:
              schema:
                type: string
  /docs:
    get:
      summary: Swagger UI.
      responses:
        "200":
          description: Swagger UI HTML.
          content:
            text/html:
              schema:
                type: string
components:
  schemas:
    GatewayResponse:
      type: object
      required:
        - status
        - verb
        - path
        - end
      properties:
        status:
          type: string
          enum: ["OK", "ERR"]
        verb:
          type: string
        path:
          type: string
        end:
          type: boolean
        lines:
          type: array
          items:
            type: string
        bytes:
          type: integer
          minimum: 0
        error:
          type: string
    EchoRequest:
      type: object
      required:
        - path
      properties:
        path:
          type: string
        line:
          type: string
    BoundsResponse:
      type: object
      required:
        - manifest_sha256
        - secure9p
        - console
        - paths
        - control_plane
        - policy
        - observability
      properties:
        manifest_sha256:
          type: string
        secure9p:
          type: object
          required: [msize, walk_depth]
          properties:
            msize:
              type: integer
            walk_depth:
              type: integer
        console:
          type: object
          required:
            - max_line_len
            - max_path_len
            - max_json_len
            - max_id_len
            - max_echo_len
            - max_ticket_len
          properties:
            max_line_len:
              type: integer
            max_path_len:
              type: integer
            max_json_len:
              type: integer
            max_id_len:
              type: integer
            max_echo_len:
              type: integer
            max_ticket_len:
              type: integer
        paths:
          type: object
          required:
            - queen_ctl
            - queen_lifecycle_ctl
            - queen_schedule_ctl
            - queen_lease_ctl
            - queen_export_ctl
            - policy_ctl
            - log
          properties:
            queen_ctl:
              type: string
            queen_lifecycle_ctl:
              type: string
            queen_schedule_ctl:
              type: string
            queen_lease_ctl:
              type: string
            queen_export_ctl:
              type: string
            policy_ctl:
              type: string
            log:
              type: string
        control_plane:
          type: object
          required: [schedule, lease, export]
          properties:
            schedule:
              type: object
              required: [enable, queue_max_entries, ctl_max_bytes]
              properties:
                enable:
                  type: boolean
                queue_max_entries:
                  type: integer
                ctl_max_bytes:
                  type: integer
            lease:
              type: object
              required: [enable, active_max_entries, preemptions_max_entries, ctl_max_bytes]
              properties:
                enable:
                  type: boolean
                active_max_entries:
                  type: integer
                preemptions_max_entries:
                  type: integer
                ctl_max_bytes:
                  type: integer
            export:
              type: object
              required: [enable, ctl_max_bytes]
              properties:
                enable:
                  type: boolean
                ctl_max_bytes:
                  type: integer
        policy:
          type: object
          required: [enable, queue_max_entries, queue_max_bytes, ctl_max_bytes]
          properties:
            enable:
              type: boolean
            queue_max_entries:
              type: integer
            queue_max_bytes:
              type: integer
            ctl_max_bytes:
              type: integer
        observability:
          type: object
          required: [proc_schedule, proc_lease]
          properties:
            proc_schedule:
              type: object
              required: [summary, queue, summary_bytes, queue_bytes]
              properties:
                summary:
                  type: boolean
                queue:
                  type: boolean
                summary_bytes:
                  type: integer
                queue_bytes:
                  type: integer
            proc_lease:
              type: object
              required: [summary, active, preemptions, summary_bytes, active_bytes, preemptions_bytes]
              properties:
                summary:
                  type: boolean
                active:
                  type: boolean
                preemptions:
                  type: boolean
                summary_bytes:
                  type: integer
                active_bytes:
                  type: integer
                preemptions_bytes:
                  type: integer
```
