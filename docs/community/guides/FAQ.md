// CLASSIFICATION: COMMUNITY  
// Filename: FAQ v1.0  
// Author: Lukas Bower  
// Date Modified: 2025-06-15

# ⚠️ Cohesix: Skeptic's FAQ  
_“If it’s so great, prove it.”_

---

### 🔹 Q1: What exactly is Cohesix — and why does the world need *yet another OS*?

**A:**  
Cohesix is a minimal, role-aware operating system built for a world where AI, sensors, and distributed agents run at the edge — not just in the cloud. It's grounded in **military-grade security** (via the formally verified seL4 kernel), but engineered for **speed, autonomy, and AI-native compute**.

Unlike general-purpose Linux distros, Cohesix is:
- **Cold-boot fast** (<200ms)
- **GPU-aware** (it can use NVIDIA CUDA where available to accelerate agents)
- **Physically grounded** (agents can reason about movement, space, and time via a built-in physics engine)
- **Trace-validating** (every syscall and event can be audited in real time)

It’s built on a **Beehive paradigm**:
- **Queens** provide secure orchestration
- **Workers** guided by queen, can act autonomously at the edge
- **Sensors, Kiosks, and Drones** each serve defined roles, preconfigured by a role manifest

Each role in the manifest — from `QueenPrimary` to `SensorRelay` — represents a real-world deployment need:
- Cloud-scale AI tasking
- On-device inference
- Physical world interaction
- Secure sandbox execution

The world doesn’t need another Linux — it needs a **small, verifiable, AI-native OS that works in a world of intelligent agents and untrusted networks**. That’s Cohesix.

---

### 🔹 Q2: Isn’t this just hobbyist Linux re-skinned with ASCII art?

**A:**  
No. Cohesix runs on the **seL4 microkernel**, which is **formally verified** — meaning it's mathematically proven to prevent whole classes of memory and timing bugs.  
It does not use systemd, bash, or standard Linux init. It runs a minimal userland derived from **Plan 9 + Busybox**, and mounts everything via a **9P namespace** with sandboxed service exposure.

The ASCII art is just the tip of a verified iceberg.

---

### 🔹 Q3: What’s wrong with Linux + Docker + Kubernetes?

**A:**  
For cloud workloads? Nothing — they dominate.  
For **secure edge AI, autonomous robotics, or latency-critical physical systems?** Plenty:

- Linux is bloated and full of legacy attack surfaces.
- Docker is coarse-grained and breaks down in real-time environments.
- Kubernetes was never built for sub-second cold boots or physical-world feedback loops.

Cohesix was built from scratch to **run small, fast, verifiable agent workloads with physics awareness, telemetry capture, and trust enforcement.** You don’t get that by gluing containers to Linux.

---

### 🔹 Q4: Prove it’s secure.

**A:**  
The kernel is based on **seL4**, which is the world’s most verified microkernel — used in defense systems and aerospace.

Security is enforced via:
- **Formal proof** of kernel isolation and correctness.
- **Immutable roles (`/srv/cohrole`)** to constrain agent privileges.
- **Runtime syscall validation** (every agent is governed).
- **9P sandboxing** of service mounts.

Want proof? Run `cohtrace` and inspect the full validator-enforced trace of every syscall and sensor event.

---

### 🔹 Q5: Who's behind this?

**A:**  
A current Big Four Partner with a deep background in AI systems architecture, backed by Codex-driven development and contributions from senior systems engineers.  
No VC fluff, no fake roadmap — just working code and real machines booting fast, securely, and verifiably.

We don’t overhype — we ship.

---

### 🔹 Q6: Can it run CUDA or physics engines like Rapier?

**A:**  
Yes. Cohesix includes:
- `/srv/cuda` interface for on-board NVIDIA Jetson-class acceleration.
- `/sim/` interface backed by Rapier physics engine for real-world grounding.
- Automatic fallback and logging if hardware is unavailable.

This means **edge agents can reason about space, time, force — and offload heavy math to GPU** where available.

---

### 🔹 Q7: Is this production-ready?

**A:**  
It’s alpha-grade, but already boots and runs tasks on:
- Jetson Orin Nano (CUDA + Rapier)
- Raspberry Pi 5
- AWS Graviton / x86_64 EC2 (Queen orchestration)
- QEMU virtual environments (test harness)

**Tests are running. Traces are captured.** This isn’t a concept — it’s a functioning prototype. A production track is underway with watchdogs, hydration checks, and zero-stub policies.

---

### 🔹 Q8: Is it open source?

**A:**  
Yes — everything is licensed under **Apache 2.0**, **MIT**, or **BSD**. No GPL contamination. All dependencies are tracked in `OSS_REUSE.md`, and SBOM generation is built into the CI pipeline.

---

### 🔹 Q9: What if the main dev gets hit by a bus?

**A:**  
The project is fully documented via Codex and milestone bundles. Every milestone is hydrated and archived.  
Codex can rebuild and rehydrate the system automatically using instructions and structured prompts, thanks to:
- Canonical `INSTRUCTION_BLOCK.md`
- Versioned boot plans
- Prompt-driven regeneration

This is **bus-proofed**, version-controlled, and modular.

---

### 🔹 Q10: Sounds ambitious. What’s the catch?

**A:**  
It’s early.  
Some userland features are still light. GUI orchestration is primitive. CLI tools are improving but not rich. Community is small (but growing).  
However, **architecture, security model, and role-based isolation are rock-solid**.

You’re seeing the foundation of something built to last — not a hype balloon.
