<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->
<!-- Copyright 2026 Lukas Bower -->

### cohsh client policy (generated)
- `manifest.sha256`: `fd4aee25dd42e51e6e4b581a46acd45299dcf2c12436f2d1fdf12d77dd177d93`
- `policy.sha256`: `b40903138201490a68b00caf3db7bfd4bd57cb48036fb4571cd702742964b83c`
- `cohsh.pool.control_sessions`: `2`
- `cohsh.pool.telemetry_sessions`: `8`
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

_Generated from `configs/root_task.toml` (sha256: `fd4aee25dd42e51e6e4b581a46acd45299dcf2c12436f2d1fdf12d77dd177d93`)._
