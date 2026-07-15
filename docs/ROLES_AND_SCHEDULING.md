<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define the as-built Cohesix roles, ticket authority, worker lifecycle, and scheduling layers. -->
<!-- Author: Lukas Bower -->

# Roles, Authority, and Scheduling

This document owns Cohesix role semantics, ticket authority, target-worker
lifecycle, and scheduling layers. It does not redefine namespace schemas,
Secure9P framing, or physical-driver scheduling. See
[INTERFACES.md](INTERFACES.md), [SECURE9P.md](SECURE9P.md), and
[DRIVERS.md](DRIVERS.md) for those contracts.

## Generated authority

The selected profile manifest and generated root-task tables are authoritative
for implemented worker roles, worker count, endpoint-cap requirements,
notification badges, affinity, and scheduling parameters. The checked-in
default-profile values are summarized in
[the generated manifest snippet](snippets/root_task_manifest.md). Do not copy
generated badge bases or quotas into client code or hand-maintained prose.

The role enum recognizes more roles than every target profile implements. A
recognized ticket label or host namespace view is not proof that a target
worker image is enabled.

## Role support matrix

The checked-in default and Pi 4 profiles currently declare the following:

| Role | Ticket and host policy | Target worker in selected profiles | Authority scope |
| --- | --- | --- | --- |
| Queen | Implemented | Root-task authority, not a separate worker image | Hive-wide access to enabled control and observability providers. |
| WorkerHeartbeat | Implemented | Implemented | Own telemetry and the minimal worker observability view. |
| WorkerGpu | Implemented | Implemented | Worker view plus its generated GPU lease scope. GPU hardware remains host-side. |
| WorkerBus | Recognized | **Not implemented** | Host/sidecar policy can describe a bus scope, but the selected target profiles must reject it as target-worker authority. |
| WorkerLora | Implemented | Implemented | Own Worker view plus bounded AI LoRA lifecycle receipts delivered through generated Worker endpoint badges. It has no separate root namespace or file-backed LoRA lease. |

`worker-bus` must not be presented as an active target role until a selected
manifest sets `implemented = true` and the required code, tests, and milestone
evidence agree. Conversely, a worker crate existing in the repository is not
sufficient proof of profile selection.

## Namespace views

Role views are capability filters over the enabled namespace, not independent
filesystems.

- Queen can access the enabled hive-wide tree and can write only the control
  nodes permitted by its ticket and lifecycle state.
- A heartbeat worker can see its own telemetry path and the bounded boot,
  lifecycle, and log information allowed by the server policy.
- GPU and bus roles add only the resource scope encoded for that role. LoRA
  remains within its own Worker view and generated receipt endpoints. None of
  these roles inherits Queen paths.
- Host-only providers remain host-only even when their paths appear in a role
  policy.

The canonical worker telemetry path is:

`/shard/<label>/worker/<id>/telemetry`

The legacy `/worker/<id>/telemetry` alias exists only when
`sharding.legacy_worker_alias` is enabled. Current checked-in profiles enable
the alias for compatibility, but new documentation and clients should prefer
the canonical sharded path. Exact sharding values are generated in
[root_task_manifest.md](snippets/root_task_manifest.md).

### Shard derivation

Host NineDoor, the target namespace adapter, and worker helpers use the same
deterministic algorithm:

1. Hash the exact UTF-8 bytes of `worker_id` with SHA-256; do not case-fold or
   normalize the identifier.
2. Read the first digest byte. When `shard_bits < 8`, keep its most-significant
   `shard_bits`; when `shard_bits = 8`, keep the full byte.
3. Format the resulting value as two lowercase hexadecimal digits.

For the checked-in `shard_bits = 8` profile, `sha256("worker-7")` begins with
`8a`, so the canonical telemetry path is
`/shard/8a/worker/worker-7/telemetry`. When sharding is disabled or
`shard_bits = 0`, the label helper returns `00`; a disabled layout nevertheless
uses `/worker/<id>/telemetry`, not `/shard/00/...`. This vector is suitable for
checking clients that construct canonical paths locally; clients must still
discover the active profile instead of assuming eight shard bits.

A ticket scope such as `/worker`, `/gpu`, or `/bus` is an authority prefix; it
does not replace the canonical telemetry path template generated for the role.
WorkerLora uses `/worker` because it is an AI worker role, not a device or
radio authority.

## Capability tickets

Tickets are MAC-protected capability claims encoded as:

`cohesix-ticket-<hex-payload>.<hex-mac>`

The current claim structure contains:

- role;
- budget (`ticks`, `ops`, and `ttl_s`);
- optional subject identity;
- optional mount specification;
- issue time;
- optional path/verb/rate scopes; and
- optional bandwidth and cursor quotas.

The implementation lives in
[`crates/cohesix-ticket`](../crates/cohesix-ticket/src/lib.rs). Ticket strings
and secrets are credentials; documentation, logs, and evidence must not print
their secret material.

### Attach rules

- Queen may attach without a ticket. If a Queen ticket is supplied on an
  enforcing path, it must be valid for Queen.
- Every worker role requires a ticket and subject identity.
- The requested role must match the ticket role.
- The requested worker identity must match the ticket subject when both are
  supplied.
- MAC, TTL, scope, quota, role, subject, and selected-manifest bounds are
  checked before authority is granted.
- Target worker attach additionally requires the role to be marked implemented
  and the generated cap-backed endpoint authority to match its role and epoch.

