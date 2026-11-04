<!-- Author: Lukas Bower -->
<!-- Purpose: Record the isolated seL4 17 build and exact Pi network evaluation, with preserved v16 evidence and explicit proof limits. -->
<!-- Copyright 2026 Lukas Bower -->

# seL4 17 build and Pi network evaluation

Upgrade to seL4 17. The selected Cohesix configuration is affected by upstream
fixes; a conclusion of no impact would be unsupported. Canonical QEMU and Pi
builds, the focused physical network regression and bounded bench restoration
passed. The fresh seven-day observation is active from 4 to 11 November 2026;
the exact interval and initial observer evidence are recorded below.

This is the execution record for `m26d1-sel4-17-pi-network`. The accepted v16
source installation, environment, historical evidence and installed release tools
remain protected while the candidate is built. The old Pi observation was
deliberately interrupted for the requested RAM test. Building the candidate does not
establish Pi boot, network, seven-day, performance or release acceptance.

## Recommendation and applicability

The [30 October announcement](https://sel4.systems/news/) recommends upgrading.
The [17.0.0 release notes](https://docs.sel4.systems/releases/sel4/17.0.0)
include these relevant fixes. Applicability below is inferred from those notes
and the actual selected kernel configuration, rather than an exploit test.

| Upstream issue | Cohesix assessment |
| --- | --- |
| AArch64 non-hypervisor ASIDs above 255 were truncated during TLB maintenance, potentially retaining access to reused frames. | Both production profiles disable hypervisor support. The 256-Worker design plus service address spaces makes this material. The twenty-Worker trial does not exercise or disprove exploitation at that boundary. |
| MCS fastpath endpoint deletion could write through NULL. | MCS and fastpath are enabled. Root owns endpoint lifecycle and teardown; restricted Worker capabilities reduce exposure but do not justify excluding the affected kernel path. |
| Arm instruction cache invalidation was missing on untyped reset. | Relevant to the selected architecture and memory reuse. No exploitation is claimed. |

The x86, RISC-V and Arm-hypervisor-specific issues do not match these selected
profiles. Their absence does not remove the applicable issues above. Cohesix's
SMP/MCS profile must not inherit a claim of formal verification from a different
seL4 configuration. Fresh generated headers, loader and image checks are required;
the version label alone supplies no compatibility evidence.

## Source and environment

The complete official seL4Test 17.0.0 manifest is pinned at
`655b1f2a67de058571b5f653688aa55440745192`; its kernel commit is
`62e345c240a15671484530b4e6e069ce266c28d7`. The source contract records every
repository. `~/seL4_17` holds the Pi operational source and the exact rebased
initial-SC/VL805 overlay. `~/seL4_17_qemu` is the separate pristine QEMU source.
The accepted `~/seL4_16` source and environment are preserved, with reference
file hashes and the complete distribution inventory recorded privately.

`~/seL4_17/.venv_aarch64` independently recreates the v16 reference's exact
38-distribution closure, including `sel4-deps==0.7.0`,
`protobuf==6.33.5`, `typing_extensions==4.16.0` and `setuptools==80.9.0`.
The existing source-controlled hash locks remain authoritative. The canonical
profile Python path resolves to this dedicated environment.

The first builds used the upstream 0.9.0 dependency metapackage. Its
`protobuf<4` compatibility constraint selected 3.20.3, while the Mac has
`protoc` 32.1. Nanopb generation then failed to import `runtime_version`.
Those attempts remain at their original build paths. The candidate restores
the reference closure and uses new empty build directories; no package,
source, freshness or profile check is disabled.

The standard macOS setup script provisions the pinned Arm compiler, GNU cpio
and U-Boot tools. The DENX archive origin timed out once. A Buildroot source
mirror supplied identical bytes: 34,172,789 bytes and SHA-256
`b60d5865cefdbc75da8da4156c56c458e00de75a49b80c1a2e58a96e30ad0d54`.
The canonical script still verifies that unchanged pin. Pi image staging uses
the existing `third_party/u-boot` and Raspberry Pi firmware inputs. QEMU uses
the retained `~/cohesix/qemu/bin/qemu-system-aarch64` 10.1.0 HVF/GIC runtime.

## Compatibility review

The generated implementation inventory is the review index. The changed
surface is the kernel source/header/ABI selection and its build inputs.

| Surface | Required compatibility treatment |
| --- | --- |
| Compiler and target runtime | Admit the exact v17 AArch64 SMP+MCS object pack while retaining v16 admission; reject unsupported packs and excess refills. Bind actual v17 headers and immutable composition inputs before a target build. |
| Native Linux host build | Add explicit `--target-profile pi4_production` to bind generated policies, provenance and Python projection to the Pi timer and manifest. Preserve the KVM default and its existing provenance fields. The previous installed host manifest differs from v17 only in the selected kernel field, but its signed producer bindings cannot be relabeled as the new manifest. Build a separate matching host-tool set for restoration. |
| `cohsh`, gateway, policy, CAS and host bridges | Console grammar, authority, framing, error classes, namespaces, job lifecycle and host operations are unchanged. Preserve the existing host tools during the physical comparison. |
| `tools/cohesix-py`, MCP, A2A and SwarmUI | Python wire/API contracts and provider behavior are unchanged. Regenerate the selected target/profile identities through `coh-rtc`; old target identities cannot represent the v17 image. |
| Benchmarks | Preserve the workload, bounds, deadlines and report schema. The requested GENET fault regression is functional recovery evidence, not throughput or latency acceptance. |
| Release inputs | Published `releases/` files and existing release evidence remain immutable. This work does not publish or promote an overall release. |

## Build and physical checks

Use `scripts/sel4_profile.py` for source preparation, fresh configuration,
build and validation, followed by the canonical
`scripts/cohesix-build-run.sh` and `scripts/pi4-image-build.sh` entry points.
Record actual commands and results in the private run directory
`~/cohesix/assets/sel4-17-pi-network-20261104-071438`.

The frozen recent GENET protocol requires three qualified cycles in at most
five attempts on one exact boot. Each needs all 13 packet/serial/counter
checks, with fresh authenticated TCP recovery before post-trial diagnostics.
The existing v16 observation must receive a deliberate interruption record
before canonical CRC-checked TFTP/U-Boot RAM deployment. Keep SD unchanged.
The fresh seven-day baseline begins only after qualified regression and
runtime restoration; elapsed time from v16 cannot count toward it.

At record creation, compiler admission checks passed 18 tests. The first
profile test run passed 118 checks and failed the current immutable-Pi-tree
check because that candidate tree still contained v16 artifacts. That check
must pass after the fresh v17 inputs are retained and committed. The next run
validated that tree and found its test still pinned the v16 contract hash;
the exact expected hash is updated to the pinned v17 contract without changing
the required configuration or cleanliness assertions.
The subsequent reference-environment kernel builds and both production
profile validations passed. Canonical QEMU image staging and generated-contract
checks passed, and all 133 compiler library tests passed. Live-target and
sustained-observation results remain open at this checkpoint. The private run
record owns later observations and their exact source/image identities.

## Repository reference replacement

The user's subsequent instruction replaces the older current entries under
`seL4/`. All three build mirrors now use v17: `build` is
`qemu_smp_production`, `SMP_build` is `qemu_smp_diagnostic`, and `build_UBOOT`
is `pi4_production`. The QEMU mirrors now match the current four-core MCS
profiles rather than their former classic configurations. Their complete
copied files are checked against successful maintained profile builds. The
older bytes are retained in the private run's reference archives and in Git
history. Historical classic-comparator identities remain unchanged.

The official Reference Manual 17.0.0 PDF is retained unchanged from
`https://sel4.systems/Info/Docs/seL4-manual-17.0.0.pdf`; the Markdown version
is its `pdftotext -layout` extraction with a provenance header. Elfloader
documentation comes from the pinned v17 `tools/seL4` source. Upstream authorship
and licenses are preserved. `seL4/README.md` records the current mapping.

## Executed target evaluation

The canonical QEMU build and authenticated root/MCS convergence passed,
including all 37 focused guards. The canonical Pi image build passed from clean
source `4daf5e903aac114f8a8a54b3727ee9d09a0df49e`. Its exact RAM image identity is
`3cfde3126677ebb759facdfb29acf8eef0c5e8778a5c6f284571e024b3257b82`.
All three payload CRCs passed before and after the mandatory peripheral reset,
the sealed BUILD matched, and 130 seconds of passive GENET settling completed
without a declared fault. SD was unchanged.

Cycles 1, 3 and 4 each passed all 13 independently recomputed packet, serial,
counter and fresh-TCP-recovery checks on that same boot. Attempt 2 is retained
as unqualified: the host controller timed out after the complete post-trial
NETSTATS reply and prompt were captured. Its missing continuation prefix can
miss a prompt split across reads; this defect was reproduced offline. The
private v4 controller carries at most eight prior bytes into the next prompt
read without duplicating them. All ten host tests passed. Commands, pacing,
deadlines, target behavior and qualification checks are unchanged. No failed
attempt was converted into a qualified result.

The physical proof names the exact image/source above. The subsequent QEMU
reference and manual replacement does not change the tested Pi composition
tree or runtime sources and does not retroactively change that image's source
identity. The private `analysis/REGRESSION_VALIDATION_COMPLETE.json` binds all
three verdicts, the unqualified attempt and exclusive-control handoff.
This is focused same-boot RAM regression evidence. Independent-boot, SD,
performance, pressure, completed-week and overall-release acceptance remain
unproven. The new observation baseline requires completed runtime restoration.

## Final host checks and bench handoff

The complete contribution baseline passed: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo check --workspace`, `cargo test --workspace`, `cargo audit`,
`cargo deny check advisories`, `scripts/check-generated.sh` (including Test
Plan consistency), `scripts/ci/test_plan_run.sh --list`, the unchanged Rust risk
gate and `git diff --check`. The first workspace test run detected stale
generated hashes embedded in `USERLAND_AND_CLI.md`. Regenerating its canonical
snippet bodies fixed that drift; the unchanged full test command then passed
2,855 tests, with five ignored. The first failure is retained.

Additional checks passed: 119 seL4 profile tests, 133 compiler library tests,
three Pi compiler profile tests, 41 Linux builder/release checks, and the
canonical Python SDK gate with 264 passed and one skipped. The QEMU convergence
record explicitly says `NON-CLAIMING TARGET DIAGNOSTIC`; it proves readiness and
one authenticated operation with the 37 focused guards, not staged acceptance.
Both its clean source and the physically tested Pi source are the exact
`4daf5e903aac114f8a8a54b3727ee9d09a0df49e` checkpoint above.

The canonical native Linux build used clean source
`9d3867810d5e4a21b155c547c0a0bec9be8fd450` and explicit
`--target-profile pi4_production`. All eight binaries passed artifact, compiled
policy and GLIBC checks. Their resolved manifest matches the tested Pi exactly:
`544f0f93b95d6882580742ebdac7c122246440114b3898f9263da21c8ad48b57`.
They are staged separately; the installed release binaries are preserved.

The restored target reports twenty ready Workers: six GPU and fourteen LoRA.
The first GPU admission preceded the publisher's first inventory commit and was
refused. Read-only reconciliation bound the original identity to a terminal
error, empty census and empty GPU lease log. After inventory readback, a fresh
replacement identity admitted that Worker; the refused identity was not replayed.
One local CUDA preparation also failed before submission because its preparation
file was missing. The acknowledged lease and original job identity were retained;
supplying the parameterized preparation file allowed exactly one submission.
All failed attempts remain preserved.

The single bounded native CUDA job succeeded. The canonical verifier accepted
all nine signed phases, and independent arithmetic matched all 65,536 output
floats, with SHA-256
`21d9732782924047cddc655ebae9099eacd30e7fa49f6d1c5ccd624d159a9d0f`.
Both agent journals have zero pending entries. This is authenticated Root/Worker
and native-output evidence with retained operator key custody, not device
attestation, LoRA execution, pressure testing or release acceptance.

The old continuous-boot observation was deliberately interrupted after 23.396
hours. It did not complete. At initial publication, native SwarmUI rendered but
remained disconnected, and automated Screen Sharing inputs produced no
application state change. No new baseline had been created; observers and the
heartbeat remained inactive. That checkpoint and its unconfirmed input failure
are retained. The later handoff below resolves that gate. Passive packet captures
and the sole receive-only UART recorder have a fixed hard stop at
`2026-11-12T08:26:05.343223Z`. Collection time cannot substitute for a verified
seven-day baseline. No SD deployment or overall release is claimed.

Private evidence remains under the run directory above and its corresponding
`/mnt/nvme/cohesix-dev/sel4-17-pi-network-20261104-071438` Jetson directory.
`RUN.json`, `SESSION.md`, `analysis/REGRESSION_VALIDATION_COMPLETE.json`,
`fleet/FLEET-IDLE-AND-EXPIRY.json` and `fleet/gpu-check/independent-audit.json`
own the current result and proof limits. Subsequent documentation and repository
reference commits do not relabel the source of an already tested image.

## Active seven-day observation

After the operator restarted Screen Sharing, native SwarmUI connected through
the existing sole gateway and showed LIVE, Queen reachable, ROOT OK, a populated
animated topology, one session, zero backlog and zero dropped work. Protected
process credential references were used; credentials and connection profiles
were not saved. The exact Pi RAM boot and image remain unchanged.

Fresh read-only reconciliation again proved twenty ready Workers, six GPU and
fourteen LoRA, both delivery journals drained, and the original single CUDA job
still succeeded. No job was replayed. The GUI received one new finite delegation
after its application had exited. The publisher and agent reached their normal
one-hour expiry during the UI handoff and were restored once before baseline.
Their scopes, one-hour lifetime and 32,768-operation allowance are unchanged;
local claim inspection confirmed the same scope, mount and quota bytes. Exact
expiry times are retained in the baseline. No tested-client renewal or restart
is planned during observation; expiry alone is configuration behavior, not a
Pi crash.

The immutable window is `2026-11-04T11:29:23.302390Z` through
`2026-11-11T11:29:23.302390Z`, exactly 604,800 seconds. In Sydney, this is
4 November 2026 at 22:29:23 AEDT through 11 November at the same time.
Thirty mirrored serial-prefix digests were checked before selection. The first
seal attempt stopped before creating a baseline because the Jetson mirror
lacked the already retained regression summary; copying that exact evidence
allowed the single fresh seal. No time was backdated or carried from v16.

All thirty observer host tests, both configuration checks and systemd unit
verification passed. The initial passive checkpoint found no issues. One bounded
Queen-log CAT completed with full END, 2,076 lines and 110,938 source payload
bytes. The three unique `cohesix-pi4-v17-20261104-071438-*` timers are enabled:
passive analysis every five minutes, Queen snapshots about fifteen minutes after
each completed sample, and the exact endpoint. The endpoint's realtime
microsecond value matched the immutable baseline. The first scheduled passive
checkpoint also found no issues. The existing six-hour heartbeat has resumed.

`BASELINE.json`, `observer/OBSERVER-ACTIVE.json` and the private `SESSION.md`
own the interval, actual schedules, evidence hashes, finite credentials and
remaining limits. Queen exports have fixed deadlines, no retry and a durable
first-failure fence; they are retained samples rather than a lossless history.
The endpoint stops only this run's observer timers and takes a final passive
checkpoint. Seven days have not elapsed, and no completed-week or overall-release
acceptance is claimed.

Material AI assistance was used for source comparison, scoped implementation,
build orchestration and evidence analysis. This record claims no independent
review, human release approval or unexecuted test result.
