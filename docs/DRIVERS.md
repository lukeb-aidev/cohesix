<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Provide a proven seL4/Pi 4 driver-development methodology and guardrails for Cohesix. -->
<!-- Author: Lukas Bower -->
# Driver Development Guide (seL4 13, Pi 4/aarch64)

This guide exists because Pi 4 driver bring-up is expensive when we discover
methodology edge-to-edge. For Cohesix, driver work must follow a repeatable
evidence ladder:

1. Prove the seL4 capability path.
2. Prove the platform description and address/IRQ truth.
3. Prove the HAL ownership boundary.
4. Prove one minimal polled device path.
5. Add IRQ delivery only after the device-clear path is known.
6. Add DMA only after cache, address, and ownership rules are explicit.
7. Bind the device into Cohesix semantics only after the hardware contract is
   stable.

Do not start at the full driver. Do not debug protocol behavior until the
seL4 resource path, MMIO mapping, IRQ line, DMA range, and reset/power state
are independently proven.

## Methodology Contract

The risk this guide addresses is not a missing register definition. The risk is
that a driver change can appear to make progress while skipping a lower-level
seL4 proof obligation. That creates wasted board time: protocol retries hide an
unproven IRQ, interrupt experiments hide an unproven device-clear path, DMA
debugging hides an unproven bus address, and feature work hides a missing HAL
boundary.

For Pi 4 work, progress means moving one step up the evidence ladder with a
durable proof. A workaround is acceptable only when it preserves the ladder and
narrows the unknown. A workaround that creates a second control path, bypasses
HAL, depends on an undocumented bootloader side effect, or weakens a bounded
failure condition is not progress.

## Scope

This document is scoped to active Pi 4 work in `docs/BUILD_PLAN.md`:

- Milestone 26: `m26-hal-boundary`, local diagnostics, and the U-Boot boot/DTB handoff.
- Milestone 26a: `m26a-bcmgenet-driver` and Pi 4 static IPv4.
- Milestone 26b: DHCP plus the profile-gated CYW43455 Wi-Fi path.

It targets seL4 13 behavior on AArch64/Pi 4 while preserving Cohesix's current
as-built constraints. `docs/BUILD_PLAN.md` currently records a future seL4 15
refresh under Milestone 26d; until that refresh lands, check local generated
headers under `seL4/build/` before assuming an API label, object type, IRQ
constant, or platform layout.

## Source Authority

Use this order when sources disagree:

1. Local generated seL4 build artifacts: `seL4/build/kernel/gen_config/*`,
   `seL4/build/kernel/gen_headers/*`, `seL4/build/libsel4/include/*`,
   and `seL4/build/kernel/generated/invocations_all.json`.
2. Cohesix manifest IR and generated artifacts: `configs/root_task*.toml`,
   `apps/root-task/src/generated/*`, and `out/manifests/*`.
3. Cohesix HAL and driver implementation: `apps/root-task/src/hal/mod.rs`,
   `apps/root-task/src/hal/dma.rs`, `apps/root-task/src/hal/cache.rs`,
   `apps/root-task/src/hal/bcmgenet.rs`, `apps/root-task/src/hal/pi4_wifi.rs`,
   `apps/root-task/src/hal/pi4_pcie.rs`, `apps/root-task/src/sel4.rs`,
   `apps/root-task/src/local_seat_pi4.rs`, and
   `third_party/usb-oxide/src/xhci.rs`.
4. Cohesix normative docs: `AGENTS.md`, `docs/BUILD_PLAN.md`,
   `docs/HARDWARE_BRINGUP.md`, `docs/ARCHITECTURE.md`,
   `docs/INTERFACES.md`, and `docs/SECURITY.md`.
5. Official seL4 sources:
   - seL4 Reference Manual 13.0.0:
     <https://sel4.systems/Info/Docs/seL4-manual-13.0.0.pdf>
   - seL4 13.0.0 release notes:
     <https://docs.sel4.systems/releases/sel4/13.0.0.html>
   - seL4 Raspberry Pi 4 platform page:
     <https://docs.sel4.systems/Hardware/Rpi4.html>
   - seL4 Rust root-task serial-device tutorial:
     <https://docs.sel4.systems/projects/rust/tutorial/root-task/serial-device.html>
   - seL4 interrupts, notifications, untyped, and mapping tutorials:
     <https://docs.sel4.systems/Tutorials/interrupts>,
     <https://docs.sel4.systems/Tutorials/notifications.html>,
     <https://docs.sel4.systems/Tutorials/untyped.html>,
     <https://docs.sel4.systems/Tutorials/mapping.html>
   - seL4 verified configurations and CAmkES DMA reference:
     <https://docs.sel4.systems/projects/sel4/verified-configurations.html>,
     <https://docs.sel4.systems/projects/camkes/manual.html#direct-memory-access>
6. Hardware/vendor or OS implementation references:
   - Raspberry Pi BCM2711 ARM peripherals:
     <https://datasheets.raspberrypi.com/bcm2711/bcm2711-peripherals.pdf>
   - Mainline Linux BCM2711 DTS:
     <https://github.com/torvalds/linux/blob/master/arch/arm/boot/dts/broadcom/bcm2711.dtsi>
   - Mainline Linux Pi 4 board DTS:
     <https://github.com/torvalds/linux/blob/master/arch/arm/boot/dts/broadcom/bcm2711-rpi-4-b.dts>
   - Mainline Linux `bcmgenet`:
     <https://github.com/torvalds/linux/blob/master/drivers/net/ethernet/broadcom/genet/bcmgenet.c>
   - U-Boot `bcmgenet`:
     <https://github.com/u-boot/u-boot/blob/master/drivers/net/bcmgenet.c>
   - OpenBSD `bwfm(4)`:
     <https://man.openbsd.org/bwfm.4>
   - Infineon WHD architecture:
     <https://infineon.github.io/wifi-host-driver/html/index.html>
   - Linux `brcmfmac` SDIO:
     <https://github.com/torvalds/linux/blob/master/drivers/net/wireless/broadcom/brcm80211/brcmfmac/sdio.c>

Linux, U-Boot, OpenBSD, and WHD are design references only. They may define
probe order, register contracts, recovery ladders, and expected evidence, but
their source code must not be copied into Cohesix.

## seL4 Driver Model

seL4 does not provide Linux-style in-kernel drivers. The root task receives
authority over remaining resources and must deliberately delegate or retain
that authority through capabilities. The kernel provides the mechanisms:

- device memory can be exposed to user-level code by retyping device untyped
  memory into frames and mapping those frames into a VSpace;
- IRQs can be represented by IRQHandler capabilities and delivered as
  notification signals;
- CPU time is controlled by TCBs, priorities, domains, and scheduling context
  policy;
- DMA safety is outside the standard verified proof unless an IOMMU/SMMU
  configuration constrains device writes.

For Cohesix, the root task remains the privileged driver owner. Drivers must
depend on the narrow HAL trait that represents the resource they need:

- `DeviceHal`: MMIO, device-untyped coverage, DMA frames, DMA guard pages,
  and IRQ notification binding/acknowledgement.
- `PciHal`: generic PCI discovery/configuration for platforms with a HAL-owned
  topology. Do not assume this is the active Pi 4 VL805 path; current
  `KernelHal::pci_topology()` returns `None`, and Pi 4 VL805 ownership is
  proven by `apps/root-task/src/hal/pi4_pcie.rs`.
- `Cyw43Hal`: SDIO, power/reset, firmware, and Wi-Fi transport support.

The compatibility `Hardware` facade may remain where legacy call sites span
several domains, but new driver logic should prefer the narrowest trait.

## HAL, MMIO, DMA, SDIO, And PCIe Contracts

These contracts are the current Cohesix guardrails. If code needs a different
shape, update the milestone, manifest/IR, implementation, tests, and docs in
one scoped change before relying on it.

### HAL Boundary

All device access goes through HAL. Drivers may read and write device registers
only through HAL-returned mapped pages or device-specific HAL transport methods.

- Raw physical-address discovery belongs in HAL, never in a driver.
- Device-untyped retyping belongs in `KernelEnv::map_device` through
  `DeviceHal::map_device`.
- IRQHandler creation, notification badging, and acknowledgement belong in
  `KernelHal::bind_irq_notification`, `KernelHal::poll_and_service_irq`, or
  `KernelHal::wait_and_service_irq`.
- DMA frame allocation, guard-page reservation, pinning, and cache maintenance
  are HAL-owned.
- Firmware, mailbox, power, reset, clock, SDHCI, and SDIO CMD52/CMD53 access
  for CYW43455 belongs behind `Cyw43Hal` / `pi4_wifi`.
- Pi 4 BCM2711 PCIe root-complex/VL805 config access belongs behind
  `pi4_pcie`; drivers must not derive config space from the xHCI BAR.

### MMIO Mapping

seL4 exposes device memory by retyping device untyped into frames and mapping
those frames into the root-task VSpace. The seL4 untyped model has a watermark:
once allocations move past a physical address within an untyped, exact mapping
of an earlier page can fail until the children are revoked. Cohesix preserves
that rule in `KernelEnv::map_device`.

- Check `device_coverage(paddr, PAGE_BITS)` before mapping a device page.
- Map multi-page apertures in ascending physical-page order.
- For exact Pi 4 pages that share one device-untyped region, never map a higher
  page first and then expect a lower page to remain available.
- HAL must verify `ARMPageGetAddress` / `page_get_address` equals the requested
  physical address before publishing a mapping.
- Device mappings use Cohesix `DEVICE_VM_ATTRIBUTES`; drivers must not invent
  mapping attributes at call sites.
- MMIO access must be volatile and bounds-checked through `MappedRegion` or
  `MappedRegisterPages` unless a narrow HAL helper documents why raw volatile
  access is required.

Current Pi 4 examples:

- GENET maps six 4 KiB pages from one HAL-selected alias and requires all pages
  to have device coverage before publishing registers.
- Wi-Fi maps mailbox and SDHCI pages behind `pi4_wifi`; the CYW43 driver sees
  SDIO transport operations, not SDHCI register ownership.
- VL805 maps BCM2711 PCIe host pages in ascending order so the EXT_CFG DATA
  page at `0xfd508000` remains mappable before the EXT_CFG INDEX page and later
  root-complex registers are touched.

### DMA And Cache

Without an IOMMU/SMMU proof, a DMA-capable device is part of the trusted path:
seL4 does not prevent the device from writing any address it can bus-master.
Cohesix therefore treats DMA buffers as explicit shared-memory objects with
device-specific bus-address policy.

