// CLASSIFICATION: COMMUNITY
// Filename: GPU_SWARM.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

# GPU Swarm Coordination

This document describes the basic GPU swarm registry used in demo deployments.
Workers advertise `gpu_capacity`, `current_load`, and `latency_score` to the Queen.
The queen writes these entries to `/srv/gpu_registry.json` for scheduling.
