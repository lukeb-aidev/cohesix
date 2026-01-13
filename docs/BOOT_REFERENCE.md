<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Documents the reference Cohesix boot transcript and expected sequence. -->
<!-- Author: Lukas Bower -->
# Cohesix Boot Reference — AArch64/virt + PL011 (2025-11-27)

This document records the known-good bootstrap configuration for the Cohesix
root task running on upstream seL4 with QEMU's `aarch64/virt` platform and the
PL011 serial console.

## Reference transcript (trimmed)

A successful boot is expected to reach the Cohesix console with output matching
the following checkpoints:

```
Bootstrapping kernel
available phys memory regions: 1
  [40000000..80000000)
reserved virt address space regions: 3
  [ffffff8040000000..ffffff8040237000)
  [ffffff8040237000..ffffff8040238e11)
  [ffffff8040239000..ffffff80402f2000)
Booting all finished, dropped to user space
[INFO root_task::kernel] [boot] bootstrap.begin
[INFO root_task::bootstrap::cspace] [cnode] canonical alias uses init CNode slot=0x0002
[INFO root_task::kernel] [rt-fix] cspace window [0x010b..0x2000), initBits=13, initCNode=0x0002
[cohesix:root-task] [vspace:map] pl011 paddr=0x09000000 -> vaddr=0x00000000a0000000 attrs=UNCACHED OK
[console] PL011 console online
[cohesix:root-task] uart logger online
[INFO root_task::console] [console] starting root shell ep=0x010b uart=0x010f
Cohesix console ready
```

The interactive console provides exactly the commands documented in
`USERLAND_AND_CLI.md`: `help`, `bi`, `caps`, `mem`, `ping`, and `quit`
(`quit` remains unsupported on the root console).

## Bootstrap invariants

- **CSpace window**: The init CSpace uses `initBits = 13` with the free window
  `[0x010b..0x2000)` anchored at the kernel-advertised empty range. Slot
  validation avoids speculative `CNode_Copy` calls to keep kernel decode logs
  silent on the default path.
- **TCB ownership**: The root task intentionally uses the kernel-provided init
  TCB capability as the canonical root TCB. Bootstrap no longer attempts a
  self-copy, preventing spurious `Target slot invalid` decode warnings while
  keeping behaviour identical to the prior, working console.
- **Device page tables**: A reserved device page-table pool is carved from a
  64 KiB RAM-backed untyped (bits=16, capacity=65,536 bytes). Reservations now
  assert that at least one table is available before device mapping proceeds.
  Any device mapping failure (including `Untyped_Retype` exhaustion) is treated
  as a fatal error so future device additions fail loudly instead of silently
  losing the console.
- **PL011 mapping**: The PL011 MMIO frame is mapped through the reserved device
  page-table pool into the uncached device window; the mapping log above should
  appear during every boot.

## Forward requirement

This configuration and the accompanying logs represent the Cohesix
AArch64/virt + PL011 + root-console baseline as of **2025-11-27**. Future
changes must preserve these invariants and keep the default boot transcript
substantially consistent with the reference shown here.
