// CLASSIFICATION: COMMUNITY
// Filename: BUILD_NOTES.md v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

# EFI Kernel Build Overview

This document summarizes the expected flow for building the Cohesix EFI kernel and packaging it into the ISO.

1. Run `init-build.sh -DKernelUEFI=TRUE` inside the seL4 workspace to create `build_uefi/`.
2. Execute `ninja` in that directory to produce `kernel.efi`.
3. Copy `kernel.efi` to `out/bin/kernel.efi` in the root of this repository.
4. `tools/make_iso.sh` then stages `kernel.efi` into `/boot/kernel.efi` and `/EFI/BOOT/BOOTAA64.EFI` or `BOOTX64.EFI` depending on architecture.
5. The ISO creation step uses `xorriso` with `-efi-boot` so that the UEFI firmware loads `kernel.efi` directly.

These steps are enforced and verified by `cohesix_fetch_build.sh`.