- Allocate DMA memory only through `DeviceHal::alloc_dma_frame*`.
- Use `alloc_dma_frame_low*` when the device has a low-address window.
- Use `seL4_ARM_Page_Uncached` when the driver contract requires uncached DMA.
- Pin every device-shared range with `hal::dma::pin` before publishing it to a
  device.
- Use `hal::dma::sync_for_cpu` before CPU reads of device-written cached data,
  and `hal::dma::unpin` before reclaim.
- Cache clean/invalidate/unify operations go only through `hal::cache`, whose
  labels come from the local generated seL4 bindings.
- xHCI device-publish labels (`xhci-*`) use `hal::dma::pin` with
  clean+invalidate before handoff, matching U-Boot's `xhci_flush_cache()`
  behavior for controller-visible rings, contexts, and transfer buffers while
  keeping SDIO/Wi-Fi DMA on its existing clean-only publish path.
- Never DMA into stack memory, parser buffers, unbounded heap buffers, or memory
  whose lifetime is not tied to a HAL frame/ring owner.
- Descriptor rings and packet buffers must be fixed-size, auditable, and have
  explicit producer/consumer ownership transitions.

Device-visible address policy is not generic:

- GENET currently uses `bcmgenet::dma_bus_addr`, `dma_uncached() == true`, and
  the diagnostic policy name `physical`.
- VL805/xHCI currently publishes `0x00000004_00000000 + CPU physical` for DMA
  pointers and keeps backing allocations below the 4 GiB inbound PCIe DMA
  window. A plain CPU physical address is wrong for xHCI rings on Pi 4.
- VL805/xHCI command, transfer, and event rings must be 64-byte aligned before
  their physical addresses are published. The Pi 4 cold-boot proof uses
  U-Boot-shaped command/event rings: 64 TRBs and one ERST entry. `CRCR`
  publication writes the target command-ring pointer directly, without a
  staged zero or stale snapshot replay, and writes the low/control dword before
  the high dword, matching U-Boot `xhci_writeq`. Event-ring
  `ERDP.EHB` acknowledgements are split into explicit 32-bit MMIO writes with
  the low/control dword before the high dword, matching the U-Boot polling path.
- Pi 4 mailbox requests use the VideoCore bus aliases selected in `pi4_wifi`.
  Wi-Fi, VL805 reset-notify, and local-seat framebuffer property calls all use
  the shared HAL mailbox call lock so one path cannot drain or consume another
  path's response. Receive waits are bounded both while the mailbox is empty and
  while draining unrelated property-channel replies. Do not reuse those aliases
  for unrelated DMA devices.
- Pi 4 local-seat cold boot is USB-first. When local-seat USB is enabled and
  the net-console policy selects Wi-Fi (`wifi`, or `auto` with credentials), the
  boot path runs one bounded USB keyboard/xHCI probe before entering CYW43
  control-plane bring-up. Wi-Fi net-console initialization is deferred out of
  early kernel net init, then resumed before the serial root console is
  published. The `Cohesix console ready` banner and `cohesix>` prompt must be
  the last visible boot milestone after Wi-Fi association/addressing succeeds,
  reaches a terminal failure, or hits the bounded pre-root wait timeout. QEMU
  virtio and Pi 4 wired NIC paths still use their immediate net-init flow.
- CYW43455 SDIO traffic is host-driven through SDHCI/CMD52/CMD53; the driver
  must not publish arbitrary DMA addresses to the Wi-Fi firmware path.

### IRQ Ordering

The seL4 interrupt pattern is derive IRQHandler, bind notification, wait/poll
notification, clear the device source, then acknowledge the IRQHandler. Cohesix
keeps that sequence in HAL helpers because acknowledging a level-triggered line
before clearing the device source can immediately redeliver the interrupt or
leave the line stuck.

- Use `IrqTrigger::Level` for Pi 4 SDIO and PCIe INTx-style lines unless source
  authority proves an edge-triggered line.
- Notification badges are IRQ-derived so shared notification objects stay
  auditable.
- Device clear callbacks must be bounded and nonblocking.
- Pi 4 SDIO uses GIC hwirq 158 through HAL binding and SDHCI
  `INT_STATUS`/`INT_ENABLE`/`SIGNAL_ENABLE`.
- IRQ 27 is the seL4 timer on this path; never treat it as a USB or Wi-Fi
  device interrupt.
- SDHCI command/data wait paths must not clear `CARD_INT` as a side effect of
  command completion, data-ready, or transfer-finish waits. `CARD_INT` belongs
  to the HAL IRQ service path, where Cohesix first clears the CYW43 dongle-side
  interrupt source, acknowledges the SDHCI/seL4 interrupt, and only then
  re-enables the SDHCI `CARD_INT` signal path.
- Current Pi 4 USB/VL805 is event-ring polled with PCI INTx/MSI/MSI-X delivery
  masked. The cold-boot command proof publishes fresh rings, starts with
  `USBCMD.RUN`, and acknowledges consumed events with `ERDP.EHB`. The gate-3/4
  proof command is Enable Slot; No Op is diagnostic-only and must not advance
  root-port sampling. Local-seat preserves already-posted current-boot PSC
  events until after it DMA-publishes Enable Slot and rings doorbell `0`; the
  wait loop then skips and acknowledges those PSC events, inserts the same
  bounded prompt-safe wait used for other unexpected events, and still accepts
  only the matching Command Completion Event as gate-4 proof. Do not unmask
  external xHCI interrupt delivery until a milestone explicitly proves it.

### SDIO/CYW43455

CYW43455 is an SDIO device, but Cohesix does not expose a generic SDIO host API
to drivers. The `Cyw43Hal` contract owns SDHCI reset, power, clock, bus width,
CMD52 direct I/O, CMD53 extended transfers, firmware bundle access, and Wi-Fi
power/reset state.

- Function 0 is CCCR/FBR control.
- Function 1 is the Broadcom backplane/control path.
- Function 2 is the data/control-plane FIFO path after firmware.
- Function 2 remains disabled before firmware/NVRAM upload.
- Production Function 2 traffic requires firmware upload/release evidence, real
  `CHIPCLKCSR.HT_AVAIL`, and live Function 2 readiness (`IOR2`/ready proof). The
  latest Pi 4 hardware trace proved the `CHIPCLKCSR=0x50`
  (`ALP_AVAIL|HT_REQ`) shape can latch `IOEX=0x06` while `IOR2` remains clear at
  `0x02`, even after the Linux-sized wait. That no-HT shape is therefore
  diagnostic evidence only, not a production Function 2 promotion.
- The bounded no-HT probe is not a success gate: `0x52`
  (`FORCE_HT|HT_REQ|ALP_AVAIL`) is only diagnostic readback of the exact `0x50`
  token after F2 clock forcing, and production boot still stops before F2 traffic
  unless real HT and live `IOR2` are both present.
- Function 2 enable follows Linux's `SDIO_WAIT_F2RDY` contract: the strict
  Cohesix path polls CCCR `IORx` for a 3000-sample F2-ready window before
  declaring `sdio-function2-ready-timeout`.
- Function 2 control-plane frames follow Linux SDIO host sizing: payloads that
  still fit CMD53 byte mode are four-byte aligned, while larger frames are
  padded to the 512-byte Function 2 block size so the HAL emits block-mode
  CMD53 rather than an unencodable byte-mode count.
- Pi 4 CLM upload follows the captured Linux `clmload` cadence: the first
  `clmload` DCMD carries a 1400-byte CLM blob chunk (`len=1412` after the
  `download_hdr`). Control-plane TX frames include Linux's 20-byte SDPCM
  control header (`hwhdr` + `hwext` + 8-byte software header), set
  `dat_offset=20`, carry the padded CMD53 request length in the on-wire
  `hwhdr`, and record the unpadded frame length plus tail padding in `hwext`.
  The first CLM frame is therefore a 1456-byte unpadded frame with an SDIO
  request length of 1536 and `tail_pad=80`, matching the Pi 4 Linux capture.
  The driver queries firmware `ver` and then `clmver` after upload before
  continuing with the remaining preinit commands.
- CLM must not be the first meaningful BCDC/iovar exchange. Linux proves the
  initial control-plane path with small iovars first: set
  `bus:txglomalign=8`, query `ulp_sdioctrl` while accepting the captured
  `BCME_UNSUPPORTED` result, set `bus:rxglom=1`, query `cur_etheraddr`, then
  issue Linux's mandatory `BRCMF_C_GET_REVINFO` (`cmd=98`) before CLM. Cohesix
  follows that order before CLM so the first Function 2 replies are small
  Linux-shaped 64-byte first-read transactions rather than a large CLM
  transfer. After that attach proof, Cohesix keeps the
  `bus:txglom`/`bus:rxglom` state established by the Linux-style SDIO preinit
  path through firmware `ver`, `clmver`, and `mpc`. Disabling `bus:rxglom` as a
  local safety shortcut before `mpc` is not Linux-equivalent and the
  2026-05-13 Cohesix boot trace showed it can move an otherwise working
  control plane into a host-`CARD_INT`/no-dongle-source stall. RX-glom data
  limits belong at the post-attach receive path, not in the preinit transport
  order.
- The first Linux-order control writes use the plain 12-byte SDPCM header. The
  8-byte SDPCM hardware-extension header is enabled only after `bus:rxglom`
  succeeds, matching `brcmfmac`'s `tx_hdrlen` transition. Sending
  `bus:txglomalign` with the extended header shifts the CDC payload to offset
  20 before the firmware has enabled that framing and leaves the host polling
  for a reply the firmware never generates.
- CYW43455 firmware upload uses Linux's 32 KiB brcmfmac backplane windows as
  the primary path (`brcmf_sdiod_ramrw write 32768 bytes`) and keeps Cohesix's
  byte-mode upload path only as the bounded seL4 recovery path after a real
  SDHCI transport failure. The HAL must still split the raw Function 1 CMD53
  commands like Linux's MMC SDIO helper: one command may carry at most 511
  blocks, so each 32 KiB F1/64-byte firmware window becomes a 511-block
  32704-byte command plus a final 1-block 64-byte command. Do not encode a
  block-mode count of zero for a 512-block command; zero count is only valid for
  512-byte byte-mode CMD53 transfers.
