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
- `python/` - Cohesix Python client (`cohesix`) and examples.
- `traces/` - canonical trace + hive replay snapshot (and hashes) for deterministic replay.
- `ui/swarmui/` - SwarmUI frontend assets.
- `docs/` - background docs for curious readers (architecture, interfaces, roles).
- `README.md` - high-level project overview.

## Host tools at a glance
- `cohsh` - primary CLI shell; use it to attach to the queen, run commands, and read logs.
- `coh` - host bridge CLI for mount/gpu/run/telemetry/peft plus `coh doctor`.
- `cohesix` (Python) - thin client with mock and TCP backends; examples under `python/cohesix-py/examples/`.
- `swarmui` - UI for replay or live observation with an embedded console panel (core verbs only).
- `cas-tool` - package and upload bundles to the `/updates` namespace (optional).
- `gpu-bridge-host` - host GPU discovery for the `/gpu` namespace (optional).
- `host-sidecar-bridge` - publish **mock** host providers into `/host` for policy/CI validation (optional).
See `docs/HOST_TOOLS.md` for details.

## Alpha2 highlights (milestones 21a-24)
- Telemetry ingest with OS-named segments: `cohsh telemetry push` + `coh telemetry pull`.
- Host bridge `coh` for Secure9P mount, GPU lease/status, and telemetry export (no new VM semantics).
- SwarmUI Live Hive visibility + embedded console panel that reuses the existing TCP session.
- Lifecycle controls (`cohsh lifecycle`) plus `/proc/lifecycle/*` and `/proc/root/*` cut signals.
- `coh run` command that records bounded GPU breadcrumb entries under `/gpu/<id>/status`.
- `coh peft` export/import/activate/rollback flows (LoRA lifecycle glue).
- Cohesix Python client + examples and `coh doctor` for deterministic host checks.

## Setup host runtime (required once per host)
Install or verify runtime dependencies (QEMU + SwarmUI runtime libs):
```bash
./scripts/setup_environment.sh
```
On Ubuntu this uses `apt-get` (via `sudo` if needed). On macOS it uses Homebrew.

## Run coh doctor + mock demos (fast)
These do not require QEMU and should finish quickly on a fresh host:
```bash
./bin/coh doctor --mock
python3 -m pip install -e python/cohesix-py
python3 python/cohesix-py/examples/lease_run.py --mock
python3 python/cohesix-py/examples/peft_roundtrip.py --mock
python3 python/cohesix-py/examples/telemetry_write_pull.py --mock
```
Note: in the source tree, the Python client lives under `tools/cohesix-py` instead of `python/cohesix-py`.

## Run the Live Hive demo 
You need two terminals:
- Terminal 1: QEMU (keeps the VM running).
  - Note: Qemu will show a serial terminal, used for core seL4 diagnostics. This is NOT intended to be the main user interface.
- Terminal 2: for either `cohsh` or `swarmui`. Use one at a time; they should not be used simultaneously.
- Command surface note (concise):
  - SwarmUI includes a console panel for core verbs; use `cohsh` for CLI-only commands.
  - `cohsh` includes the full console verbs plus CLI-only commands/options (for example `test --mode`, `pool bench`, `tcp-diag`, `--script`, `--mint-ticket`).

1. In Terminal 1, Boot the VM:
   ```bash
   ./qemu/run.sh
   ```
   Note: QEMU auto-selects hardware acceleration (`hvf` on macOS, `kvm` on Linux when `/dev/kvm` is accessible),
   falling back to `tcg` if unavailable. Override with `COHESIX_QEMU_ACCEL` or `QEMU_ACCEL`.
2. In Terminal 2, connect with cohsh (control-plane actions are CLI-driven):
   ```bash
   ./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen
   ```
   The default console auth token is `changeme`. If you see `ERR AUTH`, set
   `COHSH_AUTH_TOKEN` or pass `--auth-token`.
3. In cohsh, run a few actions:
- `help` — show the command list.
- `attach queen` — attach to the queen role (only if you started cohsh without `--role`).
- `login queen` — alias of attach (same rule as above).
- `detach` — close the current session (then use `attach` to reconnect).
- `tail /log/queen.log` — stream the queen log.
- `log` — shorthand for `tail /log/queen.log`.
- `ping` — report attachment status.
- `test --mode quick` — run quick self-tests.
- `pool bench path=/log/queen.log ops=200 batch=4 payload_bytes=64` — run a short pool benchmark.
- `tcp-diag` — debug TCP connectivity without protocol traffic.
- `ls /` — list root namespace entries.
- `cat /log/queen.log` — read the queen log once.
- `echo hello > /log/queen.log` — append a line to the log.
- `spawn heartbeat ticks=100` — request a heartbeat worker.
- `spawn gpu gpu_id=GPU-0 mem_mb=4096 streams=1 ttl_s=120` — request a GPU worker lease (see notes below).
- `ls /worker` — list current worker IDs (do not assume `worker-1`; use what you see).
- `kill worker-2` — terminate the worker id you just listed (replace with the actual id).
- `bind /queen /host/queen` — bind a path.
- `mount logs /logs` — mount the log service namespace (alias to `/log`).
- `cat /proc/lifecycle/state` — read the current lifecycle state.
- `cat /proc/root/reachable` — confirm root reachability and cut signals.
- `lifecycle cordon` — stop accepting new work.
- `lifecycle resume` — return to ONLINE.
- `quit` — exit cohsh.

