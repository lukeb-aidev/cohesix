<!-- Author: Lukas Bower -->
<!-- Purpose: Record whether Milestone 26c post-behavior external baseline has been frozen. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Post-Behavior Baseline

Status: `QEMU+PI4-FROZEN`

The QEMU post-behavior baseline is frozen for the authorized 26c behavior
changes. Pi 4 hardware closure is frozen for 26c by the final wired GENET
runtime/DMA proof bundle and target-qualified Pi Stage 01-05 pass.

QEMU-frozen behavior:

- VM worker-heart, worker-gpu, and worker-lora use bounded no_std loops.
- Implemented worker roles require generated endpoint-badge authority.
- Worker lifecycle uses generated notification badge classes for revoke,
  shutdown, lease-expiry, telemetry-pressure, and IRQ events.
- QEMU/non-MCS scheduling evidence is generated and rejects MCS budget claims on
  non-MCS profiles.
- Pi runtime/DMA proof states now distinguish target-build, diagnostic,
  qemu-or-stale-log, and fresh-pi evidence.

Pi-frozen behavior:

- Final Pi runtime/DMA proof produced `PI4_RUNTIME_DMA_PROOF=fresh-pi`,
  `PI4_RUNTIME_DMA_COUNTER_PROOF=counter-qualified`, and
  `DRIVER_TASK_DMA_BLOCKER=none` in
  `out/test-plan/m26c-pi4-live/pi4-runtime-dma-proof-genet-latest.env`.
- Target-qualified Pi Stage 01-05 passed in `out/test-plan/m26c-pi4-live`
  with `PI4_RUNTIME_DMA_PROOF_FILE` pointing at the live GENET proof bundle.
- Final Pi Stage 05 due diligence passed at `out/audit/gate/20260629T061204Z`.

Future refactor waves may compare against this frozen baseline only when the
touched surface is explicitly authorized by a later milestone and has its own
preserved-contract evidence. Broad host/root/HAL cleanup remains deferred
outside 26c.
