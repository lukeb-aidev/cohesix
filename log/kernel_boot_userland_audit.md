// CLASSIFICATION: COMMUNITY
// Filename: kernel_boot_userland_audit.md v0.1
// Author: Lukas Bower
// Date Modified: 2026-11-17

# Kernel Boot and Userland Audit

## Overview
This audit inspects Cohesix kernel boot logic for both aarch64 and x86_64.
It traces early boot from GRUB through the BootAgent to userland dispatch,
verifying MMU setup, ELF loading, privilege level transitions and syscall handling.

## Architecture-Specific Boot Steps

### Bootloader Integration
- `BootAgent::init` orchestrates early setup and finally calls `userland_bootstrap::dispatch_user("init")`【F:src/kernel/boot/bootloader.rs†L23-L46】.
- `BootContext::early_init` parses boot arguments then calls architecture stubs for paging and interrupts【F:src/bootloader/init.rs†L48-L63】.
- Both `hal::arm64::init_paging` and `hal::x86_64::init_paging` simply log and return `Ok(())` without setting page tables【F:src/hal/arm64/mod.rs†L27-L33】【F:src/hal/x86_64/mod.rs†L26-L31】.

### Privilege Mode Switch
- No assembly or Rust code configures ELR_EL1/SPSR_EL1 or CS/SS for dropping to user mode. Searches for `eret`, `sysret`, `iretq` yielded no results.
- `sel4_start.S` just calls `main` and loops indefinitely【F:src/seL4/sel4_start.S†L6-L9】.

## ELF Loader and User Process Setup
- No ELF parsing or loading routines exist in the repository. `userland_bootstrap` dispatches to statically linked functions within the kernel process and does not create isolated address spaces【F:src/kernel/userland_bootstrap.rs†L13-L33】.
- `proc_mgr` allocates a fixed 4 KiB stack per process entry but never maps or switches memory contexts【F:src/kernel/proc_mgr.rs†L18-L35】.

## Syscall and Trap Handling
- `handle_syscall` logs calls and dispatches to a simple enum-based table【F:src/kernel/syscalls/syscall.rs†L12-L29】【F:src/kernel/syscalls/syscall_table.rs†L19-L51】.
- No trap vectors or hardware-specific syscall entry points are implemented. Syscalls are invoked directly from user code without mode transitions.

## QEMU Boot Validation
- CI invokes `ci/qemu_boot_check.sh` after `make` targets to boot the ISO via QEMU【F:.github/workflows/ci.yml†L60-L80】.
- The script boots either `qemu-system-aarch64` or `qemu-system-x86_64` and looks for "Cohesix shell started" in the serial log【F:ci/qemu_boot_check.sh†L18-L41】【F:ci/qemu_boot_check.sh†L57-L65】.

## Issues Found
1. **No real MMU or page-table setup** – both architectures rely on stub functions.
2. **Missing user-mode transition** – absence of `eret`/`sysret`/`iretq` prevents switching to EL0/ring 3.
3. **ELF loader absent** – userland programs are compiled-in functions; there is no binary loader or address-space isolation.
4. **Syscall path incomplete** – traps from user mode are not wired to kernel handlers; current syscalls run in kernel context.

## Conclusion
Cohesix currently lacks the essential boot and userland infrastructure described in the task requirements. Paging, privilege changes, ELF loading and true user processes are unimplemented. QEMU boot scripts exist but only verify stub binaries. Significant development is needed to meet seL4-based kernel goals.
