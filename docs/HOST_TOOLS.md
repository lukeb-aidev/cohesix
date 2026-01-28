<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Describe Cohesix host tools, their purpose, and usage. -->
<!-- Author: Lukas Bower -->
# Host Tools

Host tools run outside the VM and speak either the TCP console protocol or Secure9P (NineDoor), depending on the tool.
They are built by `scripts/cohesix-build-run.sh` and copied into `out/cohesix/host-tools/`.

## cohsh
### Purpose
Canonical operator shell for Cohesix. Attaches to the TCP console (or an in-process NineDoor Secure9P server for mock tests) and drives `/queen/ctl`, logs, and telemetry.

### Location
- Source: `apps/cohsh`
- Binary: `out/cohesix/host-tools/cohsh`

### Usage
```bash
./cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337
./cohsh --transport tcp --role worker-heartbeat --ticket "$WORKER_TICKET"
./cohsh --transport tcp --script scripts/cohsh/boot_v0.coh
./cohsh --mint-ticket --role worker-heartbeat --ticket-subject worker-1
```

### Notes
- Worker roles require tickets; queen tickets are optional.
- `--auth-token` (or `COHSH_AUTH_TOKEN`) is the TCP console auth token, separate from tickets.
- `--mint-ticket` uses `configs/root_task.toml` by default; override with `--ticket-config`/`COHSH_TICKET_CONFIG` or `--ticket-secret`/`COHSH_TICKET_SECRET`.
- Full grammar and command behavior live in `docs/USERLAND_AND_CLI.md`.

## coh
### Purpose
Host bridge for mount, GPU leases, telemetry pulls, runtime breadcrumbs, PEFT lifecycle glue, and environment checks (`coh doctor`).

### Location
- Source: `apps/coh`
- Binary: `out/cohesix/host-tools/coh`

### Usage
```bash
./coh doctor --mock
./coh gpu list --host 127.0.0.1 --port 31337
./coh gpu lease --host 127.0.0.1 --port 31337 --gpu GPU-0 --mem-mb 4096 --streams 1 --ttl-s 60
./coh run --host 127.0.0.1 --port 31337 --gpu GPU-0 -- echo ok
./coh telemetry pull --host 127.0.0.1 --port 31337 --out ./out/telemetry
./coh peft export --host 127.0.0.1 --port 31337 --job job_8932 --out ./out/export
```

### Notes
- `coh doctor` validates tickets, mount capability, NVML (unless `--mock`), and runtime prerequisites.
- Policy enforcement is manifest-driven; `COH_POLICY` (or `out/coh_policy.toml`) must hash-match compiled defaults.

## swarmui
### Purpose
Desktop UI (Tauri) that renders the hive view and reuses cohsh-core semantics. It does not add new verbs or protocols.

### Location
- Source: `apps/swarmui`
- Binary: `out/cohesix/host-tools/swarmui` (packaged when `cohesix-dev` is enabled)

### Usage
```bash
./swarmui
SWARMUI_TRANSPORT=9p SWARMUI_9P_HOST=127.0.0.1 SWARMUI_9P_PORT=31337 ./swarmui
./swarmui --replay /path/to/demo.hive.cbor
./swarmui --replay-trace /path/to/trace_v0.trace
./swarmui --mint-ticket --role worker-heartbeat --ticket-subject worker-1
```

### Notes
- Defaults to the TCP console transport; set `SWARMUI_TRANSPORT=9p` or `SWARMUI_TRANSPORT=secure9p` for Secure9P.
- `SWARMUI_9P_HOST`/`SWARMUI_9P_PORT` supply the TCP endpoint for both console and Secure9P transports.
- `SWARMUI_AUTH_TOKEN` (or `COHSH_AUTH_TOKEN`) supplies the console auth token.
- SwarmUI allows CSP `script-src 'unsafe-eval'` to support PixiJS Live Hive rendering.
- `--mint-ticket` uses `SWARMUI_TICKET_CONFIG`/`SWARMUI_TICKET_SECRET` (fallback to `COHSH_*`); the UI also offers a "Mint ticket" button.
- `--replay` loads a snapshot from `$DATA_DIR/snapshots/` (relative paths) and forces offline mode.
- `--replay-trace` loads a trace from `$DATA_DIR/traces/` (relative paths) and auto-loads a sibling `*.hive.cbor` if present.

## cas-tool
### Purpose
Package and upload CAS bundles over the TCP console using the same append-only flows as `cohsh`.

### Location
- Source: `apps/cas-tool`
- Binary: `out/cohesix/host-tools/cas-tool`

### Usage
```bash
./cas-tool pack --epoch 1 --input path/to/payload --out-dir out/cas/1
./cas-tool upload --bundle out/cas/1 --host 127.0.0.1 --port 31337 \
  --auth-token changeme --ticket "$QUEEN_TICKET"
```

### Notes
- Epoch labels must be ASCII digits only (max 20 chars) to satisfy `/updates/<epoch>/` validation.
- Upload attaches as the queen role; pass a queen ticket if your deployment requires one.
- If signing is required in `configs/root_task.toml`, pass `--signing-key` when packing (Ed25519 key in hex).
- Payloads are chunked and sent as bounded `echo` writes (`b64:` segments) to `/updates/<epoch>/`.

