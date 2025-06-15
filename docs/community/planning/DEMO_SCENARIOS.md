// CLASSIFICATION: COMMUNITY
// Filename: DEMO_SCENARIOS.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-31

# Demo Scenarios

Below are condensed walkthroughs of the main Cohesix demos. Use `cohrun` or `cohcli` to execute them. Configuration files reside in the `examples/` directory.

## 1. Cloud Queen + Home Orin
Jetson Orin Nano streams webcam data to the Queen. Gestures trigger SLM payloads. Latency stays under 100 ms.
Trace: `/log/trace/scenario_1.log`

## 2. CUDA Inferencing at the Edge
Local YOLOv8 inference on the Orin with hot‑swappable models from the Queen.
Trace: `/log/trace/scenario_2.log`

## 3. Physics Sandbox Controlled by Webcam
Webcam tilt values drive a Rapier simulation. Results are validated via `cohtrace`.
Trace: `/log/trace/scenario_3.log`

## 4. Trace Validation Loop
Worker traces are replayed on the Queen to detect deviations in real time.
Trace: `/log/trace/scenario_4.log`

## 5. Smart Kiosk UI
Raspberry Pi kiosk fetches a UI bundle from the Queen (`cohrun kiosk_start`). Events log to `/srv/kiosk_federation.json`.
Trace: `/log/trace/scenario_5.log`

## 6. InteractiveKiosk Local Demo
Local demo of the InteractiveKiosk role on Raspberry Pi. Runs kiosk UI with mock event stream and logs interaction state to `/srv/kiosk_events.json`.
Trace: `/log/trace/scenario_6.log`

## 7. App Swap via QR Code
Scanning a QR code downloads a new SLM from the Queen and swaps the running app.
Trace: `/log/trace/scenario_7.log`

## 8. Secure Sensor Relay
A Worker declared as `SensorRelay` streams encrypted data under role policy.
Trace: `/log/trace/scenario_8.log`

## 9. Sensor‑Driven World Adaptation
Live sensor input updates agent rules. The validator records rule changes.
Trace: `/log/trace/scenario_9.log`

## 10. Multi-Agent Physics Duel
Two Workers simulate opposing agents while the Queen enforces fairness.
Trace: `/log/trace/scenario_10.log`

## 11. Codex‑Triggered Demo via CLI
`cohrun physics_demo_3` deploys and runs a full scenario from the command line.
Trace: `/log/trace/scenario_11.log`

## 12. Home Worker Auto-Attach
Rendezvous service allows a home Worker to securely join the Queen with zero config.
Trace: `/log/trace/scenario_12.log`

## 13. Offline Edge Resilience
If the Queen disappears, a Worker promotes itself to `EdgeFallbackCoordinator` until connectivity returns.
Trace: `/log/trace/scenario_13.log`

### Additional Services
- **Kiosk Federation:** UI bundles served from `/srv/ui_bundle/kiosk_v1/` and triggered by `cohtrace kiosk_ping`.
- **Webcam Tilt:** Capture from `/dev/video0` feeds force values to a Rapier beam simulation.
- **GPU Swarm Registry:** Workers publish `gpu_capacity` and `latency_score` for scheduling; stored under `/srv/gpu_registry.json`.

All services emit trace logs to `/log/trace/` and snapshots to `/history/snapshots/` for replay and validator inspection.
