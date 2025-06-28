// CLASSIFICATION: COMMUNITY
// Filename: ROLE_POLICY.md v1.1
// Author: Lukas Bower
// Date Modified: 2026-09-30

# Role Policy

This document merges `ROLE_MANIFEST.md` and the private `QUEEN_POLICY.md` into a single reference. It outlines runtime roles and the policies that govern the Queen role.

## Role Summary

| Role             | Summary Purpose                     |
|------------------|-------------------------------------|
| QueenPrimary     | Cloud orchestrator and CI manager   |
| RegionalQueen    | Multi-node cloud federation lead    |
| BareMetalQueen   | On-premise orchestration node       |
| DroneWorker      | Physical simulation + sensors       |
| InteractiveAiBooth | AI kiosk with Jetson + UI stack  |
| KioskInteractive | Standalone human interface terminal |
| GlassesAgent     | AR rendering for glasses            |
| SensorRelay      | Sensor aggregator + forwarder       |
| SimulatorTest    | Scenario trace + test validation    |

See below for full technical descriptions, interfaces, and orchestration behavior.

At boot, Cohesix reads the declared runtime role from `/srv/cohrole` to determine which services and agents to initialize.
Typical contents of /srv/cohrole might be:
  QueenPrimary
  DroneWorker
  KioskInteractive
This string is matched directly against the RoleManifest.

<!-- New hybrid AI kiosk role combining Jetson GPU features with interactive booth UI -->

| Role             | Description                                   | Interface                  |
|------------------|-----------------------------------------------|----------------------------|
| QueenPrimary     | Cloud-native orchestrator & CI coordinator: bootstraps with cloud hooks for auto-scaling, manages batch execution, monitors heartbeats, and coordinates other roles via gRPC. | gRPC control plane         |
| RegionalQueen    | Cloud-native cluster orchestrator: handles dynamic resource allocation, auto-scaling, and failover across multiple nodes, leveraging cloud hooks at boot. | gRPC control plane         |
| BareMetalQueen   | Bare-metal orchestrator for isolated or private networks, bootstrapping directly on hardware with minimal dependencies and direct device management. | Proprietary hardware interface |
| DroneWorker      | Physics & sensor processing: runs Rapier-based simulations and aggregates sensor inputs. | `/sim/` namespace          |
| InteractiveAiBooth | Hybrid AI kiosk booth with Jetson acceleration and UI services. | `/srv/cuda` + Secure9P |
| KioskInteractive | Local human–machine interface: handles AR user interactions on kiosk displays. | WebSocket + 9P namespace   |
| GlassesAgent     | Vision pipeline & UI renderer for AR glasses: processes camera feeds and renders overlays via CUDA. | `/srv/cuda` + 9P streams   |
| SensorRelay      | Sensor data aggregator: collects and forwards sensor streams to other roles. | 9P file streams            |
| SimulatorTest    | Scenario replay & integration testing: uses SimMount to replay recorded traces and validate system behavior. | SimMount + trace logs      |
```

This manifest guides both the OS initialization sequence and the Codex automation, ensuring every component is aware of its context and dependencies within the Cohesix platform.

### Additional Role Details

* **KioskInteractive** – offers a local UI terminal with restricted command set.
* **DroneWorker** – runs physics and sensor pipelines for autonomous drones.
* **GlassesAgent** – provides AR overlays using CUDA when available.
* **SensorRelay** – forwards raw sensor data to other agents.
* **SimulatorTest** – replays recorded traces to validate system behavior.

Each role's interface and privileges are strictly enforced by the Secure9P validator and policy files, ensuring only authorized namespaces and operations are available at runtime.

Federated deployments may declare hierarchical roles. A Queen inheriting from another uses `inherit:<parent_id>` in `/srv/queen_id/role` which is exchanged during federation handshakes. All child queens inherit base policies while applying their own overlays.


## Queen Policy

The policies described here apply only to QueenPrimary, RegionalQueen, and BareMetalQueen roles.

This document defines internal enforcement policies for the Queen role.

Federated queens may delegate sub-roles to peers. The policy engine resolves conflicts by preferring the latest timestamped policy file within `/srv/<peer>/policy_override.json`. Administrators can supply explicit rules to override time-based resolution.

✅ This policy file is aligned with Secure9P enforcement and runtime validator checks.
