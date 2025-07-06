// CLASSIFICATION: PRIVATE
// Filename: SECURE_BOOT.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-31

# Secure Boot and TPM Integration

This document details how Cohesix performs kernel and OS verification during
boot. The process reads expected hashes from `/srv/boot/hashes.json` and writes
validation output to `/srv/boot/verify.log`. When hardware TPM is available,
`/srv/boot/attestation.log` records attestation data.

Unsupported hardware triggers a logged warning but does not halt boot. Queens may
enter a restricted role if verification fails.

All verification steps are traced via `cohtrace` and appear in `/log/trace/secure_boot_<ts>.log`. The validator inspects these logs during CI and failover simulation.

If verification fails on a Queen node, the role is downgraded to `RestrictedQueen`, which disables federation and write access to `/srv`.

Future upgrades will support remote attestation brokered by QueenPrimary using a secure gRPC interface.
