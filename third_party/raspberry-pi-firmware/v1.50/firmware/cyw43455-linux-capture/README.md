# Author: Lukas Bower
# Purpose: Document the checked-in Pi 4 CYW43455 firmware bundle used by release builds.
# Copyright 2026 Lukas Bower

# CYW43455 Captured Linux Pi 4B Bundle

This directory is the default Pi 4 Wi-Fi firmware source for Cohesix release
builds. It is intentionally under `third_party/raspberry-pi-firmware` so the
build remains valid after deleting generated `out/` artifacts.

The files match the Raspberry Pi OS Pi 4 Model B CYW43455 identity captured
from the known-good Linux oracle rootfs and required by `apps/root-task/build.rs`:

- `cyfmac43455-sdio.bin`: `609309` bytes,
  `d608f866582519c0a28d86db43040f4f1b98dd1d153e72e9752586546b4a36c3`
- `brcmfmac43455-sdio.raspberrypi,4-model-b.txt`: `2074` bytes,
  `ca709be81a78bdb6932936374f39943acbd7af07fae6151011127599a3ce9e3d`
- `cyfmac43455-sdio.clm_blob`: `2676` bytes,
  `9823842cae9fb9a5dd1e5fb31f595516ec7deee341354bef30bb3026eee29cc1`

The source is RPi-Distro `firmware-nonfree` branch `trixie`:

- `debian/config/brcm80211/cypress/cyfmac43455-sdio-standard.bin`
- `debian/config/brcm80211/cypress/cyfmac43455-sdio.clm_blob`
- `debian/config/brcm80211/brcm/brcmfmac43455-sdio.txt`

The captured Linux rootfs exposes the binary and CLM through
`brcm/brcmfmac43455-sdio.*` aliases while storing the payloads under
`cypress/cyfmac43455-sdio.*`. Its Pi 4B NVRAM alias resolves to the generic
`brcmfmac43455-sdio.txt` payload; Cohesix stores those exact bytes under the
Pi 4B filename so the board identity and proof gate stay explicit.
