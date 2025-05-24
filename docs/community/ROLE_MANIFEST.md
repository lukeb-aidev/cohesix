// CLASSIFICATION: COMMUNITY
// Filename: ROLE_MANIFEST.md v1.2
// Date Modified: 2025-05-25
// Author: Lukas Bower

# Role Manifest

At boot, Cohesix reads the declared runtime role from `/srv/cohrole` to determine which services and agents to initialize. Each role encapsulates a distinct set of responsibilities, interfaces, and resource privileges—ensuring least-privilege operation and clear service orchestration.

| Role             | Description                                   | Interface                  |
|------------------|-----------------------------------------------|----------------------------|
| QueenPrimary     | Cloud-native orchestrator & CI coordinator: bootstraps with cloud hooks for auto-scaling, manages batch execution, monitors heartbeats, and coordinates other roles via gRPC. | gRPC control plane         |
| RegionalQueen    | Cloud-native cluster orchestrator: handles dynamic resource allocation, auto-scaling, and failover across multiple nodes, leveraging cloud hooks at boot. | gRPC control plane         |
| BareMetalQueen   | Bare-metal orchestrator for isolated or private networks, bootstrapping directly on hardware with minimal dependencies and direct device management. | Proprietary hardware interface |
| DroneWorker      | Physics & sensor processing: runs Rapier-based simulations and aggregates sensor inputs. | `/sim/` namespace          |
| KioskInteractive | Local human–machine interface: handles AR user interactions on kiosk displays. | WebSocket + 9P namespace   |
| GlassesAgent     | Vision pipeline & UI renderer for AR glasses: processes camera feeds and renders overlays via CUDA. | `/srv/cuda` + 9P streams   |
| SensorRelay      | Sensor data aggregator: collects and forwards sensor streams to other roles. | 9P file streams            |
| SimulatorTest    | Scenario replay & integration testing: uses SimMount to replay recorded traces and validate system behavior. | SimMount + trace logs      |
```

This manifest guides both the OS initialization sequence and the Codex automation, ensuring every component is aware of its context and dependencies within the Cohesix platform.