Spawn notes:
- Supported roles are `heartbeat` (aliases: `worker`, `worker-heartbeat`) and `gpu` (alias: `worker-gpu`).
- Heartbeat spawns require `ticks=<n>` and accept optional `ttl_s=<n>` and `ops=<n>` budget controls.
  - ttl_s=<n> — time‑to‑live in seconds (budget)
  - ops=<n> — operation budget (budget)
- GPU spawns require a lease spec: `gpu_id`, `mem_mb`, `streams`, `ttl_s`. Optional: `priority`, `budget_ttl_s`, `budget_ops`.
- If `/gpu` is empty, run the host GPU bridge (`./bin/gpu-bridge-host --mock --list`) and try again.

Other optional args you can try:
- `test --mode full --timeout 120` — full self-tests with a longer timeout.
- `test --mode quick --no-mutate` — quick tests without spawn/kill.
- `tcp-diag 31337` — explicitly check the console port.

4. Now, "quit" from cohsh and launch SwarmUI if you use Mac OS or Gnome:
   ```bash
   ./bin/swarmui
   ```
   On headless Linux, use:
   ```bash
   xvfb-run -a ./bin/swarmui
   ```
   In SwarmUI, use `ls /worker` in cohsh to find a worker id before clicking “Load telemetry”. Spawn multiple workers if you want more activity.
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

SwarmUI auto-starts the Live Hive replay when launched with `--replay-trace` or `--replay` — no Demo button required.
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

### coh (host bridge)
```bash
./bin/coh gpu list --host 127.0.0.1 --port 31337
./bin/coh gpu lease --host 127.0.0.1 --port 31337 --gpu GPU-0 --mem-mb 4096 --streams 1 --ttl-s 60
./bin/coh run --host 127.0.0.1 --port 31337 --gpu GPU-0 -- echo ok
./bin/coh telemetry pull --host 127.0.0.1 --port 31337 --out ./out/telemetry
./bin/coh mount --mock --at /tmp/coh-mount
```
Note: live FUSE mounts require `coh` built with `--features fuse` and a running QEMU instance:
`./bin/coh mount --host 127.0.0.1 --port 31337 --at /tmp/coh-mount`

PEFT roundtrip (mock, no VM required):
```bash
mkdir -p out/peft_adapter
printf "adapter-bytes\n" > out/peft_adapter/adapter.safetensors
printf "{\"rank\":8}\n" > out/peft_adapter/lora.json
printf "{\"loss\":0.02}\n" > out/peft_adapter/metrics.json
./bin/coh peft export --mock --job job_8932 --out out/peft_export
./bin/coh peft import --mock --model prev-model --from out/peft_adapter --job job_8932 \
  --export out/peft_export --registry out/peft_registry
./bin/coh peft import --mock --model demo-model --from out/peft_adapter --job job_8932 \
  --export out/peft_export --registry out/peft_registry
./bin/coh peft activate --mock --model prev-model --registry out/peft_registry
./bin/coh peft activate --mock --model demo-model --registry out/peft_registry
./bin/coh peft rollback --mock --registry out/peft_registry
```

Telemetry ingest demo (requires QEMU running):
```bash
mkdir -p out/telemetry
printf "telemetry demo line 1\ntelemetry demo line 2\n" > out/telemetry/demo.txt
./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen \
  telemetry push out/telemetry/demo.txt --device device-1
./bin/coh telemetry pull --host 127.0.0.1 --port 31337 --out ./out/telemetry/pull
```

### cas-tool (pack + upload)
Note: CAS = Content‑Addressed Storage.
In Cohesix it’s the update mechanism where bundles are stored and referenced by a hash of their contents, so integrity is built‑in (the content defines the address). cas-tool prepares a signed, chunked bundle and uploads it to the /updates namespace so the queen can validate and apply it deterministically.

