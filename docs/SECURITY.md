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
and audit. Queen and Worker roles receive only their generated namespace and
endpoint authority; Worker tickets are mandatory and Queen ticket requirements
are profile-controlled.

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

The authenticated root-task TCP console is the only in-VM TCP listener. It is a
line-oriented console using `AUTH`, `ATTACH`, bounded commands, `OK`/`ERR`
responses, and `END` stream terminators; it is not a 9P-over-TCP server. Worker
attachments require a valid role ticket before namespace access.

Authentication is not encryption. Bind direct console forwarding to loopback
or carry it through an authenticated encrypted tunnel. `hive-gateway` also
defaults to loopback, refuses non-loopback binding without explicit opt-in, and
does not terminate TLS. A non-loopback gateway requires an external secure
boundary such as a VPN, authenticated tunnel, or TLS reverse proxy. Use
different secrets for target console authentication and REST request
authentication.

Only one direct owner may hold the target console session. Concurrent clients
must share one `hive-gateway` owner rather than racing `cohsh`, SwarmUI, or
bridges against it.

Console parsing uses fixed-capacity buffers and a shared finite-state command
parser. A leaky-bucket rate limiter allows two failed authentication attempts in
a 60-second window; the next failure enters a 90-second cooldown. Root-task
adds bounded exponential backoff beginning at 250 ms for repeated authentication
failures. Denials and successful role assertions emit audit lines, while
pressure refusals use the bounded `busy`, `quota`, `cut`, and `policy`
categories exposed through `/proc/pressure/*`.

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
multithreading of authority state.

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

The GENETv5 implementation uses Linux device-tree and driver behavior plus
U-Boot bring-up as reference material. The CYW43455/SDIO implementation uses
OpenBSD `bwfm`, Zephyr/Infineon WHD layering, and Linux `brcmfmac` edge cases as
reference material. These are provenance references, not code sources; source
lift is prohibited. CYW43455 and SDIO runtimes are implemented research
surfaces, but current-image association, DHCP, TCP, and repeatability remain
evidence-gated in the build plan.

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
must not exceed Secure9P `msize`. LoRa radio duty-cycle controls reject
over-budget frames and record bounded audit evidence. Sidecars do not add
in-VM listeners.

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

## Profile-qualified security claims

The committed default manifest is not universal deployment policy. Security
claims must identify the selected source manifest, resolved manifest hash,
seL4 output profile, target, commit, and evidence run. Features disabled by that
profile are absent, not protected by an undocumented fallback.

Current target status and proof boundaries are maintained in
[Hardware bring-up](HARDWARE_BRINGUP.md) and the
[Build plan](BUILD_PLAN.md). The NIST 800-53 crosswalk is an evidence index, not
a certification; see [NIST mapping](SECURITY_NIST_800_53.md).
