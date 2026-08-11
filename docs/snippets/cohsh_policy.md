<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->
<!-- Copyright 2026 Lukas Bower -->

### cohsh client policy (generated)
- `manifest.sha256`: `8702c7c920c14b6449478c90ed34765787d2c3379a1ac305a4e98a99aa04ddd7`
- `policy.sha256`: `9230b5fa7c137a7c1b92d1d82fdd66ef4ced33284509ce8e034a26a0577a8ddf`
- `cohsh.pool.control_sessions`: `2`
- `cohsh.pool.telemetry_sessions`: `24`
- `cohsh.tail.poll_ms_default`: `1000`
- `cohsh.tail.poll_ms_min`: `250`
- `cohsh.tail.poll_ms_max`: `10000`
- `cohsh.host_telemetry.nvidia_poll_ms`: `1000`
- `cohsh.host_telemetry.systemd_poll_ms`: `2000`
- `cohsh.host_telemetry.docker_poll_ms`: `2000`
- `cohsh.host_telemetry.k8s_poll_ms`: `5000`
- `retry.max_attempts`: `3`
- `retry.backoff_ms`: `200`
- `retry.ceiling_ms`: `2000`
- `retry.timeout_ms`: `5000`
- `heartbeat.interval_ms`: `15000`
- `trace.max_bytes`: `1048576`

_Generated from `configs/root_task.toml` (sha256: `8702c7c920c14b6449478c90ed34765787d2c3379a1ac305a4e98a99aa04ddd7`)._
