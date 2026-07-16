<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Explain Cohesix terminology and foundational concepts for readers new to the project. -->
<!-- Author: Lukas Bower -->

# Cohesix Glossary

This glossary explains the public language used throughout Cohesix. It assumes
no prior experience with operating-system design, seL4, Plan 9, 9P, edge AI,
or the Cohesix Queen/Worker model.

Definitions describe the current source tree unless an entry is explicitly
labelled **planned**, **historical**, **host-only**, or **simulation-only**.
Exact profile-selected values, paths, bounds, and enabled features come from
the selected source manifest, its resolved manifest, and generated outputs.
Those artifacts remain authoritative when a concise definition here omits a
detail. See [Architecture](ARCHITECTURE.md), [External Interfaces](INTERFACES.md),
[Secure9P](SECURE9P.md), [Roles and Scheduling](ROLES_AND_SCHEDULING.md), and
[HAL and Physical Drivers](DRIVERS.md) for the owning contracts.

## Start with this mental model

1. AI models, agents, GPU software, and fleet integrations run on the **host**.
2. A small Cohesix **target** decides which control requests are allowed.
3. The **Queen** is the hive-wide orchestration authority exposed by the
   target's root task; narrowly authorized **Workers** handle specific roles.
4. Control and state appear as bounded paths in a role-scoped **namespace**.
   Host NineDoor serves that model through **Secure9P**; a separate target
   **NineDoorBridge** projects it through the authenticated console.
5. Tickets, capabilities, policy, lifecycle, and hard bounds limit authority.
   Logs, telemetry, receipts, and evidence packs make outcomes reviewable.

This is why Cohesix calls itself a high-assurance control-plane research OS:
it is the compact trust layer around AI-operated infrastructure, not the place
where models train or GPU kernels execute.

## Naming and capitalization

- **Queen** and **Worker** name Cohesix roles; `queen` is also their command-line
  role label.
- Use **LoRA** in prose. Lowercase `lora` in paths, file names, configuration
  keys, and source identifiers is the ASCII form of that same AI term.
- **NineDoor** is the host 9P server. **NineDoorBridge** is the target namespace
  adapter. Neither name is shorthand for the other.
- **Host**, **target**, **QEMU**, **mock**, and **hardware** describe different
  execution or evidence boundaries; success in one does not prove another.
- **ACK** means an acknowledgement as a category. The literal success token in
  the current console grammar is `OK`.

## Alphabetical index

