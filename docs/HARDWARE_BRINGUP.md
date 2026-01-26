<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document current hardware bring-up status and constraints. -->
<!-- Author: Lukas Bower -->
# Hardware Bring-up (Status)

## Current status (as-built)
- Cohesix is validated on QEMU `aarch64/virt` only.
- There is **no** in-repo UEFI bring-up script or packaging flow at this time.
- The QEMU reference boot transcript and invariants are documented in
  `docs/BOOT_REFERENCE.md`.

## UEFI bring-up (planned)
UEFI bring-up is planned under Milestone 25a in `docs/BUILD_PLAN.md`. Until that
milestone lands, any UEFI artifacts, packaging steps, or TPM/DICE attestation
hooks are **not** part of the as-built system.

## Reference usage (QEMU)
Use the existing QEMU harness described in `docs/QUICKSTART.md` and
`docs/USERLAND_AND_CLI.md` for the authoritative dev/CI workflow.