## gpu-bridge-host
### Purpose
Discover GPUs on the host (NVML or mock) and emit the `/gpu` namespace snapshot consumed by NineDoor.

### Location
- Source: `apps/gpu-bridge-host`
- Binary: `out/cohesix/host-tools/gpu-bridge-host`

### Usage
```bash
./gpu-bridge-host --mock --list
./gpu-bridge-host --list
```

### Notes
- `--list` prints JSON for host-side integration; it does not talk to the VM directly.
- NVML discovery is enabled by default on Linux builds; use `--no-default-features` to omit NVML.

## host-sidecar-bridge
### Purpose
Publish **mock** host-side providers into `/host` (systemd, k8s, nvidia, jetson, net) via Secure9P for policy/CI validation.

### Location
- Source: `apps/host-sidecar-bridge`
- Binary: `out/cohesix/host-tools/host-sidecar-bridge`

### Usage
```bash
./host-sidecar-bridge --mock --mount /host --provider systemd --provider k8s --provider nvidia
./host-sidecar-bridge --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme
```

### Notes
- Live TCP publishing requires building with `--features tcp` (otherwise use `--mock`).
- The `/host` namespace must be enabled in `configs/root_task.toml`.

---

## Using Host Tools Together
These workflows show how the tools complement each other without introducing new semantics. Each example uses the shipped commands only.

### 1) Live Hive operator flow (Queen + UI + CLI)
Goal: show a live Queen with SwarmUI as the trustable lens, and `cohsh` as the action surface.
Why this matters: proves the UI is observational only while the authoritative control plane remains the CLI and file-shaped paths.
```bash
./qemu/run.sh
./swarmui
```
Quit SwarmUI before switching to `cohsh`:
```bash
./cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337
```
In `cohsh`:
```
attach queen
cat /proc/lifecycle/state
spawn heartbeat ticks=100
```
Quit `cohsh`, relaunch SwarmUI to observe the worker activity.

### 2) GPU surface + lease + breadcrumbs (host tools only)
Goal: prove the GPU namespace and bounded runtime breadcrumbs.
Why this matters: shows GPU access is host-side and lease‑gated, and that runtime actions are logged in `/gpu/<id>/status`.
```bash
./qemu/run.sh
./gpu-bridge-host --list   # NVML discovery on Linux
./coh --host 127.0.0.1 --port 31337 gpu list
./coh --host 127.0.0.1 --port 31337 gpu lease --gpu GPU-0 --mem-mb 4096 --streams 1 --ttl-s 60
./coh --host 127.0.0.1 --port 31337 run --gpu GPU-0 -- echo ok
```
Note: if `/gpu` is empty, confirm the host GPU bridge integration is running and the snapshot shows devices.

### 3) Telemetry ingress + pull (operator + host bridge)
Goal: write telemetry to the Queen’s ingest surface and pull the bundles.
Why this matters: demonstrates the append‑only ingest surface and bounded export without introducing any new protocol.
```bash
./qemu/run.sh
./cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen \
  telemetry push demo/telemetry/demo.txt --device device-1
./coh --host 127.0.0.1 --port 31337 telemetry pull --out ./out/telemetry/pull
```

### 4) PEFT lifecycle loop (export → import → activate → rollback)
Goal: show auditable adapter handling with host tooling.
Why this matters: proves adapters are managed as auditable artifacts with reversible activation.
```bash
./qemu/run.sh
./gpu-bridge-host --mock --list
./coh peft export --mock --job job_0001 --out demo/peft_export
./coh --host 127.0.0.1 --port 31337 peft import --model demo-model \
  --from demo/peft_adapter --job job_0001 --export demo/peft_export --registry demo/peft_registry
./coh --host 127.0.0.1 --port 31337 peft activate --model demo-model --registry demo/peft_registry
./coh --host 127.0.0.1 --port 31337 peft rollback --registry demo/peft_registry
```

### 5) Host sidecar publishing + policy validation
Goal: project **mock** host providers into `/host` and observe via CLI/UI.
Why this matters: today this is primarily a **mock harness** to validate `/host` gating, queen‑only controls, and audit logging. It proves the control surface without requiring live systemd/k8s/NVML integrations.
```bash
./qemu/run.sh
./host-sidecar-bridge --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme \
  --provider systemd --provider k8s --provider nvidia
./cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337
```
In `cohsh`:
```
attach queen
ls /host
```
Quit `cohsh`, open SwarmUI to observe the live hive alongside host provider activity.

Note: `host-sidecar-bridge` currently publishes deterministic mock values for the selected providers. For live NVIDIA data, use `gpu-bridge-host` and `/gpu/*`.

### 6) CAS update bundle demo (pack + upload + verify)
Goal: show content-addressed update flows with deterministic upload paths.
Why this matters: proves update artifacts are signed, chunked, and uploaded through the same audited console path.
```bash
./qemu/run.sh
QUEEN_TICKET=$(./cohsh --mint-ticket --role queen)
./cas-tool pack --epoch 1 --input demo/telemetry/demo.txt --out-dir out/cas/1 \
  --signing-key resources/fixtures/cas_signing_key.hex
./cas-tool upload --bundle out/cas/1 --host 127.0.0.1 --port 31337 \
  --auth-token changeme --ticket "$QUEEN_TICKET"
```
In `cohsh` (optional):
```
attach queen
ls /updates
```
