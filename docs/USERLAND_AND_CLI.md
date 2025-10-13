<!-- Author: Lukas Bower -->
# Cohesix Userland & CLI

## 1. Philosophy
Cohesix replaces the traditional Unix shell with a deterministic file-oriented control plane. All human and automated interactions go through 9P namespaces exposed by NineDoor. The primary operator tool is the **Cohesix Shell (`cohsh`)**, a Rust REPL that translates commands into 9P operations.

## 2. Command Surface
| Command | Role | Effect |
|---------|------|--------|
| `help` | All | Print command summary, authenticated role, and active transports |
| `ls [path]` | All | Enumerate directory entries via `walk` + `read` |
| `cat <path>` | All | Stream file contents |
| `echo <text> > <path>` | Queen, Worker roles | Append text to append-only files |
| `spawn <role> [opts]` | Queen | Append JSON spawn command to `/queen/ctl` |
| `kill <worker_id>` | Queen | Append kill command |
| `bind <src> <dst>` | Queen | Request namespace bind |
| `mount <service> <path>` | Queen | Mount host/VM services |
| `tail <path>` | All | Continuous read with backoff; respects TCP heartbeats |
| `log` | Queen | Shortcut for `tail /log/queen.log` |
| `quit` | All | Close session and release ticket |

> **Milestone 7 Alignment:** All verbs are serviced through the
> authenticated command loop shared by serial and TCP transports. The
> parser enforces the budget, rate limiting, and capability checks
> described in `docs/BUILD_PLAN.md` milestones 7a–7c.

## 3. Example Sessions
### Queen Session
```
coh> ls /
proc queen log worker
coh> cat /proc/boot
Cohesix v0 (ARM64)
coh> spawn heartbeat ticks=100 ttl=60
Spawned worker id=worker-1
coh> tail /worker/worker-1/telemetry
{"tick":1,"ts_ms":...}
{"tick":2,"ts_ms":...}
coh> kill worker-1
Killed worker id=worker-1
coh> quit
```

### WorkerHeartbeat Session
```
coh(worker-7)> echo '{"tick":42}' > /worker/self/telemetry
coh(worker-7)> tail /log/queen.log
```

## 4. Scripted Automation
- `cohsh --script scripts/smoke.coh` executes newline-delimited commands and exits non-zero on first failure.
- `cohsh --json` emits machine-readable events for CI pipelines.
- `cohsh --mock` uses the in-memory Secure9P transport for integration tests without launching QEMU.

## 5. QEMU & Serial Console Workflow
Milestones 7a and 7b retire the legacy busy loop and expose a
deterministic serial console serviced by the event pump. Launch QEMU via
`scripts/qemu-run.sh` to exercise the integrated flow:

```
# Build artefacts and boot QEMU with the serial console attached
scripts/qemu-run.sh --profile debug --console serial --exit-after 120
```

- The script validates macOS host prerequisites, boots seL4 + Cohesix,
  and wires the virtio-console into the root-task serial driver.
- Pass `--console inline` to keep serial IO in the invoking terminal or
  `--console macos-terminal` to spawn a fresh Terminal.app session. Both
  surfaces use the authenticated parser introduced in Milestone 7a.
- Enable networking with `--net` to confirm the event pump polls smoltcp
  without starving serial or timer work. Audit logs such as
  `event-pump: init serial` and `event-pump: init net` confirm subsystem
  activation order matches the Build Plan requirements.
- The script prints the serial device path (or spawned terminal name) so
  operators can capture it in test logs and audits.

## 6. TCP Transport & Remote Sessions
Milestone 7c extends `cohsh` with a TCP transport that mirrors serial
behaviour:

- `cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337` connects
  to the root-task listener exposed by `scripts/qemu-run.sh --tcp-port
  31337`. The helper refuses non-localhost binds and prints the bound
  endpoint for audit trails.
- The TCP protocol is line oriented: the client sends `ATTACH <role>
  <ticket?>` and expects an `OK` or `ERR` response before issuing verbs
  such as `TAIL /log/queen.log`. Streams terminate with an `END`
  sentinel and `PING` / `PONG` heartbeats keep idle sessions alive.
- Rate limiting and line-length enforcement mirror the serial console:
  commands longer than 128 bytes are rejected and more than three failed
  logins within 60 seconds trigger a 90-second lockout reported as
  `RateLimited`. `cohsh` validates worker tickets locally (64 hex or
  base64url) and prints connection telemetry (`[cohsh][tcp] reconnect
  attempt …`, heartbeat latency) to stderr so operators know when
  exponential back-off is in effect.
- Scripts reuse the NineDoor command surface because the TCP transport
  proxies console verbs into the running root task. Reconnects are
  automatic; long-running `tail` commands resume after transient drops
  without discarding buffered output while the event pump continues to
  service timers and networking polls.

## 7. Packaging & Distribution
- `cohsh` is built as a standalone static binary for macOS and Linux hosts.
- Provide Homebrew formula and Cargo install instructions once CLI stabilises.
- CLI config (`~/.config/cohesix/cohsh.toml`) stores host transport endpoints and saved tickets.

## 8. Testing Checklist
- Unit tests cover command parsing and error messaging.
- Integration tests verify spawn/kill flows against a mocked NineDoor server.
- End-to-end test boots QEMU, attaches as queen, spawns a heartbeat worker, validates telemetry, then tears down. The TCP
  transport suite reuses these flows by forwarding the console port and driving the same scripted session over sockets. The
  `tests/integration/qemu_tcp_console.rs` harness exercises attach/log/quit behaviour with simulated disconnects to guard the
  reconnect logic while confirming the event pump keeps timers and networking responsive.

## 9. Accessibility & UX
- Commands should return deterministic, human-readable messages.
- Provide `help` command summarising available actions per role.
- Consider tab completion via Rustyline once base functionality stabilises.
