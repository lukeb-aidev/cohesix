<!-- Author: Lukas Bower -->
<!-- Purpose: Canonical interface definitions for NineDoor, queen/worker verbs, GPU bridge files, and telemetry schemas. -->
# Cohesix Interfaces (Queen/Worker, NineDoor, GPU Bridge)

The queen/worker verbs and `/queen/ctl` schema form the hive control API: one Queen instance uses these interfaces to control many workers over the shared Secure9P namespace.

**Figure 1.** Sequence diagram.
```mermaid
%%{
  init: {
    "theme": "base",
    "themeVariables": {
      "background": "#ffffff",
      "primaryColor": "#ffffff",
      "secondaryColor": "#ffffff",
      "tertiaryColor": "#ffffff",
      "lineColor": "#333333",
      "textColor": "#111111"
    }
  }
}%%
sequenceDiagram
  autonumber
  actor Operator
  participant Cohsh as cohsh (client)
  participant Console as root-task TCP console (:31337)
  participant ND as NineDoor (Secure9P)
  participant RT as root-task (RootTaskControl)
  participant WT as /worker/{id}/telemetry
  participant LOG as /log/queen.log
  participant GPUB as gpu-bridge-host (provider)
  participant GPUFS as /gpu/{id}/*

  Note over ND: 9P2000.L only; remove disabled; msize max 8192; append-only enforced; path UTF-8/no NUL; fid per-session.

  %% ---------------------------------------------------------
  %% A) TCP console: ATTACH + keepalive + TAIL
  %% ---------------------------------------------------------
  Operator->>Cohsh: run cohsh --transport tcp ...
  Cohsh->>Console: ATTACH role=<role> ticket=<ticket?>
  alt valid ticket and role
    Console-->>Cohsh: OK ATTACH role=<role>
  else invalid or locked out
    Console-->>Cohsh: ERR ATTACH reason=<cause>
  end

  par idle keepalive
    Cohsh->>Console: PING
    Console-->>Cohsh: PONG
  end

  Cohsh->>Console: TAIL path=<path>
  Console-->>Cohsh: OK TAIL path=<path>
  loop stream entries
    Console-->>Cohsh: <entry line>
  end
  Console-->>Cohsh: END

  Note over Console: Console is authenticated; line length max 128 bytes; auth failures rate-limited; ACK is emitted before side effects.

  %% ---------------------------------------------------------
  %% B) Secure9P session (preferred machine interface)
  %% ---------------------------------------------------------
  Operator->>Cohsh: run cohsh (9P mode)
  Cohsh->>ND: TVERSION msize=8192
  ND-->>Cohsh: RVERSION msize=8192
  Cohsh->>ND: TATTACH (ticket out-of-band)
  alt ticket ok
    ND-->>Cohsh: RATTACH
  else bad ticket
    ND-->>Cohsh: Rerror(Permission or Closed)
  end

  %% ---------------------------------------------------------
  %% C) Queen control via /queen/ctl (append-only JSON lines)
  %% ---------------------------------------------------------
  Cohsh->>ND: TWALK /queen/ctl
  ND-->>Cohsh: RWALK
  Cohsh->>ND: TOPEN /queen/ctl (append)
  ND-->>Cohsh: ROPEN
  Cohsh->>ND: TWRITE "JSON spawn=heartbeat ticks=100 budget.ttl_s=120 budget.ops=500"
  ND->>RT: validate JSON + ticket perms; RootTaskControl.spawn(heartbeat)
  alt spawn ok
    RT-->>ND: Ok(worker_id)
    ND-->>Cohsh: RWRITE
  else invalid or busy
    RT-->>ND: Err(Invalid or Busy or Permission)
    ND-->>Cohsh: Rerror(Invalid or Busy or Permission)
  end

  %% ---------------------------------------------------------
  %% D) Worker telemetry (append-only)
  %% ---------------------------------------------------------
  RT->>WT: append "telemetry tick=42 ts_ms=..."
  RT->>WT: append "telemetry tick=43 ts_ms=..."

  %% ---------------------------------------------------------
  %% E) GPU provider publishes /gpu/{id}/* (host-mirrored)
  %% ---------------------------------------------------------
  Note over GPUB,GPUFS: GPU nodes are provider-backed: info RO JSON; ctl append; job append; status RO stream.

  GPUB->>ND: Secure9P provider connect (host-only)
  ND-->>GPUB: session established
  GPUB->>GPUFS: publish /gpu/{id}/info
  GPUB->>GPUFS: publish /gpu/{id}/ctl
  GPUB->>GPUFS: publish /gpu/{id}/job
  GPUB->>GPUFS: publish /gpu/{id}/status

  %% Lease request via /queen/ctl; mirrored into /gpu/{id}/ctl and /log/queen.log
  Cohsh->>ND: TWRITE "JSON spawn=gpu lease.gpu_id=GPU-0 mem_mb=4096 streams=2 ttl_s=120"
  ND->>RT: validate + enqueue lease request
  alt provider available
    RT-->>ND: Ok(queued)
    ND-->>Cohsh: RWRITE
    RT->>GPUFS: append "LEASE ..." to /gpu/{id}/ctl
    RT->>LOG: append "lease issued ..." to /log/queen.log
    GPUB->>GPUFS: enforce lease; append status "QUEUED"
    GPUB->>GPUFS: append status "RUNNING"
  else provider unavailable
    RT-->>ND: Err(Busy)
    ND-->>Cohsh: Rerror(Busy)
  end

  %% Job submission + status/telemetry
  Cohsh->>ND: TWRITE "append job descriptor to /gpu/{id}/job"
  ND-->>Cohsh: RWRITE
  GPUB->>GPUFS: append status "OK" or "ERR"
  RT->>WT: append "gpu job state=OK/ERR ..."

  %% ---------------------------------------------------------
  %% F) Tail logs via 9P read (append-only enforced)
  %% ---------------------------------------------------------
  Cohsh->>ND: TREAD /log/queen.log offset=<n>
  ND-->>Cohsh: RREAD (append-only policy; server may ignore client offsets)
```

