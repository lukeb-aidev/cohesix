// CLASSIFICATION: PRIVATE
// Filename: THREAT_MODEL.md v1.1
// Author: Lukas Bower
// Date Modified: 2025-07-31

# Threat Model for Cohesix

This file consolidates the previous `docs/security/THREAT_MODEL.md` and `docs/private/THREAT_MODEL.md`. It identifies assets, threat agents, attack vectors, and mitigations for the Cohesix platform.


## 1. Assets

| Asset                      | Description                                     | Security Property       |
|----------------------------|-------------------------------------------------|-------------------------|
| Bootloader binary          | seL4-based boot code                            | Integrity, Availability |
| IR & Pass Framework        | Compiler intermediate representation logic      | Integrity               |
| Plan 9 Namespace Service   | 9P server for file/system access                | Confidentiality, Integrity |
| GPU Offload Service        | CUDA/TensorRT runtime container                  | Integrity, Availability |
| Userland Utilities         | BusyBox & POSIX shims                            | Integrity, Availability |
| Codex Agent Instructions   | AGENTS.md and orchestrator scripts               | Integrity, Confidentiality |
| Dependency manifest        | DEPENDENCIES.md with version pins                | Integrity               |

## 2. Threat Agents

| Agent Type            | Motivation                 | Capabilities                          |
|-----------------------|----------------------------|---------------------------------------|
| Malicious Insider    | Sabotage, data leak         | Full local system access              |
| Remote Attacker      | Ransom, code execution      | Network access to service endpoints   |
| Supply-Chain Adversary | Compromise third-party deps | Repository or package registry access |
| Software Bug         | Unintentional vulnerability | No special privileges                 |

## 3. Attack Vectors & Use Cases

| #  | Vector                           | Description                                                 | Affected Asset             |
|----|----------------------------------|-------------------------------------------------------------|----------------------------|
| AV1| Bootloader tampering             | Attacker alters bootloader image in storage                 | Bootloader binary          |
| AV2| IR manipulation                  | Malicious IR input to subvert compiler behavior             | IR & Pass Framework        |
| AV3| 9P mount spoofing               | Unauthorized file access via manipulated 9P mounts          | Plan 9 Namespace Service   |
| AV4| GPU side channel                | Extract sensitive data via CUDA side-channel attacks       | GPU Offload Service        |
| AV5| OCSP/Dependency compromise      | Malicious package or pinned version vulnerability          | Dependency manifest        |
| AV6| Agent instruction injection     | Malicious AGENTS.md causes Codex to execute harmful tasks  | Codex Agent Instructions   |

## 4. Mitigations

| Threat              | Mitigation                                                       |
|---------------------|------------------------------------------------------------------|
| Bootloader tampering| - Sign bootloader images using seL4-verified key                  |
| IR manipulation     | - Validate IR schema; sandbox compiler front-end                 |
| 9P spoofing         | - Authenticate 9P clients; use capability-based access controls  |
| GPU side channel    | - Limit precision; introduce noise; isolate workloads             |
| Dependency compromise| - Enforce SCA scanning; pin to known-good hashes in DEPENDENCIES.md |
| Instruction injection| - Lint AGENTS.md; require well-formed entries; code review      |
| Trace and replay tampering | - Sign all trace logs and snapshots; verify in CI and replay using validator  |

## 5. Risk Assessment

- **High**: Bootloader tampering, Supply-chain compromise
- **Medium**: IR manipulation, 9P spoofing, Agent injection
- **Low**: GPU side channels (mitigations reduce feasibility)
- **Medium**: Trace tampering (mitigated via signature and replay enforcement)

## 6. Review & Updates

This threat model is reviewed quarterly or after any significant incident. Updates are versioned in `docs/security/THREAT_MODEL.md` and recorded in `CHANGELOG.md`.
