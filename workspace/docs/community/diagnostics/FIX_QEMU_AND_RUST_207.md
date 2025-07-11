// CLASSIFICATION: COMMUNITY
// Filename: FIX_QEMU_AND_RUST_207.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

# Enable Deep QEMU Debug & Rust Boot Cleanup

This summary captures the improvements for task **EnableQemuDeepDebugAndRustAudit-207**.

## QEMU Changes
- Added persistent `-serial mon:stdio` output.
- Enabled deeper tracing when `DEBUG_QEMU=1` using `-d cpu_reset,int,guest_errors,mmu`.
- Structured logs written to `$LOG_DIR/qemu_debug_*`.
- Example invocation:
  ```bash
  qemu-system-aarch64 -M virt,gic-version=2 -cpu cortex-a57 -m 1024M \
    -kernel out/bin/elfloader -initrd out/boot/image.cpio \
    -nographic -serial mon:stdio -d cpu_reset,int,guest_errors,mmu \
    -D logs/qemu_debug_example.log
  ```

## Rust Cleanups
- **main.rs**: validated bootargs and namespace lines with `core::str::from_utf8` to avoid undefined behaviour.
- **sys.rs**: introduced `EINVAL` constant for clearer error mapping.

