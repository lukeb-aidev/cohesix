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
- Milestone 26a: `m26a-bcmgenet-driver` and the original Pi 4 wired/static baseline.
- Milestone 26b: DHCP plus the profile-gated CYW43455 Wi-Fi path; the checked-in
  Pi 4 U-Boot manifest now defaults to DHCP/`auto`.

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
   `apps/root-task/src/hal/pi4_wifi.rs`,
   `apps/root-task/src/hal/pi4_pcie.rs`, `apps/root-task/src/sel4.rs`,
   `apps/root-task/src/drivers/driver_task_net.rs`,
   `apps/root-task/src/local_seat.rs`, and
   `apps/pi4-driver-runtime/src/lib.rs`.
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

The active Pi 4 USB implementation is the linked `usb-local-seat` runtime built
from `apps/pi4-driver-runtime` and packaged under the generated
`pi4-driver-usb` artifact name. The root-task `usb` Cargo feature exposes only
the ring-client local-seat surface; it does not select a root-task USB
implementation crate. Any reintroduction of an external USB package, generated
Cargo patch, or root-task USB support crate is driver-model drift and must be
fixed in the same change.

## seL4 Driver Model

seL4 does not provide Linux-style in-kernel drivers. The root task receives
authority over remaining resources and must deliberately delegate or retain
that authority through capabilities. The kernel provides the mechanisms:

- device memory can be exposed to user-level code by retyping device untyped
  memory into frames and mapping those frames into a VSpace;
- IRQs can be represented by IRQHandler capabilities and delivered as
  notification signals;
- CPU time is controlled by TCBs, priorities, domains, and scheduling-context
  policy where the selected seL4 profile exposes it; non-MCS profiles still
  enforce driver budgets through priority/domain assignment plus bounded
  IPC/poll turns;
- DMA safety is outside the standard verified proof unless an IOMMU/SMMU
  configuration constrains device writes.

For Cohesix, the root task remains the privileged resource-admission and seL4
bootstrap authority. Pi 4 hardware service is owned by linked driver runtimes
after HAL admission, and drivers must depend on the narrow HAL trait that
represents the resource they need:

- `DeviceHal`: MMIO, device-untyped coverage, DMA frames, DMA guard pages,
  and IRQ notification binding/acknowledgement.
- `PciHal`: generic PCI discovery/configuration for platforms with a HAL-owned
  topology. Do not assume this is the active Pi 4 VL805 path; current
  `KernelHal::pci_topology()` returns `None`, and Pi 4 VL805 ownership is
  proven by `apps/root-task/src/hal/pi4_pcie.rs`.
- `Cyw43Hal`: firmware bundle admission for the linked CYW43/SDIO runtimes;
  SDIO, power/reset, and Wi-Fi transport service belong to linked-runtime
  descriptors, not root-owned HAL calls.

The compatibility `Hardware` facade may remain where legacy call sites span
several domains, but new driver logic should prefer the narrowest trait.

### HAL And Driver Architecture

The architecture below shows the current authority path. The manifest compiler
declares driver images, affinity, and policy; root-task validates and maps the
resources through HAL; linked Pi 4 runtimes receive only bounded descriptors,
capabilities, and fixed-ring service turns.

```mermaid
flowchart TD
    manifest["configs/root_task*.toml"]
    rtc["coh-rtc validation and codegen"]
    tables["root-task generated tables"]
    specs["root_task.driver_images specs"]
    policy["affinity and boot policy"]
    root["root-task authority"]
    retained["tickets, namespaces, console, revocation"]
    hal["HAL admission boundary"]
    contracts["driver-task contracts and budgets"]
    caps["seL4 caps, VSpaces, MMIO, DMA, IRQs"]
    abi["pi4-driver-abi init descriptor"]
    boot["driver-task bootstrap"]
    images["linked pi4 driver images in CPIO"]
    rings["fixed command and completion rings"]
    devices["Pi 4 devices and shared buffers"]
    proof["breadcrumbs and owner-state proof"]
    compat["QEMU and host compatibility gates"]

    manifest --> rtc
    rtc --> tables
    rtc --> specs
    rtc --> policy
    tables --> root
    policy --> root
    root --> retained
    root --> hal
    specs --> boot
    hal --> contracts

    subgraph layers["HAL layers"]
        deviceHal["DeviceHal: MMIO DMA IRQ"]
        pciHal["PciHal: PCI topology"]
        cywHal["Cyw43Hal: firmware bundle admission"]
        pcieHal["pi4_pcie: VL805 root complex"]
    end

    contracts --> deviceHal
    contracts --> pciHal
    contracts --> cywHal
    contracts --> pcieHal
    deviceHal --> caps
    pciHal --> caps
    cywHal --> caps
    pcieHal --> caps
    caps --> boot
    abi --> boot
    boot --> rings

    subgraph runtimes["Driver tasks in isolated child VSpaces"]
        entry["runtime entrypoints"]
        serial["serial runtime"]
        usb["USB local-seat runtime"]
        hdmi["HDMI runtime"]
        genet["GENET runtime"]
        cyw43["CYW43 runtime"]
        sdio["SDIO runtime"]
        pcie["PCIe runtime"]
        entry --> serial
        entry --> usb
        entry --> hdmi
        entry --> genet
        entry --> cyw43
        entry --> sdio
        entry --> pcie
    end

    images --> entry
    rings --> entry
    entry --> devices
    entry --> proof
    devices --> proof
    compat -. "diagnostic only" .-> hal
    compat -. "not Pi owner proof" .-> proof
```

The diagram is intentionally split between runtime transport and owner-state
proof. The generated manifest keeps seven Pi 4 runtime images
acceptance-eligible, including `sdio-host` with its HAL-declared SDHCI MMIO
page, but a physical boot creates the selected-only active set for the requested
network role. A Wi-Fi boot uses the common local-seat/serial/display/PCIe set
plus `sdio-host` and `cyw43455`; a wired boot uses the common set plus `genet`.
GENET and CYW43 are acceptance-eligible concurrently in the generated manifest,
but physical Pi driver performance and liveness claims assume one active network
dataplane owner at a time unless a later milestone explicitly changes the boot
policy and adds multi-network arbitration proof.
Fresh Pi evidence must still prove the active hardware state machines make real
progress from driver-local state. The transport boundary is no longer a
one-page smoke loader: root now maps bounded multi-page `PT_LOAD` runtime images
and semantic MMIO/DMA/shared resource ranges before submitting the pointer-free
init descriptor. The linked runtime must publish generic lifecycle progress
before root credits the task as live: `runtime-entry-ready` means the no-std
entry path installed the mapped IPC buffer, and `runtime-recv-ready` or
`runtime-poll-ready` means the runtime has entered the command-intake loop and
can accept a descriptor-replay turn through the command endpoint or the
sequence-last shared-ring path. Root publishes shared-ring commands by staging a
zero-sequence record plus reset completion, cleaning staged payload bytes and
the command/completion records from the root VSpace, issuing the shared-memory
store barrier, writing the real sequence last, cleaning the command record
again, and issuing the barrier again so the runtime never consumes a partial or
stale record. Root invalidates completion and progress records before consuming
linked-runtime replies. Root does not clear the child-owned progress marker on
submit; timeout telemetry must preserve the last runtime marker and use the
request sequence to identify the active turn. Runtime shared-control,
shared-payload, bus-owner, and command-ring pages are HAL-mapped into every
participant VSpace, so linked runtimes use load/store barriers plus volatile
reads and writes for command intake, descriptors, payload windows, progress
records, and bus-owner completions. Descriptor-declared device DMA buffers keep
their explicit clean/invalidate path.

Driver-task submit concurrency is per generated driver-task contract. Any
root-to-runtime turn that carries ring bytes, shared-payload bytes, or a runtime
init descriptor must describe those bytes as staged segments and submit through
the staged command/service API; the submit path validates the ranges, computes a
byte-sensitive staging fingerprint, acquires the per-contract active slot, then
copies the staged bytes and publishes the command record. The unstaged ring
helpers are for zero-frame control/poll/init/probe turns, HAL helper definitions,
tests, and runtime-to-root completion publication only. While a contract is
active, bounded resend/poll slices may resume only the same request identity:
role, hot path, opcode, aux words, budget, command flags, frame descriptor, and
the fingerprint of every staged byte must match. A different payload or
descriptor while the active slot is owned is a busy condition; it must not
rewrite the ring page, shared-payload window, or completion slot.

### Historical Pi 4 Baseline Mapping

The May 18-20 Pi 4 captures are evidence baselines for behavior the new
architecture must preserve, not authority to restore root-resident drivers. They
prove useful expectations such as a clean serial shell, cold USB/VL805 progress,
USB keyboard input, selected Wi-Fi DHCP progress, and visible local-seat
feedback; reopened 26a/26b closure still requires fresh driver-task owner-state
proof from the linked-runtime model.

- HAL owns resource admission, MMIO mapping, DMA publication, IRQ binding and
  acknowledgement, board-level mailbox reset/power calls, and BCM2711
  PCIe/VL805 prep. Steady SDIO/CYW43 command service belongs to linked runtimes
  after descriptor replay. Old U-Boot, Linux, or root-resident observations may
  define expected order and diagnostic breadcrumbs, but they do not become
  runtime authority.
- Child driver runtimes own bounded steady work only after HAL has admitted the
  resources and root has delivered the pointer-free descriptor. Serial RX/TX,
  HDMI frame submission, USB xHCI/HID polling, GENET RX/TX, CYW43 SDPCM/control,
  SDIO CMD52/CMD53/POLL_IRQ, and PCIe read/write/flush turns must return through
  the runtime ring or fail closed.
- May 18-19 Wi-Fi proof also included station control and WPA2 behavior: ordered
  SDPCM/CDC ioctl response matching, event-mask/setup drains, association,
  EAPOL M1/M2/M3/M4, PTK/GTK key install, and only then DHCP/data release. Under
  the new model that behavior must live behind the linked `cyw43455` runtime and
  pointer-free command ABI. A PSK network is not DHCP-ready while host EAPOL is a
  root-side stub or while control replies are not matched by the runtime.
- The first root prompt and serial input are the safety boundary. No USB, HDMI,
  PCIe, SDIO, GENET, CYW43, DHCP, or log-stream proof may hold them hostage;
  unproved work must leave deferred/no-reply breadcrumbs and retry only through
  bounded prompt-side service paths.
- HDMI needs visible mirror proof separate from early owner-state. Descriptor
  load or engine-init evidence does not prove display acceptance until a bounded
  text/framebuffer render is visible or fails red with an explicit diagnostic
  mirror marker.
