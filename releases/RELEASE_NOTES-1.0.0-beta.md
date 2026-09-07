<!-- Author: Lukas Bower -->
<!-- Purpose: Define the next release candidate and its publication prerequisites. -->
<!-- Copyright 2026 Lukas Bower -->

# Cohesix 1.0.0-beta

Status: Unpublished candidate; Milestone 26e acceptance and final release
qualification remain prerequisites. This document does not assert they passed.

This release introduces a separate Raspberry Pi 4 SD-image distribution alongside
native macOS Apple Silicon and Linux AArch64 host bundles. Linux host tools and
archive creation run on the selected ARM64 builder, with Jetson as a reference
host. Each host bundle carries its separately qualified QEMU guest: Mac HVF at
24 MHz and the supported Linux KVM profile at 31.25 MHz.

The host suite includes `cas-tool`, `coh`, `cohsh`, `gpu-bridge-host`,
`hive-gateway`, `host-sidecar-bridge`, `host-ticket-agent` and `swarmui`, plus
the target-neutral Cohesix Python wheel and generated QEMU/Pi contracts.
Host GPU execution remains host-side; packaging does not establish Worker,
driver, network, performance or full-system acceptance.

## Candidate artifacts

- `Cohesix-1.0.0-beta-MacOS.tar.gz`
- `Cohesix-1.0.0-beta-linux.tar.gz`
- `Cohesix-1.0.0-beta-Pi4.tar.gz`

All folders and archives are created under `releases/`. The Pi4 archive contains
`image/cohesix-pi4-sd.img`, its SHA-256 sidecar, MBR/FAT32 layout and boot identity
metadata, documentation and an exact file manifest. Cards must meet the recorded
`minimum_target_bytes`; larger cards retain unallocated spare capacity.

## Publication prerequisites

Complete the applicable TEST_PLAN stages and Milestone 26e target, pressure,
repeatability and review gates for the selected commit. Then follow Conditional G
to qualify the extracted Mac and Linux archives and the exact distributed Pi4
image, including media readback, fresh boot, initial network configuration and
authenticated TCP. Retain the resulting release qualification record beside the
archives. Historical releases remain immutable.
