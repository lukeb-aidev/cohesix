// CLASSIFICATION: COMMUNITY
// Filename: BOOT_KERNEL_FLOW.md v0.3
// Author: Lukas Bower
// Date Modified: 2026-10-16

# Boot → Kernel → Validator → CLI Flow

This document illustrates how a cold boot transitions into user interactions.
GRUB now performs the initial load and passes control to the seL4 kernel via
the Multiboot2 protocol. A static Plan 9 userland, bundled as
`cohesix_root.elf`, starts immediately after seL4 to mount namespaces and launch
the validator.

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

- GRUB verifies firmware and kernel hashes before handing control to seL4.
- The kernel mounts `/srv`, `/history`, and `/n` using read-only and capability-guarded rules.
- `cohesix_root.elf` contains all userland binaries and is loaded as a single static ELF.
- The validator enforces syscall patterns and emits trace logs to `/log/validator/`.
- CLI tools (including `cohcc` and `man`) interact through user-visible endpoints, with all invocations appearing in the trace log.

All stages are covered by heartbeat and watchdog checks described in `WATCHDOG_POLICY.md`.

For detailed architecture discussion see
[`MISSION_AND_ARCHITECTURE.md`](MISSION_AND_ARCHITECTURE.md).

The entire boot chain and root image are built via `cohesix_fetch_build.sh`.
Afterwards run `tools/make_iso.sh` to assemble the GRUB ISO containing
`kernel.elf` and `userland.elf`.
