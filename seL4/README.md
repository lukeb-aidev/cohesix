<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record the seL4 17 identity and profile mapping of repository-managed build artifacts. -->
<!-- Author: Lukas Bower -->

# Repository seL4 17 build artifacts

This directory contains mirrors of successful seL4Test 17.0.0 profile builds.
The seL4 source stays external. The complete manifest and kernel revisions,
compiler identity, configuration and Pi operational overlay are pinned in
`configs/sel4/profiles.toml`. The upgrade record is
`docs/audit/M26D1_SEL4_17_PI_NETWORK.md`.

| Repository path | seL4 17 source profile |
| --- | --- |
| `seL4/build` | `qemu_smp_production`: four cores, MCS, release kernel |
| `seL4/SMP_build` | `qemu_smp_diagnostic`: four cores, MCS, debug kernel and printing |
| `seL4/build_UBOOT` | `pi4_production`: four cores, MCS, release kernel and the pinned initial-SC/VL805 overlay |
| `seL4/seL4-manual-latest.pdf` and `.md` | Official seL4 Reference Manual 17.0.0 and its text extraction |
| `seL4/elfloader.md` | Elfloader documentation from the pinned seL4Test 17.0.0 `tools/seL4` revision |

The official manifest is `655b1f2a67de058571b5f653688aa55440745192`; the
kernel is `62e345c240a15671484530b4e6e069ce266c28d7`. Both QEMU snapshots
come from pristine v17 source. The Pi snapshot includes only the exact
operational overlay declared in the source contract. All three use the
maintained profile builder and retain their causal configuration and input
stamps. The QEMU profiles export the 24 MHz virtual counter; Pi exports the
54 MHz virtual counter. The production kernels disable debug checks and
kernel printing.

These are immutable input/reference trees. Their CMake paths identify the
original successful builds; never run CMake or Ninja in the tracked mirrors.
Configure fresh builds below `out/` through `scripts/sel4_profile.py`.
QEMU's runtime validator checks those original fresh build paths, including its
profile-owned DTB path; these relocated QEMU mirrors are references, not direct
runtime build selections. Use the validated fresh `out/` build with the canonical
QEMU script. Their copied bytes were compared against that validated build.
The canonical Pi image script consumes `seL4/build_UBOOT` directly and verifies
the relocated artifacts by digest. Rootserver replacement, elfloader
relinking and image wrapping happen in disposable output directories.

The PDF is the unchanged official 17.0.0 manual from
`https://sel4.systems/Info/Docs/seL4-manual-17.0.0.pdf`. The Markdown file
contains `pdftotext -layout` output with a provenance header; the upstream
copyright and GPL license are retained. Elfloader documentation likewise
retains its upstream license below the repository provenance header.

Former v16 bytes remain in Git history and in the private upgrade run's
reference archives. `~/seL4_16` and its Python environment remain preserved.
Historical classic-comparator records keep their original source and hashes;
they do not describe these refreshed MCS snapshots. Binary memoization stays
disabled, and the retired `.sel4_cache` remains absent.

Artifact/profile validation proves selected kernel, configuration and ABI
identity. QEMU boot and operations, exact-image Pi network regression, SD
deployment, sustained observation and release acceptance remain separate
evidence lanes. Read the upgrade record and its exact image identities before
reusing a result.
