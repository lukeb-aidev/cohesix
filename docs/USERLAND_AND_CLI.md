<!-- Author: Lukas Bower -->
# Cohesix Userland & CLI

## 1. Philosophy
Cohesix replaces the traditional Unix shell with a deterministic file-oriented control plane. All human and automated interactions go through 9P namespaces exposed by NineDoor. The primary operator tool is the **Cohesix Shell (`cohsh`)**, a Rust REPL that translates commands into 9P operations.

## 2. Command Surface
| Command | Role | Effect |
|---------|------|--------|
| `ls [path]` | All | Enumerate directory entries via `walk` + `read` |
| `cat <path>` | All | Stream file contents |
| `echo <text> > <path>` | Queen, Worker roles | Append text to append-only files |
| `spawn <role> [opts]` | Queen | Append JSON spawn command to `/queen/ctl` |
| `kill <worker_id>` | Queen | Append kill command |
| `bind <src> <dst>` | Queen | Request namespace bind |
| `mount <service> <path>` | Queen | Mount host/VM services |
| `tail <path>` | All | Continuous read with backoff |
| `log` | Queen | Shortcut for `tail /log/queen.log` |
| `quit` | All | Close session |

> **Milestone 1 Note:** The `cohsh` prototype uses a mocked transport (`--mock`, enabled by default)
> that exposes `attach`, `login`, `tail`, and `quit`. Additional commands in the table above are
> documented for future milestones.

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

## 5. Packaging & Distribution
- `cohsh` is built as a standalone static binary for macOS and Linux hosts.
- Provide Homebrew formula and Cargo install instructions once CLI stabilises.
- CLI config (`~/.config/cohesix/cohsh.toml`) stores host transport endpoints and saved tickets.

## 6. Testing Checklist
- Unit tests cover command parsing and error messaging.
- Integration tests verify spawn/kill flows against a mocked NineDoor server.
- End-to-end test boots QEMU, attaches as queen, spawns a heartbeat worker, validates telemetry, then tears down.

## 7. Accessibility & UX
- Commands should return deterministic, human-readable messages.
- Provide `help` command summarising available actions per role.
- Consider tab completion via Rustyline once base functionality stabilises.