- Non-captured early iovars must not be hard blockers before this attach proof
  point. Cohesix may attempt station-path compatibility knobs such as
  `bus:txglom` and AMPDU limits only after the Linux first-iovar/MAC sequence,
  and `BCME_UNSUPPORTED` on those non-captured knobs is nonfatal. Transport
  errors remain fatal. Do not set `apsta=1` on the normal Pi 4 station attach
  path; Linux reserves that iovar for AP/P2P-style paths, and the 2026-05-12
  Cohesix boot proof showed it can move a healthy station attach into a
  hintless first-read/no-IRQ control-plane stall. Do not set `country` on the
  normal Pi 4 station attach path either; the Pi 4 Linux capture reaches
  country later as a cfg80211/regulatory query path, and the 2026-05-12
  Cohesix post-`apsta` proof showed an early `country` write can move the same
  transport into the hintless first-read/no-IRQ stall. Event-mask setup is not
  optional; at least one join-event subscription path must be proven before
  `SET_SSID`. The normal Pi 4 station attach tail must also avoid local legacy
  writes that are absent from the Linux capture before join. The 2026-05-13
  19:04 Cohesix trace proved firmware `ver`, `clmver`, `mpc`, `join_pref`,
  scan timing, event-mask, and `WLC_UP`, then stalled at legacy
  `WLC_SET_GMODE` (`cmd=110`) with host `CARD_INT` latched and no dongle reply
  source. Cohesix now skips early `WLC_SET_GMODE`, `WLC_SET_BAND`,
  `WLC_SET_ANTDIV`, local AMPDU-limit writes, and `WLC_SET_PM` on the
  station attach path unless a later Linux-equivalent gate proves they belong
  there. The 20:12 follow-up trace superseded that frontier: it reached join
  programming, drained an `EVENT_IF`, then spun on Function 1 `0x0c020`
  host-latch/no-dongle-source polls. Proof tooling must report that live edge
  as `join-programming-host-latch-loop`; earlier ARMCR4 CMD53 R5 errors in the
  same trace are recovered history once later control-plane replies and `UP`
  are proven.
- Cohesix also applies the Linux attach-time `join_pref` default payload
  (`04 02 08 01 01 02 00 00`) before scan/join defaults. WPA2-PSK join setup
  must match Linux and known-good CYW43 behavior: program the supported
  WPA2-PSK/CCMP RSN IE through `wpaie`, then the primary-BSS initial WPA2
  version mask (`wpa_auth=0x00c0`), D11 auth (`auth=0`), AES security
  (`wsec=0x0004`), optional MFP only when a captured RSN/MFP policy proves it,
  final WPA2-PSK AKM (`wpa_auth=0x0080`), firmware supplicant, then
  `WLC_SET_WSEC_PMK` before the primary `join` iovar. `infra=1` is not part of
  the Linux connect-time station command sequence. For the primary BSS
  (`bsscfgidx=0`), Linux `brcmf_fil_bsscfg_*` collapses most station connect
  commands to the plain iovar name and data, so Cohesix must keep `wpaie`,
  `wpa_auth`, `auth`, `wsec`, and `join` on the plain primary path. The Pi 4
  Linux capture shows the plain `sup_wpa` feature probe returning
  `BCME_UNSUPPORTED`; the same Linux run still connects because host
  `wpa_supplicant` handles EAPOL and key install outside the
  firmware-supplicant path. Milestone 26b authorizes bounded WPA2-PSK join
  sequencing, but no data path may be released on `SET_SSID` alone, so `sup_wpa`
  failure must not authorize or weaken the secure Cohesix join rule. Cohesix may,
  after an explicit plain `sup_wpa` `BCME_UNSUPPORTED`, try the known-good
  CYW43 firmware-supplicant wrapper shape
  (`bsscfg:sup_wpa`, `bsscfgidx=0`, value `1`) plus wrapper-scoped optional
  `sup_wpa2_eapver` and `sup_wpa_tmo`. That exception is limited to firmware
  supplicant offload; it must not reintroduce wrapper-shaped `wsec` or join
  programming. `WLC_SET_WSEC_PMK` is a firmware-supplicant-offload command in
  the Pi 4 station path; if both firmware-supplicant shapes are unsupported,
  Cohesix skips PMK programming, submits the primary join request, and exports
  `wifi-host-eapol-pending` as the live next secure boundary. Until a host
  EAPOL/key-install path completes the secure handshake, Cohesix keeps DHCP and
  normal data TX disabled, runs an association-gated join-submit EAPOL proof
  window, enables a Linux-shaped receive-admission window (`mcast_list` for
  `01:80:c2:00:00:03`, `allmulti=0`, optional `WLC_SET_PROMISC=0`), and then
  keeps the deferred EAPOL-only receive lane alive with low-level SDIO
  breadcrumbs suppressed even after a terminal `host-eapol-required` verdict.
  M1/M3 are expected as unicast frames to the station after association; the
  PAE group address is retained only as an initial diagnostic EAPOL-Start probe.
  The hot host-latch clear path must not spam serial with repeated successful
  `SDIO_INT_STATUS` Function 1 reads or host-card-int-latch-only clears; logs
  stay reserved for first-edge proof, dongle source bits, unreadable-source
  failures, SDHCI transfer errors, and explicit operator diagnostics.
  HAL Function 2 writes must preserve the SDPCM channel boundary: control
  channel writes may arm a bounded ioctl-reply wait, but data/event channel
  writes such as EAPOL-Start must not arm or inherit a control-plane reply wait.
  Data TX frames also use Linux's extended Function 2 SDPCM shape after the
  control-plane `tx_hdrlen` transition: a 20-byte SDPCM TX header, software
  header at byte 12, `dat_offset=0x1a`, six bytes of padding, then the BDC
  header and Ethernet payload. The local Linux capture shows this exact shape for
  station data TX (`84 00 7b ff 80 00 00 01 ... 2a 02 00 1a`). EAPOL data TX
  must set the BDC priority byte to `6`, matching the Linux brcmfmac priority
  path for 802.1X frames. The current as-built host-EAPOL implementation
  contains bounded M1/M3 admission, M2/M4 transmit, 802.1X drain before key
  programming, and PTK/GTK `wsec_key` install logic. GTK/group keys use the
  Broadcom primary/default-key flag, while PTK/pairwise keys keep flags zero.
  The last proven Pi 4 boot (`/Users/lukasbower/pi4-serial-20260516-073942.log`)
  did not receive M1 after BSSID refresh and EAPOL-Start TX, so that hardware
  state is not a Wi-Fi connection: proof tooling reports `WIFI_GATE=7`,
  `WIFI_BLOCKER=host-eapol-required`, and prompt-side `nettest` reports
  `wifi-host-eapol-pending`. The next valid success trace must show
  `host-eapol action=data-tx-shape ... bdc_priority=6`,
  `host-eapol action=send-m2`, `host-eapol action=send-m4`,
  `host-eapol action=wait-pending-8021x-drain`,
  `host-eapol action=install-wsec-key kind=ptk`, `kind=gtk`, and final
  `join complete mode=host-eapol secure=yes` before DHCP or normal data TX is
  enabled.
  Any observed 4-way frame is classified as `m1`, `m3`,
  `group-key`, malformed, or unexpected station-originated traffic, and the log
  records the exact next required host action (`derive-ptk-send-m2`,
  `verify-mic-send-m4-install-keys`, or key inspection). On M3, Cohesix must
  keep hostap ordering: validate the AP nonce and replay counter, verify the
  MIC, send M4, then install PTK and GTK through `wsec_key` before allowing
  DHCP/data. It must not fall back to `SET_SSID`-only completion or a lower-level
  transport hint. Explicit
  diagnostic commands may still probe the HAL-owned SDIO/control path. The
  2026-05-14 22:11 trace proves the corrected
  boundary: plain `sup_wpa` and wrapper `bsscfg:sup_wpa` both return
  `BCME_UNSUPPORTED`/`0xffffffe9`, and the old Cohesix image then incorrectly
  tried `WLC_SET_WSEC_PMK` twice before receiving `BCME_BADARG`/`0xfffffffe`.
  Transport errors remain fatal. When firmware supplicant offload is enabled,
  the primary PMK payload is the 132-byte Linux
  `brcmf_wsec_pmk_le` shape (`u16 key_len`, `u16 flags`, 128-byte key area).
  For 8-63 byte passphrases Cohesix derives the 32-byte PBKDF2-HMAC-SHA1 PMK
  from SSID + passphrase and sends flags `0`; for 64 ASCII hex PSKs it decodes
  the 32-byte PMK directly. A `BCME_BADARG` response to
  `WLC_SET_WSEC_PMK` is preserved as `wsec-pmk-bad-argument`, not masked by
  later SDIO hint probes. Any PMK or supplicant-shape rejection is
  join-programming drift, not a transport failure.
- Join programming must use Linux's station command shape. After `WLC_UP`,
  Cohesix drains bounded post-`UP` interface events before the first join
  security command, then sends the upstream primary-BSS `join` extended payload.
  Only an explicit `join` iovar failure may fall back to the legacy
  `WLC_SET_SSID` payload. A pre-join Function 1 host-latch loop is not
  association progress, DHCP progress, or IRQ progress; it is a join-programming
  blocker until a control reply, `join pending`, or terminal join event proves
  the firmware accepted the command. The 2026-05-13 20:40 trace exposed a
  Cohesix-only wrapper drift (`bsscfg:wsec` with a leading zero index) at this
  gate; proof tooling reports that old image as
  `primary-bsscfg-wrapper-join-security-loop`. The 2026-05-13 22:00 trace
  superseded that wrapper blocker: the image sent plain `wsec` first and then
  stayed in IRQ158 host-latch/no-dongle-source polling until the `wsec` ioctl
  timed out. Linux sends initial `wpa_auth` and `auth` before `wsec`; proof
  tooling reports that old image as `join-security-wsec-first-loop`. The
  driver must preserve that exact join-security gate into `wifi diag` instead
  of collapsing the post-failure shell output into a lower-level Function 1 or
  Function 2 visibility symptom.
- The 2026-05-13 22:45 trace proves the ordered-security image was flashed and
  reaches the first `wpa_auth` iovar before any `auth`, `wsec`, supplicant, PMK,
  or join-submit command. A late async `EVENT_IF` arrived in the ioctl reply
  window, after which the HAL cleared only host-side IRQ158 latches with
  `progress=no` until the bounded no-progress fail-fast. Root-task now keeps the
  control-reply wait active across non-control frames, sends the Linux
  connect-time RSN IE (`wpaie`) before initial `wpa_auth`, and reports the live
  frontier as `join-security-wpa-auth-initial-loop` instead of reusing the
  stale `wsec` label.
- Firmware RAM readback is diagnostic, not a mandatory production gate. The
  Linux production brcmfmac path compiles full RAM verify out; Cohesix may run
  bounded readback for evidence, but `sdhci-byte-mode-count` and similar
  readback-transport limitations after a completed upload must be reported as
  `readback-unavailable` and must not trigger a lower-speed reupload that can
  disturb a good image. Byte mismatches remain terminal because they prove bad
  payload contents.
