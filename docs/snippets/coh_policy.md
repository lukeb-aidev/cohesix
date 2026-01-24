<!-- Author: Lukas Bower -->
<!-- Purpose: Generated coh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->

### coh policy defaults (generated)
- `manifest.sha256`: `aeacd14e34c15b39b879af95c0cc5c19de757d368702d1453024ce4cd910a8cb`
- `policy.sha256`: `35df2c524f27ce12f7417360e3f5e0e19fdf8241fd657e7619bf2c5d0223f1cb`
- `coh.mount.root`: `/`
- `coh.mount.allowlist`: `/proc, /queen, /worker, /log, /gpu, /host`
- `coh.telemetry.root`: `/queen/telemetry`
- `coh.telemetry.max_devices`: `32`
- `coh.telemetry.max_segments_per_device`: `4`
- `coh.telemetry.max_bytes_per_segment`: `32768`
- `coh.telemetry.max_total_bytes_per_device`: `131072`
- `retry.max_attempts`: `3`
- `retry.backoff_ms`: `200`
- `retry.ceiling_ms`: `2000`
- `retry.timeout_ms`: `5000`
