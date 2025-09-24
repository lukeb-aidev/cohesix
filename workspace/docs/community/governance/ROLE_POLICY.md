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
| InteractiveAiBooth | AI kiosk bridging remote CUDA annex results with local UI |
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

## Orchestrator Control Plane

QueenPrimary and RegionalQueen nodes expose the `cohesix.orchestrator.OrchestratorService`
gRPC API on the management interface. The default endpoint is
`http://127.0.0.1:50051`, and can be overridden via the
`COHESIX_ORCH_ADDR` environment variable when running clients.

The service defines the following RPCs:

* `Join` — worker registration with role, trust level, and capabilities.
* `Heartbeat` — periodic health reports including optional telemetry from Cohesix-managed CUDA microservers.
* `RequestSchedule` — agent placement queries with GPU-aware scheduling against the remote CUDA annex.
* `AssignRole` — administrative reassignment of worker roles.
* `UpdateTrust` — trust escalation or de-escalation for workers.
* `GetClusterState` — consolidated view of queen and worker health.

All RPCs require callers to originate from a trusted control network
segment. Deployments must terminate TLS at the ingress proxy and
authenticate requests using existing service-mesh credentials or mTLS
certificates issued to queen and worker nodes. Legacy filesystem drops
(`/srv/agents/active.json`, `/srv/trust_zones`) are maintained for
backwards compatibility but should be treated as read-only audit
mirrors of the gRPC source of truth.

<!-- Hybrid AI kiosk role consuming Cohesix CUDA Server outputs via Secure9P -->

| Role             | Description                                   | Interface                  |
|------------------|-----------------------------------------------|----------------------------|
| QueenPrimary     | Cloud-native orchestrator & CI coordinator: bootstraps with cloud hooks for auto-scaling, manages batch execution, monitors heartbeats, and coordinates other roles via gRPC. | gRPC control plane         |
| RegionalQueen    | Cloud-native cluster orchestrator: handles dynamic resource allocation, auto-scaling, and failover across multiple nodes, leveraging cloud hooks at boot. | gRPC control plane         |
| BareMetalQueen   | Bare-metal orchestrator for isolated or private networks, bootstrapping directly on hardware with minimal dependencies and direct device management. | Proprietary hardware interface |
| DroneWorker      | Physics & sensor processing: runs Rapier-based simulations and aggregates sensor inputs. | `/sim/` namespace          |
| InteractiveAiBooth | Hybrid AI kiosk booth that streams inference results from Cohesix CUDA Servers into local UI services. | `/srv/cuda` + Secure9P |
| KioskInteractive | Local human–machine interface: handles AR user interactions on kiosk displays. | WebSocket + 9P namespace   |
| GlassesAgent     | Vision pipeline & UI renderer for AR glasses: processes camera feeds locally and requests CUDA overlays from the remote annex. | `/srv/cuda` + 9P streams   |
| SensorRelay      | Sensor data aggregator: collects and forwards sensor streams to other roles. | 9P file streams            |
| SimulatorTest    | Scenario replay & integration testing: uses SimMount to replay recorded traces and validate system behavior. | SimMount + trace logs      |
```

This manifest guides both the OS initialization sequence and the Codex automation, ensuring every component is aware of its context and dependencies within the Cohesix platform.

### Additional Role Details

* **KioskInteractive** – offers a local UI terminal with restricted command set.
* **DroneWorker** – runs physics and sensor pipelines for autonomous drones.
* **GlassesAgent** – provides AR overlays by requesting GPU rendering from Cohesix CUDA Servers when available.
* **SensorRelay** – forwards raw sensor data to other agents.
* **SimulatorTest** – replays recorded traces to validate system behavior.

Each role's interface and privileges are strictly enforced by the Secure9P validator and policy files, ensuring only authorized namespaces and operations are available at runtime.

Federated deployments may declare hierarchical roles. A Queen inheriting from another uses `inherit:<parent_id>` in `/srv/queen_id/role` which is exchanged during federation handshakes. All child queens inherit base policies while applying their own overlays.


## Queen Policy

The policies described here apply only to QueenPrimary, RegionalQueen, and BareMetalQueen roles.

This document defines internal enforcement policies for the Queen role.

Federated queens may delegate sub-roles to peers. The policy engine resolves conflicts by preferring the latest timestamped policy file within `/srv/<peer>/policy_override.json`. Administrators can supply explicit rules to override time-based resolution.

✅ This policy file is aligned with Secure9P enforcement and runtime validator checks.