- WPA2 join completion must be gated on cryptographic completion, not on
  `WLC_SET_SSID` success. Open networks may complete on a successful `SET_SSID`
  event. Secure networks must subscribe to `SET_SSID`, `AUTH`, and `PSK_SUP`;
  when firmware supplicant mode is enabled, require both successful `SET_SSID`
  association progress and `PSK_SUP` status 6 before the data path is released.
  When firmware supplicant mode is unsupported, the only valid live next state
  is `wifi-host-eapol-pending`; EAPOL frames may be logged as proof, but DHCP
  and data TX stay blocked until the bounded host EAPOL/key-install path reports
  secure completion. Device construction after this boundary must report a
  precise bring-up status such as `wifi-host-eapol-pending`; it is not proof
  that the Wi-Fi data path is online. Deferred boot joins may keep the EAPOL-only
  receive lane running after the proof window, but the driver must suppress
  repetitive low-level SDIO breadcrumbs and continue dropping non-EAPOL data
  until the secure boundary is complete. Blocking join attempts and timed-out deferred
  attempts must stop normal smoltcp receive polling so the root console is not
  flooded with no-progress Function 1 `SDIO_INT_STATUS` latch reads;
  prompt-side `wifi diag`, `wifi retry`, and related diagnostics remain the
  explicit way to ask the HAL for another live probe. During the join-submit
  proof window, Cohesix first waits for
  association evidence (`SET_SSID`, association, or link-up) and then spends
  the EAPOL proof budget so the AP has a post-association M1 window. Successful
  association/link events may seed the AP/BSSID from the Broadcom event address
  or Ethernet source when the address is a valid unicast AP candidate, but that
  seed is only a hint: before host EAPOL-Start, Cohesix must issue
  `WLC_GET_BSSID` (`cmd=23`) and prefer the firmware-reported associated BSSID
  when it is a valid AP candidate. The seed log must include both raw event
  candidates, and the EAPOL-Start log must include the resolved destination MAC.
  EAPOL-Start sends one PAE group diagnostic probe and then prefers the resolved
  AP/BSSID for bounded retries before M1, while still letting M1 overwrite the
  AP MAC with the authenticated frame source. Cohesix
  parses the EAPOL/EAPOL-Key envelope
  for proof (`m1`/`m3` shape, key-info bits, replay-counter presence, and
  key-data length) and may complete the bounded host handshake only after M2,
  M4, and `wsec_key` PTK/GTK install succeed. Deferred join timeout and
  prompt-side `nettest` diagnostics must preserve `wifi-host-eapol-pending`
  while the deferred receive lane is still alive and `wifi-host-eapol-required`
  after the terminal proof window closes, instead of collapsing the failure into
  generic association or DHCP status. Any root-task panic after that line is a
  boot blocker and proof tooling must
  preserve the host-EAPOL Wi-Fi blocker rather than rewriting it to `none`. Any
  other `PSK_SUP` status is a failed secure join, not a pending-success edge.
  DHCP and data TX must not begin before that secure completion rule is
  satisfied; the root-task DHCP client is started only after the Wi-Fi backend
  reports no pending
  association status and a live Wi-Fi carrier. A post-join `EVENT_LINK` without
  the link flag is `wifi-link-down`; it must defer DHCP rather than being
  normalized as DHCP progress.
- Join-completion event delivery is a hard gate. Cohesix first programs the
  Linux `event_msgs_ext` shape (`ver=1`, `command=SET_MASK`, `len=27`) using
  the Pi 4 capture mask plus the Cohesix-required `AUTH`, association, and
  `PSK_SUP` bits. If and only if the firmware explicitly returns
  `BCME_UNSUPPORTED`, Cohesix falls back to a global `event_msgs` mask that
  preserves existing firmware bits and ORs the same required join events. If
  neither subscription path is proven before `SET_SSID`, join setup must fail.
- Linux clears `SBSDIO_FUNC1_SDIOPULLUP` during SDIO buscore preparation.
  Cohesix does not currently issue that optional CMD52 on Pi 4: the
  2026-05-12 Cohesix boot log proves the write can return a CRC failure and
  poison the following CCCR speed CMD52 with end-bit errors, while the same
  board reaches firmware upload and join gates when the write is skipped.
  This is a seL4 HAL adaptation, not a new hardware requirement; do not enable
  the pullup clear until the SDHCI path proves Linux-equivalent CMD52 recovery
  across the immediately following sideband access.
- During Pi 4 host reset, a stale bootloader `CMD_INHIBIT` bit in SDHCI
  `PRESENT_STATE` is not proof of a Cohesix command in flight. Match the Linux
  SDHCI reset order by proving power/card presence, programming the startup
  card clock, then issuing the command/data reset and requiring inhibit clear
  before the first SDIO command. Logs must distinguish this bounded pre-clock
  recovery edge from a real post-command `sdhci-inhibit-timeout`.
- Before the first iovar, Cohesix must drain the Linux-observed startup
  status/credit traffic emitted after `Dongle ready`. The captured first frame
  is a 12-byte SDPCM header-only event/status frame, followed by an empty
  64-byte read. That frame is not a CDC ioctl response and must be consumed
  before `bus:txglomalign`, otherwise the first ioctl wait can consume stale
  startup traffic and then spin on the SDIO-core status sideband instead of
  reading the real response.
- After real post-release HT and live Function 2 readiness are proved, the Pi 4
  firmware channel arms the Linux-shaped Function 2 interrupt path
  (`HOSTINTMASK`, `CCCR.IENx`, and SDHCI `CARD_INT`) through HAL-owned source
  clear plus seL4 ack. The SDPCM `FUNCTIONINTMASK` readback is diagnostic on
  this Pi 4 path; a zero value is not interrupt-programming drift when
  `CCCR.IENx` is armed and the SDIO-core frame-indication path is live. IRQ 158
  is the Wi-Fi SDIO interrupt; IRQ 27 remains the seL4 timer and is never Wi-Fi
  progress evidence.
- A polled Function 2 control reply may leave SDHCI `CARD_INT` visible after
  the dongle-side `SDIO_INT_STATUS` source has already been read and cleared.
  That is a stale host interrupt latch to clear and acknowledge, not a terminal
  `wifi-sdio-polled-reply-source-still-visible` blocker. It remains terminal
  only when the dongle-side source cannot be read before the seL4 IRQ ack.
- SDIO IRQ logs must separate host interrupt delivery from dongle source proof.
  A seL4 IRQ 158 notification or SDHCI `CARD_INT` latch with
  `SDIO_INT_STATUS=0` is logged as host-latch/no-dongle-source evidence with
  `progress=no`; only a nonzero dongle source bit, `I_HMB_FRAME_IND`, or a valid
  Function 2 SDPCM header proves control-plane reply progress. IRQ 27 remains
  the seL4 timer and is never part of this proof.
- A post-control-write SDIO-core `I_HMB_FRAME_IND` bit is authoritative reply
  progress even when the Function 1 frame-length sideband still reads zero.
  Mirror Linux by reading the fixed Function 2 FIFO at `0x18000000` with the
  64-byte first-read shape and then the SDPCM-indicated remainder padded to the
  same Function 2 host-transfer shape Linux uses. Do not block that read on
  stale `RFRAME` sideband hints or on a `FUNCTIONINTMASK` readback of zero when
  `CCCR.IENx` is armed and `I_HMB_FRAME_IND` is visible.
- Control-plane receive buffers must fit the captured Linux `BRCMF_FIRSTREAD`
  cadence for large replies: the initial 64-byte fixed-address Function 2 read
  followed by a 2048-byte bulk read. Smaller 2048-byte buffers are not enough to
  hold the padded SDPCM control frame and will misclassify real reply progress
  as partial hint visibility.
- The same `BRCMF_FIRSTREAD` cadence applies when `RFRAME` already exposes a
  nonzero reply length. Cohesix must not collapse a 2064-byte control reply into
  one padded 2560-byte Function 2 request; it reads 64 bytes first, then the
  2048-byte padded remainder, and logs both successful CMD53 shapes.
- If the first control-plane write succeeds but neither `RFRAME` nor
  `I_HMB_FRAME_IND` becomes visible, the HAL must not spin indefinitely on the
  SDIO-core `int_status` word. It performs a sparse, bounded Linux-shaped
  hintless Function 2 first-read probe, then reports
  `cyw43-control-plane-hintless-firstread-no-irq` /
  `control-plane-reply-idle-loop` if no reply arrives. That terminal proof keeps
  the gate loop honest and preserves IRQ 158 as the only Wi-Fi interrupt source;
  IRQ 27 remains the seL4 timer.
- Control-plane replies are accepted only when both the CDC command and CDC id
  match the outstanding request. This prevents a stale echoed `clmload`, `ver`,
  or `clmver` response from satisfying a later ioctl with the same wrapped id.
  The CDC response length must also be fully present in the SDPCM payload and
  fit the bounded control buffer; truncated or oversized replies are protocol
  failures, not shortened successful replies.
- During that control-plane receive wait, non-matching control frames and short
  SDPCM event/data side frames are drainable traffic, not terminal ioctl
  failures. Linux's `rxctl` path keeps reading Function 2 until the expected
  control response arrives, so Cohesix must not treat a 12-byte event-side frame
  as `bdc-event` failure while waiting for the real CDC reply. After a
  post-control-write Linux first-read, a 12-byte control/event header-only frame
  is consumed as status/credit traffic and the HAL performs another bounded
  64-byte fixed Function 2 first-read for the real CDC response.
- Pi 4 local-seat USB and CYW43455 Wi-Fi may both be active during boot, but
  USB owns the bounded pre-net keyboard/xHCI activity window. While USB is
  inside that window, Wi-Fi progress updates and raw `[pi4-wifi]` breadcrumbs
  must not compete for HDMI/UART output; Wi-Fi records them to the internal log
  buffer and resumes visible progress only after the USB activity window clears.
- Wi-Fi net-console bring-up must precede the serial root console on Pi 4 when
  local-seat is enabled and the selected net-console interface is Wi-Fi.
  Cohesix emits `action=root-console-wait-for-wifi`, preserves the Wi-Fi
  configuration for operator diagnostics, resumes CYW43455 bring-up before the
  prompt, and publishes the serial shell only after Wi-Fi is reachable,
  terminally failed, or bounded by the pre-root wait timeout.