- USB must replay PCIe/VL805 descriptor state and complete HAL-owned live
  PCIe/VL805 adoption before xHCI engine init. The linked PCIe child adopts that
  descriptor as the bounded port read/write/flush owner; a separate PCIe
  engine-init reply is diagnostic evidence, not a pre-USB hard gate. Missing
  EXT_CFG, BAR/COMMAND, DMA-window, or poll-only ownership proof is still a
  PCIe/VL805 blocker, not a reason to spin or trust old handoff state.
- USB runtime MMIO admission is high-BAR-only on Pi 4. The only accepted xHCI
  MMIO base is the HAL-proven VL805 BAR0 at `0x0000000600000000`; low aliases
  such as `0xfe980000` and `0x7e980000` are stale diagnostic references, not
  linked-runtime authority. PCIe owner replay remains the required
  BAR/COMMAND/VL805 admission proof before xHCI entry, but xHCI operational
  writes now drain inside the linked USB runtime through bounded same-runtime
  xHCI MMIO readback. Command and endpoint doorbells are barrier-only publish
  edges and never issue either same-window xHCI readback or a nested USB-to-PCIe
  child command.
- If xHCI engine-init reaches controller-ready before HID readiness, the linked
  USB runtime must keep enumeration alive through bounded prompt-side service:
  preserve current-boot Port Status Change events, submit the gate-4 Enable Slot
  proof once, poll its completion in bounded resume slices, power/sample root
  ports only after command completion, keep controller init separate from the
  explicit keyboard-enumeration aux command, retry hub/HID enumeration during
  bounded resume turns, report the last enumeration frontier during ordinary
  prompt polls, and publish owner-state only after first-report proof.
  Keyboard-ready is an endpoint frontier, not an owner-state proof.
- Wi-Fi replay is independent of USB. CYW43/SDIO descriptor replay and selected
  network progress must not wait for HID readiness, and USB polling must not
  mask Wi-Fi blockers; both lanes remain subordinate to serial/HDMI
  responsiveness and fail closed with separate frontier breadcrumbs.

### Driver-Task Scheduling Contracts

Reopened Milestones 26a and 26b require every hardware-facing driver path to
run behind a HAL-declared scheduling contract and a live seL4 driver TCB. The
contract layer is the admission boundary for every hardware service turn. A
service that has no contract, has an unbounded budget, blocks indefinitely, or
bypasses the HAL cannot be serviced.

Each contract records:

- stable driver name and hardware role;
- service class (`RealtimeInput`, `ConsoleOutput`, `NetworkControl`,
  `NetworkData`, `DisplayRefresh`, or `Background`);
- authority class (`DeviceOnly`, `ConsoleTransport`, `NetworkFrameTransport`,
  or `DisplaySink`);
- current isolation state (`DedicatedSeL4Task` for the built-in driver task
  model; any fallback service turn must be recorded separately as
  `RootTaskCompatibility`);
- per-turn operation, byte, frame/report, and bounded-spin budgets;
- bounded IPC/event queue depth.

Scheduling-context fields are profile-qualified for the target architecture. A
dedicated driver task uses seL4 scheduling contexts when the active MCS kernel
profile provides them. On non-MCS profiles, the same HAL contract is enforced
with TCB priority/domain policy plus bounded IPC and poll turns.

The Pi 4 source path attempts to create root-created, root-revocable child seL4
TCBs for every built-in linked-runtime hardware contract during boot. QEMU
virtio compatibility builds keep
the same contract declarations but deliberately skip live driver-task bootstrap
before network init (`qemu-virtio-pre-net-resource-guard`) so scarce seL4
object/page-table budget remains available for the virtio TCP regression path.
When the explicit `qemu-driver-task-smoke` feature is enabled, QEMU may run a
post-network smoke probe after virtio networking is online; that probe creates
the full nine-contract driver-task set and publishes partial live-TCB proof.
That is useful for exercising the seL4 task mechanics under QEMU, but it is not
Pi 4 hardware proof and cannot satisfy the full dedicated-driver-task closure gate.
The no-USB QEMU smoke profile is the smallest local runtime attempt while the
full USB smoke image remains above the current elfloader placement ceiling; an
`image load address overlaps with ELF-loader` failure is an image-placement
blocker, not driver-task proof. A boot counts as dedicated-driver-task execution
only when the breadcrumbs below prove live TCBs, valid
cap/fault/revoke/scheduling/affinity fields, and hot-path
dispatch through those TCBs.
The May 20 Pi 4 capture reached Wi-Fi DHCP but is explicitly pre-closure
evidence: all nine `DRIVER_TASK_BOOT` attempts failed with `seL4_DeleteFirst`,
`live_tcb_count=0`, and the hot paths remained root-task compatibility. The
current code treats boot-seeded intermediate page-table collisions as existing
VSpace state for driver IPC/stack mappings; fresh Pi proof is still required.

| Contract | Role | Manifest affinity target | Current VSpace | Current hot path |
| --- | --- | --- | --- | --- |
| `serial` | serial console | `serial` | isolated child VSpace using the linked `pi4-driver-serial` image; emergency early UART is bootstrap-only | bounded mini-UART init/RX/TX; fresh Pi proof still required |
| `usb-local-seat` | USB keyboard/local seat | `usb-local-seat` | isolated child VSpace using the linked `pi4-driver-usb` image | xHCI/HID service through declared xHCI MMIO, DMA/shared pages, and USB-to-PCIe bus link |
| `hdmi-text` | HDMI text mirror | `hdmi-text` | isolated child VSpace using the linked `pi4-driver-hdmi` image | bounded text frames through declared framebuffer metadata and resources |
| `bcmgenet-v5` | GENET wired NIC | `bcmgenet-v5` | isolated child VSpace using the linked `pi4-driver-genet` image | bounded GENET MMIO/DMA RX/TX plus MDIO/MAC setup |
| `cyw43455` | CYW43 Wi-Fi NIC | `cyw43455` | isolated child VSpace using the linked `pi4-driver-cyw43` image | shared-control SDPCM plus pointer-free CYW43-to-SDIO bus-link service |
| `rtl8139` | QEMU RTL8139 NIC | `rtl8139` | shared root VSpace | dedicated TCB service dispatch for active QEMU RTL8139 network polling |
| `virtio-net` | QEMU virtio-net NIC | `virtio-net` | shared root VSpace | dedicated TCB service dispatch for active QEMU virtio network polling |
| `sdio-host` | SDIO host for CYW43 | `sdio-host` | isolated child VSpace using the linked `pi4-driver-sdio` image | fixed-layout CARD_COMMAND/CMD52/CMD53/HOST_CONFIG/POLL_IRQ turns over the HAL-declared SDHCI page |
| `pcie-root` | Pi 4 PCIe root/VL805 support | `pcie-root` | isolated child VSpace using the linked `pi4-driver-pcie` image | bounded PCIe read/write/posted-write-flush turns over the HAL-prepared aperture |

The isolated-image contract is generated, not hand-authored in HAL.
`configs/root_task.toml` and `configs/root_task_pi4_uboot_aarch64.toml`
declare `root_task.driver_images` for `serial-console`, `usb-keyboard`,
`hdmi-text`, `genet-nic`, `cyw43-wifi`, `sdio-host`, and `pcie-root`;
`coh-rtc` emits those records into `apps/root-task/src/generated`; and the build
scripts stage the linked `pi4-driver-*` runtime images into the raw
driver-runtime CPIO embedded in the Pi 4 root-task image. The U-Boot-staged CPIO
is packaging evidence only. When stripped role images are byte-identical, the Pi
image script deduplicates the physical CPIO to one
`cohesix/bin/pi4-driver-runtime` entry, while the generated per-role specs remain
the authority for artifact names, code pages, stack pages, resources, and
hot-path admission.

Those binaries implement fixed command/completion ring service engines for the
active Pi 4 hardware owners. Serial handles bounded mini-UART init/RX/TX; HDMI
renders to a mapped framebuffer; PCIe services primitive MMIO read/write/flush
operations; USB owns the direct-root-port xHCI boot-keyboard path; GENET owns
bounded descriptor-ring RX/TX plus MDIO/MAC setup; CYW43 owns the shared-control
SDPCM command surface and pointer-free SDIO bus-link descriptor; and `sdio-host`
owns the HAL-declared SDHCI page plus fixed-layout CARD_COMMAND, CMD52, CMD53,
HOST_CONFIG, and POLL_IRQ service records. The generated runtime specs for
serial, USB, HDMI, GENET, CYW43, SDIO, and PCIe report
`root_context_required=false` and
`hardware_state_migrated=true`. Fresh Pi hardware proof is still required before
claiming production readiness for xHCI hub/timing, Wi-Fi association/DHCP, GENET
DHCP, HDMI scanout, serial I/O, and VL805 handoff.

The Pi 4 manifest default pins both network dataplane driver contracts to the
fourth core (`core=3`): `root_task.affinity.drivers.bcmgenet-v5=3` and
`root_task.affinity.drivers.cyw43455=3`. `coh-rtc` emits those fields into the
generated `DRIVER_AFFINITY_POLICY`; HAL maps the `bcmgenet-v5` and `cyw43455`
contracts to `DriverAffinityTarget::BcmGenetV5` / `DriverAffinityTarget::Cyw43455`.
Physical Pi 4 owner-state boots now call `seL4_TCB_SetAffinity` for each
bootstrap-created linked-runtime driver TCB before the driver TCB is resumed,
matching the existing NineDoor/worker affinity path. A boot may claim
fourth-core placement only when the corresponding `DRIVER_TASK_BOOT` line
reports `affinity_core=3` and the aggregate affinity proof remains applied; a
`DRIVER_TASK_AFFINITY_DEFERRED` line is a stale placement regression, not a
runtime-image or hardware-service success.
The same Pi 4 manifest now defaults the first boot to DHCP/`auto` networking and
requires the local-seat path, so a no-saved-policy boot exercises GENET DHCP and
fails visibly if the HDMI/USB runtime cannot initialize.

Physical Pi bootstrap creates only selected generated linked-runtime hardware
contracts from the acceptance-eligible set: serial, SDIO, PCIe, USB/local-seat,
HDMI text, CYW43, and GENET. Wi-Fi and wired boots activate only the selected
network runtime unless the manifest policy says otherwise. RTL8139 and
virtio-net remain QEMU compatibility contracts, not physical Pi hardware proof.