## 1. NineDoor 9P Operations
- Supports **9P2000.L** only (`version`, `attach`, `walk`, `open`, `read`, `write`, `clunk`, `stat`, `remove` (disabled)).
- `msize` negotiated ≤ 8192 bytes; larger requests rejected with `Rerror(TooBig)`.
- Fid tables are per-session; `clunk` invalidates handles immediately.
- Path components limited to 255 bytes and must be valid UTF-8 without NULs.

## 2. Capability Ticket
```rust
pub struct Ticket(pub [u8; 32]);

pub struct TicketClaims {
    pub role: Role,
    pub budget: Budget,
    pub mounts: MountSpec,
    pub issued_at_ms: u64,
}
```
- Minted by root task, delivered out-of-band during `attach`.
- Encoded using BLAKE3 MAC over claims to prevent tampering.

## 3. Queen Control Surface
Path: `/queen/ctl` (append-only JSON lines)
```json
{"spawn":"heartbeat","ticks":100,"budget":{"ttl_s":120,"ops":500}}
{"kill":"worker-7"}
{"bind":{"from":"/worker","to":"/shadow"}}
{"mount":{"service":"gpu-bridge","at":"/gpu"}}
{"spawn":"gpu","lease":{"gpu_id":"GPU-0","mem_mb":4096,"streams":2,"ttl_s":120}}
```
- Lines must parse as UTF-8 JSON; unknown fields logged and ignored.
- `spawn:"gpu"` queues a lease request for the host GPU bridge; if the bridge is unavailable the command returns `Error::Busy`.
- GPU spawns require the host bridge to publish `/gpu/<id>` entries via `install_gpu_nodes`; lease issuance is mirrored to `/log/queen.log` and `/gpu/<id>/ctl`.
- Optional `priority` fields raise scheduling weight on the host bridge when multiple leases compete.
- Operators typically exercise these verbs via `cohsh`, and any GUI client is expected to speak the same protocol.

## 4. Worker Telemetry
- Path: `/worker/<id>/telemetry` (append-only, newline-delimited records).
- Heartbeat payload: `{"tick":42,"ts_ms":123456789}`.
- GPU payload: `{"job":"jid-9","state":"RUNNING","detail":"scheduled"}` followed by `{"job":"jid-9","state":"OK","detail":"completed"}`.
- GPU telemetry schema (Milestone 6a):
  - Descriptor: `/gpu/telemetry/schema.json` (read-only, versioned)
  - Records must include `schema_version`, `device_id`, `model_id`, `time_window`, `token_count`, `latency_histogram`.
  - Optional fields: `lora_id`, `confidence`, `entropy`, `drift`, `feedback_flags`.
  - Max record size: 4096 bytes; append-only semantics enforced by host bridge before forwarding to `/queen/telemetry/*`.

