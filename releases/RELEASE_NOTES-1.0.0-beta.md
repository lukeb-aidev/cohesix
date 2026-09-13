<!-- Author: Lukas Bower -->
<!-- Purpose: Introduce Cohesix 1.0.0-beta, explain its architectural advances and practical impact, and welcome contributors. -->
<!-- Copyright 2026 Lukas Bower -->

# Cohesix 1.0.0-beta Release Notes

Date: 2026-09-13

**Cohesix 1.0.0-beta is here!** This is a major step forward from 0.9.0:
a move to seL4 16 with explicit CPU budgets, a substantial redesign around
isolated services and linked driver runtimes, 256 configured Workers, and a
complete Raspberry Pi 4 SD-image distribution.

The idea behind Cohesix stays simple: give an edge AI fleet a small, accountable
control plane. A Queen coordinates narrowly scoped Workers, while GPU
workloads continue in the host's native environment. Plan 9-inspired namespaces make
control and telemetry accessible through files and paths; seL4 capabilities
make authority explicit.

This release brings that idea much further into a working operating system.
You can explore it in QEMU, put it on a Pi, connect the host tools, and help
shape what comes next. We want to build a community around that work—systems
programmers, hardware enthusiasts, AI engineers, and people who enjoy making
complex technology easier to use.

## What changed since 0.9.0-beta

### MCS: CPU time becomes an explicit resource

Cohesix now uses **seL4 16.0.0 with four-core SMP and Mixed-Criticality
Scheduling (MCS)** on its production QEMU and Pi profiles. SMP lets the system
use multiple processor cores. MCS adds control over how much processor time
an active task can consume.

A task's *scheduling context* describes its CPU budget and replenishment
period. Priorities determine which eligible task runs; budgets constrain how
much execution it receives. Cohesix gives control, emergency handling,
supervision, drivers, and Worker executors explicit scheduling resources.

MCS also supports *passive* services. A passive service runs on a scheduling
context donated by its caller while it handles a request. Cohesix uses this
for its NineDoor namespace service and executable Workers: the work consumes
the calling task or executor's budget, with an explicit return of control.

**Why it matters:** CPU time joins memory and device access as a resource the
architecture can account for. This gives Cohesix a stronger foundation for
keeping control and recovery work serviceable alongside busy networking and
Worker activity. Kernel budgets, queue limits, and bounded service loops make
those scheduling decisions explicit and inspectable.

### Isolated, linked driver runtimes

The Pi driver architecture has changed substantially. Serial, USB keyboard
input, HDMI text output, GENET Ethernet, CYW43 Wi-Fi, SDIO, and PCIe service
have runtime roles declared in the build manifest. Drivers execute as separate seL4 tasks
with their own address spaces and explicitly granted resources. The hardware
abstraction layer (HAL) sets up and grants those resources; each runtime owns its device's
steady operation.

**Linked runtimes let drivers cooperate across those boundaries.** A link is
a declared connection with bounded request and completion records, shared
buffers, notifications, and explicit ownership. For example, the CYW43 runtime
handles Wi-Fi firmware and protocol state, while the SDIO runtime owns the
physical bus operations. Wi-Fi requests bus service through their generated
link without acquiring direct access to the SDIO controller.

The Ethernet path applies the same principle to networking. After DHCP and a
controlled handoff, the isolated GENET driver exchanges packets directly with
the isolated console-network service. GENET owns the device, direct memory
access (DMA), and interrupts; the console service owns TCP and transport authentication. Root
coordinates and supervises them without relaying the steady packet path.

**Why it matters:** device ownership is easier to understand, interfaces are
bounded, and the central root task carries less driver and transport work.
For contributors, there is a clear place to implement a driver, a declared
set of resources it may use, and a versioned interface to the rest of the OS.
See the [driver guide](../docs/DRIVERS.md) for the design patterns.

### 256 Workers with supervised lifecycles

Both production manifests configure **256 passive Worker instances**: one
Heartbeat Worker, 127 GPU Workers, and 128 LoRA Workers. Two active executor
lanes supply their CPU budgets and select work from bounded, fair queues.

These are control-plane instances, not 256 GPU machines. Heartbeat, GPU lease,
and LoRA lifecycle duties have separately packaged executable roles, while
CUDA, NVML, training, inference, and PEFT continue to run on the host.

Worker creation, readiness, retirement, replacement, and completion records
(receipts) now carry explicit generation identities. An old completion cannot legitimately stand
in for work performed by a replacement Worker.

**Why it matters:** Cohesix can represent a larger hive while keeping execution
and authority bounded. Operators and automation have a clearer account of
which Worker handled a request and which lifecycle its result belongs to.

### A complete Raspberry Pi 4 distribution

The new Pi archive contains a compact, complete **MBR/FAT32 SD image**, its
checksum, and boot identity metadata. The boot path is Pi firmware → U-Boot →
seL4 → the Rust root task. The release quickstart walks through writing the
image, reading it back, configuring networking, and connecting from a host.

This release also includes substantial work on PCIe admission, VL805 USB
firmware handling, boot memory and stack bounds, Wi-Fi ownership, and Ethernet
response handling. Early boot and integrity records survive ordinary log-ring
eviction, giving hardware investigations more useful context.

**Why it matters:** trying Cohesix on physical hardware no longer starts with
assembling a complete source build. A Pi, SD card, display, keyboard, and a
matching host bundle provide an accessible starting point for exploration.

### A more capable operator toolkit

The existing host tools gain improvements that make a difference during
ordinary use:

