<!-- Author: Lukas Bower -->
<!-- Purpose: Documents the root-task crate responsibilities and scope. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Root Task Crate

The `root-task` crate embodies the responsibilities described in
`docs/ARCHITECTURE.md` §1-§3:

- Own the initial capabilities and boot flow transferred from seL4.
- Configure timers and the scheduling surface that powers worker budget enforcement.
- Bootstrap the NineDoor 9P server and provision worker tickets for the queen orchestration loop.
- Orchestrate serial, timer, networking, and IPC work through a
  cooperative event pump that replaces the legacy busy loop.

## Dual-Mode Builds

`root-task` now supports both host-mode development (default via the
`host` feature) and kernel-mode execution on upstream seL4. Host builds
continue to use `std` so integration tests and mocks run on macOS, while kernel builds
are activated via the `kernel` feature and compile with `no_std` and
`sel4_runtime` providing the `_start` shim. The crate ships with an empty
`default = []` feature set, so kernel builds must list every required
capability explicitly with `--no-default-features`. Use the following
commands when switching modes:

```
# Host defaults for unit and integration tests
cargo test -p root-task

# Kernel-mode QEMU release target (serial + TCP + VirtIO + USB)
cargo build -p root-task --no-default-features --features release-qemu --target aarch64-unknown-none --release

# Kernel-mode Pi 4 release target (serial + TCP + local seat/GENET/Wi-Fi/USB)
cargo build -p root-task --no-default-features --features release-pi4 --target aarch64-unknown-none --release

# Guard to ensure sel4_start is present and milestone modules remain
scripts/check-root-task.sh <path-to-rootserver-elf>
```

The workspace now carries `.cargo/config.toml` target metadata so the
kernel build injects the seL4 linker script without disturbing host
settings. See `apps/root-task/src/main.rs` for the dual entry path and
`apps/root-task/src/platform.rs` for the platform abstraction that
bridges seL4 debug I/O and the host-mode console harness.

### Feature flags and dev boot profiles

All feature flags live in `Cargo.toml` with no defaults enabled. The two
release bundles are the supported target configurations when using
`--no-default-features`; both include TCP (`net-console`) and USB:

- **QEMU release target (PL011 serial, TCP console, VirtIO networking, USB)**

  ```
  cargo build \
    --target aarch64-unknown-none \
    --release \
    -p root-task \
    --no-default-features \
    --features release-qemu
  ```

- **Pi 4 release target (PL011/mini-UART serial, TCP console, local seat, GENET, Wi-Fi, USB)**

  ```
  cargo build \
    --target aarch64-unknown-none \
    --release \
    -p root-task \
    --no-default-features \
    --features release-pi4
  ```

`cohesix-dev` layers QEMU trace/self-test diagnostics on top of
`release-qemu`. Focused driver validation uses `driver-tests-qemu` and
`driver-tests-pi4` so the two target surfaces can be tested without ad hoc
feature strings. Both release bundles include TCP and the `usb` feature. On the
Pi 4 profile, USB acceptance comes only from the linked `pi4-driver-usb` runtime
and its owner-state proof; root-task local-seat code is a ring client and does
not select a root-owned USB implementation crate.

## Event Pump Overview

`src/event/mod.rs` introduces `EventPump`, a no-`std` coordinator that
rotates through serial IO (`serial::SerialPort`), timer ticks,
feature-gated networking polls, and IPC dispatch. Each cycle emits
structured audit lines (`event-pump: init …`, `attach accepted`,
`attach denied`) so operators can correlate subsystem activity with the
serial log. Authentication is backed by a deterministic
`TicketTable`, ensuring that bootstrap tickets are validated without
heap allocation.

## Serial Console

`src/serial/mod.rs` provides a heapless serial façade. Input is
sanitised to UTF-8, backspaces are honoured, and atomic counters expose
RX/TX saturation metrics for `/proc/boot`. The console parser enforces
maximum line length, exponential back-off for repeated authentication
failures, and capability checks before invoking orchestration verbs.

## Testing & Feature Flags

The crate ships unit and integration tests that exercise the event pump
and console authentication flows:

```
cargo test -p root-task event_pump
```

Networking glue lives behind the `net` feature flag so developers can
iterate on console-only changes without pulling in smoltcp. Enable the
feature when validating the bounded virtio queues or smoltcp polls:

```
cargo check -p root-task --features net
cargo clippy -p root-task --features net --tests
```

Driver and HAL changes should use the release-aligned bundles instead of
hand-built feature strings:

```
python3 scripts/ci/check_driver_test_coverage.py
cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::rtl8139
cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::virtio
cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::pci
cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::virtio_mmio
cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::uart
cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::driver_task_net
cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_pcie
cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi
cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::
cargo test -p pi4-driver-runtime --lib -- --test-threads=1
cargo test -p root-task --no-default-features --features cache-maintenance --test cache_maintenance
SEL4_BUILD_DIR=$REPO/seL4/SMP_build cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-qemu
SEL4_BUILD_DIR=$REPO/seL4/build_UBOOT cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-pi4
```

### Debug Console Input

The seL4 debug console exposes a non-blocking polling syscall on a subset
of architectures. Opt into this behaviour with the `debug-input` feature
when the target kernel exports `seL4_DebugPollChar`:

```
cargo build -p root-task --release \
  --no-default-features --features kernel,debug-input
```

When the syscall is absent for the selected platform the feature safely
falls back to returning `-1`, preserving the historical write-only
console semantics.

### Interactive Shell

Enable `debug-input` alongside `kernel` to reach the early interactive
console exposed on the PL011 UART:

```
cargo build -p root-task --release \
  --no-default-features --features kernel,debug-input \
  --target aarch64-unknown-none
```

Boot logs will now include the corrected capability path diagnostics and
device mapping beacons:

```
[cspace:init] root=0x0002 bits=13 window=[0x0104..0x2000)
[cnode:copy] src=TCB depth=64 -> dst=0x0104 OK
[retype:call] ut=0x00e6 type=5 size_bits=0 root=0x0002 index=0x0002 depth=64 offset=0x0105
[retype:ret] err=0
[vspace:map] pl011 paddr=0x09000000 -> vaddr=0x70000000 attrs=UNCACHED OK
[uart] init OK
```

Once the boot sequence reaches the interactive loop the shell presents a
`cohesix>` prompt and understands a minimal command set:

```
cohesix> help
Commands: help, echo <s>, hexdump <addr> <len>, caps, reboot
cohesix> caps
initCNode=0x0002 vspace=0x0003 tcb=0x0004 ep_console=0x0105 tcb_copy=0x0104
cohesix> hexdump 0x70000000 40
0x0000000070000000: 30 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00  |0...............|
...
```

### Why this works

The corrected bootstrap path now differentiates between two canonical
tuples supplied to seL4:

- **CNode tuple** — `(root=initCNode, depth=seL4_WordBits)` accompanies
  `CNode_Copy`/`CNode_Delete`, ensuring guard bits are honoured while
  resolving destination indices within the init thread CNode.
- **Retype tuple** — `(node_root=initCNode, node_index=initCNode,
  node_depth=seL4_WordBits)` is passed to `Untyped_Retype`, so the kernel
  treats `nodeOffset` as a slot inside the root CNode while preserving the
  guard parameters derived from the architectural word size.

Keeping these roles separate prevents the failed lookups that previously
wedged endpoint retype attempts and PL011 mappings during early boot.

Use the standard QEMU launch parameters to boot the resulting image:

```
qemu-system-aarch64 -machine virt,gic-version=2 -cpu cortex-a57 -m 1024 -smp 4,cores=4,threads=1,sockets=1 \
  -serial mon:stdio -display none \
  -kernel out/cohesix/staging/elfloader \
  -initrd out/cohesix/cohesix-system.cpio \
  -device loader,file=out/cohesix/staging/kernel.elf,addr=0x70000000,force-raw=on \
  -device loader,file=out/cohesix/staging/rootserver,addr=0x80000000,force-raw=on
```

The shell intentionally avoids IRQ-driven input so the root task remains
single-threaded during bring-up.

Host-mode simulation in `src/host.rs` now reuses the production event
pump. A scripted set of console commands is injected via a loopback
serial driver so developers can observe the authenticated command flow
and audit output without booting QEMU. The harness wires a sleep-backed
timer and exercises both queen and worker tickets, ensuring host runs
stay aligned with the seL4 entry path.

## QEMU Boot Readiness

- **Boot pipeline** — `sel4_start` drops into `kernel::start`, prints seL4 bootinfo, maps the
  PL011 UART, and installs the event pump before entering the polling
  loop so the image runs once the elfloader jumps into the root task on
  `qemu-system-aarch64`. 【F:apps/root-task/src/kernel.rs†L105-L210】【F:apps/root-task/src/kernel.rs†L252-L343】
- **Tickets** — The embedded `TicketTable` registers a queen ticket plus
  worker heartbeat/GPU placeholders, allowing authenticated attaches for
  all three roles during QEMU sessions. 【F:apps/root-task/src/kernel.rs†L333-L339】
- **Networking** — When built with `--no-default-features` and
  `--features cohesix-dev` the event pump initialises the virtio-net
  backed `NetStack`, binds to the
  static `10.0.0.2/24` address, and listens for TCP console input on
  port `31337`. User-space clients reach the listener via
  `scripts/qemu-run.sh --tcp-port <host-port>`, which wires QEMU user
  networking to the root task. 【F:apps/root-task/src/kernel.rs†L308-L331】【F:apps/root-task/src/net/virtio.rs†L119-L214】【F:scripts/qemu-run.sh†L127-L188】
- **Command loop** — The authenticated parser accepts `help`, `attach`,
  `tail`, `log`, `spawn`, `kill`, and `quit`. All verbs share the same
  capability checks and rate limiting across serial and TCP transports.
  Responses are emitted as audit lines on the seL4 debug console; the
  TCP listener currently consumes input without echoing structured
  responses, mirroring the host-mode simulation expectations. 【F:apps/root-task/src/console/mod.rs†L23-L112】【F:apps/root-task/src/event/mod.rs†L242-L347】【F:apps/root-task/src/net/virtio.rs†L185-L232】

## Build Plan Milestone 7 Status

- **7a (Event Pump & Authenticated Entry)** — The cooperative pump and
  authentication scaffolding now run past PL011 initialisation thanks to
  corrected CSpace path encoding during the UART retype sequence.
- **7b (Console & Networking Integration)** — Virtio/serial wiring can
  proceed under QEMU; diagnostics still surface retype telemetry, but
  the UART mapping now succeeds so subsequent milestones may validate
  the networking initialisation logs as planned.
- **7c (Follow-on tasks)** — Dependent items remain blocked until the
  root-task can successfully map the UART and complete the early boot
  pipeline.

Recent debug instrumentation continues to record the precise untyped
capability, destination slot, and object type involved in each retype at
`kernel.rs:258`, providing quick confirmation that the corrected path
parameters keep the UART mapping healthy and flagging any future device
regressions early.
