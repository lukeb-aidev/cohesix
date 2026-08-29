<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define Cohesix vulnerability reporting, security boundaries, implemented controls, and generated profile limits. -->
<!-- Author: Lukas Bower -->

# Security

Cohesix is a pre-production research operating system. Its design reduces and
makes authority visible; it does not make the complete system formally verified
or suitable for unattended production use. seL4's machine-checked proofs apply
to the kernel under their stated assumptions, not automatically to Cohesix
userspace, host tools, firmware, drivers, or deployment policy.

See the [Glossary](GLOSSARY.md) for Cohesix-specific authority, role, and
evidence terms.

## Reporting a vulnerability

Do not disclose a suspected vulnerability, exploit, secret, or sensitive log in
a public issue or discussion.

1. Open this repository's **Security** tab and use GitHub private vulnerability
   reporting when it is available.
2. Include the affected commit or release, target/profile, minimal reproduction,
   expected security boundary, and impact. Attach only redacted evidence.
3. If private reporting is unavailable, open a non-sensitive issue asking the
   maintainer for a private reporting channel. Do not include vulnerability
   details in that issue.

Cohesix is maintained as a research project and does not promise a fixed
response or remediation SLA. The maintainer will acknowledge a usable private
report, validate scope, coordinate disclosure when practical, and record a fix
and verification evidence if the issue is accepted.

Current source receives security fixes first. Versioned release directories are
immutable research snapshots; a backport exists only when the corresponding
release notes explicitly identify it. Never assume that an older bundle has the
security posture of the current tree.

## Operator diagnostic authority

`bi`, `caps mcs`, and `smp mcs` are bounded projections on an already admitted
operator surface. Printed slots, badges, and generations are identifiers, not
transferred authority. The live registry is copied with a non-blocking lock and
released before output. Inspection performs no debug dump, capability or
scheduling mutation, or `seL4_SchedContext_Consumed` call; generated state is
never labelled live and early console never claims activation.

## Security objectives and non-goals

Cohesix aims to:

- keep the privileged kernel and in-VM trusted computing base small;
- make resource and namespace authority explicit through seL4 capabilities,
  generated manifests, roles, tickets, and lifecycle state;
- validate hostile input before side effects;
- bound memory, work, retries, queues, and retained evidence;
- compartmentalize physical drivers and keep GPU ecosystems host-side;
- leave deterministic receipts for accepted and denied control-plane actions.

Cohesix does not currently provide:

- a formally verified whole system;
- a POSIX security boundary, multi-user desktop, or general-purpose server;
- in-VM TLS, HTTP, SSH, CUDA, NVML, model execution, or package-management
  services;
- encryption for the direct TCP console or `hive-gateway`;
- a Pi 4 IOMMU/SMMU boundary on BCM2711;
- a guarantee that a compromised operator host, boot firmware, selected driver
  firmware, or privileged build environment cannot compromise a deployment.

## Trust and authority boundaries

The seL4 kernel controls memory objects, execution, notifications, interrupts,
and IPC through capabilities. Cohesix root-task remains trusted for bootstrap,
HAL admission, manifest enforcement, namespace authority, tickets, lifecycle,
and audit. Queen and Worker-role sessions receive only their generated
namespace view; Worker tickets are mandatory and Queen ticket requirements are
profile-controlled. Current profiles mark every target Worker role
non-executable and disable Worker endpoint-cap and lifecycle-notification
authority. Reserved generated badges are not installed capabilities.

Physical devices run in manifest-declared, single-threaded Rust driver
runtimes. HAL owns physical-address discovery, device-untyped admission, MMIO,
IRQ, DMA, PCI, SDIO, and board-level resource assignment. Driver runtimes may
touch only the resources delivered through their generated fixed ABI. The root
task may admit resources, submit bounded service turns, and retain diagnostics,
but must not own steady-state physical drivers.

Host tools are outside the target TCB. They may use operating-system services,
CUDA/NVML, model runtimes, REST, and UI frameworks, but may only project the
documented console and Secure9P semantics. A compromised host tool with a valid
Queen secret can exercise that secret's authority; least-privilege host process
and secret handling remain deployment responsibilities.

See [Architecture](ARCHITECTURE.md) for component boundaries,
[Roles and scheduling](ROLES_AND_SCHEDULING.md) for role authority, and
[Drivers](DRIVERS.md) for the physical-device proof contract.

## Network and console exposure

The authenticated root-task TCP console is the only in-VM TCP listener. It uses
length-framed console lines with transport `AUTH`, application `ATTACH`,
bounded commands, `OK`/`ERR` responses, and `END` stream terminators. It
is not a Secure9P listener.

