<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Provide an operator walkthrough for lifecycle control and recovery. -->
<!-- Author: Lukas Bower -->
# Cohesix Operator Walkthrough

This walkthrough follows the as-built lifecycle control surfaces exposed by NineDoor and `cohsh`,
and includes Milestone 24b live GPU publish, PEFT flows, host-sidecar telemetry, and Live Hive text overlays.

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
   ```bash
   ./bin/cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 --role queen <<'COH'
   attach queen
   ls /worker
   echo heartbeat-demo > /worker/worker-1/telemetry
   tail /worker/worker-1/telemetry
   COH
   ```
4. Relaunch SwarmUI and select a worker dot to view the bounded overlay + detail panel.
