// CLASSIFICATION: COMMUNITY
// Filename: USERLAND_BOOT.md v0.6
// Author: Lukas Bower
// Date Modified: 2030-03-09

# Userland Boot Verification

## Boot Ring Crash Capture 2030-03-08

Disabling both `CONFIG_PRINTING` in the kernel and Rust debuginfo does not
eliminate our crash breadcrumbs. Every `coherr!` call still records into the
ring buffer maintained by `bootlog::record`, independent of the seL4 debug
syscalls, and the handlers dump register state into that stream during panics
and synchronous exceptions.【F:workspace/cohesix_root/src/main.rs†L66-L124】【F:workspace/cohesix_root/src/lang_items.rs†L1-L55】
The buffer is fixed at 4 KiB and persists in `.bss`, and we expose it as the
Plan 9 file `/log/boot_ring` via the minimal libc shims.【F:workspace/cohesix_root/src/bootlog.rs†L10-L156】【F:workspace/cohesix_root/src/sys.rs†L1-L127】
`mmu::init` keeps UART MMIO optional, so you can operate entirely from the
memory-backed log when serial output is unavailable.【F:workspace/cohesix_root/src/mmu.rs†L10-L29】

### Catching crashes before Plan 9 comes up

When the boot halts immediately after `Jumping to kernel-image entry point...`
we never reach the Plan 9 shell to stream `/log/boot_ring`. Build the root
server with semihosting enabled and let `coherr!` mirror every byte onto the
QEMU host:

1. Enable the feature during the cross build:

   ```bash
   cargo build -p cohesix_root --target sel4-aarch64.json --features semihosting
   ```

2. Provide a semihosting directive in `/boot/bootargs.txt` (the image now
   embeds this file verbatim) so the runtime opts into the host trap before it
   parses Plan 9 environment variables. Use `stdout` for live console output or
   `file:` to tee into a host path:

   ```
   coh.semihost=stdout
   # or
   coh.semihost=file:/tmp/cohesix_boot.log
   ```

   The loader copies the file into the image and `load_bootargs` forwards each
   token to the semihosting driver before Plan 9 comes up.【F:workspace/cohesix_root/src/sys.rs†L36-L74】【F:workspace/cohesix_root/src/main.rs†L351-L395】

3. Launch QEMU with semihosting enabled. `scripts/debug_qemu_boot.sh` now
   passes `-semihosting-config enable=on,target=native` so every `coherr!`
   invocation traps into QEMU even if UART is disabled.【F:scripts/debug_qemu_boot.sh†L52-L74】

4. `coherr!` writes into both the ring buffer and the semihosting channel. In
   `stdout` mode the bytes appear inline with the QEMU monitor; in `file:` mode
   QEMU appends them to the chosen host file. Because the hook sits inside
   `debug_putchar`, the panic handlers, MMU bring-up logs, and register dumps
   all reach the host before the root task faults.【F:workspace/cohesix_root/src/main.rs†L66-L124】【F:workspace/cohesix_root/src/semihosting.rs†L1-L128】

This lets us “catch the crash in the act” without turning kernel printing back
on. Use `tail -f /tmp/cohesix_boot.log` or watch the emulator stdout to capture
the precise panic site.

**Streaming during boot**

1. Build the image and launch QEMU (for example with
   `./scripts/debug_qemu_boot.sh`).
2. As soon as the Plan 9 shell prompt appears on the serial console, run
   `cat /log/boot_ring`. Redirect the output to a host-mounted path (for
   example `/n/hostfs/boot_ring.log` when using the default 9P export) if you
   want to retain the bytes after the root task halts.

The command issues the same `coh_open`/`coh_read` sequence the libc shims
provide and streams the buffered panic lines (e.g. `exc_el1_sync` with ESR/FAR
details) even when no characters hit the UART.

**Offline snapshot after a crash**

1. Locate the `.bss` window in the rootserver ELF (`objdump -h
   out/bin/cohesix_root.elf | grep .bss`). Note the base address and span.
2. When QEMU halts, open the monitor (`Ctrl+a`, then `c`) and dump that region:

   ```
   (qemu) pmemsave 0x00406000 0x2000 boot_ring.bin
   ```

   Adjust the size if you increased `BOOTLOG_CAPACITY`.
3. Convert the raw bytes back to text on the host:

   ```bash
   strings -a boot_ring.bin | tail -n 40
   ```

The final lines show the precise ESR/ELR/FAR tuples that triggered the abort
without re-enabling kernel printing.

The latest MMU and QEMU diagnostics (commit d357bb4b) were used to audit
`cohesix_root`. Exception vectors are now installed at EL1 and page table
setup maps the image, heap, stack and UART MMIO. The DTB is parsed by the
build script to locate the UART base.

Running `capture_and_push_debug.sh` after these changes no longer triggers
"unknown syscall 0" in the serial log. The test syscall issued from `main`
returns the expected constant and the rootserver drops into the Plan9
shell.