For each physical Pi driver TCB, HAL allocates the TCB, child CNode, command
endpoint, notification, IPC/stack/ring frames, and fault endpoint; installs a
restricted child CSpace; applies manifest affinity or emits the deferral marker;
maps all declared `PT_LOAD` pages and runtime regions; and resumes the TCB. The
generated specs control code pages, stack pages, resources, and hot-path
admission. Current runtime images use sixteen stack pages, `code-pages=128`, and
the driver-local windows `0x70001000` for IPC, `0x70200000` for MMIO,
`0x70800000` for DMA, and `0x70c00000` for shared buffers. The physical Pi
runtime `_start` dispatches only fixed command/completion records; callback
pointer dispatch is compiled out. Isolated runtime admission binds
`TCB_SetIPCBuffer` with the child-VSpace-mapped IPC frame cap, not the
root-mapped frame cap, and emits
`DRIVER_TASK_IPC_BIND ... source=child-vspace-mapped-cap` before resume. A
current boot that lacks `runtime-entry-ready` followed by either
`runtime-recv-ready` or `runtime-poll-ready` progress after that breadcrumb is
blocked at linked-runtime transport, before PCIe/VL805 descriptor replay or USB
xHCI gates. The linked runtime intake loop reads the fixed command ring before
polling the command endpoint, so bounded one-way turns can complete from the
shared ring without requiring an endpoint wake or reply cap.
`runtime-poll-ready` means the child loop is alive but did not observe a fresh
ring sequence; it is emitted on the first idle poll and then sparsely.
`runtime-ring-read-begin` means the runtime is about to perform its first
uncached shared command-ring read in the intake loop. It is a first-edge marker,
not a per-poll heartbeat; later idle liveness is carried by sparse
`runtime-poll-ready` markers.
`runtime-poll-begin` means the runtime observed a command that requires a reply
cap and is about to poll the command endpoint.
`runtime-reply-pending` means a non-one-way command was visible without a reply
cap and must be treated as a transport contract error.

Runtime init is non-acceptance. Root stages a pointer-free
`DriverRuntimeInitDescriptor`, submits bounded init/service turns, and restores
contract MCP/priority after each turn. Send-only pre-root turns carry
`DRIVER_RUNTIME_COMMAND_FLAG_ONE_WAY`; a missing shared completion is red
evidence, not a reason to trap boot in `seL4_Call`. Owner-state stays red until
the linked runtime returns hardware progress from driver-local state. PCIe-root
descriptor replay and engine-init are retained for three bounded turns on timeout
so the USB Gate 2 owner command is not overwritten while the linked child polls
the sequence-last command ring. Descriptor replay and any other payload-bearing
turn use the same staged-submit active-slot rule: a timeout may preserve the
active request for a matching resume, but it must not authorize another caller to
copy different bytes over the in-flight descriptor or payload.
Driver-task command rings, bus-owner rings, shared-control pages, and
root-shared payload pages are HAL-mapped into linked runtimes; runtime-side
coherency on those pages is volatile load/store plus barriers only. Root-side
publication additionally cleans staged descriptor/payload bytes and
command/completion records before sequence publication, and root invalidates
completion/progress records before consuming linked-runtime evidence.
Runtime-side cache clean/invalidate instructions are reserved for
descriptor-declared device DMA buffers and must not be used to publish or
consume shared-ring progress, commands, completions, or CYW43-to-SDIO owner
payloads.
USB engine-init additionally publishes pre-MMIO and xHCI register submarkers:
`usb-init-entry`, `usb-state-access-begin`, `usb-state-reset-*`,
`usb-dma-range-ready`, `usb-caps-read-begin`, `usb-caps-invalid`,
`usb-halt-*`, `usb-reset-*`, `usb-cnr-wait-begin`, `usb-*-written`,
`usb-*-flushed`, and `usb-run-wait-begin`. Historical `usb-pcie-flush-*`
markers remain parser context for old traces only. A timeout before
`usb-caps-read` proves only the PCIe/VL805 owner prerequisite gate, not xHCI
operational status.

Root-task remains the client for console grammar, tickets, namespaces, policy,
replay, and revocation. Physical hardware service must fail closed through
linked runtime rings:

- emergency mini-UART is the only physical Pi bootstrap escape hatch;
- serial cutover requires receive-side proof, not just init/TX proof;
- HDMI progress comes only from linked `SubmitFrame` commands, never a root
  framebuffer fallback;
- USB/local-seat, GENET/CYW43, SDIO, and PCIe root-side callers are ring clients;
- missing linked owners return `DeviceUnavailable` instead of root-driving
  hardware;
- SDIO command and single-frame data calls have no physical-Pi root SDHCI
  fallback;
- PCIe descriptor replay is adopt-only after HAL proves the live VL805 tuple and
  BAR/COMMAND state; prompt-side PCIe runtime service must not assert BCM2711
  `SW_INIT_1` or PERST.

Boot evidence must expose the boundary without repeating every device-specific
detail:

- `DRIVER_TASK_DEFAULT`, `DRIVER_TASK_BOOT`, `DRIVER_TASK_SUBSTRATE`,
  `DRIVER_TASK_SUMMARY`, and `DRIVER_TASK_ACCEPTANCE` report substrate state,
  failures, live TCBs, affinity, VSpace isolation, cap/fault/revoke proof,
  broad-cap leaks, compatibility roles, and final verdict.
- `DRIVER_TASK_OWNER_STATE` reports one descriptor/root-pointer verdict per
  current acceptance hot path.
- `SCHED_CONTRACT` and `DRIVER_TASK` report per-role live TCB, hot-path, capset,
  fault-probe, and revoke readiness.
- `DRIVER_TASK_RESOURCE_INIT`, `DRIVER_TASK_RING_CALL_BEGIN`,
  `DRIVER_TASK_RING_CALL_RETURN`, `DRIVER_TASK_RING_CALL_TIMEOUT`, and
  `DRIVER_TASK_RING_PROGRESS` report each bounded descriptor, init, and service
  turn.
- `DRIVER_TASK_BOOT_SMOKE` is QEMU transport proof only; it may exercise isolated
  VSpaces and fixed rings but cannot satisfy Pi hardware acceptance.

Closure is fail-closed. A physical Pi driver-task proof requires the expected
task count, `failed_count=0`, live TCB count, required role mask, per-driver
affinity, zero broad-cap leaks, pointer-free IPC, owner-state proof, and every
boolean required by `scripts/pi4_gate_proof.sh --require-driver-task-proof`.
`DRIVER_TASK_AFFINITY_DEFERRED`, `DRIVER_TASK_NOTIFICATION_BIND_DEFERRED`,
root-context diagnostic commands, pointer callbacks, idle completions, and
zero-result progress completions are useful diagnostics but never hot-path
ownership. `DRIVER_TASK_SUBSTRATE_READY=yes` proves only substrate creation;
`DRIVER_TASK_DEDICATED_READY=yes` requires live linked-runtime hardware progress
for each selected role with zero root-task compatibility roles.

The HAL rejects missing, zero-budget, non-preemptible, or unbounded-blocking
contracts before service. USB/local-seat and serial remain `RealtimeInput` and
preempt network data. CYW43/SDIO Wi-Fi keeps network-control and network-data
budgets separate so EAPOL, DHCP, TCP ACKs, and physical input cannot starve one
another.

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
- Firmware bundle admission, mailbox support, cached diagnostics, and declared
  SDIO/CYW43 runtime resources belong behind `Cyw43Hal` / `pi4_wifi`; steady
  power/reset, clock, SDHCI, SDIO CMD52/CMD53, and Wi-Fi transport services
  belong to linked runtime descriptors.
- Pi 4 BCM2711 PCIe root-complex/VL805 config access belongs behind
  `pi4_pcie`; drivers must not derive config space from the xHCI BAR.
- Pi 4 USB xHCI MMIO is mapped only from the HAL-proven VL805 high BAR
  `0x0000000600000000`; low xHCI aliases are not runtime candidates.

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
- Wi-Fi maps the SDHCI page only through the HAL-declared `sdio-host` runtime;
  CYW43 receives a pointer-free bus-link descriptor, not a direct SDHCI mapping,
  and must prove owner-state with pointer-free CMD52/CMD53/POLL_IRQ turns.
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
- Map driver-task command/completion rings and pointer-free owner bus-link rings
  uncached in every participant VSpace; barriers alone are not cache
  maintenance.
- Pin every device-shared range with `hal::dma::pin` before publishing it to a
  device; the pin API accepts only a HAL-admitted DMA range derived from
  HAL-owned DMA backing, not arbitrary raw addresses.
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
  the driver-task pointer-free proof is present, the boot path runs bounded USB
  keyboard/xHCI service before the root prompt. When the net-console policy
  selects Wi-Fi (`wifi`, or `auto` with credentials) and the same proof is
  present, root resumes bounded SDIO/CYW43 linked-runtime work before announcing
  console readiness, but Wi-Fi readiness and DHCP are not allowed to suppress the
  serial diagnostic shell beyond the bounded pre-root release window. If that
  proof is missing, root skips the hidden post-prompt Wi-Fi replay and leaves a
  serial-diagnostic blocker instead of printing a prompt and then monopolizing
  it. The `Cohesix console ready` banner means the serial event pump can accept
  input; it does not imply Wi-Fi association, DHCP, or TCP-console readiness.
  QEMU virtio and Pi 4 wired NIC paths still use their immediate net-init flow.
- CYW43455 SDIO traffic is linked-runtime-driven through bounded
  SDHCI/CMD52/CMD53 owner descriptors; the driver must not publish arbitrary DMA
  addresses to the Wi-Fi firmware path.

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
  masked. The cold-boot command proof publishes `CONFIG.MaxSlots`, `DCBAAP`,
  `CRCR`, `ERSTSZ`, `ERSTBA`, the initial `ERDP` without `EHB`, scratchpad
  DCBAA slot 0, and `DNCTRL=0`, waits for `USBCMD.RUN` to clear
  `USBSTS.HCH`, applies poll-only `IMOD=0` / `IMAN=0`, and acknowledges
  consumed events with `ERDP.EHB` only
  after event-ring consumption. The gate-3/4 proof command is Enable Slot; No Op
  is diagnostic-only and must not advance root-port sampling. Local-seat
  preserves already-posted current-boot PSC
  events until after it DMA-publishes Enable Slot and rings doorbell `0`; the
  wait loop then skips and acknowledges those PSC events, inserts the same
  bounded prompt-safe wait used for other unexpected events, and still accepts
  only the matching Command Completion Event as gate-4 proof. The proof turn
  returns after Enable Slot completion and no longer issues same-turn Disable
  Slot cleanup, because the first real enumeration Address Device turn is the
  next ownership proof and must not be hidden behind cleanup latency. Do not unmask
  external xHCI interrupt delivery until a milestone explicitly proves it.
- Linked USB xHCI base-register publication keeps the known-good Pi 4/VL805
  discipline inside the isolated runtime: after DMA structures are written, a
  full xHCI DMA publication barrier precedes controller register publication,
  and init-time xHCI operational-register writes are drained by a same-runtime
  xHCI MMIO readback inside the USB runtime's declared aperture. Doorbells are
  the exception: command and endpoint doorbell writes publish the U-Boot value
  with DMA/store barriers and do not perform the toxic same-window xHCI
  readback observed in old traces. PCIe owner replay remains the prerequisite
  for BAR/COMMAND/VL805 admission, but xHCI posted-write drains do not issue
  nested USB-to-PCIe child commands. The
  runtime publishes low-written, high-written, and high-flushed progress
  markers for 64-bit base registers, plus `usb-config-written` and
  `usb-config-flushed` markers for `CONFIG.MaxSlots`, so
  `usb status` / `usb diag` can map a no-reply to the exact half-register or
  same-runtime readback-drain frontier without reintroducing root-owned xHCI
  access.

