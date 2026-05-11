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
- VL805/xHCI command-ring `CRCR` publication writes the high dword before the
  low dword because the low dword carries command-ring control bits. Event-ring
  `ERDP.EHB` acknowledgements are also split into explicit 32-bit MMIO writes
  with the high dword before the low/control dword, then `IMAN.IP`
  write-one-to-clear is issued before the Linux-shaped polled command proof
  re-enables `IMAN.IE`.
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
  early kernel net init, then the runtime resumes the saved Wi-Fi configuration
	  before announcing the root console. The `Cohesix console ready` banner and
	  `cohesix>` prompt are emitted only after the deferred CYW43 net-console path
	  reports a reachable address state (`manifest-static`, `dev-virt`, or
	  `dhcp-lease`). Pending, failed, disabled, and unknown future address states
	  are not reachable. QEMU virtio and Pi 4 wired NIC paths still use their
	  immediate net-init flow and are not held by the Pi 4 Wi-Fi gate.
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
- Current Pi 4 USB/VL805 is event-ring polled with PCI INTx/MSI delivery
  masked. The cold-boot command proof may program Linux-shaped xHCI
  event-generation state (`USBCMD.INTE`, `IMOD`, `IMAN.IE`) only while PCI/GIC
  delivery remains masked and the event ring is consumed by polling; do not
  unmask external xHCI interrupt delivery until a milestone explicitly proves
  it.

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
  The driver queries `clmver` after upload before continuing with the remaining
  preinit commands.
- CLM must not be the first meaningful BCDC/iovar exchange. Linux proves the
  initial control-plane path with small iovars first: set
  `bus:txglomalign=8`, query `ulp_sdioctrl` while accepting the captured
  `BCME_UNSUPPORTED` result, set `bus:rxglom=1`, then query
  `cur_etheraddr`. Cohesix follows that order before CLM so the first Function
  2 replies are small Linux-shaped 64-byte first-read transactions rather than
  a large CLM transfer.
- The first Linux-order control writes use the plain 12-byte SDPCM header. The
  8-byte SDPCM hardware-extension header is enabled only after `bus:rxglom`
  succeeds, matching `brcmfmac`'s `tx_hdrlen` transition. Sending
  `bus:txglomalign` with the extended header shifts the CDC payload to offset
  20 before the firmware has enabled that framing and leaves the host polling
  for a reply the firmware never generates.
- Non-captured early iovars must not be hard blockers before this attach proof
  point. Cohesix may attempt compatibility knobs such as `bus:txglom`, `apsta`,
  `country`, and `bsscfg:event_msgs`, but only after the Linux first-iovar/MAC
  sequence, and `BCME_UNSUPPORTED` on those non-captured knobs is nonfatal.
  Transport errors remain fatal.
- Cohesix also applies the Linux attach-time `join_pref` default payload
  (`04 02 08 01 01 02 00 00`) before scan/join defaults. WPA2-PSK join setup
  must match Linux ordering: configure infrastructure/auth/WPA auth first,
  enable firmware supplicant with plain `sup_wpa=1`, then program
  `WLC_SET_WSEC_PMK`. The PMK payload is the 132-byte Linux
  `brcmf_wsec_pmk_le` shape (`u16 key_len`, `u16 flags`, 128-byte key area).
  For 8-63 byte passphrases Cohesix derives the 32-byte PBKDF2-HMAC-SHA1 PMK
  from SSID + passphrase and sends flags `0`; for 64 ASCII hex PSKs it decodes
  the 32-byte PMK directly. Sending the raw passphrase in a short PMK struct is
  firmware-bad-argument drift, not a transport failure.
- Before the first iovar, Cohesix must drain the Linux-observed startup
  status/credit traffic emitted after `Dongle ready`. The captured first frame
  is a 12-byte SDPCM header-only event/status frame, followed by an empty
  64-byte read. That frame is not a CDC ioctl response and must be consumed
  before `bus:txglomalign`, otherwise the first ioctl wait can consume stale
  startup traffic and then spin on the SDIO-core status sideband instead of
  reading the real response.
