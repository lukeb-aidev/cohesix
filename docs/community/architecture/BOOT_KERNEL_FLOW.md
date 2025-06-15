// CLASSIFICATION: COMMUNITY
// Filename: BOOT_KERNEL_FLOW.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-31

# Boot → Kernel → Validator → CLI Flow

This document illustrates how a cold boot transitions into user interactions.

```text
+-----------+       +-----------+       +------------+       +-------+
| Bootloader| --->  |  Kernel   | --->  | Validator  | --->  |  CLI  |
+-----------+       +-----------+       +------------+       +-------+
     |                  |                    |                 |
     | loads image      | mounts namespaces   | checks syscalls |
     +---------------------------------------------------------+
                              user commands
```

## Security and Trace Path

- The bootloader verifies firmware and image hash before handing control to the kernel.
- The kernel mounts `/srv`, `/history`, and `/n` using read-only and capability-guarded rules.
- The validator enforces syscall patterns and emits trace logs to `/log/validator/`.
- CLI tools interact through user-visible endpoints, with all invocations appearing in the trace log.

All stages are covered by heartbeat and watchdog checks described in `WATCHDOG_POLICY.md`.

For detailed architecture discussion see
[`MISSION_AND_ARCHITECTURE.md`](MISSION_AND_ARCHITECTURE.md).
