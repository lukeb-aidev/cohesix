<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->

### cohsh client policy (generated)
- `manifest.sha256`: `3a20adc55c8f975e20e8ef031422f8a09b4a7b8e524dd052bf69296ddf7ff1af`
- `policy.sha256`: `96262c617e5a15321d58f069f17664dfbe02ffa9e6e4df7a38169c21b4e37ee8`
- `cohsh.pool.control_sessions`: `2`
- `cohsh.pool.telemetry_sessions`: `4`
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

_Generated from `configs/root_task.toml` (sha256: `3a20adc55c8f975e20e8ef031422f8a09b4a7b8e524dd052bf69296ddf7ff1af`)._
