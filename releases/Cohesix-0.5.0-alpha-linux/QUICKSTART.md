<!-- Copyright 2026 Lukas Bower -->
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
- `bin/` - host tools (`cohsh`, `coh`, `hive-gateway`, `swarmui`, `cas-tool`, `gpu-bridge-host`, `host-sidecar-bridge`).
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
- `hive-gateway` - REST gateway that projects console/file semantics over HTTP.
- `swarmui` - UI for replay or live observation with an embedded console panel (core verbs only).
- `cas-tool` - package and upload bundles to the `/updates` namespace (optional).
- `gpu-bridge-host` - host GPU discovery + live `/gpu/models` publish for the `/gpu` namespace (optional).
- `host-sidecar-bridge` - publish mock or live host providers into `/host` for policy/CI validation and telemetry snapshots (optional).
See `docs/HOST_TOOLS.md` for details.

## 0.4.0-alpha highlights (milestones 21a-24c)
- Telemetry ingest with OS-named segments: `cohsh telemetry push` + `coh telemetry pull`.
- Host bridge `coh` for Secure9P mount, GPU lease/status, and telemetry export (no new VM semantics).
- SwarmUI Live Hive visibility + embedded console panel that reuses the existing TCP session.
- Live Hive UX polish: worker labels, role color-coding, click-to-select details panel, and bounded overlays.
- Lifecycle controls (`cohsh lifecycle`) plus `/proc/lifecycle/*` and `/proc/root/*` cut signals.
- `coh run` command that records bounded GPU breadcrumb entries under `/gpu/<id>/status`.
- `coh peft` export/import/activate/rollback flows (LoRA lifecycle glue).
- Cohesix Python client + examples and `coh doctor` for deterministic host checks.
- Authoritative scheduling/lease/export/policy control files with `/proc` observability.
- Host REST gateway (`hive-gateway`) projecting console/file semantics over HTTP.

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
python3 -m pip install ./python/cohesix-py
python3 python/cohesix-py/examples/lease_run.py --mock
python3 python/cohesix-py/examples/peft_roundtrip.py --mock
python3 python/cohesix-py/examples/telemetry_write_pull.py --mock
```
Note: in the source tree, the Python client lives under `tools/cohesix-py` instead of `python/cohesix-py`.
If you need an editable install, upgrade pip (`python3 -m pip install --upgrade pip`) and then use
`python3 -m pip install -e python/cohesix-py`.
The Python client requires Python 3.11+. If your system `python3` is older, use a 3.11 venv:
```bash
python3.11 -m venv .venv
. .venv/bin/activate
python -m pip install --upgrade pip
python -m pip install ./python/cohesix-py
```
On Ubuntu, you may need `sudo apt-get install python3.11 python3.11-venv` first.

## Run coh doctor (host checks)
This runs the real host checks (no mock backend). It does not require QEMU, but it probes host runtimes and GPU discovery.
```bash
./bin/coh doctor
```
Expected results:
- NVIDIA host with full NVML: `OK DOCTOR check=nvml ... backend=nvml` and overall success.
- Jetson (feature-limited NVML): `OK DOCTOR check=nvml status=degraded backend=cuda` and overall success. If you see `ERR DOCTOR check=nvml ...`, your build lacks the CUDA fallback.

## Run the REST gateway (hive-gateway)
The gateway projects the existing console/file semantics over HTTP. It does **not** add new control-plane behavior.

### Mock mode (no QEMU)
```bash
./bin/hive-gateway --mock --bind 127.0.0.1:8080
curl -sS http://127.0.0.1:8080/v1/meta/bounds | jq .
```
Note: `jq` is optional (pretty output only). If it is missing, use `python3 -m json.tool` instead.

### Live QEMU
1) Boot the VM:
```bash
./qemu/run.sh
```
Default SMP topology is four single-threaded cores (`-smp 4,cores=4,threads=1,sockets=1`). Override with
`COHESIX_QEMU_SMP=<count>` or supply a full topology string via `COHESIX_QEMU_SMP_TOPO`.
2) Start the gateway (Terminal 2):
```bash
COH_TCP_HOST=127.0.0.1 COH_TCP_PORT=31337 COH_AUTH_TOKEN=changeme \\
  COH_ROLE=queen HIVE_GATEWAY_BIND=127.0.0.1:8080 \\
  ./bin/hive-gateway
```
3) Validate the API:
```bash
curl -sS http://127.0.0.1:8080/v1/meta/bounds | jq .
```

## Wow: REST control + observability in 90 seconds (Milestone 24c)
These examples show the new scheduling/lease/export/policy control grammar and `/proc` observability surfaced through the REST gateway.
Keep QEMU + hive-gateway running from the previous section.

1) Enqueue a GPU worker schedule entry and read the queue:
```bash
curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo \
  -H 'Content-Type: application/json' \
  -d '{"path":"/queen/schedule/ctl","line":"{\"id\":\"sched-1\",\"role\":\"worker-gpu\",\"priority\":2,\"ticks\":3,\"budget_ms\":120}"}'
curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/schedule/summary&max_bytes=128' | jq .
curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/schedule/queue&max_bytes=256' | jq .
```

2) Grant + preempt a lease and read lease state:
```bash
curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo \
  -H 'Content-Type: application/json' \
  -d '{"path":"/queen/lease/ctl","line":"{\"op\":\"grant\",\"id\":\"lease-1\",\"subject\":\"queen\",\"resource\":\"gpu0\",\"ttl_s\":300,\"priority\":5}"}'
curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo \
  -H 'Content-Type: application/json' \
  -d '{"path":"/queen/lease/ctl","line":"{\"op\":\"preempt\",\"id\":\"lease-1\",\"reason\":\"timeout\"}"}'
curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/lease/summary&max_bytes=160' | jq .
curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/lease/active&max_bytes=256' | jq .
curl -sS 'http://127.0.0.1:8080/v1/fs/cat?path=/proc/lease/preemptions&max_bytes=256' | jq .
```

3) Open/close an export window and apply/rollback a policy revision:
```bash
curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo \
  -H 'Content-Type: application/json' \
  -d '{"path":"/queen/export/ctl","line":"{\"op\":\"open\",\"id\":\"export-1\",\"ttl_s\":900}"}'
curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo \
  -H 'Content-Type: application/json' \
  -d '{"path":"/queen/export/ctl","line":"{\"op\":\"close\",\"id\":\"export-1\",\"reason\":\"window-complete\"}"}'
curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo \
  -H 'Content-Type: application/json' \
  -d '{"path":"/policy/ctl","line":"{\"op\":\"apply\",\"id\":\"rev-2026-02-03\",\"sha256\":\"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"}"}'
curl -sS -X POST http://127.0.0.1:8080/v1/fs/echo \
  -H 'Content-Type: application/json' \
  -d '{"path":"/policy/ctl","line":"{\"op\":\"rollback\",\"id\":\"rev-2026-02-03\"}"}'
