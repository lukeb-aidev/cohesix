<!-- Author: Lukas Bower -->
# Roles & Scheduling Policy

## 1. Roles
| Role | Capabilities | Namespace |
|------|--------------|-----------|
| **Queen** | Spawn/kill workers, bind/mount namespaces, inspect logs, request GPU leases | Full `/`, `/queen`, `/worker/*`, `/log`, `/gpu/* (future)` |
| **WorkerHeartbeat** | Emit telemetry, read queen log, read boot info | `/proc/boot`, `/worker/self/telemetry`, `/log/queen.log` (RO) |
| **WorkerGpu** (future) | Submit GPU jobs, monitor leases, emit telemetry | WorkerHeartbeat view + `/gpu/<lease>/*` |
| **Observer** (future) | Read-only status access | `/proc`, `/log` |

## 2. Ticket Lifecycle
1. Queen requests spawn with desired role/budget.
2. Root task allocates capability space, minting a `Ticket` bound to the role, worker ID, and mount table.
3. Ticket is delivered during 9P `attach`; NineDoor verifies MAC and initialises session state.
4. On kill or budget expiry, root task revokes ticket and notifies NineDoor to clunk all active fids.

## 3. Scheduling Strategy
- **v0**: Round-robin over runnable endpoints with per-worker tick budgets (coarse-grained cooperative scheduling).
- **v1**: Priority bands (`system`, `control`, `worker`) with budgeted quanta; queen/control tasks reside in higher band.
- **GPU (future)**: Lease-enforced concurrency; GPU workers must honour host-provided stream counts.

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

## 7. Future Extensions
- Role hierarchy for observers/auditors.
- Quotas for memory/IPC buffers enforced via seL4 resource allocation APIs.
- Worker-side cooperative yields signalled via `/worker/self/yield` control file.
