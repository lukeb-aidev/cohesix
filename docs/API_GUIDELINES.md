<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Community-facing guidelines for Cohesix REST and Python APIs (Milestone 24c). -->
<!-- Author: Lukas Bower -->
# API Guidelines (Milestone 24c)

This document is the adoption guide for Cohesix APIs introduced in Milestone 24c. It explains what the APIs are, how they map to use cases, and how developers can validate **connectivity** to the APIs in real environments.

**At a glance**
- Host-only APIs that **project existing console and file semantics**.
- OpenAPI 3.1 spec lives in `docs/HOST_API.md` and is served at `/v1/openapi.yaml`.
- Python client is non-authoritative and supports TCP, filesystem, REST, and mock backends.
- Host tool usage, interdependencies, and mount details live in `docs/HOST_TOOLS.md`.

**Related operator docs**
- `docs/HOST_TOOLS.md` — host tool semantics, policy gates, and mounts.
- `docs/OPERATOR_WALKTHROUGH.md` — operator lifecycle and recovery flow.

## Principles (Non-Negotiable)
- **Authority lives in the VM**. The REST gateway is a stateless projection of `LS`, `CAT`, and `ECHO`.
- **Bounds are mandatory**. All clients must respect manifest-derived limits. Discover them via `/v1/meta/bounds`.
- **Single-line JSONL**. Control surfaces accept one JSON object per line.
- **No new semantics**. Clients may not introduce new verbs or change error behavior.
- **Host-only**. No in-VM HTTP servers or extra listeners.

## Compatibility and Scope (Milestone 24c)
- This document describes the **Milestone 24c** REST surface and its canonical control/observability paths.
- Namespaces are manifest-gated; missing paths usually indicate a disabled feature gate rather than a client bug.
- All control writes are append-only; retries must send a full JSON line, not partial fragments.

## Auth and Role Context
- The REST gateway attaches to the console using `COH_AUTH_TOKEN` and the role in `COH_ROLE` (typically `queen`).
- REST clients inherit the gateway role’s namespace. There is no per-request ticket in the REST surface.
- REST multiplexer deployments assume a queen-role gateway; worker-role attach remains console/9P-only.
- Policy gates still apply to REST writes; approvals must be queued in `/actions/queue` when required.

## API Surface Map
| Surface | Endpoint or Path | Semantics |
| --- | --- | --- |
| REST | `/v1/fs/ls` | `LS` projection |
| REST | `/v1/fs/cat` | `CAT` projection |
| REST | `/v1/fs/echo` | `ECHO` projection |
| REST | `/v1/meta/bounds` | Manifest-derived bounds |
| REST | `/v1/openapi.yaml` | OpenAPI 3.1 spec |
| REST | `/docs` | Swagger UI |

**Authoritative control files (JSONL, append-only)**
- `/queen/lease/ctl`
- `/queen/schedule/ctl`
- `/queen/export/ctl`
- `/policy/ctl`

**Read-only observability nodes**
- `/proc/schedule/summary`
- `/proc/schedule/queue`
- `/proc/lease/summary`
- `/proc/lease/active`
- `/proc/lease/preemptions`

Schemas and bounds are defined in `docs/INTERFACES.md`.

## Policy and Approval Flow (REST or Console)
Policy gating is manifest-enabled. When active, `/policy` and `/actions` appear and gated paths require approvals.

- `/policy/rules` lists gate targets derived from the manifest.
- `/actions/queue` is the approval/denial log (`id`, `target`, `decision`).
- If a write targets `/queen/ctl` (or other gated paths), the request returns
  `ERR ECHO reason=policy ... EPERM` until a matching approval exists.
- Approvals are consumed deterministically on success; `/policy/preflight/*` and `/proc/pressure/policy`
  provide observability into queued/consumed approvals and pressure.

## Use-Case Alignment (from `docs/USE_CASES.md`)
| Use case | API capability |
| --- | --- |
| Fleet policy GitOps boundary appliance | `/policy/ctl` apply and rollback |
| Vendor remote maintenance without VPN sprawl | lease windows and preemption via `/queen/lease/ctl` |
| GPU lease broker for multi-tenant edge | lease quotas and read-only `/proc/lease/*` |
| Edge data diode telemetry gateway | export windows via `/queen/export/ctl` |
| Kubernetes coexistence | declarative queue via `/queen/schedule/ctl` |
| Audit-first infrastructure | bounded `/proc` observability |

## Choose Your Transport
- **REST gateway**: simplest for HTTP clients and OpenAPI tooling. When using it as a multiplexer, run `hive-gateway` as the sole console client and point host tools at it with `--rest-url` (or `COH_REST_URL`).
- **TCP console**: direct `cohsh`-compatible console semantics.
- **Filesystem (Secure9P mount)**: file-shaped integration without HTTP.
- **Mock backend**: deterministic development and CI without a VM.

Notes:
- The TCP console is single-client. If a tool already holds it, other TCP clients will block or fail.
- The REST gateway itself uses the console, so do not run it concurrently with `cohsh` or `swarmui` in console mode. Use `SWARMUI_TRANSPORT=rest` instead.
- `coh mount --rest-url` is exclusive: only one REST mount per gateway URL.

