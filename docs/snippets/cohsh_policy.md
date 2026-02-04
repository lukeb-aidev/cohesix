<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->

### cohsh client policy (generated)
- `manifest.sha256`: `7d6b2ecf259049c1e431a37e693118b9bccc05395e374934f3dc6837d1004c1f`
- `policy.sha256`: `a4a9a52f2775563b9524a064ab1bea324048c73bec4f7b062e0857c71291fcb3`
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

_Generated from `configs/root_task.toml` (sha256: `7d6b2ecf259049c1e431a37e693118b9bccc05395e374934f3dc6837d1004c1f`)._
