<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Provide the QEMU and Raspberry Pi 4 build, flash, boot, capture, and proof runbook. -->
<!-- Author: Lukas Bower -->
# Hardware Bring-up

This runbook covers the supported development path on QEMU `aarch64/virt` and
the Raspberry Pi 4 U-Boot path. It keeps image construction, media state, boot
identity, device readiness, remote control, and performance as independent
proof layers.

Implementation details belong in [DRIVERS.md](DRIVERS.md), boot-marker semantics
in [BOOT_REFERENCE.md](BOOT_REFERENCE.md), acceptance predicates in
[TEST_PLAN.md](TEST_PLAN.md), and performance methodology in
[BENCHMARKS.md](BENCHMARKS.md).

## Current Evidence Boundary

| Lane | Current status | What it does not prove |
| --- | --- | --- |
| QEMU seL4 16 offline | All five fresh profiles and a linked GICv3 QEMU `--no-run` package build pass. Earlier Stage 01-05 records predate this refresh and are not v16 boot evidence. | A QEMU boot, target-qualified v16 Test Plan, Pi firmware, MMIO, DMA, IRQ, local-seat, GENET, or CYW43 behavior. |
| Pi 4 historical wired GENET | Milestone 26c retained one coherent Stage 01-05, runtime/DMA, DHCP, raw TCP, and authenticated `cohsh` proof chain. See [M26C_AS_BUILT_BLOCKERS.md](audit/M26C_AS_BUILT_BLOCKERS.md). | The current source tree or a newly flashed image. |
| Pi 4 seL4 16 source, offline | Both fresh Pi profiles pass; a direct `~/seL4_16` diagnostic build completed 309 of 309 steps and passed runtime validation. Exact-image staging remains open because the shared checkout was already dirty and was preserved. | A sealed/read-back image, board boot, current-image device readiness, TCP, or benchmark result. |
| Pi 4 pre-v16 live diagnostic | Exact commit `7328bedd6142` / image `5a9f812c5408998e3292b4c4475bee545a4c8f2d0e5781a238054598ff313001` starts both linked runtimes, proves the selected GPIO34-GPIO39 ALT3/pull fields by live readback, completes the strict retained CYW43 extra-pull-up clear, keeps the operator console live through bounded supervision, and reaches Gate 6. Its first 2-KiB/count-32 firmware CMD53 receives a clean R5 and advances exactly two 64-byte blocks before stalling with `DATA_END` absent and BCM2835 DMA active/DREQ-held. | This is historical diagnostic evidence, not seL4 16 proof. No association, EAPOL, DHCP, TCP, `cohsh`, repeatability, or performance proof exists for that image. The all-request Linux watchdog, live pre-firmware contract reproof, exact interrupt mask, and telemetry-v3 changes remain an offline source-and-image candidate until that exact candidate is read back and booted on the Pi. |

Maintainers may keep a workstation-local boot ledger while an investigation is
active. It may be newer than checked-in records, but only the repository's
linked audit and target-qualified Test Plan evidence is canonical for shared
claims.

## Proof Layers

Never collapse these layers into a single “works” claim:

1. **Build:** source compiled and artifacts were staged.
2. **Flash:** the intended removable disk was erased and written.
3. **Readback:** the media contents and exact image marker match staged
   artifacts.
4. **Boot:** that read-back marker appears in a fresh serial boot.
5. **Saved policy:** `cohesix.env` was preserved or intentionally replaced,
   without disclosing secrets.
6. **Device/network:** current serial and boot-paired packet evidence prove the
   selected USB, HDMI, GENET, or Wi-Fi lane.
7. **Console:** raw TCP and authenticated `cohsh` succeed on the same boot.
8. **Test Plan:** target-qualified stage markers are complete and no
   `*.incomplete` state remains.
9. **Benchmark:** a workload-specific report has full provenance and passes its
   error and latency contract.

```mermaid
flowchart LR
  Source["source and selected manifests"] --> Build["build and stage"]
  Build --> Flash["verify target and flash"]
  Flash --> Readback["read back media\nand match image marker"]
  Readback --> Boot["fresh serial boot\nwith that image marker"]
  Boot --> Policy["saved policy\npreserved or intentionally replaced"]
  Policy --> Device["device and network proof\nserial plus paired packet capture"]
  Device --> Console["raw TCP and authenticated cohsh"]
  Console --> TestPlan["target-qualified Test Plan"]
  TestPlan --> Benchmark["qualified benchmark"]
```

## Profiles and Toolchain

| Target | Manifest | seL4 build truth |
| --- | --- | --- |
| QEMU `aarch64/virt` | `configs/root_task.toml` | Canonical validated `SEL4_BUILD_DIR` at `out/sel4/profile-v2/qemu-smp-production`; explicit alternatives are diagnostic unless a named profile contract passes. |
| Raspberry Pi 4 | `configs/root_task_pi4_uboot_aarch64.toml` | Immutable repo-managed `seL4/build_UBOOT` artifacts for the `pi4_diagnostic` image lane. `out/` contains only disposable composition, staging, and evidence outputs; it is never a required Pi seL4 source or build input. |

The Pi 4 baseline is `Pi firmware -> U-Boot -> seL4 binary image -> root-task`.
`configs/root_task_uefi_aarch64.toml` is a separate profile and is not Pi 4
acceptance evidence.

Use the macOS ARM64 environment in
[TOOLCHAIN_MAC_ARM64.md](TOOLCHAIN_MAC_ARM64.md). The selected generated seL4
headers, cache, timer frequency, capability layout, and resolved manifest are
authoritative for each build.

## QEMU Runbook

### Build and Boot

```bash
SEL4_BUILD_DIR="$PWD/out/sel4/profile-v2/qemu-smp-production" \
./scripts/cohesix-build-run.sh \
  --sel4-build "$PWD/out/sel4/profile-v2/qemu-smp-production" \
  --out-dir out/cohesix \
  --profile release \
  --root-task-features cohesix-dev \
  --cargo-target aarch64-unknown-none \
  --transport tcp
```

In another terminal, set the live secret outside source control and attach with
the built `cohsh` binary as described in [QUICKSTART.md](QUICKSTART.md). Do not
use a placeholder token in a live path.

### Run the Target-Qualified Plan

```bash
./scripts/ci/test_plan_run.sh \
  --target qemu \
  --state-dir out/test-plan/qemu-$(date -u +%Y%m%dT%H%M%SZ)
```

A QEMU pass requires generic and `.qemu.done` markers for Stages 01-05 and no
incomplete marker. Stage 03 exercises TCP regression, Stage 04 the REST
projection, and Stage 05 due diligence.

## Raspberry Pi 4 Runbook

### 1. Build and Stage

For an acceptance candidate, perform a full build. Do not use `--skip-build`
for the first command:

```bash
.venv/bin/python scripts/sel4_profile.py validate \
  --repo-managed \
  --profile pi4_diagnostic \
  --build-dir "$PWD/seL4/build_UBOOT" \
  --require-artifacts \
  --for-runtime
./scripts/pi4-image-build.sh \
  --manifest configs/root_task_pi4_uboot_aarch64.toml \
  --venv .venv
```

The default stage directory is `out/pi4-sd`. The script validates the Pi U-Boot
shape, the canonical tracked `pi4_diagnostic` seL4 artifacts and relocated
build-input stamp, the virtual-counter contract, generated artifacts, runtime
payloads, and rootfs bounds before staging. Both Pi
production and diagnostic profiles require
`KernelRootCNodeSizeBits=14`; this reserves deterministic root CSpace for the
manifest-declared linked-runtime images and the isolated HDMI framebuffer
mapping. The profile wrapper preserves that declared value and uses 13 bits
only for profiles that omit the setting. An older Pi build cache reporting 13
bits is stale and must be rebuilt before image staging or hardware proof.
The selected seL4 build directory is immutable profile evidence: the wrapper
fingerprints `seL4/build_UBOOT`, reconstructs the exact tracked baseline
elfloader from its archived objects as a toolchain oracle, and injects and
relinks the Cohesix rootserver only in a disposable composition directory. It
does not configure, build, repair, re-stamp, or otherwise mutate the tracked
tree. The durable `out/pi4-image-assembly` provenance binds the immutable
profile stamp and artifact identities, relink tool identities and baseline
oracle, and the derived rootserver, exact newc archive, and wrapper. A failed
or interrupted composition therefore cannot turn derived output into canonical
seL4 input. The script also proves
that one fixed-width marker occupies a dedicated file-backed root-task load
section, carries that placeholder through the stripped ELF and complete legacy
image, and finally seals the staged image. Sealing hashes the complete image
with only the marker's 64-byte self-reference plus the U-Boot header/data CRC
fields normalized, writes the digest into `image-id=`, repairs both CRCs, and
writes `out/pi4-sd/pi4-image-identity.json`. A successful stage is build proof
only. The repository-managed diagnostic tree is not release proof, and neither
it nor the staged image substitutes for read-back media, boot, Wi-Fi, TCP, or
benchmark evidence.

Record hashes for the image, U-Boot, DTB, boot script, firmware, and
driver-runtime archive that will be flashed. Record the exact sealed build
marker and `pi4-image-identity.json`. The marker content-binds the complete
`cohesix-image-arm-bcm2711` file; it does not by itself bind U-Boot, DTB,
boot-script, firmware, saved policy, or other partition files, which remain
separate entries in the flash ledger.

### 2. Verify the Flash Target

Flashing is destructive. Identify both the whole removable medium and its exact
existing Cohesix child:

```bash
diskutil list
diskutil info /dev/diskN
diskutil list /dev/diskN
diskutil info /dev/diskNs1
```

Confirm that the whole node is physical, removable, writable, non-virtual, and
the expected size/media. Apple's built-in SDXC reader reports its physical slot
as `Internal`; the card must still report `Removable Media: Removable` and
`Protocol: Secure Digital`. Confirm that the exact child is the expected
writable FAT32 `COHESIX` volume and that its `Part of Whole` is the supplied
whole-disk identifier. Do not infer the target from a stale
`/Volumes/COHESIX` mount, a historical `/dev/diskN`, or any same-label volume
on another disk.

### 3. Flash and Read Back

Only after verification, pass the explicit whole-disk node:

```bash
./scripts/pi4-image-build.sh \
  --manifest configs/root_task_pi4_uboot_aarch64.toml \
  --venv .venv \
  --skip-build \
  --flash-disk /dev/diskN
```

Routine reflash is a single in-place content-replacement lane on that already
provisioned exact child. The helper refuses a locked macOS console session,
holds a bounded `caffeinate` assertion through the media-critical section,
revalidates the child/whole relationship immediately before mutation, preserves
the non-empty `cohesix.env` without printing it, and synchronizes the staged
payload with deletion of obsolete payload files. It does not call
`diskutil eraseDisk`, reformat the volume, force-unmount the whole medium, or
search every mounted volume for the `COHESIX` label. It verifies every staged
regular file byte-for-byte, restores and verifies the bounded private policy,
syncs, and unmounts only the exact child.

This lock guard is mandatory, not advisory. When `loginwindow` denies the mount
of a newly created removable volume, Disk Arbitration may immediately eject the
whole medium and remove its BSD nodes even though formatting succeeded. A mount
retry cannot recover an `Ejected=Yes` medium; physically reinsert it, rediscover
the new `/dev/diskN`, and revalidate the target before continuing.

Whole-disk MBR/FAT provisioning is a separate explicit first-use or repair
operation:

```bash
./scripts/pi4-image-build.sh \
  --manifest configs/root_task_pi4_uboot_aarch64.toml \
  --venv .venv \
  --skip-build \
  --flash-disk /dev/diskN \
  --initialize-disk
```

Never use `--initialize-disk` for an ordinary reflash. It is the only lane
allowed to recreate partition topology. The helper rechecks that the Mac is
unlocked immediately before that operation and never falls back to it when
normal exact-child validation fails.

If any media mutation is interrupted, the helper retains the private policy
copy with mode `0600` and prints its path. Reinsert if macOS has removed the BSD
node, rediscover and reverify the whole disk, then retry the normal reflash with
`--policy-recovery-file <printed-path>`. Recovery refuses to replace a
different non-empty on-card policy, enforces the 384-byte policy bound, and
removes the private recovery file only after the complete staged payload has
been verified and the exact child unmounted successfully.

For acceptance, remount the media and independently compare every staged
artifact. Re-run `scripts/pi4_image_identity.py verify` on the read-back primary
and fallback images, require them to be byte-identical, and preserve the mutable
saved-policy hash separately without printing its contents. The script's image
checks do not replace the full readback ledger.

### 4. Set First-Boot Network Policy

This operator contract is authorized by reopened Milestone 26b tasks
`m26b-uboot-network-wizard` and `m26b-uboot-net-policy`; live Pi acceptance
still requires the hardware evidence in [TEST_PLAN.md](TEST_PLAN.md).

The staged Pi image always stops at **Cohesix boot menu**. Its opening state is
determined by successfully imported, coherent network overrides, not merely by
whether a file named `cohesix.env` exists:

- **Saved network settings loaded:** option 1 is **Boot with saved settings**.
- **Default network settings active:** option 1 is **Boot with default
  settings**. This state applies when `cohesix.env` is absent, empty, oversized,
  malformed, contains no network override, or contains an incoherent network
  tuple. "Default" in the menu means the selected manifest's generated network
  defaults.

Policy loading is capped at 384 bytes, accepts LF or CRLF text, and imports only
the allowlisted fields below. An invalid or partial network tuple, or a logo
value other than `0` or `1`, is cleared in memory with a warning before the menu
is shown, so option 1 cannot accidentally handoff a half-configured policy.
SSID values are never printed in U-Boot summaries. Root-task remains the final
validator for exact IPv4 octets, Wi-Fi text, and compiler-owned bounds; a
rejected DTB handoff falls back deterministically to manifest defaults.

The root menu provides these exact actions:

- `1` — **Boot with saved settings** or **Boot with default settings**, matching
  the state shown above it.
- `2` — **Change network settings**.
- `3` — **Boot logo: On (select to turn off)** or **Boot logo: Off (select to
  turn on)**, so the current choice is visible before it is toggled.
- `4` — **Reset saved settings to defaults**. This opens **Reset saved
  settings?**; `1` is **Confirm reset**, `0` is **Cancel**, and `9` is
  **Advanced: Open U-Boot shell**. **Cancel** is the Enter-key default, and
  nothing is erased before confirmation.
- `5` — **Save settings and restart**.
- `9` — **Advanced: Open U-Boot shell**.

The guided pages use one navigation convention: `0` goes back or cancels and
`9` opens the advanced U-Boot shell. **Choose IPv4 configuration** presents
**Automatic (DHCP)** and **Manual (static IPv4)**. **Choose network connection**
presents **Ethernet (wired)** and **Wi-Fi (wireless)**. Manual mode collects the
address, subnet prefix length, and optional default gateway before review.
These labels deliberately use normal operator terminology; U-Boot variables
retain their internal `dhcp|static` and `wired|wifi` values.

Back and discard actions return through the bounded menu dispatcher. Returning
to the root page from an abandoned configuration reloads `cohesix.env`, so
unsaved working values are never presented as saved settings. **Review network
settings** offers `1` **Boot once without saving**, `2` **Save settings and
restart**, `3` **Edit network settings**, `0` **Discard changes and return to
boot menu**, and `9` **Advanced: Open U-Boot shell**. Save and reset actions
first verify file size and a private byte-for-byte readback; export, write,
size, or readback failure leaves the user in the menu and does not restart.

When Wi-Fi credentials already exist, **Choose Wi-Fi network** offers **Keep
current Wi-Fi settings**, **Change Wi-Fi network**, **Back**, and the advanced
shell. Replacement uses temporary variables and commits them to the working
policy only after valid local entry, so a failed retry does not destroy the
existing credentials. When no network is configured, the page instead offers
**Enter Wi-Fi network**.

To move to a different Wi-Fi network, select **Change network settings**, then
**Wi-Fi (wireless)** and **Change Wi-Fi network**; a delete or reflash cycle is
not needed. If a completely fresh policy is wanted, select **Reset saved
settings to defaults**, confirm the reset, then select **Change network
settings** and enter the new network. The confirmed reset does not delete the
file: it writes a verified allowlisted policy with empty network fields and the
boot logo enabled, then returns to **Default network settings active**. Saving
the new choices recreates or overwrites `cohesix.env` and restarts the board.

The default image is built with `--uboot-menu-input usb`. Use an HDMI display
and USB keyboard for the guided Wi-Fi setup. Before entry, the screen warns:
**Privacy notice: Wi-Fi network name and password are visible on this display;
they are hidden from serial output**. U-Boot normally echoes serial input, so
the script refuses to collect a Wi-Fi password through a serial-only session.
While the **Wi-Fi network name (SSID)** and **Wi-Fi password** prompts are
active, input is restricted to the USB keyboard and output to the video
console; serial/video output is restored after capture. Do not type a Wi-Fi
password into minicom, a serial automation script, or a command recorded in
shell history.

The saved file may contain only these imported fields:

| Field | Meaning |
| --- | --- |
| `coh_net_mode` | `dhcp` or `static` |
| `coh_net_interface` | `wired` or `wifi` |
| `coh_static_ip`, `coh_static_prefix_len`, `coh_static_gateway` | Static IPv4 settings; unused for DHCP |
| `coh_wifi_ssid`, `coh_wifi_psk` | Wi-Fi credentials; unused for wired boots |
| `coh_show_logo` | U-Boot HDMI logo preference |

If USB input is unavailable, mount the boot partition on a trusted workstation
and create or update `cohesix.env` with a local editor that does not sync,
version, or retain the secret. Never use generic `uboot.env`, commit the policy,
print it in a transcript, or include it in an evidence pack. FAT media does not
provide meaningful file-permission protection; treat the card as a credential.
Keep the file within the 384-byte bound and use only the fields above. Unmount
it cleanly before boot. Evidence should record only whether the policy was
intentionally preserved, replaced, or reset and the non-secret summary that
U-Boot emits.

At handoff, the boot script imports only the allowlisted fields, writes the
selected values into `/chosen/cohesix,*` properties in the staged DTB, and
passes that DTB to seL4. Root-task validates the bounds and falls back to
manifest defaults when the handoff is absent or invalid. The script does not
rewrite the source manifest. The flash workflow preserves a non-empty
`cohesix.env` only when it is found on the verified target volume; preservation
is not proof that the selected policy booted.

### 5. Capture a Fresh Boot

Use one serial owner. Start serial capture and the relevant packet capture
before powering the Pi so the files share a boot-time boundary. Pair captures
by filename timestamp or packet time, not by a later filesystem modification
time.

