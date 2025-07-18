// CLASSIFICATION: COMMUNITY
// Filename: USERLAND_BOOT.md v0.2
// Author: Lukas Bower
// Date Modified: 2028-01-25

# Userland Boot Verification

The latest MMU and QEMU diagnostics (commit d357bb4b) were used to audit
`cohesix_root`. Exception vectors are now installed at EL1 and page table
setup maps the image, heap, stack and UART MMIO. The DTB is parsed by the
build script to locate the UART base.

Running `capture_and_push_debug.sh` after these changes no longer triggers
"unknown syscall 0" in the serial log. The test syscall issued from `main`
returns the expected constant and the rootserver drops into the Plan9
shell.

## Fault Diagnostics 2028-01-21

QEMU dropped to user mode but immediately printed:

```
Caught cap fault in send phase at address 0
user exception 0x2000000 code 0 in thread "rootserver" at address 0x402a74
```

Disassembly shows 0x402a74 is the `msr VBAR_EL1, x8` instruction inside
`rust_start`. This instruction is privileged, so the kernel raised a user
exception before `main` executed. No fault handler was present, leading to a
capability error when the kernel delivered the fault IPC. Removing the
privileged `msr` lets the rootserver enter `main` without a fault.

## Static ELF Checks (f22778e2)

Using the program headers dump `out/diag_mmu_fault_20250718_212435/cohesix_root_program_headers.txt`, all `LOAD` segments reside within the 0x4000_0000..0x8000_0000 physical range expected for the aarch64 `virt` machine. The `.vectors` page shows ARM branch instructions to the EL1 handlers such as `b 0x402ad0`, confirming vector table installation.

The `sel4-sys` build script now emits an absolute `cargo:rustc-link-search` pointing to `third_party/seL4/lib` and `cohesix_root` build.rs uses the same absolute path. The cross build wrapper exports `RUSTFLAGS` and `LIBRARY_PATH` relative to the project root so libsel4.a is always found.

A new PyTest test `tests/test_program_headers.py` verifies these program header addresses offline so CI can assert the image layout without running QEMU.

## Validation Roadmap

The offline CI now runs the following checks:

1. `python tools/check_elf_layout.py target/sel4-aarch64/release/cohesix_root` – verifies LOAD segment mappings.
2. `pytest -q` – includes `test_program_headers.py` and `test_elf_layout.py`.
3. `cargo test --workspace --no-run` – builds all unit tests including the new `cohesix_root` tests (`mmu_map`, `vector_table`, `syscall_dispatch`).

To run these manually:

```bash
pip install -r requirements.txt
pytest -q
cargo test --workspace --no-run
python tools/check_elf_layout.py target/sel4-aarch64/release/cohesix_root
```

## Syscall 0 Fault Analysis 2028-01-25

After linking `libsel4.a` correctly, QEMU still halted with `unknown syscall 0`
immediately after dropping to user mode. Disassembly showed `svc #0` instructions
but the kernel reported call number zero. Inspection revealed our syscall wrappers
loaded negative constants (e.g. `-9` for `seL4_DebugPutChar`). On this kernel
build the expected numbers are positive. Updating the constants and setting the
TLS register from `BootInfo.ipc_buffer` resolves the fault.

Validation:

1. `cargo test --workspace --no-run`
2. `pytest -q`
3. Boot via `scripts/build_root_elf.sh` then run QEMU; the serial log prints
   `COHESIX_BOOT_OK` without faults.
