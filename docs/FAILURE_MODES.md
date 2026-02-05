<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document deterministic failure behavior and operator recovery actions. -->
<!-- Author: Lukas Bower -->
# Cohesix Failure Modes

This document lists deterministic failure behavior and the required operator responses. All behavior here is **as-built**: observed via `/proc` nodes, control files, and `/log/queen.log` audit lines.

**Operating principles**
- All failures are deterministic and bounded. An `ERR` implies **no side effects** unless explicitly documented.
- `/log/queen.log` is the authoritative audit trail for control denials and lifecycle gates.
- `/proc/*` is the authoritative read-only source for lifecycle, pressure, and queue state.

**Quick triage checklist**
- `cat /proc/lifecycle/state` and `cat /proc/lifecycle/reason`
- `cat /proc/root/reachable` and `cat /proc/root/cut_reason`
- `cat /proc/pressure/*` (busy/quota/cut/policy)
- `tail /log/queen.log` (or `cat` for bounded inspection)

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

## Policy gate failures

### 1) Missing approval for gated control write
**Signal**
- `ERR ECHO reason=policy ... EPERM` when writing to a gated path (for example `/queen/ctl`).
- `/log/queen.log` line indicating policy denial.

**Impact**
- The control action is refused deterministically.

**Recovery**
1. Read `/policy/rules` to confirm the target is gated.
2. Queue an approval in `/actions/queue` with `id`, `target`, and `decision`.
3. Retry the control write.

### 2) Replay attempt for a consumed approval
**Signal**
- `ERR` on a gated write even though an approval was previously queued.
- `/log/queen.log` indicates a consumed or replayed action.

**Impact**
- Approvals are single-use; replays are refused.

**Recovery**
- Queue a fresh approval in `/actions/queue`, then retry the write.

## Console and transport failures

### 1) Console already in use
**Signal**
- `cohsh` or a tool hangs on connect, or a tool reports a busy/locked console.

**Impact**
- Only one TCP console client can attach at a time.

**Recovery**
1. Quit the active console client (`cohsh`, `swarmui`, `hive-gateway`, `coh`, `gpu-bridge-host`, `host-sidecar-bridge`).
2. Retry the connection.

### 2) Connection refused or wrong port
**Signal**
- TCP connection refused when attaching.

**Impact**
- The VM or gateway is not running, or the host-forwarded port is incorrect.

**Recovery**
- Verify QEMU is running and the console port matches your configuration (default `127.0.0.1:31337`).

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

## Host bridge visibility failures

### 1) `/gpu` or `/gpu/models` is empty
**Signal**
- `ls /gpu` returns empty or `ERR` and `/gpu/models` is missing.

**Impact**
- The host GPU bridge has not published a snapshot yet.

**Recovery**
- Run `./bin/gpu-bridge-host --publish ...` and verify `/gpu/bridge/status`.

### 2) `/host` is empty
**Signal**
- `ls /host` returns empty or `ERR`.

**Impact**
- The host sidecar bridge is not publishing providers.

**Recovery**
- Run `./bin/host-sidecar-bridge --watch --provider ...` and re-check `/host/*`.

## Bounds and path violations

### 1) Path or read size exceeds bounds
**Signal**
- `path exceeds max length`, `path component '..' is not permitted`, or `read exceeds max bytes`.

**Impact**
- Request is refused deterministically.

**Recovery**
- Use `/v1/meta/bounds` (REST) or the manifest-derived limits to size requests within bounds.
