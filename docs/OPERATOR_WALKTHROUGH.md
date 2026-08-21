<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Provide the canonical end-to-end Cohesix operator runbook. -->
<!-- Author: Lukas Bower -->
# Cohesix Operator Walkthrough

This is the canonical source-tree runbook for a live QEMU Queen, one
`hive-gateway`, concurrent REST clients, optional host publishers, and evidence
capture. It deliberately uses one topology and one ordered journey. Reference
details live in:

- [USERLAND_AND_CLI.md](USERLAND_AND_CLI.md) for console and `.coh` grammar;
- [HOST_TOOLS.md](HOST_TOOLS.md) for tool ownership and alternatives;
- [API_GUIDELINES.md](API_GUIDELINES.md) for HTTP behavior;
- [FAILURE_MODES.md](FAILURE_MODES.md) for diagnosis and recovery; and
- [OPERATOR_RECIPES.md](OPERATOR_RECIPES.md) for advanced evidence, mount,
  ticket, lifecycle, and PEFT workflows after this topology is healthy.

See the [Glossary](GLOSSARY.md) whenever a Cohesix-specific term is unfamiliar.

The same host workflow can target a Pi 4 Queen after hardware boot and network
proof, but board acceptance remains governed by
[HARDWARE_BRINGUP.md](HARDWARE_BRINGUP.md) and [DRIVERS.md](DRIVERS.md).

## Expected topology

- QEMU exposes the authenticated VM TCP console on `127.0.0.1:31337`.
- `hive-gateway` is the only process connected directly to that console.
- `cohsh`, `coh`, Python, SwarmUI, and host bridges use REST through the
  gateway.
- The gateway binds to `127.0.0.1:8080`.
- TCP console authentication and gateway request authentication use distinct,
  non-placeholder secrets.

Do not start a direct TCP client while the gateway is running.

## Prerequisites

1. Use the repository toolchain described in
   [TOOLCHAIN_MAC_ARM64.md](TOOLCHAIN_MAC_ARM64.md).
2. Provide an external seL4 build directory matching the selected profile.
3. Load the intended deployment secrets into each terminal that needs them.
   Use a secret manager or a protected local environment file outside version
   control; do not paste secrets into documentation or shell history.
4. From the repository root, confirm the required variables are present without
   printing them:

```bash
: "${COH_AUTH_TOKEN:?set the VM console auth token}"
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?set the gateway request-auth token}"
```

The examples assume the repository root is the current directory. Release
bundle users can substitute the matching executable under `bin/` and follow
that bundle's `QUICKSTART.md` for boot paths.

## 1. Build and boot the Queen

In terminal 1:

```bash
SEL4_BUILD_DIR="$PWD/out/sel4/profile-v2/qemu-smp-production" ./scripts/cohesix-build-run.sh \
  --sel4-build "$PWD/out/sel4/profile-v2/qemu-smp-production" \
  --out-dir out/cohesix \
  --profile release \
  --root-task-features release-qemu,bootstrap-trace \
  --cargo-target aarch64-unknown-none \
  --transport tcp
```

Keep this terminal open. The serial transcript is the independent recovery
surface if host networking or authentication fails.

At the `cohesix>` prompt, perform a bounded boot check:

```text
help
ping
bi
mem
netstats
```

Expected outcomes:

- `ping` returns the liveness response;
- `bi` and `mem` complete without a fatal bootstrap error;
- `netstats` identifies an enabled, running profile and the expected TCP
  endpoint, or returns a specific blocker to route through
  [FAILURE_MODES.md](FAILURE_MODES.md).

Do not infer physical-hardware readiness from this QEMU boot.

## 2. Start the gateway

In terminal 2, load the same deployment values and configure the REST URL:

```bash
: "${COH_AUTH_TOKEN:?set the VM console auth token}"
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?set the gateway request-auth token}"
export COH_TCP_HOST="127.0.0.1"
export COH_TCP_PORT="31337"
export COH_ROLE="queen"
export COH_REST_URL="http://127.0.0.1:8080"

cargo run -p hive-gateway -- --bind 127.0.0.1:8080
```

The request-auth token protects HTTP mutations. The gateway's `COH_ROLE` and
optional `COH_TICKET` determine the VM authority inherited by every REST
client.

