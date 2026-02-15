<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Provide an operator walkthrough for lifecycle control and recovery. -->
<!-- Author: Lukas Bower -->
# Cohesix Operator Walkthrough

This walkthrough follows the as-built lifecycle control surfaces exposed by NineDoor and `cohsh`
for v0.6.0-alpha. It includes hive-gateway (REST multiplexer), live GPU publish, PEFT flows,
host-sidecar telemetry, and Live Hive text overlays.
For host tool usage, interdependencies, and policy/mount details, see
[HOST_TOOLS.md](HOST_TOOLS.md).

## Assumptions and conventions
- `coh>` indicates the `cohsh` prompt.
- Only one console client at a time. When using `hive-gateway`, keep it attached and route
  all other tools through REST.
- Live examples assume QEMU is running and the TCP console is reachable at `127.0.0.1:31337`.
- If policy gating is enabled (see `/policy/rules`), writes to `/queen/ctl` require approvals queued in `/actions/queue`.
- `/gpu/*` appears only after `gpu-bridge-host --publish` runs; `/host/*` appears only after `host-sidecar-bridge` runs.
- Mock mode commands (`--mock`) do not talk to the VM; do not mix mock and live in the same session.

## Hive-gateway mental model (v0.6.0-alpha)
- `hive-gateway` is the **sole** console client; everything else must use REST (`--rest-url`).
- REST is a 1:1 projection of `LS`, `CAT`, and `ECHO`. It does not add new verbs or semantics.
- REST clients inherit the gateway role (typically `queen`); there is no per-request ticket.
- Keep the gateway bound to loopback and use an SSH tunnel for remote operators.

## Where `--watch` data lands
- `host-sidecar-bridge --watch` continuously refreshes `/host/*` (for example `/host/systemd/status`, `/host/nvidia/gpu/0/status`).
- View `/host/*` with `cohsh` (`ls /host`, `cat /host/systemd/status`), REST (`/v1/fs/ls`, `/v1/fs/cat`), or a `coh mount`.

## Quickstart: gateway multiplexing (single host)
Goal: run `hive-gateway` as the only console client and use REST for all tools.

1. Boot the Queen VM.
   ```bash
   ./qemu/run.sh
   ```
2. Start the gateway (queen role).
   ```bash
   COH_TCP_HOST=127.0.0.1 COH_TCP_PORT=31337 COH_AUTH_TOKEN=changeme \
     COH_ROLE=queen HIVE_GATEWAY_BIND=127.0.0.1:8080 \
     ./bin/hive-gateway
   ```
3. Verify the gateway is healthy.
   ```bash
   curl -sS http://127.0.0.1:8080/v1/meta/bounds | jq .
   curl -sS 'http://127.0.0.1:8080/v1/fs/ls?path=/' | jq .
   ```
4. Attach `cohsh` via REST (not TCP).
   ```bash
   ./bin/cohsh --transport rest --rest-url http://127.0.0.1:8080 --role queen
   ```
5. Publish host snapshots through the gateway.
   ```bash
   ./bin/gpu-bridge-host --publish --rest-url http://127.0.0.1:8080 --interval-ms 1000
   ./bin/host-sidecar-bridge --rest-url http://127.0.0.1:8080 --watch
   ```
6. Launch SwarmUI over REST.
   ```bash
   SWARMUI_TRANSPORT=rest SWARMUI_REST_URL=http://127.0.0.1:8080 ./bin/swarmui
   ```

## Real-world multiplexer scenarios (hive-gateway)
These scenarios use `hive-gateway` as the **sole** console client and route all host tools through REST. This keeps the console single-client while enabling multi-tool usage.

### A) Queen on a GPU host + SwarmUI on a remote Mac
1. On the GPU host, boot the queen (`./qemu/run.sh` in the release bundle).
2. Start the gateway (queen role):
   ```bash
   COH_TCP_HOST=127.0.0.1 COH_TCP_PORT=31337 COH_AUTH_TOKEN=changeme \
     COH_ROLE=queen HIVE_GATEWAY_BIND=127.0.0.1:8080 \
     ./bin/hive-gateway
   ```
