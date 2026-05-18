<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record Milestone 26b completion evidence for Pi 4 DHCP/Wi-Fi and QEMU compatibility. -->
<!-- Author: Lukas Bower -->

# Milestone 26b Completion Evidence

Milestone 26b is complete. Milestone 26c has not started.

## Scope

This evidence closes the Milestone 26b Pi 4 DHCP/Wi-Fi baseline and QEMU compatibility guardrail. It does not start the Milestone 26c target-qualified runner, audit refactor, or cleanup work.

## QEMU Evidence

- State directory: `out/test-plan/m26b-qemu-20260517T111933Z`
- Stage 01 integrity: `out/test-plan/m26b-qemu-20260517T111933Z/stage_01.done`
- Stage 02 host fast checks: `out/test-plan/m26b-qemu-20260517T111933Z/stage_02.done`
- Stage 03 QEMU TCP checks: `out/test-plan/m26b-qemu-20260517T111933Z/stage_03.done`
- Stage 04 REST checks: `out/test-plan/m26b-qemu-20260517T111933Z/stage_04.done`
- Stage 05 due diligence: `out/test-plan/m26b-qemu-20260517T111933Z/stage_05.done`
- Stage 05 audit log root: `out/audit/gate/20260517T192734Z`

The QEMU raw TCP build/run used the Milestone 26b-compatible command shape:

```sh
SEL4_BUILD_DIR=seL4/SMP_build ./scripts/cohesix-build-run.sh \
  --sel4-build "seL4/SMP_build" \
  --out-dir out/cohesix \
  --profile release \
  --root-task-features cohesix-dev \
  --cargo-target aarch64-unknown-none \
  --raw-qemu \
  --transport tcp
```

Stage 04 used a loopback `hive-gateway` against that raw QEMU TCP console and passed REST core, parity, and Python smoke checks. The macOS-only run marked the Linux FUSE mount regression as not applicable.

## Pi 4 Evidence

- Raw serial boot evidence: `/Users/lukasbower/pi4-serial-20260517-204318.log`
- Pi evidence directory: `out/test-plan/m26b-pi4-evidence-20260517T111457Z`
- Combined serial plus `cohsh` proof log: `out/test-plan/m26b-pi4-evidence-20260517T111457Z/pi4-combined-serial-cohsh.log`
- Pi image build/stage output: `out/pi4-sd`

The raw serial boot reached USB Gate 10 and Wi-Fi Gate 9. Final Wi-Fi Gate 10 proof is in the combined serial plus `cohsh` evidence because TCP `cohsh` output is not mirrored to UART.

Follow-up review of the same long-running raw serial capture found a post-proof
RX resilience bug: after the final remote `cohsh` proof, a malformed or
oversized RX-glom descriptor could leave the SDIO Function 2 runtime receive
path reporting `cyw43-frame-oversize` repeatedly. The completion evidence above
still proves DHCP, EAPOL security, `netstats`, and remote `cohsh`, but the
runtime fix now disables Pi 4 runtime RX glom after the Linux-shaped `mpc`
preinit point, clears/acks oversized Function 2 receive conditions, and caps the
corresponding Wi-Fi/self-test logs so the UART and local seat remain responsive.

Final `cohsh` post-nettest `netstats` proved the Pi 4 was DHCP-bound on Wi-Fi:

```text
netstats: mode=dhcp policy=wifi active=wifi standby=none addr_src=dhcp-lease ip=192.168.86.154 gateway=192.168.86.1 dhcp=bound
netstats: wifi_assoc=1 wifi_link=1 eapol_rx=2 eapol_start=0 eapol_secure=1
netstats: rx_pkts=934 tx_pkts=472 rx_used=934 tx_used=472 polls=19310
netstatus: ip=192.168.86.154 gateway=192.168.86.1 src=dhcp-lease dhcp=bound
nettest: backend=bcmgenet-v5 enabled=true running=false udp=192.168.86.154:31338 tcp=192.168.86.154:31339 last=Some(NetSelfTestResult { tx_ok: true, udp_echo_ok: false, tcp_ok: false, console_ok: true, peer_assisted_ok: true })
```

The combined gate summary command passed:

```sh
python3 scripts/pi4_trace_normalize.py \
  out/test-plan/m26b-pi4-evidence-20260517T111457Z/pi4-combined-serial-cohsh.log \
  --gate-summary \
  --expect USB_GATE=10 \
  --expect USB_BLOCKER=none \
  --expect WIFI_GATE=10 \
  --expect WIFI_BLOCKER=none \
  --expect WIFI_EXACT=none \
  --expect USB_BOOTLOADER_HANDOFF_SEEN=no \
  --expect USB_COLD_BOOT_SEEN=yes \
  --expect BOOT_HALTED=no \
  --expect TIMER_IRQ27_SEEN=no
```

Expected summary fields:

```text
USB_GATE=10
USB_BLOCKER=none
WIFI_GATE=10
WIFI_BLOCKER=none
WIFI_EXACT=none
BOOT_HALTED=no
TIMER_IRQ27_SEEN=no
USB_BOOTLOADER_HANDOFF_SEEN=no
USB_COLD_BOOT_SEEN=yes
```

`SERIAL_CLEAN=no` is expected only for the combined file because it intentionally includes non-UART `cohsh` transcript lines. The raw serial log remains the source of truth for boot-stream integrity.

## Acceptance

- Milestone 26a: Complete.
- Milestone 26b: Complete.
- Milestone 26c: Not Started.
