// CLASSIFICATION: COMMUNITY
// Filename: BOOT_KERNEL_FLOW.md v0.4
// Author: Lukas Bower
// Date Modified: 2026-12-31

# Boot → Kernel → Validator → CLI Flow

This document illustrates the boot pipeline for Cohesix in its pure UEFI form.
The UEFI firmware loads `CohesixBoot.efi`, which in turn starts the seL4 kernel
and passes control to the static Plan9 userland image `cohesix_root.elf`.
Namespaces are mounted immediately and the validator service begins tracing.

```text
+-------------+       +-----------+       +------------+       +-------+
| UEFI        | --->  |  Kernel   | --->  | Validator  | --->  |  CLI  |
+-------------+       +-----------+       +------------+       +-------+
      |                    |                    |                 |
      | loads CohesixBoot  | mounts namespaces   | checks syscalls |
      +-----------------------------------------------------------+
                              user commands
```

## Security and Trace Path

- UEFI verifies signed PE images before launching `CohesixBoot.efi`.
- The kernel mounts `/srv`, `/history`, and `/n` using read-only and
  capability-guarded rules.
- `cohesix_root.elf` contains all userland binaries and is loaded as a
  single static ELF.
- The validator enforces syscall patterns and emits trace logs to
  `/log/validator/`.
- CLI tools (including `cohcc` and `man`) interact through user-visible
  endpoints, with all invocations appearing in the trace log.

All stages are covered by heartbeat and watchdog checks described in
[`WATCHDOG_POLICY.md`](../governance/WATCHDOG_POLICY.md).
For detailed architecture discussion see
[`MISSION_AND_ARCHITECTURE.md`](MISSION_AND_ARCHITECTURE.md).

The entire boot chain and root image are built via `cohesix_fetch_build.sh`.
Run `tools/make_iso.sh` to assemble the UEFI ISO containing `kernel.efi` and
`cohesix_root.elf`.
