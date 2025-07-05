// CLASSIFICATION: COMMUNITY
// Filename: DEMO_SCENARIOS.md v2.0
// Author: Lukas Bower
// Date Modified: 2026-07-05

# Cohesix Demo Scenarios — "Mother of All Demos"

This document outlines Cohesix's landmark demonstration suite: a set of interlinked, live scenarios designed to **stun the community and captivate investors**, proving Cohesix as the future of secure, physically-aware, GPU-accelerated distributed systems.

Unlike traditional stacks, Cohesix does **not embed CUDA directly**, but instead orchestrates GPU-enabled systems via secure roles, remote telemetry, and the **Cohesix CUDA Server**. This decoupled architecture ensures robust policy control and hardware abstraction.

---

## 🌐 Core Narrative: "The Queen Commands the Swarm"

At the heart of the demo: a **QueenPrimary** orchestrates a global swarm of heterogeneous Workers — from edge nodes to GPU clusters — using Secure9P and Plan9's powerful namespace. Together, they achieve real-time, physics-validated, policy-enforced AI execution.

Each scenario builds on the last, culminating in a full spectacle of distributed learning, live rule enforcement, and transparent validation.

---

## 🚀 Demo Index

| # | Name | Highlights |
|---|------|------------|
| 1 | The Gesture Tamer | Physically-grounded gesture control via webcam + Rapier |
| 2 | GPU Swarm Scheduler | The Queen dispatches workloads to a fleet of Cohesix CUDA Servers |
| 3 | Rule-Bound Physics Duel | Multi-agent simulation with real-time policy clamps |
| 4 | Trace Replay and Mutation | Traces mutate under new policies and replay across swarm |
| 5 | Live Security Breach Response | Simulated role breach triggers autonomous isolation |
| 6 | Investor “God View” | Full topological map, live telemetry, and time-warped replays |

---

## 🕹️ 1. The Gesture Tamer

- **Goal:** Show a webcam feed processed on a local Worker. Gestures directly tilt a Rapier-based physical beam.  
- The Cohesix Validator watches for unsafe force vectors (e.g. sudden whips) and flags violations.  
- Each action is logged to `/log/trace/gesture_tamer.log`, with immediate replay capability.

---

## 🚀 2. GPU Swarm Scheduler

- **Goal:** Demonstrate how the Queen uses published `gpu_capacity` and `latency_score` from remote Cohesix CUDA Servers to schedule heavy inference workloads.
- The Queen migrates a stable diffusion or object detection task to the optimal node.
- Snapshots captured under `/history/snapshots/gpu_demo/`.

---

## ⚖️ 3. Rule-Bound Physics Duel

- **Goal:** Two Workers, each running separate physics agents, try to destabilize a shared Rapier beam.
- The Queen enforces fairness by adjusting role privileges mid-game.
- Live rule updates are pushed via Secure9P, tested under temporal conditions (`max_force_window_5s`), and logged.

---

## 🔁 4. Trace Replay and Mutation

- **Goal:** Show the power of Cohesix’s trace system.  
- A captured scenario is replayed under a new rule set — e.g. tightening energy budgets or changing permitted torque.  
- Deviations are flagged, with mutations stored to `/log/trace/mutated_runs/`.

---

## 🛡️ 5. Live Security Breach Response

- **Goal:** Simulate a Worker attempting a forbidden role escalation (from `SensorRelay` to `DroneWorker`).
- Queen’s validator instantly severs and quarantines the node.
- The breach and subsequent recovery are streamed to investors on a dedicated UI.

---

## 🌍 6. Investor “God View”

- **Goal:** All of the above scenarios merge into a finale:
  - A full global topology of Workers, CUDA Servers, physics sandboxes, and validators.
  - Live streams of webcam control, GPU metrics, rule triggers, and historical trace replays.
  - The Queen executes a controlled time-warp replay of violations to prove deterministic auditability.

---

## 🔎 Additional Services

- **Kiosk Federation:** UIs hosted at `/srv/ui_bundle/kiosk_v1/` for public or investor interaction, triggered by `cohtrace kiosk_ping`.
- **Secure Sensor Relays:** Distributed sensor nodes maintain encrypted streams, proving Cohesix’s multi-role trust enforcement.
- **GPU Registry:** Every GPU node publishes into `/srv/gpu_registry.json` for live Queen scheduling.