## 5. GPU Bridge Files (host-mirrored)
| Path | Mode | Description |
|------|------|-------------|
| `/gpu/<id>/info` | read-only | JSON metadata: vendor, model, memory, SMs, driver/runtime versions |
| `/gpu/<id>/ctl` | append-only | Lease management: `LEASE`, `RELEASE`, `PRIORITY <n>` |
| `/gpu/<id>/job` | append-only | JSON job descriptors (validated hash, grid/block dims, optional `payload_b64`) |
| `/gpu/<id>/status` | read-only append stream | Job lifecycle entries (QUEUED/RUNNING/OK/ERR) |
| `/gpu/models/available/<model_id>/manifest.toml` | read-only | Host-authored model manifests; no uploads from the VM |
| `/gpu/models/active` | append-only pointer | Symlink-like pointer to the active model (atomic swap on host) |
| `/gpu/telemetry/schema.json` | read-only | Versioned schema descriptor (`gpu-telemetry/v1`) with field and size limits |
| `/gpu/telemetry/*` | append-only | Bounded telemetry windows tagged with `model_id` / `lora_id`; forwarded unchanged to `/queen/telemetry/*` and `/queen/export/lora_jobs/*` |

- WorkerGpu must read `/gpu/models/active` before emitting telemetry and propagate the `model_id`/`lora_id` into every record.
- Telemetry writes that exceed `max_record_bytes` or omit required fields are rejected by the host bridge prior to mirroring.

## 6. Root Task RPC (internal trait)
```rust
pub trait RootTaskControl {
    fn spawn(&self, role: Role, spec: WorkerSpec) -> Result<WorkerId, SpawnError>;
    fn kill(&self, id: WorkerId) -> Result<(), KillError>;
    fn bind(&self, session: SessionId, from: &str, to: &str) -> Result<(), NamespaceError>;
    fn mount(&self, session: SessionId, service: &str, at: &str) -> Result<(), NamespaceError>;
}
```
- NineDoor invokes these methods after validating JSON commands and ticket permissions.
- `WorkerSpec` includes budget, initial telemetry seed, and optional GPU lease request.

## 7. CLI (`cohsh`) Protocol
- Client attaches using the queen or worker ticket, negotiates `msize`, then issues 9P ops corresponding to shell commands.
- `tail` uses repeated `read` calls with offset tracking; NineDoor enforces append-only by ignoring provided offsets.
- `bind` and `mount` commands are no-ops for non-queen roles.
- `--transport tcp` connects to the root-task console listener (default `127.0.0.1:31337`) and speaks a line-oriented protocol:
  - `ATTACH <role> <ticket?>` → `OK ATTACH role=<role>` on success or `ERR ATTACH reason=<cause>` on failure.
  - `TAIL <path>` emits `OK TAIL path=<path>` before newline-delimited log entries; the stream still terminates with `END`.
  - All other verbs mirror serial behaviour and return a single acknowledgement (`OK LOG`, `ERR SPAWN reason=unauthenticated`, …)
    before triggering side effects.
- `PING` / `PONG` probes keep sessions alive; the client sends `PING` every 15 seconds of inactivity and expects an immediate
    `PONG` even when the server is mid-stream.
  - The TCP console enforces a maximum line length of 128 bytes and rate-limits failed authentication attempts (3 strikes within
    60 seconds triggers a 90-second cooldown). `cohsh` additionally validates worker tickets locally, rejecting whitespace or
    malformed values so automation does not leak failed attempts over the wire.
- Cohesix ships regression scripts in `.coh` format consumed by `coh> test`; see the canonical spec in [USERLAND_AND_CLI.md](./USERLAND_AND_CLI.md#coh-scripts-coh) for syntax and assertion rules.
- For `dev-virt`, QEMU uses the RTL8139 NIC and forwards `127.0.0.1:{31337/tcp,31338/udp,31339/tcp}` to `10.0.2.15` for the console and self-test ports; the virtio-net backend remains available behind a feature gate but is not the default. Operators generally do not need to care which NIC is active, but the backend label appears in boot logs for diagnostics.
- `cohsh` is the authoritative implementation of this protocol, and the planned WASM GUI is conceptually another client that wraps the same verbs without introducing a new control surface.

## 8. Error Surface
| Error | Meaning |
|-------|---------|
| `Permission` | Role not permitted to access path or mode |
| `NotFound` | Path or worker ID missing |
| `Busy` | Resource in use (GPU lease, worker slot) |
| `Invalid` | JSON parse failure or malformed 9P frame |
| `TooBig` | Frame exceeds negotiated `msize` |
| `Closed` | Fid used after `clunk` or revoked ticket |
| `RateLimited` | Console authentication locked out due to repeated failures |

## 9. Documentation Hooks
- Any new command or file path must be documented here and referenced from `ROLES_AND_SCHEDULING.md` and `BUILD_PLAN.md` before implementation.
