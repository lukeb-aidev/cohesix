<!-- Author: Lukas Bower -->
<!-- Purpose: Record whether Milestone 26c post-behavior external baseline has been frozen. -->
<!-- Copyright 2026 Lukas Bower -->

# M26C Post-Behavior Baseline

Status: `PI4-HISTORICAL-FROZEN / WORKER-BASELINE-SUPERSEDED`

The earlier QEMU Worker-execution baseline is superseded: its tests covered
helper/model behavior, not loaded Worker tasks. Pi 4 hardware closure remains
historical 26c evidence for the final wired GENET runtime/DMA proof bundle and
target-qualified Pi Stage 01-05 pass; it does not prove Worker execution.

QEMU-frozen behavior:

- Worker-heart, worker-gpu, and worker-lora expose bounded no_std helper loops,
  but current root-task does not load or resume them as Worker tasks.
- Every checked-in Worker role is non-executable; endpoint-cap and lifecycle
  notification requirements are disabled.
- Reserved badge ranges and the QEMU/non-MCS scheduling record are compiler
  metadata only, not live authority or applied Worker scheduling evidence.
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
