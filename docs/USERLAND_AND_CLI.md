<!-- Author: Lukas Bower -->
# Cohesix Userland & CLI

## 1. Philosophy
Cohesix replaces the traditional Unix shell with a deterministic file-oriented control plane. All human and automated interactions go through 9P namespaces exposed by NineDoor. The primary operator tool is the **Cohesix Shell (`cohsh`)**, a Rust REPL that translates commands into 9P operations.

## 2. Root-task Console (Milestone 7)
The authenticated console introduced in Milestone 7 is implemented by
`apps/root-task/src/console/mod.rs` and shared by serial plus TCP
transports. The parser is heapless, validates UTF-8, and enforces
per-command capability checks inside the event pump.

### 2.1 Supported Commands
| Command | Role | Effect |
|---------|------|--------|
| `help` | All | Emit an audit line indicating that help was requested |
| `attach <role> [ticket]` | All | Authenticate the session and bind a role; workers must supply a ticket |
| `tail <path>` | Worker, Queen | Record a request to stream a path via NineDoor once the bridge is online |
| `log` | Queen | Shortcut for initiating the log stream verb |
| `spawn <json>` | Queen | Forward JSON payloads to NineDoor for worker orchestration |
| `kill <worker>` | Queen | Request termination of a worker via NineDoor |
| `quit` | All | Close the session |

> **Note:** File-oriented verbs such as `ls` and `cat` are provided by the
> Secure9P namespace once NineDoor lands in Milestone 8; they are not part
> of the Milestone 7 console.

### 2.2 Authentication & Limits
- Maximum console line length is 128 characters; role identifiers are
  capped at 16 characters and tickets at 128 characters. 【F:apps/root-task/src/console/mod.rs†L11-L36】
- The login rate limiter blocks sessions that exceed three failed
  attempts within 60 seconds for 90 seconds. 【F:apps/root-task/src/console/mod.rs†L38-L87】
- The event pump records outcomes in `PumpMetrics`, incrementing
  `accepted_commands` and `denied_commands` counters for auditability. 【F:apps/root-task/src/event/mod.rs†L84-L115】【F:apps/root-task/src/event/mod.rs†L240-L347】

### 2.3 Output Semantics
The console emits explicit acknowledgements for every command. Both
serial and TCP transports return `OK <VERB> …` or `ERR <VERB>
reason=<cause>` before any side effects occur so operators and
automation can synchronise on deterministic state transitions. These
acknowledgements are produced by the shared event-pump dispatcher and are
mirrored by the audit sink, ensuring `/log/queen.log` records the same
outcome. Streaming verbs such as `tail` and `log` also emit an
acknowledgement before beginning payload delivery and terminate with
`END`. 【F:apps/root-task/src/event/mod.rs†L329-L360】【F:apps/root-task/src/ninedoor.rs†L4-L63】【F:apps/root-task/src/net/queue.rs†L526-L559】
Early boot traces always use the UART path; the IPC sink remains disabled until `sel4::ep_ready()` publishes the root endpoint, preventing send-phase faults. 【F:apps/root-task/src/trace.rs†L10-L62】【F:apps/root-task/src/sel4.rs†L74-L154】

## 3. Example Sessions
The host-mode harness demonstrates the exact command sequence exercised
under QEMU. The scripted input runs `help`, authenticates as queen and
observes the `OK ATTACH` acknowledgement, invokes `log`/`spawn`,
re-attaches as a worker, issues `tail` (receiving `OK TAIL` before the
stream), then terminates with `quit`. 【F:apps/root-task/src/host.rs†L52-L104】

## 4. Scripted Automation
`cohsh` continues to ship scripting helpers that exercise the in-process
Secure9P mock even though the TCP transport is now feature-complete:

- `cohsh --script scripts/smoke.coh` executes newline-delimited commands
  against the mock transport and aborts on the first error.
- `cohsh --json` emits machine-readable events for CI.
- `cohsh --mock` runs entirely in-process without launching QEMU, easing
  unit testing of parser behaviour.

## 5. QEMU & Serial Console Workflow
Use `scripts/qemu-run.sh` to package a compiled root task into a CPIO
archive and boot seL4 under QEMU. Supply pre-built elfloader and kernel
artefacts produced by the upstream build system:

```
scripts/qemu-run.sh \
  --elfloader out/elfloader --kernel out/kernel \
  --root-task target/aarch64-unknown-none/release/root-task \
  --out-dir out/qemu-run --tcp-port 31337
```

- The helper assembles the root task into `/bin/root-task` inside the
  generated CPIO archive and prints SHA-256 digests for traceability. 【F:scripts/qemu-run.sh†L63-L116】【F:scripts/qemu-run.sh†L188-L204】
- Passing `--tcp-port` wires QEMU user networking so host clients can
  send console commands over TCP while the PL011-backed serial channel
  continues to accept the same verbs. 【F:scripts/qemu-run.sh†L127-L188】
- Serial output includes seL4 boot logs followed by root-task audit
  lines such as `event-pump: init serial` and `attach accepted`.

## 6. TCP Transport Status
The virtio-net backed `NetStack` listens on TCP port 31337 and mirrors the
serial console: each command receives an `OK <verb> ...` or `ERR <verb>
reason=...` acknowledgement that travels over both transports. Heartbeat
probes continue to use `PING`/`PONG`, and `TAIL` replies now emit an
acknowledgement before streaming log lines terminated by `END`. The host-side
`cohsh` client surfaces these responses as `[console] ...` lines so scripts see
consistent attach/command feedback regardless of transport. 【F:apps/root-task/src/net/stack.rs†L204-L264】【F:apps/cohsh/src/lib.rs†L471-L519】【F:apps/cohsh/src/transport/tcp.rs†L203-L356】
