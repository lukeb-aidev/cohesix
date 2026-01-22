<!-- Copyright (c) 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Quickstart instructions for Cohesix alpha bundle runs. -->
<!-- Author: Lukas Bower -->
# Cohesix Alpha Quickstart

## What is Cohesix?
Cohesix is a small control-plane operating system for secure orchestration and telemetry of
edge GPU nodes. It runs as a seL4 VM and exposes a file-shaped Secure9P namespace instead
of a traditional filesystem. A deployment is a "hive": a queen role orchestrates worker
roles (worker-heart for telemetry and worker-gpu for lease state), while host tools attach
to the TCP console to drive and observe the system.

## Bundle layout
- Bundles are host-OS specific; use the `-linux` tarball on Linux hosts.
- `bin/` - host tools (`cohsh`, `swarmui`, `cas-tool`, `gpu-bridge-host`, `host-sidecar-bridge`).
- `configs/` - manifest inputs for host tools (includes `root_task.toml` for ticket minting).
- `image/` - prebuilt VM artifacts (elfloader, kernel, rootserver, CPIO, manifest).
- `qemu/run.sh` - QEMU launcher for the bundled image.
- `traces/` - canonical trace plus hash for deterministic replay.
- `ui/swarmui/` - SwarmUI frontend assets.
- `docs/` - background docs for curious readers (architecture, interfaces, roles).
- `README.md` - high-level project overview.

## Host tools at a glance
- `cohsh` - primary CLI shell; use it to attach to the queen, run commands, and read logs.
- `swarmui` - UI for replay or live observation; live mode is read-only.
- `cas-tool` - package and upload bundles to the `/updates` namespace (optional).
- `gpu-bridge-host` - host GPU discovery for the `/gpu` namespace (optional).
- `host-sidecar-bridge` - publish host providers into `/host` (optional).
See `docs/HOST_TOOLS.md` for details.

## Setup host runtime (required once per host)
Install or verify runtime dependencies (QEMU + SwarmUI runtime libs):
```bash
./scripts/setup_environment.sh
```
On Ubuntu this uses `apt-get` (via `sudo` if needed). On macOS it uses Homebrew.

## Run the live hive demo (read-only UI)
You need two terminals:
- Terminal 1: QEMU (keeps the VM running).
- Terminal 2: either `cohsh` or SwarmUI. Use one at a time; they should not be used simultaneously.

1. Boot the VM:
   ```bash
   ./qemu/run.sh
   ```
2. Connect with cohsh (control-plane actions are CLI-driven):
   ```bash
   ./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen
   ```
   The default console auth token is `changeme`. If you see `ERR AUTH`, set
   `COHSH_AUTH_TOKEN` or pass `--auth-token`.
3. In cohsh, run a few actions (for example):
   - `help`
   - `spawn heartbeat ticks=100`
   - `tail /log/queen.log`
4. Launch SwarmUI:
   ```bash
   ./bin/swarmui
   ```

SwarmUI is observational in live mode; issue mutations only via cohsh.

## cohsh command examples
Examples for the built-in command surface (use in the cohsh prompt):
- `help` — show the command list.
- `attach queen` — attach to the queen role.
- `login queen` — alias of attach.
- `detach` — close the current session.
- `tail /log/queen.log` — stream the queen log.
- `log` — shorthand for `tail /log/queen.log`.
- `ping` — report attachment status.
- `test --mode quick` — run quick self-tests.
- `pool bench --duration 2s --clients 4` — run a short pool benchmark.
- `tcp-diag` — debug TCP connectivity without protocol traffic.
- `ls /` — list root namespace entries.
- `cat /log/queen.log` — read the queen log once.
- `echo hello > /log/queen.log` — append a line to the log.
- `spawn heartbeat ticks=100` — request a heartbeat worker.
- `kill 1` — terminate worker id 1.
- `bind /queen /host/queen` — bind a path.
- `mount ninedoor /ninedoor` — mount the NineDoor namespace.
- `quit` — exit cohsh.

## Run the deterministic replay demo
Canonical trace location:
- `traces/trace_v0.trace`
- `traces/trace_v0.trace.sha256`

Replay via cohsh (mock transport is required for trace replay):
```bash
./bin/cohsh --transport mock --replay-trace ./traces/trace_v0.trace
```

Replay via SwarmUI:
```bash
./bin/swarmui --replay-trace "$(pwd)/traces/trace_v0.trace"
```

## Verification note
Already verified for this bundle at a high level: Build Integrity, CLI Semantics, Role
Enforcement, Concurrency, Regression, and Packaging (Cohesix Test Plan phases).
You are expected to run the demo steps above and observe that SwarmUI matches cohsh output.

## Ports and signals
- TCP console: `127.0.0.1:31337`
- UDP echo test: `127.0.0.1:31338`
- TCP smoke test: `127.0.0.1:31339`

The QEMU launcher prints the ready line and connection hints once the console is available.