- **SwarmUI and discovery follow the generated Worker topology and live state.**
  Host and GPU snapshots include authentication, freshness, and identity
  checks, so unavailable information stays visibly unavailable.
- **Hive Gateway shares console access more efficiently.** Session pooling
  avoids redundant attachments when authority is unchanged, and REST
  deadlines account for the broker's bounded operation.
- **An unfinished local command no longer excludes network service.** A
  partially typed serial or USB keyboard command can wait for its operator
  without preventing the system from servicing network requests.
- **FUSE and evidence export handle more real-world cases.** Mac and Linux
  mount startup, large records, empty audit streams, and failed exports have
  received fixes, with clearer errors in evidence timelines.
- **Maintenance follows outstanding work more accurately.** Drain and resume
  account for active leases and terminal Worker containment. GPU/PEFT receipts
  retain real results, model commits, and generation identities.
- **Python makes the system easier to explore and automate.** The portable
  wheel includes typed Worker support and operator/evidence examples, with
  separate generated contracts for QEMU and Pi. Its package version remains
  independent of the overall Cohesix release version.

The eight host executables are `cohsh`, `coh`, `swarmui`, `hive-gateway`,
`gpu-bridge-host`, `host-sidecar-bridge`, `host-ticket-agent`, and `cas-tool`.
Together with Python, they give contributors several ways into the project:
interactive commands, a desktop view, automation, or host integrations.

## Get started

You do not need a GPU to explore the OS and its control surfaces in QEMU.
Choose a native host bundle, then follow the [quickstart](../docs/QUICKSTART.md).
For physical hardware, add the Pi archive and operate it with the same host
toolkit.

| Archive | Start here for |
| --- | --- |
| [Cohesix-1.0.0-beta-MacOS.tar.gz](Cohesix-1.0.0-beta-MacOS.tar.gz) | Apple Silicon host tools, a Mac HVF QEMU guest, Python, configuration and guides |
| [Cohesix-1.0.0-beta-linux.tar.gz](Cohesix-1.0.0-beta-linux.tar.gz) | Linux AArch64 host tools, a Linux KVM QEMU guest, Python, configuration and guides |
| [Cohesix-1.0.0-beta-Pi4.tar.gz](Cohesix-1.0.0-beta-Pi4.tar.gz) | A complete Pi SD image, checksums, boot metadata and installation instructions |

The Mac and Linux guests use different native timer profiles—24 MHz for Mac
HVF and 31.25 MHz for Linux KVM. Keep each guest with its matching host bundle.
Every archive includes a quickstart and `MANIFEST.sha256`. The Pi image fits
any card meeting the metadata's `minimum_target_bytes`; larger cards retain
unallocated spare capacity.

Linux GPU integrations use the host's compatible CUDA/NVML stack. Jetson is
one reference host; the wider contract is a Linux AArch64 NVIDIA host.

### Upgrading from 0.9.0-beta

1. Export any evidence you want to keep, and back up saved Pi network settings.
2. Extract the new archives into fresh directories and verify their manifests.
3. Run the matching host bundle's setup script and use its Python environment.
4. Keep the guest, binaries, and generated policies together. Review your
   credentials, tickets, and configuration against the new contracts before
   carrying settings forward.
5. Install the Pi image using the quickstart, then configure its network and
   host connection. Writing the raw image replaces the whole card.

The 0.9.0-beta packages remain in `releases/` for comparison. Earlier releases
remain available at their Git tags.

Use Hive Gateway for concurrent clients. The direct console has one
authenticated owner and no transport encryption; keep it on loopback or inside
an authenticated tunnel. Automatic Queen failover and AWS/UEFI are outside
this release's supported scope.

## Help build the Cohesix community

There is plenty to contribute without starting in kernel code. Try the
quickstart and tell us where it gets confusing. Share a reproducible hardware
observation, improve a Python example, make an operator workflow easier, or
help explain the architecture to someone encountering seL4 for the first time.
Rust and driver contributors can build on the explicit runtime and resource
contracts introduced in this release.

Start with [GitHub Issues](https://github.com/lukeb-aidev/cohesix/issues) for
questions, reproducible bugs, and scoped design ideas. Include the release,
host or board, and the smallest useful reproduction. The
[contribution guide](../CONTRIBUTING.md) and [build plan](../docs/BUILD_PLAN.md)
help turn an idea into a focused change. Report suspected vulnerabilities
through the private process in [Security](../docs/SECURITY.md).

Cohesix is Apache-2.0 licensed and maintained by Lukas Bower. If you are curious
about capability-based operating systems, practical edge AI orchestration, or
what a small OS can make possible, come explore it with us.

## Beta and audit notes

Cohesix remains a research beta; no formal verification of Cohesix or its
selected SMP+MCS system is claimed. The [architecture](../docs/ARCHITECTURE.md)
and [security guide](../docs/SECURITY.md) explain the hardware and trust boundaries.

Release Stage 5 was accepted with residual risk. DD26–29 are closed; the
remaining dynamic fault/wake evidence gap, DD30, has a release-specific waiver
expiring on 2026-10-13. Acceptance carries forward original Stage 1–4 evidence
and later scoped repairs. The original timed burn-in failed after 86 minutes
and 43 jobs; focused repairs were checked, but the full timed run was not
repeated. The fresh published builds are marked `NOT_RUN` at the owner's
request; their archive contents, profiles, and hashes were verified.

The [audit report](../docs/audit/AUDIT_REPORT_2026-09-13.md) and
[carry-forward policy](../docs/audit/RELEASE_1_0_0_BETA_CARRY_FORWARD.toml)
contain the detailed evidence, accepted risks, and scope of those decisions.