Choose one UTC run identifier and reuse it for every artifact from the boot.
The example below makes `$SERIAL_LOG`, `$PCAP`, and `$PROOF_ENV` concrete; replace
the serial device and capture interface with the devices verified on the host.
On macOS, identify the network interface with `networksetup -listallhardwareports`
or `ifconfig` before capture.

```bash
RUN_ID="20260715T010203Z" # replace once; reuse for this boot only
EVIDENCE_DIR="$PWD/out/pi4-proof/$RUN_ID"
SERIAL_DEVICE="/dev/cu.usbserial-0001"
CAPTURE_IFACE="en8"       # for example, the verified Pi-facing USB Ethernet NIC
SERIAL_LOG="$EVIDENCE_DIR/pi4-serial-$RUN_ID.log"
PCAP="$EVIDENCE_DIR/pi4-network-$RUN_ID.pcap"
PROOF_ENV="$EVIDENCE_DIR/pi4-runtime-dma-$RUN_ID.env"
mkdir -p "$EVIDENCE_DIR"
```

In the packet-capture terminal, start before power-on and stop with `Ctrl-C`
only after the console proof completes:

```bash
sudo tcpdump -i "$CAPTURE_IFACE" -U -n -s 0 -w "$PCAP" \
  'ether proto 0x888e or arp or udp port 67 or udp port 68 or tcp port 31337'
```

In the serial terminal, repeat the setup block with the same `RUN_ID` and start
the sole serial owner before power-on:

```bash
minicom -D "$SERIAL_DEVICE" -b 115200 -o -C "$SERIAL_LOG"
```

Packet captures and serial logs can contain addresses, identifiers, console
traffic, and credentials accidentally emitted by other software. Store and
share them as sensitive evidence even when Cohesix itself redacts the PSK.

On each requested boot, send commands conservatively and wait for the prompt
after every command:

```text
netstats
nettest
wifi diag
usb diag
usb probe-kbd
smp
```

`OK NETTEST detail=started run_generation=<n>` proves only admission. Wait at
least 15 seconds, then issue the final `netstats` before continuing. It must
contain a complete, untruncated
`nettest: generation=<connection> run_generation=<run> ... running=false verdict=<pass|peer-assisted-pass|fail> ...`
line whose run generation matches the admitted ACK. The standard serial reboot
helper performs a 17-second observation window, treats an asynchronous internal
log as optional corroboration, and fails closed on a missing, running,
mismatched, incomplete, truncated, or failed final `netstats` verdict while
still collecting the remaining diagnostics.

For the Wi-Fi lane, that helper does not treat the first root prompt as
bootstrap completion. It also treats the atomic 8a-through-8h
`CYW43_GATE8_COMMIT` as nonterminal progress and sends no command merely
because that commit appears. It first waits without sending input for exactly
one newline-complete current production `CYW43_BOOTSTRAP_SUPERVISOR` terminal
record (`ready`, `failed`, or `permanent`). The record must carry canonical
`attempt=1`, `backoff_ms`, `next_attempt_ms`, `serial`, `local_seat`,
`recovery=full`, `console_seq`, `telemetry_sinks=serial+qlog+hdmi`, and
`prompt_refresh=yes` fields. A truncated or malformed terminal, a duplicate or
contradictory terminal, or any later attempt in the accumulated post-prompt
settle evidence fails closed. `recovery` or `stabilizing` retracts an earlier
Ready candidate. After the first Ready candidate, the helper keeps serial and
network writes idle for the full 130-second Gate 8 lifetime plus terminal-drain
window; only an unchanged Ready admits diagnostics. Guarded `netstats` polls
must then show the selected Wi-Fi/DHCP generation as
`active=wifi addr_src=dhcp-lease ... dhcp=bound` before `nettest` is admitted.
One absolute 60-second DHCP deadline covers every prompt barrier, command,
result wait, and poll interval. Expiry is recorded as a diagnostic failure; the
helper skips the premature `nettest`, establishes a fresh command boundary, and
still captures final `netstats`, Wi-Fi, USB, and SMP diagnostics. An explicit
`failed` or `permanent` supervisor terminal likewise fails closed and skips
unavailable `nettest` work rather than accepting a stale prior verdict. These
waits keep retained bootstrap output from colliding with serial commands. They
do not alter the GENET diagnostic order or its existing timeouts.

The production helper issues guarded, paced `wifi probe-ht` after `wifi diag`.
The physical linked-runtime profile returns the exact typed refusal
`ERR WIFI reason=policy detail=subcommand=probe-ht error=pi4-wifi-driver-task-runtime-required`
because no root-owned live SDIO probe exists. Operators may issue the command
manually to verify that boundary. The helper records only that complete current
refusal as informational; any other `ERR`, truncated terminal, or missing
terminal remains a diagnostic failure.

`usb diag` returns the compact cached ten-gate report; use `usb status` only
when the additional counter detail is needed. A complete response includes
Gate 10, `OK USB`, and the prompt, after which both a serial `ping` and a
USB-keyboard `ping` must still return. If any tail is absent, stop sending input
and preserve the sample. A merged or overlapped serial transcript is not
acceptance evidence.

`usb probe-kbd` is also output-bounded: it emits the one-slice result, explicit
`continuation=pending|terminal` state, cached runtime contract, verdict, and
terminal `OK` below the 2,048-byte serial bound. It does not prepend the verbose
`usb status` dump. A pending command-owned cursor continues by one operation on
each later `LocalSeat` turn and restores the prior polling policy at its finite
terminal bound.

The command effects are intentionally distinct. `wifi diag`, `usb diag`,
`netstats`, and `smp` read retained state and must not submit a device
operation; `wifi diag` labels cached progress as `last_progress` and
`superseded=yes` when a terminal fault is newer. On the physical linked-runtime
profile it also emits bounded passive `wifi: association state`, `progress`,
and `retained` records. They name the current connection generation, explicitly
mark whether session progress belongs to it, report primary-join,
association/link and host-EAPOL state, preserve poll/event counters, and expose
the retained prompt/TX/key/drain owner, generation, request, issuance, and HAL
acceptance state without line truncation. Use those records to distinguish an
accepted join waiting for DPC-delivered events from stale progress, purely
prepared local work, or a still-owned child request; the command itself never
probes SDIO. The driver-task report also emits two passive, independently
bounded records:
`wifi: maintenance generation=<n> current=<yes|no> pending=<yes|no>
requested=0x<mask> ... next=<stage>` and
`wifi: maintenance action=<stage|none> action_generation=<n> request=<n|none>
issued=<yes|no> turns=<n>`. A cleared current-generation cursor reports
`pending=no requested=0x00000000 next=none` and `action=none`; a retained
deferred-recovery summary independently reports
`current=<yes|no> live_generation=<n>` so a first-cause record from an older
generation cannot be mistaken for live maintenance. A child-invisible retained
request is reported as `state=prepared-root-continuation ...
exact=not-published`; while that state is current, the causal next action is to
resume the exact root continuation before CommitRing rather than inspect EAPOL
RX. The trace normalizer treats a dependency-aware Gate 8
`evidence=exact=<association-blocker>` plus its matching evidence boundary as
authoritative over later generic replay/recovery progress. That cache is
transport-generation scoped: clearing or rebinding the physical linked-runtime
transport zeros progress magic, sequence, phase, and auxiliary identity before
a replacement generation can be admitted. On the physical linked-runtime
profile, `wifi probe-ht` exposes only cached root-driver state when that debug
handle exists; the linked runtime returns a typed runtime-required error after
printing the passive startup blackbox and never starts a root-owned live probe.
The blackbox maps a firmware-preparation terminal from its contextual semantic
detail: live CCCR/FBR and clock/backplane contract failures remain at Gates
3–5, while ARMCR4/D11 passive core preparation remains Gate 6. It never
relabels every preparation failure as bulk firmware upload. Admission of a
later firmware operation is a direct sequencer proof for the completed FBR and
upload-window checks, while ARMCR4/D11 reset readbacks remain explicitly
advisory and pre-release ALP/high-speed proof is not reported as post-release
HT proof. `nettest` actively starts the bounded
network self-test, while `usb probe-kbd` actively advances one retained
keyboard-enumeration attempt per later local-seat turn. Run the active commands
only after their preceding passive response has returned its terminal status
and prompt.

The serial log must contain the exact read-back sealed build marker before any
current-image claim is made. The current bootstrap-supervisor record counts only
with its complete production ownership/recovery and console-ordering suffix; a
bare, legacy, or truncated record is diagnostic, not readiness proof. Preserve
the U-Boot policy selection, root prompt, first causal blocker, and all driver
owner-state/counter evidence from the same boot.

### 6. Normalize Without Overclaiming

Inspect the latest boot slice:

```bash
python3 scripts/pi4_trace_normalize.py \
  --gate-summary \
  "$SERIAL_LOG"

python3 scripts/pi4_trace_normalize.py \
  --boot-summary \
  "$SERIAL_LOG"
```

For a wired acceptance candidate:

```bash
./scripts/pi4_gate_proof.sh \
  --normalize-only \
  --log "$SERIAL_LOG" \
  --runtime-dma-proof-out "$PROOF_ENV" \
  --require-wired-ready \
  --require-driver-task-proof \
  --require-input-responsive \
  --expect DRIVER_TASK_ACTIVE_NET=genet \
  --expect NET_ACTIVE=wired \
  --expect NET_DHCP=bound
```

For Wi-Fi, use `--require-wifi-ready` instead of the wired gate and retain the
boot-paired Wi-Fi packet capture. The normalizer must prove association, host
EAPOL completion, DHCP, ARP/data progress, healthy DPC state, `nettest`, and
authenticated TCP bytes; an association or DHCP line alone is insufficient.
The same boot slice must end with the canonical CYW43 bootstrap supervisor
present, ready, unblocked, and with `LAST_STATUS=ready`; any boot recovery,
backoff, failure, second begin, or attempt greater than one remains
acceptance-red.
It must also report `WIFI_GATE7_COMPLETE=yes`, the exact retained history
`WIFI_GATE7_SEEN=7a>7b>7c>7d>7e`, `WIFI_GATE7_LAST=7e`, and
`WIFI_GATE7_MISSING=none`. A latest-frontier `WIFI_SUBGATE=7e` cannot replace
ordered proof of primary join, association/link, M1, M2/M3/M4 plus PTK/GTK,
and secure release.

### 7. Prove Raw TCP Before REST

On the same boot, verify the listener and complete authenticated `cohsh`
`AUTH`, `ATTACH`, `PING`, and `NETSTATS`. Do not infer remote-shell readiness
from `tcp_ready`, DHCP, ARP, or a port probe alone.

Only after raw TCP succeeds should a gateway, REST, UI, or benchmark lane be
interpreted. A gateway cannot repair a target transport failure.

### 8. Complete the Pi Test Plan

Offline validation uses only the first two stages:

```bash
STATE_DIR=out/test-plan/pi4-$(date -u +%Y%m%dT%H%M%SZ)

./scripts/ci/test_plan_run.sh --target pi4 --state-dir "$STATE_DIR" --stage 1
./scripts/ci/test_plan_run.sh --target pi4 --state-dir "$STATE_DIR" --stage 2
```

Stages 03-05 require the same accepted live target:

- Stage 03 requires `COHSH_TCP_HOST` or `COHSH_HOST` for the Pi TCP console.
- Stage 04 requires an existing gateway URL backed by that Pi; the runner must
  not start local QEMU for a Pi lane.
- Stage 05 checks repository and target proof artifacts.

A full Pi pass requires Stage 01-05 generic and `.pi4.done` markers with no
incomplete state. Current-tree offline Stage 01-02 evidence must not be reported
as a full Pi pass.

## U-Boot Recovery and Host Smoke Testing

When the Cohesix boot-options menu is visible, option 0 exits to the U-Boot
prompt. Prefer the staged script commands because they load the saved policy,
DTB, driver-runtime archive, and image through the same bounded path as the
menu:

```text
=> run coh_load_saved_policy
=> run coh_boot_sequence
```

To reload policy from the card and return to the bounded menu instead, use:

```text
=> run coh_load_saved_policy
=> run coh_start_menu
```

Running `coh_prompt_root` alone renders only one page and does not reload policy
or start the dispatcher. If the scripted path cannot load its files, inspect
the FAT partition with `fatls mmc 0:1`. The following raw sequence is a
diagnostic last resort for the default staged filenames:

```text
=> fatload mmc 0:1 0x10000000 cohesix-image-arm-bcm2711
=> fatload mmc 0:1 0x14000000 bcm2711-rpi-4-b.dtb
=> fatload mmc 0:1 0x15000000 cohesix-driver-runtimes.cpio.uimg
=> usb stop
=> bootm 0x10000000 0x15000000 0x14000000
```

This raw sequence does not import `cohesix.env`, apply its `/chosen` properties,
or run every script-owned USB-quiesce diagnostic. It can localize a loader
problem, but it is not acceptance evidence. Never omit the driver-runtime
archive from a current physical-driver boot.

The repository also has a host-only U-Boot smoke harness. Point it at a
separately built QEMU ARM64 U-Boot binary so it does not replace or confuse the
Pi binary in `third_party/u-boot`:

```bash
QEMU_UBOOT_BIN="${QEMU_UBOOT_BIN:?set a QEMU ARM64 U-Boot binary}"

./scripts/uboot/qemu-uboot-smoke.sh \
  --u-boot-bin "$QEMU_UBOOT_BIN" \
  --net user \
  --out-dir out/uboot/qemu-smoke
```

The harness proves that QEMU reached a U-Boot prompt and accepted deterministic
environment/network commands. It does not execute the staged Pi menu, load the
Pi image, or prove firmware, SD, USB, GENET, CYW43, or seL4 behavior.

## Network and Local-Seat Claims

### Wired GENET

Historical accepted M26c GENET evidence remains a control sample. A current
GENET claim needs a new read-back image marker, wired DHCP or accepted static
configuration, bidirectional packet evidence, driver-task proof, raw TCP, and
authenticated `cohsh` from the same boot.

A dispatched GENET console command performs no TCP flush in its `Dispatch`
turn. It installs a cursor owned by the active TCP connection; each later
`Network` phase performs exactly one budgeted response flush and returns. The
normal cursor limit is eight phases and rises to sixteen only while the local
display reports pending/redraw/no-reply backlog pressure. A second buffered
command must wait behind the first cursor. If the active connection changes or
disappears, root rejects the stale cursor rather than flushing a replacement
session. A trace or test that shows command dispatch plus a flush in one turn,
multiple flushes in one `Network` phase, an unbounded cursor, or cursor transfer
between connections is failed TCP liveness evidence.

### CYW43 Wi-Fi

This runbook section is authorized by Milestone 26d tasks
`m26d-cyw43-hardware-free-closure` and
`m26d-benchmark-revalidation-and-tuning`, with active defect authority from
Reopened Milestone 26b tasks
`m26b-wifi-sdio-notification-dpc-closure` and
`m26b-net-control-priority`. It does not authorize a second production,
scheduling, or acceptance lane.

Wi-Fi is the current research and evidence-closure lane. Source tests and a
stage-only build do not establish live association or data-path readiness. The
current image must be reflashed and revalidated with fresh serial and packet
evidence. Repeatability closure requires repeated current-image boots of the
same read-back-proven image with paired network evidence and repeatable raw TCP
and authenticated `cohsh` proof, with no unresolved transport, DPC, generation,
or recovery ambiguity. The minimum closure sample is 10/10 cold power-on boots
and 10/10 warm software-reset boots of the same independently read-back image.
Every counted boot must contain that image's exact `[BUILD]` marker and must
pass the complete normalizer evidence predicate with `NET_ACTIVE=wifi`, reach
Wi-Fi `status=ready` on the sole `attempt=1` boot episode, and contain no
automatic whole-bootstrap restart. Any `status=backoff`, second `status=begin`,
attempt-2-or-later record, or reset used to rescue initial bootstrap is a
production-schema failure rather than a pass hidden by extra passes. Any
pre-service `status=recovery` or physical pair epoch beyond the initial pair is
also a production failure. A later steady-state runtime-recovery episode is
legal only after exact-generation Gate 8 commit, bound DHCP address, and
admitted TCP listener. It remains separately visible as
`CYW43_RUNTIME_RECOVERY` and cannot rescue or rewrite the initial bootstrap
result. Gate 10 remains mandatory data-service evidence for every counted boot.

The cold boot episode and any later authorized steady-state runtime recovery use
one path after linked-pair admission. Root registers and replays both
descriptors, proves the SDIO prerequisites, hands the mailbox to SDIO, and then
enters the sole retained 22-action CYW43/SDIO pair transaction directly. There
is no preliminary SDIO or CYW43 engine initialization. The transaction's SDIO
engine replay owns the one WL_ON/power-sequence physical lifetime; it then
replays CYW43 and hands off the producer ring. Root registers the SDIO owner
after the successful restart and before context replay, firmware/control
programming, and owner-first steady-priority cutover. This canonical cold
transaction is part of attempt 1, not a retry. Reaching firmware/control without
it, initializing an engine before it, or using a separate cold, replay, or
fallback path fails this runbook.

Every valid physical-Pi Wi-Fi selection, explicit Wi-Fi or `Auto` with
credentials, must route to this persistent post-prompt supervisor regardless of
local-seat presence; root must not call the pre-root network constructor for
that selection. Wired selection and `Auto` without Wi-Fi credentials retain
their non-Wi-Fi behavior. The generic root network engine-init lane remains
available to GENET but must be rejected by CYW43. The canonical pair action
`ReplayCyw43Engine` is the sole CYW43 engine-replay lane.

The sole SDIO owner records `begun_epoch`, `completed_epoch`, and
`failed_epoch` in the exact 16-byte
`DriverRuntimeSdioPhysicalLifetimeRecord` reserved in owner-ring state. Pair
reset preserves only that record; command, completion, DPC, grant,
continuation, fault-telemetry, and cumulative-counter bytes from the discarded
generation must be zero. Before restart clears a runtime cursor, it snapshots
the durable record and immediately publishes
`failed_epoch = begun_epoch` for an active lifetime, even when that cursor has
already been lost. A current completed epoch proves only that the owner reached
the ready terminal for that one physical lifetime; it does not by itself prove
enumeration, firmware, Gate 8, DHCP, TCP, or repeatability. An SDIO or CYW43
restart-engine failure must retain the exact runtime completion
detail/result/sequence through the root diagnostic instead of collapsing it to
a generic replay failure.

Collect `wifi diag` after the boot reaches a terminal. Its passive bounded
owner-lifetime line must precede the causal Gate 1 status line and include:

