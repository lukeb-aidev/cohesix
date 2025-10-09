// CLASSIFICATION: COMMUNITY
// Filename: USERLAND_BOOT.md v0.7
// Author: Lukas Bower
// Date Modified: 2030-10-09

## Kernel entry hang triage 2030-10-09

When the boot halts immediately after the ELF loader transfers control to the
kernel, treat it as an EL1 fault that fires before seL4 can bring up the debug
console. The following checklist narrows the problem space before we change any
Plan 9 components:

1. **Re-enable kernel diagnostics while reproducing the fault.** The generated
   `gen_config.h` currently disables both `CONFIG_DEBUG_BUILD` and
   `CONFIG_PRINTING`, which prevents early printk output and strips extra
   assertions from the kernel.【F:third_party/seL4/include/generated/kernel/gen_config.h†L69-L108】
   Re-run the seL4 configuration with those options set to `y` so the rebooting
   kernel logs assertion sites to the UART. The rootserver already warns when it
   detects `CONFIG_PRINTING=0`, so you should see this message disappear after
   regenerating the kernel image.【F:workspace/cohesix_root/src/main.rs†L554-L566】

2. **Capture the faulting PC using the existing QEMU trace harness.** The
   `debug_qemu_boot.sh` helper stores both the invocation and the trace output
   under `logs/`, and it already enables `-d in_asm,exec,…` to record the last
   guest instructions before QEMU exits.【F:scripts/debug_qemu_boot.sh†L10-L85】
   Open `logs/qemu_boot_trace.log` immediately after a failed boot and scroll to
   the final `guest_errors` or `exec` entries to identify the exact PC/VA pair
   that faulted.

3. **Stop execution before the kernel entry point and attach GDB.** Re-run the
   command recorded in `logs/qemu_invocation.log`, append `-gdb tcp::1234`, and
   let the existing `-S` flag hold the CPUs in reset while you attach
   `aarch64-none-elf-gdb`.【F:scripts/debug_qemu_boot.sh†L10-L85】 Load
   `kernel.elf` as the symbol file, set breakpoints on `init_kernel` (or the
   first C entry symbol reported by `nm kernel.elf`), and single-step until the
   exception triggers. Inspecting `x0`–`x3`, `elr_el1`, and `spsr_el1` inside GDB
   usually reveals whether we jumped to an unmapped address or tripped a data
   abort during the first capability retype.

4. **Keep semihosting enabled so EL1 panics mirror into the host.** The root
   server already mirrors every `coherr!` byte into the semihosting channel when
   `coh.semihost=stdout` (or `file:`) appears in `/boot/bootargs.txt`, and
   `handle_bootarg` switches the mode before Plan 9 processes any other
   environment variables.【F:workspace/cohesix_root/src/sys.rs†L36-L114】【F:workspace/cohesix_root/src/main.rs†L359-L412】【F:workspace/cohesix_root/src/semihosting.rs†L8-L134】
   Leave this in place while you single-step the kernel so the EL1 panic handler
   still records a register dump into `/log/boot_ring` if the fault happens after
   the rootserver resumes.

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