---

## 📝 Retired or Consolidated

- Older single-device demos (Jetson local CUDA, direct YOLO inferencing) are deprecated. All CUDA work is now orchestrated via **Cohesix CUDA Servers**, maintaining clean separation of compute roles and GPU policy control.

---

✅ **Conclusion:**  
This demonstration suite is engineered to **shock the market**, proving that secure, physically-validated, distributed GPU orchestration is possible — and only possible — through the novel architecture of Cohesix.

// CLASSIFICATION: COMMUNITY
// Filename: DEMO_SCENARIOS.md v2.1
// Author: Lukas Bower
// Date Modified: 2026-07-05

# Cohesix Demo Scenarios

This document introduces a collection of live demonstrations showcasing how secure, distributed GPU workloads, physics-grounded validation, and dynamic policy enforcement can operate across heterogeneous systems using Plan9 namespaces and Secure9P. These scenarios are designed to be independently verifiable, replayable, and extensible.

---

## 🌐 Overview

Each demo highlights a different aspect of the system: from gesture-controlled physical environments to distributed GPU task scheduling and live breach containment. Logs, traces, and replay capabilities are built into every workflow.

---

## 🚀 Demo Index

| # | Name | Description |
|---|------|-------------|
| 1 | Gesture-Controlled Physics | Control a physics beam via live webcam gestures |
| 2 | Distributed GPU Scheduling | Schedule GPU workloads based on capacity and latency metrics |
| 3 | Policy-Bound Physics Duel | Competing agents constrained by real-time policies |
| 4 | Trace Replay with Policy Mutation | Apply new rule sets to previous execution traces |
| 5 | Automated Breach Response | Simulate and contain unauthorized role escalations |
| 6 | System Topology and Time-Warped Replay | Visualize the global topology with historic rule triggers |

---

## 🕹️ 1. Gesture-Controlled Physics

- A local Worker processes webcam input to generate force vectors that tilt a Rapier physics beam.
- The validator monitors these forces, recording any threshold violations.
- All actions and outcomes are logged to `/log/trace/gesture_tamer.log` and can be replayed under different rule constraints.

---

## 🚀 2. Distributed GPU Scheduling

- Multiple Cohesix CUDA Servers report available GPU resources (`gpu_capacity` and `latency_score`) to the Queen.
- Heavy inference tasks are scheduled dynamically based on current availability.
- Snapshots and execution traces are stored under `/history/snapshots/gpu_demo/`.

---

## ⚖️ 3. Policy-Bound Physics Duel

- Two Workers each control agents that attempt to destabilize a shared Rapier beam.
- Mid-run, the Queen updates fairness rules (like `max_force_window_5s`) and propagates them via Secure9P.
- Violations are flagged and logged.

---

## 🔁 4. Trace Replay with Policy Mutation

- Execution traces from earlier scenarios are replayed under new or stricter policies.
- Deviations from the original run are highlighted, and mutated traces are stored under `/log/trace/mutated_runs/`.

---

## 🛡️ 5. Automated Breach Response

- A Worker attempts to escalate its role from `SensorRelay` to `DroneWorker`.
- The system detects the breach and isolates the offending node without disrupting the remaining environment.
- The process is streamed to a monitoring UI for review.

---

## 🌍 6. System Topology and Time-Warped Replay

- Provides a real-time map of all Workers, CUDA Servers, physics engines, and validators.
- Enables controlled time-warped replays of past rule violations, illustrating deterministic enforcement and auditability.

---

## 🔎 Additional Features

- **Kiosk UIs:** Hosted at `/srv/ui_bundle/kiosk_v1/`, triggered by `cohtrace kiosk_ping`, for local or remote interaction.
- **Secure Sensor Relays:** Encrypt and forward data streams under strict role validation.
- **GPU Registry:** Maintains a live JSON file (`/srv/gpu_registry.json`) that tracks all registered GPU nodes for on-demand scheduling.

---

### Note on Direct GPU Integration

CUDA is not embedded directly within the core system. Instead, GPU workloads are executed via Cohesix CUDA Servers, maintaining clear separation of roles and policy enforcement.

---

All scenarios are designed to be independently verified, replayed with alternative policies, and extended into new environments. Logs and traces are retained for forensic or compliance review.