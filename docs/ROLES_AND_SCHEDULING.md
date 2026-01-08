<!-- Author: Lukas Bower -->
<!-- Purpose: Define Cohesix roles, ticket lifecycle, and scheduling constraints. -->
# Roles & Scheduling Policy

## 1. Roles
| Role | Capabilities | Namespace |
|------|--------------|-----------|
| **Queen** | Hive-wide orchestrator driven by `cohsh`: spawn/kill workers, bind/mount namespaces, inspect logs, request GPU leases across many worker instances | Full `/`, `/queen`, `/worker/*`, `/log`, `/gpu/*` (when installed) |
| **WorkerHeartbeat** | Minimal worker that emits heartbeat telemetry and confirms console/attach paths; many instances may run concurrently under the Queen | `/proc/boot`, `/worker/self/telemetry`, `/log/queen.log` (RO) |
| **WorkerGpu** | GPU-centric worker that reads ticket/lease state and reports telemetry for host-provided GPU nodes; treated as another worker type under the Queen | WorkerHeartbeat view + `/gpu/<id>/*` |
| **Observer** (future) | Read-only status access | `/proc`, `/log` |

Exactly one Queen exists per hive, but many worker instances (across worker-heart, worker-gpu, and future types) can be orchestrated simultaneously. The queen session attached via `cohsh` is the canonical path for operators and automation to exercise these roles.

## 2. Ticket Lifecycle
1. Queen requests spawn with desired role/budget.
2. Root task allocates capability space, minting a `Ticket` bound to the role, worker identity (`subject`), and mount table.
3. Ticket is delivered during 9P `attach`; NineDoor verifies MAC and initialises session state.
4. On kill or budget expiry, root task revokes ticket and notifies NineDoor to clunk all active fids.

Attachments always arrive via NineDoor: queen mounts the full namespace, worker-heartbeat mounts only its telemetry and boot views, and worker-gpu attaches to the `/gpu/<id>/` subtrees exposed to its ticket. Ticket values (when present) select the role-specific namespace, and NineDoor aborts attaches on ticket mismatch, timeouts, or unsupported roles, leaving `cohsh` detached with an explicit error.

## 3. Scheduling Strategy
- **v0**: Round-robin over runnable endpoints with per-worker tick budgets (coarse-grained cooperative scheduling).
- **v1**: Priority bands (`system`, `control`, `worker`) with budgeted quanta; queen/control tasks reside in higher band.
- **GPU (future)**: Lease-enforced concurrency; GPU workers must honour host-provided stream counts.

Control flows are file-oriented (e.g., appends to `/queen/ctl`) instead of the deprecated RPC/virtual-console sketches; `cohsh` always runs outside the Cohesix instance—QEMU during development and UEFI hardware in deployment—and speaks the NineDoor transport.

Scheduling contexts originate in root-task: initial SCs are held by root, carved out for NineDoor and per-worker threads, and reclaimed on revocation without altering seL4 SC semantics or time accounting.

## 4. Budget Types
```rust
pub struct Budget {
    pub ticks: Option<u32>,     // scheduler quanta
    pub ops: Option<u32>,       // NineDoor operations
    pub ttl_s: Option<u32>,     // wall-clock lifetime
}
```
- Budgets default to conservative limits; queen can request overrides but root task may clamp to policy maximums.
- NineDoor decrements `ops` budgets per successful request; when depleted it signals root task for revocation.

## 5. Revocation Flow
1. Budget exhaustion detected by NineDoor or root task watchdog.
2. Root task sends `Revoke(ticket_id)` to NineDoor.
3. NineDoor marks session closed, replies `Rerror(Closed)` on further operations, and appends revocation reason to `/log/queen.log`.
4. Root task deallocates resources (TCB caps, scheduling context).

## 6. Testing Expectations
- **Unit**: Role-path filter tests ensure workers cannot traverse outside assigned mounts. Budget counters validated with deterministic scenarios.
- **Integration**: Scenario test spawns two heartbeat workers with different TTLs; verifies early expiry worker is revoked and log entry recorded.
- **Fuzz**: Randomised spawn/kill command sequences ensure scheduler state remains consistent (no leaked caps).

Cross-refs: see `SECURE9P.md` for namespace enforcement, `USERLAND_AND_CLI.md` for attach semantics, and `ARCHITECTURE.md` for the serial + TCP console model.

## 7. Future Extensions
- Role hierarchy for observers/auditors.
- Quotas for memory/IPC buffers enforced via seL4 resource allocation APIs.
- Worker-side cooperative yields signalled via `/worker/self/yield` control file.
