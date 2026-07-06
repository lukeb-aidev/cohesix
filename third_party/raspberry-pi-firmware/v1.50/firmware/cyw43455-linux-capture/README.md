# Author: Lukas Bower
# Purpose: Document the checked-in Pi 4 CYW43455 firmware bundle used by release builds.
# Copyright 2026 Lukas Bower

# CYW43455 Linux-Firmware Pi 4B Bundle

This directory is the default Pi 4 Wi-Fi firmware source for Cohesix release
builds. It is intentionally under `third_party/raspberry-pi-firmware` so the
build remains valid after deleting generated `out/` artifacts.

The files match the pinned upstream `linux-firmware` Pi 4 Model B CYW43455
identity required by
`apps/root-task/build.rs`:

- `cyfmac43455-sdio.bin`: `643651` bytes,
  `d408faa9d0d5b1a2f9912dcea53ab0be48217288e398406d117f0edafe7c3edd`
- `brcmfmac43455-sdio.raspberrypi,4-model-b.txt`: `1883` bytes,
  `edb6f4e4fb19e18940004124feb4ffe160d72fc607243a07a4480338a28b2748`
- `cyfmac43455-sdio.clm_blob`: `4733` bytes,
  `15f50a27020b263d1bea215c8f68d0550d912932d1d9ef19ffd59f18d82dd460`

The source is `linux-firmware` commit
`c95059a3774b1164a2d6a4db5371fb8406b22692`. Linux exposes the binary and CLM
through the `brcm/brcmfmac43455-sdio.*` aliases while storing the payloads under
`cypress/cyfmac43455-sdio.*`; the Pi 4B board NVRAM remains
`brcm/brcmfmac43455-sdio.raspberrypi,4-model-b.txt`.
