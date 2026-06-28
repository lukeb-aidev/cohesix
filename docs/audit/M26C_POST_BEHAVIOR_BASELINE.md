<!-- Author: Lukas Bower -->
<!-- Purpose: Record whether Milestone 26c post-behavior external baseline has been frozen. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Post-Behavior Baseline

Status: `QEMU-FROZEN / PI4-HARDWARE-OPEN`

The QEMU post-behavior baseline is frozen for the authorized 26c behavior
changes. Pi 4 hardware closure is not frozen until a fresh live runtime/DMA
proof bundle and target-qualified Pi Stage 01-05 pass.

QEMU-frozen behavior:

- VM worker-heart, worker-gpu, and worker-lora use bounded no_std loops.
- Implemented worker roles require generated endpoint-badge authority.
- Worker lifecycle uses generated notification badge classes for revoke,
  shutdown, lease-expiry, telemetry-pressure, and IRQ events.
- QEMU/non-MCS scheduling evidence is generated and rejects MCS budget claims on
  non-MCS profiles.
- Pi runtime/DMA proof states now distinguish target-build, diagnostic,
  qemu-or-stale-log, and fresh-pi evidence.

Open before full 26c closure:

- Live Pi runtime/DMA proof must produce `PI4_RUNTIME_DMA_PROOF=fresh-pi`,
  `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`, and
  `DRIVER_TASK_DMA_BLOCKER=none`.
- Target-qualified Pi Stage 01-05 must pass with `PI4_RUNTIME_DMA_PROOF_FILE`
  pointing at the live proof bundle.

Phase 4 refactor waves may compare QEMU-only edits against this baseline only
when the touched surface is explicitly QEMU-scoped. Pi 4 HAL/network/local-seat
cleanup remains blocked by live hardware evidence.
