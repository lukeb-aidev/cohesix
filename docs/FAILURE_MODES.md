<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document deterministic failure behavior and operator recovery actions. -->
<!-- Author: Lukas Bower -->
# Cohesix Failure Modes

This document lists deterministic failure behavior and the required operator responses. All behavior here is **as-built**: observed via `/proc` nodes, control files, and `/log/queen.log` audit lines.

## Lifecycle failures

### 1) Invalid lifecycle transition
**Signal**
- `ERR` on `/queen/lifecycle/ctl` write.
- `/log/queen.log` line:
  - `lifecycle denied action=<cmd> state=<STATE> reason=invalid-transition`

**Impact**
- State **does not** change.
- No hidden retries.

**Recovery**
- Read `/proc/lifecycle/state` and choose a valid command:
  - `cordon` only from `ONLINE` or `DEGRADED`
  - `drain` only from `DRAINING`
  - `quiesce` from `ONLINE`, `DEGRADED`, or `DRAINING`
  - `resume` from any non-`ONLINE` state
  - `reset` from any non-`BOOTING` state

### 2) Outstanding leases block `drain`, `quiesce`, or `reset`
**Signal**
- `ERR` on `/queen/lifecycle/ctl` write.
- `/log/queen.log` line:
  - `lifecycle denied action=<cmd> state=<STATE> reason=outstanding-leases leases=<n>`

**Impact**
- State **does not** change.
- Work remains leased or attached.

**Recovery**
1. Inspect active workers (for example, via `/worker` or `/shard/.../worker`).
2. Explicitly revoke or kill workers using `/queen/ctl`.
3. Re-issue the lifecycle command once leases are zero.

### 3) Lifecycle gate denial
**Signal**
- `ERR` on a gated path (worker attach, telemetry ingest, host publishes, or GPU job writes).
- `/log/queen.log` line:
  - `lifecycle denied action=<gate> state=<STATE> reason=gate-denied`

**Impact**
- No side effects occur.
- Access is blocked deterministically until lifecycle state changes.

**Recovery**
- Move the node to an allowed state (typically `ONLINE` or `DEGRADED`).
- For maintenance windows, use `cordon` → `drain` → `quiesce` instead of forcing actions in blocked states.

## Telemetry ingest pressure
Telemetry ingest refusal is deterministic and policy-driven.

**Signals**
- `ERR` on `/queen/telemetry/<device>/seg/<id>` append when over limits.
- `/log/queen.log` entries indicate quota or wrap behavior (for example `telemetry quota reject` or `telemetry ring wrap`).

**Recovery**
- Adjust `telemetry_ingest.*` quotas in the manifest and regenerate with `coh-rtc`.
- For persistent spool behavior (Milestone 25b), inspect `/proc/spool/status` once available.

## Host publish denial
Host providers are gated by lifecycle state and policy.

**Signals**
- `ERR` on `/host/...` append when state disallows host publishes.
- `/log/queen.log` contains a `lifecycle denied` gate line.

**Recovery**
- Move lifecycle back to `ONLINE` or `DEGRADED`.
- If policy is enabled, ensure required approvals exist in `/actions/queue`.

## Worker attach denial
Worker roles cannot attach when lifecycle gates are closed.

**Signals**
- Attach fails with `ERR` and `/log/queen.log` shows `lifecycle denied action=worker-attach`.

**Recovery**
- Resume lifecycle (`resume`) once maintenance is complete.
- Re-attach with valid worker ticket.