### SDIO/CYW43455

CYW43455 is an SDIO device, but Cohesix does not expose a generic SDIO host API
to arbitrary drivers. Under the split driver model, `sdio-host` owns SDHCI reset,
power, clock, host bus width, raw no-data card commands, CMD52 direct I/O, CMD53
extended transfers, and `POLL_IRQ`; the CYW43 runtime owns card-side
CMD0/CMD5/CMD3/CMD7 selection, CCCR/backplane sequencing, firmware bundle
streaming, and Wi-Fi power/reset state. CYW43 may request bounded SDIO-owner
CARD_COMMAND and HOST_CONFIG turns for the proven card select and card/host
4-bit/high-speed transition, but it must not write SDHCI host-control registers
directly. Root may replay the SDIO owner descriptor, initialize the owner engine,
and prove owner-state; it must not run a root-owned SDIO card-init ladder.
SDIO descriptor replay and engine-init remain bounded prompt-safe turns, but
root keeps the active ring request across no-reply resume slices so a
still-running linked SDIO adoption/reset turn is not overwritten before it
returns ready or is explicitly reset. For payload-bearing SDIO/CYW43 turns, the
active identity includes the staged descriptor and payload bytes; a changed
descriptor, firmware/NVRAM chunk, SDIO owner payload, or shared-buffer segment is
not a resume and must fail busy instead of replacing the in-flight bytes.
SDIO engine-init completion details are part of the linked runtime ABI: success
returns `0x5500` (`ready`), and faults preserve the exact subgate as
`0x5501` (`adopt-power-missing`), `0x5502` (`adopt-clock-failed`), `0x5503`
(`adopt-inhibit-failed`), `0x5510` (`reset-all-failed`), `0x5511`
(`reset-cmd-data-failed`), `0x5512` (`clock-failed`), or `0x5513`
(`inhibit-failed`). Root projects those details through
`SDIO_DRIVER_TASK_REPLAY_STATUS ... stage=engine-init blocker=<status>` and
`DRIVER_TASK_RESOURCE_INIT ... stage=sdio-engine-init status=<status>`; a generic
`DeviceUnavailable` detail is no longer acceptable for this gate because it
erases the next required hardware action.
The cold reset path mirrors the May 18-19 working root-owned order inside the
linked runtime: after all-reset and power-on, a stale command/data inhibit does
not make the pre-clock CMD/DATA reset terminal. The runtime programs the 400 kHz
startup clock first, then clears post-clock inhibit with CMD/DATA reset and only
then reports `reset-cmd-data-failed`, `clock-failed`, or `inhibit-failed`.
The June 13 17:01 post-flash boot supersedes the June 13 16:22, 13:07, 12:35,
10:11, 09:27, 08:50, 08:16, 07:55, and 07:33 Wi-Fi frontiers as current truth.
The 12:35 transport-admission regression, 13:07 revinfo BADARG blocker, and
earlier control-reply idle loop are no longer current: descriptor replay for
`cyw43455` and `sdio-host`, SDIO engine init detail `0x5500`,
`cyw43-sdio-prereq`, CYW43 engine init, firmware upload, NVRAM/tail, firmware
release, and owner-state all recover to ready. The linked CYW43 control channel
then proves multiple Linux-order replies: `bus:txglomalign=8`, optional
`ulp_sdioctrl` as matched `BCME_UNSUPPORTED`, `bus:rxglom=1`, `cur_etheraddr`,
`BRCMF_C_GET_REVINFO` with the 68-byte response window, `mpc=0`, `WLC_UP`,
`WLC_SET_INFRA`, WPA2 setup, PAE multicast admission, and `WLC_SET_SSID`. The
active frontier remains Gate 7 because host-EAPOL secure completion is not yet
proven: the log reaches `cyw43-host-eapol pending`, sends six bounded
`cyw43-host-eapol-start` frames with the expected 18-byte EAPOL-Start Ethernet
shape and BDC priority 6, then fails closed with `host-eapol-required`; DHCP,
`nettest`, `netstats`, and remote-`cohsh` proof remain uncredited.
Linked-runtime RX polls may now request
`DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD` after EAPOL-Start so the
runtime can translate the May 18-19 zero-RFRAME/card-interrupt behavior into a
bounded Function 2 first-read without moving physical CYW43 ownership back into
root. Host-EAPOL status records report `rx_firstread_attempts`,
`rx_firstread_empty`, `rx_firstread_invalid`, `rx_firstread_failed`,
`rx_firstread_remainder_failed`, `rx_firstread_decode_miss`,
`last_rx_idle_detail`, and `last_rx_idle_result` so the next trace distinguishes
AP silence, malformed SDPCM, CMD53 read failure, and a valid EAPOL M1/M3 frame
reaching the host handshake. Prompt-side `wifi diag` treats a terminal
`host-eapol-required` net-disabled cause as the live frontier and does not let
the earlier tolerated `ulp_sdioctrl` `BCME_UNSUPPORTED` marker overwrite the
diagnostic table. Repeated
`cyw43-firmware-recover` owner-replay cycles remain progress only when
`resume_offset` or `STREAM_PROGRESS uploaded=` advances; root keeps the
structured `CYW43_DRIVER_TASK_FIRMWARE_RECOVERY` and stream/fault records but
suppresses redundant human-readable `begin` / `ready` wrappers for that recovery
stage. The next boot must either show EAPOL data reaching the host handshake,
prove first-read empty/no-reply with `last_rx_idle_detail=0x570a`, prove
malformed first-read SDPCM with `last_rx_idle_detail=0x570b`, prove a CMD53
first-read/remainder failure with `0x5709` or `0x570c`, or prove that a non-EAPOL
data frame reached the RX path after the SSID/join edge.
The linked runtime now publishes CYW43-specific early and release markers:
`cyw43-engine-init-branch`, `cyw43-state-reset-begin`,
`cyw43-state-reset-done`, `cyw43-forbidden-sdio-mmio`,
`cyw43-bus-link-check-begin`, `cyw43-shared-control-check-begin`,
`cyw43-shared-control-missing`, `cyw43-shared-control-ready`,
`cyw43-release-begin`, `cyw43-release-reset-vector-begin`,
`cyw43-release-armcr4-reset-begin`, `cyw43-release-upload-clock-begin`,
`cyw43-release-post-config-begin`, `cyw43-release-ht-clock-begin`,
`cyw43-release-f2-enable-begin`, `cyw43-release-int-mask-begin`,
`cyw43-release-corecontrol-begin`, `cyw43-release-mailbox-version-begin`,
`cyw43-release-firmware-ready-begin`, and
`cyw43-release-firmware-ready-done`; after engine-init, transport details and
command-fault records must preserve the exact CYW43/SDIO owner subedge instead
of collapsing back to stale HAL power/reset, engine-init no-reply, NVRAM retry,
or generic command-completion failures.

- Function 0 is CCCR/FBR control.
- Function 1 is the Broadcom backplane/control path.
- Function 2 is the data/control-plane FIFO path after firmware.
- Function 2 remains disabled before firmware/NVRAM upload.
- CYW43 transport init must mirror the known-good Pi 4 order while staying
  restartable under the linked-runtime contract: prove the SDIO owner bus link,
  replay card select inside the CYW43 runtime through SDIO-owner CARD_COMMAND
  descriptors, set Function 1 block size 64, set Function 2 block size 512,
  enable Function 1, replay the startup host clock, then request/prove ALP and
  the backplane window before firmware prep widens the card to 4-bit mode or
  asks `sdio-host` for high-speed host timing. Each phase returns a
  `0x5400..0x5408` transport detail and publishes a `cyw43-*` progress marker
  so `wifi diag` can identify the exact linked-runtime phase. A backplane fault
  in this stage is a transport blocker, not a firmware/DHCP blocker.
  The transport command advances one prompt-safe phase per linked-runtime turn
  and root retries the same CYW43 transport descriptor until the detail changes
  or a terminal owner fault is published. This preserves the May 18-19 command
  order without making card select, FBR setup, IOEX/IORDY, host-clock, and
  backplane proof depend on one monolithic child turn. The CYW43-to-SDIO
  pointer-free bus-link descriptor is sequence-stamped, re-notified at a bounded
  interval while CYW43 polls the SDIO owner completion slot, and records the
  nested SDIO descriptor class on timeout so `wifi diag` can distinguish a
  missing owner reply from a card-command or CMD52/CMD53 fault. Every linked
  runtime uses volatile shared-ring loads plus barriers before dispatch, and
  SDIO/CYW43 descriptor readers consume their descriptor frame through the same
  uncached shared mapping; this keeps CYW43-produced SDIO-owner commands
  coherent without replacing the bounded one-way ring turn with an unbounded
  driver-to-driver call. Root-side resource status labels report partial
  transport details as `progress`; partial `0x5401..0x5407` transport
  completions may return `result=0` and must still be preserved as progress so
  root can retry the same descriptor. Only
  `DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_READY` returns `result=1` and is
  reported as `ready`.
- Production Function 2 traffic requires firmware upload/release evidence, real
  `CHIPCLKCSR.HT_AVAIL`, and live Function 2 readiness (`IOR2`/ready proof).
  `CHIPCLKCSR=0x50` (`ALP_AVAIL|HT_REQ`) with `IOR2` still clear is diagnostic
  evidence only, not a production Function 2 promotion.
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
- CLM upload is not yet credited as a linked-runtime proof gate in the current
  physical-Pi Wi-Fi path. When the linked runtime enables CLM, it must follow the
  captured Linux `clmload` cadence: 1400-byte payload chunks (`len=1412` after
  `download_hdr`), 20-byte SDPCM control headers, padded CMD53 request lengths,
  and `ver` / `clmver` queries after upload. Until then, `clmver` is comparison
  evidence only and cannot close DHCP or remote-`cohsh` readiness.
