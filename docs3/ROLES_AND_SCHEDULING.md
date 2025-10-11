# Roles and Scheduling (Queen/Worker)

## Roles
- **Queen**: Orchestrator with authority to spawn/kill, bind/mount, and read system logs.
- **WorkerHeartbeat**: Emits `"heartbeat <tick>"` periodically to its own telemetry file.
- **WorkerGpu** (future): Manages GPU job streams under a lease, appending status to telemetry.

### Role Isolation
- Each process owns a `Ticket` and `Role`; NineDoor mounts only the paths allowed for that role.
- Queen can see `/worker/*` metadata; workers see only their subtree.

## Scheduling
- v0: **FCFS with budgets** — time/ops budget enforced by root task; revoke on exceed.
- v1: **Priority Bands** — `{system, control, worker}` classes.
- GPU (future): **Lease-based** — stream/memory quotas per lease; jobs per stream; fair-share across leases.

## Policies
- **Budget**: `ticks`, `ops`, or wall-clock `ttl_s` at spawn.
- **Revocation**: root task revokes endpoint/caps; NineDoor reports `Closed` on further access.
- **Back-off**: workers exceeding budgets are killed; queen may respawn with lower priority.

## Scheduling Interfaces
- `/queen/ctl`: supports `{"spawn":..., "budget":{"ttl_s":120,"ops":1000}}`
- `/worker/<id>/telemetry`: append-only; used for liveness and policy logging.

## Testing
- Unit: role path filters; attempt cross-role write must error `Permission`.
- Integration: spawn 2 heartbeat workers; budget one to expire; verify revocation.