- After real post-release HT and live Function 2 readiness are proved, the Pi 4
  firmware channel arms the Linux-shaped Function 2 interrupt path
  (`FUNCTIONINTMASK`, `CCCR.IENx`, and SDHCI `CARD_INT`) through HAL-owned
  source clear plus seL4 ack. IRQ 158 is the Wi-Fi SDIO interrupt; IRQ 27 remains
  the seL4 timer and is never Wi-Fi progress evidence.
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
- Wi-Fi net-console bring-up may gate the root console only on the Pi 4
  local-seat path that explicitly selects Wi-Fi (`wifi`, or `auto` with
  credentials). Cohesix must emit
  `action=root-console-wait-for-wifi`, resume the saved net-console
  configuration exactly once from the userland runtime, attach the resulting
  network stack to the event pump, and continue polling until the Wi-Fi
  transport is reachable before it emits `Cohesix console ready` or the
  `cohesix>` prompt. The pre-root wait polls only timer/network progress; it
  must not consume UART, local-seat, or net-console command input before the
  banner. A terminal deferred CYW43 init failure is not a progressing Wi-Fi
  stack; terminal association or DHCP failure is also not a progressing Wi-Fi
  stack. Those failures must release the serial diagnostic shell with the exact
  failure detail instead of spinning forever. A lingering
  `wifi-net-console-pending-before-root-console` state after the prompt is drift:
  it means the prompt escaped before Wi-Fi net-console readiness was proved.
- Pi 4 local-seat USB is not a reason to disable Wi-Fi diagnostics. The serial
  console retains the HAL-backed Wi-Fi debug path after root-console handoff so
  `wifi diag`, `wifi load-fw`, and `wifi retry` can exercise CYW43455 without
  preventing boot from reaching the shell. Once a terminal boot/control-plane
  failure is preserved, `wifi diag` is passive: it emits the before/after state
  from cached evidence and skips the long live HT re-probe. Operators can still
  run the explicit `wifi probe-ht` command when they want the stateful HT probe.
- Wi-Fi association completion is event-pump driven for both explicit `wifi`
  and `auto` interface policies. The driver may issue the join command during
  boot, but root-console publication on the Pi 4 Wi-Fi net-console path must
  wait until the event pump has advanced association and DHCP/static addressing
  to a reachable net-console state.
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
  `Mem+ BusMaster+`.
- HAL reselects VL805 `01:00.0` via BCM2711 `EXT_CFG_INDEX` before each
  `EXT_CFG_DATA` access and rejects selector echoes.
- Ownership can promote only on exact live `1106:3483`, class `0x0c0330`,
  BAR0 translation, COMMAND readback, and MSI-disabled poll-only proof.
- If the exact VL805 tuple appears with an unassigned 64-bit BAR, HAL may assign
  the Pi 4 outbound-window BAR value through EXT_CFG and read it back. Do not
  assign BARs for bad IDs, bad class, selector echoes, absent link proof, or
  any other tuple.
- Doorbell, Linux-shaped command-event setup, and `USBCMD.RUN` posted-write
  flushes use HAL-owned BCM2711 EXT_CFG selector/COMMAND readback only. The
  2026-05-10 Pi 4 trace proved that even an xHCI capability dword read at BAR
  offset `0x0000` can halt immediately after `USBCMD.RUN`; never use xHCI BAR
  reads, `USBSTS`, or any `PORTSC` as the posted-write drain on this
  prompt-safe path. Command-timeout recovery follows the same rule:
  stop/reset/RUN recovery waits are blind bounded settles, not live
  `USBCMD`/`USBSTS` polls. The active `platform-reset-complete` path uses an
  extended bounded blind HCRST/CNR settle before publishing fresh rings, and
  command-timeout recovery reuses that extended settle before retrying the first
  command. Command timeout diagnostics on that lane report deferred state
  instead of live `CRCR`, `DCBAAP`, interrupter, or `PORTSC` reads.
