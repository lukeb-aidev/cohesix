<!-- Author: Lukas Bower -->
<!-- Purpose: Generated coh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->

### coh policy defaults (generated)
- `manifest.sha256`: `7ac2fcc56751bb4670a74fd0063bfc4993c18367450aca3961ab65ad7ad37634`
- `policy.sha256`: `82d58b9decba0256d723e5324cdb3cdd46a677b8af3f1c4ba1d26ca40a2df6f1`
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