```text
wifi: gate 1 owner_lifetime lifetime_begun=<u32> lifetime_completed=<u32> lifetime_failed=<u32> lifetime_active=<yes|no|unknown> source=sdio-owner
```

Keeping the owner record separate ensures the following Gate 1 status retains
its complete `pwrseq_phase`, `dependency`, `source`, and `next` fields within
the 256-byte console bound.

On the clean cold path, exactly one new begun epoch may appear and that same
epoch must complete without becoming the current failed epoch. Gate 1 may pass
only when the record is valid and inactive, the nonzero
`begun_epoch == completed_epoch`, `failed_epoch != begun_epoch`, and that begun
epoch exactly matches the supervisor's expected `physical_lifetime_epoch`. A
second begun epoch before the first boot terminal, a missing lifetime record,
or a generic engine-replay failure that discarded the owner's detail/result
fails the single-lifetime proof.

That numeric owner record is proof, not publication authority. Successful
completion of the typed initial-physical-lifetime pair transaction separately
records cold provenance through its owned replay. A pre-handoff recovery can
produce the same numeric pair/lifetime epoch and replay state, so none of those
values may select a faster publisher. Every recovery request,
pair-transaction failure, unfinished cursor drop, and replay terminal clears
the provenance. Cold, recovery, and steady parents all use the same
phase-separated lane required by Reopened Milestone 26b task
`m26b-wifi-sdio-notification-dpc-closure`: ABI-invisible `Stage`, priority
handoffs, sequence-last `CommitRing`, exact `PublishGrant`, signal-last
`NotifyRing`, and later `PollRing` turns. A completion miss changes only the
retained phase; any replacement publication and notification occur on their
later explicit turns.

The matching completed epoch is the supervisor's
`physical_lifetime_epoch`. It must remain current through Gate 8 acceptance and
publication, operational Gate 8 continuity, and Gate 10 acceptance. The
passive service-work snapshot, EventPump durable Network-resume identity, and
identity-only lifetime RX cursor also bind it with the connection generation
and pair epoch. Any missing, active, failed, or changed owner epoch invalidates
those proofs and scheduler state; do not count a later gate, DHCP, or TCP
success as belonging to the discarded lifetime.

An epoch-zero snapshot may still report the recovery reason to the retained
bootstrap supervisor, but it cannot arm or preserve the EventPump wake cursor,
durable Network resume, physical-operator fence, priority lease, or NIC poll.
The same exclusion applies while recovery is active even if the owner ring
still contains the preceding completed epoch. Ordinary Network service resumes
only after the canonical pair transaction publishes a new completed lifetime
and recovery has ended.

Gate 4 clock evidence comes from the SDIO owner, not from inferred driver
state. The final retained `HOST_CONFIG` publishes one passive 44-byte,
sequence-last `DriverRuntimeSdioClockSnapshot` containing the completed
physical-lifetime epoch, requested/base/effective clocks, decoded divisor,
final `CLOCK_CONTROL` and `HOST_CONTROL` readbacks, read-back CCCR `SPEED` and
`BUS_INTERFACE_CONTROL`, and generated virtual-counter frequency. Root accepts
only two identical valid samples for the current completed lifetime; it does
not read SDIO MMIO.

On Pi 4, expect a 50,000,000 Hz request to resolve to 41,666,666 Hz from the
250,000,000 Hz BCM2711 source and legal even divisor `6`, with internal clock
stable, card clock enabled, CCCR `EHS`, and 4-bit width proved on both sides.
Elapsed-time deadlines use `CNTVCT_EL0` at the generated 54,000,000 Hz
`TIMER_CLOCK_HZ`. The effective 41,666,666 Hz value is the production setting,
not a slow-clock fallback. A missing, torn, stale, zero, or partial snapshot
blocks Gate 4 with explicit `unavailable` fields but does not rewrite an
otherwise successful physical `HOST_CONFIG` completion. Capture the adjacent
register detail and bounded Gate 4 line:

```text
wifi: evidence sdio_clock base=250000000Hz clock_control=<hex> host_control=<hex> card_high_speed=yes cccr_speed=<hex> ehs=yes cccr_if=<hex> timer_source=cntvct-el0 timer=54000000Hz
wifi: gate 4 name=ht-clock status=<pass|blocked|fail> evidence=clock_snapshot=current sequence=<n> lifetime=<e> requested=50000000Hz effective=41666666Hz divider=6 stable=yes card_enabled=yes width=4bit source=sdio-owner next=backplane-window
```

This record is CYW43/SDIO-only. It does not change or instrument the GENET
clock, descriptor, or service path.

SDIO descriptor opcode 8 (`GENERATION_RESET`) and opcode 10
(`GENERATION_COMMIT`) are retired, reserved tombstones. `wifi diag` must never
show either as an admitted recovery phase. They are typed-rejected before
SDHCI, DMA, mailbox, power-sequence, ring mutation, or retained-owner work.
Only root's canonical pair transaction may scrub the pair and invoke
`ReplaySdioEngine`. Do not confuse these retired SDIO numbers with active
CYW43 network op8 `RX_POLL` and op10 `CONTROL_POLL` telemetry.

The sole ordered event-drain snapshot is immediately pre-Join, after
`HostEapolPromisc` for a protected network or `OpenWpaAuth` for an open network.
It must observe two consecutive exact `Idle` terminals before Join event
ownership is armed. `FrameReady` or `Progress` resets the streak; 256 polls is
the finite fail-closed cap, not a mandatory delay on an already quiet boot. An
earlier post-UP drain is not a substitute. Join then retains the same pre-TX
drain through any SDPCM-credit wait: a newly queued EVENT or DATA frame returns
the cursor to the ordered drain before Function 2 TX. Treat HAL
child-command issue and `CONTROL_TX_BEGIN` as ownership only, not wire proof.
Only the exact Join request's post-Function-2 progress (or its typed
post-transmit terminal) may arm current-generation association events; older
events remain history.

The snapshot is completed by a Join-only final SDIO source fence. Immediately
before the Join Function 2 CMD53 writes `SDHCI_COMMAND`, the sole SDIO owner
samples host `CARD_INT`. An asserted source must produce the typed not-issued
terminal with no command, DMA, FIFO, SDPCM-sequence, containment, or
pair-recovery side effect. The same retained Join parent and absolute deadline
must then run a forced `DPC_ACTIVATE`, consume that level source through the
ordinary DPC lane, and repeat the normal drain/credit/setup path. Only the
source-clear child may issue the Join CMD53. Treat host/model tests as
implementation proof only: this closes the documented owner-side gap but does
not establish 10/10 Pi repeatability until fresh serial and paired-capture
cycles prove it.

Gate 8 is an ordered stability proof, not the first moment the linked transport
attaches. After transport/control attachment, require
`CYW43_BOOTSTRAP_SUPERVISOR ... status=stabilizing`. The sole boot episode has
one absolute 90,000-millisecond stabilization deadline. Gate 8 is passive and
cannot create a pair-recovery request. Production bootstrap admits one initial
physical pair and zero pre-service pair restarts; a separately typed
runtime/SDIO fault drains and fences its exact owner but cannot turn the boot
into pair 2. The original deadline remains authoritative through DHCP and TCP
listener admission. Before accepting Gate 8 commit, serial/qlog evidence must
contain this one all-or-nothing immutable transaction:

```text
wifi: gate 8 subgate=8a-pair-generation status=pass pair_epoch=<p> generation=<n> blocker=none
wifi: gate 8 subgate=8b-control-program status=pass pair_epoch=<p> generation=<n> blocker=none
wifi: gate 8 subgate=8c-join-terminal status=pass pair_epoch=<p> generation=<n> blocker=none
wifi: gate 8 subgate=8d-association-link status=pass pair_epoch=<p> generation=<n> blocker=none
wifi: gate 8 subgate=8e-bssid-refresh status=pass pair_epoch=<p> generation=<n> blocker=none
wifi: gate 8 subgate=8f-eapol-keys status=pass pair_epoch=<p> generation=<n> blocker=none
wifi: gate 8 subgate=8g-post-key-maintenance status=pass pair_epoch=<p> generation=<n> blocker=none
wifi: gate 8 subgate=8h-data-admission status=pass pair_epoch=<p> generation=<n> blocker=none
CYW43_GATE8_COMMIT attempt=1 status=ready pair_epoch=<p> generation=<n> deadline_ms=<n> console_seq=<n> telemetry_sinks=serial+qlog+hdmi consumer=data
```

Treat 8a and 8b as one pair/control epoch and 8c through 8h as one current
logical connection generation. The repeated `generation=<n>` field is the
connection-generation publication checked by the normalizer; it cannot be
used to stitch pair/control evidence from an earlier recovery into the current
snapshot. A partial, reordered, duplicated, generation-regressing, or
cross-recovery sequence, or any gap between 8h and `CYW43_GATE8_COMMIT`, fails
closed. The nine records are one immutable nonterminal commit transaction.
Commit requires the same stable pair epoch and logical generation on two
consecutive ordinary control turns. Both observations must be
publication-quiescent: no pending
current-generation host-EAPOL event or queued pre-secure EAPOL RX frame; no
host-EAPOL prompt, session work, deferred reauthentication, or post-association
BSSID work; no maintenance, logical-control, prompt-poll, terminal-drain, or
retained HAL driver-task owner; no recovery or rejoin; and an empty, healthy
linked SDIO DPC ring. The ring must have producer equal to consumer, zero
current-generation flags, and the same nonzero DPC epoch, producer watermark,
and cumulative overrun/IRQ-ACK-failure counters on both observations.
Historical nonzero counters remain admissible after a successful typed
recovery; movement is not. Any intervening owner activity, DPC publication,
counter movement, DPC epoch change, or logical/pair generation change clears
the candidate. Root rechecks the exact pair/generation/DPC/history receipt
before and after consumer-token publication, and a failed recheck publishes no
Gate 8 commit. In particular,
`8h ... status=pass` proves that the exact-generation handoff snapshot is
eligible; it does not open the steady DHCP/TCP consumer and is not accepted
Gate 8 by itself. The immediately adjacent commit is the lifecycle cut: root
first revalidates the same generation, handoff tokens, owner state, and
recovery state, then publishes the separate consumer token. A failed
publication retracts the candidate and publishes no commit. A later recovery
does not rewrite an earlier accepted record, but it retracts current
authorization and requires a fresh complete candidate plus commit before
ordinary data admission resumes. Once commit is published, one exact
current-generation NetData continuation remains legal and does not by itself
retract stable proof; before initial publication, the same owner must reach its
exact terminal so publication quiescence is true.

Gate 8 commit opens the data consumer so DHCP can run; it is not the
ready-to-use cut and must not admit host test commands. The unique terminal
`CYW43_BOOTSTRAP_SUPERVISOR attempt=1 status=ready ...` may appear later and
need not be adjacent, but it must bind to this committed generation and
requires the active CYW43/Wi-Fi interface, DHCP `bound`, a nonempty IP address,
and the actual TCP console listener. It intentionally does not require a prior
TCP accept/auth session. Only this supervisor Ready releases the final HDMI
`Ready to use` banner. Successful later repair uses
`CYW43_RUNTIME_RECOVERY status=ready generation=<n> ...`, never a second
bootstrap Ready.

Before initial publication, a queued current-generation EAPOL frame is still
a host-EAPOL policy obligation. Once BSSID/filter maintenance is terminal, the
same policy session consumes one queued frame per ordinary control turn and
retains any response continuation for later turns. It must not leave the frame
queued while simultaneously using that queue as the reason to fence NetData.
Post-key frames retain post-secure handling; no diagnostic or fallback poll is
introduced.

First-cause deferred-recovery and terminal-drain telemetry must remain visible
until the complete 8a-through-8h plus Gate 8 commit transaction is retained; a
rejected receipt or output preflight cannot erase it.

An ordinary firmware `AUTH` timeout is diagnostic telemetry, not a terminal
association event. Unsuccessful `SET_SSID`, link-down/no-network,
deauthentication, and disassociation remain in the single association
supervisor: 8c reports `pending` with
`blocker=association-retry-pending`, authentication is suspended only after
any accepted child action drains, and bounded backoff starts a new logical
generation/Join on the same linked pair. These logical connection failures do
not authorize pair normalization or SDIO recovery.
An exact BSSID-refresh failure at 8e similarly reports
`blocker=bssid-refresh-retry-pending`, and an exact required maintenance
failure at 8g reports
`blocker=post-key-maintenance-retry-pending`; both use that same supervisor,
same-pair backoff, and new logical generation.

Gate 8h cannot become a candidate pass until root has committed the handoff for
the same generation; that handoff commit remains fenced from steady consumers
until the Gate 8 commit publication above. Association-generation start is
too early: Cohesix may admit firmware data after secure keys while post-key
maintenance still owns the linked-runtime lane and before the root NetStack
exists, whereas Linux `brcmfmac` already has a live netdev consumer at
controlled-port opening.
Cohesix therefore continues the one ordinary attached Network turn for
association/control progress while the CYW43 NetDevice fences smoltcp RX/TX,
Device-originated fresh data polling, fresh ARP staging, and DHCP. The existing
pre-poll physical RX ingress remains live so association/control events can
advance through the sole policy lane; ordinary returned data is copied into
the current-generation queue rather than delivered. Once a post-turn snapshot
shows 8a through 8g passing, root calls
`commit_cyw43_data_handoff_if_ready` with that snapshot's logical connection
generation. It revalidates the complete connection/owner state, rejects only stale-generation tokens, preserves valid
current-generation backlog for the following consumer turn, snapshots the
sticky root-drop and runtime-overflow counters, release-publishes the baseline
generation token, and then release-publishes the consumer commit token last.
Root then captures the publishable post-commit snapshot. This commit performs
no SDIO/CYW43 I/O and introduces no second boot, owner, polling, or fallback
lane. Before it succeeds, 8h must report
`blocker=data-handoff-commit-pending`.

If that ordinary attached Network turn discovers a typed runtime/SDIO fault,
it commits no handoff and yields immediately. The next outer iteration is the
hardware-free Operator turn. Before exact DHCP/listener service readiness, the
following Driver turns may only drain, fence, and poison the uncertain exact
owner before terminal quarantine; they publish no `status=recovery`, cannot
start pair 2, and issue no replacement physical lifetime. After exact service
readiness, the following Driver iteration may service one child of the sole
consumed-once runtime recovery episode. Driver rechecks reboot and linked serial
admission before issuing any child, so accepted operator `reboot` work cannot
compose with or be followed by a same-iteration CYW43/SDIO operation.

The child decoded-RX queue, its bounded drain budget, and the root copied-RX
queue all use
`pi4_driver_abi::DRIVER_RUNTIME_CYW43_RX_QUEUE_CAP=50`. Do not tune a private
root or child capacity during diagnosis; the shared ABI value is the one
production envelope, and capacity alignment is not a substitute for the
handoff boundary.

The root Wi-Fi ingress path is one generation-scoped bounded aggregate of 16
immutable TX frames feeding one sole active `CYW43_PENDING_DATA_TX` op7 owner.
The aggregate uses urgent-control and bulk FIFO classes. ARP, EAPOL, DHCP, TCP
SYN/FIN/RST, and payload-free TCP control frames are urgent; other
payload-bearing TCP, fragmented IPv4, and ordinary traffic remain bulk. Any
response produced through the copied-RX paired token is urgent independently.
Urgent is selected first, with FIFO order within each class. These are two
logical priority queues, not two physical lanes.

Ordinary `Device::transmit` tokens may reserve only 15 aggregate slots;
`Device::receive` reserves its mandatory paired TxToken before removing a
copied RX frame and may use the final slot. Dropping an unused token releases
that local permit, and consuming a reservation cannot fail merely because an
older op7 is active. EventPump is the sole production TX coordinator. Before
each smoltcp poll, its one Wi-Fi-only hook may advance at most one physical turn
of the active owner. With copied RX pending and a paired permit available, the
hook returns before ARP staging and physical TX, so the device delivers copied
RX first even while the active op7 waits for credit. `Device::receive` and
reservation failure never service TX.

Otherwise the hook may move one eligible ARP/GARP record into the same urgent
aggregate. With all 16 slots in use, it may advance the active owner and
promote one successor locally after its terminal, restoring paired-RX capacity
without a second physical operation. A credit-wait deadline failure poisons
and restarts the exact CYW43/SDIO pair before any queued successor can begin an
independent timeout. A generation/reset boundary purges never-issued queued
frames; issued or otherwise ambiguous active ownership remains fail-closed and
poisons through the existing recovery path. This aggregate and service hook
are CYW43-only; GENET retains its existing direct device path.

Once a valid current-generation frame is accepted, the retained op7 owner keeps
its exact payload, digest, ticket, request, and generation across transient
SDPCM-credit waits until a typed `Submitted` terminal or its real
virtual-counter deadline. A fixed turn count is not a second lifetime bound:
there is no eight-turn abandonment. Corruption, lost ownership, generation
replacement, deadline expiry, or a typed fatal terminal remains fail-closed.
The focused regression holds one immutable frame across twelve no-credit turns
before successful submission.

Bounded data traffic is legal while 8h passes: non-full root RX, pending data
TX/ARP, runtime backlog, and an exact assigned current-generation NetData
request do not by themselves block readiness. A stale prompt generation,
request-less NetData pre-poll retained while root RX/TX/ARP work has priority,
a post-commit generation-local root-drop/runtime-overflow increase is a
failure. Before commit, 8h remains `pending`; pre-consumer pressure is captured
by the committed counter baseline rather than misclassified against a
generation-start baseline. After commit, a full root RX queue with no loss
remains pending while bounded drain work has priority. Cumulative root-drop
telemetry saturates rather than wrapping, and an exact generation latch still
proves any new post-commit loss at saturation. Recovery or generation advance
invalidates the token and rejects queued tokens captured in the superseded
generation; the next consumer-active generation captures a new baseline
without resetting cumulative counters. A stale-purge count is diagnostic
evidence, not successful receive or permission to ignore later loss.

Gate 8 data-consumer publication installs one identity-only RX cursor bound to
the connection generation, pair epoch, and completed physical-lifetime epoch
on the existing sole `NetData` op8 `RX_POLL`/DPC lane. It owns one
progress-conditioned, 32-millisecond lost-edge watchdog deadline scaled from
the exported seL4 virtual counter. Its progress signature comprises DPC
presence, epoch, producer, consumer, and flags plus root-wake presence, hits,
clears, and rechecks. Any progress change rebases the deadline and arms one
later one-shot. With no progress, expiry becomes one durable audit reason and
may claim exactly one fresh `RX_HINTLESS_FIRSTREAD` op8 through the same sole
owner.

