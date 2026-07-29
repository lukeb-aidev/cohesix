<!-- Author: Lukas Bower -->
<!-- Purpose: Generated coh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->
<!-- Copyright 2026 Lukas Bower -->

### coh policy defaults (generated)
- `manifest.sha256`: `fb62b38622b289d3f9cd3bcd7171f270b3d45849f9e556f46d8fde381b423561`
- `policy.sha256`: `d53758596be0231f1211aa4328f76d0c662771cceb7a1371f5f706f82d9f626a`
- `coh.mount.root`: `/`
- `coh.mount.allowlist`: `/proc, /queen, /worker, /log, /gpu, /host`
- `coh.telemetry.root`: `/queen/telemetry`
- `coh.telemetry.max_devices`: `32`
- `coh.telemetry.max_segments_per_device`: `4`
- `coh.telemetry.max_bytes_per_segment`: `131072`
- `coh.telemetry.max_total_bytes_per_device`: `524288`
- `coh.run.lease.schema`: `gpu-lease/v1`
- `coh.run.lease.active_state`: `ACTIVE`
- `coh.run.lease.max_bytes`: `1024`
- `coh.run.breadcrumb.schema`: `gpu-breadcrumb/v1`
- `coh.run.breadcrumb.max_line_bytes`: `512`
- `coh.run.breadcrumb.max_command_bytes`: `256`
- `coh.peft.export.root`: `/queen/export/lora_jobs`
- `coh.peft.export.max_telemetry_bytes`: `131072`
- `coh.peft.export.max_policy_bytes`: `8192`
- `coh.peft.export.max_base_model_bytes`: `1024`
- `coh.peft.import.registry_root`: `out/model_registry`
- `coh.peft.import.max_adapter_bytes`: `67108864`
- `coh.peft.import.max_lora_bytes`: `65536`
- `coh.peft.import.max_metrics_bytes`: `65536`
- `coh.peft.import.max_manifest_bytes`: `8192`
- `coh.peft.activate.max_model_id_bytes`: `128`
- `coh.peft.activate.max_state_bytes`: `4096`
- `retry.max_attempts`: `3`
- `retry.backoff_ms`: `200`
- `retry.ceiling_ms`: `2000`
- `retry.timeout_ms`: `5000`