- Pi 4 local-seat USB is not a reason to disable Wi-Fi diagnostics. The serial
  console retains the HAL-backed Wi-Fi debug path after root-console handoff so
  `wifi diag`, `wifi load-fw`, and `wifi retry` can exercise CYW43455 without
  preventing boot from reaching the shell. Once a terminal boot/control-plane
  failure is preserved, `wifi diag` is passive: it emits the before/after state
  from cached evidence and skips the long live HT re-probe. Operators can still
  run the explicit `wifi probe-ht` command when they want the stateful HT probe.
- Wi-Fi association completion is event-pump driven for both explicit `wifi`
  and `auto` interface policies. The driver issues the join command before the
  serial prompt on Pi 4 local-seat Wi-Fi boots, and the pre-root event-pump wait
  keeps polling until association and DHCP/static addressing reach a usable
  state, terminally fail, or hit the bounded pre-root timeout.
- `auto` interface policy may fall back from Wi-Fi to wired only when CYW43 is
  truly absent or Wi-Fi credentials are missing. CYW43 protocol, HAL transport,
  firmware, join, and post-Function-2 errors are Wi-Fi gate evidence and must
  remain fatal so gates 7 and 8 cannot be hidden by the wired backend.
- Until a bounded deaggregator exists, any received SDPCM glom channel frame
  after attach is terminal gate-8 evidence (`cyw43-rxglom-unsupported`) rather
  than silent dropped data. Do not prevent that evidence by changing Linux's
  preinit `bus:rxglom=1` transport order before `mpc`.
- `KSO`, cached `DEVON`, `ALP_AVAIL`, or `FORCE_HT` are diagnostic or sideband
  evidence only; they do not authorize strict Function 2 traffic by themselves.
- No-HT / forced-HT paths remain diagnostics. Forced HT does not authorize
  production Function 2 traffic without real HT and live `IOR2`.
- Pi 4 uses the CYW43455 ARMCR4 firmware path. Do not run the Linux CM3-only
  SOCSRAM bank remap writes (`bankidx=3`, `bankpda=0`) before firmware upload;
  Linux applies those writes only to the 43430/43439 CM3 path, and on CYW43455
  they are a Function 1 backplane blocker rather than progress.
- Pi 4 Linux capture identifies the CYW43455 ARM_CR4 wrapper at `0x18102000`.
  `0x18103000` is the adjacent PCIe2 wrapper, so ARMCR4 reset/release proof must
  target `0x18102000` through the HAL backplane helpers.

### PCIe/VL805

Pi 4 VL805 ownership is a platform proof, not generic PCI enumeration. The
active path is Cohesix-owned cold start:

- U-Boot USB state, stopped register seeds, old DT trust tokens, and captured
  COMMAND shadows are diagnostics only.
- The boot script no longer exports `cohesix,xhci-mmio`, PCI COMMAND, or final
  xHCI handoff trust tokens as runtime authority.
- HAL powers the USB HCD domain through the VideoCore mailbox module `3`.
- HAL masks/clears PCIe host interrupt sources for the poll-only lane.
- HAL validates link/root-complex state and always refreshes the BCM2711 DMA and
  outbound MMIO windows before EXT_CFG proof; if the link/root state is not
  already ready, it runs one bounded BCM2711 root-complex reset/window init and
  drains posted writes with same-block readbacks.
- HAL maps the BCM2711 root-port config page before higher PCIe pages and
  programs the Linux/U-Boot bridge aperture before endpoint ownership:
  primary/secondary/subordinate buses `00/01/01`, memory window
  `0xc0000000..0xc00fffff`, prefetch disabled, and root-port COMMAND
  `Mem+ BusMaster+`. It also applies U-Boot's BCM2711 root-complex quirks:
  BAR2 PCIe-to-SCB little-endian mode and unadvertised ASPM support.
- HAL reselects VL805 `01:00.0` via BCM2711 `EXT_CFG_INDEX` before each
  `EXT_CFG_DATA` access and rejects selector echoes.
- Ownership can promote only on exact live `1106:3483`, class `0x0c0330`,
  BAR0 translation, COMMAND readback, command-proof PCIe Device Control
  (`DevCtl=0x281f`: 128-byte MPS, 512-byte MRRS, error reporting, Relaxed
  Ordering set, and No Snoop set, matching the Linux/U-Boot-compatible VL805
  read-transaction shape), and poll-only proof that INTx is masked, MSI is
  disabled when present, and MSI-X is mask-all/disabled when present.
- If the exact VL805 tuple appears with an unassigned 64-bit BAR, HAL may assign
  the Pi 4 outbound-window BAR value through EXT_CFG and read it back. Do not
  assign BARs for bad IDs, bad class, selector echoes, absent link proof, or
  any other tuple.
- U-Boot's Pi 4 order is USB-HCD power, BCM2711 PCIe root/PERST bring-up, then
  the firmware reset-notify for VL805. If Cohesix has to assert the BCM2711
  PCIe `SW_INIT_1`/PERST path after an earlier reset-notify because the link was
  not ready yet, HAL must replay the VL805 firmware reset-notify after link/RC
  readiness and before trusting EXT_CFG/xHCI ownership. If link/RC is already
  ready on the post-mailbox phase, HAL still issues the VL805 reset-notify and
  settle before reporting the refreshed PCIe windows as xHCI ownership evidence;
  link-ready alone is not firmware-ready. A live VL805 PCI
  function with `USBSTS.CNR` observed before HCRST is a reset-order clue;
  `USBSTS.CNR` stuck after the Cohesix-owned HCRST is the
  firmware-load/reset-order blocker, not command-ring or DMA evidence.
- xHCI ownership-register, `USBCMD.RUN`, and endpoint-doorbell
  posted-write flushes use HAL-owned BCM2711 EXT_CFG selector/COMMAND readback,
  endpoint BAR readback, and root bridge status, never xHCI BAR drains. The
  2026-05-10 Pi 4 trace proved that even an xHCI capability dword read at BAR
  offset `0x0000` can halt immediately after `USBCMD.RUN`; never use xHCI BAR
  reads, `USBSTS`, or any `PORTSC` as the posted-write drain on this
  prompt-safe path. The live xHCI BAR reads permitted on the Pi 4
  `platform-reset-complete` command gate are U-Boot's pre-HCRST
  `USBSTS.HCH==1` halt revalidation, a diagnostic-only pre-HCRST
  `USBSTS.CNR` observation, U-Boot's live
  `USBCMD` read-preserve-write seed immediately before HCRST, U-Boot's
  pre-`RUN` `USBCMD.HCRST==0` / `USBSTS.CNR==0` reset-completion handshake
  after the Cohesix-owned HCRST, plus the later pre-`RUN` `CRCR` seed read.
  The active `platform-reset-complete` path uses the same extended Pi 4
  platform-reset budget for the live HCH/HCRST/post-HCRST CNR polls
  that the previous blind-settle lane used; it must not assert HCRST,
  publish fresh rings, or ring doorbell 0 while `USBSTS.HCH` is unset,
  `USBCMD.HCRST` is set, or `USBSTS.CNR` remains set after HCRST.
  Pre-HCRST `USBSTS.CNR` is not a hard gate because U-Boot asserts HCRST
  after the halted check and then waits for CNR to clear after reset.
  `reset-controller-not-halted`, `reset-hcrst-timeout`, and
  `reset-controller-not-ready` are pre-command blockers, not Gate 4 evidence;
  old `reset-pre-hcrst-controller-not-ready` traces identify Cohesix's former
  over-strict pre-HCRST CNR wait rather than a U-Boot gate.
  Command timeout diagnostics on that lane emit one bounded final live state
  snapshot and avoid repeated live `PORTSC` or post-doorbell operational-register
  polling.
- CONFIG, DCBAAP, CRCR, initial ERDP, ERSTSZ, ERSTBA, DNCTRL, RUN,
  command-ring recovery, command-doorbell, and endpoint-doorbell posted-write
  drains fail closed when the HAL cannot prove the EXT_CFG selector, link/root
  readiness, PCIe IRQ-source masking, or poll-only VL805 COMMAND ownership, or
  when the drain read returns a selector echo or invalid config value. PCIe
  IRQ-source masking proof must reject all-ones sentinel readbacks and log that
  edge as untrusted instead of setting the posted-write proof latch.
- U-Boot-compatible command/event proof must publish the fresh ring registers in
  U-Boot order on the Pi 4 platform-reset lane (`DCBAAP`, `CRCR`, initial
  `ERDP`, `ERSTSZ`/`ERSTBA`), issue a HAL EXT_CFG drain after each
  controller-ownership register write, start the controller with `USBCMD.RUN`,
  then apply U-Boot's poll-only post-start interrupter state (`IMOD=0`,
  `IMAN=0`) through HAL-drained writes. On that lane, DCBAA slot `0` must stay
  zero while `DCBAAP`, `CRCR`, `ERDP`, `ERSTSZ`, and `ERSTBA` are published,
  then be rewritten with the HAL-returned scratchpad pointer-array bus address
  and shared again before `DNCTRL=0`. `CRCR`
  composition preserves the low
  `CMD_RING_RSVD_BITS` exactly as U-Boot does before OR-ing the command-ring
  pointer and producer cycle. On the Pi 4 `platform-reset-complete` lane, that
  means one pre-`RUN` live `CRCR` seed read after HCRST; when another seL4
  prompt-safe lane cannot live-read `CRCR` and has no trusted snapshot, it
  publishes from a zero reserved-bit seed instead of synthesizing Linux's later
  observed `CRCR` running-status bit. The U-Boot `DNCTRL=0` write is also
  HAL-drained
  before `USBCMD.RUN`. It must write the submitted command TRB in U-Boot
  `queue_trb()` order: parameter low, parameter high, status, then the control
  dword with the cycle bit last. Cohesix then DMA-publishes the whole
  command-ring allocation with HAL clean+invalidate before issuing the
  completion-grade command-ring publish barrier. This is stricter than U-Boot's
  16-byte cache flush while preserving the same ownership handoff. Cohesix then
  writes U-Boot's `DB_VALUE_HOST` to doorbell `0` and drains that posted write
  through the HAL-owned EXT_CFG/endpoint-BAR/root-status path before polling the
  event ring with the same 5 s command-event budget.
  The command proof is Enable Slot, matching U-Boot's first non-root-hub xHCI
  allocation gate. Already-posted current-boot Port Status Change events remain
  on the event ring and are skipped/acknowledged while the Enable Slot command
  is outstanding. The U-Boot-shaped Enable Slot lane must not perform a
  same-command re-doorbell or pre-poll event-ring debug sync; after the DB0
  posted-write flush it immediately enters the command-event wait and only the
  matching Command Completion Event can advance the gate. No Op is only a
  diagnostic helper and must not unlock root-port sampling. There is no
  pre-command `ERDP.EHB` acknowledgement, no pre-command PSC drain, and no
  same-command re-doorbell counted as proof.
  `ERDP.EHB` acknowledgement publishes the low/control dword before the high
  DMA-alias dword and drains both writes through HAL only after an event has
  been consumed; if the event-ring dequeue pointer cannot be translated to the
  device-visible DMA address, the path fails closed instead of publishing
  `ERDP.EHB` against address zero. All 64-bit ownership-register publications
  also drain both low and high dwords. Runtime `ERDP.EHB` ack logs must identify
  the low/control flush separately from the high DMA-alias flush so Gate 3 proof
  does not collapse both halves into one ambiguous stage. Command doorbell `0`
  itself still uses the U-Boot command
  value, but seL4 userland must drain that posted PCIe write through HAL-owned
  EXT_CFG selector/COMMAND readback plus endpoint BAR and root bridge status
  readbacks; it must not use an xHCI BAR read as the drain. A missing platform
  posted-write hook is a hard failure for the Pi 4 high-BAR VL805
  ownership-register, command-doorbell, and endpoint-doorbell paths.