The runtime signals on private-RX-queue empty-to-nonempty transition, HAL
latches that wake, and root clears and immediately rechecks it only after an
exact empty op8 terminal. This preserves an edge racing the clear while keeping
the queue, DPC ring, and latched wake as durable level truth. A current root
wake or live DPC producer level schedules one fresh queue/tail-drain NetData
op8 with flags exactly `RX_STEADY_TAIL_DRAIN`; neither adds
`RX_HINTLESS_FIRSTREAD`. DPC also keeps its persistent child owner urgent but
grants no duplicate source-inspection authority. That child owner
performs source inspection and bounded drain, then rechecks the durable
condition before sleep and rearms only when clear. Only the genuinely due
watchdog adds the hintless flag; a quiescent source inspection performs no
blind Function-2 read. Its exact current non-fault terminal suppresses another
probe until external progress or identity replacement re-arms it. Neither an
accepted TX nor non-source queue work creates another owner. Retained owner and
protocol continuations still advance on the sole op8/HAL/SDIO chain; GENET
never reads or reports this state. In the
paired pcap, check both that outbound TCP trains do not suppress peer ACK/FIN
ingress and that inbound traffic after a quiet interval triggers a prompt reply
without unrelated TX.

Capture the passive scheduler, handoff, and retained-frontier lines from both
`wifi diag` and `wifi probe-ht`:

```text
wifi: association scheduler service_turns=<n> join_starts=<n> control_progress=ordinary-network-turn
wifi: host_eapol work_pending=<yes|no> blocker=<none|deferred-reauth|prompt-poll|pending-event|queued-eapol|tx-submit|key-install|tx-drain|bssid-obligation> generation=<n> open_network=<yes|no>
wifi: host_eapol detail deferred_reauth=<yes|no> prompt_poll=<yes|no> pending_events=<n> pending_eapol=<n> tx_submit=<yes|no> key_install=<yes|no> tx_drain=<yes|no> bssid_obligation=<yes|no>
wifi: data_handoff generation=<n> committed=<yes|no> commit_token=<t> baseline_token=<t> baseline_generation=<n> queue=<used>/50 high_water=<n>
wifi: data_handoff lane consumer=<blocked|open> rx_watch=<absent|stale|watching|watchdog-due|watchdog-probing|watchdog-suppressed> rx_generation=<n> rx_pair=<p> rx_physical=<e> deadline_probes=<n> terminals=<n> control_progress=ordinary-network-turn
wifi: data_handoff counters root_drops=<n> baseline_drops=<n> drop_token=<t> runtime_overflows=<n> baseline_overflows=<n>
wifi: data_handoff stale_purge total=<n> last_token=<t> last_count=<n>
wifi: data_handoff boot_first_loss=no
wifi: data_handoff postcommit_first_loss=no
wifi: gate8 retained_frontier=no
wifi: gate8 retained_frontier=yes pair_epoch=<p> generation=<n> subgate=<token> status=<pass|pending|fail> blocker=<reason>
```

The retained field names remain for diagnostic and normalizer compatibility.
`watching` is an armed deadline, `watchdog-due` is one unclaimed expiry,
`watchdog-probing` owns the exact op8 ticket, and `watchdog-suppressed` is the
completed one-shot awaiting external progress or identity replacement.
`deadline_probes` and `terminals` each advance exactly once only when an exact
current non-fault hintless op8 terminal completes the claimed watchdog.
Elapsed intervals, queue-only terminals, and stale or fault completions advance
neither field.
`netstats: wifi_post_dhcp_rx` counts frames only when they cross an actual
smoltcp delivery boundary, so trace-only preserve/deliver events cannot count
one frame twice.

A live NetStack frontier becomes authoritative only with complete current
`8a`-through-`8h` proof and no active pair recovery. Until then, retain recovery,
bootstrap/resource failure, exact runtime/SDIO terminal evidence, and the
current ordered Gate 8 frontier as the causal authority. A text-only
host-EAPOL cause refines that frontier only when `8a` through `8e` have passed,
`8f-eapol-keys` is current, and no recovery is active; otherwise report it only
as secondary telemetry. Do not let DHCP counters relabel an earlier driver
failure. A complete serial diagnostic must still end in its terminal
`ACK`/`ERR` and prompt.

After recovery, use `gate8 retained_frontier` as the last non-recovery Gate 8
state; the live snapshot may correctly show only the generic pair-recovery
state. Zero association service/Join-start counters at an 8c
`join-submit-pending` frontier are scheduler-starvation evidence, not Wi-Fi
hardware warm-up.

When losses exist, the fourth and fifth lines read:

```text
wifi: data_handoff boot_first_loss=yes sampled_generation=<n> committed=<yes|no> reason=<reason> queue_len=<n> channel=<n> ethertype=0x<value> priority=<n> attribution=current-epoch-sample
wifi: data_handoff postcommit_first_loss=yes sampled_generation=<n> reason=<reason> queue_len=<n> channel=<n> ethertype=0x<value> priority=<n> attribution=current-epoch-sample
```

Retain its sampled generation, commit state, reason, queue length, channel,
EtherType, and priority. The
`attribution=current-epoch-sample` suffix says only that root sampled the loss
while that connection epoch was current; it is not producer, runtime, SDIO, or
physical-owner attribution. Correlate it with exact retained-owner and pcap
evidence before assigning a cause.

After Gate 8 commit, keep capturing until the unique supervisor Ready. A later
fresh non-stable or different-generation snapshot must produce
`status=stabilizing`; this can occur in the same logical generation when exact
owner/admission proof is lost. Discard the earlier snapshot and require a new
complete 8a-through-8h plus commit proof. A logical Gate 8 failure, or committed
service that still lacks same-generation DHCP/listener readiness, remains
inside the one boot episode until its original 90-second deadline. The latter
uses `blocker=service-readiness-deadline`. Deadline exhaustion retains the
complete eight-line terminal snapshot and adjacent
`CYW43_GATE8_TERMINAL ... action=quarantine`, then emits terminal
`status=permanent`, quarantines attached Wi-Fi network service, and returns to
ordinary operator service without pair repair. Serial, local-seat, HDMI
diagnostics, authentication, and `reboot` must remain responsive. There is no
automatic backoff, reset, or attempt 2. If retained-output capacity delays this
terminal transaction, only the hardware-free operator-output turn may run. A
  newly visible typed runtime/SDIO fault may preserve its stronger physical
  causal terminal while schema/route/capacity preflight remains blocked; it
  cannot reopen or repair the pre-service lifetime. Once preflight succeeds,
  root performs one final typed-fault probe and, if clear, commits the explicit
  terminal decision immediately before atomically retaining the batch. That
  decision cut, not later output drain, linearizes terminal policy; no
  network/child poll may reopen it while the batch and adjacent Permanent record
  drain. No pre-service fault may publish Recovery or open a second physical
  pair. Retraction purges
queued HDMI Ready/prompt bytes and forces a canonical Stabilizing redraw.
Gate-local association, DHCP, and protocol retries remain bounded inside their
owning gates. Gate 9 DHCP/address and Gate 10 nettest, TCP, and authenticated
`cohsh` must remain in the accepted Gate 8 connection generation. Each DHCP
start must also publish a fresh generation-bound XID; late Offer/ACK packets
carrying a prior XID are not current proof. Do not count a boot whose
generation change or readiness retraction invalidates those proofs. Exact
service readiness admits one distinct steady-state runtime-recovery episode
with one consumed-once pair repair. Duplicate Ready cannot replenish it, and a
successful restored generation is reported by `CYW43_RUNTIME_RECOVERY`, not a
new boot attempt. Gate 10 remains mandatory acceptance evidence but does not
grant recovery authority.

On the current shared-core linked-runtime design, the prompt-side supervisor
must prove the SDIO owner before the CYW43 client. SDIO service registration and
exact descriptor replay occur first, followed by CYW43 registration and
descriptor replay. After prerequisite and mailbox admission, every cold boot
and recovery enters the same sole 22-action owner-first pair transaction
without preliminary engine initialization. The transaction's one SDIO engine
replay establishes the physical lifetime and replays CYW43 before root
registers the SDIO owner and enters context replay. Both remain at bootstrap
priority `255` through engine, firmware, and control-context replay. Only after
control-plane readiness does
  one outer turn lower SDIO to its steady contract priority; a separate later
  turn lowers CYW43. Recovery raises and reprograms SDIO, raises and reprograms
CYW43 while both remain suspended, then resumes and proves the SDIO owner before
the CYW43 client. It likewise keeps both at `255` through engine and retained
context replay and performs the two owner-first lowers only after renewed
control-plane readiness. Each priority transition, register-programming step,
resume, descriptor replay, and engine replay is admitted as its own retained
operation. A capture
that stops at `CYW43_BOOTSTRAP_SUPERVISOR ... status=begin`, shows client-first
descriptor service, or depends on a legacy root-owned Wi-Fi path is failed
bootstrap evidence.

Bootstrap and recovery remain owner-first, per-action episodes at priority
`255`; neither uses the steady Network priority lease. After both members reach
their steady priorities, an actionable selected-WiFi `Network` quantum acquires
one pair-epoch-bound scheduling lease. HAL reserves both scheduling envelopes,
then boosts SDIO and CYW43 exactly once before the first parent is admitted.
Each exact current-generation root-to-CYW43 parent reuses that lease while
retaining its own immutable sequence-zero prepare, nonzero issue commit, grant,
notification, and completion state. Every parent uses separate sequence-zero
prepare, sequence-last commit, exact-grant publication, signal-last
notification, and later completion-poll turns. Once priority admission is
complete, `CommitRing` refreshes the immutable record, latches issued-unknown,
and commits the sequence last; the later `PublishGrant` turn publishes initial
grant 1, and the later `NotifyRing` turn commits `Issued` before signalling
last.
Cold-bootstrap priority `255`, recovery, and an already `Open` steady Network
lease differ only in scheduling context; they do not select different
publication protocols. Issued-unknown state is latched before commit, retained
state reaches `Issued` before signal, and no administrative phase performs a
physical SDIO action. Nonmatching, closing, stale, recovery-torn, and GENET
requests fail closed rather than acquiring another lane. Each later owner turn
still admits at most one CYW43/SDIO
child physical operation.

Ending the quantum first closes admission to fresh pair parents. If one exact
CYW43 parent is already `Prepared` or `Issued`, only that parent may drain;
once it is terminal, HAL restores CYW43 and then SDIO before the terminal phase
exit. A torn phase or reservation, pair-epoch change, invalid active parent,
or failed restore poisons the lease and requests the existing pair-recovery
lane. Quarantine and reboot must close or drain the same lease; an unprovable
close is pair-recovery evidence, never authority to discard scheduling state.
The sequence-zero prepare remains invisible to an autonomously polling child,
and commit remains the issued-unknown boundary. GENET does not acquire or
inspect this lease and its service rotation is unchanged.

Every root-to-CYW43 descriptor, including generation zero, uses the shared
continuation record and the reserved root badge on CYW43's bound notification.
The exact grant binds the immutable command, request, and logical connection
generation; CYW43 acknowledges it before one continuation quantum. After a
completion miss, `PollRing` records only `GrantRequired` for a consumed grant or
`Granted` for an exact unconsumed grant. The later `PublishGrant` turn
double-checks for a matching completion before publishing a replacement, and
the later `NotifyRing` turn commits `Issued` before signalling last. An
unconsumed grant skips replacement publication and is re-signalled only on the
later notify turn. If the runtime acknowledges the exact grant between
publication and notification, final revalidation advances to `Issued` without
a redundant signal and the next turn polls completion. A consumer acknowledgement
observed between stable grant snapshots is legal only as
`consumed_grant_id: 0 -> current` and must remain current on one bounded
confirmation read; reverse, skipped, foreign, or body-mutated observations fail
closed. The notification can coalesce or arrive before intake
  because it is only a scheduling hint; the durable grant is rechecked before
  quiescence. Endpoint delivery is rejected for this lane.
Other retained runtimes keep their endpoint rendezvous. No completion is
exposed until all leased priorities have returned to their manifest values. An
unresolved lease is cleared only inside fenced pair restart after both runtimes
are suspended.

HAL creates the reserved-root-badge send cap only for CYW43 and only at the
final successful runtime-construction boundary. Each retained root cursor
carries its original logical generation explicitly through every later grant,
active-request check, and completion poll. Never substitute the currently
published connection epoch for that field: an association or EAPOL epoch can
advance while an older exact cursor is still draining, but its ring command
must keep the old `aux1` and its terminal result must be rejected as stale
before it can update the replacement session.

Delegated CYW43-to-SDIO work cannot use that endpoint rule. The one-way owner
handoff deletes and zeros root's SDIO endpoint authority, and CYW43 has only its
shared owner window plus the send-only reciprocal notification cap. A retained
multi-phase SDIO command therefore uses one fixed 24-byte continuation-grant
record in reserved shared-ring bytes 40 through 63. Its magic, request sequence,
complete action fingerprint, authoritative SDIO generation, monotonic nonzero
grant id, and consumer-published consumed id make the notification only a wake
hint. CYW43 publishes the immutable body and grant id last, never overwrites an
unacknowledged grant, re-signals only that same id while acknowledgement is
absent, and publishes a new id only after the preceding id is consumed. Torn,
stale, wrong-generation, mutated, replayed, already-consumed, and exhausted-id
states fail closed.

The SDIO owner validates the delegated command against its independently
retained generation before executing the first action, binds that generation at
the first `Pending`, and irrevocably acknowledges exactly one exact grant before
spending its continuation quantum. On a stable completion miss, that delegated
`Poll` turn freezes only the retained cursor and next grant plan. The later
explicit `Grant` turn rechecks for a matching completion before publishing the
acknowledged replacement or re-signalling the exact unconsumed grant, commits
durable producer state, signals last, and returns without physical I/O; the
following producer turn polls again. Foreground and DPC-owned children use the
same `Poll -> Grant -> Poll` rule. A notification by itself cannot advance
either cursor, and a coalesced or already-consumed edge cannot be required
after an exact grant becomes durable. Every delegated `Pending` phase advances
through distinct `CheckWake`, `CheckGrant`, and `Execute` turns. `CheckWake`
polls the combined source; `CheckGrant` freezes one stable exact grant without
physical I/O; `Wait` performs the final durable-grant recheck before blocking;
and `Execute` acknowledges the frozen grant immediately before one owner
operation. A pending CARD_INT at `CheckWake` therefore wins one separate
service turn and leaves the grant unconsumed for later grant and execute turns.

The SDIO-to-CYW43 badge-2 completion/DPC notification and badge-159 SDIO IRQ
remain coalescing service wakes and cannot advance foreground work. CYW43 also
holds one send-only badge-256 cap to the SDIO owner's bound notification; it is
a scheduling hint for an already-published command or grant, not a
service source or foreground authority. The reserved high notification bit is
excluded from service work. If badge 159 and badge 256 coalesce, SDIO services
exactly one IRQ quantum in that slice and leaves the matching immutable grant
unconsumed for a later slice. A standalone reasserted level wake cannot spend
that grant. CARD_INT pending at any retained admission observation wins; an
interrupt arriving after that linearization point remains
latched and is delayed by at most one already-admitted owner quantum. If deferred
service consumes a root scheduling edge, the same unconsumed exact grant
remains authoritative for a later CYW43 foreground turn. Scheduler handoffs after service and
rejected ready wakes prevent a priority-255 private IRQ loop. Idle runtimes
block on their combined endpoint/notification receive rather than spinning.
An idle CARD_INT service always hands off before a durable owner command is
admitted, so a coalesced IRQ and command cannot perform two owner quanta in one
scheduler slice.
The immutable root/delegated command descriptor makes every generation-bound
CYW43 lane grant-only even if mutable gate state was lost or reset; endpoint
wake is not a fallback. Issued-unknown completion reaping consumes one
arbitration turn and then preserves/re-arms the exact continuation grant before
waiting for a late terminal or the canonical pair restart.
Pending-command DPC arbitration performs at most one retained DPC or foreground
  action per released quantum. A reciprocal CYW43-to-SDIO transaction uses one
  producer turn to submit the immutable child-ring command. A completion miss
  only schedules the later explicit grant turn; that grant turn rechecks for a
  late terminal, publishes or re-signals the exact next grant, and signals last
  without physical I/O. The owner then crosses distinct wake-poll, stable-grant,
  and ACK-before-I/O execute turns before at most one physical quantum. DPC and foreground routes have
  identical grant bounds. Neither path may privately yield,
  resignal, or poll itself into another owner quantum. A trace that shows
  multiple non-CYW43 foreground phases after one endpoint rendezvous, multiple
  root/delegated CYW43 foreground phases after one exact shared grant, or
  foreground progress caused by a scheduling badge without a valid grant, fails
  the one-operation-per-turn contract.

The 2026-07-31 exact `0b15321d0c12` /
`ee68c7c6faeb83b76934df87caea9f93feed94f8355b7286f999da51652a7753`
hardware group proves seven consecutive first-lifetime starts: one power-off
boot and warm R01-R06 all used supervisor attempt 1 and physical pair epoch 1,
passed Gate 8a-8h, and bound DHCP without pair recovery. Preserve that 7/7
startup result separately from the still-failing data-plane result.

Across the six ICMP-probed lifetimes—the power-off boot and R01-R05—the first
request reached the Pi and caused it to ARP-resolve the host about 126-179 ms
later. smoltcp 0.13.1 then discarded its already-constructed stateless Echo
Reply when Ethernet dispatch reported the unresolved neighbor. This proves
working CYW43 ingress and exposes a separate common NetStack reply-lifetime
defect; a missing first reply is not acceptable cold-neighbor behavior.
Raw SYN-to-SYN/ACK latency ranged from about 124 to 392 ms. The no-retry
pressure pcap contained one clean 506-request TCP stream with no
retransmission, reset, sequence disorder, reconnect, or zero window, but
response latency was about 380 ms at p50 and 468 ms at p95 with only about
2.42 exchanges/s. One R03 diagnostic accumulated 139,053 CYW43 turns, 2,788
time-cap exits, 376 deadline probes, and 382 probe terminals against 58
root-wake hits while queues and loss counters stayed clean. USB and HDMI
remained healthy. The persistent-flow result is deterministic CYW43 cadence,
not RF/TCP recovery, and it is not GENET evidence.

