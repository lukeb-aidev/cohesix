<!-- Author: Lukas Bower -->
<!-- Purpose: Define delegated mutation, replay identity and Release A authority deployment contracts. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Milestone 27a authority contract

Authority hardening is implemented and Milestone 27a is **Complete** under the
2026-09-14 owner decision. Evidence, Rust sign-off, DD30 accepted risk and the
approved absence of the M26d status-baseline comparison are recorded in
[audit/M27A_COMPLETION_EVIDENCE.md](audit/M27A_COMPLETION_EVIDENCE.md).

## Caller delegation

Every REST mutation, including `/v1/fs/echo-batch`, requires request authentication
(`Authorization: Bearer` or `x-cohesix-auth`) and exactly one `x-cohesix-ticket`.
The gateway verifies the ticket MAC with the explicitly selected
`--delegation-key-ref env:NAME` or `file:/absolute/path`. The delegated claims must
have a subject, finite unexpired TTL, and write scopes. Role, mount, scope, rate,
operation and bandwidth restrictions intersect the configured gateway ceiling.
Each batch line consumes one operation. Refusals use `EPERM` or `ELIMIT`.
When a REST ticket specifies a mount, every mutation must also fall beneath its
canonical mount point; a broader role or write scope cannot widen that bound.

Caller quota state is keyed by the SHA-256 of the canonical signed ticket. Live
entries are not evicted to admit other clients. All writes retain the existing
serialized authenticated console path. Audit records name `gateway_enforced`,
the ticket hash, configured upstream credential class, concrete path and action,
and confirmed `ACK`, `ERR`, or an unconfirmed transport outcome. The VM still
verifies the gateway session; this is not VM-verified caller identity. Read-only
REST compatibility remains scoped to that session. Delegated quotas are local to
the gateway process; retain one gateway owner per issuer/transport ceiling.

Rust REST clients accept `COH_REST_TICKET` or an explicit caller ticket.
`cohsh` REST attach replaces its caller binding. Python `RestBackend` accepts
`delegated_ticket=` and the same environment source. Client parsing is only a
convenience check; only the gateway verifies the MAC. A client must not retry an
ambiguous raw write automatically. Explicit strict-intent retries reuse all bytes
and identity fields.

## Strict Queen commands

The legacy `/queen/ctl` JSON command is unchanged in compatibility profiles.
`/queen/intents/ctl` accepts `queen-intent/v1` with `schema`, `id`,
`idempotency_key`, `issued_unix_ms`, and `cmd`. `cmd` is a JSON string containing
one existing Queen control object. Production disables the legacy path and its
console `SPAWN`/`KILL` shortcuts before acknowledgement or dispatch. Compatibility
profiles retain all three entry points.

```json
{"schema":"queen-intent/v1","id":"spawn-one","idempotency_key":"retry-one","issued_unix_ms":1000,"writer_epoch":1,"cmd":"{\"spawn\":\"heartbeat\",\"ticks\":3}"}
```

This is a structural example, not a current timestamp or deployment epoch.
Identifiers are single ASCII components of at most 128 bytes; empty values,
option prefixes, `..`, separators and controls are rejected. The envelope is at
most 2048 bytes, further bounded by the manifest. Unknown and duplicate fields
are refused. The dedupe reservation precedes policy approval consumption and the
side effect. An exact duplicate returns the retained outcome without consuming
another approval or repeating the effect. Changed fields under the same identity
produce `EPERM idempotency-conflict`. Capacity exhaustion returns `ELIMIT`;
completed identities are not evicted. Compatibility policy defaults hold 64
entries; Release A selects 512 on both QEMU and Pi. Release A schema 1.28 and
current schema 1.29 both permit 1–512 entries and project the selected capacity
into the Python SDK. Pressure
qualification must budget every distinct intent, including refusals and fault
preflight, before the first mutation. Increasing capacity changes no identity,
approval, fencing or audit semantics.
Each strict terminal journal record includes `dedupe: "fresh"` or
`dedupe: "duplicate"` alongside the unchanged full envelope and outcome.
Legacy journal records retain their existing format.
When audit is enabled, the complete encoded successful terminal must fit the
selected journal before policy approval or a side effect is attempted. An
oversized record returns `ELIMIT` without reserving an identity or creating a
Worker. Journal wrap may discard older whole records; it does not make a
retained intent fresh.

