// CLASSIFICATION: PRIVATE
// Filename: Q_DAY.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

# Q-Day Preparation for Cohesix

Q-Day refers to the projected future date when quantum computers will be capable of breaking classical encryption methods such as RSA and ECC. This event would render vast swaths of existing digital infrastructure insecure, particularly where long-term confidentiality or identity is at stake.

While timelines vary, conservative estimates suggest a plausible window between 2030 and 2045. Given the potential disruption, Cohesix must incorporate forward-compatible mechanisms to maintain post-quantum resilience across its architecture.

## 1 · Threat Surface

The primary vectors affected by Q-Day are:
- Public key cryptography (e.g., TLS, SSH, signing keys)
- Software update signing and SBOM provenance
- Encrypted telemetry or log archives
- Long-lived key materials in Queen/Worker boot identity
- Certificate-based API or agent validation

## 2 · Preparation Plan

To prepare Cohesix for Q-Day, we will implement the following:

### 2.1 · Algorithm Agility

All cryptographic modules must support algorithm agility. Keys, signatures, and encryption schemes should be replaceable at runtime or during patch cycles without hardcoding dependencies.

- TLS: Enable hybrid classical + post-quantum ciphersuites (e.g., Kyber + AES)
- Signing: Integrate post-quantum signing (Dilithium, Falcon) into build pipelines
- Agent Manifest: Support PQC upgrade flag and dual-signature mode

### 2.2 · Key Isolation and Rotation

- Ensure all Worker and Queen boot identity keys are rotatable and non-persistent across trust domains.
- Embed rotation hooks into validator and sandbox runtime.

### 2.3 · Dependency Scrutiny

- Audit all crypto-related dependencies for PQC roadmap or agility support.
- Prefer crates or libraries with NIST Round 3 candidates or migration plans.

### 2.4 · Encrypted Archives

- Avoid long-lived encrypted blobs unless sealed with PQC.
- Archive trace logs and telemetry with key expiration logic.

## 3 · Simulation and Testing

- Add Q-Day failure simulation mode in SimulatorTest role
- Run integrity revalidation with revoked legacy crypto
- Benchmark PQC-ready boot and agent validation pipelines
- Track NIST Post-Quantum Cryptography Standardization (FIPS 203–205), ETSI quantum-safe recommendations, and BSI TR-02102 guidelines; incorporate published vectors and validation criteria into Cohesix test harnesses as they become available.

## 4 · Timeline and Trigger

- Begin simulation tests by 2026
- Migrate key validation paths to hybrid/PQC by 2028
- Freeze legacy crypto paths by 2029, pending industry alignment

## 5 · Strategy Summary

Cohesix is built with strong isolation and small TCBs, which positions it well for post-quantum upgrades. However, its reliance on cryptographic signatures for agent trust, trace validation, and inter-role coordination makes it vulnerable to Q-Day risks.

By implementing agile cryptographic design, proactive simulation, and PQC-readiness audits now, we ensure that Cohesix remains trustworthy even in a post-quantum world.

> All crypto interface code must be tagged for PQC upgrade audit by Q1 2026.