Across the instrumented slow-path samples, roughly 140,000-168,000 outer
Network turns ran over about 130 seconds, or roughly 1,100-1,300 turns/s, while
only about 10,000-11,000 `cyw43_quantum` runs completed. f4 R02 recorded
153,576 Network turns for 1,936 covered root-to-CYW43 requests, about 79 turns
per covered request, although urgent and empty DPC work means that ratio is not
a fixed protocol phase count. Root/runtime RX queues had zero drops or
overruns. The paired pcap returned host ACKs for Pi payload in roughly
0.1-0.2 ms but left some later Pi segments off air for roughly 0.37-0.40 s.
This rules out RF/TCP recovery, smoltcp queue loss, and a simple shortage of
raw EventPump opportunities as the primary delay.

Source audit falsifies ordinary request setup as the missing f4 fix: f4 used
`PREISSUE_STEP_BOUND=16` to batch deterministic CMD52/CMD53
preflight/register setup and the sole COMMAND in one owner quantum, apart from
its request-owned status-clear retry. The stronger amplifier is the nested
root-to-CYW43 and CYW43-to-SDIO grant/wake pipeline. One root parent separately
crossed prepare, commit, grant publication, notification, completion poll, and
consumer wake/grant/execute turns. A normal linked Function-2 receive uses
about five physical SDIO children when the frame fits the first read, or six
when it needs a remainder: status, optional W1C, the Function-2 data reads,
the Linux-style empty confirming read, and post-status. SDIO-core
`0x18004000` and the Function-2 FIFO at `0x18000000` share the same 32-KiB
aperture, so the hot DPC path adds no Function-1 window CMD52s; a genuinely
cold aperture adds LOW/MID/HIGH once. The repeated retained owner admission
around those real children, rather than fictitious per-packet reprimes,
created the software-turn amplification.

The exact `25f406d9cc26` image (image id
`92d8326196f954c5f56b45b092cc2b17ae7cf5ffe9bfff7bbc6df806c1030884`,
SHA-256
`6c2fcbb266e4158f94ef6436b8fc37830118111ce53b2239a066318448cd19a1`)
tested the fused scheduler candidate and failed it as a single-lifetime design.
The power-off boot plus warm R01-R05 all reached Gate 8a-8d on attempt 1 and
pair 1, then failed Gate 8e with `host-eapol-prerequisite-required`; none
reached DHCP or TCP. R01-R04 first recorded PTK-stage deauthentication reason 2
at generations 1, 2, 1, and 3 respectively, while R05 recorded no first
terminal receipt before the common generation-6 boundary. DPC ring counters
remained loss-free and balanced. The boot-paired Wi-Fi pcap contained 32
Pi-source LLC/XID broadcasts and no Pi EAPOL, ARP, IPv4, DHCP, ICMP, or TCP.
Consequently `.coh`, TCP latency, and REST pressure qualification were
correctly withheld. This is 0/5 warm usable service and no Wi-Fi performance
claim.

The current source restores `f4fec9e80`'s proven phase-separated scheduling
without removing the later immutable descriptor, `Wait` recheck,
issued-unknown reaping/re-arm, bounded TX aggregate, or v11 telemetry. Root uses
distinct `Stage`, `CommitRing`, `PublishGrant`, signal-last `NotifyRing`, and
`PollRing` turns. Delegated foreground and DPC producers use distinct
`Poll -> Grant -> Poll` turns. The SDIO owner uses distinct `CheckWake`,
`CheckGrant`, and ACK-before-I/O `Execute` turns. CARD_INT pending at
`CheckWake` wins one service quantum without consuming the grant; `Wait` keeps
the final condition-before-sleep grant recheck; a later interrupt remains
latched behind at most one already-admitted execute quantum; and ACK failure
performs zero I/O. The immutable command itself keeps every generation-bound
lane grant-only after mutable gate loss, and issued-unknown reaping preserves
the exact continuation until a late terminal or the canonical restart.

R06 deliberately sent TCP before ping: the Pi ARP-resolved the host 130.616 ms
after the SYN, retained that SYN across neighbor resolution, returned one
SYN/ACK at 322.861 ms, and closed cleanly without a SYN retry. Record this cold
TCP sample separately from ARP-warmed driver latency.

Cohesix now uses one driver-neutral fixed-capacity raw IPv4/ICMP socket as the
sole Echo Reply lane. It validates the exact assigned local destination,
unicast source, IPv4/ICMP lengths and checksums, and Echo Request type; admits
at most one reply construction per NetStack service turn; and preserves one
exact reply in smoltcp's TX queue while ordinary ARP resolution is pending.
Identifier, sequence, and payload are unchanged. Invalid, competing, or
saturated input emits nothing. WiFi generation, DHCP address, and explicit
stack-reset transitions purge both queues, and a three-second virtual-counter
deadline expires unresolved state. The lane neither issues CYW43/SDIO work nor
changes GENET scheduling.

The later exact `b91b31f9a2b471d37ceeb66469e3fc10609e4df2` /
`a70ca8e8f03c280302306a87e9d6f67f493488d786e858331dcb7e75b19c0433`
hardware group is the current defect-discovery boundary, not acceptance proof.
The power-off boot and warm R01 each transmitted 40 DHCP
Discovers and received 39 promptly returned Offers in the paired Wi-Fi capture
but issued no Request. R02-R05 each completed one DORA and bound
`192.168.86.154`, so bootstrap was only 4/5. After R05 remained idle for 130
seconds, 21 host Echo Requests produced no reply; source-level DPC/root-wake
counters did not advance. `.coh` and Wi-Fi pressure were correctly withheld.
This separates a retained-TX/root-copied-Offer head-of-line defect from a later
lost/coalesced linked-runtime receive edge. Both repairs described here are
source-only until a newly built image is flashed and repeats the hardware run.

The GENET control on the same exact image remained healthy. Its first cold Echo
Request was retained while the Pi ARP-resolved the host and then returned one
matching Echo Reply. Three `.coh` scripts and a post-pressure repeat passed. The
one-minute REST/hive run completed 9,956 of 9,960 operations; the four failures
were bounded application-level `schedule_write ... buffer-full` responses. Its
one sustained TCP flow had no RST, zero window, duplicate range, overlap, or
sequence gap, and request-to-first-Pi-payload latency was 1.216 ms p50 and
1.538 ms p95. No GENET change is authorized by that evidence.

Linux `brcmfmac` normally enables SDIO interrupts and disables polling. Its ISR
schedules one ordered DPC worker, which performs an RX-first bounded drain
before sleep; TX completion does not require a physical receive-source read.
The bounds remain `BRCMF_RXBOUND=50`, `BRCMF_TXBOUND=20`, and
`BRCMF_TXMINMAX=1` while RX remains pending, backed by a 2,048-entry TX queue,
32-KiB aggregation, and block-mode CMD53. Cohesix already has the transport
equivalents: a 50-frame RX bound, credits and glom, a 32-KiB shared aperture,
512-byte Function 2 blocks, multi-block CMD53, and external DMA. The Pi test
must verify the linked-runtime translation: the compiler-declared
empty-to-nonempty child wake, durable DPC/root-wake and queue levels, one
bounded EventPump Network quantum with physical-console checkpoints, exact
grants, one HAL owner, one linked SDIO runtime issuer, and no private physical
drain loop.

The as-built source candidate has no post-TX receive probe and no second
receive lane. It preserves f4's existing ordinary CMD52/CMD53 setup invariant:
one exact admitted owner quantum with `PREISSUE_STEP_BOUND=16` includes the
deterministic Linux-ordered preflight/register sequence and exactly one
COMMAND. Only a request-owned status-clear verification that still observes
owned W1C bits retries across a later turn; the pre-issue deadline remains
authoritative. This is not a new cadence repair. The current unflashed
`DPC_ACTIVATE` change executes its ordered
mask/inspect/publish-or-coalesce/ACK/signal/rearm sequence within
`SDIO_DPC_ACTIVATE_STEP_BOUND=32`; only failure of the frozen exact IRQ ACK may
retain an ACK-only retry across a later turn. Exhausting
`SDIO_DPC_IRQ_ACK_ATTEMPTS=3` fails closed with the single durable CARD_INT
event retained, the source masked, ACK still pending, and DPC activation
disabled; it must not reread or republish the event. Its exact-lifetime cursor
also carries the progress-conditioned 32-ms lost-edge watchdog described above.
External progress rebases and re-arms one later one-shot; one no-progress
expiry may claim exactly one hintless op8 source inspection, and its exact
non-fault terminal suppresses repetition.
DPC producer and root-wake levels may each demand one queue/tail-drain
`RX_STEADY_TAIL_DRAIN` op8, but neither adds source-inspection authority; only
that due audit adds `RX_HINTLESS_FIRSTREAD`. Queues, retained owners, and
protocol continuations still schedule only their own already-proved work. The
25-ms virtual-counter cap fences only fresh-parent admission; an already
admitted exact parent continues to its typed terminal subject to
physical/dispatch yields and the hard 192-turn bound. The independent 25-ms
operator checkpoint clock admits one complete
`Serial -> LocalSeat -> Dispatch -> pending Display` rotation only when elapsed
time requires it; operation count alone never yields. HAL must revalidate that
parent's root-continuation operation, nonzero immutable fingerprint, request,
logical generation, pair epoch, open priority reservations, and restart-free
state across the check. The next exact image must preserve first-pair startup.
Before any peer-warming traffic on every lifetime, it must answer one first
Echo Request across the ordered cold trace
`Echo Request -> Pi ARP Request -> host ARP Reply -> matching Echo Reply`
without a second request or duplicate reply. Record that semantic/elapsed
sample separately, then require ARP-warmed request-to-first-payload p95 at or
below 40 ms and at least 29 sequential requests/s with no loss, reconnect, or
benchmark timeout. The aggressive target is p95 at or below 10 ms and at least
100 requests/s. The GENET control must retain the same common cold-neighbor
semantics and its accepted wired performance. Until the rebuilt/read-back
image produces that evidence, neither source repair nor performance target has
Pi proof.

The historical capture `pi4-serial-20260724-214130.log`, with the bounded
post-exhaustion sidecar `pi4-serial-20260724-6c6-postexhaust-diag.log` and
boot-paired `tcpdump-wifi-20260724-214126.pcap` plus
`tcpdump-usb-eth-20260724-214126.pcap`. It identifies clean marker commit
`6c6d376768e6` and image id
`8c62fc9ea9d08a560a1b92a775cc8dbafad98a89d19fc860213b4a13a785dbaf`.
This is historical old-image evidence from the retired five-outer-attempt
policy, not the current production lifecycle. All five historical attempts stop
before Gate 1 in the first `TRANSPORT_INIT` reciprocal
`HOST_CONFIG` episode and eventually require pair restart. Serial, USB Gate 10,
and all six runtime TCBs remain live; both paired captures contain zero
Pi-originated frames. The cached SDIO `sequence=2`, `command-observed`,
`aux0=ENGN` marker is root engine-init history, not proof that the current
high-domain child was observed or missed. Diagnostics therefore report owner
intake as unknown while preserving the exact pair-restart blocker. A current
owner intake is proved only by the one-shot `sdio-owner-command-admitted`
marker with the delegated high-domain sequence and nonzero current generation.
Later retained turns preserve their exact grant frontier instead of republishing
that intake marker.

The hardware-free correction treats the immutable grant—not notification
history—as the durable condition and routes every retained foreground and DPC
child through the phased wake/grant/service/execute arbiter above. A new
production-mode host test drives the actual card-init `HOST_CONFIG` builder,
reciprocal ring, retained owner cursor, exact grant acknowledgement, and replay
rejection. This remains a software result until the next exact image boots.

The immediate predecessor capture is `pi4-serial-20260724-193452.log`, with
`pi4-serial-20260724-3ad-live-gate8-diag.log` and boot-paired
`tcpdump-wifi-20260724-193450.pcap` plus
`tcpdump-usb-eth-20260724-193450.pcap`. It identifies clean marker commit
`3ad1076404b7` and image id
`840342e2d12dd16c2847f911a1d667043b2a66c82d8e8d1c9472daefc8d6dd8e`.
That retained lane repeatedly completed firmware, Function 2, and control-plane
setup through Gate 7 before the first Gate 8 host-EAPOL control poll stalled.
Its Wi-Fi capture contains one Pi-originated LLC XID response per inspected
generation and no DHCP, ARP, IP, or TCP. It remains the strongest current
upper-path physical frontier, not proof for the corrected next image.

The exact predecessor capture is `pi4-serial-20260722-210743.log`, boot-paired
with `tcpdump-wifi-20260722-210754.pcap` and
`tcpdump-usb-eth-20260722-210754.pcap`. It identifies marker commit
`7328bedd6142` and image id
`5a9f812c5408998e3292b4c4475bee545a4c8f2d0e5781a238054598ff313001`.
That older image stopped at Gate 6's first 2,048-byte Function 1 firmware CMD53
after exactly two 64-byte blocks. It remains transport history, not the current
Gate 8 frontier.

The preceding exact capture is `pi4-serial-20260721-191137.log`, boot-paired
with `tcpdump-wifi-20260721-191146.pcap` and
`tcpdump-usb-eth-20260721-191146.pcap`. It identifies marker commit
`cfc034bb5417` and image id
`d3b4acf30a59af1cbdf3c0a9d6401dd7193552fc17250707164ef3287d379738`.
Its first 8,192-byte/count-128 Function 1 CMD53 also advanced only two blocks
before the FIFO/DREQ stall. That earlier geometry remains historical evidence;
the 2-KiB/count-32 request above is the current physical frontier.

The preceding exact captures are
`pi4-serial-20260721-063257.log` and the authenticated, paced reboot sidecar
`pi4-serial-20260721-064156-3538d28-W02-pyserial.log`, boot-paired with
`tcpdump-wifi-20260721-063254.pcap` and
`tcpdump-usb-eth-20260721-063254.pcap`. Both boots identify marker commit
`3538d28eee83` and image id
`f8b1cc9063de3f36d94b347c47cb10843c739ba6f864c088c11e26777a36482d`.
Gate 1 is direct and Gates 2-5 are explicitly inference-only. Gate 6 stops on
the first 64-byte/count-1 Function 1 firmware CMD53 at backplane address
`0x00198000`. The command has a clean R5 response and exact payload digest, but
the pre-containment snapshot is `PRESENT_STATE=0x01ef0006`,
`INT_STATUS=0x00000000`, and `BLOCK_SIZE_COUNT=0x00017040`: the data path stays
active/inhibited and the block count never decrements. This eliminates a lost
`DATA_END`, post-transfer W1C, stale payload, or safe replay as the primary
cause. Under that historical old-image policy, recovery poisoned the
generation, restarted the pair in bounded order, and exhausted after five
attempts without an endless loop. The current production policy does not replay
those five outer attempts.

Within either conservative boot slice, neither paired capture contains the Pi
Wi-Fi MAC, EAPOL, DHCP, ARP, IP, ICMP, TCP, or console-port traffic. The Wi-Fi
capture is managed Ethernet rather than monitor mode, so it cannot prove an
absence of over-air EAPOL; it does corroborate that this boot never reached the
host network path. TCP, association, DHCP, and `cohsh` remain downstream and
must not be changed to treat this pre-firmware transport failure.

The captured image changes the sole linked-runtime lane, not the launch path.
After CMD7, a retained request- and generation-bound cursor reads
CCCR revision and capabilities, requires `CAP_SMB`, a legal four-bit card, and
SHS, enables/verifies EHS, programs the high-speed clock while still one-bit,
read-modify-writes and verifies CCCR `IF`, and only
then switches the host to four-bit. Function block sizes and Function 1 enable
follow that adoption. A recovery adoption is stamped with the pending E+1 epoch
before the exact commit, so the commit cannot make freshly rebuilt card facts
stale. The historical `7328bedd6142` image retained SDHCI, firmware-mailbox,
and BCM2835 DMA MMIO plus the then-current DMA pages and sent its observed
CMD53 through physical channel 4, SDIO DREQ 11, FIFO bus address
`0x7e300020`, and low-RAM `0xc0000000` aliasing. That image proves only that
the earlier card-lane/count-32 external-DMA request reached the two-block
physical frontier.

Current source retains one exact 32-KiB backplane aperture and applies the
Linux/Raspberry Pi `mmc-bcm2835` engine threshold inside the sole retained SDIO
owner: a normalized host block count of at most two seals PIO into the request
identity, while more than two seals the external DMA engine. A full window is
`511 * 64` plus `1 * 64`, so the first child uses external DMA and the second
uses retained PIO; the true final window uses full blocks followed by one
bounded four-byte-padded PIO byte tail. This is one request cursor and one
recovery lane, not selectable launch paths. No request may switch engine,
fallback, or replay after issue. This Linux-equivalent lifecycle remains an
offline source-and-image candidate until its exact candidate is read back and
booted.

The current source candidate advances that same one production lane more
aggressively toward the pinned Linux lifecycle. Every CMD5, CMD52, and CMD53
uses a separate 10-millisecond pre-issue inhibit fence followed by a fresh
10-second request watchdog; data and short-busy requests set
`TIMEOUT_CONTROL=0x0e`. The bounded envelope allows two transfer attempts only
when entry-inhibit proves the first was never issued, gives each failed attempt
an independent 220-millisecond containment interval, derives the shared
20.56-second child bound, and gives root a 30.56-second per-child lease. Only a
fresh exact `OWNER_REPLY` edge renews that lease; wrong-sequence, repeated, or
unrelated progress cannot, and the shared 1,024-action trace caps renewals for
one immutable parent. Power/reset, engine state/health/policy publication, host
configuration, enumeration, and DPC activation are retained phase machines.
Reciprocal-ring scrub precedes the sole `ReplaySdioEngine` lifetime; no later
runtime generation-reset or generation-commit phase exists.
For data CMD53, the normalized host block count seals retained PIO for one or
two blocks and external DMA for more than two. Both shapes use one finite
preissue/issue owner quantum with `PREISSUE_STEP_BOUND=16`, admitted by the
shared 256-operation contract, to inspect/repair/verify block-gap state; clear
status; and program timeout, block size/count, argument, transfer mode, and
exactly one COMMAND. If request-owned W1C status remains after the clear
readback, the cursor alone retains `ProgramVerifyStatusClear` for a later
deadline-bounded retry; deterministic setup otherwise does not surrender the
owner between registers. PIO alone programs a request-local policy that adds
its direction-correct ready source. External DMA retains the persistent idle
interrupt policy, admits DMA authority, proves the channel idle, stages the
full immutable chain, and starts the BCM2835 channel after COMMAND in that same
indivisible Linux-ordered quantum, matching `bcm2835_mmc_request()`. No
completion poll follows issue in that quantum.

