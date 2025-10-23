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
Commands do not currently stream structured responses. Instead the
`AuditSink` implementation writes prefixed audit lines to the seL4 debug
console (visible on the QEMU serial log) while NineDoor forwarding stubs
mirror accepted verbs for future integration. 【F:apps/root-task/src/event/mod.rs†L314-L347】【F:apps/root-task/src/ninedoor.rs†L15-L63】
Early boot traces always use the UART path; the IPC sink remains disabled until `sel4::ep_ready()` publishes the root endpoint, preventing send-phase faults. 【F:apps/root-task/src/trace.rs†L10-L62】【F:apps/root-task/src/sel4.rs†L74-L154】

## 3. Example Sessions
The host-mode harness demonstrates the exact command sequence exercised
under QEMU. The scripted input runs `help`, authenticates as queen,
invokes `log`/`spawn`, re-attaches as a worker, issues `tail`, then
terminates with `quit`. 【F:apps/root-task/src/host.rs†L52-L104】

## 4. Scripted Automation
`cohsh` continues to ship scripting helpers that target the in-process
Secure9P mock until the TCP transport is feature-complete:

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
consistent attach/command feedback regardless of transport. 【F:apps/root-task/src/net/virtio.rs†L200-L244】【F:apps/cohsh/src/lib.rs†L470-L516】【F:apps/cohsh/src/transport/tcp.rs†L207-L356】