[0–9](#0-9) · [A](#a) · [B](#b) · [C](#c) · [D](#d) · [E](#e) ·
[F](#f) · [G](#g) · [H](#h) · [I](#i) · [J](#j) · [K](#k) ·
[L](#l) · [M](#m) · [N](#n) · [O](#o) · [P](#p) · [Q](#q) ·
[R](#r) · [S](#s) · [T](#t) · [U](#u) · [V](#v) · [W](#w) ·
[X](#x)

## 0–9

### 9door

A historical shorthand still found in some planning text. Use **NineDoor** for
the current host server and **NineDoorBridge** for the separate target adapter.

### 9P

A small file-access protocol created for the Plan 9 operating system. Instead
of inventing a different RPC API for every service, a client walks named paths,
opens nodes, and reads or writes bounded bytes. Cohesix borrows this file-shaped
control model; it does not run Plan 9.

### 9P2000.L

The only 9P dialect accepted by Secure9P. `.L` identifies the Linux-oriented
9P2000 extension family, but Cohesix implements only the bounded request subset
documented in [Secure9P](SECURE9P.md), not a Linux or POSIX environment.

## A

### AArch64 / ARM64

The 64-bit ARM instruction-set architecture used by Cohesix targets. QEMU uses
an `aarch64/virt` machine profile; Raspberry Pi 4 uses the BCM2711 platform.

### Acceptance evidence

Evidence strong enough to satisfy a stated gate for a stated image, profile,
target, and test. A successful build is build evidence, not boot evidence; a
boot prompt is not device evidence; QEMU success is not Pi hardware evidence.
See [Hardware Bring-up](HARDWARE_BRINGUP.md#proof-layers).

### ACK / acknowledgement

A generic name for a response that acknowledges a command. Cohesix source and
tests may call response records or parsers `ack`, but the current console emits
the literal success token `OK`, refusal token `ERR`, and stream terminator
`END`. Do not send or expect a literal `ACK` success line.

### Actions queue

`/actions/queue` is a manifest-gated control path for single-use policy
decisions. An entry identifies an action, its target, and an `approve` or
`deny` decision; status is read separately under `/actions/<id>/status`.

### Active / standby

A high-availability arrangement in which one hive is the current writer and a
second hive is prepared for host-orchestrated takeover. Cohesix does not treat
active/active multi-Queen writes to one logical hive as safe. See
[Failover](FAILOVER.md).

### Agent Action Airlock

A use-case name for putting an AI agent's proposed action through explicit
ticket, policy, lifecycle, and evidence checks before execution. It is a
deployment pattern, not a separate Cohesix daemon or protocol.

### AGI / “infrastructure for AGI”

AGI means artificial general intelligence. Cohesix uses “infrastructure for
AGI” as project positioning: increasingly autonomous AI needs a trustworthy,
auditable control layer around real systems. It is not a claim that Cohesix
contains an AGI model, trains one, or proves one safe.

### AI agent

Software that observes context, proposes decisions, and may call tools. In a
Cohesix deployment, an agent stays host-side and submits intent through an
approved host surface; it receives no ambient target, host, or GPU authority.

### AI Harness

A host-side integration pattern in which an AI application uses Cohesix paths,
tickets, policy, and receipts to propose and observe actions. It is not a
second target control plane and does not change Secure9P or console authority.

### Allowlist

An explicit set of permitted roles, paths, verbs, providers, peers, or actions.
Anything outside the relevant allowlist is refused; an allowlist does not
override capability, ticket, lifecycle, size, or profile checks.

### Append-only

A file mode in which new records may be added but existing retained records
cannot be overwritten. The provider still validates the expected offset,
payload, role, ticket, and bounds; “append-only” does not mean “accept any
write.”

### Approval

A single-use policy decision submitted through the actions queue. Acceptance
of an approval record proves only that the decision was recorded, not that the
controlled action completed.

### As-built

Implemented behavior supported by current source, selected generated artifacts,
tests, and—when a runtime or hardware claim is made—matching evidence. It is
different from a design intention, planned milestone, historical result, or
simulation.

### Attach / `ATTACH` / `Tattach`

Attaching selects a role-scoped namespace identity. `ATTACH` is the target
console command used after transport authentication; `Tattach` is the
Secure9P request used with host NineDoor. They serve related authority
semantics but belong to different wire protocols.

### Audit / AuditFS

The manifest-gated `/audit/*` namespace containing bounded journals, policy
decisions, and export metadata. Audit records provide evidence about accepted
or denied control activity; they do not grant authority or provide permanent,
unbounded storage.

### Auth token

A secret used to enter the target TCP console. It proves access to the
transport, not a Queen or Worker namespace role. Application authority is
selected later by `ATTACH` and, where required, a capability ticket.

### Authority

Permission to perform a specific operation on a specific object or path.
Cohesix composes kernel capabilities, generated role views, tickets, policy,
lifecycle state, and bounds instead of assuming that a connected process has
ambient administrator power.

### Authority epoch

A generation number reserved for a future executable Worker's endpoint
authority. In such a profile, revocation or replacement can make an older
badged capability stale. Current profiles carry only reserved epoch metadata;
they install no Worker endpoint capability.

## B

### Backpressure

A bounded system's deliberate refusal, delay, or shedding of nonessential work
when a queue, ring, tag window, or consumer cannot keep up. Backpressure is an
expected safety mechanism, not automatically a crash or data-plane failure.

### Badge

A small value seL4 can attach to an endpoint or notification capability.
Cohesix uses live badges on implemented root/driver paths and reserves generated
Worker badge ranges for a future executable profile. A reserved value does not
identify a live sender or grant authority until the corresponding capability is
minted, delivered, and invoked.

### Batch frame

One complete Secure9P request inside a transport batch. Batch byte size, frame
count, tag use, and ordering are bounded by the selected profile; the checked-in
default may allow only one frame even though batching machinery exists.

### BCM2711

The Broadcom system-on-chip in Raspberry Pi 4. It defines the Pi target's CPU,
interrupt, timer, peripheral, and memory environment; QEMU `aarch64/virt`
evidence does not prove BCM2711 behavior.

### Bounded

Having an explicit maximum: for example bytes, records, retries, queue entries,
walk components, service work, or retained history. Bounds make overload and
malformed input fail predictably and keep resource use auditable.

### Breadcrumb

A small status or progress record retained for diagnosis. A breadcrumb reports
what a component observed or attempted; it is not by itself proof that a host
action, device operation, or model activation completed.

### Bridge

A host or target adapter that projects an existing contract across a boundary.
Examples include `gpu-bridge-host`, `host-sidecar-bridge`, and
`NineDoorBridge`. “Bridge” never implies a new authority path.

### Budget

Finite authority encoded in a ticket or scheduling record. Ticket budgets can
limit operation count (`ops`), service allocation (`ticks`), and lifetime
(`ttl_s`); leases have their own lifetime and resource limits.

### Build host

The computer that compiles, packages, runs host tools, or flashes media. The
primary development host is macOS 26 on Apple Silicon. Host success does not
prove code executed on the Cohesix target.

### Build marker

A version, hash, banner, or deliberately changed output shape used to associate
a live boot with a specific newly built image. It helps reject stale-boot
evidence after rebuilding or flashing.

## C

### Capability

An unforgeable kernel-managed permission to use a particular seL4 object in
particular ways. Possessing a capability is necessary for kernel access; it
does not automatically satisfy Cohesix role, ticket, policy, or lifecycle
checks at the namespace layer.

### Capability space / CSpace

A task-local table of seL4 capabilities. Separate CSpaces help ensure that a
Worker or driver runtime receives only the endpoints, notifications, frames,
and device resources selected for it.

### CAS / content-addressed storage

Storage addressed by a cryptographic digest of the content rather than an
arbitrary mutable name. Cohesix's manifest-gated `/updates/*` and `/models/*`
surfaces accept bounded manifests and chunks; a hash match identifies bytes but
does not grant deployment authority.

### CBOR

Concise Binary Object Representation, a compact structured-data encoding.
Some generated telemetry and observability nodes have CBOR forms. CBOR is an
optional payload format inside a documented node, not the native Secure9P wire
protocol and not a new control channel.

### Clunk / `Tclunk`

The 9P operation that releases a fid. Secure9P retires that fid for the rest of
the session; reusing it after clunk is a deterministic error.

### `coh`

The host integration CLI for diagnostics, namespace mounts, GPU lease records,
PEFT workflows, host-command breadcrumbs, telemetry export, fleet views, and
evidence packs. Its actions remain projections of documented host or target
interfaces. See [Host Tools](HOST_TOOLS.md#coh).

### `.coh` script

A bounded text script interpreted by `cohsh`. Its grammar includes commands,
assertions, variables, and timing controls defined in
[Userland and CLI](USERLAND_AND_CLI.md); it is not a shell script and does not
inherit arbitrary POSIX shell syntax.

### Cohesix

An open-source, high-assurance control-plane **research operating system** for
edge AI. It uses seL4 isolation, a Queen/Worker hive model, bounded file-shaped
interfaces, and auditable evidence while keeping models, GPU stacks, and heavy
integrations on the host.

### `coh-rtc`

The Cohesix root-task compiler. It validates a source manifest, resolves
profile-selected behavior, and generates Rust tables, resolved manifests,
scripts, policy defaults, and documentation snippets. Generated files are
regenerated through `coh-rtc`, never edited by hand.

### `cohsh`

The Cohesix operator shell. It can connect through direct TCP, REST, QEMU, or a
mock backend, attach as a role, issue the documented console grammar, run
`.coh` scripts, and render bounded replies. See
[Userland and CLI](USERLAND_AND_CLI.md).

### `cohsh-core`

The shared Rust library for command parsing, wire responses, trace records, and
transport-faithful behavior used by host clients such as `cohsh` and SwarmUI.
Keeping semantics here prevents a UI or adapter from inventing new verbs.

### Console

The target's operator command surface. Serial carries bounded lines directly;
TCP carries the same grammar in four-byte length-prefixed frames after
transport authentication. The console is not 9P-on-the-wire and is the only
permitted in-target TCP listener.

### Console owner / sole console owner

The one process holding the target's single TCP console session. In direct
mode it is one foreground tool or bridge; in gateway mode it is
`hive-gateway`. Two direct owners must not compete for the connection.

### Control file

A namespace node whose validated write requests a state transition or action,
such as `/queen/ctl` or `/policy/ctl`. Writing a control file records accepted
intent; completion must be established through the owning status, receipt, or
evidence surface.

### Control plane

The low-volume authority and coordination path: commands, policy, tickets,
leases, lifecycle, status, and evidence. Cohesix deliberately keeps bulk model
artifacts, training data, GPU memory, and high-rate application traffic in the
host-side data plane.

### CPIO / root filesystem archive

CPIO is the archive format used to package target userspace images and related
assets. Cohesix calls the archive the rootfs, but it is not a writable
general-purpose POSIX root filesystem; target artifacts remain bounded and
`no_std`.

### Current-image proof

Evidence tied to the exact image running now, normally through a fresh boot
marker plus boot-paired logs or captures. Historical success, a newly produced
artifact, or a successful flash does not substitute for current-image proof.

### Cursor

A bounded position in retained records, telemetry, audit, or host-ticket
processing. Cursors support resumption without requiring unbounded history;
the selected contract defines valid ranges and what happens after wrap or
expiry.

### Cutover

The host-orchestrated switch from an active hive to its standby, together with
the required fencing and operator-path update. Cutover is not target-side
multi-Queen consensus.

### CUDA

NVIDIA's host GPU programming and runtime platform. CUDA never enters the
Cohesix VM or trusted computing base; a deployment-specific host executor owns
real GPU work.

### CYW43 / CYW43455

The Broadcom/Cypress Wi-Fi device family used by Raspberry Pi 4. Cohesix splits
its Wi-Fi logic from the SDIO host controller into separate isolated runtimes;
association, DHCP, raw TCP, and repeated clean boots are separate proof gates.

## D

### Data plane

The high-volume path where models, datasets, GPU kernels, media, and application
traffic move or execute. Cohesix governs this work through bounded control
records but intentionally does not pull the data plane into the target.

### Deadletter

`/host/tickets/deadletter` stores terminally failed or expired host-ticket
receipts for operator review. A deadletter is evidence of a final failure state,
not a retry queue with implicit authority.

### Deterministic

Producing behavior governed by explicit inputs, ordering rules, and bounds so a
result can be tested or replayed consistently. Cohesix does not use the word to
claim that every external device, network, host process, or wall-clock outcome
is perfectly reproducible.

### Device tree / DTB

A firmware-provided description of hardware addresses, interrupts, clocks, and
boot properties. On Pi 4 it carries bounded Cohesix boot policy through selected
`/chosen/cohesix,*` properties; the selected manifest still governs compiled
authority.

### Device untyped

An seL4 untyped-memory capability representing a physical device range rather
than ordinary RAM. HAL alone may discover, retype, and map admitted device
untypeds; Workers and driver runtimes do not scan physical address space.

### Direct mode

A topology in which exactly one host tool owns the target TCP console. Use
gateway mode when multiple host clients must operate concurrently. See
[Host Tools](HOST_TOOLS.md#choose-one-live-topology).

### DMA

Direct memory access: hardware reading or writing memory without the CPU copying
each byte. HAL owns DMA allocation and publication; drivers must use declared
buffers, explicit cache transitions, and the selected protection profile.

### DPC

Deferred procedure call, Cohesix shorthand for bounded work scheduled after a
device interrupt or wake event. It keeps interrupt handling short while making
deferred work, re-signalling, drops, and budget exhaustion observable.

### Driver-task ABI

The fixed application binary interface between root-task and an isolated
physical-driver runtime. It consists of generated runtime-init resources,
pointer-free bounded command/completion records, shared rings, and declared
endpoints or notifications. It is not a device-discovery API.

### Driver runtime / isolated driver runtime

A separate seL4 task that owns one manifest-declared physical-device role, such
as serial, HDMI, USB, PCIe, GENET, SDIO, or CYW43. Root-task admits its resources
through HAL and submits bounded service turns; the runtime has no ambient access
to other devices or root memory.

## E

### Edge GPU node / fleet

A computer with GPU capability deployed near the workload or data source rather
than only in a central cloud. Cohesix coordinates and audits these nodes; GPU
drivers, CUDA/NVML, models, training, and inference stay in each host OS.

### ELF / elfloader

ELF is the executable file format used for seL4, root-task, and driver images,
and for any future profile-selected executable Worker image. The seL4 elfloader
is the small boot component that receives the
platform handoff, loads the kernel and initial userspace, and transfers control.

### `END`

The terminal line for a bounded console stream or listing. A leading `OK` may
acknowledge the command, zero or more records may follow, and `END` marks the
stream boundary. Not every successful non-streaming command emits `END`.

### Epoch

A monotonic generation or namespace label. Its exact meaning depends on the
contract: CAS uses `/updates/<epoch>` for a bundle generation. A future
executable Worker's authority can use an epoch to reject stale invocation
capabilities; current Worker-role profiles retain only reserved metadata.

### Error surface

The stable way an interface reports refusal. The console uses textual `ERR`
reasons; Secure9P uses bounded `Rerror` codes such as `Permission`, `NotFound`,
`Busy`, `Invalid`, `TooBig`, and `Closed`. Similar meanings do not imply a
one-to-one wire mapping.

### Evidence

Source-backed information used to support a specific claim: generated hashes,
test output, serial logs, packet captures, status records, receipts, or
benchmarks. Evidence is meaningful only with its image, profile, target, time,
and acceptance gate.

### Evidence pack

A bounded directory exported by `coh evidence pack` containing metadata,
limits, summaries, and selected namespace snapshots. It supports review and
CI/SIEM ingestion; it must be handled under the deployment's data-classification
and redaction policy.

### Evidence plane

The paths, logs, counters, receipts, and exported artifacts used to explain what
the authority plane accepted, refused, or observed. Evidence informs review but
does not itself authorize a new action.

### Evidence timeline

An offline NDJSON and Markdown correlation built from an evidence pack by
`coh evidence timeline`. It orders retained events for investigation without
creating a live control connection.

### Export window

A bounded period opened or closed through `/queen/export/ctl` during which
selected records may be exported. It limits evidence exposure and retention; it
does not expand role access to unrelated paths.

## F

### Feature gate

A manifest field that enables or disables a component, namespace family,
provider, or interface. A path documented as manifest-gated is not guaranteed
to exist in every profile.

### Fid

A numeric 9P file identifier scoped to one session. A client obtains or walks
fids to represent paths; active and retired fids cannot be reused contrary to
the Secure9P session rules.

### Field bus / sidecar

A host-facing integration for systems such as MODBUS or DNP3. Cohesix projects
bounded control, telemetry, link, and spool paths while the heavy protocol
stack and physical access remain outside the target authority boundary selected
by the profile.

### Firmware

Low-level software loaded before or into hardware, such as Raspberry Pi boot
firmware or CYW43 Wi-Fi firmware. Firmware provenance and upload success are
separate from driver ownership, network association, or application readiness.

### Flash proof

Evidence that a specific image was written to the intended removable medium and
optionally read back correctly. It is not evidence that the board booted or
executed that image.

### Fencing

Controls that prevent two hosts or hives from mutating the same logical
control plane during failover. Fencing preserves single-writer behavior before
and during cutover.

### Formal verification

Mathematical proof, checked by machines, that a model or implementation meets
specified properties under stated assumptions. seL4 has formal proofs for its
kernel configurations and assumptions; this does not make all Cohesix code,
drivers, host tools, firmware, or deployments formally verified.

### FUSE / mount

FUSE lets a userspace program present a filesystem view to the host OS.
`coh mount` exposes permitted Cohesix namespace paths through FUSE; it remains
a host projection of Secure9P/console semantics, not a POSIX filesystem inside
the target.

## G

### Gateway mode

A topology in which `hive-gateway` is the sole target TCP console owner and
multiple host clients use its bounded REST projection. Gateway clients share
the gateway's upstream attached role and optional ticket.

### Gateway request-auth token

A host-side secret required for mutating REST requests to `hive-gateway`. It
protects the HTTP write edge but does not create a delegated target identity or
replace console authentication, attach, ticket, policy, and lifecycle checks.

### GENET / bcmgenet-v5

The Raspberry Pi 4 built-in Ethernet MAC. Cohesix runs GENET as an isolated
driver runtime with bounded RX/TX descriptor rings; link, DHCP, TCP console,
repeatability, and performance remain separate evidence gates.

### Generated artifact / generated truth

Output produced from validated compiler IR by `coh-rtc`, including Rust tables,
resolved manifests, snippets, policy defaults, and scripts. Generated output
defines selected behavior and must not be hand-edited.

### GICv3

Arm's Generic Interrupt Controller version 3, used by the QEMU
`aarch64/virt` target profile. It routes interrupt authority to seL4 tasks;
its behavior is not Pi 4 interrupt-controller proof.

### GPU bridge / `gpu-bridge-host`

The host tool that discovers local GPU inventory and publishes a bounded
`/gpu` snapshot through an authorized console or REST path. It does not run
kernels, enforce hardware leases, train models, or reload inference services.

### GPU lease

A time-bounded control record claiming GPU capacity for a Worker or workload.
An accepted or `ACTIVE` record expresses control-plane state; a host executor
must separately enforce memory, stream, lifetime, revocation, and device policy.

### GPU node

Either a host GPU represented under `/gpu/<id>` or, more broadly, an edge
computer containing GPUs. The target stores bounded inventory, lease, status,
and model-pointer records; it does not own the GPU device.

### GPU Worker / `worker-gpu`

The recognized Worker session/model role scoped to its telemetry and generated
GPU lease view. Current profiles do not launch it as a target task. It has no
GPU MMIO, device node, CUDA, or NVML access and does not automatically reload a
host model runtime.

## H

### HAL

The hardware abstraction layer and mandatory admission boundary for physical
resources. HAL alone discovers device authority, validates manifest records,
retypes untypeds, maps MMIO, allocates/publishes DMA, binds IRQs, and starts
physical-driver runtimes.

### Hardware proof

Evidence collected from the actual named hardware while the identified image is
running. QEMU, mock, compiler output, packaging, and historical captures can
support development but cannot substitute for current target-qualified
hardware proof.

### High assurance

Engineering for strong, reviewable confidence through small trusted components,
explicit authority, isolation, validation, deterministic bounds, fail-closed
behavior, and evidence. It is not shorthand for “bug-free” or “the entire
system is formally verified.”

### Hive

One Cohesix authority domain: a Queen/root-task control plane, its
profile-declared Workers and driver runtimes, and its role-scoped namespace.
A fleet can contain multiple independent hives.

### `hive-gateway`

The host-only REST multiplexer that owns one target console session and
schedules bounded control and telemetry requests from multiple clients. It is
a projection, not a second in-target service or authority source.

### Host

The conventional operating system outside the Cohesix target. It runs models,
agents, CUDA/NVML, storage, networking integrations, REST, UI, bridges, and
deployment-specific executors. “Host” can mean the build/operator machine or
the host OS surrounding a deployed target; documentation should qualify it
when that distinction matters.

### Host provider / `host-sidecar-bridge`

A host collector that projects bounded systemd, Kubernetes, Docker, NVIDIA,
Jetson, or network state into `/host/*`. A projection is evidence or a ticketed
control sink; it does not put those host APIs inside the VM.

### Host ticket

A strict JSONL request under `/host/tickets/spec` asking an authorized host
agent to perform an allowlisted action. It is different from a role capability
ticket: a host ticket is work intent with lifecycle receipts, not a credential
used to attach to a namespace.

### Host-ticket agent

The host executor that claims enabled host-ticket requests, validates action
and lifecycle policy, performs supported host operations, and appends status or
deadletter receipts. Federation remains host-side and manifest-bounded.

### Hold-down timer

A failover cooldown after cutover that prevents rapid switching between active
and standby during unstable health signals.

## I

### Idempotency key

A stable identifier allowing repeated delivery of the same intended request to
be recognized without executing it twice. Federation extends correlation with
source and target hive identifiers; idempotency correlates work but grants no
additional authority.

### Inference / training

Inference runs a trained model to produce an output; training changes model
parameters from data. Both remain host-side in Cohesix. The target coordinates
bounded intent, leases, state, and evidence rather than executing either
workload.

### Integration pattern

A documented way to compose implemented Cohesix primitives for a deployment.
It is not automatically a packaged product, accepted hardware configuration,
or claim that every external adapter is implemented.

### IPC

Inter-process communication. seL4 endpoints and notifications provide explicit
IPC between isolated target tasks; Cohesix also uses HAL-admitted shared pages
and bounded rings where the generated ABI declares them.

### IR

Intermediate representation: the validated structured model consumed by
`coh-rtc` before code generation. New manifest-controlled behavior belongs in
IR so generated code, defaults, tests, and documentation can agree.

### IRQ

Interrupt request: a hardware signal that a device needs service. HAL binds
declared IRQ authority; a driver runtime acknowledges the source, performs
bounded work, and may schedule DPC work rather than monopolizing the event loop.

### Isolated task / runtime

A target process with its own VSpace and CSpace. Isolation means it has only
delegated capabilities and mapped pages; it is stronger than placing modules in
separate source directories but does not by itself prove safe device DMA.

## J

### JSON

JavaScript Object Notation, used for strict structured control and status
records in selected paths. Cohesix validates required fields, rejects unknown
fields where the schema says so, and applies independent byte bounds.

### JSONL / NDJSON

Newline-delimited JSON: one complete JSON object per line. Cohesix uses it for
append-oriented records such as host tickets, policy, audit, and some telemetry.
It is a payload convention within a path, not the native 9P wire format.

## K

### Kernel

The privileged core that controls execution, memory, capabilities, interrupts,
and IPC. In Cohesix the kernel is upstream seL4; the root task, Workers, drivers,
NineDoor, and host tools are outside the kernel.

### Kernel-derived truth

Generated headers, slot layouts, configuration, timer settings, and metadata
from the selected seL4 build directory. Target code must match those artifacts;
a different or stale seL4 build cannot be assumed compatible.

### Kubernetes coexistence intent

A host-ticket action such as cordon, drain, or lease synchronization. Cohesix
records and constrains the intent; the host-ticket agent and Kubernetes API
remain host-side, and their receipt is distinct from target acceptance.

## L

### Lease

A time-bounded allocation or claim on a resource. Queen lifecycle files can
record general leases, and GPU nodes record GPU leases; each contract owns its
fields and enforcement boundary. WorkerLora does not introduce a separate LoRA
lease or lease namespace.

### Lifecycle gate

A check against current state before an operation is accepted. Attaching,
publishing, telemetry, host-ticket execution, lease changes, and other actions
may be refused even when the role and path are otherwise valid.

### Live Hive

SwarmUI's graphical, host-side rendering of agents, work, flow, status, and
replay derived from existing telemetry and events. It is a presentation view,
not an authoritative scheduler, protocol, or source of target state.

### Local seat

The physical operator interface attached to a target, currently a USB keyboard
with HDMI feedback where present. Serial remains the emergency and first
operator path; local-seat readiness requires its own current-image evidence.

### LoRA

**LoRA (low-rank adaptation)** is a parameter-efficient technique for adapting
an AI model without retraining all of its weights. In Cohesix, `worker-lora` is
a ticket-scoped, receipt-only control-plane Worker for LoRA adapter and model
lifecycle coordination. Lowercase `lora` in paths, files such as `lora.json`,
configuration keys, and source identifiers is the ASCII form of this AI term.
Training, evaluation, artifact import, inference, and model reload remain
host-side.

## M

### MAC

Two unrelated meanings occur in Cohesix. A **message authentication code**
protects capability tickets against forgery; a network **media access control**
address identifies an Ethernet or Wi-Fi interface. Context must make the
meaning explicit.

### Manifest / source manifest

A human-authored TOML configuration, such as `configs/root_task.toml`, that
declares a target profile, roles, mounts, bounds, feature gates, drivers, and
policy. It is compiler input, not generated output and not proof that selected
hardware ran successfully.

### Manifest fingerprint

A deterministic hash identifying selected manifest truth. Fingerprints tie
generated artifacts and evidence to a configuration; they do not replace the
source revision, seL4 build identity, target image hash, or live proof.

### MCS

Mixed-Criticality Systems, an seL4 kernel configuration with scheduling
contexts, budgets, and periods for controlled CPU-time allocation. Cohesix
profiles and generated records must match the selected kernel's MCS behavior.

### Microkernel

An operating-system kernel designed to keep privileged mechanisms small, moving
most services and drivers into isolated userspace tasks. Cohesix uses seL4 so
device drivers and control services do not need to share one large privileged
kernel address space.

### MMIO

Memory-mapped input/output: device registers accessed at physical addresses as
if they were memory. HAL maps only declared MMIO pages into the owning driver
runtime; arbitrary physical-address access is prohibited.

### Mock / mock mode

A local substitute used for deterministic development without a live target.
Rust tools commonly use process-local in-memory state; Python `MockBackend` can
use a shared filesystem root. Neither is QEMU, a live VM, or hardware evidence,
and two separate in-memory processes do not share state.

### Model registry

Host storage and metadata for model artifacts. `/gpu/models/*` can expose
host-published descriptors and an active-model pointer; top-level
`/models/*` is a separate manifest-gated content-addressed namespace. A pointer
change is not proof that an inference runtime reloaded.

### Model runtime

The host software that loads and executes a model for inference or training.
Cohesix may constrain and observe its surrounding workflow, but the runtime and
model weights do not enter the target.

### MLOps

The operational discipline around building, deploying, observing, governing,
and updating machine-learning systems. Cohesix contributes a small,
high-assurance authority and evidence layer to MLOps; it is not an end-to-end
training, registry, serving, or data platform.

### Mount

A host filesystem view produced by `coh mount` through FUSE. A mount exposes
only the attached role's permitted namespace and remains bound by the underlying
transport, ticket, policy, and provider modes.

### `msize`

The maximum negotiated 9P message size, including framing. Secure9P accepts a
profile-selected value no greater than 8192 bytes; clients should discover the
negotiated value rather than assume a snapshot.

### Multi-hive federation

Host-side relay of allowlisted host-ticket intents between independent hives.
Each relay revalidates peers and actions, preserves single-writer authority per
hive, and records correlation; it is not target-side consensus or shared root
authority.

### Mutating REST route

A `hive-gateway` endpoint that can change projected target state, such as an
ECHO operation. It requires gateway request authentication and still inherits
the gateway's upstream role, ticket, and target-side checks.

## N

### Namespace

A role-scoped tree of named paths representing control files, status, telemetry,
policy, and evidence. Different roles can see different views of the same
provider set; a path name alone does not grant permission.

### NineDoor

The complete **host-side** userspace 9P server. It implements Secure9P session
state, ticket verification, role policy, providers, batching, metrics, and
error mapping for host builds, tests, and supported host endpoints. It is not
the target TCP listener.

### NineDoorBridge

The target root-task's separate `no_std` namespace/control adapter. It is
reached through the serial or authenticated TCP console event pump, preserves
overlapping namespace semantics, and does **not** decode 9P frames or expose an
in-VM 9P-over-TCP listener.

### `no_std`

A Rust build mode that does not depend on the standard library. Cohesix VM
artifacts use `no_std` to avoid importing an OS/POSIX runtime; host tools may
use Rust's standard library and host OS services.

### Notification

An seL4 signalling object used for asynchronous events. Cohesix uses generated
notification capabilities and badges for lifecycle, pressure, IRQ, and driver
events without granting unrelated endpoint authority.

### NVML

NVIDIA Management Library, a host API for GPU inventory and telemetry.
`gpu-bridge-host` may use it through a compiled backend; NVML never enters the
Cohesix target or TCB.

## O

### Observability

Read-only or append-only information that explains state and resource pressure:
for example `/proc` nodes, logs, telemetry, status, and audit records.
Observability reports behavior; it must not become an undocumented control
surface.

### Operator / operator host / operator surface

The human or automation controlling a hive; the machine running its tools; and
the interface used to act, respectively. Operator surfaces include serial,
local seat, direct `cohsh`, a mounted namespace, gateway REST, and SwarmUI,
each with distinct transport and authority conditions.

### Owner-state proof

Evidence that the expected isolated driver runtime accepted its sealed
descriptor, owns the declared hot path, and made useful bounded service
progress without a root-owned steady-state fallback. Merely packaging or
starting a child image is insufficient.

### `OK` / `ERR` / `END`

The current target console response family. `OK <VERB>` means the command was
accepted at that interface, `ERR <VERB> ...` reports a bounded refusal, and
`END` terminates streams that use it. An `OK` is not automatically proof that
an asynchronous host, GPU, device, or lifecycle outcome completed.

## P

### PEFT

Parameter-efficient fine-tuning: techniques that adapt a model by training a
small set of parameters rather than all weights. LoRA is one PEFT method.
Cohesix host tools coordinate adapter metadata, activation, rollback, and
receipts; they do not perform training in the VM.

### Plan 9

A Bell Labs research operating system built around per-process namespaces and
the idea that services and devices can be accessed through file-like paths.
Cohesix adopts that conceptual simplicity for control and evidence, but it is
not Plan 9 and does not provide a Plan 9 or POSIX userland.

### Planned

Authorized future work recorded in the build plan but not yet implemented or
accepted. A planned role, target, path, or feature must not be described as
as-built until source, generated artifacts, tests, and required evidence agree.

### Policy gate / PolicyFS

The manifest-gated `/policy/*` and `/actions/*` mechanisms that validate
sensitive control intent against generated rules and optional single-use
approvals. Policy augments rather than replaces capabilities, tickets,
lifecycle checks, provider validation, and bounds.

### Policy preflight

Read-only text or CBOR projections showing queued and consumed approvals and
their relation to rules before an action is attempted. Preflight helps explain
a likely decision but does not reserve success or grant authority.

### Pollen

SwarmUI's visual metaphor for short-lived work or message particles moving
between agents in Live Hive. Pollen is derived presentation state, not a
protocol record, scheduler queue, or unit of authority.

### POSIX

The Unix-style operating-system interface expected by conventional processes
and filesystems. Cohesix VM artifacts do not provide a POSIX façade or libc
emulation layer; heavy POSIX-dependent software stays on the host.

### Pressure

Bounded indicators showing that a queue, ring, ingest path, policy surface, or
broker is approaching or at capacity. Pressure can trigger backpressure and
degraded nonessential output while preserving control liveness.

### Profile

A selected system configuration for a target and purpose, including platform,
kernel contract, roles, drivers, mounts, limits, and gates. Default, QEMU, and
Pi 4 profiles are not interchangeable; commands and evidence must identify the
profile used.

### Proof / proof bundle / proof layers

A proof is evidence sufficient for one stated claim. A proof bundle preserves
the inputs and outputs needed to review it. Proof layers keep build, flash,
readback, boot, saved policy, device/network, console, Test Plan, and benchmark
claims separate rather than promoting a lower layer.

### Provider

A component that owns the behavior and storage contract for a namespace
subtree. Host NineDoor providers, target adapters, and host bridge projections
may expose similar paths, but each operates only within its documented
authority boundary.

### `/proc`

The conventional namespace root for generated, bounded observability nodes.
It is inspired by pseudo-filesystems but is not Linux procfs and exposes only
the profile-enabled Cohesix records documented by generated snippets.

## Q

### QEMU

A machine emulator used as Cohesix's reference `aarch64/virt` development and
regression target. QEMU can prove target software and interface behavior for
its profile; it cannot prove Pi firmware, BCM2711, GENET, SDIO, CYW43, USB,
HDMI, or physical timing.

### Queen

The hive-wide orchestration role exposed through root-task and enabled
namespace providers. Queen is not a separate Worker image or an AI model; it
is the authority context that manages permitted worker, lifecycle, lease,
policy, and evidence operations. A Queen ticket is optional where the selected
attach path permits it.

### Queue / quota

A queue is bounded retained or pending work; a quota is a limit assigned to a
role, ticket, session, cursor, provider, or client. Saturation or exhaustion
must produce explicit backpressure or refusal, not unbounded growth.

## R

### Receipt

A bounded status record reporting the lifecycle or result of previously
accepted intent. Host-ticket status, model/PEFT operations, and deployment
executors use receipts; the exact owning schema determines whether a receipt
proves acceptance, execution, or observation.

### Relay / relay correlation

The host-side forwarding of an allowlisted host ticket from a source hive to a
target hive. `relay_hop` bounds forwarding depth and
`relay_correlation_id` connects spec, status, and evidence records; neither
field grants authority.

### Replay

A family of deliberately different operations:

- `/replay/ctl` validates a retained Cohesix audit acknowledgement sequence and
  publishes cursor, count, and fingerprint status; it does not re-execute the
  original actions.
- `cohsh` mock trace replay feeds recorded frames and replies into tests.
- SwarmUI replay reconstructs a visual timeline offline.
- Sidecar spool replay drains retained bus telemetry after a link returns.

Documentation should name the replay type rather than imply one universal
re-execution facility.

### ReplayFS

The manifest-gated `/replay/*` audit-validation surface. It accepts a bounded
starting cursor and reports `idle`, `ok`, or `err` plus deterministic sequence
metadata; it creates no alternate authority path.

### Resolved manifest

The generated, normalized profile output produced by `coh-rtc` after validation
and default resolution. It is distinct from the human-authored source manifest
and is authoritative for the selected generated behavior represented in that
build.

### REST projection

The host HTTP view provided by `hive-gateway` over existing console/namespace
operations. REST can multiplex and format requests, but it cannot invent a
target path, role, verb, or authority absent from the underlying contract.

### Role / role view

A named authority class such as Queen or WorkerHeartbeat. Attaching as a role
selects a filtered namespace view; the role label alone does not bypass ticket,
subject, scope, lifecycle, operation, or provider checks.

### Role ticket / capability ticket

A MAC-protected credential binding a role to claims such as subject, mount,
path/verb/rate scopes, operation and tick budgets, lifetime, bandwidth, and
cursor quotas. Worker roles require one; Queen may attach without one on
permitted paths. Tickets and their secrets must never appear in logs or docs.

### Root task / root-task

The initial and most authoritative userspace task started by seL4. It creates
tasks and seL4 objects, validates generated descriptors, installs capabilities,
admits hardware through HAL, runs the target console and NineDoorBridge, and
schedules bounded service work. It must not own steady-state physical drivers
assigned to isolated runtimes.

### Rootfs

The bounded target userspace archive packaged with the image. “Rootfs” is a
build artifact name; it does not imply a Linux distribution, writable disk
root, libc, or POSIX environment.

### Runtime-init descriptor

A versioned, sealed, pointer-free record prepared by HAL/root-task for an
isolated driver runtime. It names admitted identity, MMIO, DMA, shared pages,
rings, IRQs, bus links, and optional framebuffer resources; accepting it is
necessary but not sufficient owner-state proof.

## S

### Scheduling context

The seL4 MCS object carrying a thread's execution budget and period. Kernel
scheduling contexts are distinct from root-task service-turn limits and from
declarative requests under `/queen/schedule/ctl`.

### Schedule queue

`/queen/schedule/ctl` accepts bounded declarative scheduling requests, while
`/proc/schedule/*` exposes read-only snapshots. It does not let a client bypass
kernel MCS parameters or generated role authority.

### seL4

A capability-based microkernel with machine-checked formal verification for
documented configurations and assumptions. Cohesix uses upstream seL4 for
memory isolation, task execution, capabilities, IPC, interrupts, and
scheduling; the rest of Cohesix remains separately tested and audited.

### Secure9P

Cohesix's bounded host-side subset of 9P2000.L. It defines framing, attach,
walk, open, read, write, clunk, session state, error codes, tags, fids,
batching, and hard limits for host NineDoor. The target console is a separate
protocol even where NineDoorBridge exposes overlapping paths.

### Secure9P operations

`Tversion/Rversion` negotiate the protocol and `msize`;
`Tattach/Rattach` select an identity view; `Twalk/Rwalk` resolve paths;
`Topen/Ropen` open a fid; `Tread/Rread` and `Twrite/Rwrite` transfer bounded
bytes; `Tclunk/Rclunk` retire a fid; `Rerror` reports a bounded error. Create,
remove, and stat are not in the current accepted subset.

### Serial console

The line-oriented operator and emergency diagnostic path over a physical or
emulated UART. It uses the target command grammar without TCP framing and must
remain available for bounded fatal/recovery reporting even when a migrated
serial runtime or network path is unavailable.

### Service turn

One bounded unit of work. For current driver runtimes, root-task submits the
turn through the fixed driver-task ABI and then completes, yields, or times out.
For current Worker roles, the event pump advances root-owned model state
in-process; only a future executable Worker could receive a separate task turn.
Service turns prevent one device or role from monopolizing the event pump.

### Shard / sharding

A deterministic partition of Worker telemetry paths. The canonical path is
`/shard/<label>/worker/<id>/telemetry`; the label derives from the Worker ID
and selected `shard_bits`. Clients should discover the active profile rather
than assume a label width.

### Sharding legacy alias

The compatibility path `/worker/<id>/telemetry`, present only when
`sharding.legacy_worker_alias` is enabled. New clients and documentation should
use the canonical sharded path.

### Short write

A transport writer accepting fewer bytes than requested. Secure9P's selected
policy either refuses immediately or performs only the documented bounded retry
sequence; short-write handling does not relax provider write permissions.

### Sidecar

A host-side adapter that integrates an external service or field protocol
without moving its heavy stack into the VM. Sidecars publish and consume only
manifest-enabled, bounded namespace records.

### SIEM

Security information and event management software used to collect and analyze
security evidence. Cohesix evidence packs can feed a SIEM after deployment
redaction and classification; the SIEM remains outside the target.

### Single writer

The rule that exactly one active owner may mutate a logical control path or hive
at a time. It applies to direct console ownership, bridge snapshot publication,
and failover; readers or gateway clients can still be multiplexed behind that
one owner.

### SMP

Symmetric multiprocessing: using more than one CPU core. Cohesix scheduling and
proof must identify the selected seL4 SMP profile; multi-core host or QEMU
behavior does not automatically prove physical Pi scheduling.

### smoltcp

A small Rust TCP/IP stack used by the target's permitted authenticated console
listener. It does not introduce general in-VM network services; all other TCP
services remain host-side.

### Source manifest

See [Manifest](#manifest--source-manifest). It is the human-authored compiler
input, while the resolved manifest and generated artifacts are compiler output.

### Split brain

A failure in which more than one writer believes it is active for the same
logical hive. Fencing, health thresholds, WAL handling, and host-orchestrated
cutover are intended to prevent it.

### SPSC ring

A fixed single-producer/single-consumer shared ring used by selected driver-task
command or completion paths. Its fixed layout, ownership indices, capacity, and
memory ordering are part of the ABI; it is not a general shared-memory RPC
escape hatch.

### SwarmUI

The host-side desktop workbench for bounded telemetry, status, replay, Live
Hive visualization, and the shared operator console. It reuses `cohsh-core`
and documented transports, and it adds no target verbs or authority.

## T

### Tag / tag window

A 9P request identifier echoed in the response, and the bounded number of tags
that may be in flight in a session. Reusing an active tag or exceeding the
window is refused.

### Target / VM

The Cohesix environment running seL4, root-task, root-owned Worker session/model
state, and any profile-selected driver runtimes. Current profiles launch no
Worker child tasks. QEMU supplies the reference virtual machine; Pi 4 is
physical target hardware. The host and target have different trust, tooling,
and evidence boundaries.

### Target-qualified evidence

Evidence labelled with the exact target, profile, image or build identity,
transport, and run. It prevents a QEMU result, old Pi boot, or host test from
being presented as proof for a different target.

### TCB / trusted computing base

The components that must behave correctly for a security claim to hold.
Cohesix keeps its target TCB intentionally small by using seL4, `no_std`
userspace, explicit capabilities, and host-side heavy stacks. “Small” does not
mean “only the kernel” or “everything is formally verified.”

### Telemetry

Bounded observations associated with Worker roles or emitted by drivers, host
providers, and tools. Current Worker telemetry may be produced by an authorized
session, a root-owned model helper, or host simulation; it is not Worker-TCB
evidence.
Canonical Worker telemetry lives at
`/shard/<label>/worker/<id>/telemetry`; its payload schema and retention are
provider/profile specific. Telemetry informs decisions but does not grant
authority.

### Telemetry ring / segment

A ring retains a bounded moving window and can overwrite old bytes under its
documented policy. Queen ingest uses OS-named segments under
`/queen/telemetry/<device_id>/seg/*`. A segment is an ingest record container,
not a disk partition or unbounded data lake.

### Test Plan / stage

The repository's staged regression system and its numbered execution groups.
Stage results prove only the surfaces, target, profile, and evidence contract
named by that run; the Test Plan does not collapse QEMU and hardware proof.

### Ticket

In role/security context, see [Role ticket](#role-ticket--capability-ticket). In
host-automation context, see [Host ticket](#host-ticket). The two are different
objects and should not be called interchangeable.

### Ticket claims / scope / quota / subject / secret

Claims are the fields protected by the ticket MAC. Scope limits paths, verbs,
rates, bandwidth, or cursors; quotas and budgets cap consumption; subject binds
the credential to a Worker identity; the secret is host-only key material used
to mint or verify the MAC.

### Trace

A bounded record of requests, replies, ordering, or events used for debugging
and deterministic tests. A trace is evidence from its capture boundary; replay
of it does not prove live hardware behavior.

### Transport

The mechanism carrying a documented interface: direct TCP console, serial,
host Secure9P, gateway REST, FUSE, QEMU harness, or mock. Choosing a transport
does not change the underlying role or create extra authority.

### TTL

Time to live, a maximum lifetime measured in seconds or milliseconds according
to the owning schema. Tickets, budgets, leases, requests, and cached state may
have different TTLs; expiry is a refusal or lifecycle event, not a silent
extension.

## U

### U-Boot

The bootloader in the accepted Raspberry Pi 4 path:
Pi firmware → U-Boot → seL4 binary image → root-task. It stages and passes
bounded boot properties; it is not the Cohesix kernel or acceptance proof by
itself.

### UEFI

A standardized PC/server firmware interface. UEFI/AWS support is
profile-scoped future work and is not part of the Pi 4 acceptance path, which
uses U-Boot and the seL4 binary-image handoff.

### Untyped

An seL4 capability representing physical memory from which kernel objects can
be created. Root-task and HAL manage untyped authority according to generated
truth; child tasks receive only the resulting capabilities they require.

### USB / local seat

USB is the peripheral bus used for Pi keyboard input through the isolated xHCI
runtime. Discovering a controller, enumerating a device, receiving a HID report,
and accepting a local command are separate gates; HDMI feedback is another
separate surface.

## V

### Virtual counter / `CNTVCT_EL0`

The read-only Arm counter used for Pi hardware deadlines when the selected seL4
build exports it. Drivers scale time from generated `TIMER_CLOCK_HZ` and must
not substitute raw CPU-speed spin loops or unauthorized physical timer
registers.

### VL805

The Raspberry Pi 4 USB controller device reached behind PCIe. Cohesix requires
separate PCIe identity/link/resource proof and xHCI/keyboard functional proof;
seeing VL805 in firmware or U-Boot is not live Cohesix ownership.

### VM

Virtual machine. In Cohesix documentation this usually means the QEMU target,
not the macOS host process or a generic cloud VM. A VM result must still name
its target profile and selected seL4 build.

### VSpace

A task's seL4-managed virtual address space. Separate VSpaces can prevent a
future executable Worker or a current driver runtime from directly reading
root-task or peer memory unless HAL and the generated capability layout
explicitly share pages. Current profiles provide this boundary for selected
driver runtimes, not Worker roles.

## W

### WAL / write-ahead log

A host-side log that records intended mutations before they are applied so
failover tooling can recover or reconcile work. The WAL is bounded deployment
state, not target authority, and must not be shared unsafely between concurrent
agents.

### Walk / `Twalk` / walk depth

The 9P operation that resolves path components from one fid into another. Walk
depth is the maximum number of components accepted; Secure9P also rejects
empty, oversized, NUL-containing, slash-containing, and `..` components.

### Watchdog

Host-side failover automation that probes active and standby gateways, applies
failure/success thresholds and hold-down, fences writers, and switches the live
operator path. It does not run target-side leader election.

### Worker

A narrowly authorized role in the ticket, namespace, lifecycle, and telemetry
model. A profile may make a Worker executable only by declaring and proving its
target image, task objects, capabilities, notifications, scheduling, faults,
and revocation. Current profiles launch no Worker child tasks. Workers are not
generic AI agents, physical drivers, host daemons, or unrestricted subprocesses.

### WorkerHeartbeat / `worker-heartbeat` / `worker-heart`

The recognized Worker session/model role for liveness telemetry and the minimal
Worker observability view. Current profiles do not launch it as a target task.
`worker-heart` is the historical crate or project shorthand;
`worker-heartbeat` is the public role label.

### WorkerGpu / `worker-gpu`

See [GPU Worker](#gpu-worker--worker-gpu). The root-owned session/model mirrors
ticket, lease, status, and telemetry state while all GPU hardware access remains
host-side.

### WorkerLora / `worker-lora`

See [LoRA](#lora). This recognized session/model role observes ticket,
lifecycle-model, receipt, and telemetry state through its Worker view. Current
profiles do not launch it as a target task or enable its reserved endpoint and
notification ranges. It has no separate root namespace or file-backed LoRA
lease. It does not train or execute models, access GPU hardware, import
artifacts, or reload a host model.

### WorkerBus / `worker-bus`

A recognized role and host/sidecar policy label that is **not executable in the
checked-in profiles**. The presence of a crate, namespace contract, or ticket
label does not make it active target authority.

### Worker authority

Two distinct layers: valid session role/ticket authority, and, only for a future
executable target Worker, current cap-backed seL4 invocation authority. Current
profiles implement only the session layer. Reserved endpoint badges,
notification values, and scheduling records are metadata and cannot substitute
for live task objects and delivered capabilities.

### Worker ticket

A required role ticket whose role and subject match the Worker attachment.
It grants only its bounded namespace and operation scope; it does not grant
Queen, driver, host, or GPU hardware authority.

### Write modes

Provider-owned rules describing whether a node is read-only, append-only, or a
validated control sink. Transport retry behavior and write offsets do not
override the node's mode or schema.

## X

### xHCI

The standard USB host-controller interface implemented by the Raspberry Pi 4
VL805. Cohesix's isolated xHCI runtime owns controller state and bounded USB
keyboard service after HAL admits the PCIe, MMIO, DMA, and shared resources.

## Where to go next

| If you want to understand… | Read… |
| --- | --- |
| The trust boundary and component model | [Architecture](ARCHITECTURE.md) |
| Console, namespace, record, and error contracts | [External Interfaces](INTERFACES.md) |
| 9P framing, fids, tags, bounds, and operations | [Secure9P](SECURE9P.md) |
| Queen/Worker roles, tickets, and scheduling | [Roles and Scheduling](ROLES_AND_SCHEDULING.md) |
| HAL, driver runtimes, DMA, IRQ, and proof | [HAL and Physical Drivers](DRIVERS.md) |
| Host tools, direct/gateway mode, and authentication | [Host Tools](HOST_TOOLS.md) |
| GPU, model, PEFT, and host-executor boundaries | [GPU Nodes](GPU_NODES.md) |
| Current target status and proof layers | [Hardware Bring-up](HARDWARE_BRINGUP.md) |
| Runnable commands and scripts | [Userland and CLI](USERLAND_AND_CLI.md) |
| Milestone status, planned work, and task authority | [Build Plan](BUILD_PLAN.md) |