- RUN, command-ring recovery, and command-doorbell posted-write drains fail
  closed when the HAL cannot prove the EXT_CFG selector, link/root readiness,
  PCIe IRQ-source masking, or poll-only VL805 COMMAND ownership, or when the
  drain read returns a selector echo or invalid config value.
- Linux-shaped command/event proof must DMA-publish the command TRB first,
  issue a device-visible command-ring publish barrier, then drain its own
  `DNCTRL=0x2`, `IMOD=0xa0`, `ERDP.EHB`, `IMAN.IP`, `IMAN.IE`, and
  `USBCMD.RUN|INTE` writes immediately before ringing the doorbell. `ERDP.EHB`
  acknowledgement drains both the high DMA-alias dword and the low/control
  dword in order. A missing platform posted-write hook is a hard failure on the
  Pi 4 high-BAR VL805 path.
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
  run bounded live settle/sampling passes, and reset root ports through the
  Cohesix-owned controller. Linux or U-Boot captures must not synthesize a
  connected mask, speed, enabled-port state, or skipped root-port reset.
- The Pi 4 prompt-safe high-BAR lane proves command/event-ring consumption with
  Linux-shaped command-event state (`USBCMD.RUN|INTE`, `IMOD=0xa0`, `IMAN.IE`)
  while PCI INTx/MSI/GIC delivery remains masked. For the fresh Pi 4
  platform-reset path it preserves already posted port-status events in the
  event ring, matching the Linux capture where the first Port Status Change
  Event precedes the first Enable Slot Command Completion Event. It submits the
  Linux-captured cold-boot first command shape, a bounded Enable Slot, then the
  command wait skips any leading non-command events before accepting the Enable
  Slot completion and reopening live root-port sampling. Each skipped event
  republishes `ERDP.EHB` and drains the high/low dword writes through the HAL
  posted-write hook before polling the next event-ring slot, so VL805 can see
  the advanced dequeue pointer before it posts the command completion. On
  success it immediately submits bounded Disable Slot
  cleanup for that slot; a cleanup failure is logged and tolerated only as
  command-ring proof, and later enumeration must not assume a pristine slot
  table. Only a completed command may reopen live root-port sampling.
- The current boot-compatible keyboard/trackpad combo is treated as a normal
  HID composite device. Linux captures identify SINO WEALTH `258a:0f0a`, with
  interface 0 as Boot Keyboard (`Sub=01`, `Prot=01`) and interface 1 as a
  protocol-none touchpad. Cohesix local-seat enumeration must rank the Boot
  Keyboard interface as the primary target; the protocol-none touchpad must not
  displace keyboard bring-up.
- The external VL805 high-BAR path must not apply the generic Broadcom xHCI
  wrapper AXI read/write attribute quirk. On Pi 4 that quirk's `0x0c08/0x0c0c`
  offsets are not part of the live VL805 PCI controller contract; the
  root-complex AXI/outbound-window setup belongs to the HAL-owned BCM2711 PCIe
  path and posted-write drains still go through HAL-owned EXT_CFG readback.

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
  only and do not authorize deferred port enumeration.
- Cold-boot high-BAR xHCI must not touch generic Broadcom wrapper AXI
  attribute registers; BCM2711 PCIe AXI/outbound-window setup is a HAL
  root-complex responsibility, not a VL805 BAR responsibility.
- Root-port reads known to be toxic must stay behind explicit HAL gates.
- USB keyboard feeds only the existing root-console parser.

Do not use xHCI as a general USB stack. Milestone 26 local seat is keyboard
input plus primitive HDMI text output only.

### UART

Required Cohesix shape:

- UART is the minimal debug lifeline, so it must initialize early and fail
  explicitly.
- Mini-UART and PL011 selection is profile/platform-derived.
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
  covers CYW43 protocol state, first-reply recovery, and bounded SDPCM/CDC
  behavior.
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
