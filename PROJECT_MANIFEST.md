// CLASSIFICATION: COMMUNITY
// Filename: PROJECT_MANIFEST.md v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-02

# 🔄 Changelog

- **v0.71** (2025-07-21) – HTTP route and CLI docs for the GUI orchestrator. README clarified usage.
- **v0.70** (2025-07-20) – Updated `gui_orchestrator.md` with chi router and API usage notes.
- **v0.69** (2025-07-20) – Introduced Go GUI orchestrator with chi router and API endpoints.

# 📊 Metadata Summary

```
{
  "project": "Cohesix",
  "version": "0.8-beta",
  "description": "A distributed, role-aware OS built on seL4 with Plan9 userland and CUDA/Rapier acceleration",
  "roles": ["QueenPrimary", "Worker", "DroneWorker", "KioskInteractive", "GlassesAgent", "SensorRelay", "SimulatorTest"],
  "hardware_targets": ["Jetson Orin Nano 8GB", "Raspberry Pi 5 8GB", "AWS EC2 (Graviton/x86)", "Intel NUC-13 Pro"],
  "boot": {"default_role": "Worker", "override_env": "COHROLE", "cold_start_goal_ms": 200},
  "features": {"rapier_physics": true, "cuda_runtime": true, "plan9_9p": true, "sel4_kernel": true, "webcam_support": true, "busybox_fallback": true}
}
```

# 📜 OSS Licenses and Dependencies

| Name | Version | SPDX License | Purpose | Upstream |
|------|---------|--------------|---------|---------|
| sha2 | 0.10 | MIT OR Apache-2.0 | Hashing for integrity checks | https://crates.io/crates/sha2 |
| env_logger | 0.11.7 | MIT OR Apache-2.0 | Structured logging | https://crates.io/crates/env_logger |
| log | 0.4 | MIT OR Apache-2.0 | Logging facade | https://crates.io/crates/log |
| clap | 4.5.4 | MIT OR Apache-2.0 | CLI argument parsing | https://crates.io/crates/clap |
| sysinfo | 0.30 | MIT | System information | https://crates.io/crates/sysinfo |
| libloading | 0.7 | MIT OR Apache-2.0 | Dynamic library loading | https://crates.io/crates/libloading |
| inotify | 0.10 | MIT OR Apache-2.0 | Device hotplug events | https://crates.io/crates/inotify |
| rapier3d | 0.17.2 | Apache-2.0 | Physics engine | https://crates.io/crates/rapier3d |
| tokio | 1 | MIT | Async runtime | https://crates.io/crates/tokio |
| ureq | 2.9 | MIT OR Apache-2.0 | HTTP client | https://crates.io/crates/ureq |
| serde | 1 | MIT OR Apache-2.0 | Serialization | https://crates.io/crates/serde |
| serde_json | 1 | MIT OR Apache-2.0 | JSON serialization | https://crates.io/crates/serde_json |
| cobra | 1.8.0 | Apache-2.0 | Go CLI framework | https://github.com/spf13/cobra |
| chi | v5 | MIT | HTTP router for GUI orchestrator | https://github.com/go-chi/chi |