`cas-tool` requires a signing key (bundled at `resources/fixtures/cas_signing_key.hex`) and a payload size aligned to `cas.store.chunk_bytes` (128 bytes). Run the commands below from the bundle root (don’t paste the ``` lines into your shell):
```bash
mkdir -p out/cas
QUEEN_TICKET=$(./bin/cohsh --mint-ticket --role queen)
python3 - <<'PY'
from pathlib import Path
src = Path("traces/trace_v0.trace")
dst = Path("out/cas/trace_v0.padded")
data = src.read_bytes()
pad = (-len(data)) % 128
dst.write_bytes(data + b"\0" * pad)
print(f"padded {len(data)} -> {len(data) + pad} bytes")
PY
./bin/cas-tool pack --epoch 1 --input out/cas/trace_v0.padded --out-dir out/cas/1 \
  --signing-key resources/fixtures/cas_signing_key.hex
./bin/cas-tool upload --bundle out/cas/1 --host 127.0.0.1 --port 31337 \
  --auth-token changeme --ticket "$QUEEN_TICKET"
```
What this does: pads the trace to the 128-byte CAS chunk size, packs it into a signed update bundle (epoch 1), then uploads it to the queen’s `/updates` namespace over the TCP console using your minted queen ticket.

### gpu-bridge-host (mock list)
```bash
./bin/gpu-bridge-host --mock --list
```
NVML discovery is enabled by default on Linux bundles; use `--no-default-features` to omit NVML.

### host-sidecar-bridge (mock publishing)
```bash
./bin/host-sidecar-bridge --mock --mount /host --provider systemd --provider k8s --provider nvidia
```
Publish mock provider data over TCP (bundle includes TCP support):
```bash
./bin/host-sidecar-bridge --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme
```
The `/host` namespace must be enabled in `configs/root_task.toml`.

## Ports and signals
- TCP console: `127.0.0.1:31337`
- UDP echo test: `127.0.0.1:31338`
- TCP smoke test: `127.0.0.1:31339`

## Root console note
The serial root console (`cohesix>`) uses the same verb grammar as `cohsh`, but it does **not** parse `key=value` shorthand. 

When testing `swarmui`, you can use the root console to spawn more workers and expand the hive, which should be reflected in the Live Hive view.

`cohesix>` expects the raw JSON payloads used by NineDoor. For example, `spawn heartbeat ticks=25 ops=555` works in `cohsh`, but on the root console you must send:
```text
cohesix> spawn {"spawn":"heartbeat","ticks":25,"budget":{"ops":555}}
```
Root console commands still require a session. If you see `ERR ... reason=unauthenticated`, attach with a queen ticket first:
```text
cohesix> attach queen <queen_ticket>
```
You can mint a queen ticket from the host with:
```bash
./bin/cohsh --mint-ticket --role queen
```

## cohsh user manual
`cohsh` is the primary operator CLI. It connects to the TCP console, attaches to a role, and issues Secure9P-style commands.

### cohsh in a nutshell
`cohsh` is a thin, deterministic client for the NineDoor Secure9P control plane:
- Every command maps to a bounded file operation (read/write/tail) in the `/` namespace.
- The root-task emits `OK <VERB>` / `ERR <VERB>` acknowledgements; `cohsh` shows those verbatim.
- No extra RPC or hidden APIs exist — all control flows through files, tickets, and the manifest-defined policy.

### Quota checks (why you see `ELIMIT`)
`cohsh` enforces ticket-scoped quotas in the root-task. Each attached session carries a ticket with:
- **Scope** (which paths/verbs are permitted),
- **Rate/bandwidth** limits (bytes/second, total bytes),
- **Cursor bounds** for telemetry tails.

If a command exceeds these limits, the console returns `ERR ... reason=ELIMIT` (quota) or `ERR ... reason=EPERM` (scope). Fixes are:
- attach with a **queen** ticket (higher limits),
- reduce the tail rate/size,
- reattach to reset counters after a long session.

### Tips & gotchas
- Only one client at a time: `cohsh` and `swarmui` should not be attached simultaneously.
- Worker IDs are dynamic: always `ls /worker` before `tail`/`kill`.
- GPU spawns require `/gpu` entries: if `/gpu` is empty, run `./bin/gpu-bridge-host --mock --list` and retry.
- `ELIMIT` errors on `tail` indicate ticket quota limits; reattach with a queen ticket or slow the tail.

### Start and attach
- Connect as queen (most common):
  ```bash
  ./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen
  ```
- Attach after startup:
  ```text
  coh> attach queen
  ```
- Use tickets when required:
  ```bash
  QUEEN_TICKET=$(./bin/cohsh --mint-ticket --role queen)
  ./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen --ticket "$QUEEN_TICKET"
  ```

### Navigate and inspect
- List namespaces:
  ```text
  coh> ls /
  coh> ls /worker
  ```
- Read a file once:
  ```text
  coh> cat /log/queen.log
  ```
- Stream a file:
  ```text
  coh> tail /log/queen.log
  coh> tail /worker/<id>/telemetry
  ```

### Common control actions
- Spawn heartbeat workers:
  ```text
  coh> spawn heartbeat ticks=100
  coh> spawn heartbeat ticks=50 ttl_s=60 ops=500
  ```
- Spawn GPU workers (requires GPU bridge):
  ```text
  coh> spawn gpu gpu_id=GPU-0 mem_mb=4096 streams=1 ttl_s=120
  ```
- Kill a worker:
  ```text
  coh> kill worker-<id>
  ```

### Self-tests and diagnostics
- Quick vs full tests:
  ```text
  coh> test --mode quick
  coh> test --mode full --timeout 120
  ```
- TCP health check:
  ```text
  coh> tcp-diag
  coh> tcp-diag 31337
  ```