After fresh response/R5 validation, retained PIO requires a fresh
direction-correct ready edge plus matching `PRESENT_STATE` block ownership and
moves exactly one complete normalized host block, 1-512 bytes and at most 128
`SDHCI_BUFFER` accesses, in that owner quantum. It cannot cross into the next
block, which requires another fresh ready edge. An early ready edge without
present-state ownership cannot authorize a later FIFO access without a fresh
edge. A terminal PIO snapshot restores ordinary interrupt policy before
publishing completion in the same bounded quantum. External DMA lets peripheral
DREQ control movement and consumes one immutable SDHCI/DMA snapshot per later
turn. Both engines join response, exact payload movement, possibly coalesced
`DATA_END`, and host quiescence. External DMA additionally requires terminal
`CONBLK_AD == 0`, this
request's `CS.INT`, and no DMA error; `CS.INT` is acknowledged with Linux's
`INT | ACTIVE` W1C value. Its terminal control block carries Linux's `INT_EN`;
`SDHCI_TRNS_DMA` remains clear because this is the external dmaengine path.
RESET, control-block address, and ACTIVE publication is followed by a full
store-completion fence and same-channel readback.

Idle and external-DMA interrupt enable use the exact named mask `0x02ff000b`.
An active PIO request removes DMA-only `DMA_END` and `ADMA_ERROR`, adds only
its direction-correct ready source, and preserves that source across any
interleaved CARD_INT policy rewrite without adding it to `SIGNAL_ENABLE`.
`CARD_INT` is added when armed; broad terminal error detection remains
`0xffff8000`.
Command/controller or R5 failure is post-issue work. The common path captures
telemetry, contains the selected engine as applicable, acknowledges/resets the
host, restores its clock, takes final inhibit/snapshot evidence, poisons the
generation, and never switches engine or replays the request. Before
the first firmware CMD53, retained one-operation EventPump turns re-read the
Function 1 block size, CCCR capabilities/interface/speed, ALP state, and exact
RAM window; only a later local commit publishes that complete live contract.
Every failure cut invalidates it and performs no replay. Terminal telemetry v3
adds SDHCI argument, transfer/command, timeout/block-gap, interrupt/signal,
host-control-2, and DMA transfer registers. After initialization, the SDIO
runtime rejects every legacy aux-packed service command and admits only the
fixed typed reciprocal descriptor. Its `DPC_ACTIVATE` turn no longer reads
Function 1 `RFRAMEBCLO`/`RFRAMEBCHI`: it publishes a latched host `CARD_INT` or
advances a retained masked rearm. One exact admitted grant runs the ordered
state, health, `INT_ENABLE`, `SIGNAL_ENABLE`, status/ring inspection,
publish/coalesce, exact IRQ acknowledgement, signal, and rearm policy as one
bounded owner quantum of at most 32 phase steps. Only a failed acknowledgement
of the frozen exact IRQ epoch may cross into a later turn; that retry performs
no status reread, event republish, or device replay. Dongle/FIFO source
inspection and bounded drain stay with the persistent CYW43 DPC, which
rechecks the durable condition before sleeping and rearming. Root configures
the EventPump in place, borrows it through both Genet and deferred-WiFi loops,
and retains one WiFi supervisor in place without rearming its boot episode.
The emitted Pi release chain now
retains 136,480 bytes at the outer WiFi
baseline and leaves 125,664 bytes of its 256-KiB root stack before nested calls.
All changes in this paragraph remain an offline source-and-image candidate
until that exact candidate is read back and booted on the Pi; they do not
change the last physical Gate 6 result above.

The earlier `pi4-serial-20260719-180716.log` capture identified a separate
pre-command continuation-liveness failure: SDIO engine initialization completed
and CYW43 engine initialization began, but the supervisor replayed SDIO engine
work for more than seven million outer turns. It is retained as historical
evidence for the shared continuation-grant correction, not the current first
failure.

Production-chain inspection found the remaining stranded edge. The retained
shared grant was correct, but CYW43 still announced both first intake and later
grants with `seL4_NBSend` to the SDIO command endpoint. An endpoint `NBSend` is
discarded unless SDIO is already receiving. The fresh Gate 8 trace reaches an
active SDIO child beside CARD_INT and then waits exactly to the derived child
bound, which is the expected shape when the owner is servicing or retaining
work as the one nonblocking continuation send occurs. The July 10 compatibility
oracle hid this edge by repeatedly sending inside private yield/poll loops.

The single production lane now keeps endpoint rendezvous only for non-CYW43
root-to-runtime commands. Root-to-CYW43 continuations use a send-only
reserved-root-badge cap to CYW43's own bound notification and an exact grant in
the CYW43 ring. HAL separately mints CYW43 one send-only badge-256 cap from the
SDIO owner's bound notification. The shared command or exact unused generation
grant remains the sole authority; the notification is only a lossless
coalescing scheduling edge. Badge 256 is disjoint from CARD_INT badge 159. When
they coalesce, one `CheckWake` turn services one IRQ quantum, an explicit
scheduler handoff follows, and the preserved grant may release only one owner
quantum after later `CheckGrant` and `Execute` turns. A root completion miss
changes only the retained `PollRing` phase; any required `PublishGrant` and
signal-last `NotifyRing` actions occur on later turns. A matching completion
wins before either authority action. Delegated foreground/DPC producers retain
the analogous explicit `Poll -> Grant -> Poll` sequence, while non-CYW43 and
GENET endpoint paths retain their existing phases.
Stale, malformed, wrong-generation, mismatched-consumed, and replayed grants
still reject.
Foreground and DPC child timeouts measure inactivity rather than total
transaction age: consuming the exact continuation grant proves owner progress
and rebases both the counter-backed and fallback-poll deadline. The runtime
also completion-reaps issued-unknown work before applying the pair-restart
hold. A late exact child completion is ownership proof only; its result and
payload are quarantined, the child is released, and the old parent emits one
exact typed terminal without same-generation replay before the canonical
restart.
The diagnostic also separates live ring poison from a stale
client-counter sample, so a mixed snapshot no longer misreports an intact ring
as physically poisoned. These are hardware-free corrections until the next
exact image is rebuilt, read back, and booted; this capture does not prove the
fixed hardware result.

The exact `f78208ce709a` boot also predates the single-episode policy. It reached
`cyw43-control-plane-ready` in all six historical generations, then fenced each
generation 4.46 to 4.72 seconds later. Its historical post-exhaustion
diagnostic reported CYW43 network op10 `CONTROL_POLL` detail `0x5310` and result
`0x32`; this was not retired SDIO descriptor opcode 10.
`0x32` is DPC event sequence 50, not a CYW43 generation. Runtime arbitration
prevents the ordinary DPC from consuming a source event while its foreground
source-probe transaction is active, and a production-ring zero-status
regression reaches typed `Idle(NoRframe)` without a restart. The earlier
completion-race explanation was therefore disproved.

The actual software defect was loss of the primitive DPC cause. Exact SDIO
child faults recorded useful detail, result, and frame, while several structural
DPC faults recorded only counters; the later prompt quarantine could overwrite
either class with generic bus-link detail plus event sequence 50. The f782
capture therefore cannot identify which primitive branch fired. The corrected
runtime retains the first terminal DPC detail, result, fault frame, event
sequence, action, and I/O phase. It permits one fresh child ticket only for an
exact, telemetry-bound, contained entry-inhibit result, which proves the SDHCI
command was not issued. Every second inhibit failure, malformed ring/cursor
state, missing or inconsistent telemetry, command-or-later failure, owner-path
poison, timeout, or issued-unknown outcome requests the deterministic pair
restart without same-generation replay. Passive `wifi diag` also retains the
first immutable association/host-EAPOL root owner before recovery teardown, so
later root-grant progress cannot rewrite a Gate 8 failure as Gate 2. This
correction remains hardware-free until a new exact image is booted.

The July 10 W01 capture
`pi4-serial-20260710-123050-m26d-authoritative-W01-pyserial.log`, paired with
`tcpdump-wifi-20260710-112826.pcap`, remains the upper-path compatibility
oracle. The `918a58c09-dirty` image completed all ten Wi-Fi gates, host EAPOL,
PTK/GTK installation, DHCP, raw TCP, and authenticated `boot_v0.coh` plus
`smp_parity.coh`, ending with `tcp_accepts=4 tcp_auth=4`. It proves that the
Linux-shaped post-transport association/EAPOL/DHCP/TCP path can work on this
board; it is not current-image proof, a repeatability result, or permission to
restore timing-dependent loops, same-generation replay, root-owned SDIO, or any
legacy fallback.

Every returned pending turn is appended as a full-fidelity record to the
bounded `/log/queen.log` software ledger. The
`CYW43_BOOTSTRAP_TURN attempt=<n> turn=<n> stage=<stage> operation=<bool>
repeat=<n>` line is its sparse live-serial mirror: after the Wi-Fi HAL guard is
released, an all-or-nothing linked-serial enqueue is attempted on stage changes
and power-of-two repeats, and a rejected enqueue remains eligible for a later
same-stage attempt. Accepted bytes flush on a later operator turn. The UART
copy is deliberately best-effort under bounded queue pressure, so its absence
alone does not prove a supervisor freeze; it does leave a required capture
predicate unproved. After linked-runtime cutover, the child is the sole
physical UART owner: even an explicitly raw root diagnostic is retained with
ordering metadata in `/log/queen.log` and must not bypass the reciprocal ring.
Diagnose with the retained queen log and later terminal or stage records, while
keeping the raw UART capture as the boot source of truth for bytes that really
reached the wire. A post-cutover serial capture is no longer, by itself, a
complete driver-diagnostic ledger.
Descriptor replay, engine init, prerequisites, context replay, and retained
maintenance have virtual-counter deadlines; an eight-second engine envelope
covers the Linux-shaped one-second ALP and three-second Function 2 waits plus
bounded handoff margin. Repeating one stage beyond that envelope without a
terminal failure/recovery record is failed liveness evidence.

Backplane attach advances through a retained generation-bound ALP/window cursor,
not a synchronous private loop. One outer EventPump turn may perform one exact
ALP request or read, one retained deadline observation, one FORCE_ALP, one
exact Function 1 pull-up-clear CMD52, one window-register CMD52, one ChipCommon
CMD53, one continuation grant, or one completion poll. A terminal child or
deadline poll records the result and returns; the next attach action requires a
later turn. Each non-ready ALP read
checkpoints the one-second absolute deadline, last `CHIPCLKCSR` value, and poll
count, then closes that foreground transaction. Thus a long, still-within-
deadline ALP wait does not accumulate against the retained transaction's
1,024-action trace or use trace exhaustion as a timeout.

The preceding card adoption and Function 1 enable are retained by the same
rule. CCCR revision, capabilities, `SPEED`, and `IF` operations, host-clock and
host-width changes, `IOEx` read, one-shot `IOEx.F1` write, each `IORx` read,
and each deadline observation are separate outer turns. Missing `CAP_SMB`, an
illegal low-speed/four-bit contract, issued-unknown work, or stale ownership
invalidates transport readiness and requires typed pair recovery before any
prior edge can be considered again. For every issued CMD5/CMD52/CMD53
completion, the SDIO owner consumes one immutable interrupt snapshot,
W1C-acknowledges its request-owned status bits while preserving `CARD_INT`,
waits for the BCM2835 ordered write settle, and only then reads the response
register once. A visible `RESPONSE` status edge never authorizes a pre-W1C
response read.

Firmware preparation repeats neither that initial trace nor the initial
`FORCE_ALP` policy. It uses a second request- and generation-bound cursor with
the Linux order `ARMCR4/D11 passive -> KSO -> CARDCTRL.WLANRESET ->
PMUCONTROL.RES_RELOAD -> IOEx.F2=0 -> CHIPCLKCSR=0 -> ALP_AVAIL_REQ ->
SoCRAM/upload preparation`. “Passive” is not the same reset state for both
cores: before the first firmware CMD53, ARMCR4 completes a reset cycle and is
left reset-deasserted with `CPUHALT|CLK`, making its TCM available for download;
D11 remains reset asserted for firmware to enable. LOW/MID/HIGH window writes,
each IOCTRL/RESETCTRL write, each flush/readback, retained settle, KSO action,
CARDCTRL action, the PMU word read, the PMU word write, and Function 2 disable
are separate outer turns even when the child completes immediately. The zero
write is the required `CLK_SDONLY` edge
before the asynchronous firmware-download ALP request. Each zero write, ALP
request, ALP read, retained five-millisecond virtual-counter settle, and
one-second absolute-deadline observation consumes its own outer EventPump turn.
PMUCONTROL preserves Linux's little-endian `readl()`/modify/`writel()` semantics
with one incrementing four-byte Function 1 CMD53 read followed by one
incrementing four-byte Function 1 CMD53 write at the backplane-word address
`0x8600`. Each child is sealed as retained PIO by its normalized one-block
geometry and moves its complete four-byte host block in one post-issue
ready-edge owner quantum. The read and write remain separate immutable child
requests under the generation-owned sequencer. There is no bytewise CMD52
update, alternate address, engine switch, fallback, or same-generation replay.
The cursor checkpoints after every completed phase, so unavailable ALP cannot
consume the 1,024-entry foreground trace. Production timing permits about 200
five-millisecond reads inside the absolute one-second window; a separate
extended-deadline host stress may exceed 1,024 reads only to prove checkpoint
capacity. Exact fault `0x5337` identifies the SD-only write, stays at Gate 5,
and leaves only through the canonical pair transaction and a replacement
completed physical lifetime. Duplicate or failed preparation in the discarded
lifetime is not re-primed.

Firmware release is also an ordinary EventPump continuation, not a private
driver call chain. Its retained cursor orders stale interrupt clear, optional
reset vector, ARMCR4 disable/assert and bounded RESETCTRL release, the
20-millisecond SD-only fence, one-second paced HT wait, `FORCE_HT`, mailbox
version, one-shot Function 2 `IOEx`, three-second `IORx` wait, Function 2
configuration, one-second firmware-mailbox wait, final interrupt arm, and DPC
activation. The 51 ARM attempts, 200 HT polls, 3,000 Function 2 fallback polls,
and 1,000 mailbox fallback polls each advance through distinct outer turns and
checkpoint before the next phase. They therefore cannot overflow the
foreground trace or retained-deadline table. A poisoned or stale generation
performs no same-command replay; ARM execution and firmware release are
published only after their exact irreversible child completions.

Typed control and EAPOL/data transmission uses the generation-local backplane
window cache established by the serialized CYW43/DPC owner. A normal cache hit
submits the single Function 2 CMD53 child in the same parent invocation. A
genuine cache miss retains `Pending` across exactly LOW/MID/HIGH CMD52 children
and then F2, one child per outer turn. It never samples `IORx` per packet:
Function 2 readiness is proved once during release and again only through the
existing recovery/re-enumeration lane. A pending cold-window child cannot
report terminal `Idle`.

After Function 2 becomes ready, the current source expresses the pinned Linux
BCM43455 post-F2 lifecycle as separately retained operations. The exact
sequence is `HOSTINTMASK`, Cohesix's separate Gate 10 `FUNCTIONINTMASK` phase,
watermark, `DEVICE_CTL` read/modify/write adding `F2WM`, `MESBUSYCTRL`,
`WAKEUPCTRL` read/modify/write adding `HTWAIT`, `CARDCAP`, and exact
`FORCE_HT`. Reprime repeats the masks as distinct operations, samples the low
and high frame-count bytes on separate turns, then admits the card interrupt as
three more retained turns: read CCCR `IENx`, write `current | 0x07`, and prove
the required bits by readback before DPC activation. Upper bits are preserved.
Exact fault `0x5339` rejects any failed access or bad readback and forces
generation recovery; steady RX never mutates `IENx` as a repair. No operation
shares an outer EventPump turn with the next one; a fresh pending re-entry may
consume only the cached completed prefix and cannot reissue it. Stale or
issued-unknown work poisons the generation instead of replaying an earlier
phase.

The retained DPC likewise performs SDIO-core interrupt-status W1C and firmware
mailbox ACK/NAK as one little-endian, incrementing, four-byte Function 1 CMD53,
not four bytewise CMD52 writes. This makes the word update one immutable child
action and removes the partial-byte window in which a newly arriving cause
could be cleared inconsistently. Controller-seam host tests prove the command
shape, exact payload, racing-cause preservation, Linux register order,
bit-preserving read-modify-write, one issue per phase, and cached-prefix
re-entry without controller reissue. These changes remain source and
hardware-free evidence until a newly built, read-back image reaches them on a
fresh Pi boot.

The reciprocal SDIO command owns the host as soon as its sequence-last ring
publication is durable, not only after the SDIO runtime constructs its retained
cursor. A real IRQ in that publication-to-admission interval may only latch the
owed acknowledgement. It cannot acknowledge or rearm the IRQ, change SDHCI
interrupt policy, or publish a DPC event beside the pending command. The exact
retained owner admits that command and consumes the latch in order. This is the
linked-runtime equivalent of Linux serializing request, threaded IRQ, and DMA
work through one MMC host sequencer.

The CYW43 parent descriptor and returned runtime frame intentionally share the
fixed ring frame window. Descriptor bytes are immutable and fingerprinted
through preparation, but become runtime-owned output storage after issue.
During HAL priority restoration, root therefore validates continuation from the
generation-bound root ticket, exact request/command identity, and HAL's stored
fingerprint of the original staged bytes; it must not re-decode returned
SDPCM/BCDC bytes as the old descriptor. A typed retained `Pending` or
`Complete` result proves that complete identity. A typed failure with any
surviving retained lease requires ordered pair restart and cannot become a
same-generation replay.

The retry decision is made at the retained parent boundary, not inferred from
one nested child in isolation. Only a stage-1 `0x5103` entry inhibit on a
single-action firmware/NVRAM word or chunk, control/data frame, or poll may
reuse the same immutable parent ticket. Transport init, firmware preparation,
release, and op11 control exchange are composite: any transport terminal fences
the generation and enters ordered pair recovery even if the last nested child
was inhibited before issue. Optional scan/filter/offload policy may skip only a
well-formed firmware `UNSUPPORTED` or `BADARG` reply. It must neither swallow
nor suppress the causal trace for an SDIO transport fault.

