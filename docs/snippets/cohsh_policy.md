<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->
<!-- Copyright 2026 Lukas Bower -->

### cohsh client policy (generated)
- `manifest.sha256`: `bdd02b671876b5f9fc0c96c7750c46cc20988cdd3c8841be6b1c585ca65ae3da`
- `policy.sha256`: `3274954fd43491b1d866cb2e77c77a6eecc5bd59d2871792dd63493721de0712`
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

_Generated from `configs/root_task.toml` (sha256: `bdd02b671876b5f9fc0c96c7750c46cc20988cdd3c8841be6b1c585ca65ae3da`)._
