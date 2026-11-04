<!-- Author: Lukas Bower -->
<!-- Purpose: Record the isolated seL4 17 build and exact Pi network evaluation, with preserved v16 evidence and explicit proof limits. -->
<!-- Copyright 2026 Lukas Bower -->

# seL4 17 build and Pi network evaluation

This is the candidate record for `m26d1-sel4-17-pi-network`. The accepted v16
source installation, environment, historical evidence and installed release tools
remain protected while the candidate is built. The old Pi observation was
deliberately interrupted for the requested RAM test. Building the candidate does not
establish Pi boot, network, seven-day, performance or release acceptance.

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

Material AI assistance was used for source comparison, scoped implementation,
build orchestration and evidence analysis. This record claims no independent
review, human release approval or unexecuted test result.
