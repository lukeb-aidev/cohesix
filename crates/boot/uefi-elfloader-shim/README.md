<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Documents the uefi-elfloader-shim crate scope and usage. -->
<!-- Author: Lukas Bower -->
# UEFI Elfloader Shim

This helper crate does not expose runtime code. It documents how the
seL4 elfloader is repackaged as a UEFI application for the Cohesix
``Adapter B1`` boot flow and keeps build tooling colocated with the cargo
workspace.

The shim summarises the expected ESP layout:

```
ESP/
  EFI/BOOT/BOOTAA64.EFI   # elfloader built as a UEFI application
  cohesix/kernel.elf      # seL4 kernel payload
  cohesix/rootserver      # Cohesix root-task binary
  cohesix/initrd.cpio     # optional initrd bundle
  startup.nsh             # shell script invoking BOOTAA64.EFI
```

The repository now ships a deterministic UEFI ESP packaging helper in
`scripts/uefi/esp-build.sh`. This shim remains documentation-only; the
authoritative plan for future platform-specific UEFI work lives in
`docs/BUILD_PLAN.md`, while the current reference boot flows are documented in
`docs/BOOT_REFERENCE.md` and `docs/AWS_AMI.md`.
