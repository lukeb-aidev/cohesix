<!-- Author: Lukas Bower -->
<!-- Purpose: Generated coh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->

### coh policy defaults (generated)
- `manifest.sha256`: `a40f87e1b0e148da7f7be9cab2a960bbb41cf9ef4e29e7c71c6847d92de9f509`
- `policy.sha256`: `dd0ae4f9001bdb61aacba713605b5c5b7b7c6ffbfa988919563c8979185e6113`
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
