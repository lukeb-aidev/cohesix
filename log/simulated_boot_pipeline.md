// CLASSIFICATION: COMMUNITY
// Filename: simulated_boot_pipeline.md v0.1
// Author: Lukas Bower
// Date Modified: 2026-11-20

# Cohesix Simulated Boot Pipeline

This document walks through the entire Cohesix build and boot flow using static analysis of the repository. It is intended for environments where hardware emulation is unavailable. All steps reference current source files and scripts.

## 1. Build Phase

### 1.1 Repository Fetch & Toolchain Setup
- `cohesix_fetch_build.sh` clones the repository and installs cross toolchains if needed. On `aarch64` it ensures `aarch64-linux-musl-gcc` via apt lines 8‑24【F:cohesix_fetch_build.sh†L8-L24】.
- CUDA paths are detected and exported (lines 42‑84). The script verifies `cuda.h` and `nvcc` presence with warnings when missing【F:cohesix_fetch_build.sh†L120-L177】.
- Rust targets are installed via `rustup target add` depending on `$COH_ARCH` (lines 178‑217).

### 1.2 Building Components
- BusyBox is built through `scripts/build_busybox.sh` and staged into `out/iso/bin` (lines 253‑289).
- All Rust workspace binaries are compiled with features `cuda,std,rapier,physics,...` (lines 309‑332). Output is copied to `out/bin`.
- seL4 kernel ELF is expected in `$SEL4_WORKSPACE/build_*`. If missing the script exits (lines 374‑419).
- A root ELF (`cohesix_root.elf`) is built via `scripts/build_root_elf.sh` (lines 337‑358).
- Config files and role YAMLs are staged into `out/iso/etc` and `out/iso/roles` (lines 452‑510).

### 1.3 ISO Creation
- After verifying staging, `tools/make_iso.sh` generates a GRUB ISO. It writes `grub.cfg` dynamically and calls `grub-mkrescue` (lines 520‑571). For x86_64 it uses a multiboot entry; for aarch64 a standard `linux` entry.

## 2. QEMU Boot Phase
- If QEMU is available, the build script boots the ISO for either architecture. For x86_64 a serial log is captured and checked for `BOOT_OK` (lines 615‑708). A similar block handles aarch64 with `QEMU_EFI.fd` firmware (lines 722‑755).
- `ci/qemu_boot_check.sh` is also provided for CI. It searches for firmware paths and waits up to 30 s for the shell banner "Cohesix shell started"【F:ci/qemu_boot_check.sh†L10-L76】【F:ci/qemu_boot_check.sh†L80-L107】.

## 3. Bootloader and Kernel Entry
- The ISO loads GRUB which runs `kernel.elf` and `userland.elf` from `/boot`. `BootAgent::init` drives early init. It logs startup, reads `/proc/cmdline`, invokes `bootloader::init::early_init`, discovers devices and finally calls `userland_bootstrap::dispatch_user("init")`【F:src/kernel/boot/bootloader.rs†L23-L46】.
- `bootloader::init::early_init` parses boot arguments and calls HAL paging stubs for the current architecture【F:src/bootloader/init.rs†L48-L64】.

## 4. HAL Paging
- `hal::arm64::init_paging` allocates simple L1/L2 tables, loads `ttbr0_el1`, enables the MMU and logs the mapped range【F:src/hal/arm64/mod.rs†L21-L57】.
- `hal::x86_64::init_paging` builds PML4/PDPTE/PDE/PT tables, sets CR3/CR4/CR0 bits, enabling paging【F:src/hal/x86_64/mod.rs†L21-L55】.
- Interrupt setup functions return `Ok(())` but perform no real hardware configuration yet.

## 5. Kernel Boot Flow
- BootAgent prints memory zone information and enumerates `/dev` nodes before starting seL4【F:src/kernel/boot/bootloader.rs†L66-L82】.
- seL4 jumps to `_sel4_start`, which immediately calls `main` then loops. `switch_to_user` is provided for both architectures to drop privileges when launching userland【F:src/seL4/sel4_start.S†L6-L32】.

## 6. Userland Dispatch
- `userland_bootstrap::dispatch_user` loads `/bin/init` via `loader::load_user_elf`. On success it spawns a process and switches to EL0/ring 3 using `switch_to_user` after `init_syscall_trap` installs the trap vector【F:src/kernel/userland_bootstrap.rs†L12-L25】.
- `loader::load_user_elf` parses the ELF headers with `xmas_elf`. It logs each loadable segment and returns an entry point plus a stack pointer but does not yet map memory pages【F:src/kernel/loader.rs†L30-L60】.

## 7. Syscalls
- Assembly vectors named `syscall_vector` save registers and call `syscall_trap` which forwards to `handle_syscall`【F:src/kernel/syscalls/syscall.rs†L11-L36】【F:src/kernel/syscalls/syscall.rs†L37-L71】.
- `handle_syscall` logs the syscall and dispatches via `syscall_table::dispatch` to basic stubs for read/write/open/close/exec【F:src/kernel/syscalls/syscall.rs†L72-L99】【F:src/kernel/syscalls/syscall_table.rs†L12-L38】.

## 8. Expected Console Output (Simulated)
Assuming all stages succeed, the boot log would include lines such as:
```
[BootAgent] Starting bootloader initialization...
[BootAgent] Running preflight checks...
[BootAgent] Configuring memory zones...
[BootAgent] Enumerating early devices...
[BootAgent] Launching seL4 with role QueenPrimary
Entry point: 0x80000
User stack allocated at 0x801000
Switching to EL0 at entry 0x80000, stack 0x801000
Cohesix shell started
```

## 9. Identified Gaps
Based on static review the following issues remain:
1. **Toolchain assumptions** – missing Rust targets or cross compilers cause build failures as seen in `cargo test` errors.
2. **Page table stubs** – both HAL modules map only the first 2 MiB; no user address space isolation occurs.
3. **ELF loader limitations** – segments are logged but not mapped into separate page tables; `page_table` field is always `None`.
4. **Privilege switching** – while `switch_to_user` exists, no code sets up a user-mode stack in separate memory.
5. **Syscall path** – trap vectors exist but rely on the host environment; no vector table or IDT/GIC setup is present.
6. **ISO creation** – fails if GRUB modules for the target arch are missing; `make_iso.sh` exits early with warnings.

Real hardware would likely halt after attempting to jump to user mode because paging and privilege transitions are incomplete.