3. Publish host telemetry through REST:
   ```bash
   ./bin/gpu-bridge-host --publish --rest-url http://127.0.0.1:8080 --interval-ms 1000
   ./bin/host-sidecar-bridge --rest-url http://127.0.0.1:8080 --watch --provider systemd --provider nvidia
   ```
4. From the Mac, tunnel the gateway:
   ```bash
   ssh -L 8080:127.0.0.1:8080 <gpu-host>
   ```
5. Start SwarmUI via REST:
   ```bash
   SWARMUI_TRANSPORT=rest SWARMUI_REST_URL=http://127.0.0.1:8080 ./bin/swarmui
   ```
6. Confirm Live Hive view updates and console commands work (no other console clients attached).

### B) Two host publishers (g5g + Jetson) into one queen
1. Start `hive-gateway` on the queen host (Scenario A, step 2).
2. On the Jetson, forward the gateway port:
   ```bash
   ssh -L 8080:127.0.0.1:8080 <queen-host>
   ```
3. Run Jetson publishers against the tunnel:
   ```bash
   ./bin/gpu-bridge-host --publish --rest-url http://127.0.0.1:8080 --interval-ms 1000
   ./bin/host-sidecar-bridge --rest-url http://127.0.0.1:8080 --watch --provider systemd --provider nvidia
   ```
4. On the queen host, also publish local telemetry with the same `--rest-url`.
5. `/gpu/*` and `/host/*` are single namespaces. If two hosts publish simultaneously, the most recent write wins. For deterministic demos, stagger publishes (alternate every N seconds) or keep one publisher active at a time.

### C) CAS updates + REST mount via the gateway
1. Pack and upload a CAS bundle over REST:
   ```bash
   ./bin/cas-tool pack --epoch 1 --input ./out/cas/payload --out-dir ./out/cas/1 --signing-key ./resources/fixtures/cas_signing_key.hex
   ./bin/cas-tool upload --bundle ./out/cas/1 --rest-url http://127.0.0.1:8080
   ```
2. Use a live REST-backed mount:
   ```bash
   ./bin/coh mount --rest-url http://127.0.0.1:8080 --rest-auth-token "$HIVE_GATEWAY_REQUEST_AUTH_TOKEN" \
     --at /tmp/coh-mount-rest
   ```
3. REST mount is **exclusive** per gateway URL; stop the mount before starting another.

### D) Headless ops (REST + cohsh script)
Use REST to automate an operator script without taking the console.
1. Start `hive-gateway` (Scenario A, step 2).
2. Run a `.coh` script over REST:
   ```bash
   ./bin/cohsh --transport rest --rest-url http://127.0.0.1:8080 --role queen \
     --script scripts/cohsh/boot_v0.coh
   ```
   In a release bundle, replace the script path with your own `.coh` file.

## FUSE mounts (`coh mount`) over TCP vs REST
`coh mount` exposes a host filesystem view over Secure9P namespaces. It is a projection of `LS`/`CAT`/`ECHO` with manifest-derived bounds and policy allowlists enforced.

Prerequisites:
- Linux: install FUSE3 (`sudo apt-get update && sudo apt-get install -y fuse3`). Confirm `fusermount3` exists.
- macOS: FUSE mounts require MacFUSE installed and approved (verify `/dev/macfuse0` exists, or `/dev/osxfuse0` on older OSXFUSE). Cohesix bundles ship `coh` with FUSE enabled, but mounts fail until the MacFUSE runtime is active.

Transport selection:
- **TCP mount**: attaches directly to the console. Only one console client is supported; do not run `hive-gateway` concurrently.
- **REST mount**: connects through `hive-gateway` and supports multi-tool/multi-host usage while keeping the console single-client. REST writes require request-auth.

