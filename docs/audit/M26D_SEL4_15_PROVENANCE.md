<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record Milestone 26d seL4 15 source, toolchain, profile-build, and evidence-class provenance. -->
<!-- Author: Lukas Bower -->

# Milestone 26d seL4 15 Provenance

## Scope and authorization

This ledger records the static source, configuration, toolchain, and artifact
closure authorized by Milestone 26d tasks
`m26d-kernel-provenance-refresh`,
`m26d-domain-schedule-debt-removal`,
`m26d-canonical-sel4-profile-closure`,
`m26d-isolated-runtime-wx-closure`,
`m26d-worker-execution-truth-repair`, and
`m26d-sel4-capability-utilization-audit`.

It deliberately keeps these evidence classes separate:

- canonical seL4 source/profile/artifact validation;
- a linked Cohesix QEMU image and QEMU boot;
- a sealed, read-back-bound Pi image and current-image boot;
- Pi Wi-Fi, USB/local-seat, TCP/`cohsh`, and benchmark proof; and
- upstream proof-configuration eligibility.

A PASS in the first or last class does not satisfy any intervening live class.

## Accepted upstream source

The build input is the complete tag-pinned seL4Test project, not an isolated
kernel checkout:

| Input | Accepted identity |
| --- | --- |
| Manifest | `https://github.com/seL4/sel4test-manifest.git` |
| Manifest ref | `refs/tags/15.0.0` |
| Manifest commit | `367a780d5f3f1b59711618b3b321666cecb2f8a2` |
| seL4 kernel commit | `881de507fe528490dc5e570c7810a149bad5880f` |
| Component revisions | Every project revision pinned in `configs/sel4/profiles.toml` |
| Wrapper | `tools/sel4-profile-project/CMakeLists.txt` |

The wrapper consumes the pinned upstream project without introducing the
obsolete seL4Test `KernelDomainSchedule` input. The validator rejects that
cache key even when its value is empty.

Pi operational profiles permit one authenticated source change: the VL805
high-BAR device-untyped overlay at
`configs/sel4/patches/bcm2711-vl805-device-untyped.patch`. Its canonical raw
Git-diff SHA-256 is
`3c82bf606f76823398f91070c9787d13aa137e7e7c2665a01db11df9797f69a6`.
`prepare-source` applies it only to a pristine, revision-complete checkout;
`validate` compares the live diff byte-for-byte and never mutates source.
QEMU and proof-eligibility profiles require a pristine source set.

The authenticated diff adds the required Lukas Bower author, purpose, and 2026
copyright metadata in the patched DTS file's native comment header. The raw
patch therefore remains byte-for-byte authenticated without a charter
exception.

