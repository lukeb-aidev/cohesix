<!-- Author: Lukas Bower -->
# Cohesix Boot Adapters

Cohesix currently supports two boot adapters for the AArch64 `virt`
platform. Both ultimately launch the same seL4 kernel and root-task
payloads while preserving the existing debug console behaviour.

## Adapter A — Direct elfloader

The legacy flow launches QEMU with the seL4 elfloader provided as a raw
ELF image. The loader, kernel, and root-task binaries are injected using
QEMU's `-kernel`, `-initrd`, and `-device loader` arguments. This path
remains untouched and is still invoked through `scripts/qemu-run.sh`
with explicit artefact paths:

```
scripts/qemu-run.sh \
  --elfloader out/elfloader \
  --kernel out/kernel.elf \
  --root-task target/aarch64-unknown-none/release/root-task \
  --out-dir out/qemu-direct
```

## Adapter B1 — UEFI elfloader

Adapter B1 introduces a UEFI-compatible build of the elfloader that boots
through the EDK2 firmware (`QEMU_EFI.fd`). The build process generates a
raw FAT32 ESP image containing both the firmware entry point and the
Cohesix payloads:

- `\EFI\BOOT\BOOTAA64.EFI` — elfloader compiled as an EFI application.
- `\cohesix\kernel.elf` — seL4 kernel image.
- `\cohesix\rootserver` — Cohesix root-task binary.
- `\cohesix\initrd.cpio` — optional initrd payload.
- `startup.nsh` — shell script that launches `BOOTAA64.EFI` automatically.

Use the helper script to assemble the ESP image on macOS:

```
make esp
```

Then boot QEMU with the bundled UEFI firmware. The helper selects a
suitable `QEMU_EFI.fd` from common Homebrew locations or honours the
`QEMU_FIRM` override.

```
make run-uefi
```

Both adapters converge on the same Cohesix root-task binary, which
remains `no_std`, preserves the dual-mode host entry point, and performs
all early console I/O via the minimal `Platform` trait.
