<!-- Author: Lukas Bower -->
# Root Task Crate

The `root-task` crate embodies the responsibilities described in
`docs/ARCHITECTURE.md` §1-§3:

- Own the initial capabilities and boot flow transferred from seL4.
- Configure timers and the scheduling surface that powers worker budget enforcement.
- Bootstrap the NineDoor 9P server and provision worker tickets for the queen orchestration loop.
- Orchestrate serial, timer, networking, and IPC work through a
  cooperative event pump that replaces the legacy busy loop.

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

Host-mode simulation in `src/host.rs` now reuses the production event
pump. A scripted set of console commands is injected via a loopback
serial driver so developers can observe the authenticated command flow
and audit output without booting QEMU. The harness wires a sleep-backed
timer and exercises both queen and worker tickets, ensuring host runs
stay aligned with the seL4 entry path.

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