Authentication is not encryption. Bind direct console forwarding to loopback
or carry it through an authenticated encrypted tunnel. Hive Gateway also
defaults to loopback, refuses a non-loopback bind without explicit opt-in, and
does not terminate TLS. Use different secrets for target-console and gateway
request authentication.

Only one process may own the target TCP session. Concurrent clients share one
Hive Gateway owner rather than racing direct `cohsh`, SwarmUI, or bridge
connections.

### Console compartment boundary

The active `console-network-runtime` child uses console-network ABI v5 and owns
smoltcp, Ethernet/IP/TCP state, length framing, constant-time transport-token
comparison, and pre-authentication timeouts. Root owns no TCP parser. It receives
only a copied, authenticated bounded command and remains the sole authority for
Queen policy, role tickets, namespace operations, and command execution.

Four fixed shared pages carry the ordinary pointer-free sequence-last packet,
control, egress, and event records between root and the child. Page mappings are
directional, reserved fields must be zero, lengths are validated before copying,
and an incomplete or unstable commit is rejected. The child cannot access root
CSpace, namespace-wide authority, `SchedControl`, or undeclared memory. Its only
device-side authority is the exact generated backend: QEMU direct-VirtIO
resources or, on the Pi `bcmgenet-v5` profile, a CPU-only direct-GENET extension
that grants no MMIO, DMA, device-visible, or physical-address authority.

The Pi extension becomes eligible only after DHCP and exact quiescence of every
root-mediated GENET command, RX, and TX cursor. Root publishes the pending
generation before one generation-bound zero-payload `DGHO`; only its exact
`PROGRESS/READY` terminal activates the link. Exact `IDLE/QUIESCING` retains the
old path for bounded drain and retry. The 32 reused pages map as cacheable
Normal/XN memory in the GENET and console children: page 0 carries aligned
sequence-last control, pages 1 through 15 carry GENET-to-console RX, and pages
16 through 31 carry console-to-GENET TX. GENET remains the sole MMIO/DMA/IRQ and
private-DMA-ring owner. Console-network remains the sole smoltcp/TCP/auth owner.
Root retains lifecycle, control-event, and fault supervision but no steady
packet-copy, poll, or GENET packet-command authority after READY.

Each direct direction has one producer and one consumer. Generation-bound
monotonic cursors and sequence-last commits carry truth; reciprocal send-only
notifications are coalescing wake hints, and each consumer rechecks durable
state before waiting. A handoff failure, peer fault, invalid cursor or sequence,
stale generation, descriptor drift, or containment error poisons the link and
pair-contains both generations. Coupled containment suspends GENET, removes both
signal caps and all 32 external console mapping caps before anchor revoke, and
cannot return to root packet mediation as a fallback.

The selected Pi direct-GENET console image spans 66 PT_LOAD pages and is admitted
by a generated service inventory of 104 frames and 161 retained root CSpace slots.
The one-page increase is one immutable executable image frame and its retained
mapping cap, not an enlargement of a data-plane or scheduling budget.
The 32 direct pages reuse external GENET-owned frames and add mapping caps, not
new data-plane frame objects or a larger child untyped. Exact construction
bounds constrain authority; they are not runtime or performance evidence.

Publication requires explicit credit after root has copied, validated, and
durably handled the indicated records. Notifications are wakeups, not data.
Duplicate, stale, uncredited, or generation-mismatched records fail closed.
Graceful shutdown publishes one terminal record before containment; fault and
revoke paths scrub mappings, signals, pending responses, and capabilities
before the child generation can be reused.

Root control, the console child, and the Pi resumable GENET runtime use the
selected generated timeout policy.
Under `NaturalPostpone`, exhaustion of the current scheduling-context refill
postpones execution until replenishment; standard faults remain installed and
terminal. GENET protocol, cursor, DMA, IRQ, device-deadline, and paired
containment failures remain fail closed; only ordinary legal budget exhaustion
postpones. Reserved timeout identities remain accounted, but client timeouts,
retry policy, public grammar, and fault authority are unchanged. Exact temporal
values and response analysis belong to the selected generated profile and
[Roles and Scheduling](ROLES_AND_SCHEDULING.md).

### Authentication and attachment

Transport `AUTH` proves access to the console listener. Application
`ATTACH` separately selects a role and validates any required ticket. A
Worker-role attachment does not create a Worker task; executable Worker
lifecycle is admitted independently by the generated supervisor.

Attachment is one authority transaction. The namespace child validates and
prepares the role/ticket context, NineDoor commits that context, and root emits
`OK ATTACH` only after the bridge transaction succeeds. Optional logging,
tracing, or endpoint diagnostics are best-effort observations and cannot grant,
veto, or roll back authority.

