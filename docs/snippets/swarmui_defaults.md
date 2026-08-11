<!-- Author: Lukas Bower -->
<!-- Purpose: Generated SwarmUI defaults snippet consumed by docs/USERLAND_AND_CLI.md. -->
<!-- Copyright 2026 Lukas Bower -->

### SwarmUI defaults (generated)
- `manifest.sha256`: `a19ad1fb83f549ef46780ef1066d750b72331ddc3988adb9a13a164ced857cfa`
- `swarmui.defaults.sha256`: `b0dc523ba8540ee3ad0eb6453067a6d84d130bbfc1d7a2e40cfe3194376eb9ec`
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

_Generated from `configs/root_task.toml` (sha256: `a19ad1fb83f549ef46780ef1066d750b72331ddc3988adb9a13a164ced857cfa`)._
