<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Documents the reference Cohesix boot transcript and expected sequence. -->
<!-- Author: Lukas Bower -->
# Cohesix Boot Reference — AArch64/virt + PL011 + virtio-net (2026-01-22)

This document records the QEMU `aarch64/virt` bootstrap reference for the Cohesix
root task with the PL011 serial console and virtio-net TCP console. It is a QEMU
development/CI reference, not Raspberry Pi 4 or UEFI hardware proof; Pi 4 boot
truth lives in `docs/HARDWARE_BRINGUP.md`.

## Reference transcript (trimmed)

A successful boot is expected to reach the root task, emit the manifest
summary, and bring up the TCP console. The transcript below is trimmed from a
known-good QEMU serial log. The literal hash and network line below are historical
for the cited run; current builds must compare their own manifest summary against
the generated `configs/generated/root_task_resolved.json` and current boot logs.

```
ELF-loader started on CPU: ARM Ltd. Cortex-A57 r1p0
ELF-loading image 'kernel' to 40000000
ELF-loading image 'rootserver' to 40239000
Booting all finished, dropped to user space
[kernel:entry] root-task entry reached
[MARK] boot_state=COLD
[bootinfo:cspace] root=0x0002 init_bits=13 empty=[0x01ef..0x2000)
[boot] allocator ready
[BUILD] 509f7639 2026-01-21T21:30:10.495334+00:00 features=[kernel:1 bootstrap-trace:1 serial-console:1 net:1 net-console:1]
[cohesix:root-task] Cohesix v0 (AArch64/virt)
[cohesix:root-task] manifest.schema=1.5
[cohesix:root-task] manifest.profile=virt-aarch64
[cohesix:root-task] manifest.sha256=61c0fcf26398e77b38f9ea82dc2f1a619bd3151de43f90acab748b9a7dc88435
[cohesix:root-task] manifest.secure9p.msize=8192
[cohesix:root-task] manifest.secure9p.walk_depth=8
[cohesix:root-task] manifest.features.net_console=true
[cohesix:root-task] event_pump.fds=serial,timer,ipc,net-console,ninedoor
[console] PL011 console online
[INFO root_task::net::stack] [net-console] config: iface_ip=10.0.2.15/24 gateway=10.0.2.2 listen_port=31337 udp_echo_port=31338 tcp_smoke_port=31339
[INFO root_task::drivers::virtio::net] [net-console] virtio-net ready: rx_buffers=16 tx_buffers=16 mac=52-55-00-d1-55-01
```

## Console access

- Serial (PL011) is the boot log transport.
- Interactive control uses the TCP console; connect with `cohsh` as described in
  `docs/QUICKSTART.md` and `docs/USERLAND_AND_CLI.md`.
- The TCP console expects `AUTH` then `ATTACH`, and replies with `OK/ERR/END`
  lines consistent with the shared console grammar.

## Bootstrap invariants

- **CSpace window**: The init CSpace uses `initBits = 13` with the free window
  `[0x01ef..0x2000)` anchored at the kernel-advertised empty range.
- **Boot markers**: `[kernel:entry] root-task entry reached` and
  `[MARK] boot_state=COLD` must precede the manifest summary and driver bring-up.
- **Manifest summary**: `manifest.schema`, `manifest.profile`, and
  `manifest.sha256` are logged at boot; the active run's hash must match the compiled
  manifest in `configs/generated/root_task_resolved.json`.
- **Secure9P bounds**: `manifest.secure9p.msize=8192` and
  `manifest.secure9p.walk_depth=8` must remain unchanged.
- **Console & event pump**: `event_pump.fds=serial,timer,ipc,net-console,ninedoor`
  and the net console config line should appear on every TCP-enabled boot.

## Forward requirement

This configuration and the accompanying logs represent a Cohesix
AArch64/virt + PL011 + virtio-net baseline as of **2026-01-22**. Future QEMU
changes must preserve the invariants above or update this reference with fresh
generated-manifest evidence.