The table belongs to one VM authority lifetime. VM restart persistence belongs
to Milestone 29; restarting the VM is not an approved retry mechanism.
`/proc/queen/dedupe` contains a bounded NDJSON summary and at most sixteen recent
outcomes. Each identity hash is SHA-256 of `id`, one zero byte, then
`idempotency_key`; outcomes are `ACK`, `ERR`, or `in_flight`.
`/proc/authority` exposes the selected writer epoch and authority class.

## Host execution and writer ownership

`writer_epoch` is an optional extension of compatibility host-ticket schemas and
is required by production policy. When present, it must equal the selected
manifest epoch. Local execution and relay enforce it independently of decision
freshness. Durable execution and relay journals retain an owner floor and refuse
configuration rollback. Promotion is refused while the old epoch has pending
recovery. Preserve the journal directory across process restarts.

Version-1 provider work now retains prepared, executing, receipt and terminal
state. A persisted receipt is republished exactly after ambiguous delivery.
Executing work without a receipt is deadlettered as `replay-ambiguous`, never
executed again. The version-1 journal is bounded to 256 identities and 1 MiB;
full state refuses new work rather than dropping replay protection. The existing
version-2 admission-sequence compaction remains supported. Provider identifiers,
target components, action-specific argument names and types, and manifest
allowlists are checked before dispatch.

Strict intents, host tickets, WAL entries, results and audit bodies can retain
an optional `admission` object: `admission_id`, `intent_hash`, `policy_hash`,
`state_epoch`, `resource_generation`, `decision_expiry`. Hashes are lowercase
SHA-256 hex. This is correlation only. No missing admission is fabricated and no
27a component issues a 28c admission decision. `writer_epoch` is separate.

## Production profile and secret sources

Manifest schema 1.21 introduces `[authority]` and ticket `secret_ref`. The
compiler materializes the selected single-writer Release A source as follows:

```sh
cargo run -p coh-rtc --bin coh-rtc-authority-profile -- \
  --base configs/root_task.toml \
  --verification-key /absolute/deployment/cas-verification.hex \
  --writer-epoch 1 --out out/release-a/root_task.toml
```

Select that generated TOML as the manifest for every subsequent target build,
compiler invocation, host tool build and evidence run. The deployment public key
must exist and differ from the published fixture key. Materialization enables
audit/replay, strict intents and required writer epochs, disables arbitrary
memory diagnostics and federation, and replaces ticket literals with
`env:COH_TICKET_QUEEN_KEY`, `env:COH_TICKET_WORKER_HEARTBEAT_KEY`,
`env:COH_TICKET_WORKER_GPU_KEY`, `env:COH_TICKET_WORKER_BUS_KEY`, and
`env:COH_TICKET_WORKER_LORA_KEY`. Supply deployment secrets before compiling the
root image. Secret references, not key values, appear in resolved JSON.
The root binary nevertheless contains the resolved ticket credentials: references
protect generated configuration, not the confidentiality of a compiled image.
Keep privately provisioned target images within the deployment's credential
boundary. Public evaluation images require a separate explicitly shared keyset;
never publish an image compiled with an active private hive's keys. Credential
rotation requires rebuilding and qualifying the target image, together with its
matching host configuration.