### TCP mount (single console client only)
```bash
./bin/coh mount --host 127.0.0.1 --port 31337 --auth-token "${COH_AUTH_TOKEN}" --at /tmp/coh-mount-tcp
```
If you need more than one host/tool concurrently, stop the TCP mount and use the gateway + REST mount instead.
Remote TCP mounts over high-latency links (for example AWS → Mac over an SSH reverse tunnel) are supported, but remain single-client; prefer `hive-gateway` + REST for remote multi-host operation.

### REST mount (gateway multiplexing; recommended)
Start the gateway first (Scenario A), then mount through REST:
```bash
./bin/coh mount --rest-url http://127.0.0.1:8080 --rest-auth-token "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN}" \
  --at /tmp/coh-mount-rest
```
Validate reads:
```bash
cat /tmp/coh-mount-rest/proc/lifecycle/state
head -n 5 /tmp/coh-mount-rest/log/queen.log
```

### Bidirectional telemetry transfer smoke (supported MIME types)
This is the safe, OS-owned “file transfer” surface: create telemetry segments via `/queen/telemetry/<device>/ctl` with a MIME type, then append records to the OS-named segment.

On **Host A** (for example Jetson):
```bash
MNT=/tmp/coh-mount-rest
DEV=jetson-xfer-1
printf '{"new":"segment","mime":"text/plain"}\n' >> "${MNT}/queen/telemetry/${DEV}/ctl"
printf "hello-from-jetson ts_ms=%s\n" "$(date +%s000)" >> "${MNT}/queen/telemetry/${DEV}/seg/seg-000001"
```

On **Host B** (for example g5g), confirm visibility:
```bash
MNT=/tmp/coh-mount-rest
DEV=jetson-xfer-1
cat "${MNT}/queen/telemetry/${DEV}/latest"
tail -n 5 "${MNT}/queen/telemetry/${DEV}/seg/seg-000001"
```
Repeat the same flow in the opposite direction with a different `DEV` value (for example `g5g-xfer-1`) and verify Host A can read it.

## 0) Preflight: verify console or gateway access (optional but recommended)
Choose one path depending on your transport:

TCP console:
Attach and verify the root namespace is reachable:
```bash
./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen
coh> ping
coh> ls /
```
If policy gating is enabled, confirm rules and current pressure:
```bash
coh> cat /policy/rules
coh> cat /proc/pressure/policy
```

REST gateway:
```bash
curl -sS http://127.0.0.1:8080/v1/meta/bounds | jq .
curl -sS 'http://127.0.0.1:8080/v1/fs/ls?path=/' | jq .
```

## 1) Attach a queen session
```bash
coh> attach queen
```
Expected: `OK ATTACH`.

## 2) Inspect lifecycle state
```bash
coh> cat /proc/lifecycle/state
coh> cat /proc/lifecycle/reason
coh> cat /proc/lifecycle/since
```
Example output:
```
state=ONLINE
reason=boot-complete
since_ms=0
```

## 3) Begin maintenance (cordon)
```bash
coh> lifecycle cordon
coh> cat /proc/lifecycle/state
```
Expected:
```
state=DRAINING
```
A matching audit line appears in `/log/queen.log`:
```
lifecycle transition old=ONLINE new=DRAINING reason=cordon
```

## 4) Drain to quiesced
Ensure there are no outstanding leases or active workers, then drain:
```bash
coh> lifecycle drain
coh> cat /proc/lifecycle/state
```
Expected:
```
state=QUIESCED
```
If leases remain, the command returns `ERR` and `/log/queen.log` reports:
```
lifecycle denied action=drain state=DRAINING reason=outstanding-leases leases=<n>
```

## 5) Resume service
```bash
coh> lifecycle resume
coh> cat /proc/lifecycle/state
```
Expected:
```
state=ONLINE
```

## 6) Reset (explicit reboot intent)
Use `reset` to move back to `BOOTING`, then `resume` after maintenance:
```bash
coh> lifecycle reset
coh> cat /proc/lifecycle/state
coh> lifecycle resume
```
Expected:
```
state=BOOTING
state=ONLINE
```