A leaky-bucket rate limiter allows two failed authentication attempts in a
60-second window; the next failure enters a 90-second cooldown. Root adds
bounded exponential backoff beginning at 250 ms. Denials and successful role
assertions emit bounded audit lines without printing secrets.

### Response and availability rules

Responses preserve the public `OK`/`ERR`/`END` order. Large bodies are
split into bounded records and service units. An authenticated response lane
receives bounded flush priority without starving physical operator input,
fatal output, timers, containment, or ordinary service.

Routine diagnostics are best-effort and may be dropped under their declared
bound. Security-relevant authority, lifecycle, completion, and fault decisions
remain in typed records or evidence surfaces rather than relying on an
unstructured log line. A full diagnostic queue cannot create an alternate
output owner, retry loop, or unbounded backlog.
## Input, protocol, and memory controls

All user-controlled console lines, paths, 9P frames, JSON records, tickets, and
configuration values must be validated before side effects. The public
Secure9P red lines are 9P2000.L only, `msize <= 8192`, walk depth at most 8, no
`..`, and no fid reuse after clunk. Short writes, tag concurrency, cursor
advances, scope rates, and retained bytes are generated and bounded. See
[Secure9P](SECURE9P.md) for session invariants and
[Interfaces](INTERFACES.md) for record schemas.

Network RX/TX, serial, pending console lines, driver rings, logs, telemetry,
evidence, and retry work use fixed or manifest-bounded storage. Overload must
surface a deterministic refusal or counter; it must not create an unbounded
queue or silent retry loop. The event pump serializes authoritative target
state. SMP is used for separate single-threaded tasks, not shared-memory
multithreading of authority state. The Pi direct-GENET pages carry only bounded
SPSC data-plane state between two fixed owners; they do not make root authority
shared or make notifications authoritative.

The namespace-service contract treats path, payload, partial
frame, sequence, and generation fields as hostile. The as-built
`nine-door-runtime` source validates them in a restricted `no_std` child and
returns only a typed bounded prepared operation; the root-side contract
independently checks the exact response identity and bytes before policy or
mutation. Root's endpoint cap is Write + GrantReply with neither Read nor
Grant, while the child's endpoint cap is Read-only. Request and response
mappings are directionally restricted, exactly two pages each, and backed by
disjoint live frame handles validated against generated virtual addresses.
The QEMU constructor validates the embedded image digest, entry, and W^X load
span, allocates only the compiler-budgeted anchor generation, and registers the
still-suspended TCB before registry seal. During construction, root configures
and binds one compiler-budgeted, root-retained bootstrap scheduling context;
the child receives neither that scheduling-context cap nor `SchedControl`
authority. After registry seal and root-fault activation, root resumes the
child, validates an empty `Log` parser probe, observes the child's atomic
`ReplyRecv` transition into the next receive, and unbinds the exact bootstrap
scheduling context before declaring the service passive. Activation, probe, or
unbind failure revokes the namespace boundary and suspends the child where
possible, so no later Call can block on a failed bootstrap. The receive-loop
Reply object is shared only with the generated root-fault recovery slot. On
fault, an outstanding donor receives exactly one typed `Closed` failure before
the durable containment record is published;
without an outstanding Call no Reply is attempted. Recovery authority is then
retained and serialized until all four request/response mapping lifecycles are
scrubbed and unmapped. The recovery Reply cap is then quietly deleted before
the two fault caps and retained anchor are revoked. Root-control advances that fixed NineDoor containment cursor one
material unit per exclusive Recovery turn and only after all higher-priority
console-network recovery work is absent. Steady operation is passive donation
only after the bootstrap
scheduling context has been unbound; no `SetAffinity` placement path is used.
The active console service cannot enter this passive path. Closing
with a partial frame, queue saturation, cancellation, child-generation
revocation, replay, and late completion fail closed. The child receives only
its service endpoint/Reply object and two bounded shared-frame mappings; it
receives no Queen policy, root CSpace, device, broad namespace, scheduling
context cap, or `SchedControl` authority. The bootstrap scheduling-context
implementation remains subject to live QEMU qualification. This internal ABI does
not relax the rule
that the authenticated console is the only in-VM TCP listener. Live QEMU fault
injection remains evidence required beyond source and target checks; it is not
Pi hardware acceptance.

