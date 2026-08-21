<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define evidence-led diagnosis and recovery for Cohesix operator failures. -->
<!-- Author: Lukas Bower -->
# Cohesix Failure Modes

This runbook maps observable symptoms to bounded evidence and recovery. It does
not redefine protocol errors; canonical response and path semantics are in
[INTERFACES.md](INTERFACES.md).

See the [Glossary](GLOSSARY.md) for Cohesix-specific error, role, and evidence
terms.

An explicit target `ERR` has no side effects unless the interface contract says
otherwise. A connection loss or client-side timeout is different: completion
may be unknown, so inspect read-only state before repeating a mutation.

`/log/queen.log` is a bounded live ring, not permanent storage. It retains up
to 2048 lines, and ordinary `log`/`tail` reads expose bounded windows. Export
evidence before relevant entries are overwritten.

## First response

From an attached `cohsh` session, collect the paths that exist in the selected
profile; do not use shell globs because the command grammar does not expand
them:

```text
ping
cat /proc/lifecycle/state
cat /proc/lifecycle/reason
cat /proc/root/reachable
cat /proc/root/cut_reason
cat /proc/pressure/busy
cat /proc/pressure/quota
cat /proc/pressure/cut
cat /proc/pressure/policy
tail /log/queen.log 64
```

Then record:

- target/profile and active manifest hash;
- transport topology: direct TCP, gateway, mounted filesystem, or mock;
- exact command or HTTP request, response, and local timestamp;
- gateway `/v1/meta/status` and `/v1/meta/bounds` when using REST;
- whether the operation was a read or mutation.

Optional paths can be absent by manifest design. List the parent directory
before treating a missing path as a failure.

## Authentication and authority

| Symptom | Evidence | Recovery |
| --- | --- | --- |
| Direct client reports missing or placeholder TCP token | Client startup error before `AUTH` | Supply the intended non-placeholder token through the deployment secret boundary; do not change target policy to bypass authentication. |
| `ERR AUTH` or connection closes during authentication | TCP endpoint is reachable, but listener rejects the token; repeated failures may enter bounded cooldown | Stop repeated attempts, verify the selected target and secret source, wait for cooldown when reported, then retry once. |
| `ERR ATTACH` | Role, ticket MAC, expiry, subject, scope, or profile policy does not match | Verify the exact role, ticket issuer, subject, validity window, and active manifest. Mint or provision a valid ticket; do not reuse a ticket for another role or subject. |
| A previously working operation reports ticket quota/scope denial | `ERR` detail and ticket/audit counters; generated quota policy | Reduce request rate or payload, wait only when the policy permits replenishment, or obtain a correctly scoped ticket. Do not broaden scope client-side. |
| REST write returns HTTP `401` | JSON `status=ERR`; missing or invalid gateway request-auth header | Supply the gateway request-auth token. This fixes only the HTTP edge; target authority may still refuse the write. |
| REST caller expects its own role/ticket but sees gateway permissions | Gateway startup role/ticket and shared namespace behavior | Run a separately configured gateway when a distinct upstream identity is required. REST has no per-request delegated target identity. |

Generated ticket policy and quotas are summarized in
[snippets/cohsh_ticket_policy.md](snippets/cohsh_ticket_policy.md) and
[snippets/ticket_quotas.md](snippets/ticket_quotas.md).

## Console and transport

| Symptom | Evidence | Recovery |
| --- | --- | --- |
| A direct client hangs, is reset, or reports the console is busy | Another direct `cohsh`, `coh`, SwarmUI, bridge, ticket agent, CAS upload, or Python `TcpBackend` owns the single TCP console | Stop the existing owner, or make `hive-gateway` the sole TCP owner and move concurrent clients to REST. |
| Connection refused | No listener at the configured address/port | Verify the target is running, the network profile is ready, and the host-forwarded or physical address and port are correct. Use the serial `ping`/`netstats` path to separate target liveness from networking. |
| Connection resets after `quit` | `OK QUIT` followed by network close | Expected for a network console session. Reconnect only when a new session is intended. |
| `coh mount` is already active or lock acquisition fails | Existing mount process or endpoint lock | Use the existing mount or unmount it cleanly. Do not run two REST mounts for the same gateway URL. |
| Filesystem path behaves differently from direct client | Mount was created with a different role, ticket, manifest, or endpoint | Inspect the mount process configuration. Filesystem consumers inherit the mount owner's authority. |

