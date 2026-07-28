<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record the seL4 16 identity and profile mapping of repository-managed build artifacts. -->
<!-- Author: Lukas Bower -->

# Repository seL4 16 Build Artifacts

This directory contains repository-managed seL4 16.0.0 build artifacts.
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

The tracked trees are byte-for-byte mirrors of successful profile builds whose
historical CMake paths remain recorded in their causal stamps. The Pi image
lane consumes `seL4/build_UBOOT` as its immutable canonical
`pi4_diagnostic` input and validates each relocated artifact by digest. It must
never run CMake or Ninja in that tree. Rootserver replacement, elfloader
relinking, image wrapping, and evidence generation happen only in disposable
output directories.

The repository-managed Pi tree proves the accepted diagnostic kernel,
elfloader, generated configuration, and ABI inputs. It is not a release build,
fresh source-build proof, staged/read-back media proof, boot proof, Wi-Fi proof,
or TCP proof. Paths below `out/` are derived output and evidence only; the Pi
lane must not require a seL4 source or build input from `out/`.

The former tracked `.sel4_cache` is intentionally absent. All seL4 16 profiles
disable binary memoization so cached seL4 15 objects cannot enter refreshed
evidence.