Operational host/GPU projection begins `unavailable source=none`; target source
contains no fabricated provider or GPU topology. GPU snapshots use
`gpu-bridge-snapshot/v2` and arrive only through an authenticated Queen
session. Root validates the production source identity, epoch and strictly
increasing sequence, observation time, bounded TTL, catalog and per-manifest
digests, CAS/base/adapter compatibility, and activation generation/receipt
before atomically replacing a generation. Fixture-mode, stale, replayed,
forged, incompatible, or oversized snapshots fail closed. Accepted data is
withdrawn at TTL and direct writes to the active-model pointer are denied.
Console/REST placeholder credentials fail before connection, and fixture
signing keys are forbidden from operational target and release closures.
Operational manifests select `cas.signing.verification_key_path`; coh-rtc
validates a public Ed25519 point and emits only those public bytes. The
corresponding signing key remains in an external secret store. The checked-in
fixture signing seed and its public counterpart are test-only and cannot be
selected by the QEMU/Pi runtime or release manifest.

<!-- coh-rtc:ticket-quotas:start -->
### Ticket quota limits (generated)
- `ticket_limits.max_scopes`: `8`
- `ticket_limits.max_scope_path_len`: `128`
- `ticket_limits.max_scope_rate_per_s`: `64` (0 = unlimited)
- `ticket_limits.bandwidth_bytes`: `131072` (0 = unlimited)
- `ticket_limits.cursor_resumes`: `16` (0 = unlimited)
- `ticket_limits.cursor_advances`: `256` (0 = unlimited)

_Generated by coh-rtc (sha256: `1b869521f68c26d43c1ad278fbc557f2442e438ab12d443a142e53a33e4466fb`)._
<!-- coh-rtc:ticket-quotas:end -->

These values are the committed default-profile snapshot. The selected source
manifest and resolved manifest govern a target build.

## Hardware, DMA, and firmware

QEMU is the reference development and regression target; it is not physical
hardware proof. On Raspberry Pi 4, selected physical devices are admitted to
isolated driver runtimes after HAL coverage checks. BCM2711 has no supported
IOMMU/SMMU isolation path in the current Cohesix profile, so DMA safety relies
on HAL-owned ranges, bounded rings, cache policy, quarantine rules, and
single-owner driver admission. Documentation must not describe that as hardware
DMA isolation.

The direct-GENET CPU link does not weaken that owner boundary or enlarge the DMA
claim. GENET alone retains its private DMA buffers and device descriptors and
copies validated frames to or from CPU-only shared pages. Console-network sees
neither DMA mappings nor physical addresses. Coupled fail-closed containment is
mandatory because the peers share data-plane state: a fault in either
generation removes reciprocal signalling and console page copies before revoke,
without reviving root packet mediation.

The GENETv5 implementation uses Linux device-tree and driver behavior plus
U-Boot bring-up as reference material. The CYW43455/SDIO implementation uses
OpenBSD `bwfm`, Zephyr/Infineon WHD layering, and Linux `brcmfmac` edge cases as
reference material. These are provenance references, not code sources; source
lift is prohibited. CYW43455 and SDIO runtimes are implemented research
surfaces, but current-image association, DHCP, TCP, and repeatability remain
evidence-gated in the build plan. Likewise, a generated direct-GENET descriptor,
successful target build, or staged image proves neither handoff nor traffic.
Fresh same-image Pi evidence must separately qualify GENET correctness,
containment, latency, throughput, and benchmark behavior.

Pi firmware, U-Boot, Wi-Fi firmware/NVRAM, and the external seL4 build are
deployment dependencies. Pin their provenance, verify staged hashes, and keep
flash proof separate from proof that the board booted the same image. The
hardware runbook owns the accepted evidence sequence.

## Secrets and sensitive data

- Do not commit deployment credentials or copy them into examples, issue text,
  evidence, command history, or screenshots.
- Root-task and the primary `coh`, `cohsh`, Python, and live-gateway paths reject
  the literal placeholder `changeme`. Some ancillary compatibility binaries
  still expose legacy placeholder defaults; that value cannot authenticate a
  current target and must be overridden. Select real, distinct target-console
  and gateway request secrets.
- Pass secrets through protected deployment configuration or environment
  variables, prefer hidden shell input, and unset them after use.
- Host-ticket arguments must contain opaque secret references, not bearer
  tokens or raw credentials.
- Treat manifest ticket secrets, Wi-Fi PSKs, CAS signing material, evidence
  packs, and raw serial logs as sensitive even when the repository contains
  development fixtures.

Boot-time `cohesix.env` may persist only documented network policy fields. It is
not a general secret store, and writing saved policy is a separate proof state
from successfully using that policy on the current image.

## Audit, evidence, and replay