- If the first U-Boot-shaped Enable Slot attempt times out after consuming
  current-boot PSC events, Cohesix may run exactly one bounded command recovery
  lane: stop the controller, poll `USBSTS.HCH` with a bounded U-Boot-style halt
  window, assert HCRST, require bounded HCRST clear and post-reset `USBSTS.CNR`
  clear, then republish fresh DCBAA/command/event rings in U-Boot register order
  (`DCBAAP`, `CRCR`, initial `ERDP`, `ERSTSZ`, `ERSTBA`), write `DNCTRL=0`,
  start the controller with `USBCMD=RUN`, and apply the U-Boot post-start
  poll-only interrupter state (`IMOD=0`, `IMAN=0`) before submitting a new
  Enable Slot. Recovery
  uses the same U-Boot cold-publish `CRCR` rule: after its local HCRST settle,
  the Pi 4 `platform-reset-complete` recovery lane performs the pre-`RUN` live
  `CRCR` seed read and preserves those low bits before writing the fresh command
  ring pointer. Other recovery lanes preserve low bits from a trusted snapshot or
  publish from a zero reserved-bit seed when no live read is allowed.
  The recovery lane still must not use xHCI BAR reads, `USBSTS`, or `PORTSC` as
  proof before command completion, and it must not acknowledge `ERDP.EHB` before
  the retry command completes. If both the cold U-Boot-shaped Enable Slot and
  this fresh U-Boot-shaped recovery lane time out while current-boot PSC events
  prove that the event ring is live, Cohesix may run one bounded
  Linux-captured command-event-generation fallback. That fallback is not
  U-Boot proof; it must be logged as
  `linux-captured-command-event-generation-after-uboot-timeout`, use a
  one-shot command path with no hidden retry, reset and republish fresh
  command/event rings again so the fallback Enable Slot TRB is at the published
  `CRCR` dequeue pointer, perform the Linux-captured event-ring/IMAN
  acknowledgement through HAL-drained paths, write the captured Linux
  command-event controls (`DNCTRL=2`, `IMOD=0xa0`, `IMAN.IE`, and
  `USBCMD.RUN|INTE`), then ring DB0 for that fresh Enable Slot command. A
  fallback command queued behind an already timed-out TRB is
  `cmd-stale-crcr-dequeue`, not proof. Only a matching Command Completion Event
  from that bounded fallback advances gate 4, and the cleanup lane must identify
  `cleanup_generation=linux-captured-command-event-generation`. Stale legacy
  Linux-shaped labels, pre-command status reads, post-enqueue event-generation
  replay without the preceding U-Boot recovery timeout, interrupt-delivery bits
  without MSI ownership, or a same-command re-doorbell are not proof.
- Pi 4 cold boot must attempt one bounded local-seat keyboard probe before
  net-console initialization. USB keyboard availability must not depend on the
  Wi-Fi/CYW43 bring-up reaching the cooperative event loop first. When local
  seat is enabled, explicit Wi-Fi net-console bring-up, and Auto policy with
  Wi-Fi credentials, proceed only after that pre-net USB probe has completed;
  the root console then waits in the event pump until Wi-Fi association and
  addressing are reachable. USB and Wi-Fi may only be interleaved at explicit
  boot/event-pump phase boundaries where the root task is not holding
  overlapping HAL ownership.
- Root-port state is cold-boot live evidence only. After mailbox reset, live
  HAL EXT_CFG proof, local HCRST, and fresh ring publication, direct `PORTSC`
  reads remain gated until command/event-ring proof succeeds; local-seat may
  then assert root-port power, require bounded `PORTSC.PP` readback evidence,
  run the U-Boot-shaped 5 s debounce window with 20 ms polls, and reset root
  ports through the Cohesix-owned controller. Linux or U-Boot captures must not
  synthesize a connected mask, speed, enabled-port state, or skipped root-port
  reset.
- The Pi 4 prompt-safe high-BAR lane proves command/event-ring consumption with
  U-Boot-shaped poll-only command state while PCI INTx/MSI/MSI-X/GIC delivery remains
  masked. For the fresh Pi 4 platform-reset path, already posted port-status
  events remain on the event ring until after the Enable Slot command is
  DMA-published and doorbell `0` is rung. The command wait then matches U-Boot's
  `xhci_wait_for_event()` behavior: unexpected Port Status Change events are
  skipped, acknowledged with `ERDP.EHB`, and only the matching command
  completion is accepted. Direct `PORTSC` reads remain blocked during this
  drain. Each skipped event republishes `ERDP.EHB` and drains the low/high dword
  writes through the HAL posted-write hook before polling the next event-ring
  slot; skipped PSC events also take one bounded prompt-safe settle before the
  next sync so command completions racing behind preserved PSCs are not hidden
  by a tight ERDP update loop. On success it immediately submits bounded
  poll-only Disable Slot
  cleanup for that slot; a cleanup failure is logged and tolerated only as
  command-ring proof, and later enumeration must not assume a pristine slot
  table. Linux-shaped cleanup or event-generation writes cannot advance gate 4.
  Only a completed command may reopen live root-port sampling.
- After an exact `cmd-event-ring-timeout` on the U-Boot-shaped lane, the bounded
  recovery lane remains U-Boot poll-only unless a later milestone implements the
  full Linux MSI ownership contract. Proof still requires a fresh matching
  command-completion TRB before root-port sampling resumes. Because preserved
  PSC events now prove the event ring can receive DMA writes, the summary labels
  that frontier as `event=psc-only command_completion=missing` instead of
  implying a dead event ring.
- On Pi 4 runtime/deferred handoff lanes, scratchpad publication follows the
  U-Boot cold-start edge: publish and DMA-clean DCBAA slot `0` with the
  scratchpad pointer-array bus address, then fill and DMA-clean the scratchpad
  pointer array. This keeps the command gate from depending on a prefilled
  scratchpad array that U-Boot never exposes before the DCBAA slot update.
  Prompt-safe recovery keeps the same visible order: slot `0` stays withheld
  through the fresh HCRST, DCBAAP, CRCR, ERDP, ERSTSZ, and ERSTBA writes, then
  recovery publishes/fills the scratchpad array before DNCTRL and `RUN`.
- Address Device follows U-Boot's EP0 max-packet ordering: low-speed uses 8,
  full-speed tries 64 first with 8 only as fallback, high-speed uses 64, and
  SuperSpeed uses 512. This keeps full-speed keyboard/composite devices on the
  same initial xHCI slot/EP0 context shape as U-Boot.
- SET_CONFIGURATION follows U-Boot's xHCI interception order: Cohesix programs
  the active configuration's endpoint contexts with Configure Endpoint before
  forwarding the SET_CONFIGURATION control transfer to the device. Later HID
  attach may reuse the already configured interrupt endpoint, but it must not
  make endpoint setup depend on a successful post-configuration control path.
- Periodic endpoint contexts must carry U-Boot-equivalent scheduler fields.
  For the current SINO WEALTH `258a:0f0a` keyboard interface, Linux reports a
  low-speed interrupt-IN endpoint (`0x81`) with `wMaxPacketSize=8` and
  `bInterval=10`. Cohesix therefore converts the low-speed frame interval the
  same way U-Boot does (`10 ms * 8` microframes rounded down to exponent `6`),
  publishes `Max ESIT Payload=8`, and sets `Average TRB Length=8`. A configured
  HID endpoint with those fields missing is a Gate 8/9 scheduler failure, not a
  reset, command-ring, event-ring, or IRQ27 issue.
- HID interrupt-IN transfer TRBs must also match U-Boot's normal-transfer
  completion flags: `IOC` is always set and `ISP` is set for IN endpoints so
  short keyboard reports generate transfer events. The endpoint doorbell write
  uses the xHCI DCI target (`3` for endpoint `0x81`), and HAL logs aligned
  doorbells beyond doorbell `0` as `role=endpoint-doorbell`. Command waits must
  preserve any non-command transfer event they drain while waiting for a later
  command completion, then replay it to the matching endpoint poller; otherwise
  the first HID report can be acknowledged in ERDP and lost before Gate 8 sees
  it. CPU-side HID/control descriptor reads must invalidate the DMA buffer after
  the transfer event before decoding device-written bytes.
- For keyboards behind a USB hub, the local-seat runtime must retain the hub
  device slot for as long as the HID keyboard is attached. Dropping the hub
  `UsbDevice` disables that xHCI slot and can silently orphan the interrupt-IN
  pipe after Gate 8. HDMI may show `local-seat USB keyboard online` only after
  the existing runtime first-byte proof reaches Gate 10; enumeration-only
  Gate 8 is reported as detected/pending input. Gate 10 proves at least one
  byte entered the root-console path. It is not full keyboard closure unless a
  printable key is also proven. Printable-key closure is separately evidenced
  by the first non-empty HID report and first printable-byte diagnostic, while
  the first unmapped HID usage is logged once if decode rejects a key. The
  current Pi 4 hardware frontier has proven Enter (`key=0x28`,
  `ascii=0x0a`) through Gate 10 but has not yet proven a printable letter byte.
  Treat `USB_BLOCKER=none` in that state as "xHCI/HID first-byte path works",
  not as proof that all boot-keyboard usages are usable.
- Root-port reset before Address Device follows the U-Boot retry envelope:
  retry reset/enable timeouts up to five attempts with a short first settle and
  longer subsequent settles, but do not synthesize a device when live root-port
  evidence says no connection. On the Pi 4 `platform-reset-complete` lane,
  after command/event-ring proof has reopened live port access, Cohesix also
  performs one extra bounded root-port reset/settle cycle before Address Device.
  That extra cycle is a Cohesix-owned cleanup for keyboards or hubs that U-Boot
  may have configured before `bootm`; it never runs before fresh command proof
  and never substitutes for controller ring adoption.
