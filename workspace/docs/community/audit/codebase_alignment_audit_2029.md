// CLASSIFICATION: COMMUNITY
// Filename: codebase_alignment_audit_2029.md v0.1
// Author: Lukas Bower
// Date Modified: 2029-02-21

# Cohesix Vision Alignment Audit (February 2029)

## Overview
This audit reviews whether the current codebase fulfils the architecture and process expectations documented under `workspace/docs`. Each finding links back to the vision statements and highlights concrete code gaps.

## Summary of Gaps
| # | Area | Severity | Gap Description |
|---|------|----------|-----------------|
| 1 | GUI Orchestrator | High | HTTP façade never queries the gRPC orchestrator service, so dashboards cannot reflect live cluster state or trigger orchestration actions. |
| 2 | Trace Logging | High | Runtime tracing defaults to `/srv/trace`, diverging from the `/log/trace/` contract used by docs, tooling, and replay workflows. |
| 3 | Boot Performance | Medium | CI boot script only waits for a success marker; it does not measure or enforce the sub‑200 ms cold‑boot target. |
| 4 | Remote GPU Annex | Medium | CUDA executor silently succeeds when no remote endpoint is configured, offering no graceful fallback path or telemetry. |
| 5 | Metadata Hygiene | Medium | New gRPC queen orchestrator source is missing from `METADATA.md`, violating mandatory source registration rules. |

## Detailed Findings

### 1. GUI Orchestrator bypasses the gRPC control plane
The product vision states that the GUI must pull cluster state via the `GetClusterState` RPC and expose live controls over gRPC【F:workspace/docs/community/architecture/GUI_ORCHESTRATOR.md†L14-L34】. The Go implementation, however, serves a hard-coded status payload (`Workers: 3`) and wires `api.DefaultController()`, a no-op executor, behind `/api/control`【F:go/orchestrator/api/status.go†L23-L34】【F:go/orchestrator/api/control.go†L30-L55】【F:go/orchestrator/http/server.go†L50-L64】. As a result, the web dashboard cannot reflect actual orchestrator state or issue real commands.

### 2. Trace logs written outside the `/log/trace/` contract
The mission document requires that "all syscalls, agent actions, and CLI invocations are recorded in `/log/trace/`" for replayability and validator hooks【F:workspace/docs/community/architecture/MISSION_AND_ARCHITECTURE.md†L16-L43】. The runtime recorder defaults to `/srv/trace` (falling back to a temp dir) and never targets `/log/trace/`, breaking documented tooling expectations and downstream analytics that read from `/log/trace/`【F:workspace/cohesix/src/trace/recorder.rs†L32-L116】.

### 3. CI boot test ignores the sub‑200 ms cold-start objective
The vision emphasizes reaching userland in under 200 ms during cold boot and highlights this metric as a differentiator【F:workspace/docs/community/architecture/MISSION_AND_ARCHITECTURE.md†L24-L55】. The `ci/qemu_boot_check.sh` workflow merely polls for a "Cohesix shell started" marker for up to 30 seconds with one-second sleeps, but never records boot timing or enforces the 200 ms objective【F:ci/qemu_boot_check.sh†L51-L176】. Without instrumentation, regressions against the latency target go undetected.

### 4. Remote CUDA executor lacks graceful fallback
Documentation promises that `/srv/cuda` proxies to managed CUDA microservers "with graceful fallback if absent"【F:workspace/docs/community/architecture/MISSION_AND_ARCHITECTURE.md†L16-L25】. The `CudaExecutor` simply trims the configured address and, if no `tcp:` endpoint is present, returns success without dispatching work, telemetry, or CPU fallback handling【F:workspace/cohesix/src/cuda/runtime.rs†L16-L55】. Missing endpoints silently discard GPU work instead of degrading in a controlled way.

### 5. Required metadata entry missing for queen orchestrator
All source files must be registered in `METADATA.md` alongside versioning details【F:workspace/docs/community/governance/INSTRUCTION_BLOCK.md†L6-L12】. The new queen orchestrator gRPC server lives at `workspace/cohesix/src/queen/orchestrator.rs`【F:workspace/cohesix/src/queen/orchestrator.rs†L1-L40】, yet the metadata table lists neighbouring components (e.g., `src/cloud/orchestrator.rs`, Go command stubs) without any entry for this file【F:workspace/docs/community/governance/METADATA.md†L300-L337】. This breaks traceability and version control required by the governance process.

## Recommendations
1. Refactor the GUI orchestrator to depend on the existing tonic gRPC client so `/api/status` and `/api/control` surface real orchestrator data.
2. Update trace recording utilities and related tooling to log under `/log/trace/`, keeping `/srv/trace` only as a compatibility symlink if needed.
3. Extend `ci/qemu_boot_check.sh` (or a Rust smoke test) with boot timestamp capture—e.g., measure QEMU serial timestamp differences—and fail builds exceeding the 200 ms budget.
4. Implement explicit CUDA fallback: report "no remote GPU" through telemetry, trigger CPU alternatives, or raise actionable errors instead of returning success.
5. Add the queen orchestrator to `METADATA.md` (and `CHANGELOG.md`) with the proper version to restore metadata compliance.