## 3. Verify gateway and VM connectivity

In terminal 3, set the REST URL and retain request authentication for later
writes:

```bash
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?set the gateway request-auth token}"
export COH_REST_URL="http://127.0.0.1:8080"

curl --fail-with-body --silent --show-error \
  "$COH_REST_URL/v1/meta/status"

curl --fail-with-body --silent --show-error \
  "$COH_REST_URL/v1/meta/bounds"

curl --fail-with-body --silent --show-error --get \
  --data-urlencode 'path=/' \
  "$COH_REST_URL/v1/fs/ls"

curl --fail-with-body --silent --show-error --get \
  --data-urlencode 'path=/proc/boot' \
  --data-urlencode 'max_bytes=1024' \
  "$COH_REST_URL/v1/fs/cat"
```

Continue only when:

- `/v1/meta/status` reports `connected: true`;
- `/v1/meta/bounds` includes the expected gateway generated-policy
  `manifest_sha256`;
- `/proc/boot` reports the expected target `manifest.profile` and
  `manifest.sha256`;
- the root listing contains the profile's expected namespace entries.

The bounds response is compiled into the gateway and is not queried from the
VM. Compare it with the `manifest.sha256` reported by `/proc/boot` before
claiming manifest parity between gateway and VM.

An optional namespace can be absent by design. The canonical worker root is
`/shard`; do not require the legacy `/worker` alias. `coh doctor` is a local
host-prerequisite audit rather than a gateway connectivity test; use it when
the operator workflow requires its mount, GPU, or runtime checks. Its generated
inventory is documented in
[snippets/coh_doctor_checks.md](snippets/coh_doctor_checks.md).

## 4. Open the operator shell through REST

Still in terminal 3:

```bash
cargo run -p cohsh -- --transport rest --rest-url "$COH_REST_URL" \
  --role queen
```

At `coh>`:

```text
ping
ls /
cat /proc/lifecycle/state
cat /proc/root/reachable
tail /log/queen.log 32
test --mode quick --no-mutate
```

`--role queen` configures the local shell session. It does not replace the
gateway's upstream role or create a per-request Queen identity.

Expected outcomes:

- every streaming command begins with `OK` and terminates normally;
- lifecycle and reachability nodes return bounded current state;
- the non-mutating quick test completes without writing control paths.

Use `quit` to close `cohsh`; the gateway remains connected for other clients.

## 5. Publish optional host state

Only run publishers whose paths are enabled by the active manifest.

### GPU snapshot

One-shot publication discovers the compiled host GPU backend, publishes a
snapshot through the gateway, and exits:

```bash
cargo run -p gpu-bridge-host -- \
  --publish --rest-url "$COH_REST_URL"
```

Verify with REST-backed `cohsh`:

```text
ls /gpu
cat /gpu/bridge/status
```

`gpu-bridge-host --list` is local inventory only and does not populate the VM.
Use `--mock` only for deterministic development; it does not prove live GPU
discovery.

### Host provider snapshot

Publish one selected provider and exit:

```bash
cargo run -p host-sidecar-bridge -- \
  --rest-url "$COH_REST_URL" --provider net
```

Verify with `ls /host` and the provider paths exposed by the active manifest.
For continuous collection, select a scheduled provider such as `docker`,
`systemd`, `k8s`, or `nvidia`, add `--watch`, and keep the bridge in its own
terminal. `net` and `jetson` are one-shot providers. REST watch mode can coexist
with other REST clients because the gateway remains the sole TCP owner.

## 6. Perform a controlled mutation

Skip this section on a read-only or production system unless the scheduler
record is an intended operation. First inspect the target and policy:

```text
cat /proc/schedule/summary
cat /policy/rules
```

If a manifest rule matches `/queen/schedule/ctl`, queue a unique approval for
that exact target before the write. Otherwise, do not add an unnecessary
approval.

Submit one strict JSONL record:

```text
echo {"id":"walkthrough-schedule-1","role":"worker-gpu","priority":2,"ticks":3,"budget_ms":120} > /queen/schedule/ctl
cat /proc/schedule/queue
```