- The current boot-compatible keyboard/trackpad combo is treated as a normal
  HID composite device. Linux captures identify SINO WEALTH `258a:0f0a`, with
  interface 0 as Boot Keyboard (`Sub=01`, `Prot=01`) and interface 1 as a
  protocol-none touchpad. Cohesix local-seat enumeration must rank the Boot
  Keyboard interface as the primary target; the protocol-none touchpad must not
  displace keyboard bring-up. The keyboard decode contract consumes USB HID
  Usage Page `0x07` usage IDs (`A=0x04`, Enter `0x28`, keypad Enter `0x58`),
  not PS/2 set scancodes. If Enter works but letters do not, the next proof
  target is the actual non-empty HID report and printable/unmapped usage
  diagnostics, not a controller reset, command-ring, DMA-alias, or IRQ27 path.
- Cold-owned external VL805 high-BAR lanes (`None` and
  `platform-reset-complete` without a runtime seed) follow U-Boot's Pi 4 PCI
  xHCI path (`USB_XHCI_PCI`). Cohesix must not apply the Broadcom generic xHCI
  wrapper AXI quirk (`USB_XHCI_BRCM`, `AXIWRA/AXIRDA` at `0x0c08/0x0c0c`) to
  the VIA VL805 PCI BAR. Those labels are valid only for a generic Broadcom xHCI
  wrapper lane; on the Pi 4 VL805 PCI lane their absence is expected. Toxic
  post-`RUN` root-port or operational register sampling remains behind the
  command-completion gate.

## Evidence Ladder

Every new driver or risky driver change must climb this ladder in order. A
failure at any step means stop and fix that step; do not compensate at a later
layer.

### 1. Platform And Kernel Evidence

Record the exact local seL4 build facts before touching driver code:

```sh
rg -n "ARCH_AARCH64|PLAT_|ARM_GIC|SMMU|AARCH64_USER_CACHE|IRQ" \
  seL4/build/kernel/gen_config/kernel/gen_config.yaml \
  seL4/build/kernel/gen_headers/plat/platform_gen.h
```

For Pi 4, verify that the boot path is `Pi firmware -> U-Boot -> seL4 image ->
root-task`, matching `docs/HARDWARE_BRINGUP.md`. The upstream seL4 Pi 4 flow
uses `rpi_4_defconfig`, `fatload`, and `go`; Cohesix may use a staged `bootm`
DTB handoff where documented, but runtime driver authority still starts only
after seL4 transfers control to root-task.

Stop conditions:

- The target platform is not the expected Pi 4/aarch64 build.
- The local generated seL4 headers do not expose the object/API labels the
  driver code intends to invoke.
- Required device-untyped coverage is missing for the MMIO or PCIe aperture.

### 2. Device Description Evidence

Prove the physical address range, size, IRQ line, DMA addressing, reset/power
dependencies, and clock dependencies from source authority before mapping
anything.

For Pi 4 GENET:

- Mainline Linux declares `ethernet@7d580000` with compatible
  `brcm,bcm2711-genet-v5`, 64 KiB register span, SPI IRQs 157 and 158, and
  MDIO child `mdio@e14`.
- The Pi 4 board DTS enables GENET with PHY handle `phy1`, PHY address `1`,
  and mode `rgmii-rxid`.
- Cohesix HAL currently maps aliases around `0xfd58_0000`, `0x7d58_0000`,
  and `0xfe58_0000`; the HAL owns alias selection.

For Pi 4 Wi-Fi:

- Treat CYW43455 as an SDIO device behind the Pi 4 SDHCI path.
- WHD is useful for architecture: resource download, bus abstraction, SDPCM,
  CDC/BDC headers, and strict HAL split.
- OpenBSD `bwfm` is the first design reference for minimal SDIO Wi-Fi shape.
- Linux `brcmfmac` is a recovery and edge-case reference, especially firmware,
  NVRAM, Function 1/2, clock, and SDPCM behavior.

For Pi 4 xHCI/VL805:

- Treat PCIe/xHCI ownership as a separate HAL proof. A captured BAR or
  bootloader stop-state is not enough to touch runtime rings.
- Live PCI config, BAR translation, COMMAND state, MSI/INTx policy, and reset
  state must be proved through HAL before controller ownership is published.

Stop conditions:

- Address provenance is only a captured log with no DT/generated-header tie-in.
- An IRQ number collides with a known seL4/kernel source such as the Pi 4 timer.
- A bootloader state snapshot is being treated as runtime authority.

### 3. HAL Ownership Evidence

All direct MMIO, physical-address selection, IRQHandler creation, PCI config,
SDIO host access, firmware power/reset, and DMA frame allocation belong in HAL
modules. Driver modules may manipulate device registers only through mapped
regions returned by HAL.

The HAL must prove:

- `device_coverage(paddr, PAGE_BITS)` succeeds before `map_device(paddr)`;
- mapping code preserves page alignment and maps every page in the aperture;
- IRQ bindings are created through `bind_irq_notification`;
- IRQ ack happens only after the device source is cleared;
- DMA buffers are allocated through HAL and pinned through `hal::dma`;
- cache maintenance is tied to the generated cache policy;
- all `unsafe` blocks document the invariant with `SAFETY:`.

Stop conditions:

- A driver owns raw physical address discovery.
- A driver retypes untyped memory directly.
- A driver creates IRQHandler capabilities directly.
- A driver calls firmware or boot services after seL4 handoff.
- DMA uses ordinary stack/global buffers or untracked heap memory.

### 4. Minimal Polled Bring-Up

Start with polling, not interrupts. The first device proof should do the least
that can distinguish "mapped and alive" from "wrong address or unowned".

Examples:

- UART: read/write status and one byte path.
- GENET: read version/status, configure MAC/PHY minimally, prove link poll,
  and emit bounded register breadcrumbs before network stack integration.
- SDIO Wi-Fi: enumerate card, prove CCCR/FBR values, prove Function 1 register
  windowing, and stop before Function 2 traffic until firmware gate evidence is
  valid.
- xHCI: prove live PCI config/BAR/COMMAND and a no-touch ownership verdict
  before any ring or RUN writes.

Stop conditions:

- The code needs an interrupt to prove the register block exists.
- The code enters a protocol stack before a hardware-status proof exists.
- The first proof can hang indefinitely or lacks a bounded timeout.

### 5. IRQ Delivery

Use seL4's proven interrupt pattern:

1. Derive an IRQHandler from `seL4_CapIRQControl`.
2. Bind it to a notification.
3. Wait or poll the notification.
4. Clear the device's interrupt source.
5. Acknowledge the IRQHandler.

The seL4 interrupts tutorial documents that further interrupt delivery is
blocked until the handler is acknowledged. For level-triggered Pi 4 device IRQs,
acknowledging before clearing the device source can immediately redeliver or
wedge the line.

Cohesix guardrails:

- Use `IrqTrigger::Level` for Pi 4 SDIO and PCIe INTx-style lines unless the
  platform description proves an edge line.
- Badge notifications by IRQ number so one notification object can be audited.
- Keep the device-clear callback bounded and nonblocking.
- Do not add background IRQ workers unless the active milestone explicitly
  allows them.

### 6. DMA And Cache Coherency

DMA is where seL4's verification assumptions can stop helping us. seL4's
verified-configuration documentation states that current verified configurations
do not account for device address translation, and the CAmkES DMA reference
notes that without an IOMMU devices DMA to physical memory. Without SMMU/IOMMU
proof, the DMA-capable driver and device must be trusted.

Cohesix rules:

- Allocate device-visible frames through HAL only.
- Pin every shared range through `hal::dma::pin`.
- Use `seL4_ARM_Page_Uncached` where the driver contract requires uncached
  mappings.
- For cached mappings, run generated-policy cache maintenance before device
  ownership transfer and after reclaim.
- Use AArch64 VSpace cache operations only through `hal::cache`.
- Never DMA into stack memory, unbounded heap buffers, or protocol parser
  storage.
- Keep descriptor rings and packet buffers fixed-size and auditable.

seL4 13 notes for AArch64:

- Check `CONFIG_AARCH64_USER_CACHE_ENABLE` in local generated config. seL4 13
  documents user-level VA cache maintenance access for AArch64 when enabled.
- seL4 13 changed AArch64 VSpace object naming and behavior relative to older
  releases; do not use old `PageDirectory`/`PageUpperDirectory` assumptions.
- Use local `seL4/build/libsel4/include` and generated invocation labels as the
  final API contract.

Stop conditions:

- A device-visible address is guessed from a CPU physical address without a bus
  address policy.
- Cache clean/invalidate is ad hoc at individual call sites.
- A ring can be advanced without proving descriptor ownership and memory
  visibility.

### 7. Protocol And Cohesix Integration

Only after the hardware path is stable may the driver join higher-level
Cohesix behavior:

- Network devices implement existing `NetDevice`/smoltcp integration.
- The authenticated TCP console remains the only in-VM TCP listener.
- DHCP is client-only and bounded.
- Wi-Fi policy is `off|static|dhcp` plus `wired|wifi|auto` only where the
  manifest and U-Boot DTB handoff allow it.
- Console grammar, ACK/ERR/END behavior, `/proc` layouts, and NineDoor errors
  do not change unless the breaking-change process is followed.

Stop conditions:

- A hardware workaround adds a new command grammar or hidden RPC path.
- A driver-specific debug surface is available on nonmatching profiles.
- A protocol parser accepts unbounded user-controlled input.

## Driver-Specific Guardrails

### GENETv5 Wired NIC

Source order: Linux `bcmgenet` behavior, Linux BCM2711/Pi 4 DT bindings,
then U-Boot `bcmgenet` sanity checks.

Required Cohesix shape:

- HAL owns MMIO alias selection and maps the whole register aperture.
- Driver owns GENET register programming only after HAL returns mapped pages.
- MDIO polling is bounded; PHY address is validated against platform evidence.
- TX/RX descriptors are fixed-size; RX buffers are preallocated.
- TX completion and RX producer/consumer movement have breadcrumbs for stalls.
- DMA address policy is explicit (`physical` vs VC/bus alias) and logged.
- No DHCP logic is inside the GENET driver; DHCP belongs above `NetDevice`.

Do not proceed to smoltcp until link and one bounded RX/TX smoke path are
observable.

### CYW43455 Wi-Fi

Source order: OpenBSD `bwfm`, Infineon WHD HAL split, Linux `brcmfmac` SDIO
edge behavior.

