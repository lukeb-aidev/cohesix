<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record the seL4 16 identity and profile mapping of the repository-managed reference artifacts. -->
<!-- Author: Lukas Bower -->

# Repository seL4 16 Reference Artifacts

This directory contains repository-managed seL4 16.0.0 reference artifacts.
Upstream source remains external and is never vendored. Exact source revisions,
toolchain identities, and acceptance boundaries are recorded in
`configs/sel4/profiles.toml` and
`docs/audit/M26D_SEL4_16_PROVENANCE.md`.

The 2026-07-23 refresh maps the tracked build subdirectories to fresh,
validated profile outputs as follows:

| Repository path | seL4 16 source profile |
| --- | --- |
| `seL4/build` | `qemu_smp_production` |
| `seL4/SMP_build` | `qemu_smp_diagnostic` |
| `seL4/build_UBOOT` | `pi4_diagnostic` |
| `seL4/seL4-manual-latest.pdf` and `.md` | Official seL4 Reference Manual 16.0.0 |
| `seL4/elfloader.md` | Elfloader documentation from the pinned seL4Test 16.0.0 `tools/seL4` revision |

The build trees are byte-for-byte mirrors of the successful profile builds
under `out/sel4/profile-v2`. Their embedded CMake paths and causal build stamps
therefore identify those original build locations. Treat the tracked copies as
reviewable reference artifacts, not as relocated fresh-build or runtime proof.
For a claim, select and validate the corresponding `out/sel4/profile-v2`
directory, then retain target-qualified evidence.

The former tracked `.sel4_cache` is intentionally absent. All seL4 16 profiles
disable binary memoization so cached seL4 15 objects cannot enter refreshed
evidence.