The normative interface reference is the
[seL4 Reference Manual 15.0.0](https://sel4.systems/Info/Docs/seL4-manual-15.0.0.pdf).
The tracked `seL4/seL4-manual-latest.pdf` and
`seL4/seL4-manual-latest.md` are the corresponding carried references.

## Immutable host supply chain

The profile contract and setup script bind the following host inputs:

| Input | Accepted identity |
| --- | --- |
| AArch64 compiler | Official Arm GNU Toolchain `15.2.Rel1` macOS Arm64 archive; GCC `15.2.1`; target `aarch64-none-elf` |
| Compiler archive | `arm-gnu-toolchain-15.2.rel1-darwin-arm64-aarch64-none-elf.tar.xz`; size `80863820`; SHA-256 `37084c99bc05fda43a6c48900c638ae4fd6d93e2287ceb3e9bcda55437f1aadd` |
| Compiler programs | Exact SHA-256 identities for GCC, G++, CPP, assembler, linker, objcopy, ar, and ranlib in `configs/sel4/profiles.toml` |
| Python | Dedicated `out/toolchain/sel4-profile-venv`; exact 38-distribution closure |
| Python bootstrap lock | SHA-256 `a582a400e97b0b830952482a8923c4097024c9960fe2197315c341ca00a9548a` |
| Python build lock | SHA-256 `5ef6b3b1e5edc912a0041417b7386ad17a57fab6ecd3b64c797ecbb3e37ea929` |
| Pi packaging source | Official DENX `u-boot-2026.01.tar.bz2`; size `34172789`; SHA-256 `b60d5865cefdbc75da8da4156c56c458e00de75a49b80c1a2e58a96e30ad0d54` |
| U-Boot identity | Release `2026.01`; reference commit `127a42c7257a6ffbbd1575ed1cbaa8f5408a44b3` |
| Built `mkimage` | SHA-256 `b40094c4a5f7174afa3439fe48a3e14d2e80a6e5e6169bcafc8ce79dd583f3e1`; version `mkimage version 2026.01` |

The final Python closure pins `setuptools==80.9.0`. The first fresh wrapper
attempt exposed that upstream nanopb imports `pkg_resources`, which is absent
from the initially selected setuptools 83 environment. Setup now verifies that
import explicitly. The failed attempt was retained rather than relabelled.

`toolchain/setup_macos_arm64.sh` verifies archive sizes and hashes before
extraction, verifies installed executable identities and versions, builds
`mkimage` from the pinned release archive, and emits provenance records bound
to the setup script and profile contract. Profile validation rechecks the live
compiler programs, complete Python distribution and installed-file closure,
both lock digests, and the `mkimage` source and binary records.

## Canonical profile-v2 results

On 2026-07-17, all five fresh canonical defaults built and passed the final
validator:

| Profile | Canonical build directory | Claim class | Individual evidence |
| --- | --- | --- | --- |
| `qemu_smp_production` | `out/sel4/profile-v2/qemu-smp-production` | Static release-profile eligibility; no boot claim | `out/audit/m26d-profile-v2-qemu-smp-production.json` |
| `qemu_smp_diagnostic` | `out/sel4/profile-v2/qemu-smp-diagnostic` | Static diagnostic/runtime profile; no boot claim | `out/audit/m26d-profile-v2-qemu-smp-diagnostic.json` |
| `pi4_production` | `out/sel4/profile-v2/pi4-production` | Static release-profile eligibility; no exact-image or board claim | `out/audit/m26d-profile-v2-pi4-production.json` |
| `pi4_diagnostic` | `out/sel4/profile-v2/pi4-diagnostic` | Static diagnostic profile; does not replace the CYW43 tree | `out/audit/m26d-profile-v2-pi4-diagnostic.json` |
| `bcm2711_proof_eligibility` | `out/sel4/profile-v2/bcm2711-proof-eligibility` | Upstream verified-configuration compatibility only | `out/audit/m26d-profile-v2-bcm2711-proof-eligibility.json` |

The final aggregate evidence is
`out/audit/m26d-profile-v2-all.json`, schema
`cohesix-sel4-profile-evidence-set/v2`. It records
`valid=true`, `failed_profiles=[]`, source and artifact requirements
enabled, and all five profile records valid with empty error lists.

QEMU build, release, regression, staged-test, smoke, and newcomer entrypoints
now default to `out/sel4/profile-v2/qemu-smp-production`. The normal build path
validates runtime intent, and the release path validates release intent before
consuming the selected tree. Explicit alternate trees are claim-ineligible
unless a named contract passes. Default-consumer integration alone does not
create linked-image or boot evidence; the separate target-qualified Test Plan
below supplies that runtime class.

The first integrated default-consumer run exposed that the release-optimized
seL4Test placeholder left only 1,089,536 bytes in the linked elfloader CPIO,
while the boot-minimized Cohesix root task required a 2,548,964-byte rebuilt
archive. The rewrite helper correctly refused to grow a linked ELF in place.
At M26d acceptance, the production contract reserved 2,097,152 file-backed
placeholder bytes and required at least 3,145,728 bytes of validated archive
capacity. The reserve was discarded when the placeholder was replaced; the
bounded rewrite and rootfs size guard remained fail-closed. The fresh
production wrapper exposed 3,186,688 bytes of linked archive capacity. The
integrated default-consumer build then
rewrote it to 2,548,964 bytes, stripped the 755,528-byte boot-only rootserver
ELF, passed the rootfs size guard, selected GICv3, and completed its `--no-run`
packaging path successfully.

Milestone 26e retains the exact 4,393,984-byte MCS driver archive inside the
rootserver. The current QEMU policy therefore reserves 7 MiB and requires an
8 MiB minimum elfloader archive; replacement and rootfs guards remain separate
and fail closed.

## Current canonical QEMU runtime evidence

The target-qualified QEMU Test Plan at
`out/test-plan/m26d-qemu-sel4-15-gap-audit` passed stages 1 through 5 on
2026-07-17 local time. Its Stage 03 and Stage 04 logs independently validate
`qemu_smp_production`, record `virt,gic-version=3`, boot the linked Cohesix
image to `Cohesix console ready`, complete authenticated TCP handshakes, and
exercise the live console and REST projection.

Stage 03 passed all 18 `.coh` scripts across fresh QEMU processes, including
the base, telemetry, 1,000-worker shard, and gated-policy groups. Stage 04
passed the REST core and parity batches plus the Python client smoke test.
Stage 05 passed the complete due-diligence gate using the Stage 03 evidence,
including workspace tests, dependency/advisory policy, risk ratchet,
generated-artifact drift, hard-coded-secret scan, and release guardrails.
The `stage_01.qemu.done` through `stage_05.qemu.done` markers and per-stage
input hashes bind the accepted run. This is current QEMU runtime evidence; it
does not satisfy any Pi image, board, Wi-Fi, local-seat, or benchmark class.

The separately coordinated external Pi diagnostic input at
`/Users/lukasbower/seL4/build_UBOOT` was previously rebuilt under the same
profile contract. Tightening the validator to reject unknown evidence classes
and class/eligibility mismatches changed a causal build input. Current evidence
at `out/audit/m26d-pi4-diagnostic-build-uboot.json` therefore records
`valid=false` with the expected stale build-input-stamp error. The tree was not
rebuilt here because it is the active exact-image/CYW43 input; its owning lane
must refresh it from an empty path before the next image build. The last
accepted pre-guard identities, retained only to identify the superseded input,
were:

| External Pi input | SHA-256 |
| --- | --- |
| Completed build-input stamp | `8ffb262930ae5925d73c1d0e68c1e3a7dcf0f94596f9d5900ed1b44477bd3acb` |
| Kernel ELF | `28703ba33f52ab8127bda714998f0d9fb036c8fd76c61a3bd2c838bea087cf88` |
| Rootserver ELF | `f97b86c2db3ce2a981aa7ccb8a0c8a9af2f052becc07dd23d29b4ed982ac9db5` |
| Elfloader ELF | `95831566e8678b5be2b3363d58164b61115f2444aaaf46297ba956dd1ac062a3` |
| U-Boot-wrapped seL4Test profile image | `b48506c6e91de207924c91978b0ecd61d97238ddbdf6287f6bf66c54f1e78680` |
| Kernel DTB | `b46ea949e8b837aa218a50d6b6482ac13e1c8bc9c269d04b1a90dfe1d1e95897` |

The superseded external tree has `KernelArmExportVCNTUser=ON`, the
physical-counter and EL0 timer exports OFF, and generated
`TIMER_CLOCK_HZ=54000000`. Those settings remain useful diagnostic identity,
but the stale causal stamp makes the tree claim-ineligible until the CYW43 lane
rebuilds and revalidates it. It is not itself a Cohesix root-task image, board
boot, or Wi-Fi result.

The superseded pass was bound to:

- profile contract SHA-256
  `6d765257df7d8c764f3ddf02bbcd3d1d592cf2b801558a56cc193ade31fd2f1e`;
- validator SHA-256
  `3ae3de20809c8ac8292a1b239aa4e0da3ad08a81d2428f8a66d60f0d324a3ba6`.

The current validator SHA-256 is
`3b95da8aaf06aaecfd693df0aa54477eaaacc78e1604c78d282e58f44e40d76f`;
the mismatch is intentional and fail-closed. The Python bootstrap/build lock
SHA-256 values remain recorded above.

The focused profile suite passes `91` tests. Coverage includes source and
patch provenance, compiler and Python tampering, packaging-tool provenance,
fresh-tree and causal-stamp enforcement, disabled memoization, generated
configuration, independent DTS/DTB GICv3 and PSCI parsing, rootservers-last Pi
memory-map generation, QEMU rootserver-archive capacity, strict AArch64
`ET_EXEC`/entry/load validation, and shipping versus non-shipping RWX policy.

The final fresh builds completed 301/301 targets for the production QEMU
wrapper, including its bounded reserve object; 300/300 for the diagnostic QEMU
wrapper; 308/308 for each canonical Pi wrapper; and 31/31 plus the
preprocessed-kernel target for the proof-eligibility lane. The external Pi
tree's earlier 308/308 build predates the current validator and is not current
closure evidence.

Every configuration starts in an empty directory. A pending, missing, copied,
path-moved, no-op, re-stamped, or causally stale artifact set is invalid.
Configure and verified-config builds disable both seL4 cache variables:
`SEL4_CACHE_DIR` and `MEMOIZE_CACHE_DIR`. The completion record binds the
source revisions, contract, validator, wrapper, build commands, toolchain,
configuration, and required artifact hashes.

The upstream seL4Test kernel, rootserver, elfloader, and combined image contain
RWX load segments. The evidence records the explicit
`upstream-sel4test-rwx-load` exception and marks that artifact set
non-shipping and not a Cohesix system image. A shipping-eligible artifact is
fail-closed on RWX. This classification is independent of the Cohesix isolated
runtime W^X loader, which now rejects effective W+X pages and removes executable
frames' writable root aliases before child resume.

## Commands and observed results

```bash
./toolchain/setup_macos_arm64.sh

out/toolchain/sel4-profile-venv/bin/python -m pytest -q \
  tests/test_sel4_profile.py
# 91 passed

out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate \
  --profile qemu_smp_production \
  --source out/sel4/v15-worktree-project \
  --build-dir out/sel4/profile-v2/qemu-smp-production \
  --require-source --require-artifacts --for-release \
  --evidence out/audit/m26d-profile-v2-qemu-smp-production.json

out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate \
  --profile qemu_smp_diagnostic \
  --source out/sel4/v15-worktree-project \
  --build-dir out/sel4/profile-v2/qemu-smp-diagnostic \
  --require-source --require-artifacts --for-runtime \
  --evidence out/audit/m26d-profile-v2-qemu-smp-diagnostic.json

out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate \
  --profile pi4_production \
  --source out/sel4/v15-pi4-project \
  --build-dir out/sel4/profile-v2/pi4-production \
  --require-source --require-artifacts --for-release \
  --evidence out/audit/m26d-profile-v2-pi4-production.json

out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate \
  --profile pi4_diagnostic \
  --source out/sel4/v15-pi4-project \
  --build-dir out/sel4/profile-v2/pi4-diagnostic \
  --require-source --require-artifacts --for-runtime \
  --evidence out/audit/m26d-profile-v2-pi4-diagnostic.json

out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate \
  --profile bcm2711_proof_eligibility \
  --source out/sel4/v15-worktree-project \
  --build-dir out/sel4/profile-v2/bcm2711-proof-eligibility \
  --require-source --require-artifacts \
  --evidence out/audit/m26d-profile-v2-bcm2711-proof-eligibility.json

out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate \
  --profile pi4_diagnostic \
  --source out/sel4/v15-pi4-project \
  --build-dir /Users/lukasbower/seL4/build_UBOOT \
  --require-source --require-artifacts --for-runtime \
  --evidence out/audit/m26d-pi4-diagnostic-build-uboot.json
# Expected until the CYW43 lane performs a coordinated fresh rebuild:
# ERROR: profile build-input stamp does not match current source, commands,
# configuration, tools, or artifacts

out/toolchain/sel4-profile-venv/bin/python scripts/sel4_profile.py validate \
  --all --require-source --require-artifacts \
  --evidence out/audit/m26d-profile-v2-all.json
# PASS profiles=5

scripts/ci/test_plan_run.sh --target qemu \
  --state-dir out/test-plan/m26d-qemu-sel4-15-gap-audit
# PASS stages 1 2 3 4 5; Stage 03 passed 18 scripts
```

## Preserved, claim-ineligible trees

The following are retained for diagnosis or historical identity and are not
canonical aggregate defaults:

| Tree | Status |
| --- | --- |
| `seL4/build` | Historical single-core/GICv2 tree from the older seL4Test project |
| `seL4/SMP_build` | Historical SMP/GICv2 comparison evidence; explicit diagnostic input only, not a publication/runtime default |
| `seL4/build_UBOOT` | Historical tracked Pi build evidence; not the active exact-image input |
| `/Users/lukasbower/seL4/build_UBOOT` | External Pi exact-image/CYW43 tree; separately owned, causally stale after the validator guard change, and pending coordinated rebuild |
| `out/sel4/profile-v2/*.pre-final-frozen-20260716T144348Z` | Five preserved defaults that predate the final causal rebuild |
| `/Users/lukasbower/seL4/build_UBOOT.pre-final-frozen-20260716T144348Z` | Preserved external tree that predates the final causal rebuild |
| `out/sel4/profile-v2/*.failed-setuptools83` | Preserved first attempts that failed before completing the Python build-tool contract |
| `out/sel4/profile-v2/*.pre-memoize-fix` | Preserved successful builds bound to the superseded validator before both memoization variables were disabled |

The failed and superseded siblings cannot satisfy default-path validation and
must not be renamed over a canonical tree.

## Historical runtime evidence

Earlier Milestone 26d QEMU and Pi observations remain useful only with their
original artifact identities:

- `out/regression-logs/m26d-qemu-base` and
  `out/regression-logs/m26d-qemu-remaining` record the earlier
  `seL4/SMP_build` TCP regression runs;
- `out/bench/m26d-qemu-sel4-15-*.log` records earlier QEMU REST runs;
- `/Users/lukasbower/pi4-serial-20260629-220122.log`,
  `/Users/lukasbower/tcpdump-wifi-20260629-202452.pcap`, and
  `out/test-plan/m26d-pi4/pi4-runtime-dma-proof-20260629-220122.env`
  record the accepted historical Pi boot/network/runtime-DMA slice;
- the later saved-policy Wi-Fi cycle evidence remained non-repeatable, while
  the corrected GENET saved-policy cycle evidence passed 10/10.

Those results do not upgrade the new static profile-v2 artifacts to live
runtime evidence and do not establish current-image Wi-Fi reliability. The
current QEMU Test Plan is recorded separately above and does not upgrade any
historical Pi artifact. The
parallel CYW43 lane owns the required fresh external-tree rebuild, Cohesix
exact-image identity,
10-cold/10-warm repeatability, current-boot serial/pcap pairing, raw
TCP/`cohsh`, and refreshed Pi benchmark proof. This profile work did not
rewrite that state machine or its evidence classifiers.

## Current closure boundary

The canonical seL4 15 profile, provenance, and default-consumer gate is
complete:

- all five fresh defaults pass source and artifact validation;
- QEMU defaults select GICv3 consistently through source, generated config,
  both parsed DTBs, and launcher detection;
- the linked canonical GICv3 QEMU image passes the target-qualified five-stage
  Test Plan, authenticated TCP console regression, and REST projection checks;
- the production wrapper carries a contract-bound, validated archive reserve
  large enough for the current boot-minimized Cohesix root task;
- one-domain builds contain no legacy domain-schedule input;
- the external Pi diagnostic tree is fail-closed as causally stale after the
  evidence-class guard change and remains a coordinated CYW43-lane rebuild
  gate;
- production, diagnostic, and proof-eligibility claims remain distinct;
- evidence classes have fixed release/runtime eligibility and entrypoints
  validate the applicable intent before claims;
- compiler, Python, `mkimage`, source, build, and artifact identities are
  fail-closed.

Milestone 26d remains **In Progress**. The profile result does not close:

- a sealed/read-back-bound current Pi image and fresh board boot;
- a fresh, current-validator rebuild of the external Pi exact-image input;
- CYW43 10-cold/10-warm repeatability, Wi-Fi DPC, TCP/`cohsh`, and operator
  gates;
- refreshed target-qualified benchmark evidence.

Proof-eligibility output means only that the pristine upstream
source/configuration matches that profile's entry conditions. It is not formal
verification of Cohesix userspace, its boot chain, the operational Pi overlay,
DMA isolation, timing, or hardware behavior.