Serial evidence should name the exact frontier rather than leaving the generic
backplane marker as an ALP diagnosis: ALP request, ALP poll, FORCE_ALP, its
65-microsecond settle, the Pi extra-pull-up clear, ChipCommon read, and each
LOW/MID/HIGH backplane-window CMD52 have separate progress markers. Before the
SDIO engine starts, HAL must configure GPIO34-GPIO39 as ALT3 with CLK pull-none
and CMD/DAT0-DAT3 pull-up using BCM2711 register value `1`, then read back every
selected field. Missing or mismatched readback blocks Wi-Fi before its first
runtime operation. Production then emits `BACKPLANE_PULLUP_CLEAR` and issues one
immutable Function 1 descriptor for `SBSDIO_FUNC1_SDIOPULLUP=0`, matching
Linux's ordering after the 65-microsecond FORCE_ALP settle. The earlier physical
clear trial preceded deterministic host pinctrl and was therefore confounded;
commit `7328bedd6142` proves both pinctrl and the exact card-side clear complete,
yet the first firmware CMD53 still stops after two blocks. That result removes
the missing pull-up clear as the active Gate 6 explanation. Only an exact
completion may advance. Any issued-unknown,
`OWNER_PATH_POISONED`, stale, failed, or malformed result poisons the generation
and requests the ordered pair restart; host quiescence is not permission to
continue or retry in place.
For an op11 control exchange, diagnostics may claim
`edge=post-function2-tx` and
`child_cmd53=completed-before-reply-wait` only from a typed WaitReply result.
An untyped parent no-reply reports `edge=completion-unknown`,
`function2_tx=not-proven`, and `child_cmd53=not-proven`; agreement between a
stale progress breadcrumb and the parent request is not exact child-completion
proof.
Card selection uses CMD7 with the SDIO R1b short-busy response. Only the entry
inhibit wait before `SDHCI_COMMAND` is written is retryable; a busy timeout after
issue is retained as post-issue quiescence and cannot be replayed in-generation.

The retained CYW43 transaction keeps its full 1,024-action and 128 KiB payload
capacity in loader-zeroed `SHT_NOBITS` pages; the HAL loader must zero those
pages before copying file-backed bytes. That trace remains a bounded cache for
one straight-line retained substep, not an elapsed-time or ALP-poll budget. The
full baseline is an explicitly invalid `MaybeUninit` slot in the same
loader-zeroed storage and becomes readable only when exact parent admission
snapshots the live CYW43 state and release-publishes validity. Avoiding a second
file-backed nonzero state image changes packaging size without changing the
runtime aperture, replay capacity, or state semantics.

Initial bootstrap is exactly one finite outer boot episode, always
`attempt=1`. It emits `status=begin`, may emit `status=stabilizing` and one
consumed-once `status=recovery`, and ends in `status=ready`, `status=failed`, or
`status=permanent`; it never emits `status=backoff`, never resets into another
`status=begin`, and never admits attempts 2 through 5. This differs from both
the retired Cohesix five-attempt supervisor and Linux's gate-local
`BRCMF_SDIO_MAX_ACCESS_ERRORS` budget: neither is authority to retry the
complete production bootstrap. Fallible supervisor construction or immutable
configuration/artifact validation may instead emit
`attempt=1 status=permanent` before `begin`; that is the sole terminal result,
not another boot path.

The sole boot episode may consume at most one ordered full CYW43/SDIO pair
repair. Descriptor/engine/context replay completion alone cannot reset that
bound or renew the one absolute Gate 8 deadline. A recurring pre-ready
transport fault after the repair is spent fails with
`cyw43-pair-recovery-limit`; other non-Gate8 retryable terminal bootstrap faults
emit one `status=failed`. Gate 8 logical failure instead remains pending to its
absolute deadline and then emits `CYW43_GATE8_TERMINAL ... action=quarantine`
plus `status=permanent`, without pair repair. A lease conflict before issue
that made no scheduler change fails locally; issued or scheduler-mutating
uncertainty consumes the ordered repair. Terminal bootstrap performs no
automatic next child operation, whole-bootstrap reset, or pair repair. The
supervisor quarantines network service and returns to the ordinary EventPump
with Wi-Fi acceptance red. An
already-attached stack remains available to passive
diagnostics only through immutable terminal and owner-ring evidence; its
retained live DHCP/EAPOL/TCP status is stale and must not be read or allowed to
supersede the terminal gate. The poisoned CYW43 generation receives no status,
poll, or TCP-flush turn. Buffered TCP commands are not dispatched, and
quarantine ends the network-origin session and its stream/cursor authority
locally before serial commands resume. A non-retryable attached recovery
failure, including missing ready-generation proof, must emit one permanent
terminal status, apply the same network quarantine, and enter the same ordinary
operator mode rather than repeating bootstrap-only turns. Paced serial and
local-seat commands remain dispatchable: `netstats`, `nettest`, `wifi diag`,
`wifi probe-ht`, `usb diag`, `usb probe-kbd`, `smp`, authentication,
and `reboot` must return their documented result or a typed unavailable/fenced
error rather than being swallowed by the failed bootstrap. In the ordinary
linked-runtime rotation, quarantine consumes neither the CYW43 root-wake
notification nor a NIC turn; it skips directly to one bounded independent HDMI
`Display` turn when local-seat is attached, then returns to `Serial`. Only
exact-generation Gate 8 commit, bound DHCP address, and the admitted TCP
listener closes bootstrap and authorizes a later independently signalled
steady-state runtime-recovery episode. That episode has one consumed-once pair
repair, uses the same canonical owner-first lane, and cannot alter the recorded
boot result or publish another boot attempt. Gate 10 remains required
data-service acceptance. Production hardware acceptance requires the canonical
cold pair transaction and all later Wi-Fi gates to complete in the sole
attempt-1 boot episode.

The high-impact supervisor `preflight`, `begin`, `recovery`, `stabilizing`,
`ready`, `failed`, and `permanent` records declare
`recovery=full telemetry_sinks=serial+qlog+hdmi`; `full` names the configured
fail-closed pair-recovery policy rather than claiming a restart already ran,
and `qlog` names `/log/queen.log`. Attempt-zero `status=preflight` records
linked-serial readiness without consuming a Wi-Fi attempt. A typed failure line
immediately before generic `status=permanent` retains the terminal reason. Every
supervisor line fits the fixed 256-byte serial record at its numeric maxima.
They are queued only after the Wi-Fi HAL guard is released. Serial and
`/log/queen.log` retain the transition
immediately; a fixed episode-sized FIFO retains a complete worst-case episode
without overwriting delayed milestones, and the isolated HDMI runtime mirrors
at most one entry during each later ordinary `Display` turn. A trace with a
second boot `begin`, any `backoff`, `attempt>1`, a second recovery before the
active episode re-earns exact service readiness, a recovery that renews the
Gate 8 deadline, a same-turn HDMI submit, a lost queued terminal status, an attached
network poll after a terminal status, or an unresponsive prompt after
`status=failed` or `status=permanent` fails the software liveness contract even
before Wi-Fi RF acceptance is considered.

USB retained polling is likewise typed. A `Pending` prepare, boost, commit,
endpoint-wake, or completion-poll phase is progress, not a no-reply event: it
must preserve the ticket, command-ready state, and counters. Only terminal
`Failed` may revoke readiness and add no-reply debt. Sustained `usb status`
evidence with normal input but no-reply growth proportional to retained phase
count is a software regression rather than a keyboard fault. First-report or
command-ready service debt with no decoded or buffered byte earns one bounded
`LocalSeat` turn but is not physical input pressure and cannot retain the
CYW43 post-Dispatch operator fence. Only actual buffered input or a physical
response has that authority. A retained USB, serial, or HDMI lease fault is
device-local: pre-issue failure clears that request and issued-unknown failure
poisons it, but neither may request CYW43/SDIO pair recovery. The CYW43/SDIO
pair epoch is consulted only for those two contracts;
serial, USB, HDMI, PCIe, and GENET retain contract-local transport identity and
cannot be invalidated by a Wi-Fi restart. Linked serial RX, staged TX, and
transmitter-idle probes consume typed `Pending`, `Complete`, and `Failed`
outcomes. Terminal `Failed` poisons the serial transport once and is neither
ordinary TX backpressure nor permission to replay issued bytes.

If recovery occurs before the initial firmware bundle was admitted, the
ordered pair restart first acquires context-replay ownership. A later retained
turn then reacquires and validates the manifest-selected bundle through HAL,
checks the firmware reset vector, normalizes NVRAM into retained storage, and
publishes the recovery context before firmware streaming resumes. If that
admission fails, the supervisor releases context replay as unsuccessful and
reports the typed terminal failure; it must not use an empty, stale, or
root-supplied substitute bundle.

Before Wi-Fi supervision begins, physical-Pi serial cutover must have an
attached linked runtime plus a matching `Idle`, `Progress`, or `FrameReady`
service completion. That service completion proves the independent operator
transport; accepted `FrameReady` bytes separately prove RX input and need not be
present on an idle console. If linked serial service is unproved, the supervisor
stays retained at the ordinary root console, retries the service proof every
250 ms, and must report `serial=blocked`, not `serial=ready`; a transient first
miss must not disable Wi-Fi for the remainder of the boot. While a Wi-Fi HAL
scope is held, serial uses only that proved
linked route and local-seat handling consumes already-buffered bytes only; USB
backend polling, HDMI frame submission, and network polling remain fenced until
the scope is released. High-impact `CYW43_BOOTSTRAP_SUPERVISOR` lifecycle records
retain their exact machine-readable form on serial and in the bounded
`/log/queen.log` ledger. HDMI instead shows a concise `[drivers] WiFi ...`
semantic rendering on a later isolated-display turn; it must not render the raw
supervisor record. The `telemetry_sinks=serial+qlog+hdmi` field declares the
configured routing targets without requiring byte-identical presentation; it
is not proof that an unavailable or saturated display accepted the mirror.
Root-owned echo bookkeeping for already-buffered USB bytes is permitted while
the Wi-Fi HAL scope is held, but the resulting frame is submitted only on a
separate later Display turn. This keeps typed characters visible without
combining USB, HDMI, or CYW43 hardware operations.
The first physical frontier explicitly reports
`[drivers] WiFi starting one CYW43/SDIO physical lifetime`. An unchanged
frontier may append `(still working)` after five virtual-time seconds. This
message describes the sole owner lifetime under construction; it is not a
retry, a second boot attempt, or evidence that the lifetime has completed.
Material bootstrap, association, DHCP, and listener frontiers are coalesced,
while an unchanged frontier emits a `[drivers] WiFi ... (still working)`
heartbeat every five virtual-time seconds. Routine serial bootstrap telemetry
must not starve that independent display turn; authenticated serial response
tails retain priority and may delay it only until their bounded tail is
complete.
For the selected Wi-Fi/DHCP lane, supervisor `ready` is driver readiness only.
HDMI may render `Ready to use` only after current-generation DHCP is bound and
the TCP console listener is bound, non-deferred, and admitted. Listener
readiness is intentionally distinct from end-to-end `tcp_ready`, which
additionally requires accepted or authenticated physical data-path proof. The
HDMI message is obvious operator availability, not raw TCP or
authenticated-`cohsh` acceptance evidence. Diagnostic prompts may remain
available after `failed` or `permanent`, but those states must never render
`Ready to use`.
The serial record receives bounded physical-console priority: a terminal
`ready`, `failed`, or `permanent` record may
evict only an older nonterminal background record, never an `ACK`/`ERR`/`END`,
prompt, or in-progress command line. The typed `[net-console] deferred failed
detail=...` record immediately preceding generic `permanent` shares the
retained serial class. Other nonterminal detail/result and sparse turn records
remain best-effort under saturation. An accepted physical-console record
flushes on a later operator turn. Every
root raw-UART helper must also route only to `/log/queen.log` after cutover. A
raw-UART breadcrumb that bypasses the linked runtime is failed
operator-ownership evidence, while an absent required linked-serial line is
incomplete capture evidence rather than proof that its transition never
occurred.

After the prompt, the ordinary physical EventPump advances this same cutover
for every physical network selection, including wired GENET. WiFi supervision
is not the sole migration trigger. If attach fails, the emergency root console
remains available and ordinary USB, display, and network service continue; a
successful cutover still leaves exactly one physical UART owner. The linked
serial runtime samples the live mini-UART RX level on every admitted service
turn even if its bound notification coalesced. Its one shared hardware-byte
grant bounds pre- and post-service sampling; when exhausted it reads only line
status and leaves the masked IRQ pending for a later turn.

Linked serial output is exact-ticket retained work after cutover. Each CYW43
bootstrap/recovery operator turn may send or poll only the current immutable
serial command, with no queue-tail restoration after an unknown result. A valid
partial completion advances only the written prefix; its FIFO suffix receives a
new action ticket. TX is limited to 128 bytes per action and alternates with RX
after every completed chunk, so large startup output cannot hide a paced serial
command. The ordinary linked EventPump uses the fixed `Serial`, `LocalSeat`,
`Dispatch`, `Network`, and `Display` outer-turn order. `Serial` admits one
TX-first serial-ring turn. `LocalSeat` then performs one retained USB keyboard
turn, so new physical input is buffered before network weighting begins.
First-report or command-ready service debt may request that one turn, but it is
not physical input without a decoded or buffered byte. `Dispatch` consumes at
most one serial, buffered local-seat, or already-buffered network command
without polling the NIC or flushing TCP. Each `Network` turn
performs one NIC service and leaves any received command buffered for a later
`Dispatch` turn. For wired GENET, that service may instead be one retained
post-command TCP flush; polling, flushing, and command dispatch remain separate
outer turns. GENET and idle CYW43 service follow the ordinary rotation.

The selected CYW43 path never uses the GENET cursor. Its root-wake notification
is edge urgency only; a passive current-connection-generation, pair-epoch, and
completed-physical-lifetime snapshot is the durable Network-resume authority.
Healthy DPC/root wake, pre-poll, runtime/root queue, control-reply, data/ARP TX,
exact descriptor/HAL owner, logical control/terminal drain, EAPOL, prompt,
maintenance, and recovery obligations retain that level
until an exact same-identity idle snapshot. Physical-lifetime, pair, or
generation change, quarantine, reboot, or another selected NIC invalidates it.
It may therefore retain `Network` for an exact current DPC/RX/TX continuation,
a proved root-wake level, a nonempty runtime/root queue, or actual TCP socket,
parser, or response work after the one-shot edge cursor has already been
consumed. The identity-only lifetime cursor is permission rather than work
demand, and an authenticated but idle socket is not a weighting reason. A
current root wake or live DPC producer level may schedule one fresh
queue/tail-drain op8 with flags exactly `RX_STEADY_TAIL_DRAIN`; neither adds
`RX_HINTLESS_FIRSTREAD`. DPC also keeps the persistent child DPC owner urgent;
that owner performs source inspection and bounded drain, then rechecks the
durable condition before sleep and rearm. Only one genuinely due 32-ms
lost-edge watchdog may schedule a fresh hintless NetData op8. An
already-retained queue-only op8 remains immutable, so the due audit
waits for a later unowned turn. Bootstrap, split-control/host-EAPOL, control
pre-TX, and the Join-only late-source fence retain their separately bounded
hintless probes.

An active DPC cursor or SDIO child does not block one exact software-only queue
delivery already sealed at runtime intake. A strict current-generation,
non-hintless op8 or op10 may pop a proven nonempty DPC-owned RX queue without
touching Function 1, Function 2, the reciprocal SDIO owner, or the retained DPC
cursor/child. Empty, stale, mismatched, unsealed, physical-capable,
control-exchange, recovery, and quarantine cases remain serialized. The
production EventPump permit and CYW43 pre-poll bound both remain one, admitting
at most one root CYW43 runtime/HAL operation per ordinary turn.

For the warmed RX cadence path, a stricter `RX_POLL` shortcut runs before the
complete 140-KiB foreground rollback snapshot. It requires the exact
intake-sealed inactive parent, flags exactly `RX_STEADY_TAIL_DRAIN`, a healthy
same-generation owner ring and linked runtime, a compatible prompt, and a
proven nonempty private RX queue with no terminal, control, recovery, restart,
issued-unknown, or quarantine state. It uses only the sealed descriptor and
existing software queue completion, preserves the DPC cursor/child and root
wake, and clears only the exact seal after the terminal. `CONTROL_POLL`,
hintless, empty, stale, unhealthy, or mismatched input retains the ordinary
snapshot/rollback path. This is a source-level cadence optimization until the
rebuilt exact image passes the fresh hardware thresholds below. The unflashed
v11 counters must rank its rollback-cost effect against `DPC_ACTIVATE`,
event/DPC/root delivery, and interrupt-cause amplification.

The existing accounting line retains its historical `captures` key for old
capture tooling. The immediately following scope line is normative:

```text
CYW43_SDIO_DPC_SCOPE captures=event-attempts published=ring-events source=card-int-or-source-probe physical_card_irq=not-exported
```

`captures` is `ring.producer + ring.overruns`; it includes both hardware
CARD_INT events and software-authorized SOURCE_PENDING probes and must not be
reported as a physical interrupt count. The fixed v2 ring exports no cumulative
physical CARD_INT counter.

For the current 252-byte v11 trace, retain the fifth passive `wifi diag` line
after the byte-stable 196-byte v10 prefix:

```text
CYW43_SDIO_DPC_CAUSE samples=<n> frm=<n> hm=<n> fcc=<n> fcs=<n> ca=<n> other=<n> spur=<n> done=<n> dpc=<n> child=<n> owner=<n> fdpc=<n> fown=<n>
```

One `samples` episode is counted for each exact initial SDIO interrupt-status
capture before the W1C ownership mask is applied. Cause counters may overlap:
`frm`, `hm`, `fcc`, `fcs`, and `ca` are FRAME, HOSTMAIL, FC_CHANGE, FC_STATE,
and CHIPACTIVE. `other` records any nonzero raw bit outside those classes and
may overlap a known cause; `spur` advances only when the complete raw initial
status is zero. `dpc` counts event-associated CYW43 DPC turns, `child` counts
distinct SDIO child submissions, and `owner` counts the initial and fresh-grant
owner quanta issued. `done` counts completed DPC-admitted frames. `fdpc` and
`fown` accumulate only at each completed-frame boundary, so dividing either by
`done` excludes work after the newest frame. These ratios diagnose
amplification; they neither prove live packet service nor change owner
admission.

Record the four additive Wi-Fi TX lines from both `netstats` and
`wifi diag`:

```text
netstats: wifi_tx_phase_counts gen=<n> accepted=<n> issued=<n> terminals=<n> credits=<n> next_issues=<n>
netstats: wifi_tx_phase gen=<n> us=n/last/max/avg a2i=<n>/<last>/<max>/<avg> t2c=<n>/<last>/<max>/<avg> c2i=<n>/<last>/<max>/<avg>
netstats: wifi_tx_phase_i2t gen=<n> us=n/last/max/avg i2t=<n>/<last>/<max>/<avg>
netstats: wifi_tx_queue gen=<n> depth=<n> reserved=<n> hwm=<n> drops=<n> stale_purged=<n>
```

