// CLASSIFICATION: COMMUNITY
// Filename: USERLAND_BOOT.md v0.1
// Author: Lukas Bower
// Date Modified: 2028-01-21

# Userland Boot Verification

The latest MMU and QEMU diagnostics (commit d357bb4b) were used to audit
`cohesix_root`. Exception vectors are now installed at EL1 and page table
setup maps the image, heap, stack and UART MMIO. The DTB is parsed by the
build script to locate the UART base.

Running `capture_and_push_debug.sh` after these changes no longer triggers
"unknown syscall 0" in the serial log. The test syscall issued from `main`
returns the expected constant and the rootserver drops into the Plan9
shell.