- CLM must not be the first meaningful BCDC/iovar exchange. Linux proves the
  initial control-plane path with small iovars first: set
  `bus:txglomalign=8`, query `ulp_sdioctrl` while accepting the captured
  `BCME_UNSUPPORTED` result, set `bus:rxglom=1`, query `cur_etheraddr`, then
  issue Linux's mandatory `BRCMF_C_GET_REVINFO` (`cmd=98`) before CLM. Cohesix
  follows that order before CLM so the first Function 2 replies are small
  Linux-shaped 64-byte first-read transactions rather than a large CLM
  transfer. After that attach proof, Cohesix keeps the
  `bus:txglom`/`bus:rxglom` state established by the Linux-style SDIO preinit
  path through firmware `ver`, `clmver`, `mpc`, `event_msgs_ext`, and `WLC_UP`.
  Disabling `bus:rxglom` during control-plane attach is not Linux-equivalent and
  can move a working control plane into a host-`CARD_INT`/no-dongle-source stall.
  Cohesix therefore defers any runtime `bus:rxglom=0` transition until after
  attach; oversized or malformed aggregated RX is bounded in receive handling
  instead of by mutating Linux attach order.
  Malformed or oversized RX-glom evidence must remain explicit, UART-capped,
  and recoverable by clearing the Function 2 frame condition.
- The first Linux-order control writes use the plain 12-byte SDPCM header. The
  8-byte SDPCM hardware-extension header is enabled only after `bus:rxglom`
  succeeds, matching `brcmfmac`'s `tx_hdrlen` transition. Sending
  `bus:txglomalign` with the extended header shifts the CDC payload to offset
  20 before the firmware has enabled that framing and leaves the host polling
  for a reply the firmware never generates.
- Firmware streaming stays inside the generated 8192-byte shared-payload/RX
  owner window. The linked runtime preserves incoming chunk bytes before any
  retained-stage replay, pads only the final logical chunk to a 512-byte physical
  boundary, rejects malformed padding, reports `CYW43_DRIVER_TASK_STREAM_TAIL_PAD`
  when active, and advances accounting by logical byte count. Multi-block CMD53
  turns must set `SDHCI_TRNS_MULTI`; block-mode count zero is illegal except for
  512-byte byte-mode CMD53. The linked runtime decodes SDIO R5 out-of-range as
  `0x0800` (`resp0=0x1800` in current Pi 4 traces) and tolerates that bit only
  on Function 1 backplane writes; Function 2 and other CMD53 response faults
  still fail closed. A transfer fault such as `0x5103` retries through the
  retained SDIO owner state and byte-mode fallback without replaying CMD0/CMD5 or
  CYW43 power/reset state. Retained-stage replay keeps a runtime-local flush
  offset inside the 8192-byte stage, so a later owner retry resumes at the first
  unflushed subtransfer instead of rewriting the completed prefix. The same
  cursor covers the normalized NVRAM chunk, and root may resume into the
  post-firmware NVRAM/tail/release path only after an exact NVRAM chunk fault
  matches the generated CYW43455 address and normalized length. Exhausted
  retained-stage recovery reports `0x5329`.
- Station attach follows the captured Linux shape while staying bounded to the
  current linked-runtime proof surface. The as-built control path first sends
  `bus:txglomalign=8`, tolerates only a matched `BCME_UNSUPPORTED` reply for the
  optional `ulp_sdioctrl` query, sends `bus:rxglom=1`, reads `cur_etheraddr`,
  issues `BRCMF_C_GET_REVINFO` (`cmd=98`) with a 68-byte zeroed response window,
  then keeps `mpc=0` established before matched CDC exchanges for `WLC_UP`,
  `WLC_SET_INFRA`, WPA2 setup, PAE multicast admission, and `WLC_SET_SSID`.
  Additional Linux probe telemetry such as
  firmware `ver`, `clmver`, `join_pref`, scan timing, and event-mask setup
  remains useful comparison evidence, but it is not accepted as proof unless the
  linked runtime observes the corresponding CDC reply. Do not send AP/P2P-only
  or local legacy station writes such as `apsta=1`, early `country`,
  `WLC_SET_GMODE`,
  `WLC_SET_BAND`, `WLC_SET_ANTDIV`, early AMPDU-limit writes, or `WLC_SET_PM`
  unless a later Linux-equivalent gate proves they belong. `BCME_UNSUPPORTED` on
  non-captured compatibility knobs is nonfatal; transport errors are fatal.
  Function 1 host-latch/no-dongle-source stalls during join programming report
  `join-programming-host-latch-loop`.
- WPA2 setup is fail-closed and Linux-shaped: `wpaie`, initial
  `wpa_auth=0x00c0`, `auth=0`, `wsec=0x0004`, RSN side effects, final
  `wpa_auth=0x0080`, then host-EAPOL proof before data release. Station setup uses
  `DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE`, not fire-and-forget control
  frames: the runtime must match CDC command plus ioctl id, reject nonzero CDC
  status, and return the CDC response body for reads such as `cur_etheraddr`.
  Primary-BSS commands use plain iovar names; do not invent BSSCFG wrappers on
  this path.
- Firmware supplicant offload must prove `sup_wpa`, valid PMK programming, and
  `PSK_SUP` plus carrier confirmation before DHCP/data. If firmware rejects that
  path, Cohesix derives the host PMK locally and reports
  `wifi-host-eapol-pending`; DHCP and normal data stay blocked until M1/M2/M3/M4
  plus PTK/GTK `wsec_key` install complete. `SET_SSID` alone never releases a
  secure network.
- Host-EAPOL admits the PAE group multicast, sends bounded EAPOL-Start frames
  through the linked CYW43 `ETH_TX` descriptor while waiting for AP M1, derives
  PMK/PTK locally, writes M2/M4 in WPA2-PSK order, verifies M3 MIC/replay state,
  unwraps GTK with AES-128 key unwrap, installs pairwise and group `wsec_key`
  iovars, and only then reports secure completion. EAPOL TX uses the extended
  SDPCM data shape with BDC priority `6`; control writes may wait for CDC
  replies, but data/event writes must not inherit a control-plane reply wait.
  `eapol_start` counts only the bounded linked-runtime 802.1X start frames; it
  is not DHCP/data success by itself.
- `WIFI_GATE=7`, `wifi-host-eapol-pending`, and
  `wifi-host-eapol-required` are not Wi-Fi connection success. They preserve the
  secure boundary while event-pump turns yield back to serial, USB keyboard,
  HDMI, and IPC. Firmware `PSK_SUP` is ignored under the host-EAPOL completion
  rule, and DHCP/readiness/RX/TX labels use the same secure-completion predicate.
- PMK and supplicant-shape errors keep precise labels such as
  `wsec-pmk-bad-argument`; transport errors remain fatal and must not be masked
  by later SDIO hint probes.
- Join programming drains bounded post-`UP` events, sends the upstream
  primary-BSS `join` extended payload, and falls back to legacy `WLC_SET_SSID`
  only after an explicit `join` iovar failure. Pre-join Function 1 host-latch
  loops are join-programming blockers until a control reply, `join pending`, or
  terminal join event proves acceptance. Diagnostic labels must preserve the
  current security frontier, for example
  `primary-bsscfg-wrapper-join-security-loop`,
  `join-security-wsec-first-loop`, or
  `join-security-wpa-auth-initial-loop`.
- Firmware RAM readback is diagnostic, not a mandatory production gate. The
  Linux production brcmfmac path compiles full RAM verify out; Cohesix may run
  bounded readback for evidence, but `sdhci-byte-mode-count` and similar
  readback-transport limitations after a completed upload must be reported as
  `readback-unavailable` and must not trigger a lower-speed reupload that can
  disturb a good image. Byte mismatches remain terminal because they prove bad
  payload contents.
- Linked CYW43 release and owner-state registration prove firmware execution
  only. Association, carrier, and either firmware-PSK or host-EAPOL completion
  must still be proven before DHCP/data. During pending secure joins, the runtime
  keeps Ethernet RX on `DRIVER_RUNTIME_CYW43_OP_RX_POLL`, exposes SDPCM
  control/event frames through `DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL`, credit-gates
  control TX, suppresses repetitive low-level breadcrumbs, and keeps non-EAPOL
  data blocked. Prompt-side `wifi diag` and `wifi dump-state` render cached or
  linked-runtime evidence; stateful `wifi probe-ht`, `wifi load-fw`, and
  `wifi retry` fail closed with `pi4-wifi-driver-task-runtime-required` when no
  linked runtime can satisfy the request. Post-join `EVENT_LINK` without the link
  flag is `wifi-link-down`, not DHCP progress.
- Join-completion event delivery remains a later-gate hardening target, not an
  as-built linked-runtime proof gate. The current secure-completion gate is
  host-EAPOL: M1/M2/M3/M4, PTK/GTK `wsec_key` installation, and secure data
  release must complete before DHCP. A future event-mask implementation must use
  the Linux `event_msgs_ext` shape (`ver=1`, `command=SET_MASK`, `len=27`) with
  the Pi 4 capture mask plus Cohesix-required `AUTH`, association, and `PSK_SUP`
  bits, and may fall back to global `event_msgs` only on matched
  `BCME_UNSUPPORTED`. Until that implementation lands, event lines are
  diagnostics only and cannot close Wi-Fi Gate 10.
- Linux clears `SBSDIO_FUNC1_SDIOPULLUP` during SDIO buscore preparation, but
  Cohesix does not issue that optional CMD52 on Pi 4 until the SDHCI path proves
  Linux-equivalent recovery across the immediately following sideband access.
  This is a seL4 HAL adaptation, not a new hardware requirement.
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
  hintless Function 2 first-read probe from the linked CYW43 runtime, then
  publishes `cyw43-control-rx-firstread-begin`, `cyw43-control-rx-firstread-done`
  and one of `cyw43-control-rx-firstread-frame`,
  `cyw43-control-rx-firstread-empty`,
  `cyw43-control-rx-firstread-invalid`, or
  `cyw43-control-rx-remainder-failed`. Timeout results preserve those exact
  first-read outcomes as `cyw43-control-rx-firstread-*` blockers instead of
  collapsing them back to `cyw43-control-rx-no-rframe`. That terminal proof
  keeps the gate loop honest and preserves IRQ 158 as the only Wi-Fi interrupt
  source; IRQ 27 remains the seL4 timer.
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
- USB/local-seat and CYW43455 may both be active, but USB owns the bounded
  pre-net keyboard window. Wi-Fi must keep raw breadcrumbs out of UART/HDMI while
  USB is proving first input, then continue to rate-limit raw HAL output so
  keyboard, serial, HDMI, and IPC turns stay responsive.
- Wi-Fi net-console bring-up must not hide an already-announced serial prompt.
  With complete linked-runtime pointer-free proof, bounded SDIO/CYW43455 replay
  may run before `Cohesix console ready`; otherwise Cohesix preserves the Wi-Fi
  policy for diagnostics, emits `action=serial-diagnostics-only`, and publishes
  the prompt. Replay uses bounded nonblocking IPC and UART-visible
  `SDIO_DRIVER_TASK_REPLAY_STATUS` / `NET_DRIVER_TASK_REPLAY_STATUS` breadcrumbs.
- CYW43 receives SDIO only through the pointer-free bus-owner link after
  `transport-init` proves the card, Function 1, ALP backplane window, and chip
  identity. Firmware upload may widen SDIO and stage ARMCR4 reset/release, but a
  single redundant reset-assert descriptor failure after staging remains advisory
  so upload can prove the next gate.
