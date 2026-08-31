<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->
<!-- Copyright 2026 Lukas Bower -->

### cohsh client policy (generated)
- `manifest.sha256`: `8108f22e580a653df9e65e4509d0841aacef0d1c5825094ee861a3a09fb14c66`
- `policy.sha256`: `3df0462d7ea046cd55b4780ee8df4d619c6996cf53aaa5874d827c41f3a444ef`
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

_Generated from `configs/root_task.toml` (sha256: `8108f22e580a653df9e65e4509d0841aacef0d1c5825094ee861a3a09fb14c66`)._