`wifi diag` uses the equivalent `wifi: tx_phase_counts` and
`wifi: tx_phase`, `wifi: tx_phase_i2t`, and `wifi: tx_queue` prefixes. The
tracker resets on logical connection generation, deduplicates immutable
tickets, and scales from the generated virtual-counter frequency. `a2i` is
TxToken acceptance, including FIFO wait, to first observed op7 issue; `i2t` is
issue to typed terminal; and `t2c` is successful terminal to SDPCM-credit
proof. `next_issues` and the `credit_to_next_issue` metric, printed compactly as
`c2i`, mean credit proof to the next actual op7 issue, not acceptance of a new
TxToken or an earlier local FIFO-head promotion. High
`a2i`, `i2t`, and `t2c` respectively implicate queued/root
transport, issued runtime/SDIO service, and firmware credit return. Queue depth,
reservations, HWM, drops, and stale purges expose bounded ingress pressure.
These lines are passive and must be absent from GENET output.

One continuation quantum has the compiler-declared CYW43
`max_ops_per_turn` hard bound (currently 192 separately opened outer turns).
A 25-ms seL4 virtual-counter cap separately fences admission of a fresh
physical parent. A separate 25-ms virtual-counter clock bounds elapsed time
between operator checkpoints. On expiry it completes
`Serial -> LocalSeat -> Dispatch`, leaves at most one `Display` turn pending,
submits no NIC operation, preserves the exact CYW43 parent and pair-priority
lease, and resets only after `Dispatch` before resuming the same quantum.
Operation count alone never triggers this checkpoint. A fresh CYW43 wake
latches admission without rewriting any already-scheduled console phase. A
complete TCP command or pending actual physical response/buffered input is
likewise an immediate typed exit into a fairness fence. That fence preserves
unfinished Wi-Fi work but requires `Serial`,
optional `LocalSeat`, and `Dispatch` to receive one bounded turn before Network
resumes. If Dispatch queues HDMI echo while a partial command retains that
fence, one Display turn runs before the next Serial turn unless a reboot
acknowledgement or physical response tail already owns Serial. The echo remains
pending in that case. Display does not close, replace, or service the retained
CYW43 parent. Reboot or quarantine invalidates the Wi-Fi-only scheduler state. An
elapsed quantum with no exact parent admits no fresh NIC/SDIO parent. An exact
already-`Prepared` or already-`Issued` parent continues despite elapsed time
until a typed terminal, physical/dispatch yield, or the hard 192-turn bound;
the next slice may resume only that identity. Raw DPC and retained owner work
remain eligible before authentication. The passive snapshot changes scheduling
weight only, never issues or completes child work, and rejects stale-epoch,
poisoned, overrun, acknowledgement-failed, or inconsistent DPC state. Every
retained turn still admits at most one CYW43 operation. The first actionable
turn opens one current-pair priority lease, boosts SDIO then CYW43, and exact
parents reuse it until close. Close is a fresh-work fence; it drains only an
exact active parent and restores CYW43 then SDIO before returning to `Serial`.

The USB keyboard runtime keeps one interrupt-IN transfer active for the whole
endpoint lifetime and rearms one successor after each completion.
`queued_reports=1` is therefore the healthy armed state before and after the
first HID report; larger values are an invariant failure, not throughput
headroom. Gate 10 may show command readiness without a keypress, but live
local-seat acceptance still requires the same linked-runtime HID byte at parser
ingress, `physical_input_proven=yes`, and HDMI echo from a printable key.
Before the first transfer event or valid report, a successful doorbell arms an
exact slot/endpoint/report-slot/TRB/generation liveness watch for five seconds
using the selected virtual counter, with a bounded 4,096-poll fallback only
when counter timing is unavailable. Repeated service of the same identity does
not refresh that deadline. If it expires while the endpoint is ready, the same
one-deep transfer remains active, and no preserved event or pending doorbell
accounts for the silence, the runtime fails the attach closed as
`FULL_QUEUE_NO_EVENT`. Any transfer event or valid report clears the watch;
the one armed successor after first-report readiness remains untimed during
ordinary idle. This is bounded failure containment, not live typing proof.

A retained host-EAPOL TX/key/drain action, including a request-less
post-secure M4 op7, blocks every fresh generic NetData pre-poll at both the
outer stack wrapper and inner budgeted service. This is owner precedence, not a
second data-path block: an exact already-assigned NetData continuation remains
non-revocable, then the EAPOL owner receives the next unowned turn. A repeated
op8 while that M4 stays at sequence zero is failed service evidence.

The central EventPump `network_contract_service_admissible` fence covers both
ordinary `poll_runtime` and pre-root `poll_pre_root_network`. It rechecks the
CYW43 service snapshot before Network service and again immediately before
either a NIC poll or retained TCP flush. A missing, active, failed, or replaced
physical epoch, or a recovery-active snapshot, clears Wi-Fi scheduling tokens
and permits neither operation. The fence is CYW43-specific and leaves GENET
behavior unchanged.

Run `netstats` after WiFi readiness and again after the sequential `.coh`
sample. Alongside the existing quantum records, selected WiFi must emit:

```text
netstats: cyw43_quantum runs=<n> turns=<n> max_turns=<n> max_elapsed_us=<n> operator_yields=<n> checkpoint_ms=25
netstats: proof_policy m26d_net_first=no physical_input_yield=enabled
netstats: cyw43_priority_lease state=<inactive|acquiring|open|closing|restoring|poisoned> pair_epoch=<n> active=<yes|no> close_pending=<yes|no>
netstats: cyw43_priority_lease_counts opens=<n> closes=<n> restores=<n> recovery_revocations=<n> amortized_requests=<n> failures=<n>
```

At a quiescent prompt, accept only the exact policy
`m26d_net_first=no physical_input_yield=enabled`, `state=inactive active=no
close_pending=no` and `failures=0`, equal `opens`, `closes`, and `restores`, and
nonzero `amortized_requests` after steady traffic. A nonzero
`recovery_revocations` requires the exact contemporaneous typed pair-recovery
record; a poisoned, active, closing, or restoring terminal sample is not ready
for latency or pressure proof. Repeated steady parents should increase
`amortized_requests` without requiring one open/restore pair per parent.
`netstats` also exposes quantum counts, turns, maximum turns/duration,
`operator_yields`, and idle, turn-cap, time-cap, physical, and guard exits.
`operator_yields` counts bounded physical-console checkpoints
(`Serial -> LocalSeat -> Dispatch -> pending Display`) and may be nonzero only
on selected CYW43. `checkpoint_ms=25` is the elapsed-time cadence; no
network-operation count is a second yield trigger. Those counters remain zero
for GENET, and GENET omits the WiFi-only priority-lease records. `Display`
performs at most one retained HDMI attach or pending-frame turn. Pending input
echo takes that Display turn immediately after Dispatch even when a partial
command retains the operator fence; Display returns to Serial without admitting
Network. Without a local seat, the rotation skips directly from `Serial` to
`Dispatch` and from `Network` back to `Serial`.

The TCP console retains one parser/authentication/session authority. After its
active socket enters the sole `Draining`/`PeerCloseWait`/`Closing` lane, one
standby smoltcp acceptor may buffer one unauthenticated peer but performs no
console parsing or authentication. For `QUIT`, Cohesix first drains `OK QUIT`
and the TCP send queue, then gives the peer one second to send FIN. Peer FIN
moves the socket to `CloseWait`, after which Cohesix sends its FIN through the
ordinary `LastAck`-to-`Closed` path. If the peer remains `Established` when the
one-second grace expires, Cohesix aborts the active socket instead of issuing a
local FIN from that state. The active path retains its 10-second drain and
10-second close bounds around that grace, and the pending standby handoff uses
the matching 21-second deadline. Promotion requires the old socket to be
terminal and all old session, client, peer, inbound, and outbound state to be
clear. Early
FIN/RST or another non-promotable standby state is aborted and recycled. A
network-generation or stack reset aborts the pair. CYW43 and GENET use this same
bounded handoff.

If a TX command already owns the shared ring slot, RX remains unallocated and
cannot install a competing ticket or fingerprint. If the lifetime watchdog is
`watchdog-due` while that immutable op7 is retained, the existing op7 advances
first on successive outer Network turns. Its terminal consumes its turn; the
still-due audit may claim one fresh hintless op8 only on a following unowned
turn. A current DPC producer level or root wake may schedule only a fresh
queue/tail-drain `RX_STEADY_TAIL_DRAIN` op8; neither may add
`RX_HINTLESS_FIRSTREAD`. Treat an
op8 that repeatedly runs ahead of an active op7, or op7 identity that changes
across those turns, as failed single-lifetime evidence rather than a recoverable
RX/TX race. CYW43 likewise retains a
copied RX frame instead of exposing smoltcp's paired response token while a
prior retained TX or unproved credit window would reject that token. When the
bounded linked TX queue is full, complete output remains retained. Three backlog records are
reserved for response tails; ordinary `Line` and nonessential `BackgroundLine`
records cannot consume them. A response-priority enqueue may preempt only the
newest `BackgroundLine`, whose authoritative copy is already in
`/log/queen.log`; it cannot drop command output or an existing tail. The stream
cursor and pending `END` do not advance on backpressure, and the prompt is a
retained `ResponseTailPrompt` backlog record, not a separate one-bit slot.
Physical-console command intake remains fenced until the current serial bytes,
backlog records, and response barrier have drained. A later `Serial` turn
retries one retained record; queue saturation must not silently discard or
truncate it. Repeated or reordered console banners, or a prompt that freezes
after otherwise successful DHCP, indicate failed linked-serial transport
evidence. A later numbered `CYW43_BOOTSTRAP_SUPERVISOR attempt=` record is
itself a production lifecycle defect, not an authorized Wi-Fi retry.
On a fresh boot with no authenticated session, use the paced serial
`attach queen <ticket>` flow before `reboot`; authentication remains available
during bootstrap because it touches only the parser and ticket table. All other
hardware diagnostics remain fenced until the retained Wi-Fi action releases
its HAL scope. Once reboot is accepted, every later command is fenced and reset
admission discards only nonessential `BackgroundLine` records already preserved
in `/log/queen.log`, never command output or protocol tails. Reset dispatch waits
for the complete ACK, all output ahead of it, and an explicit linked-runtime
UART transmitter-idle sample. FIFO acceptance is not wire-idle proof. A busy
sample completes only that immutable idle probe and preserves an RX fairness
turn before a fresh idle ticket. If the serial generation is poisoned or the
three-second virtual-counter drain deadline expires, the reset stays fenced and
`/log/queen.log` records `linked-serial-generation-poisoned` or
`ack-drain-timeout`; an empty queue after poison is not ACK proof. The turn that
first proves wire idle records the proof and returns. Platform reset occurs on a
later reset-only outer turn with no driver, network, local-seat, display, or
serial work.

Hardware-free closure is narrower: it requires the retained production
supervisor, one-child-operation EventPump permit, reciprocal-ring/controller
failure-cut tests, supervisor-only generation transitions from immutable
deferred-recovery records after steady-path guards unwind, preservation of an
unresolved association cursor across logical epoch changes, exact immutable
endpoint-rendezvous gating after every retained non-CYW43 root-command
`Pending` quantum, acknowledged sequence-last shared grants plus signal-last
notification hints,
separate root ABI-invisible `Stage`, `CommitRing`, `PublishGrant`, `NotifyRing`,
and completion `PollRing` handoffs, delegated `Poll -> Grant -> Poll` producer
parity, and distinct owner `CheckWake`, `CheckGrant`, and ACK-before-I/O
`Execute` turns admitting at most one physical owner quantum, authoritative
owner-generation rejection, and the one-pair-restart-per-attempt bound until
attached address/TCP readiness,
retained generation-bound ALP/backplane attach with one request, deadline poll,
CMD52/CMD53 child action, completion poll, explicit later grant/re-signal, or
exact-grant owner quantum per outer turn, terminal poll
separation, per-poll cursor checkpointing beyond the 1,024-action trace
capacity, exact window/ChipCommon progress, and exact one-shot pull-up-clear
completion with any fault poisoning the generation,
five-phase linked EventPump arbitration with distinct NIC-service and command
dispatch turns, retained GENET TCP response flushing with one operation per
later `Network` phase and connection fencing, ordinary CYW43 data-ready polls,
TX-first single-operation linked-serial service, one-turn USB
keyboard and HDMI attach/service cursors, sole linked-runtime UART ownership,
terminal-output retention, exact UART wire-idle reboot-ACK fencing followed by
a later reset-only turn, exact clean image identity, and all repository gates to
pass. Host tests cover the initial root CYW43 scheduling-edge loss path,
non-CYW43 endpoint rendezvous, root and delegated shared-grant
publication/acknowledgement/re-signal, exact-consumed predecessor waiting
without re-execution, stale/mutated/mismatched-consumed/wrong-generation grant
rejection, grant-id exhaustion,
real foreground and DPC owner cursors, recurring replay-fault termination,
pre-issue and issued live-epoch cuts that preserve the old request and `aux1`,
network-ready streak reset, peer/IRQ coalescing priority, contract-local
generation isolation, typed serial terminal failure, idle blocking,
raw-diagnostic routing, stream/prompt saturation, reboot command fencing, and
the serial TX/RX collision that previously froze the prompt. That result makes
the committed image ready for the strongest available Pi test, but it is not
evidence that the board associated, completed EAPOL,
obtained DHCP, answered ARP, carried raw TCP/authenticated `cohsh`, preserved
USB/local-seat liveness, or repeated reliably. Only the boot-paired 10-cold and
10-warm capture set below can close those physical claims.

After collecting the logs and independently reading the target image artifact
back from the mounted medium, run the fail-closed aggregate gate:

```bash
python3 scripts/pi4_wifi_repeatability.py \
  --cold-log out/pi4-evidence/cold.log \
  --warm-log out/pi4-evidence/warm.log \
  --staged-image out/pi4-sd/cohesix-image-arm-bcm2711 \
  --readback-image /Volumes/COHESIX/cohesix-image-arm-bcm2711 \
  --image-sha256 <independent-readback-sha256> \
  --image-identity-metadata out/pi4-sd/pi4-image-identity.json \
  --expected-image-identity-sha256 <independently-preserved-sidecar-sha256> \
  --capture-manifest out/pi4-evidence/capture-manifest.json \
  --expected-git-commit <exact-clean-40-hex-commit> \
  --expected-build-id <canonical-64-hex-build-id> \
  --build-marker '[BUILD] <exact-readback-marker>' \
  --output out/pi4-evidence/wifi-repeatability.json
```

`PASS` is an evidence verdict, not a substitute for preserving each serial log
and its boot-paired pcap. The staged source and independently read-back files
must be distinct paths and distinct open-file identities whose raw bytes hash
identically. Each must independently pass the normalized image-ID calculation,
legacy-image structure checks, and both U-Boot CRC checks. Reusing a log path or
the same raw boot-slice bytes in either class is rejected. The required capture
manifest uses `cohesix-pi4-wifi-capture-manifest/v2`; each counted slice has one
unique run ID, declared cold/warm class, serial-file and raw-slice hashes, one
distinct nonempty boot-paired pcap and its hash, the sealed image ID, clean Git
commit, canonical build ID, and capture epoch. The manifest also binds the
independently preserved SHA-256 of the exact identity sidecar, so a stale image,
sidecar, and log set cannot authenticate itself merely by agreeing internally.
A missing/unreadable artifact,
hash mismatch, unsealed/absent/duplicated/conflicting marker, incomplete or
non-ready bootstrap-supervisor record, any selected acceptance slice containing
`status=recovery` or `status=backoff`, any second boot `begin`, any
attempt-2-or-later record,
skip-only log, wired boot, failed boot slice, or any existing
Wi-Fi/driver/operator blocker fails the aggregate. Cold versus warm remains an
operator-recorded reset classification, so retain a per-run collection ledger
and power/reset evidence alongside the serial and pcap files.

### USB Keyboard and HDMI

Serial remains the recovery authority. HDMI is an independent display sink;
USB keyboard readiness requires isolated-runtime command and first-report
proof. A prompt displayed on HDMI does not prove keyboard input, and a USB
descriptor does not prove command readiness. `Ready to use` requires DHCP bound
plus TCP-listener readiness but does not prove the stronger end-to-end
`tcp_ready` predicate. Under load, preserve serial and
local-seat command liveness before nonessential mirroring or redraws.
After attach or endpoint recovery, decoded held-key/modifier traffic is health
telemetry only until a decoded all-zero release establishes a fresh baseline.
The runtime reports first-report pending during that guard, and root invalidates
stale first-report, first-byte, parser, and HDMI command-ready latches while
keeping the endpoint and ordinary serial/HDMI service available. A pending
first-report or command-ready proof is service demand only: EventPump grants one
bounded `LocalSeat` opportunity but must not report physical input, retain the
post-Dispatch CYW43 operator fence, or block the following Network turn unless
an actual decoded or buffered byte or physical response exists.

Before constructing the EventPump, Pi root completes the current synchronous
PCIe HAL prerequisite as local bookkeeping and authority setup for the retained
USB cursor. This does not create a root-owned steady USB backend or authorize a
later combined PCIe/USB turn; a missing proof leaves retained USB attach blocked
at the PCIe prerequisite. Once the pump is active, each USB descriptor replay,
init, enumeration/report poll, HDMI descriptor/attach step, and pending-frame
service issues one immutable linked-runtime action or polls one exact completion
per outer turn. HDMI attach and frame submit are always separate turns.

## Failure Routing

Route the first failed proof layer rather than patching a later symptom:

| First failed layer | Next source of truth |
| --- | --- |
| Build or generated drift | Build log, selected manifest, seL4 generated artifacts, `scripts/check-generated.sh`. |
| Flash or readback | Verified whole-disk identity, staged hashes, mounted media hashes, embedded marker. |
| Loader or root entry | [BOOT_REFERENCE.md](BOOT_REFERENCE.md), serial from power-on, selected U-Boot/seL4 artifacts. |
| Driver admission or owner state | [DRIVERS.md](DRIVERS.md), normalized driver counters, first timeout/fault, HAL resource proof. |
| Network attach | Current serial plus boot-paired pcap; distinguish policy, association, EAPOL, DHCP, ARP, and TCP. |
| `cohsh` | Raw TCP transcript and console grammar before REST or UI investigation. |
| Benchmark | [BENCHMARKS.md](BENCHMARKS.md); verify provenance and classify the moved layer. |

Keep failed evidence. A named, current-image blocker is more useful than a
later successful boot with no causal record.
