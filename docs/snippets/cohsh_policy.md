<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->

### cohsh client policy (generated)
- `manifest.sha256`: `2e64f09fb17eafce52fe3e7a29fa7eb11f2299022ca7d13eabf9b31c809b4234`
- `policy.sha256`: `d2a81b1375a68d34be68083e058dee51046daa7ecdeec71d296a5790fa5cb367`
- `cohsh.pool.control_sessions`: `2`
- `cohsh.pool.telemetry_sessions`: `4`
- `cohsh.tail.poll_ms_default`: `1500`
- `cohsh.tail.poll_ms_min`: `500`
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

_Generated from `configs/root_task.toml` (sha256: `2e64f09fb17eafce52fe3e7a29fa7eb11f2299022ca7d13eabf9b31c809b4234`)._
