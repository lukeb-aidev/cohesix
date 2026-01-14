<!-- Author: Lukas Bower -->
<!-- Purpose: Generated manifest snippet consumed by docs/ARCHITECTURE.md. -->

### Root-task manifest schema (generated)
- `meta.author`: `Lukas Bower`
- `meta.purpose`: `Root-task manifest input for coh-rtc.`
- `root_task.schema`: `1.4`
- `profile.name`: `virt-aarch64`
- `profile.kernel`: `true`
- `event_pump.tick_ms`: `5`
- `secure9p.msize`: `8192`
- `secure9p.walk_depth`: `8`
- `secure9p.tags_per_session`: `16`
- `secure9p.batch_frames`: `1`
- `secure9p.short_write.policy`: `reject`
- `cas.enable`: `true`
- `cas.store.chunk_bytes`: `128`
- `cas.delta.enable`: `true`
- `cas.signing.required`: `true`
- `cas.signing.key_path`: `resources/fixtures/cas_signing_key.hex`
- `telemetry.ring_bytes_per_worker`: `1024`
- `telemetry.frame_schema`: `legacy-plaintext`
- `telemetry.cursor.retain_on_boot`: `false`
- `observability.proc_9p.sessions`: `true`
- `observability.proc_9p.outstanding`: `true`
- `observability.proc_9p.short_writes`: `true`
- `observability.proc_9p.sessions_bytes`: `8192`
- `observability.proc_9p.outstanding_bytes`: `128`
- `observability.proc_9p.short_writes_bytes`: `128`
- `observability.proc_ingest.p50_ms`: `true`
- `observability.proc_ingest.p95_ms`: `true`
- `observability.proc_ingest.backpressure`: `true`
- `observability.proc_ingest.dropped`: `true`
- `observability.proc_ingest.queued`: `true`
- `observability.proc_ingest.watch`: `true`
- `observability.proc_ingest.p50_ms_bytes`: `64`
- `observability.proc_ingest.p95_ms_bytes`: `64`
- `observability.proc_ingest.backpressure_bytes`: `64`
- `observability.proc_ingest.dropped_bytes`: `64`
- `observability.proc_ingest.queued_bytes`: `64`
- `observability.proc_ingest.watch_max_entries`: `16`
- `observability.proc_ingest.watch_line_bytes`: `192`
- `observability.proc_ingest.watch_min_interval_ms`: `50`
- `observability.proc_ingest.latency_samples`: `32`
- `observability.proc_ingest.latency_tolerance_ms`: `5`
- `observability.proc_ingest.counter_tolerance`: `1`
- `client_policies.cohsh.pool.control_sessions`: `2`
- `client_policies.cohsh.pool.telemetry_sessions`: `4`
- `client_policies.retry.max_attempts`: `3`
- `client_policies.retry.backoff_ms`: `200`
- `client_policies.retry.ceiling_ms`: `2000`
- `client_policies.retry.timeout_ms`: `5000`
- `client_policies.heartbeat.interval_ms`: `15000`
- `cache.kernel_ops`: `true`
- `cache.dma_clean`: `true`
- `cache.dma_invalidate`: `true`
- `cache.unify_instructions`: `false`
- `features.net_console`: `true`
- `features.serial_console`: `true`
- `features.std_console`: `false`
- `features.std_host_tools`: `false`
- `namespaces.role_isolation`: `true`
- `sharding.enabled`: `true`
- `sharding.shard_bits`: `8`
- `sharding.legacy_worker_alias`: `true`
- `tickets`: 3 entries
- `manifest.sha256`: `99878893a38c8b0b632e10d1f9f39973eb1a9fea97bc4be58c963e4be946f196`

### Namespace mounts (generated)
- (none)

### Sharded worker namespace (generated)
- `sharding.enabled`: `true`
- `sharding.shard_bits`: `8`
- `sharding.legacy_worker_alias`: `true`
- shard labels: `00..ff` (count: 256)
- canonical worker path: `/shard/<label>/worker/<id>/telemetry`
- legacy alias: `/worker/<id>/telemetry`

### Ecosystem section (generated)
- `ecosystem.host.enable`: `false`
- `ecosystem.host.mount_at`: `/host`
- `ecosystem.host.providers`: `(none)`
- `ecosystem.audit.enable`: `false`
- `ecosystem.audit.journal_max_bytes`: `8192`
- `ecosystem.audit.decisions_max_bytes`: `4096`
- `ecosystem.audit.replay_enable`: `false`
- `ecosystem.audit.replay_max_entries`: `64`
- `ecosystem.audit.replay_ctl_max_bytes`: `1024`
- `ecosystem.audit.replay_status_max_bytes`: `1024`
- `ecosystem.policy.enable`: `false`
- `ecosystem.policy.queue_max_entries`: `32`
- `ecosystem.policy.queue_max_bytes`: `4096`
- `ecosystem.policy.ctl_max_bytes`: `2048`
- `ecosystem.policy.status_max_bytes`: `512`
- `ecosystem.policy.rules`: `queen-ctl` → `/queen/ctl`
- `ecosystem.policy.rules`: `systemd-restart` → `/host/systemd/*/restart`
- `ecosystem.models.enable`: `false`
- Nodes appear only when enabled.

_Generated from `configs/root_task.toml` (sha256: `99878893a38c8b0b632e10d1f9f39973eb1a9fea97bc4be58c963e4be946f196`)._