## Golden Path (REST Adoption)
These steps both **adopt** the API and **validate connectivity**.

1. Boot the Queen VM and start the gateway.
```bash
./qemu/run.sh
COH_TCP_HOST=127.0.0.1 COH_TCP_PORT=31337 COH_AUTH_TOKEN=changeme \
  COH_ROLE=queen HIVE_GATEWAY_BIND=127.0.0.1:8080 \
  ./bin/hive-gateway
```
If you built from source, run `target/release/hive-gateway` (or `cargo run -p hive-gateway`) instead of `./bin/hive-gateway`.

2. Confirm the gateway is reachable and returns bounds.
```bash
curl -sS http://127.0.0.1:8080/v1/meta/bounds
```

3. Confirm the gateway can list the namespace.
```bash
curl -sS 'http://127.0.0.1:8080/v1/fs/ls?path=/'
```

4. Confirm a read-only `/proc` path exists in your build.
```bash
curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/lifecycle/state&max_bytes=128'
```

5. Optional write-path check (test VM only).
```bash
curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo \
  -H 'Content-Type: application/json' \
  -d '{"path":"/queen/schedule/ctl","line":"{\"id\":\"conn-check\",\"role\":\"worker-gpu\",\"priority\":1,\"ticks\":1,\"budget_ms\":10}"}'
```
Use this only in disposable environments, then confirm visibility via `/proc/schedule/queue`.

**Connection success signals**
- JSON response contains `status: "OK"` and `end: true`.
- `/v1/meta/bounds` includes `manifest_sha256`.
- `ls` returns a non-empty `lines` array (typically includes `proc`, `queen`, `worker`, `log`, `gpu`, `host`).

## OpenAPI and Swagger Guidance
The OpenAPI spec is a public contract. It must remain a strict projection of file semantics.

- **Source of truth**: `docs/HOST_API.md`.
- **Access points**: `/v1/openapi.yaml` and `/docs`.
- **No new verbs**: operations must map 1:1 to `LS`, `CAT`, and `ECHO`.
- **Bounds first**: clients must read `/v1/meta/bounds` and size requests accordingly.
- **Air-gapped use**: download the YAML spec instead of relying on Swagger UI CDN assets.

## Python API Support Guidelines
The Python client is a thin wrapper over existing semantics. It must remain non-authoritative.

- **Backends**: `TcpBackend`, `FilesystemBackend`, `RestBackend`, and `MockBackend`.
- **Bounds**: enforce manifest-derived limits from `docs/USERLAND_AND_CLI.md`.
- **No new semantics**: do not introduce verbs not supported by console or Secure9P.
- **Examples**: keep `tools/cohesix-py/examples/` aligned with new control grammar.
- **Docs**: update `docs/PYTHON_SUPPORT.md` when REST or backend behavior changes.

## Python Connection Checks (Expanded)
These checks validate connectivity only. They do not test API code.

**Install the client**
```bash
python3 -m pip install -e tools/cohesix-py
```

**REST backend connectivity**
```bash
python3 - <<'PY'
from cohesix.backends import RestBackend

backend = RestBackend("http://127.0.0.1:8080")
print(backend.list_dir("/"))
print(backend.read_file("/proc/lifecycle/state", 128).decode("utf-8").strip())
PY
```

**TCP console connectivity**
```bash
python3 - <<'PY'
from cohesix.backends import TcpBackend

backend = TcpBackend("127.0.0.1", 31337, "changeme", "queen", None)
print(backend.list_dir("/"))
PY
```

**Filesystem backend connectivity**
```bash
./bin/coh --host 127.0.0.1 --port 31337 mount --at /tmp/coh-mount
python3 - <<'PY'
from cohesix.backends import FilesystemBackend

backend = FilesystemBackend("/tmp/coh-mount")
print(backend.list_dir("/"))
PY
```

If `/proc/lifecycle/state` is unavailable, use a read-only path that exists in your build, such as `/proc/schedule/summary` or `/proc/lease/summary` for Milestone 24c.

## Troubleshooting
- `ERR AUTH` or `ERR ATTACH` means the auth token, role, or ticket does not match the Queen.
- Connection refused usually means the VM or gateway is not running.
- `ERR ECHO reason=policy ... EPERM` means policy gating requires an approval in `/actions/queue`.
- Gateway running but empty `/gpu` or `/host` paths means the host bridges are not publishing.
- `path exceeds max length` or `path component '..' is not permitted` means you violated manifest bounds.
- `read exceeds max bytes` means `max_bytes` is too large for the path or for its manifest limit.

## References
- `docs/BUILD_PLAN.md` — Milestone 24c scope and DoD.
- `docs/USE_CASES.md` — API-to-use-case alignment.
- `docs/INTERFACES.md` — canonical schemas and path definitions.
- `docs/HOST_API.md` — OpenAPI 3.1 spec and REST examples.
- `docs/PYTHON_SUPPORT.md` — Python client usage and backends.
