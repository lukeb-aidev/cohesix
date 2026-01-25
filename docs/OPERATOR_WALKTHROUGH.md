<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Provide an operator walkthrough for lifecycle control and recovery. -->
<!-- Author: Lukas Bower -->
# Cohesix Operator Walkthrough

This walkthrough follows the as-built lifecycle control surfaces exposed by NineDoor and `cohsh`.

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