Treat `OK ECHO` and the matching read-only queue entry as separate evidence.
If the write returns `ERR`, do not edit the payload until the reported policy,
lifecycle, scope, quota, or schema cause is understood.

## 7. Capture evidence

Create an evidence pack while the gateway still has the live connection:

```bash
run_id="operator-$(date -u +%Y%m%dT%H%M%SZ)"
cargo run -p coh -- evidence pack \
  --rest-url "$COH_REST_URL" \
  --out "out/evidence/$run_id"

cargo run -p coh -- evidence timeline \
  --input "out/evidence/$run_id"
```

Review the pack manifest for missing or errored optional paths. Preserve the
gateway generated-policy fingerprint and `/proc/boot` target manifest evidence
as separate records. An exported pack is a bounded snapshot; it is not a
substitute for target-specific boot or hardware proof.

For the normative pack inventory, redaction rules, offline timeline behavior,
and CI/SIEM commands, continue with
[OPERATOR_RECIPES.md#evidence-packs-ci-and-siem](OPERATOR_RECIPES.md#evidence-packs-ci-and-siem).

## 8. Optional SwarmUI session

With the gateway still running:

```bash
export SWARMUI_TRANSPORT="rest"
export SWARMUI_REST_URL="$COH_REST_URL"
cargo run -p swarmui
```

SwarmUI resolves REST write authentication from
`SWARMUI_REST_AUTH_TOKEN`, `HIVE_GATEWAY_REQUEST_AUTH_TOKEN`,
`COHSH_REST_AUTH_TOKEN`, or `COH_REST_AUTH_TOKEN`. Its telemetry and Live Hive
views are presentation surfaces; its embedded console remains control-capable
and subject to the same gateway and VM checks as `cohsh`.

## Direct-mode alternative

Direct mode is useful for one foreground client. Stop `hive-gateway` first,
then run:

```bash
: "${COH_AUTH_TOKEN:?set the VM console auth token}"
cargo run -p cohsh -- --transport tcp --tcp-host 127.0.0.1 \
  --tcp-port 31337 --role queen
```

Do not run a direct bridge, direct Python `TcpBackend`, `coh`, SwarmUI, or a
second `cohsh` concurrently. Restart the gateway before returning to the
multiplexed workflow.

## Pi 4 adaptation

For a Pi 4 Queen:

1. Complete the image, flash, current-boot, and selected-network proof required
   by [HARDWARE_BRINGUP.md](HARDWARE_BRINGUP.md).
2. Keep serial as the recovery and evidence surface.
3. Set `COH_TCP_HOST` to the current proven control-plane address rather than
   the QEMU loopback forward.
4. Start one host gateway and perform the same bounds, status, shell, publisher,
   and evidence checks above.

GENET and CYW43/SDIO have different acceptance and performance evidence. Do not
convert a diagnostic Wi-Fi stress run into a production capacity claim; use
[BENCHMARKS.md](BENCHMARKS.md) and [TEST_PLAN.md](TEST_PLAN.md).

## Shutdown and cleanup

1. Finish or cancel intended control work and capture final read-only state.
2. Exit `cohsh` and SwarmUI.
3. Stop continuous REST publishers and ticket agents.
4. Unmount any `coh mount` filesystem using the verified procedure in
   [OPERATOR_RECIPES.md#mounted-namespace-with-fuse](OPERATOR_RECIPES.md#mounted-namespace-with-fuse).
5. Stop `hive-gateway` so it releases the VM console.
6. Stop QEMU from terminal 1.
7. Store evidence according to the deployment retention policy; remove local
   secret material from shell/session state as required by that policy.

## Completion checklist

- [ ] The gateway was the only TCP console owner during concurrent operation.
- [ ] Gateway status reported a live upstream connection.
- [ ] Gateway policy and target `/proc/boot` manifest fingerprints were
      recorded separately and compared.
- [ ] Read-only lifecycle, reachability, and log checks completed.
- [ ] Any mutation was intentional and separately verified through read-only state.
- [ ] Optional publishers used the intended live transport, not isolated mock state.
- [ ] Evidence was exported before the retained log window could wrap.
- [ ] Physical-hardware claims, if any, were backed by current target evidence.