> **Regenerating the diagnostics**: After building `cohesix_root`, run
> `./capture_and_push_debug.sh` from the repository root. The script
> captures the ELF dumps and QEMU logs into a timestamped directory under
> `diagnostics/`. Copy the resulting files into `out/` only when you need a
> local scratch space; the generated tree stays reproducible via the script
> and no longer needs to remain in version control.

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

Using the program headers dump generated by `capture_and_push_debug.sh`
(`diagnostics/diag_mmu_fault_<timestamp>/cohesix_root_program_headers.txt`),
all `LOAD` segments reside within the 0x4000_0000..0x8000_0000 physical range
expected for the aarch64 `virt` machine. The `.vectors` page shows ARM branch
instructions to the EL1 handlers such as `b 0x402ad0`, confirming vector table
installation.

The `sel4-sys` build script now emits an absolute `cargo:rustc-link-search` pointing to `third_party/seL4/lib` and `cohesix_root` build.rs uses the same absolute path. The cross build wrapper exports `RUSTFLAGS` and `LIBRARY_PATH` relative to the project root so libsel4.a is always found.

## Validation Roadmap

The offline CI now runs the following checks:

1. `python tools/check_elf_layout.py target/sel4-aarch64/release/cohesix_root` – verifies LOAD segment mappings.
2. `pytest -q` – exercises the remaining Python diagnostics including `test_elf_layout.py`.
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

## Syscall 0 Fault Regression 2028-01-30

QEMU again halted with `unknown syscall 0` at the first `svc` in `main`.
The syscall wrappers were correct, but the TLS register still held zero
because logging occurred before calling `sel4_set_tls`. The kernel read
the syscall label from address zero and rejected it. Moving the TLS
initialisation ahead of the first log resolves the issue. The boot log
now prints `ROOTSERVER ONLINE` and `COHESIX_BOOT_OK` before spawning
`/bin/init`.

## VM Fault 0x20 2028-02-15

Booting the February image revealed a data fault at address `0x20` as soon as
the rootserver entered user mode. Analysis of the stack dump showed a null
BootInfo pointer; the first load from `x0` at offset `0x20` corresponded to the
`ipc_buffer` field.

The entry stub in `entry.S` failed to preserve the pointer provided by the
kernel. The updated code stores `x0` into `BOOTINFO_PTR` before clearing `.bss`
and sets up Rust helpers (`set_bootinfo_ptr`, `bootinfo`) to read it.

`mmu::init` now identity maps this BootInfo frame alongside the device tree and
UART regions. `main` retrieves the pointer, configures TLS with
`BootInfo.ipc_buffer`, and proceeds into the Plan9 init sequence without faults.

## Syscall Constant Correction 2028-02-16

The rootserver still halted with `unknown syscall 0` once the BootInfo pointer
was valid.  Investigation confirmed the syscall wrapper constants were
positive even though the seL4 ABI expects negative numbers. The constants in
`src/sys.rs` now use `-9`, `-3`, `-5`, `-7` and `-11`. The new test
`debug_putchar_const` ensures the value for `SYS_DEBUG_PUTCHAR` remains
correct.

### Rootserver VSpace Layout

```
0x0040_0000  .vectors
0x0040_1000  .text
0x0040_2000  .rodata
0x0040_4000  .data
0x0040_6000  .bss
0x0040_8000-0x0048_8000  heap
0x0048_9000-0x0049_9000  stack
0x0900_0000  UART MMIO
BootInfo frame mapped at runtime
```

## libsel4 Mismatch 2028-07-19

Updating the seL4 sources replaced `libsel4.a` with a 32-bit ARM build.
Linking the aarch64 rootserver against this archive failed with `ld.lld`
reporting incompatible object files. The fix was to remove the library from
`link.ld` and the build script, then implement `seL4_GetBootInfo` directly in
Rust to return the saved pointer from `_start`. Rebuilding now produces a
64-bit ELF that links cleanly.
\n## HeaderIntegrationAudit 2028-08-30\n\nseL4 headers under third_party/seL4/include are now used directly by cohesix_root. build.rs defines a local sel4/config.h to satisfy the includes and bindgen regenerates bindings.rs from sel4/syscall.h. libsel4.a and the sel4-sys crate were removed from the rootserver build.

## seL4 Path Audit 2028-09-04

All artefact paths in the codebase were cross-checked against `third_party/seL4/sel4_tree.txt`. No references to 32-bit builds remain. The build scripts reference `third_party/seL4/artefacts/elfloader` and `kernel.dtb` for AArch64. `libsel4.a` is linked from `third_party/seL4/lib` in `sel4-sys` and `cohesix_fetch_build.sh`. The only mismatch found was `sel4-sys/wrapper.h` including `sel4/bootinfo_types.h`, which is absent under `include/`; a local replacement exists in `cohesix_root`.
