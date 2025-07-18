// CLASSIFICATION: COMMUNITY
// Filename: boot_workflow_rust.md v0.8
// Author: Lukas Bower
// Date Modified: 2028-01-11

# Rust Boot Workflow

This note summarises the full boot path for the `cohesix_root` binary when built
with Cargo for the seL4 13.0.0 kernel. It consolidates the findings from the
`HOLISTIC_BOOT_FLOW_20250717.md` diagnostics and the latest logs in
`out/diag_mmu_fault_20250717_212342/`.

1. **Cargo Build** – `cargo +nightly build -p cohesix_root --release` with the
   custom target JSON (`sel4-aarch64.json`) and `build-std` features produces the
   ELF `cohesix_root`. The workspace now includes the `sel4-sys` crate providing
   raw FFI bindings. `link.ld` is passed via `-C link-arg=-Tlink.ld`.
   The target JSON no longer injects `rustflags`; panic behavior is set by the
   workspace `[profile.release]` stanza.
2. **Image Packaging** – `cohesix_fetch_build.sh` gathers `kernel.elf`,
   `kernel.dtb` and `cohesix_root.elf` into `cohesix.cpio` in that order.
3. **Elfloader Phase** – QEMU boots the `elfloader` which extracts the DTB and
   loads both ELF images. Program headers from
   `cohesix_root_program_headers.txt` confirm the text segment at `0x400000`.
4. **Kernel Handoff** – The kernel enables paging and logs the reserved regions
   before jumping to user mode. The serial log shows
   `Booting all finished, dropped to user space`.
5. **MMU Setup** – `startup::VECTORS` is linked into `.vectors` and mapped by
   the kernel. `startup::rust_start` sets `VBAR_EL1` to this address, clears the
   `.bss`, and calls `main()`.
6. **Rootserver Init** – `main` initialises UART via `seL4_DebugPutChar` and
   mounts the minimal Plan9 namespace. Diagnostic logs verify the message
   `ROOTSERVER ONLINE` is printed and no MMU faults occur.

To validate the build chain run:

```bash
cargo +nightly clean && \
cargo +nightly build -p cohesix_root --release \ 
  --target=cohesix_root/sel4-aarch64.json \ 
  -Z build-std=core,alloc,compiler_builtins \ 
  -Z build-std-features=compiler-builtins-mem && \
qemu-system-aarch64 -M virt -nographic \
  -kernel target/cohesix_root/release/cohesix_root && \
bash capture_and_push_debug.sh
```

Verify that the `sel4-sys` crate is linked with:

```bash
cargo +nightly tree -p cohesix_root | grep sel4-sys
```

If the serial log shows `✅ rootserver main loop entered` the boot path is
healthy.

## Linker Configuration

seL4 Library: We link in `libsel4.a` from `third_party/seL4/lib` by passing
`-Lthird_party/seL4/lib -lsel4` to the Rust linker via our JSON target or
`RUSTFLAGS`.
Linker Configuration Audit: Updated JSON target, build.rs, .cargo/config.toml, link.ld, and env vars to ensure libsel4.a in third_party/seL4/lib is found.

## sel4-sys integration

Path Resolution: build.rs now derives the `third_party/seL4/lib` directory from `CARGO_WORKSPACE_DIR` (falling back to `CARGO_MANIFEST_DIR`), with a clear panic message if the directory is missing.
