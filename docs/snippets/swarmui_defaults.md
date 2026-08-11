<!-- Author: Lukas Bower -->
<!-- Purpose: Generated SwarmUI defaults snippet consumed by docs/USERLAND_AND_CLI.md. -->
<!-- Copyright 2026 Lukas Bower -->

### SwarmUI defaults (generated)
- `manifest.sha256`: `8702c7c920c14b6449478c90ed34765787d2c3379a1ac305a4e98a99aa04ddd7`
- `swarmui.defaults.sha256`: `762f74169709822247b37e5ab20e1c44108c44adbba9e1558f9a8c9584f06aa7`
- `swarmui.ticket_scope`: `per-ticket`
- `swarmui.cache.enabled`: `false`
- `swarmui.cache.max_bytes`: `262144`
- `swarmui.cache.ttl_s`: `3600`
- `swarmui.hive.frame_cap_fps`: `30`
- `swarmui.hive.step_ms`: `16`
- `swarmui.hive.lod_zoom_out`: `0.7`
- `swarmui.hive.lod_zoom_in`: `1.25`
- `swarmui.hive.lod_event_budget`: `512`
- `swarmui.hive.snapshot_max_events`: `4096`
- `swarmui.hive.overlay_lines`: `3`
- `swarmui.hive.detail_lines`: `50`
- `swarmui.hive.line_cap_bytes`: `160`
- `swarmui.hive.per_worker_bytes`: `2048`
- `swarmui.hive.pending_lines_per_worker`: `64`
- `swarmui.hive.pending_event_cap`: `4096`
- `swarmui.hive.poll_workers_per_tick`: `32`
- `swarmui.hive.status_poll_ms`: `500`
- `swarmui.hive.degrade_pressure`: `1.0`
- `swarmui.paths.telemetry_root`: `/worker`
- `swarmui.paths.proc_ingest_root`: `/proc/ingest`
- `swarmui.paths.worker_root`: `/shard`
- `swarmui.paths.namespace_roots`: `/proc, /queen, /shard, /worker, /log, /gpu`
- `swarmui.worker_runtime.maximum_live_tasks`: `3`
- `swarmui.worker_runtime.canonical_telemetry_template`: `/shard/<label>/worker/<id>/telemetry`
- `swarmui.worker_runtime.shard_bits`: `8`
- `swarmui.worker_runtime.legacy_worker_alias`: `true`
- `swarmui.worker_runtime.role.worker-heartbeat`: declaration=`executable`, executable_slots=`1`
- `swarmui.worker_runtime.role.worker-gpu`: declaration=`executable`, executable_slots=`1`
- `swarmui.worker_runtime.role.worker-bus`: declaration=`model-only`, executable_slots=`0`
- `swarmui.worker_runtime.role.worker-lora`: declaration=`executable`, executable_slots=`1`
- `trace.max_bytes`: `1048576`

_Generated from `configs/root_task.toml` (sha256: `8702c7c920c14b6449478c90ed34765787d2c3379a1ac305a4e98a99aa04ddd7`)._
