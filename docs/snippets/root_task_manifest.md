<!-- Author: Lukas Bower -->
<!-- Purpose: Generated manifest snippet consumed by docs/ARCHITECTURE.md. -->

### Root-task manifest schema (generated)
- `meta.author`: `Lukas Bower`
- `meta.purpose`: `Root-task manifest input for coh-rtc.`
- `root_task.schema`: `1.2`
- `profile.name`: `virt-aarch64`
- `profile.kernel`: `true`
- `event_pump.tick_ms`: `5`
- `secure9p.msize`: `8192`
- `secure9p.walk_depth`: `8`
- `secure9p.tags_per_session`: `16`
- `secure9p.batch_frames`: `1`
- `secure9p.short_write.policy`: `reject`
- `telemetry.ring_bytes_per_worker`: `1024`
- `telemetry.frame_schema`: `legacy-plaintext`
- `telemetry.cursor.retain_on_boot`: `false`
- `cache.kernel_ops`: `true`
- `cache.dma_clean`: `true`
- `cache.dma_invalidate`: `true`
- `cache.unify_instructions`: `false`
- `features.net_console`: `true`
- `features.serial_console`: `true`
- `features.std_console`: `false`
- `features.std_host_tools`: `false`
- `namespaces.role_isolation`: `true`
- `tickets`: 3 entries
- `manifest.sha256`: `4d97e292f06d2311493fd09b6e1442edef3b03c544148ffad4f00559a8529111`

### Namespace mounts (generated)
- (none)

### Ecosystem section (generated)
- `ecosystem.host.enable`: `false`
- `ecosystem.host.mount_at`: `/host`
- `ecosystem.host.providers`: `(none)`
- `ecosystem.audit.enable`: `false`
- `ecosystem.policy.enable`: `false`
- `ecosystem.models.enable`: `false`
- Nodes appear only when enabled.

_Generated from `configs/root_task.toml` (sha256: `4d97e292f06d2311493fd09b6e1442edef3b03c544148ffad4f00559a8529111`)._
