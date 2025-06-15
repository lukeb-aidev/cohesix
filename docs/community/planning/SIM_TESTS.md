// CLASSIFICATION: COMMUNITY
// Filename: SIM_TESTS.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-31

# Simulation Snapshot & Replay Tests

This guide documents the deterministic simulation harness and snapshot format.

## Snapshot Format

Snapshots are written to `/sim/world.json` and contain:

```json
{
  "step": <u64>,
  "bodies": [
    {
      "index": <u32>,
      "generation": <u32>,
      "position": [x, y, z],
      "velocity": [vx, vy, vz],
      "rotation": [ix, jy, kz, w]
    }
  ]
}
```

Trace files corresponding to snapshot runs are saved under `/log/trace/sim_<timestamp>.log` and include every force, state change, and agent action.

## Replay Instructions

1. Run any simulation that creates `/sim/world.json`.
2. Copy the file to a new environment.
3. On startup, `SimBridge` will load the snapshot and resume from the saved step.
4. The deterministic harness can be invoked using `deterministic_harness(seed, steps)`.

Logs for each run are stored under `/srv/trace/sim.log` and should be identical across architectures when using the same seed.

Snapshot and trace consistency is validated automatically by the CI harness using `cohtrace diff`.
