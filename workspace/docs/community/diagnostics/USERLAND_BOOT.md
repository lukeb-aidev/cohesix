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

## Static ELF Checks (f22778e2)

Using the program headers dump `out/diag_mmu_fault_20250718_212435/cohesix_root_program_headers.txt`, all `LOAD` segments reside within the 0x4000_0000..0x8000_0000 physical range expected for the aarch64 `virt` machine. The `.vectors` page shows ARM branch instructions to the EL1 handlers such as `b 0x402ad0`, confirming vector table installation.

The `sel4-sys` build script now emits an absolute `cargo:rustc-link-search` pointing to `third_party/seL4/lib` and `cohesix_root` build.rs uses the same absolute path. The cross build wrapper exports `RUSTFLAGS` and `LIBRARY_PATH` relative to the project root so libsel4.a is always found.

A new PyTest test `tests/test_program_headers.py` verifies these program header addresses offline so CI can assert the image layout without running QEMU.
