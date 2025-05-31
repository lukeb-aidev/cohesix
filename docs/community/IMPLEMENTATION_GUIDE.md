// CLASSIFICATION: COMMUNITY
// Filename: IMPLEMENTATION_GUIDE.md v1.1
// Author: Lukas Bower
// Date Modified: 2025-05-31

# IMPLEMENTATION GUIDE

This guide outlines the architectural implementation of Cohesix, including boot logic, roles, userland, service layout, and integration strategies.

## 1 · Kernel and Bootloader

- **Kernel:** seL4 L4-microkernel (vanilla upstream + Cohesix patches)
- **Bootloader:** Minimal seL4-compatible loader
- **Startup Path:** <200ms boot on Pi5/Orin using precompiled image
- **CohRole Declaration:** Passed via bootarg; immutable once set
- **Telemetry:** Early init trace to ring buffer + optional UART
- **Watchdog:** Startup heartbeat to ensure boot complete in 5s

## 2 · Userland and Roles

- **Base Environment:** Plan 9-style 9P namespace + rc shell
- **Roles:**
  - `QueenPrimary` – Orchestrator, telemetry sink, agent validator
  - `DroneWorker` – Compute node, physics+CUDA, acts on goals
  - `KioskInteractive` – UX-forward node, sandbox GUI host
  - `SensorRelay` – Streams real-world inputs to agents
  - `SimulatorTest` – Virtual test harness for validator rules
  - `GlassesAgent` – Minimal node for AR/vision overlay tasks

## 3 · Services and Mounts

- **Services:** Exposed via `/srv/` and modular
  - `cuda` – exposed on Jetson nodes
  - `telemetry` – logs, metrics, diagnostics
  - `sandbox` – syscall limits, trace filtering
  - `trace` – unified rule and system event log
  - `agent` – loader and runner for Cohesix agents
- **Namespace Convention:** Worker namespace overlays with Queen map
- **Service Init:** Called from `initialize_services()` in Rust runtime

## 4 · Runtime and Validation

- **Validator:** Embedded rule engine monitors syscalls + sandbox
- **Trace Format:** Unified JSONL, emitted per agent+tick
- **Goals:** Loaded from `/etc/goals/` or `/sim/` inputs
- **Replay:** All events can be replayed with full trace revalidation
- **Scenarios:** Validator metadata tracks active rules per scene
- **Upgrades:** Role can request runtime restart via envelope syscall

## 5 · Development and Deployment

- **Build Matrix:** aarch64 (Orin, Pi5), x86_64 (Queen)
- **Dev Flow:** Rust core + Python tooling + optional Go services
- **Snapshots:** Saved to `/history/` as ZIP+log bundles
- **Upstream Sync:** seL4 and 9front rebased monthly
- **OSS Policy:** All modules under MIT, BSD, or Apache 2.0

## 6 · Testing and Safety

- **Trace Validator:** Required for all agent actions
- **Rule Violations:** Logged + gated at envelope layer
- **Watchdog Loop:** 5s recovery for boot, 30s for hung services
- **Power Loss Recovery:** Fast restart via sandbox trace cache
- **Role Testbed:** Simulator harness runs every declared role pre-deploy