See [HOST_TOOLS.md](HOST_TOOLS.md) for the complete composition model.

## Gateway and REST

| Symptom | Meaning | Recovery |
| --- | --- | --- |
| Gateway refuses non-mock startup | TCP auth or request-auth secret is missing, empty, or a rejected placeholder | Provide both secrets through the deployment secret boundary. |
| Gateway refuses a non-loopback bind | Exposure guard is active | Prefer loopback plus a secure tunnel. Use the explicit non-loopback opt-in only behind an approved authenticated network boundary. |
| HTTP `400` | Invalid query, path, size, or JSON request | Correct the request using `/v1/meta/bounds` and the OpenAPI schema. |
| HTTP `429` | Bounded broker queue backpressure | Respect `Retry-After` when present, apply bounded backoff, and reduce concurrency or request volume. |
| HTTP `503` | Upstream console/session is unavailable | Check `/v1/meta/status`, the target, console ownership, and gateway logs. Restore upstream connectivity before retrying. |
| HTTP `504` | Broker response deadline expired | Inspect gateway and target state. For a write, verify the target's read-only status before retrying. |
| HTTP `200` with JSON `status=ERR` | The target completed a deterministic refusal | Read `error`, correct the policy/lifecycle/schema cause, and do not treat HTTP success as operation success. |
| `/v1/meta/bounds` hash differs from client defaults | Gateway binary and client defaults were generated from different manifests | Treat the gateway response as authoritative for gateway request bounds, identify the intended build, and compare both with `/proc/boot` or equivalent image evidence. |

## Lifecycle

Lifecycle writes use `/queen/lifecycle/ctl`. A refused transition leaves state
unchanged.

| Symptom | Evidence | Recovery |
| --- | --- | --- |
| `reason=invalid-transition` | Current `/proc/lifecycle/state` does not admit the command | Select a valid transition from the table below. |
| `reason=outstanding-leases leases=<n>` | Active leases block `drain`, `quiesce`, or `reset` | Inspect `/proc/lease/active`, explicitly finish or revoke the work through its documented control path, then retry after the active count is zero. |
| `reason=gate-denied` on worker attach, telemetry, GPU, or host publish | Current lifecycle closes that operation's gate | Move the node through an authorized lifecycle transition; do not bypass the gate in a bridge or client. |

As-built command admission:

| Command | Allowed state | Result |
| --- | --- | --- |
| `cordon` | `ONLINE`, `DEGRADED` | Enter `DRAINING`. |
| `drain` | `DRAINING` and no blocking leases | Enter `QUIESCED`. |
| `quiesce` | `ONLINE`, `DEGRADED`, `DRAINING` and no blocking leases | Enter `QUIESCED`. |
| `resume` | Any state except `ONLINE` | Enter `ONLINE`. |
| `reset` | Any state except `BOOTING`, with no blocking leases | Enter lifecycle state `BOOTING`. This is not a platform reboot. |

Use the separate authenticated `reboot` console command for a platform reboot.

## Policy approvals

| Symptom | Evidence | Recovery |
| --- | --- | --- |
| `ERR ECHO reason=policy ... EPERM` | `/policy/rules` contains a rule matching the exact target and no usable approval exists | Queue one `{"id":"...","target":"...","decision":"approve"}` record in `/actions/queue`, then retry the target once. |
| A previously used approval is refused | `/actions/<id>/status` is consumed or the log reports replay | Approvals are single-use. Queue a new uniquely identified approval for the exact target. |
| Approval exists but write still fails | Target mismatch, deny decision, role/ticket denial, lifecycle gate, or invalid payload | Compare the exact normalized target and inspect the next refusal. Approval does not bypass other checks. |
| `/policy` or `/actions` is absent | Policy feature is disabled in the selected manifest | Do not fabricate the paths. Confirm the profile and use its as-built policy. |