Required Cohesix shape:

- HAL owns SDHCI/MMIO, GPIO/power/reset, firmware bundle access, and SDIO
  CMD52/CMD53 transport.
- Driver owns CYW43 protocol state: firmware/NVRAM/CLM staging, SDPCM, CDC/BDC,
  association, and Ethernet dataplane.
- Function 2 traffic is forbidden until firmware release, real
  `CHIPCLKCSR.HT_AVAIL`, and live Function 2 readiness are proven.
- The Linux F2 enable timeout shape remains a 3000-sample CCCR `IORx` F2-ready
  window, but `FORCE_HT` / `0x50 -> 0x52` evidence is diagnostic only and cannot
  replace HT or `IOR2`.
- NVRAM normalization is deterministic and logged.
- Firmware upload proof distinguishes proven byte mismatch from readback
  unavailable.
- Diagnostic no-HT or forced clock paths must be explicit. They are not
  production promotions unless a future milestone adds a separate hardware proof
  and updates this guide in the same change.
- Pi 4 CYW43455 firmware upload follows the ARMCR4 path: Function 1 backplane
  control, firmware/NVRAM into ARMCR4 RAM, reset-vector release, then Function 2
  readiness. CM3-only SOCSRAM remap writes are not part of this path.
- Wi-Fi credentials remain bounded: SSID 1-32 printable ASCII bytes; PSK empty,
  8-63 printable ASCII bytes, or 64 ASCII hex digits.

Do not debug DHCP over Wi-Fi until association and the first CYW43 Ethernet
frame path are proven independently.

### xHCI/VL805 Local Seat

Required Cohesix shape:

- PCIe root-complex and VL805 BAR/COMMAND proof belongs to HAL.
- Bootloader stop-state evidence is diagnostic. Current Pi 4 USB profiles have
  no xHCI ownership handoff opt-in; stop-state, preserve-state, and U-Boot
  reset-authority evidence must fail gate proof instead of authorizing rings.
- MSI remains disabled unless the milestone explicitly proves it.
- Keyboard enumeration must use live cold-boot root-port sampling and reset
  after command/event-ring proof; Linux/U-Boot captures are layout diagnostics
  only and do not authorize deferred port enumeration. Any U-Boot-stale keyboard
  cleanup must be a post-command-proof Cohesix root-port reset, not a
  bootloader-state handoff.
- Cold-owned high-BAR xHCI follows U-Boot's Pi 4 PCI xHCI path and does not
  write the Broadcom generic wrapper `AXIWRA/AXIRDA` registers into the VIA
  VL805 BAR. Root-port reads known to be toxic still stay behind explicit HAL
  gates and fresh command-completion proof.
- Root-port reads known to be toxic must stay behind explicit HAL gates.
- USB keyboard feeds only the existing root-console parser after decoding USB
  HID Usage Page `0x07` keyboard usages to bounded ASCII/control bytes.
- Operator proof uses a single 10-gate USB ladder: 1 controller candidate, 2 live
  PCIe/VL805 ownership, 3 controller-ready, 4 command-ring completion, 5
  root-port connection, 6 device address, 7 descriptors/configuration, 8 HID
  keyboard ready, 9 first HID report, and 10 first console byte. `usb status`,
  `usb probe-kbd`, and `scripts/pi4_trace_normalize.py` must preserve
  `proof_gate` evidence rather than silently inferring only the current blocker.

Do not use xHCI as a general USB stack. Milestone 26 local seat is keyboard
input plus primitive HDMI text output only.

### UART

Required Cohesix shape:

- UART is the minimal debug lifeline, so it must initialize early and fail
  explicitly.
- Mini-UART and PL011 selection is profile/platform-derived. On the Pi 4
  U-Boot path, runtime serial probing prefers mini-UART before Pi 4 PL011 so
  the root-console prompt uses the same serial lane as the seL4/U-Boot debug
  capture; selecting the mapped PL011 first can make logs visible while the
  interactive prompt and input go to the wrong UART.
- RX/TX buffering is bounded and parser input is sanitized.
- UART debug output must not mask hardware proof failure with excessive logs.

### Future Devices

Before implementing a new device, add its manifest device declaration and update
docs describing:

- role and milestone authorization;
- physical address and IRQ source;
- HAL trait surface;
- DMA/cache policy;
- reset/power/clock authority;
- operator-visible evidence;
- tests and hardware proof commands.

## Compliant Test Coverage Matrix

Cohesix has two release driver targets:

- `release-qemu`: QEMU `aarch64/virt` with serial, TCP (`net-console`),
  VirtIO networking, USB, and cache-maintained DMA.
- `release-pi4`: Raspberry Pi 4 with serial, TCP (`net-console`), local
  seat, GENETv5, CYW43455 Wi-Fi, USB, PCIe/VL805, SDIO, MMIO, and
  cache-maintained DMA.

Both release bundles include TCP and USB; the `usb` feature owns the
`usb-oxide` dependency so USB cannot silently disappear from a release-target
compile. Do not test driver changes with ad hoc feature strings when the target
bundle applies. Use the focused aliases:

- `cargo test -p root-task --no-default-features --features driver-tests-qemu --lib`
  is not the staged command because it runs unrelated root-task tests too; use
  the focused filters below instead.
- `cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::rtl8139`
  covers the QEMU RTL8139 fallback PCI/MMIO contract.
- `cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::virtio`
  covers QEMU VirtIO MMIO identification, bounded ring ownership, and DMA
  cache hooks.
- `cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::pci`
  covers HAL PCI topology lookup semantics.
- `cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::virtio_mmio`
  covers QEMU VirtIO slot bounds and register mapping authority.
- `cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::uart`
  covers QEMU and Pi 4 UART physical address constants.
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::bcmgenet`
  covers GENET descriptor, ring, link, and DMA address invariants.
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::cyw43`
  covers CYW43 protocol state, Linux-shaped join payloads, first-reply
  recovery, and bounded SDPCM/CDC behavior.
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::bcmgenet`
  covers GENET HAL MMIO coverage and DMA policy.
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_pcie`
  covers BCM2711 PCIe/VL805 BAR, INTx/MSI, posted-write, DMA-window, and page
  mapping contracts.
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi`
  covers SDIO, CYW43455 firmware/HT/Function 2 gates, mailbox, and R5 error
  contracts.
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::`
  covers target-neutral local-seat parser, queue, mirror, and USB/Wi-Fi command
  policy helpers.
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat_pi4::driver_coverage_tests::driver_coverage_pi4_local_seat_usb_vl805_dma_contracts`
  covers Pi 4 local-seat USB/VL805/xHCI policy, Enable Slot plus Disable Slot
  command-ring proof, event-ring polling, PCIe DMA aliasing, and HAL
  interrupt-source ordering.
- `cargo test -p root-task --no-default-features --features cache-maintenance --test cache_maintenance`
  covers HAL cache-clean/invalidate/error paths and DMA pin/sync/unpin audit
  ordering.
- `SEL4_BUILD_DIR=$REPO/seL4/SMP_build cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-qemu`
  proves the QEMU release bundle builds against the seL4 target artifacts.
- `SEL4_BUILD_DIR=$REPO/seL4/build_UBOOT cargo check -p root-task --target aarch64-unknown-none --no-default-features --features release-pi4`
  proves the Pi 4 release bundle compiles the target-only local-seat/USB path.
- `python3 scripts/ci/check_driver_test_coverage.py` verifies that this matrix,
  `docs/TEST_PLAN.md`, the staged runner, release feature bundles, and critical
  driver/HAL test tokens stay aligned.

## Review Checklist

Use this before merging any driver change:

- The exact `docs/BUILD_PLAN.md` milestone/task authorizes the work.
- The driver cites source provenance in docs or comments when behavior follows
  Linux/U-Boot/OpenBSD/WHD.
- Local `seL4/build/` generated artifacts were checked for object/API/IRQ truth.
- MMIO, IRQ, DMA, PCI, SDIO, power/reset, and firmware service calls are HAL
  owned.
- Polled proof exists before IRQ use.
- Device source is cleared before IRQHandler ack.
- DMA buffers are HAL-allocated, pinned, and cache-maintained.
- All loops that wait for hardware are bounded and emit exact blocker labels.
- No new in-VM listener, RPC path, shell grammar, or POSIX facade was added.
- QEMU behavior remains compatible unless a profile gate explicitly changes it.
- Pi 4 hardware evidence is not claimed from QEMU.
- Tests cover touched logic paths; hardware-only behavior has deterministic
  capture commands and expected evidence lines.

## Standard Task Template

Use this template for driver tasks:

```text
Title/ID: <build-plan task id>
Goal: <one sentence>
Inputs: <seL4 generated artifacts, manifest, DT/source references, local files>
Changes:
  - <file> -- <summary>
Commands:
  - rg -n "ARCH_AARCH64|PLAT_|ARM_GIC|SMMU|AARCH64_USER_CACHE|IRQ" seL4/build/kernel/gen_config/kernel/gen_config.yaml seL4/build/kernel/gen_headers/plat/platform_gen.h
  - python3 scripts/ci/check_driver_test_coverage.py
  - cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::rtl8139
  - cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::virtio
  - cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::pci
  - cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::virtio_mmio
  - cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::uart
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::bcmgenet
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib drivers::cyw43
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::bcmgenet
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_pcie
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat_pi4::driver_coverage_tests::driver_coverage_pi4_local_seat_usb_vl805_dma_contracts
  - cargo test -p root-task --no-default-features --features cache-maintenance --test cache_maintenance
  - scripts/ci/test_plan_run.sh --list
  - scripts/ci/test_plan_run.sh --state-dir out/test-plan/<run-id>
Checks:
  - <capability/MMIO/IRQ/DMA proof>
  - <driver-specific bounded proof>
  - <no protocol drift>
Deliverables:
  - <code/tests/docs/logs>
```

## Practical Debug Order

When a Pi 4 driver stalls, debug in this order:

1. Boot path and manifest profile.
2. seL4 generated platform config and device-untyped coverage.
3. HAL map/probe breadcrumb.
4. Single register read from the expected block.
5. Reset/power/clock prerequisite.
6. Polled hardware status with bounded timeout.
7. IRQ binding and notification delivery.
8. Device-source clear plus IRQ ack ordering.
9. DMA address, descriptor ownership, and cache maintenance.
10. Protocol state machine.
11. Cohesix console/NineDoor/net integration.

If a later step fails, preserve the last known-good proof and add the narrowest
breadcrumb needed to distinguish the next frontier. Do not add broad logging,
new protocol surfaces, or speculative alternate paths.
