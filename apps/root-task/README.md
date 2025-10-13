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

Host-mode simulations continue to live in `src/host.rs`; they can be
expanded to exercise the event pump by wiring deterministic timers or
mock serial transports as the milestone progresses.

## Build Plan Milestone 7 Status

- **7a (Event Pump & Authenticated Entry)** — The cooperative pump and
  authentication scaffolding exist, but QEMU still aborts during the
  initial PL011 mapping, preventing verification of serial/network
  activation logs mandated by the milestone.
- **7b (Console & Networking Integration)** — Virtio/serial wiring is
  gated by the same boot failure; runtime telemetry now surfaces the
  failing `seL4_Untyped_Retype` call so future work can confirm the pump
  reaches the networking initialisation milestones.
- **7c (Follow-on tasks)** — Dependent items remain blocked until the
  root-task can successfully map the UART and complete the early boot
  pipeline.

Recent debug instrumentation records the precise untyped capability,
destination slot, and object type involved in the failing retype at
`kernel.rs:258`, clarifying that seL4 rejects the destination slot before
any device mapping occurs.
