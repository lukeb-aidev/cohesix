<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh client snippet consumed by docs/USERLAND_AND_CLI.md. -->
<!-- Copyright 2026 Lukas Bower -->

### cohsh client defaults (generated)
- `manifest.sha256`: `cdbfdfa9f4de5c1cd8f8f9ef7233aff9465e15e5469cce6604bdde50872996ba`
- `worker.task_abi_schema`: `worker-task-abi/v2`
- `worker.task_abi_version`: `2`
- `worker.observation_schema`: `cohesix-worker-observation/v1`
- `worker.integration_evidence_schema`: `cohesix-worker-integration-evidence/v1`
- `worker.maximum_live_tasks`: `256`
- `worker.canonical_telemetry_template`: `/shard/<label>/worker/<id>/telemetry`
- `worker.shard_bits`: `6`
- `worker.legacy_worker_alias`: `true`
- `worker.lifecycle`: `absent, queued, starting, ready, closing, faulted, terminal`
- `worker.receipt`: `none, pending, confirmed, rejected, stale`
- `worker.artifact`: `missing, verified, mismatch`
- `worker.execution_proof`: `none, host-model, qemu, fresh-pi`
- `worker.role.worker-heartbeat`: declaration=`executable`, executable_slots=`1`
- `worker.role.worker-gpu`: declaration=`executable`, executable_slots=`127`
- `worker.role.worker-bus`: declaration=`model-only`, executable_slots=`0`
- `worker.role.worker-lora`: declaration=`executable`, executable_slots=`128`
- `secure9p.msize`: `8192`
- `secure9p.walk_depth`: `8`
- `trace.max_bytes`: `1048576`
- `client_paths.queen_ctl`: `/queen/ctl`
- `client_paths.queen_lifecycle_ctl`: `/queen/lifecycle/ctl`
- `client_paths.queen_schedule_ctl`: `/queen/schedule/ctl`
- `client_paths.queen_lease_ctl`: `/queen/lease/ctl`
- `client_paths.queen_export_ctl`: `/queen/export/ctl`
- `client_paths.policy_ctl`: `/policy/ctl`
- `client_paths.log`: `/log/queen.log`
- `telemetry_ingest.max_segments_per_device`: `4`
- `telemetry_ingest.max_bytes_per_segment`: `131072`
- `telemetry_ingest.max_total_bytes_per_device`: `524288`
- `telemetry_ingest.max_reference_entries_per_segment`: `1024`
- `telemetry_ingest.max_reference_manifest_bytes_per_segment`: `131072`
- `telemetry_ingest.max_reference_bytes_per_segment`: `1073741824`
- `telemetry_ingest.eviction_policy`: `evict-oldest`

_Generated from `configs/root_task.toml` (sha256: `cdbfdfa9f4de5c1cd8f8f9ef7233aff9465e15e5469cce6604bdde50872996ba`)._
