<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->
<!-- Copyright 2026 Lukas Bower -->

### cohsh client policy (generated)
- `manifest.sha256`: `91a0c04d4d6591ac87f6e228ba4f0f4ec79cdb0b87acd00e584450ea776cae53`
- `policy.sha256`: `60addb4122ed4a42343fdfab653b83c8567af0314081b6a146cc45f2832f5244`
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
- `trace.max_duration_ms`: `60000`

_Generated from `configs/root_task.toml` (sha256: `91a0c04d4d6591ac87f6e228ba4f0f4ec79cdb0b87acd00e584450ea776cae53`)._