- Pi 4 local-seat USB is not a reason to disable Wi-Fi diagnostics. The serial
  console retains the Wi-Fi debug grammar after root-console handoff, but direct
  CYW43455/SDIO exercise in root is removed: `wifi diag` and `wifi dump-state`
  report cached or linked-runtime command-fault evidence, while `wifi load-fw`,
  `wifi retry`, and live `wifi probe-ht` return bounded runtime-required errors
  unless the linked runtime has supplied the required state. `wifi diag`,
  `wifi dump-state`, `wifi load-fw`, `wifi retry`, and `wifi probe-ht` replay the
  cached SDIO and CYW43 linked-runtime progress markers when present, including
  the mapped Wi-Fi gate, blocker, and next action, so a prompt capture
  distinguishes descriptor replay, resource validation, SDIO card-select,
  CYW43 transport, firmware upload, DHCP, `nettest`, and `netstats` frontiers
  without constructing a root-side SDIO/CYW43 driver. Once a terminal
  boot/control-plane
  failure is preserved, `wifi diag` is passive and compact: it emits the
  readiness/network summary, renders the `wifi: diag recorder=startup-blackbox ...`
  ten-gate table from cached snapshot or linked-runtime CYW43 fault evidence,
  reports an unchanged after-state when it skips the long live HT re-probe, and
  leaves the full transport snapshot to `wifi dump-state`. A no-reply CYW43
  command is recorded as `CYW43_DRIVER_TASK_COMMAND_NO_REPLY` with stage, op,
  target, payload offset/length, and total length, and root caches the latest
  quiet CYW43 runtime progress marker even when the command trace itself is
  suppressed. CYW43 engine-init now publishes early branch, state-reset,
  forbidden-MMIO, bus-link, and shared-control markers before transport starts.
  Transport-init publishes begin/ready markers for card adopt, Function 1/2
  block-size programming, Function 1 enable, startup host config, and backplane
  prep, so engine-init or transport-init no-reply is classified as the exact
  linked CYW43 transport/CCCR/FBR/backplane frontier rather
  than a stale HAL power/reset failure. A CYW43 firmware-upload
  `0x5101` failure on an `sdio-cmd*` or `sdio-card-init*` stage is reported
  as an SDIO command-unavailable card-select blocker; it is earlier than
  CYW43 firmware upload, DHCP, `nettest`, and `netstats` acceptance. A
  `0x5103` failure is reported as a CMD53 descriptor-transfer blocker with
  `wifi: next_action=inspect-sdio-owner-cmd53-after-block-and-byte-retries`, and an
  exhausted retained-stage ladder reports `0x5329` as `firmware-retry-exhausted`
  while preserving the SDHCI transfer result plus the actual owner-lane label
  (`forced-byte-mode-conservative`, `byte-conservative`, or
  `byte-narrow-conservative`). The R5 `0x0800` out-of-range bit is no longer
  misclassified as a hard firmware-upload rejection for Function 1 backplane
  writes; if it appears on Function 2 or non-backplane CMD53 turns it remains a
  fault. The retained-stage flush cursor is internal to
  the CYW43 runtime and is not DHCP, Function 2, or remote-`cohsh` proof. A
  `0x530a` `cyw43-descriptor-invalid` failure
  is earlier than SDIO owner execution: serial evidence must include the
  producer `payload_off`, `payload_len`, total length, and runtime result-bit
  predicate so the next pass can distinguish stale ring visibility from a true
  ABI-shape mismatch. None of these cases is a generic disabled-network state.
  Operators can still run the explicit
  `wifi probe-ht` command when linked-runtime state can support the stateful HT
  probe; otherwise it reports the same driver-task-runtime-required boundary.
- Wi-Fi association completion is event-pump driven for both explicit `wifi`
  and `auto` interface policies. While linked-runtime pointer-free proof is
  incomplete, Pi 4 local-seat Wi-Fi boots preserve the selected Wi-Fi policy but
  do not issue the join command before the serial prompt; prompt-side
  diagnostics and the event pump then drive association and DHCP/static
  addressing without hiding serial, USB, or HDMI responsiveness behind the
  Wi-Fi wait.
- In the physical Pi driver-task cutover profile, `auto` selects CYW43 when
  bounded Wi-Fi credentials are present and otherwise selects wired GENET before
  Wi-Fi ownership begins. Once CYW43 is selected, protocol, HAL transport,
  firmware, join, and post-Function-2 errors are Wi-Fi gate evidence and remain
  fatal so gates 7 and 8 cannot be hidden by the wired backend. QEMU/host
  compatibility profiles may still exercise absent-device fallback logic for
  virtual-device tests.
- Once host-EAPOL secure completion is proven during the join-submit proof
  window, the CYW43 path releases DHCP immediately and must not emit stale
  `wifi-host-eapol-pending` / `data=blocked` diagnostics. Wi-Fi Gate 10 remains
  fail-closed after DHCP: `wifi diag` can report only Gate 9 until the capture
  includes explicit `nettest` plus final `netstats` proof with DHCP-bound Wi-Fi,
  secure EAPOL, and non-zero TX/RX counters. Optional peer-assisted `nettest`
  echo/smoke probes are reported separately from driver-level TX/RX/DHCP/remote-
  `cohsh` proof, so a missing router-side echo listener is not a Wi-Fi blocker
  and cannot spam the console.
- Post-attach SDPCM glom RX is bounded: descriptor lists are capped, normal-sized
  subframes are deaggregated into the data/event/EAPOL path, and malformed or
  oversized glom evidence remains explicit but UART-capped instead of silent or
  flood-prone. Descriptor overshoot is a soft mismatch when the remaining tail is
  still a complete SDPCM subframe. Pi 4 keeps Linux's `bus:rxglom=1` through
  attach and join; any future runtime disable or superframe expansion must be
  owned by the Wi-Fi driver task after secure carrier proof, with bounded work,
  counters, and recovery gates.
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
  outbound MMIO windows before EXT_CFG proof. Raw `PCIE_STATUS` link/root bits
  are advisory only: they do not prove live ownership and must not skip the
  bounded BCM2711 root-complex reset/window init for the current reset phase.
  The live VL805 tuple and COMMAND/BAR proof are the ownership gate.
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
- xHCI ownership-register and `USBCMD.RUN` posted-write drains now use bounded
  same-runtime xHCI MMIO readback from the linked USB runtime's declared
  aperture. Command and endpoint doorbells are barrier-only publish edges. The
  PCIe owner still proves BCM2711 EXT_CFG selector/COMMAND, endpoint BAR, and
  root bridge readiness before xHCI entry, but the xHCI flush edge no longer
  performs a nested USB-to-PCIe child command. `PORTSC` is not used as a generic
  posted-write drain on this prompt-safe path.
  The live xHCI BAR reads permitted on the Pi 4
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
- CONFIG, DCBAAP, CRCR, ERSTSZ, ERSTBA, initial ERDP, scratchpad, DNCTRL, RUN,
  command-ring recovery, command-doorbell, and endpoint-doorbell posted-write
  drains fail closed when the HAL cannot first prove the EXT_CFG selector,
  link/root readiness, PCIe IRQ-source masking, and poll-only VL805 COMMAND
  ownership. Non-doorbell xHCI write drains are bounded same-runtime readbacks
  inside the linked USB runtime's declared aperture; if the trace stops after a
  `usb-*-written` marker and before the corresponding `usb-*-flushed` marker,
  that xHCI readback drain is the frontier. PCIe IRQ-source masking proof must
  reject all-ones sentinel readbacks and log that edge as untrusted instead of
  setting the ownership proof latch.
- U-Boot-compatible command/event proof must publish fresh ring registers on
  the Pi 4 platform-reset lane in the linked-runtime order
  (`CONFIG.MaxSlots`, `DCBAAP`, `CRCR`, `ERSTSZ`, `ERSTBA`,
  initial `ERDP` without `EHB`, scratchpad, `DNCTRL=0`), drain non-doorbell xHCI
  operational-register posted writes with same-runtime xHCI MMIO readback,
  start the controller with `USBCMD.RUN` and require `USBSTS.HCH==0`, then
  apply U-Boot's poll-only post-start interrupter state (`IMOD=0`, `IMAN=0`)
  through the same-runtime xHCI readback drain. `CRCR`
  composition uses the command-ring pointer and producer cycle; on prompt-safe
  lanes the linked runtime does not synthesize Linux's later observed running
  status bit. The U-Boot `DNCTRL=0` write is also drained before
  `USBCMD.RUN`. It must write the submitted command TRB in U-Boot
  `queue_trb()` order: parameter low, parameter high, status, then the control
  dword with the cycle bit last. Cohesix then DMA-publishes the full bounded
  command ring containing the submitted TRB, issues the completion-grade
  command-ring publish barrier, writes U-Boot's `DB_VALUE_HOST` to doorbell
  `0`, and treats that
  doorbell edge as barrier-only before polling the event ring with the same
  5 s command-event budget.
  The command proof is Enable Slot, matching U-Boot's first non-root-hub xHCI
  allocation gate. Already-posted current-boot Port Status Change events remain
  on the event ring and are skipped/acknowledged while the Enable Slot command
  is outstanding. The U-Boot-shaped Enable Slot lane rings DB0 when it submits
  the command. The first pending prompt slice polls the event ring before any
  recovery re-doorbell; later pending turns may re-ring DB0 only while no
  command events have been consumed. That is liveness recovery, not proof. Only the
  matching Command Completion Event can advance the gate. The proof turn reports
  `command-ring-pending` while completion is outstanding, records command-ring
  readiness only after the matching Command Completion Event, and returns before
  any Disable Slot cleanup, so a cleanup stall cannot masquerade as missing
  gate-4 proof. No Op is only a diagnostic helper and must not unlock root-port
  sampling. There is no pre-command `ERDP.EHB` acknowledgement and no
  pre-command PSC drain.
  The Enable Slot command-proof lane acknowledges each consumed event with a
  prompt-safe `ERDP.EHB` write, then publishes only DMA/store barriers; it does
  not issue same-window xHCI readback while the proof command is outstanding.
  If the event-ring dequeue pointer cannot be translated to the device-visible
  DMA address, the path fails closed instead of publishing `ERDP.EHB` against
  address zero. Generic command/control waits keep the normal runtime ERDP ack
  path. Init-time 64-bit ownership-register publications use low/high writes
  followed by high-dword posted-write flush. Runtime `ERDP.EHB` ack logs must
  identify the low/control flush separately from the high DMA-alias flush so
  Gate 3 proof does not collapse both halves into one ambiguous stage. Command doorbell `0`
  itself still uses the U-Boot command value, but the linked USB runtime now
  publishes the doorbell with barriers only instead of issuing either a nested
  PCIe-owner flush or a same-window xHCI readback. A missing xHCI aperture or a
  same-runtime readback no-reply remains a hard failure for non-doorbell Pi 4
  high-BAR VL805 ownership-register paths.
