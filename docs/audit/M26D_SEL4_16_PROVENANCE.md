<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record the official seL4 16 source identity and the Cohesix profile and evidence gates required before baseline acceptance. -->
<!-- Author: Lukas Bower -->

# Milestone 26d seL4 16 Provenance

```
Title/ID: m26d-kernel-provenance-refresh
Milestone: Milestone 26d — seL4 16 Baseline Refresh + Reference/Performance Realignment / m26d-kernel-provenance-refresh
Goal: Bind the requested seL4 16.0.0 refresh to exact upstream inputs and prevent source, build, or live-target evidence from being promoted across proof classes.
Inputs: Official seL4 16.0.0 release notes and manual, sel4test-manifest 16.0.0, CAmkES 3.13.0 release notes, configs/sel4/profiles.toml, the external seL4 project, profile build trees, and target-qualified evidence.
Changes:
  - docs/audit/M26D_SEL4_16_PROVENANCE.md — record exact upstream pins, required profile invariants, companion-release scope, and acceptance boundaries.
Commands: git ls-remote --tags https://github.com/seL4/sel4test-manifest.git refs/tags/16.0.0; git ls-remote --tags https://github.com/seL4/seL4.git refs/tags/16.0.0 refs/tags/16.0.0^{}; out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate --all --require-source --require-artifacts --evidence out/audit/m26d-profile-v2-all-sel4-16.json
Checks: Every accepted v16 build resolves the complete official project, reproduces the selected profile from an empty build directory, and remains in its exact evidence class.
Deliverables: Reviewable v16 source ledger plus explicit static-build, linked-image, exact-image, live-board, and proof-eligibility gates.
```

## Authority and status

