// CLASSIFICATION: COMMUNITY
// Filename: DEMO_SCENARIOS.md v1.1
// Author: Lukas Bower
// Date Modified: 2026-02-11

# Demo Scenarios

These demonstrations are intended as Cohesix's own **"Mother of All Demos."** Each scenario combines real‑time vision, physics, policy enforcement, and full traceability. Run them via `cohrun` or `cohcli` using configuration files in `examples/`.

## Demo Index
1. The Bee Learns — Physically Grounded Gesture Teaching and Rule Enforcement [Jetson + webcam]
2. Cloud Queen + Home Orin [Jetson + webcam]
3. CUDA Inferencing at the Edge [Jetson]
4. Physics Sandbox Controlled by Webcam [Jetson + webcam]
5. Trace Validation Loop
6. Secure Sensor Relay [Jetson]
7. Sensor‑Driven World Adaptation [Jetson + sensors]
8. Multi-Agent Physics Duel [Jetson]

## 1. The Bee Learns — Physically Grounded Gesture Teaching and Rule Enforcement
Real-time webcam input on the Jetson Orin Nano is fed to a CUDA pose estimator. Gestures map to intents that drive a Rapier physics simulation. The validator enforces learned safety rules and logs violations for replay. Policy changes can be applied to the trace and re-executed.
Trace: `/log/trace/bee_learns.log`

## 2. Cloud Queen + Home Orin
Jetson Orin Nano streams webcam data to the Queen. Gestures trigger SLM payloads with latency under 100 ms.
Trace: `/log/trace/scenario_1.log`

## 3. CUDA Inferencing at the Edge
Local YOLOv8 inference on the Orin with hot‑swappable models from the Queen.
Trace: `/log/trace/scenario_2.log`

## 4. Physics Sandbox Controlled by Webcam
Webcam tilt values drive a Rapier simulation. Results are validated via `cohtrace`.
Trace: `/log/trace/scenario_3.log`

## 5. Trace Validation Loop
Worker traces are replayed on the Queen to detect deviations in real time.
Trace: `/log/trace/scenario_4.log`

## 6. Secure Sensor Relay
A Worker declared as `SensorRelay` streams encrypted data under role policy.
Trace: `/log/trace/scenario_8.log`

## 7. Sensor‑Driven World Adaptation
Live sensor input updates agent rules. The validator records rule changes.
Trace: `/log/trace/scenario_9.log`

## 8. Multi-Agent Physics Duel
Two Workers simulate opposing agents while the Queen enforces fairness.
Trace: `/log/trace/scenario_10.log`

### Additional Services
- **Kiosk Federation:** UI bundles served from `/srv/ui_bundle/kiosk_v1/` and triggered by `cohtrace kiosk_ping`.
- **Webcam Tilt:** Capture from `/dev/video0` feeds force values to a Rapier beam simulation.
- **GPU Swarm Registry:** Workers publish `gpu_capacity` and `latency_score` for scheduling; stored under `/srv/gpu_registry.json`.

All services emit trace logs to `/log/trace/` and snapshots to `/history/snapshots/` for replay and validator inspection.

### Retired or Minor Demos
- Smart Kiosk UI and InteractiveKiosk Local Demo were consolidated under **Kiosk Federation**.
- App Swap via QR Code merges with the **Cloud Queen + Home Orin** workflow.
- CLI-triggered deploys, Home Worker Auto-Attach, and Offline Edge Resilience are now covered by automated scripts and tests rather than dedicated demos.
