// CLASSIFICATION: COMMUNITY
// Filename: DEMO_SCENARIOS.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-12

# Demo Scenarios

These demos showcase key Cohesix features across Queen and Worker roles.

1. **Cloud Queen + Home Orin:** Webcam loop with <100 ms latency.
2. **CUDA Inferencing:** YOLOv8 model runs on the Orin with hot‑swap from Queen.
3. **Physics Sandbox:** Webcam tilt controls a Rapier simulation.
4. **Trace Validation Loop:** Queen replays and verifies Worker traces.
5. **Smart Kiosk:** Remote UI deployment on a Raspberry Pi kiosk.
6. **App Swap via QR Code:** Scan a QR to load a new SLM instantly.
7. **Secure SensorRelay:** Encrypted sensor streaming with role policy.
8. **Sensor‑Driven World Adaptation:** Live sensors modify agent behavior.
9. **Multi‑Agent Physics Duel:** Competing agents validated for fairness.
10. **Codex‑Triggered Demo:** CLI triggers a full scenario via `cohrun`.
11. **Home Worker Auto‑Attach:** NAT rendezvous connects Worker to Queen.
12. **Offline Edge Resilience:** Worker promotes itself if Queen is unreachable.

### Additional Components
- **GPU Swarm Coordination:** Workers advertise `gpu_capacity` and `latency_score`; Queen stores results in `/srv/gpu_registry.json`.
- **Kiosk Federation:** UI bundles pulled from `/srv/ui_bundle/` and deployed by `cohrun kiosk_start`.
- **Webcam Tilt Service:** `/dev/video0` offsets drive a Rapier beam balance; validation reports stored under `/trace/reports/`.

Use these scenarios to demonstrate Cohesix at meetups or conferences.