```

4) Optional UI check (read-only): launch SwarmUI and confirm the new **Schedule Queue** and **Lease/Preemption Timeline** panels populate from `/proc`.

## Run the Live Hive demo
You need two terminals:
- Terminal 1: QEMU (keeps the VM running).
  - Note: Qemu will show a serial terminal, used for core seL4 diagnostics. This is NOT intended to be the main user interface.
- Terminal 2: for either `cohsh` or `swarmui`. Use one at a time; they should not be used simultaneously.
- Console lock note:
  - SwarmUI includes a console panel for core verbs; use `cohsh` for CLI-only commands.
  - Only one console client at a time: quit SwarmUI before attaching `cohsh`, and vice versa.

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
3. In cohsh, run a few actions (core):
- `help` — show the command list.
- `ls /` — list root namespace entries.
- `cat /log/queen.log` — read the queen log once.
- `echo {"id":"spawn-1","target":"/queen/ctl","decision":"approve"} > /actions/queue` — approve one `/queen/ctl` action when policy gating is enabled.
- `spawn heartbeat ticks=100` — request a heartbeat worker.
- `ls /worker` — list current worker IDs (do not assume `worker-1`; use what you see).
- `echo {"id":"kill-1","target":"/queen/ctl","decision":"approve"} > /actions/queue` — approve the kill if policy gating is enabled.
- `kill worker-2` — terminate the worker id you just listed (replace with the actual id).
- `quit` — exit cohsh.

Optional extras:
- `log` or `tail /log/queen.log` — stream the queen log.
- `ping` — report attachment status.
- `test --mode quick` — run quick self-tests.
- `pool bench path=/log/queen.log ops=200 batch=4 payload_bytes=64` — run a short pool benchmark.
- `tcp-diag` — debug TCP connectivity without protocol traffic.
- `bind /queen /host/queen` — bind a path.
- `mount logs /logs` — mount the log service namespace (alias to `/log`).
- `cat /proc/lifecycle/state` — read the current lifecycle state.
- `cat /proc/root/reachable` — confirm root reachability and cut signals.
- `lifecycle cordon` — stop accepting new work.
- `lifecycle resume` — return to ONLINE.

Spawn notes:
- Supported roles are `heartbeat` (aliases: `worker`, `worker-heartbeat`) and `gpu` (alias: `worker-gpu`).
- Heartbeat spawns require `ticks=<n>` and accept optional `ttl_s=<n>` and `ops=<n>` budget controls.
  - ttl_s=<n> — time‑to‑live in seconds (budget)
  - ops=<n> — operation budget (budget)
- If policy gating is enabled (see `/policy/rules`), each `/queen/ctl` action needs a queued approval:
  - `echo {"id":"approve-1","target":"/queen/ctl","decision":"approve"} > /actions/queue`
- GPU spawns require a lease spec: `gpu_id`, `mem_mb`, `streams`, `ttl_s`. Optional: `priority`, `budget_ttl_s`, `budget_ops`.
- If `/gpu` is empty, run the host GPU bridge (`./bin/gpu-bridge-host --mock --list`) and try again.
- For non-mock PEFT flows, use a live publish (`./bin/gpu-bridge-host --publish --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme`) so `/gpu/models` is visible.

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
   In SwarmUI, click **Connect** → **Hive Start**. Worker dots show numeric labels and role colors; click a dot to populate the detail panel.
5. If Live Hive shows "No telemetry yet", quit SwarmUI and seed a line into a worker ring (writes `/worker/<id>/telemetry`):
   ```bash
   ./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen <<'COH'
   attach queen
   ls /worker
   echo heartbeat-demo > /worker/worker-1/telemetry
   tail /worker/worker-1/telemetry
   COH
   ```
   Relaunch SwarmUI to observe overlays and details.
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
./bin/coh gpu --host 127.0.0.1 --port 31337 list
./bin/coh gpu --host 127.0.0.1 --port 31337 lease --gpu GPU-0 --mem-mb 4096 --streams 1 --ttl-s 60
./bin/coh run --host 127.0.0.1 --port 31337 --gpu GPU-0 -- echo ok
./bin/coh telemetry --host 127.0.0.1 --port 31337 pull --out ./out/telemetry
./bin/coh mount --mock --at /tmp/coh-mount
```
Policy gates: if policy gating is enabled (see `/policy/rules`), any `coh` action that writes `/queen/ctl`
(`coh gpu lease`, `coh run`, `coh peft ...`) requires an approval queued in `/actions/queue`. Otherwise you'll
see `ERR ECHO reason=policy ... EPERM`. Queue an approval with `cohsh`, then re-run the `coh` command:
```bash
./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen <<'COH'
echo {"id":"approve-1","target":"/queen/ctl","decision":"approve"} > /actions/queue
COH
```
Note: the TCP console is single-client; exit `cohsh` before running `coh`.

Mount behavior: `coh mount` starts a long-running FUSE process and stays in the foreground. Use a second
terminal to access the mount, or run it in the background:
```bash
./bin/coh mount --host 127.0.0.1 --port 31337 --at /tmp/coh-mount > /tmp/coh-mount.log 2>&1 &
```
Unmount with `fusermount -u /tmp/coh-mount` (Linux) or `umount /tmp/coh-mount` (macOS).

Note: live FUSE mounts require a running QEMU instance, a FUSE runtime, and `coh` built with FUSE
enabled (default on Linux). On macOS, FUSE is disabled by default; rebuild `coh` with `--features fuse`
and install MacFUSE.
`./bin/coh mount --host 127.0.0.1 --port 31337 --at /tmp/coh-mount`

GPU visibility: `coh gpu list`/`lease` only see GPUs after the host bridge publishes `/gpu` (live:
`./bin/gpu-bridge-host --publish ...`; mock: `./bin/gpu-bridge-host --mock --list`).
Host visibility: anything reading `/host/*` requires `host-sidecar-bridge` to be running and publishing providers.
Mock vs live: `--mock` uses an in-process backend and ignores the VM; live commands require QEMU + the TCP console.
Mixing mock and live in the same session commonly leads to empty views or unexpected failures.
`coh run` requires an active lease in `/gpu/<id>/lease` and will refuse to execute without one.

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
./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen <<'COH'
attach queen
telemetry push out/telemetry/demo.txt --device device-1
quit
COH
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

### gpu-bridge-host (mock list + live publish)
```bash
./bin/gpu-bridge-host --mock --list
./bin/gpu-bridge-host --publish --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme
```
NVML + CUDA discovery are enabled by default on Linux bundles; use `--no-default-features` to omit NVML/CUDA.
Note: `/gpu/models` and `/gpu/telemetry/schema.json` appear only after a live publish.

### host-sidecar-bridge
```bash
./bin/host-sidecar-bridge --mock --mount /host --provider systemd --provider k8s --provider nvidia
```
Publish live NVidia, systemd, k8s, and/or Docker telemetry:
```bash
./bin/host-sidecar-bridge --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme
./bin/host-sidecar-bridge --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --watch \
  --provider systemd --provider k8s --provider docker --provider nvidia
```

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
