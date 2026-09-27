<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Guide safe single-writer cutover and recovery across two Cohesix hives. -->
<!-- Author: Lukas Bower -->
# Failover between two hives

Cohesix has no built-in cross-Queen replication or leader election. A failover
deployment has two independent hives and one externally enforced writer. Moving
an operator path or finding a healthy standby does **not** prove that the old
writer has stopped. Never send writes to the standby until an external fence
has made the old writer unable to act and the new writer has accepted a higher
durable epoch.

The selected single-host [authority profile](M27A_AUTHORITY.md) disables
production failover and federation. Use this guide only with a separately
selected profile and external provider that implement and verify the controls
below. The source tree includes the host-side watchdog transaction, but its
tests do not establish a physical multi-host cutover. Treat the release's exact
profile and evidence as the authority for a particular deployment.

The current watchdog sends gateway request authentication on probes but does
not send a delegated read ticket. With the default delegated-read policy,
its non-public status and Root-reachability probes are refused. A Queen
gateway's `--read-compatibility` mode permits these reads for a single-caller
installation; it is not a general multi-client production solution. Confirm
the exact gateway read policy and run a successful authenticated probe before
considering the watchdog. Do not weaken a shared gateway's read policy merely
to make a cutover drill pass.

## What each part owns

| Part | Responsibility |
| --- | --- |
| Queen A and Queen B | Separate target state, authenticated console, lifecycle and writer epoch. Neither replicates the other's state. |
| Gateway A and Gateway B | One console owner per Queen; authenticated, delegated REST reads and writes for their own target. |
| Mount A and mount B | Optional REST-backed namespace views. Each gateway URL has one mount per host. A stable `live` symlink is routing convenience only. |
| External fence and promotion provider | Stop the old writer and durably install the new epoch; return independently verified receipts. |
| Watchdog | Probe both gateways, journal the cutover steps, call the enrolled hooks and change routing only after the required receipts and fresh health checks. It cannot create fence evidence itself. |

The [host tools guide](HOST_TOOLS.md) explains gateway and mount setup. Before
enabling cutover, identify both target images, manifests, gateways, mounts,
writer epochs, provider ownership, relay processes, and credential boundaries.
Read each gateway's `/v1/meta/status` and the target's
`/proc/root/reachable` through an operator client with request
authentication and a scoped delegated read ticket. Check the selected
`/proc/authority` epoch as well. Keep the target's
serial surface and each side's evidence separately available.

If standby state or pending external work must be transferred, the deployment
must supply a separately verified method. Audit and replay files are evidence,
not a replication channel. A lost reply to a mutation is an unknown outcome;
reconcile its original identity before any retry or transfer. See
[causal evidence](CAUSAL_EVIDENCE.md) and [failure modes](FAILURE_MODES.md).

## Set up the production transaction

Use `scripts/failover_watchdog.py --help` for its exact options. The durable
`--production` mode requires:

- distinct `--a-rest-url`, `--b-rest-url`, `--a-mount`, `--b-mount` and
  `--live-link` values for the selected hives;
- an explicit `--writer-epoch` greater than the journal's epoch floor, an
  absolute `--cutover-state` path, and one deployment-wide `--lock-file`;
- all five JSON argument-array hooks: `--pause-hook-json`,
  `--fence-hook-json`, `--promote-hook-json`, `--resume-hook-json` and
  `--stop-hook-json`. Each array starts with an absolute executable path;
- authenticated gateway probes, including Root reachability, under the
  compatible gateway read policy described above; and
- a verified old writer identity before starting. An unknown old writer
  refuses automatic cutover.

Provision the hooks as deployment-owned programs. Each receives one
`writer-cutover/v1` JSON request on standard input with a unique transaction
`id`, `src`, `dst`, `writer_epoch` and `action`. It must return one bounded
JSON receipt echoing those fields with `status=succeeded`,
`terminal=true` and `verified=true`. A process exit code alone is not a
receipt. The fence hook must independently establish that the old writer
cannot act. Promotion must durably enforce the new epoch. The stop hook must
stop both writers and the relay. Hooks must retain their own transaction
identity so an interrupted call can be reconciled without replaying a
possibly completed effect.

The default optional shell hooks (`--fence-cmd`, `--relay-pause-cmd` and
similar flags) and `--dry-run` are useful for development rehearsals. They
do not provide production fencing. Do not use a shell command, mount
permission or symlink as the sole writer fence.

## Planned or unplanned cutover

1. Record the active writer, its epoch, both target identities and both
   gateway health results. For planned maintenance, stop new work and drain
   leases and Worker lifecycles through the selected control contract. For an
   unplanned failure, assume the old writer may still act until fencing proves
   otherwise.
2. Pause relays and writers through the enrolled pause hook. Fence the old
   writer through the external provider. Do not route writes yet.
3. Promote the standby with a strictly higher durable epoch. Require the
   provider's terminal receipt and a fresh Root health check at that epoch.
4. Only then change the stable routing path. Check Root health again through
   the new route. Resume relays and writers only after that check passes.
5. Retain the transaction journal, hook receipts, both sides' target and
   gateway observations, and an evidence pack. Review the resulting case
   with [operator evidence](OPERATOR_EVIDENCE.md).

The watchdog persists `prepared -> pause -> fence -> promote -> routing ->
resume -> complete`, syncing each boundary before the corresponding effect.
It removes routing and calls stop-both on a missing or ambiguous receipt,
failed post-routing health, or interrupted transaction. Restart never repeats
an ambiguous promotion. A failed stop remains `stop-unverified`: keep writes
disabled and investigate the provider directly. Storage failure still attempts
external stop. Preserve the journal and its epoch floor.

For failback, treat the original side as a *new* standby. Verify its recovered
state and provider ownership, then run a new cutover with a higher epoch and a
new invocation. Restoring a former symlink target or replaying old relay work
does not re-authorize that writer.

## Check the result

With the selected gateway credentials and a scoped read ticket already
provisioned, collect an inspection and pack from the promoted side:

```sh
: "${COH_REST_URL:?set the promoted gateway URL}"
: "${COH_REST_AUTH_TOKEN:?set gateway request authentication}"
: "${COH_REST_TICKET:?set a scoped delegated read ticket}"

coh inspect --rest-url "$COH_REST_URL" --json
coh evidence pack --rest-url "$COH_REST_URL" --out out/evidence/failover
coh evidence timeline --input out/evidence/failover --scenario federation
```

Confirm target identity, `/proc/authority` writer epoch, Root reachability,
lifecycle, lease state and any pending original-operation identities. Verify
the old writer's fence from the external provider, not from the new gateway
alone. A complete evidence pack supports review; it does not substitute for
the fence or prove that both hives share state.
