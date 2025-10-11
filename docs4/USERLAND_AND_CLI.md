# Cohesix Userland & CLI (Minimal Interactive Environment)
Date: 2025-10-11

Cohesix exposes a deterministic, file-based control surface instead of a Unix shell.  
User interaction occurs via the **Cohesix Shell (`cohsh`)**, a small Rust REPL that communicates with the 9P namespace exposed by the `nine-door` server.

```
cohsh (REPL) → 9P attach/walk/read/write → nine-door FS → /queen /worker /log /proc
```

## Commands
| Command | Description |
|----------|--------------|
| `ls [path]` | List directory entries |
| `cat <path>` | Read file contents |
| `echo <text> > <path>` | Append text to file |
| `spawn <type>` | Spawn worker (`heartbeat` or `gpu`) |
| `kill <id>` | Terminate worker |
| `tail <path>` | Stream appended data |
| `bind <src> <dst>` | Bind mount (Queen only) |
| `mount <svc> <path>` | Mount service (Queen only) |
| `log` | Show `/log/queen.log` tail |
| `quit` | Exit REPL |

### Example Session (Queen)
```
coh> ls /
proc queen log worker
coh> cat /proc/boot
Cohesix v0 (ARM64)
coh> spawn heartbeat
Spawned worker id=1
coh> tail /worker/1/telemetry
heartbeat 1
heartbeat 2
coh> kill 1
Killed worker id=1
coh> quit
```

### Example Session (WorkerHeartbeat)
```
coh(worker)> echo "heartbeat 42" > /worker/self/telemetry
```

## Role Permissions
| Role | Allowed Commands |
|------|------------------|
| **Queen** | All commands |
| **WorkerHeartbeat** | echo, cat, tail (own telemetry) |
| **WorkerGpu** | echo, cat, tail (own + gpu nodes) |
| **Observer (future)** | ls, cat, tail |

## Testing
- `ls`/`cat` verify 9P codec and mount table.
- `spawn`/`kill` test IPC and task lifecycle.
- `tail` tests append-only streams and revocation.
- `--script <file>` runs scripted tests.
- `--json` emits machine-readable output for CI.
- `--mock` runs against an in-memory 9P server.

## Implementation Sketch
```rust
pub enum Cmd { Ls, Cat, Echo, Spawn, Kill, Tail, Bind, Mount, Log, Quit }

fn run(cmd: Cmd, sess: &mut NinepSession) -> Result<(), CohError> {
    // Convert to 9P ops and send through transport
}
```
