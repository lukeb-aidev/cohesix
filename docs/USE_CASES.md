<!-- Author: Lukas Bower -->
# USE_CASES.md
Author: Lukas Bower — October 15, 2025

## Purpose
This document enumerates concrete, high‑value **use cases** for Cohesix across sectors, highlighting why the platform is a fit, what (if anything) needs to be added, and any notable compliance or operational constraints.

---

## Edge & Industrial

### 1) Smart‑factory / Industrial IoT gateway
**Why Cohesix:** tiny TCB (seL4), strong isolation between PLC/robot cells, append‑only telemetry, offline‑first.  
**Needs:** MODBUS/CAN sidecars; telemetry ring buffers + cursors; QUIC uplink (gateway/host).  
**Constraints:** Deterministic timing, safety certification paths.

### 2) Energy substation / Micro‑grid controller
**Why:** deterministic scheduling, minimal attack surface at OT/IT boundary.  
**Needs:** DNP3/IEC‑104 adapters; signed config updates; GPS/PTP time beacons.  
**Constraints:** NERC/CIP, IEC 61850 contexts.

### 3) Retail / Computer‑vision hub (store analytics)
**Why:** private LAN for cameras/Jetsons; secure UEFI worker as the only WAN node.
**Needs:** content‑addressed model updates; CBOR telemetry; local summarization.
**Constraints:** Privacy/PII handling at edge.
Each store deployment is a hive with one Queen coordinating many workers via `cohsh` or a GUI client that speaks the same protocol, running on physical ARM64 hardware booted via UEFI; QEMU remains the development/QA harness.

### 4) Logistics & ports (ALPR, container ID, crane safety)
**Why:** harsh networks, need resilient telemetry & updates.
**Needs:** durable disk spooling; batch uploads; ring buffers.
**Constraints:** Physical security, RF noise.
Hives with a single Queen orchestrate multiple workers across yard devices, commanded through `cohsh` or compatible clients on physical ARM64 hardware, with QEMU used during development to mirror deployment behaviour.

### 5) Telco MEC micro‑orchestrator
**Why:** coordinate accelerators at cell sites; capability tickets; multi‑tenant scheduling.
**Needs:** SR‑IOV/NIC telemetry sidecars; per‑tenant quotas; shard namespaces.
**Constraints:** Carrier‑grade Ops, slice isolation.
Each MEC node is a hive (one Queen, many workers and GPU workers) steered through `cohsh` or GUI tooling that reuses the same protocol, hosted on physical ARM64 hardware booted via UEFI; QEMU is reserved for dev/CI equivalence testing.

### 6) Healthcare imaging edge → cloud PACS
**Why:** minimize PHI footprint, deterministic control plane.  
**Needs:** DICOM proxy; de‑identification; audit‑grade append logs.  
**Constraints:** HIPAA/ISO 27001, locality of data.

### 7) Autonomous depots (AV/AGV fleets)
**Why:** bandwidth‑aware model deltas; offline autonomy.
**Needs:** CAS manifests, delta packs; multicast to many vehicles.
**Constraints:** Safety, predictable update windows.
Depot controllers run as hives, with the Queen coordinating many workers and GPU workers via `cohsh`-driven flows on physical ARM64 hardware; the QEMU harness mirrors these deployments during development.

### 8) Defense ISR kits / forward ops
**Why:** seL4 assurance, LoRa for low‑bandwidth control.  
**Needs:** LoRa scheduler; tamper logging; rapid key rolls.  
**Constraints:** Export controls, contested networks.

### 9) Smart‑city sensing (air/noise/traffic)
**Why:** many cheap sensors behind a single secure gateway.  
**Needs:** sensor bus sidecars (I2C/SPI); coarse summarization before uplink.  
**Constraints:** Public data, OTA safety.

### 10) Broadcast/DOOH signage controller
**Why:** signed content updates, simple auditable playback.
**Needs:** CAS assets + schedule provider; proof‑of‑display receipts.
**Constraints:** Bandwidth caps, SLA reporting.
Each signage hub is a hive with one Queen orchestrating multiple workers, all commanded through `cohsh` or GUI clients that speak the same protocol on physical ARM64 hardware, validated during development on the QEMU reference board.

---

## Security & Fintech

### 11) HSM‑adjacent signing gateway
**Why:** auditable control in front of HSMs/KMS/Enclaves.  
**Needs:** sign/verify provider; rate/role caps; immutable logs.  
**Constraints:** FIPS modes, key custody.

### 12) OT/IT segmentation appliance
**Why:** formally small boundary device; tickets instead of VPN sprawl.  
**Needs:** dual‑NIC profile; policy compiler → AccessPolicy; ring telemetry.  
**Constraints:** Audits, change control.

---

## Science & Remote Ops

### 13) Environmental science stations (polar, offshore)
**Why:** limited power/links; need store‑and‑forward.  
**Needs:** delay‑tolerant queues; trickle CAS updates; clock beacons.  
**Constraints:** Power budget, severe weather.

### 14) HAPS/satellite ground gateway
**Why:** deterministic, low‑memory control processes.  
**Needs:** CCSDS/TCP bridge; very high‑latency backpressure tuning.  
**Constraints:** Link budgets, long RTTs.

---

## Developer & Platform Tooling

### 15) “Secure OTA lab” appliance
**Why:** demonstrate signed content → staged apply → A/B rollback with attestation.  
**Needs:** golden‑image verifier; CLI scripts; dashboards.

### 16) Classroom OS/security labs
**Why:** small, readable microkernel userland; 9P surfaces ideal for labs.  
**Needs:** mock transports; fuzz harnesses; trace viewer.

---

## Cross‑cutting capabilities that unlock many use cases
- **9P scalability upgrades:** pipelining, batching (framed CBOR), sharded namespaces, ring buffers with cursors, short‑write backpressure.  
- **Tickets & leases:** signed capability tokens with TTL/scopes/rate‑limits; revocation.  
- **Content‑addressed updates (CAS):** Merkle manifests, delta packs, resumable fetches.  
- **Gateway API (optional):** HTTP/3 + JSON/CBOR on std host/sidecar; keep VM no_std.  
- **Edge identity:** UEFI Secure Boot + TPM attest for workers; device keys and enrollment.