Security-relevant accepts and denials write bounded audit lines to
`/log/queen.log` and, when enabled by the selected profile, `/audit` records.
Host ticket actions use versioned, allowlisted schemas, idempotency keys, and
explicit lifecycle receipts so operators can distinguish requested, claimed,
running, terminal, and dead-letter states.

`coh evidence pack` records captured and missing paths instead of inventing
data. It hashes audit `ticket` fields and recursively redacts JSON keys
containing token, secret, password, signing-key, or API-key material. Redaction
reduces accidental disclosure; it is not a substitute for reviewing a pack
before sharing it. The exact pack contract and CI/SIEM recipes live in
[Operator recipes](OPERATOR_RECIPES.md#capture-an-evidence-pack).

Replay is limited to retained Cohesix control-plane records. It cannot recreate
external host state, reverse a host side effect, or prove that an omitted event
did not occur. Policy approvals are single-use; replaying a consumed approval
fails deterministically and emits an audit record.

## Sidecars and host actions

Sidecar mounts and providers are manifest-gated. Namespace collisions receive
deterministic hash-prefixed labels; role and path scopes are checked on each
operation. Offline spool and replay are bounded by selected manifest limits and
must not exceed Secure9P `msize`. Sidecars do not add in-VM listeners.

Host actions under `/host/tickets/*` are requests, not implicit target access to
the host. The host ticket agent validates schema, action allowlist, arguments,
idempotency, and state before a configured host adapter performs a side effect.
Use dedicated host identities, least-privilege adapter configuration, and the
request/result/federation contracts in [Interfaces](INTERFACES.md#host-tickets-and-federation).

## Content-addressed storage

CAS updates are file-backed and Queen-writable; they do not create another
network service. Chunks are verified by SHA-256, invalid content is quarantined
and audited, and signature requirements are selected by the manifest. A delta
must identify and validate a non-delta base epoch before it can be accepted.

<!-- coh-rtc:cas-security:start -->
### CAS integrity stance (generated)
- `cas.signing.required`: `true`
- Hash mismatches are rejected, quarantined, and audited without side effects.
- Signature failures emit deterministic ERR plus audit entries.
- `/models` exposure remains gated by `ecosystem.models.enable`.

_Generated by coh-rtc (sha256: `674f8c3ed5412b48f6d8e4804d75735aa6b40237b15fa0be463f06e777132101`)._
<!-- coh-rtc:cas-security:end -->

## Generated observability limits

<!-- coh-rtc:observability-security:start -->
### Observability tolerances (generated)
- `observability.proc_ingest.latency_samples`: `32`
- `observability.proc_ingest.latency_tolerance_ms`: `5`
- `observability.proc_ingest.counter_tolerance`: `1`
- `observability.proc_ingest.watch_min_interval_ms`: `50`

_Generated by coh-rtc (sha256: `aae20e12321a8a009e32d6e163c28d7ab51ca76a211a6ef0f1dd753f88b1c6ce`)._
<!-- coh-rtc:observability-security:end -->

The generated `cohsh` pooling, retry, heartbeat, and trace limits are owned by
[`docs/snippets/cohsh_ticket_policy.md`](snippets/cohsh_ticket_policy.md) and
embedded in [Userland and CLI](USERLAND_AND_CLI.md); they are not a second
security policy.

<!-- metrics:latency:start -->
### Telemetry Ring Latency (generated)
- Suite: `nine-door/telemetry_ring`
- Samples: `7`
- P50: `0.014 ms`
- P95: `0.025 ms`
- Unit: `ms`
_Generated from `apps/nine-door/out/metrics/telemetry_ring_latency.json`._
<!-- metrics:latency:end -->

This generated microbenchmark record is regression evidence for its named
suite, not an end-to-end deployment latency claim. Benchmark methodology and
publishable report requirements live in [Benchmarks](BENCHMARKS.md).
Developers refresh the snippet and this embedded projection together:

```sh
scripts/ci/update_latency_metrics.sh \
  apps/nine-door/out/metrics/telemetry_ring_latency.json \
  docs/snippets/latency_metrics.md docs/SECURITY.md
```

## Profile-qualified security claims

The committed default manifest is not universal deployment policy. Security
claims must identify the selected source manifest, resolved manifest hash,
seL4 output profile, target, commit, and evidence run. Features disabled by that
profile are absent, not protected by an undocumented fallback.

Current target status and proof boundaries are maintained in
[Hardware bring-up](HARDWARE_BRINGUP.md) and the
[Build plan](BUILD_PLAN.md). The NIST 800-53 crosswalk is an evidence index, not
a certification; see [NIST mapping](SECURITY_NIST_800_53.md).
