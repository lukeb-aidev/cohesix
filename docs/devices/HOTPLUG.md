// CLASSIFICATION: COMMUNITY
// Filename: HOTPLUG.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-14

# Device Hotplug Handling

Cohesix monitors `/dev` using `inotify` and automatically responds to device changes.
When a new device node appears, the runtime queries capability rules via the validator
and registers the device path under `/srv/dev/`.

Missing optional devices such as webcams or IMU sensors trigger a telemetry event
`device_missing` and fall back to stub drivers. The validator logs any attempt to
access an unavailable device as `hotplug_denied`.

Telemetry events:
- `device_added` with path and driver name
- `device_removed` with path
- `device_missing` when expected hardware is not present

Validator rules enforce that only roles with the `dev-hotplug` capability may
install or remove device drivers. All hotplug actions are written to
`/srv/trace/hotplug.log`.
