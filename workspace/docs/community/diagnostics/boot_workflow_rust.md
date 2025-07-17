// CLASSIFICATION: COMMUNITY
// Filename: boot_workflow_rust.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-31

# Rust Boot Workflow

This note summarises the full boot path for the `cohesix_root` binary when built
with Cargo for the seL4 13.0.0 kernel. It consolidates the findings from the
`HOLISTIC_BOOT_FLOW_20250717.md` and related diagnostic files.

1. **Image Packaging** – `cohesix_fetch_build.sh` gathers `kernel.elf`,
   `cohesix_root.elf` and `kernel.dtb` into `cohesix.cpio`.
2. **Elfloader Phase** – QEMU boots the `elfloader` which extracts the DTB and
   loads both ELF images using addresses from `kernel.lds`.
3. **MMU Setup** – The rootserver exception vectors are placed in `.vectors` and
   linked at the offset specified in `kernel.lds`. The build script embeds the
   table using `global_asm!` so the addresses match the seL4 spec.
4. **Rust Entry** – `startup::rust_start` clears `.bss`, sets the vector base
   register, and jumps to `main()`.

With these pieces aligned, QEMU boots without triggering the previous MMU fault
at startup and enters the rootserver loop cleanly.