The version authorities are the official
[seL4 16.0.0 release notes](https://docs.sel4.systems/releases/sel4/16.0.0.html),
[seL4 Reference Manual 16.0.0](https://sel4.systems/Info/Docs/seL4-manual-16.0.0.pdf),
tag-pinned [seL4 16 caveats](https://github.com/seL4/seL4/blob/16.0.0/CAVEATS.md),
and the official
[sel4test-manifest 16.0.0 project](https://github.com/seL4/sel4test-manifest/tree/16.0.0).
The release is dated 2026-07-22 and is explicitly marked breaking.

This ledger resolves the immutable upstream candidate for the Cohesix refresh.
It does not by itself accept a Cohesix v16 baseline. Acceptance additionally
requires the source-controlled profile, regenerated bindings, fresh builds,
linked images, and target-qualified evidence described below. A renamed
directory, a retained CMake cache, or a v15 result stored under a v16 path is
not v16 evidence.

The existing `M26D_SEL4_15_PROVENANCE.md` and
`M26D_SEL4_15_CAPABILITY_AUDIT.md` remain historical truth for the v15
baseline. They must not be rewritten or cited as v16 build or runtime proof.

## Exact official seL4Test source

The Cohesix build input is the complete official seL4Test project. The
`16.0.0` manifest tag is a lightweight tag at
`b81fd440977a8ad89ed8478a9a5a027f062551f6`.
The kernel's annotated `16.0.0` tag object is
`bae644095ed24ff3a3982fecb06fb34201416fee`, peeled to commit
`6e7c3b733d296cfd88d5fbf635c96e447a882374`.

| Path | Repository | Required revision |
| --- | --- | --- |
| `.repo/manifests` | `https://github.com/seL4/sel4test-manifest.git` | `b81fd440977a8ad89ed8478a9a5a027f062551f6` |
| `kernel` | `https://github.com/seL4/seL4.git` | `6e7c3b733d296cfd88d5fbf635c96e447a882374` |
| `projects/musllibc` | `https://github.com/seL4/musllibc.git` | `b0005f86fecbd6d0257b15363a5b013446914265` |
| `projects/seL4_libs` | `https://github.com/seL4/seL4_libs.git` | `d8abd95e7114c852f6636e3084eb1f66091a75ee` |
| `projects/sel4_projects_libs` | `https://github.com/seL4/sel4_projects_libs.git` | `5b3c81127b191232489df09a59b22edead1c9db7` |
| `projects/sel4runtime` | `https://github.com/seL4/sel4runtime.git` | `86489cf6efab9f314964e79468c036e9035394c7` |
| `projects/sel4test` | `https://github.com/seL4/sel4test.git` | `60b7b47ee0a67f56e15951735a6aa9d270d47f55` |
| `projects/util_libs` | `https://github.com/seL4/util_libs.git` | `6e55b3c62687779692150e1de411ce61b9d2919a` |
| `tools/nanopb` | `https://github.com/nanopb/nanopb.git` | `cad3c18ef15a663e30e3e43e3a752b66378adec1` (`0.4.9.1`) |
| `tools/opensbi` | `https://github.com/riscv/opensbi.git` | `234ed8e427f4d92903123199f6590d144e0d9351` |
| `tools/seL4` | `https://github.com/seL4/seL4_tools.git` | `7dd5ba144b1fecf1358a12d2bef3eb365aab35c7` |

These revisions are the contents of the official
[`default.xml`](https://github.com/seL4/sel4test-manifest/blob/16.0.0/default.xml).
All projects must be present at the recorded commit. Validating only the kernel
commit is insufficient.

## Cohesix profile requirements

The v16 refresh must preserve the established Cohesix profile distinctions. A
version change does not authorize a scheduler, target, driver, authority, or
operator-protocol redesign.

| Lane | Required v16 contract |
| --- | --- |
| QEMU production | AArch64 `qemu-arm-virt`, four nodes, GICv3 in generated DTBs and launcher, one domain, non-MCS, non-hypervisor, no SMMU or SMC forwarding, printing/debug/benchmark facilities disabled, and release/runtime eligibility validated from a pristine source tree. |
| QEMU diagnostic | The same four-core AArch64/GICv3/non-MCS system model with diagnostic-only printing/debug settings and no release eligibility. |
| Pi 4 production | AArch64 `bcm2711`, four nodes, one domain, non-MCS, non-hypervisor, `KernelRootCNodeSizeBits=14`, rootservers-last U-Boot image flow, no SMMU or SMC forwarding, and the exact counter contract below. |
| Pi 4 diagnostic | The same Pi system model and counter contract with explicitly diagnostic kernel settings; it is not release or exact-image evidence. |
| BCM2711 proof eligibility | A pristine source tree built from v16 `kernel/configs/AARCH64_bcm2711_verified.cmake`; separate from operational profiles and classified only as upstream configuration compatibility. |

For all operational profiles:

- `KernelNumDomains=1` remains the selected contract and the obsolete
  `KernelDomainSchedule` cache input remains forbidden.
- `KernelIsMCS=OFF` remains the 26d contract. seL4 16 does not implicitly
  activate the separately gated Milestone 26e SMP+MCS transition.
- Every v16-sensitive default is recorded as an explicit effective value in
  generated configuration and evidence. In particular, the v16 change of
  `KernelArmVtimerUpdateVOffset` to default `OFF` must not be inherited
  silently.
- QEMU and proof-eligibility sources are pristine. Pi operational sources may
  contain only the authenticated Cohesix Pi overlay.
- The existing Pi overlay must be reapplied and reviewed against the exact v16
  kernel. Because the upstream v16 base bytes are unchanged at that hunk, the
  canonical full-index raw diff remains byte-identical at
  `3c82bf606f76823398f91070c9787d13aa137e7e7c2665a01db11df9797f69a6`;
  any other source diff remains invalid.
- Source preparation, configure, and build start from empty paths with
  `SEL4_CACHE_DIR` and `MEMOIZE_CACHE_DIR` disabled. Copied or renamed v15
  output is not accepted.

The Pi timing contract remains:

- `KernelArmExportVCNTUser=ON`;
- `KernelArmExportPCNTUser=OFF`;
- `KernelArmExportPTMRUser=OFF`;
- `KernelArmExportVTMRUser=OFF`; and
- generated `TIMER_CLOCK_HZ=54000000`.

Only `CNTVCT_EL0` scaled by the generated frequency may support elapsed-time
proof. The v16 MCS timer-frequency fix does not relax the non-MCS Pi contract
or make dummy timers, `CNTPCT_EL0`, timer-control exports, or retry loops valid
latency evidence.

## Host and external-tree migration

Renaming `~/seL4_15` to `~/seL4_16` is administrative only. Before the renamed
tree is accepted, its manifest repository and every project must match the
table above, its permitted Pi overlay must match the newly authenticated v16
diff, and unrelated dirt must be absent. No claim may derive from the directory
name.

The external `~/seL4_16/.venv_aarch64` and the repository profile environment must be
rebuilt or upgraded against the official v16 project requirements, then
captured as an exact distribution and installed-file closure. Merely moving a
v15 virtual environment leaves absolute interpreter paths and stale package
provenance and is not acceptable. Compiler, Python, and `mkimage` identities
remain separately pinned supply-chain inputs; they change only when fresh v16
configuration/build evidence demonstrates a requirement and the profile
contract records the new identity.

Every accepted build must bind:

- manifest tag and commit plus all repository revisions;
- any permitted source diff and its exact digest;
- compiler executable identities and versions;
- Python interpreter, distribution, lock, and installed-file identities;
- wrapper, profile contract, validator, and build commands;
- generated configuration and headers;
- required artifact hashes and completion timestamps; and
- a causal stamp proving the artifacts were produced after the accepted
  inputs.

## CAmkES 3.13.0 companion release

[CAmkES 3.13.0](https://docs.sel4.systems/releases/camkes/camkes-3.13.0.html)
is the ecosystem companion release for seL4 16.0.0. The official
`camkes-manifest` lightweight tag `camkes-3.13.0` is commit
`125cc7823f510b7e0f7e86c286695df138bdf1a5`; the `camkes-tool` annotated tag
object is `495c544a2144d1bd9f41e20998d4ff07cd688e08`, peeled to
`00965e755c918da493222f36886b8743a4c1b152`.

The release adds GCC 14 and Python 3.10-or-newer support, concurrent unit-test
support, and domain-schedule units in ticks or microseconds, with no special
upgrade requirement. It is a compatibility reference only. Milestone 26d
explicitly excludes CAmkES adoption, so neither CAmkES tag is a Cohesix source
input, no CAmkES-generated component becomes part of the VM, and its
domain-schedule feature must not reintroduce `KernelDomainSchedule` into
one-domain Cohesix profiles.

A separate macOS companion smoke on 2026-07-23 configured the CAmkES `adder`
example against seL4 16.0.0 with Arm GNU Toolchain 15.2.1, generated the
component ELFs and capDL inputs, and built the pinned `parse-capDL` tool. Ninja
then stopped at step 234 of 289 because macOS BSD `cpio` rejects the upstream
GNU-style `--append --owner=+0:+0` invocation. Simulation was therefore not
run. The smoke checkout remains isolated below
`out/sel4/camkes-3.13-smoke`; this host-tool incompatibility does not enter the
Cohesix profile, target, or TCB and is not a seL4 16 Cohesix build failure.

## Evidence-class boundary

| Evidence class | What it can establish | What it cannot establish |
| --- | --- | --- |
| Upstream provenance | Official tag and complete repository revision identity | A configured or compiled Cohesix profile |
| Static profile build | Fresh source/configuration/toolchain/artifact agreement for one named lane | Linked Cohesix execution, another target, or live hardware |
| Linked QEMU image | The exact QEMU kernel/root-task image reached observed behavior | Pi image, board, Wi-Fi, benchmark, or formal-proof evidence |
| Sealed Pi image | Image composition, wrapper, readback, and exact identity | A current boot or any live device result |
| Fresh Pi boot | The read-back-bound image booted on the named board | Wi-Fi repeatability, TCP/`cohsh`, operator liveness, or benchmarks unless separately exercised |
| Live Pi gates | Only the exact named Wi-Fi, USB/local-seat, TCP/`cohsh`, operator, or benchmark gate | Other live gates or general kernel verification |
| Proof eligibility | The pristine v16 source/config matches an upstream verified-configuration entry condition | Verification of Cohesix userspace, Rust bindings, boot, SMP operation, DMA, devices, timing, or the shipping image |

The v16 caveats list BCM2711 in the AArch64 functional-correctness
configuration family, but that proof assumes AArch64 hypervisor mode and FPU
and currently establishes integrity rather than confidentiality or
non-interference. Cohesix operational SMP profiles remain unverified. The
proof-eligibility lane must therefore retain its narrow name and cannot be
promoted into a Cohesix proof claim.

## Acceptance record

The 2026-07-23 offline refresh established these bounded results:

| Gate | Result |
| --- | --- |
| Complete source | `~/seL4_16` contains every project from the official seL4Test 16.0.0 manifest at the revisions above. `~/seL4_15` is absent. The displaced kernel-only tree is recoverable at `~/seL4_16.kernel-only-pre-full-project-20260723`. |
| Pi source policy | The only project diff is the authenticated Pi overlay with digest `3c82bf606f76823398f91070c9787d13aa137e7e7c2665a01db11df9797f69a6`. |
| Python environment | `~/seL4_16/.venv_aarch64` was recreated with Python 3.13.7 from the two hash-locked requirement sets; `pip check`, required imports, and a scan for embedded `~/seL4_15` paths passed. |
| Five canonical profiles | Fresh QEMU production/diagnostic, Pi production/diagnostic, and BCM2711 proof-eligibility builds passed the aggregate source-and-artifact validator. Evidence is `out/audit/m26d-profile-v2-all-sel4-16.json`. |
| Repo-managed reference trees | `seL4/build`, `seL4/SMP_build`, and `seL4/build_UBOOT` now mirror the completed v16 QEMU production, QEMU diagnostic, and Pi diagnostic profile outputs respectively. The old memoization cache was removed so no v15 object is retained. |
| Direct external-source build | A fresh `pi4_diagnostic` build from `~/seL4_16` completed 309 of 309 steps and passed runtime validation. Evidence is `out/audit/m26d-home-sel4-16-pi4-diagnostic.json`. |
| Cohesix compatibility | `sel4-sys`, `sel4-runtime`, the Pi driver ABI/runtime tests, AArch64 driver-runtime check, QEMU/Pi root-task target checks, generated-artifact guard, and a linked QEMU `--no-run` package build passed against the v16 profiles. |

These passes close source, static-profile, and offline link compatibility only.
The non-destructive exact Pi staging attempt was correctly refused because the
shared Cohesix checkout already contained tracked and untracked work; that
state was preserved rather than hidden or stashed. A clean-worktree exact
image, booted target-qualified QEMU Test Plan, Pi media readback, fresh board
boot, CYW43 repeatability, TCP/`cohsh`, operator-liveness, and refreshed
performance gates remain open. No v15 result, path rename, upstream CAmkES
smoke, or offline PASS may fill those evidence classes.

## Current Pi artifact consumption

The historical refresh record above remains the source-build provenance for the
tracked bytes. The current Pi exact-image lane consumes
`seL4/build_UBOOT` as the immutable canonical `pi4_diagnostic` artifact input.
`scripts/sel4_profile.py validate --repo-managed` requires that exact tracked
path, a clean subtree, the completed v2 build-input stamp, the current contract
hash, and matching relocated configuration and artifact identities. It does not
claim that the historical absolute source, CMake, or build paths still exist.

Image composition must not invoke CMake or Ninja in `seL4/build_UBOOT`, repair
or re-stamp it, or select a seL4 source/build input from `out/`. Instead, the
image wrapper verifies its relink tool family by reproducing the tracked
baseline elfloader byte-for-byte, then creates the new rootserver archive,
elfloader wrapper, staged image, and provenance only in disposable output
directories. This consumption proves diagnostic artifact identity and linked
image inputs only. It remains ineligible for seL4 release, fresh source-build,
media read-back, boot, Wi-Fi, TCP, or benchmark claims.
