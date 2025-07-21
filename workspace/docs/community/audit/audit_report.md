// CLASSIFICATION: COMMUNITY
// Filename: audit_report.md v0.2
// Author: Lukas Bower
// Date Modified: 2028-11-21

# Cohesix Boot Plumbing Audit

This report reviews boot viability and kernel-to-root plumbing for Cohesix. Findings are grouped by severity per [INSTRUCTION_BLOCK.md](../governance/INSTRUCTION_BLOCK.md).

## Boot Viability
- **UEFI stub present:** `CohesixBoot.inf` declares a `UEFI_APPLICATION` entry point `efi_main`【F:CohesixBoot.inf†L7-L14】.
- **Linker script:** `link.ld` defines `ENTRY(_start)` and maps text, rodata, data, heap and stack sections with 4 KiB alignment【F:workspace/cohesix_root/link.ld†L1-L39】【F:workspace/cohesix_root/link.ld†L80-L126】.
- **Early init:** `bootloader/init.rs` parses cmdline and passes a `BootContext` to Rust code【F:workspace/cohesix/src/bootloader/init.rs†L8-L18】【F:workspace/cohesix/src/bootloader/init.rs†L19-L32】.
- **Page tables:** `mmu.rs` creates L1/L2 tables and writes `TTBR0_EL1` during boot【F:workspace/cohesix_root/src/mmu.rs†L1-L33】【F:workspace/cohesix_root/src/mmu.rs†L34-L61】.
- **Interrupt vectors:** `vec.S` provides a minimal EL1 vector table for exceptions and IRQs【F:workspace/cohesix_root/src/vec.S†L1-L18】.
- **Serial output:** `sys.rs` keeps a UART frame and exposes `init_uart()` as the console hook【F:workspace/cohesix_root/src/sys.rs†L14-L25】.
- **Root filesystem mount:** `kernel/init.rc` mounts the 9P boot volume and links BusyBox applets【F:workspace/cohesix/src/kernel/init.rc†L7-L18】.

**Severity:** Low – Boot path components exist but are basic.

## Subsystem Gaps
- No dedicated filesystem driver; `sys.rs` embeds binaries via `include_bytes!` and stubs file I/O【F:workspace/cohesix_root/src/sys.rs†L94-L135】.
- Interrupt handlers in `vec.S` jump to undefined routines; no real IRQ controller driver【F:workspace/cohesix_root/src/vec.S†L1-L18】.
- Scheduler relies entirely on seL4 with no custom hooks (noted previously). 
- Missing network and storage drivers restrict reaching userland on real hardware.

**Severity:** Medium – Stubs could block hardware boot if not replaced.

## Plumbing Architecture
- `sel4-sys` uses `bindgen` to generate Rust FFI bindings with extensive include scanning【F:workspace/sel4-sys/build.rs†L28-L66】.
- `cohesix_root/build.rs` copies the target spec and vectors, injecting include paths via `CFLAGS`【F:workspace/cohesix_root/build.rs†L8-L35】【F:workspace/cohesix_root/build.rs†L80-L109】.
- This coupling requires environment variables (`SEL4_INCLUDE`, `SEL4_LIB_DIR`) and duplicate logic across build scripts. Directly binding a trimmed `sel4.h` plus manual extern definitions would simplify compilation and reduce rebuilds.

**Severity:** Low – Build complexity but functional.

## Recommendations
1. Consolidate build-time configuration into a single script or `build.rs` helper to avoid repeated environment checks.
2. Replace the `sel4-sys` crate with a small `extern "C"` wrapper generated from one header to minimize compile time.
3. Implement minimal filesystem and IRQ controller drivers to progress past the embedded initfs.
4. Add QEMU boot tests for the provided `sel4-aarch64.json` target ensuring the ELF image runs without manual patching.

Overall the project contains the core pieces to boot under QEMU but lacks several drivers and cleanup of build tooling to streamline seL4 integration.