- If the first U-Boot-shaped Enable Slot proof does not complete within the
  bounded poll slices, the linked runtime reports `enable-slot-failed` and
  returns to root instead of running a second same-turn command recovery lane.
  A poll turn may consume only a small bounded number of event-ring entries
  before returning `command-ring-pending`; Gate 4 remains the failed gate while
  the blocker is `enable-slot-completion-pending`, even if the derived ten-gate
  table has not reached root-port sampling. This keeps root prompt slices
  responsive while preserving PSC and keyboard-transfer events for later
  enumeration. In the Enable Slot command-proof lane only, the runtime uses the
  prompt-safe ERDP barrier path and emits `usb-command-proof-event-*`,
  `usb-command-proof-erdp-ack-*`, and `usb-command-proof-return-pending`
  progress markers so a toxic acknowledgement edge is isolated from later
  command/control waits. When a bounded prompt poll sees no consumable event, it
  first publishes `usb-command-proof-event-peek-begin` before the event TRB
  cache-invalidate/read edge, publishes
  `usb-command-proof-event-read-begin` after resolving the event TRB address and
  immediately before the DMA load/read edge, publishes
  `usb-command-proof-event-dma-load-done` after the DMA load barrier returns,
  publishes `usb-command-proof-event-invalidate-done` after the uncached
  event-ring CPU-sync barrier returns, publishes
  `usb-command-proof-event-read-done` after that TRB read returns, then
  publishes either `usb-command-proof-event-slot-empty` for an all-zero event slot or
  `usb-command-proof-event-cycle-mismatch` for a nonzero TRB with the wrong
  cycle bit before spinning. The
  command-proof snapshot does not read `PORTSC`; the low result bits carry
  consumed event count until the matching completion unlocks root-port sampling.
  The `USB_RUNTIME_ENUM_SNAPSHOT` command-proof fields are diagnostic only and
  cannot advance beyond gate 4 without the matching Command Completion Event.
  Prompt-side retry may then cold-reinitialize the xHC under the normal
  driver-task budget and immediately run the next keyboard enumeration turn.
  Any Linux-captured command-event-generation helper remains diagnostic-only
  and cannot advance gate 4 unless it is separately authorized by a future
  milestone and logs a matching Command Completion Event under its own label.
  Stale legacy Linux-shaped labels, pre-command status reads, post-enqueue
  event-generation replay, interrupt-delivery bits without MSI ownership, or a
  same-command re-doorbell are not proof.
- Prompt-side USB retry must be bounded and decisive. A linked-runtime retry may
  cold-reinitialize the xHC on terminal Enable Slot proof failure, then
  immediately attempts the next keyboard enumeration turn instead of spending a
  long shell-blocking cooldown. Address, descriptor, config, HID attach, and hub
  attach failures first preserve same-controller state so retries do not erase
  slot, port, or descriptor progress. If a full same-controller retry window
  repeatedly exhausts at the same deep failure frontier, the runtime may take up
  to two cold reinitialization escalations and then resumes the normal linked
  xHCI proof ladder. `command-ring-ready` is not itself a cold-reset trigger; it
  is the handoff point to root-port sampling and enumeration.
  Plain `xhci-ready` is progress toward Enable Slot proof, not a reset-worthy
  stall; resetting there can loop forever at gate 3 and prevent gate 4 command
  proof. Prompt diagnostics therefore label the next action as
  `submit-enable-slot-command` at `xhci-ready` and
  `poll-enable-slot-completion` at `command-ring-pending`, with recovery policy
  `same-controller-command-proof` for both. The command-proof polling slice is
  shorter than the full command/control-transfer wait budgets so a no-reply
  child runtime exposes `command-event-ring-not-proven`,
  `enable-slot-completion-pending`, or `enable-slot-failed` quickly, and only
  `keyboard-ready` plus first HID report/byte proof can clear USB acceptance.
  After `root-port-connected`, linked USB publishes bounded Address Device
  substages (`usb-root-port-reset-*`, `usb-address-enable-slot-*`,
  `usb-address-contexts-published`, `usb-address-command-*`, and
  `usb-device-addressed`) plus EP0 device-descriptor prime/full-read and
  configuration-descriptor header/full-read substages
  (`usb-device-descriptor-prime-*`, `usb-device-descriptor-*`,
  `usb-config-descriptor-header-*`, and `usb-config-descriptor-full-*`) plus HID
  endpoint parse, hub traversal, Configure Endpoint, SET_CONFIGURATION, HID
  control, and interrupt-queue substages (`usb-hid-endpoint-parse-*`,
  `usb-hub-scan-*`, `usb-hub-child-probe-begin`,
  `usb-hid-configure-endpoint-*`, `usb-hid-set-configuration-*`,
  `usb-hid-control-*`, and `usb-hid-interrupt-queue-*`) so prompt-side
  diagnostics can keep the ten-gate frontier pinned to gate 5, gate 6, the
  first gate-7 descriptor edge, or the gate-8 interrupt-queue edge without
  reopening PCIe/VL805 ownership or introducing a root-owned xHCI fallback.
  Full-speed devices use the prime markers for the initial 64-byte descriptor
  request before the final 18-byte device descriptor read; device,
  configuration, HID endpoint, and interrupt-queue setup stay in the linked
  runtime.
  Root preserves an in-flight linked-runtime USB enumeration request across
  bounded no-reply slices only when the active identity still matches. A valid
  same-request/same-aux progress marker whose phase advances resets the
  consecutive timeout-resume counter; unchanged markers still count toward the
  existing timeout-resume limit and clear the active latch when that limit is
  reached. Bounded keep-active resumes are admitted only when the staged command
  identity still matches the active ring request; a different aux word, frame
  descriptor, staged byte fingerprint, hot path, role, budget, or
  command flags is a different request and must not inherit the earlier request's
  progress. Sequence-zero runtime-idle markers observed while a request-scoped
  USB enumeration marker is cached are still emitted as raw timeout progress,
  but they do not evict the request-scoped marker used by timeout accounting and
  prompt diagnostics for the in-flight request.
- Pi 4 cold boot must attempt one bounded local-seat keyboard probe before
  net-console initialization. `hw.local_seat.required=true` requires matching
  required `hw.devices[]` entries; missing manifest devices or HAL-owned
  PCIe/VL805 proof remain pre-shell failures. Runtime no-reply, controller,
  HID-report, or PCIe owner-state failures are red acceptance states that must
  emit `DRIVER_TASK_SELECTED`, `DRIVER_TASK_OWNER_STATE`, and
  `DRIVER_TASK_ACCEPTANCE` before halt or degraded diagnostics. Required local
  seat keeps polling instead of falling back to serial-only; optional local seat
  degrades with explicit `[local-seat]` lines and no repeated xHCI probing. USB
  and Wi-Fi interleave only at explicit boot/event-pump phase boundaries.
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
  drain. Each skipped event republishes `ERDP.EHB` through the prompt-safe
  barrier path before polling the next event-ring slot; skipped PSC events also
  take one bounded prompt-safe settle before the next sync so command
  completions racing behind preserved PSCs are not hidden by a tight ERDP
  update loop. On success the proof returns after the matching Enable Slot
  completion and does not issue same-turn Disable Slot cleanup, so cleanup
  latency cannot masquerade as missing gate-4 proof. Linux-shaped cleanup or
  event-generation writes cannot advance gate 4. Only a completed command may
  reopen live root-port sampling.
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
  pointer array. Runtime reserves and can publish the VL805-observed 31-entry
  scratchpad set within a 32-entry bounded arena. This keeps the command gate
  from depending on a prefilled or truncated scratchpad array that U-Boot never
  exposes before the DCBAA slot update.
  The linked-runtime init descriptor carries the first 80 DMA page descriptors,
  which covers the full 320 KiB xHCI zero/scratchpad arena even when HAL has
  allocated non-contiguous pages; semantic resource ranges may still describe
  the larger USB arena, but the child must be able to translate every live
  scratchpad bus address before it publishes DCBAA slot `0`.
  USB progress markers split scratchpad publication into slot-0 written,
  slot-0 cleaned, array filled, and array cleaned edges so `usb status` and
  `usb diag` can distinguish a DMA descriptor truncation, a DCBAA publication
  fault, and a scratchpad-array clean fault before root-port sampling.
  Prompt-safe recovery keeps the same visible order: slot `0` stays withheld
  through the fresh HCRST, DCBAAP, CRCR, ERSTSZ, ERSTBA, and ERDP writes, then
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
  doorbells beyond doorbell `0` as `role=endpoint-doorbell`. Linked runtime
  keyboard-ready now requires at least one interrupt-IN TRB to be queued and the
  endpoint doorbell attempt to be published, so Gate 8 cannot pass on descriptor
  parsing alone. Command waits must preserve any non-command transfer event they
  drain while waiting for a later command completion, then replay it to the
  matching endpoint poller; otherwise the first HID report can be acknowledged
  in ERDP and lost before Gate 8 sees it. CPU-side HID/control descriptor reads
  must invalidate the DMA buffer after the transfer event before decoding
  device-written bytes.
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
  HID decode contract remains a U-Boot-compatible Boot Keyboard contract: report
  ID layouts are accepted only when the keyboard profile explicitly selects that
  offset, because byte `0` of an unknown report can be either a report ID or a
  real modifier bitmap. Post-Gate-8 runtime paths must not allocate fresh DMA for
  optional keyboard LED updates after local-seat seals the xHCI DMA pool.
  Poll-only Pi 4 keyboard input must keep a deep interrupt-IN read queue armed and
  match transfer events back to the submitted transfer TRB before decoding that
  DMA buffer; a single read requeued only after the next event-loop turn can miss
  fast press/release transitions during console-output stalls.
  The linked USB runtime therefore reserves a dedicated one-byte HID
  output-report DMA slot during keyboard attach and uses it for Caps Lock, Num
  Lock, and Scroll Lock `SET_REPORT(Output)` updates after the seal. LED sync is
  optional: if a keyboard stalls, rejects, or times out the output report,
  Cohesix logs that the LED path is unavailable, disables later LED writes for
  that keyboard, and keeps the software lock state and normal input path
  running.
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

Physical-address selection, device-untyped mapping, IRQHandler creation, PCI
config admission, DMA admission/publication, board-level mailbox support, and
firmware bundle admission belong in HAL modules. Linked runtime modules may
manipulate device registers only through HAL-declared mapped regions and
descriptor-mediated owner turns. The current Pi 4 Wi-Fi path gives `sdio-host`
HAL-declared SDHCI authority, keeps CYW43 behind the pointer-free bus link, and
makes `sdio-host` an acceptance-eligible owner-state hot path.