Host NineDoor verifies tickets against registered role keys. The target console
performs transport authentication first for TCP, then validates the
application `ATTACH` role/ticket. Transport `AUTH` proves access to the console;
it does not grant Queen or worker namespace authority by itself.

### Budgets and quotas

Budget defaults are role-aware. Queen uses an unbounded default when no ticket
overrides it; GPU and heartbeat-family workers use finite defaults from the
ticket library or their worker record. The server clamps applicable ticket
budgets to the worker record rather than allowing a ticket to expand authority.

Generated ticket limits bound scope count, path length, per-scope rate,
bandwidth, cursor resumes, and cursor advances. The current generated values
are documented in [ticket_quotas.md](snippets/ticket_quotas.md) and
[root_task_manifest.md](snippets/root_task_manifest.md).

Exhaustion or expiry is a refusal, not a successful no-op. The server closes or
revokes the affected authority and returns the protocol-specific error surface.

## Worker authority lifecycle

Target worker authority has two independent proofs:

1. **Session authority.** A valid role, subject, ticket, lifecycle gate, and
   namespace scope authorize an operation at the control-plane layer.
2. **seL4 invocation authority.** For implemented target roles, generated
   endpoint badges encode the action, role, and authority epoch. Generated
   notification badges carry revoke, shutdown, lease-expiry, telemetry-pressure,
   and optional IRQ events.

Root-task rejects metadata-only worker authority when the profile requires
badged endpoint caps. The decoder and checks are implemented in
[`worker_authority.rs`](../apps/root-task/src/worker_authority.rs); exact badge
values remain generated data.

Revocation is bounded and explicit:

1. Policy denial, ticket expiry, budget exhaustion, lease expiry, or a valid
   Queen lifecycle operation selects the authority to close.
2. The session or worker record is marked closed/revoked with a reason.
3. Implemented target workers receive the generated lifecycle notification
   where required by the selected profile.
4. Subsequent endpoint, namespace, and console operations fail rather than
   silently continuing.
5. Logs and enabled observability providers record the transition.

A generated notification record proves the configured topology. Target
acceptance still requires evidence that the selected image created, delivered,
and handled the notification correctly.

## Scheduling layers

Cohesix exposes three scheduling concepts. They must not be collapsed into one
claim.

### 1. Kernel and target-task scheduling

The selected manifest defines the target worker scheduling profile. Current
checked-in profiles select a non-MCS record with a generated priority, domain,
and bounded service-turn budget; MCS budget/period fields are zero and consumed
budget evidence is disabled for that profile. Affinity is also profile-generated.

These values are target execution policy. Claims about applied affinity,
priority, scheduling contexts, or consumed budget require the selected seL4
build and target evidence, not just generated metadata.

### 2. Root-task service turns

The event pump gives workers, consoles, timers, and driver clients bounded
service opportunities. A service-turn budget limits cooperative work performed
before yielding. It does not create a new kernel scheduler and must not be used
to claim CPU isolation or real-time latency without target evidence.

Physical operator input and an authenticated TCP console follow the bounded
priority rules in `AGENTS.md`; nonessential mirroring and verbose output may be
reduced under load, but command acknowledgements and emergency diagnostics must
remain live.

### 3. Namespace schedule queue

`/queen/schedule/ctl` is a bounded control-plane queue. Its JSONL entries and
`/proc/schedule/*` snapshots describe requested orchestration state. They do not
directly set seL4 TCB priorities or create scheduling contexts. Exact paths and
record fields are in [INTERFACES.md](INTERFACES.md), with generated capacities
in [observability_interfaces.md](snippets/observability_interfaces.md).

## Lifecycle and scheduling evidence

Documentation must label evidence by layer and profile:

| Evidence | What it proves | What it does not prove |
| --- | --- | --- |
| Generated role table | Role selection and configured bounds | Target worker boot or invocation |
| Endpoint/notification unit test | Encoding and validation logic | On-target capability delivery |
| QEMU worker transcript | QEMU profile behavior | Pi 4 behavior |
| Pi 4 boot log with matching image | Behavior for that image and boot | A newer image or another transport |
| Schedule-queue test | Control-file parsing and bounds | Kernel scheduling or latency |
| Counter-qualified target trace | Timing for the recorded target/profile | A general performance guarantee |

## Source map and verification

- Profile inputs: [`configs/root_task.toml`](../configs/root_task.toml) and
  [`configs/root_task_pi4_uboot_aarch64.toml`](../configs/root_task_pi4_uboot_aarch64.toml)
- Generated worker tables:
  [`apps/root-task/src/generated`](../apps/root-task/src/generated)
- Ticket format: [`crates/cohesix-ticket`](../crates/cohesix-ticket)
- Shared role/ticket parsing:
  [`crates/cohsh-core/src/ticket.rs`](../crates/cohsh-core/src/ticket.rs)
- Target worker authority:
  [`apps/root-task/src/worker_authority.rs`](../apps/root-task/src/worker_authority.rs)
- Host role enforcement:
  [`apps/nine-door/src/host`](../apps/nine-door/src/host)
- Target namespace adapter:
  [`apps/root-task/src/ninedoor.rs`](../apps/root-task/src/ninedoor.rs)
- Validation workflow: [TEST_PLAN.md](TEST_PLAN.md)

Any change to a role, ticket claim, namespace scope, endpoint badge layout,
notification, or scheduling field must update manifest IR, generated artifacts,
tests, and this suite in the same authorized change.
