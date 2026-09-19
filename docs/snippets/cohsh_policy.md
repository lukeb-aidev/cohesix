<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->
<!-- Copyright 2026 Lukas Bower -->

### cohsh client policy (generated)
- `manifest.sha256`: `33ac378131c163399b747dac5175baf2619a92674de81f5ed3f8cf4e649f4de6`
- `policy.sha256`: `d29c3334b059890983960038ff116c262aadca5a8774c4178974fa85a2b3cdcf`
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

_Generated from `configs/root_task.toml` (sha256: `33ac378131c163399b747dac5175baf2619a92674de81f5ed3f8cf4e649f4de6`)._
