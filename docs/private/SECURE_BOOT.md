// CLASSIFICATION: PRIVATE
// Filename: SECURE_BOOT.md v1.0
// Author: Codex
// Date Modified: 2025-06-07

# Secure Boot and TPM Integration

This document details how Cohesix performs kernel and OS verification during
boot. The process reads expected hashes from `/srv/boot/hashes.json` and writes
validation output to `/srv/boot/verify.log`. When hardware TPM is available,
`/srv/boot/attestation.log` records attestation data.

Unsupported hardware triggers a logged warning but does not halt boot. Queens may
enter a restricted role if verification fails.
