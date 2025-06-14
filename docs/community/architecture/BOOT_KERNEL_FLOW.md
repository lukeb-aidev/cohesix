// CLASSIFICATION: COMMUNITY
// Filename: BOOT_KERNEL_FLOW.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-23

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

For detailed architecture discussion see
[`MISSION_AND_ARCHITECTURE.md`](MISSION_AND_ARCHITECTURE.md).