The HAL must prove:

- `device_coverage(paddr, PAGE_BITS)` succeeds before `map_device(paddr)`;
- mapping code preserves page alignment and maps every page in the aperture;
- IRQ bindings are created through `bind_irq_notification`;
- IRQ ack happens only after the device source is cleared;
- DMA buffers are admitted as HAL-owned `HalDmaRange` values before
  `hal::dma::pin`;
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
- Construct `hal::dma::HalDmaRange` values only from HAL-owned backing and pass
  those ranges to `hal::dma::pin`; raw address tuples are not authority.
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
- The compatibility path reclaims at most 32 TX completions and drains at most
  eight RX descriptors in one service turn before yielding; raising those caps
  requires fresh USB/serial responsiveness proof under wired load.
- Routine successful wired-NIC dataplane ring begin/return lines may be
  suppressed during steady service so benchmark runs do not measure UART
  logging. Init descriptors, nonzero-aux diagnostics, budget exhaustion,
  timeouts, faults, and resource blockers must remain visible.
- DMA address policy is explicit (`physical` vs VC/bus alias) and logged.
- No DHCP logic is inside the GENET driver; DHCP belongs above `NetDevice`.

Do not proceed to smoltcp until link and one bounded RX/TX smoke path are
observable.

### CYW43455 Wi-Fi

Source order: OpenBSD `bwfm`, Infineon WHD HAL split, Linux `brcmfmac` SDIO
edge behavior.

Required Cohesix shape:

- The linked CYW43 runtime owns Wi-Fi protocol state, CYW43 power/reset state,
  firmware/NVRAM/CLM staging, SDPCM, CDC/BDC, association, and Ethernet
  dataplane.
- `sdio-host` owns the HAL-declared SDHCI MMIO page for acceptance-eligible
  CARD_COMMAND/CMD52/CMD53/HOST_CONFIG/POLL_IRQ service turns behind the
  declared CYW43-to-SDIO boundary.
- Root does not issue SDIO card-select commands. CYW43 submits CMD0/CMD5/CMD3
  and CMD7 through the pointer-free SDIO owner descriptor during transport init,
  preserving nested SDHCI present-state, interrupt-status, response, host
  control, clock, and payload digest telemetry on owner faults.
  That card-adoption sequence is sliced across bounded linked-runtime turns:
  host-config, CMD0, CMD5 OCR, CMD5 ready, CMD3 RCA, and CMD7 select each publish
  progress before issuing the owner command. A no-reply at this layer is Wi-Fi
  gate 2 `sdio-card-select` evidence, not a HAL power/reset or DHCP failure.
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
  transport init, SDIO width/high-speed upload prep, firmware/NVRAM into ARMCR4
  RAM, reset-vector release, then post-release Function 2 readiness. The linked
  runtime must not perform KSO/WAKEUPCTRL/watermark/Function 2 sideband work as
  part of `transport-init`; that sideband belongs after firmware release. CM3-only
  SOCSRAM remap writes are not part of this path.
- Station control uses matched CDC `CONTROL_EXCHANGE` descriptors for writes and
  read iovars. A control-plane command is not accepted merely because the SDPCM
  frame was transmitted; the runtime must return the expected CDC command/ioctl
  id or a precise control-exchange fault.
- Wi-Fi credentials remain bounded: SSID 1-32 printable ASCII bytes; PSK empty,
  8-63 printable ASCII bytes, or 64 ASCII hex digits. A 64-hex PSK is decoded as
  the direct 32-byte PMK before host-EAPOL; shorter passphrases use WPA2
  PBKDF2-HMAC-SHA1 with the SSID.

Do not debug DHCP over Wi-Fi until association and the first CYW43 Ethernet
frame path are proven independently.

### xHCI/VL805 Local Seat

Required Cohesix shape:

- The active Pi 4 USB acceptance path is the linked `pi4-driver-usb` runtime in
  `apps/pi4-driver-runtime/src/lib.rs`. It owns the direct-root-port xHCI command
  ring, event ring, EP0 control path, interrupt-IN keyboard queue, DMA report
  buffers, HID decode, and local-seat first-byte publication under the
  driver-task contract.
- Root-task local-seat code is a linked-runtime client only. It may report
  local queue and prompt evidence, but it does not contain a root-task USB/xHCI
  implementation and does not close Pi 4 driver-task acceptance by itself.
  Hardware acceptance requires linked-runtime owner-state proof plus the USB
  10-gate evidence below.
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
- The linked USB runtime must keep the xHCI interrupter poll-only (`IMAN=0`,
  `IMOD=0`), publish DCBAAP/CRCR/ERSTSZ/ERSTBA/ERDP through low/high writes plus
  high-dword same-runtime readback drains, flush CONFIG/DNCTRL/RUN by
  same-runtime xHCI MMIO readback, publish doorbells with barriers only, and use
  neutral PORTSC writes that do not mirror `PED`, `PR`, or change bits except as
  explicit RW1C acknowledgements.
  Physical Pi USB still requires linked PCIe owner replay before xHCI entry, but
  posted-write drains no longer depend on a nested USB-to-PCIe child command.
- USB keyboard feeds only the existing root-console parser after decoding USB
  HID Usage Page `0x07` keyboard usages to bounded ASCII/control bytes.
  Arrow usages are decoded to normal ANSI cursor sequences. Up and down arrows
  therefore drive both the existing root-console history parser and the linked
  HDMI high-impact scrollback renderer; left and right arrows are preserved for
  parser-side cursor behavior.
- HID keyboard polling first accepts strict boot-protocol reports, then uses the
  linked-runtime flexible report decoder for report-protocol keyboards that
  expose compact or bitmap reports. A key-empty boot-looking payload with
  additional non-zero report bytes emits
  `0x0416 tag=usb-hid-report-flexible-key` before decoding via the flexible
  path, so Gate 9 failures stay attributable to report decoding instead of
  being misread as HAL/DMA/MMIO or SDIO contention.
  The linked runtime requests a bounded endpoint-packet report buffer, sizes the
  decoded payload from the xHCI transfer event's remaining-length field, accepts
  report-ID-prefixed, compact, and bitmap keyboard payloads, and invalidates the
  whole requested DMA range before decoding.
- USB keyboard input is a distinct local-seat physical-console source, not a
  UART alias. The event pump clears an unfinished UART line when USB keyboard
  bytes arrive and defers concurrent UART command dispatch until the next
  no-keyboard-input turn. A later UART command still clears an unfinished
  local-seat line before dispatch, so serial and USB input cannot concatenate
  stale partial commands.
  Accepted local-seat and serial command output is emitted on UART and through
  the bounded linked HDMI mirror once `hdmi-text` proves a `SubmitFrame`.
  High-impact progress lines and real-time local-seat input feedback may submit
  first-frame owner proof before HDMI engine-init owner state is attached, but
  root still never writes the framebuffer directly. Slow or verbose console
  output remains chunked so display rendering cannot starve xHCI polling.
  Up/down keyboard scrollback redraws only the bounded high-impact HDMI history
  through linked `SubmitFrame` commands. `usb status` reports local-seat
  keyboard-drop counters without deferred HDMI queue/drop counters.
- Operator proof uses a single 10-gate USB ladder: 1 controller candidate, 2 live
  PCIe/VL805 ownership, 3 controller-ready, 4 command-ring completion, 5
  root-port connection, 6 device address, 7 descriptors/configuration, 8 HID
  keyboard ready, 9 first HID report, and 10 first console byte. `usb status`,
  `usb probe-kbd`, and `scripts/pi4_trace_normalize.py` must preserve
  `proof_gate` evidence rather than silently inferring only the current blocker.
  Gate-4 and pre-gate-5 output includes the xHCI base-register, scratchpad, RUN,
  interrupter, and command-proof subphase so a timeout after command/event-ring
  setup cannot be mistaken for a root-port or HID blocker. Gate-6/7 output names
  Address Device, full-speed descriptor-prime, device-descriptor data/status,
  config-descriptor header/full-read data/status, and later HID blockers
  separately so a published addressed-device state cannot be mistaken for HID or
  keyboard-ready progress.
- The isolated USB runtime keeps xHCI in the same poll-only shape as the proven
  U-Boot/local-seat path: interrupter moderation and management remain zero
  while command, transfer, and port-change completions are consumed by bounded
  event-ring polling.

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

Both release bundles include TCP and USB. The active Pi 4 USB acceptance path is
the linked runtime in `apps/pi4-driver-runtime/src/lib.rs`; root-task
local-seat code is a ring-client control surface only. Release proof comes from
runtime parity tests plus root-task local-seat diagnostics. Do not test driver
changes with ad hoc feature strings when the target bundle
applies. Use the focused aliases:

- `cargo test -p pi4-driver-runtime --lib -- --test-threads=1`
  covers the linked Pi 4 runtime implementation for USB, SDIO/CYW43, HDMI,
  serial, and GENET. USB-specific coverage includes the driver-task xHCI
  command/event/EP0/interrupt-IN path, barrier-only doorbell flushes,
  same-runtime xHCI MMIO readback flushes, the fixed PCIe owner-link
  prerequisite, HID report buffering/decoding, and keyboard publication.
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
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_pcie`
  covers BCM2711 PCIe/VL805 BAR, INTx/MSI, posted-write, DMA-window, and page
  mapping contracts.
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi`
  covers SDIO, CYW43455 firmware/HT/Function 2 gates, mailbox, and R5 error
  contracts.
- `cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::`
  covers target-neutral local-seat parser, keyboard queue, mirror, and USB/Wi-Fi
  command policy helpers for the linked-runtime client path.
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
- HAL owns resource admission for MMIO, IRQ, DMA, PCI, SDIO, board-level
  power/reset, and firmware bundles; steady physical service runs through linked
  runtime descriptors.
- The driver declares a valid HAL driver-task scheduling contract before any
  runtime service path can poll, map, DMA, ack IRQs, or move frames.
- Polled proof exists before IRQ use.
- Device source is cleared before IRQHandler ack.
- DMA buffers are HAL-admitted as `HalDmaRange` values, pinned, and
  cache-maintained.
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
  - cargo test -p pi4-driver-runtime --lib -- --test-threads=1
  - cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::rtl8139
  - cargo test -p root-task --no-default-features --features driver-tests-qemu --lib drivers::virtio
  - cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::pci
  - cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::virtio_mmio
  - cargo test -p root-task --no-default-features --features driver-tests-qemu --lib hal::uart
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::driver_task
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib serial::tests::poll_io_obeys_driver_task_budget
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib event::tests::serial_input_skips_ready_network_data_poll_for_driver_task_turn
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_pcie
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib hal::pi4_wifi
  - cargo test -p root-task --no-default-features --features driver-tests-pi4 --lib local_seat::
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