## Bounds, schemas, and namespace

| Symptom | Recovery |
| --- | --- |
| `path must be absolute`, path too long, walk too deep, or `..` rejected | Use an absolute canonical path and the limits from the live manifest. Parent traversal is never valid. |
| Read exceeds `max_bytes` | Reduce the requested size or use a bounded tail where supported. |
| `ECHO` line or JSON record exceeds a bound | Reduce the record within its schema; do not split a single JSONL control record across writes. |
| Unknown JSON field or invalid enum/token | Use the strict schema in [INTERFACES.md](INTERFACES.md). Clients must not coerce or silently drop fields. |
| `/worker/...` is absent | Use `/shard/<label>/worker/<id>`. `/worker` is a legacy alias only when enabled. |
| `/gpu`, `/host`, `/policy`, or a `/proc` subtree is absent | Verify the active manifest and whether the responsible host publisher has completed. Absence can be a disabled feature, not a transport error. |

## Telemetry pressure

### Worker telemetry ring

`telemetry ring wrap dropped_bytes=<n> new_base=<n>` in the retained Queen log
means the bounded worker telemetry ring overwrote old data. Pull or tail more
frequently, or reduce the producer rate. Changing generated ring capacity is an
engineering/configuration change: update manifest IR, regenerate, and run the
required test plan rather than treating it as an immediate operator command.

### Queen telemetry ingest

`telemetry quota reject bytes=<n> quota=<n>` means the host append exceeded the
configured ingest budget. Stop blind retries, inspect the current segment and
quota state, and reduce or rotate input according to the OS-owned segment
protocol. Segment identifiers are assigned by Cohesix; clients must use the
acknowledged `seg_id` rather than guessing the next name.

## Host publisher visibility

| Symptom | Evidence | Recovery |
| --- | --- | --- |
| `/gpu` has no published devices/models | `gpu-bridge-host --list` may show local inventory, but `/gpu/bridge/status` has no completed live snapshot | Run a one-shot or continuous `gpu-bridge-host --publish` against the intended live transport. `--list` alone does not publish. |
| `/host` has no provider data | Parent path is enabled but selected provider nodes are empty/absent | Run `host-sidecar-bridge` with the required providers and inspect its bounded provider errors. |
| A publisher works alone but blocks `cohsh` | Publisher uses direct TCP and retains the single console connection | Stop it or move the gateway, publisher, and shell to REST mode. |
| In-memory mock data appeared in one Rust tool but not another | Each executable owns separate in-process mock state | Test within one process or use a live gateway/mount. Do not infer live publication from mock output. |
| Python mock data unexpectedly persists or appears in another process | Both clients selected the same filesystem-backed mock root | Use a unique `COHESIX_MOCK_ROOT` for isolation or remove the intended test tree after the run. Shared local mock state is still not live evidence. |

## Generated drift

If prose, client defaults, and the target disagree:

1. Record the target/image manifest fingerprint, selected profile, and any
   gateway/client generated-policy fingerprints separately.
2. Run `scripts/check-generated.sh` for committed default-profile drift.
3. Change manifest/IR inputs, not generated Rust, snippets, scripts, or Python
   defaults.
4. Regenerate all required outputs and run the staged test plan.

Generated mismatch is a build defect; it is not repaired by editing a copied
number in this runbook.

## Physical hardware failures

Serial boot evidence, Pi 4 USB/HDMI gates, GENET, CYW43/SDIO, and flash/current-
image proof have target-specific evidence contracts. Preserve the current boot
sample and route diagnosis through [DRIVERS.md](DRIVERS.md),
[HARDWARE_BRINGUP.md](HARDWARE_BRINGUP.md), and
[TEST_PLAN.md](TEST_PLAN.md). QEMU or mock success does not close physical-
hardware acceptance.
