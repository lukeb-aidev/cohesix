<!-- Copyright © 2025 Lukas Bower -->
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

Refer to `docs/BOOT_PATHS.md` plus the `esp-build.sh` helper script for
assembly and packaging details.