For `cohsh`, the GPU bridge and Python, TCP credential resolution is explicit
input first, then `COH_AUTH_TOKEN_REF`, then `COH_AUTH_TOKEN`, then
`COHSH_AUTH_TOKEN`; Python can finally inspect the selected manifest. Other
clients accept references through their documented token option or environment
variable; see the tool-specific catalog in [HOST_TOOLS.md](HOST_TOOLS.md).
A selected reference is exactly `env:NAME` or
`file:/absolute/path`. Failure does not fall back. Secret reads are bounded to
4096 bytes; empty, malformed and generated placeholder credentials are refused.
Live credential values must be ASCII and contain no whitespace or controls.
`bootstrap`, `changeme` and Worker fixture literals cannot authenticate live
SwarmUI, GPU bridge or console clients. Explicit mock/replay constructors are
separate. File sources are read again for rotation at each new authentication.
CAS public verification material may be packaged; private signing material may
not. The release inventory names exact files and forbids the fixture private key.
The compiler writes `configs/generated/cas_verification_key.hex` from the same
selected public key embedded in the root image. Artifact records bind those
bytes, and release assembly copies them into the public resource slot. Assembly
rejects mismatches and the published fixture public key. It never re-reads a
mutable deployment key file as a substitute for the retained compiler artifact.

The GPU console frame bound defaults to 8192 bytes including its four-byte header; an
invalid peer length is rejected before allocation. Arbitrary root-console
`hexdump` is disabled by the generated default and rejected in production policy.
An explicit non-release bring-up profile may enable reads of 1–256 bytes only
within the HAL-classified immutable root code span. Reads of rodata, mutable
state, device mappings, crossing ranges or overflowing addresses are refused
before dereference. Bounded serial audit lines record admission and refusal.

Production failover and federation remain disabled in the single-host Release A
profile. The existing optional-hook failover watchdog is development tooling;
the durable `--production` transaction is documented in [FAILOVER.md](FAILOVER.md).
Host tests are not accepted physical cutover evidence. A production cutover requires
old-writer fencing, durable epoch promotion, uniquely correlated terminal
receipts, and health checks before and after routing changes.

## Evidence and compatibility

Evidence packs capture `/proc/authority` and `/proc/queen/dedupe` alongside
existing audit, replay and host-ticket records. Python target profiles use
`cohesix-python-profile/v2` to carry selected authority limits; a target-neutral
wheel default cannot establish a live target's identity or production readiness.

Generated flags reject premature VM-verified delegation, production Worker or
driver ledger binding, structured quarantine, host AI and production failover.
This preserves accepted 26e task/driver authority and any independently accepted
Milestone 29 storage bundle. Milestone 28d owns production ledger and quarantine
projection; Milestone 28c owns per-intent admission.

The gateway authority microbenchmark measures status reads, delegated writes,
duplicate/conflicting identities and stale epochs without client retries. Its
latency, refusal, queue, cache and audit counters are host measurements and do
not establish Pi throughput or target acceptance.

## Release migration and historical errata

Existing immutable release snapshots retain their historical examples. Any
example using `bootstrap`, `changeme`, Worker fixture secrets, or the public
fixture CAS trust chain is development-only and must not be used for deployment.
Do not edit those snapshots to relabel their qualification. Provision private
sources and a deployment public key, rebuild from the selected manifest, and
retain new exact-image evidence.

`release_bundle.sh --release-manifest <provisioned-source.toml>` binds assembly
to that selected input. The retained resolved artifact must have production
authority enabled, strict intents, writer fencing, execution WAL and bounded
audit/replay. Both tested and build-only assembly reject a development profile;
build-only assembly still makes no target acceptance claim. Exact inventory
validation and `scripts/authority_release_gate.py` content scanning reject
unexpected files, private fixture key bytes under renamed paths and release
secret canaries. `--allow-development` is used only by source due diligence and
cannot authorize release assembly.

`cas-tool pack --signing-key env:NAME` or `file:/absolute/path` selects a bounded
hex Ed25519 signing key. Explicit input precedes `COH_CAS_SIGNING_KEY_REF`;
a missing selected source never falls back. Compatibility file paths remain
supported. The 10 MiB REST response cap applies to success and error bodies;
peer Content-Length cannot authorize an allocation above that bound.
