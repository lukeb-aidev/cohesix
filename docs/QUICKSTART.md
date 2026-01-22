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
- `traces/` - canonical trace + hive replay snapshot (and hashes) for deterministic replay.
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

## Run the Live Hive demo 
You need two terminals:
- Terminal 1: QEMU (keeps the VM running).
  - Note: Qemu will show a serial terminal, used for core seL4 diagnostics. This is NOT the main user interface.
- Terminal 2: for either `cohsh` or SwarmUI. Use one at a time; they should not be used simultaneously.

1. In Terminal 1, Boot the VM:
   ```bash
   ./qemu/run.sh
   ```
2. In Terminal 2, connect with cohsh (control-plane actions are CLI-driven):
   ```bash
   ./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen
   ```
   The default console auth token is `changeme`. If you see `ERR AUTH`, set
   `COHSH_AUTH_TOKEN` or pass `--auth-token`.
3. In cohsh, run a few actions:
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

4. Now, "quit" from cohsh and launch SwarmUI if you use Mac OS or Gnome:
   ```bash
   ./bin/swarmui
   ```
   On headless Linux, use:
   ```bash
   xvfb-run -a ./bin/swarmui
   ```
## Run the SwarmUI deterministic replay demos
Quit SwarmUI

```bash
./bin/swarmui --replay-trace "$(pwd)/traces/trace_v0.trace"
```

```bash
./bin/swarmui --replay "$(pwd)/traces/trace_v0.hive.cbor"
```
Headless Linux replay:
```bash
xvfb-run -a ./bin/swarmui --replay-trace "$(pwd)/traces/trace_v0.trace"
```

SwarmUI auto-starts the Live Hive replay when `--replay-trace` is used — no Demo button required.
The replay should show:
- multiple agents (queen + heart/gpu workers) drifting in clusters,
- pollen streams flowing toward the queen on telemetry bursts,
- heat glows around active agents,
- red error pulses when GPU/heartbeat faults occur.

Canonical trace location:
- `traces/trace_v0.trace`
- `traces/trace_v0.trace.sha256`
Hive replay snapshot (used by SwarmUI for Live Hive visuals):
- `traces/trace_v0.hive.cbor`
- `traces/trace_v0.hive.cbor.sha256`

## Optional host tool demos
These are safe demo commands to prove the host tooling works. Live uploads require QEMU to be running.

### cas-tool (pack + upload)
```bash
./bin/cas-tool pack --epoch v1 --input ./traces/trace_v0.trace --out-dir ./out/cas/v1
./bin/cas-tool upload --bundle ./out/cas/v1 --host 127.0.0.1 --port 31337 \
  --auth-token changeme --ticket "$QUEEN_TICKET"
```

### gpu-bridge-host (mock list)
```bash
./bin/gpu-bridge-host --mock --list
```
NVML discovery requires rebuilding with `--features nvml`.

### host-sidecar-bridge (mock + live)
```bash
./bin/host-sidecar-bridge --mock --mount /host --provider systemd --provider k8s --provider nvidia
```
Live publish over TCP (bundle includes TCP support):
```bash
./bin/host-sidecar-bridge --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme
```
The `/host` namespace must be enabled in `configs/root_task.toml` for live publishing.

## Ports and signals
- TCP console: `127.0.0.1:31337`
- UDP echo test: `127.0.0.1:31338`
- TCP smoke test: `127.0.0.1:31339`