## 7) Telemetry during drain
Telemetry ingest remains enabled in `DRAINING`.
```bash
coh> echo '{"new":"segment","mime":"text/plain"}' > /queen/telemetry/dev-1/ctl
coh> echo maintenance-event > /queen/telemetry/dev-1/seg/seg-000001
```
Writes should return `OK` and `/queen/telemetry/dev-1/latest` updates deterministically.

---

## 8) Live GPU registry publish (required for non-mock PEFT)
The VM does not expose `/gpu/models` until the host GPU bridge publishes it.
Run the publish on the host (same machine that can reach the Queen TCP console):
```bash
./bin/gpu-bridge-host --publish --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme \
  --interval-ms 1000 --registry /home/models/peft_registry
```
Validate in `cohsh` (quit SwarmUI first if it is running):
```bash
coh> ls /gpu/models
coh> cat /gpu/telemetry/schema.json
```
Expected: `OK LS` on `/gpu/models` and a readable schema file.

## 9) PEFT live flow (import -> activate -> rollback)
This is the non-mock flow that requires `/gpu/models` to be published.
```bash
./bin/coh --host 127.0.0.1 --port 31337 peft import --publish \
  --model lejepa-edge-v1 \
  --from /home/models/lejepa/adapter \
  --job job_0001 \
  --export /home/models/lejepa/export \
  --registry /home/models/peft_registry
./bin/coh --host 127.0.0.1 --port 31337 peft activate \
  --model lejepa-edge-v1 --registry /home/models/peft_registry
```
Confirm pointer and availability (in `cohsh`):
```bash
coh> ls /gpu/models/available
coh> cat /gpu/models/active
```
Rollback if needed:
```bash
./bin/coh --host 127.0.0.1 --port 31337 peft rollback --registry /home/models/peft_registry
```

## 10) Live host telemetry providers (`/host/*`)
Publish host-side providers into the VM (systemd, k8s, docker, nvidia).
```bash
./bin/host-sidecar-bridge --tcp-host 127.0.0.1 --tcp-port 31337 --auth-token changeme --watch \
  --provider systemd --provider k8s --provider docker --provider nvidia
```
Validate in `cohsh`:
```bash
coh> ls /host
coh> cat /host/systemd/status
coh> cat /host/nvidia/gpu/0/status
```
Expected: bounded `status` lines; `state=unknown` when a provider is unavailable.

## 11) Live Hive telemetry text overlays (SwarmUI)
SwarmUI is read-only and must not run concurrently with `cohsh`.
1. Quit `cohsh`, launch SwarmUI:
   ```bash
   ./bin/swarmui
   ```
2. Click **Connect** -> **Hive Start**.
3. If you see "No telemetry yet", quit SwarmUI and seed a line:
   - If `/worker` is empty, approve and spawn a heartbeat first, then re-run `ls /worker`:
     - `echo {"id":"spawn-1","target":"/queen/ctl","decision":"approve"} > /actions/queue`
     - `spawn heartbeat ticks=100`
   ```bash
   ./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen <<'COH'
   attach queen
   ls /worker
   # Replace worker-1 with the actual worker id from the ls output.
   echo heartbeat-demo > /worker/worker-1/telemetry
   cat /worker/worker-1/telemetry
   COH
   ```
4. Relaunch SwarmUI and select a worker dot to view the bounded overlay + detail panel.

---

## Troubleshooting quick hits
- `ERR ECHO reason=policy ... EPERM`: queue an approval in `/actions/queue`, then retry the control write.
- `ERR AUTH` or `connection refused`: verify QEMU is running and the console port matches `127.0.0.1:31337`.
- `cohsh` hangs or `coh` cannot connect: another console client is already attached.
- REST calls fail: confirm `hive-gateway` is running, bound to the expected address, and is the only console client.
- `/gpu` empty: run `./bin/gpu-bridge-host --publish ...` (live) or `--mock --list` (mock).
- `/host` empty: run `./bin/host-sidecar-bridge --watch --provider ...`.
