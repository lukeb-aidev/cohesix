// CLASSIFICATION: COMMUNITY
// Filename: END_USER_DEMOS.md v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-12

# Cohesix End User Demos

> These demo scenarios are designed to showcase the capabilities of Cohesix in real-world edge/cloud configurations. The goal is to impress community users, excite developers, and dazzle investors with the unique features of the Queen–Worker architecture, CUDA + Rapier integration, real-time 9P orchestration, and secure role-based execution.

---

## \ud83c\udf1f 1. Cloud Queen + Home Orin: AI Webcam Loop

**Scenario:** A community member connects their Jetson Orin Nano and USB webcam to the Cohesix cloud Queen.

**Demo:** The Orin captures webcam input and reacts in real time to visual gestures (e.g. colored glove, marker card). The Queen triggers different SLM payloads (e.g. physics sim, CUDA model) based on what the camera sees. Telemetry streams back live.

**Wow Factor:** Live feedback loop with visible latency <100ms, 9P namespace exposed on Queen.

---

## \ud83d\ude80 2. CUDA Inferencing at the Edge

**Scenario:** The Orin Nano runs a YOLOv8 or custom CUDA-optimized model locally.

**Demo:** A webcam feed is analyzed to detect people, pets, or objects. The Queen can hot-swap models or stream the results.

**Wow Factor:** Hardware acceleration auto-detected; model deployed from Queen in under 5 seconds.

---

## \ud83e\uddea 3. Physics Sandbox Controlled by Webcam

**Scenario:** Live webcam input is mapped to control parameters in a Rapier physics simulation (e.g. beam balance).

**Demo:** A user tilts a printed card left/right; this affects torque in a sim on the Orin.

**Wow Factor:** Physical interaction with virtual simulation via home equipment.

---

## \ud83d\udd01 4. Trace Validation Loop

**Scenario:** A simulation runs on a Worker, with results sent to Queen for replay validation.

**Demo:** User triggers a physics scenario. Queen replays traces, detects deviation or trust zone breach.

**Wow Factor:** Governance + debugging visible; trace replay happens live with user prompt.

---

## \ud83e\udd11 5. Smart Kiosk (KioskInteractive Role)

**Scenario:** Raspberry Pi in a kiosk runs a UI and connects to the Queen.

**Demo:** UI is remotely deployed. A user inserts a card; the kiosk infers data locally, submits to Queen for further processing.

**Wow Factor:** Remote provisioning + real-time data streaming to Queen. Swap interface components live.

---

## \ud83d\udce6 6. App Swap via QR Code

**Scenario:** A Worker scans a QR code that encodes a Cohesix namespace or app profile.

**Demo:** Scanning the QR triggers an SLM download from the Queen. App changes on the Worker instantly.

**Wow Factor:** No manual config. Instant deployment from scan to execution.

---

## \ud83d\udd10 7. Secure Role Demo: Queen + SensorRelay

**Scenario:** A Worker is declared as `SensorRelay`. It streams encrypted sensor data.

**Demo:** Webcam, accelerometer, or temperature data is gated by role policy; Queen can inspect or deny.

**Wow Factor:** Live enforcement of trust policy, real-time security visualized.

---

## \ud83c\udf81 8. Multi-Agent Physics Duel

**Scenario:** Two Workers simulate competing agents (e.g. Tilt Duel). Queen governs the scenario.

**Demo:** Agents push against each other in Rapier. Queen validates fairness, logs rules.

**Wow Factor:** Competitive agent simulation with real-time trace and fairness logic.

---

## \ud83c\udfae 9. Codex-Triggered Demo via CLI

**Scenario:** Codex CLI sends commands to Queen to start a scenario on the Worker.

**Demo:** `cohrun physics_demo_3` invokes a chain of deployment + execution + trace replay.

**Wow Factor:** End-to-end control from terminal. Codex-CLI visible as developer power tool.

---

## \ud83c\udf10 10. Home Worker on NAT, Auto-Attach

**Scenario:** A user runs a Worker on home Wi-Fi. No static IP or config needed.

**Demo:** Worker discovers Queen in cloud and joins securely via rendezvous service. Namespace appears in Queen within 3 seconds.

**Wow Factor:** Zero-config edge-to-cloud bootstrapping. "It just works."

---

## \ud83d\udfe1 11. Offline Edge Resilience

**Scenario:** The Queen becomes unreachable mid-session.

**Demo:** A Worker automatically promotes itself to `EdgeFallbackCoordinator` and serves cached SLMs locally. Once the Queen returns, the worker hands control back.

**Wow Factor:** Seamless continuity even when cloud connectivity drops.

---

> Want to contribute your own demo? Fork, build, and share in the community.

