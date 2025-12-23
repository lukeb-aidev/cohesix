<!-- Author: Lukas Bower -->
# Root-task Debug Log Notes

## Retype Instrumentation Markers
- The root-task's `kernel` module emits lines beginning with `retype status=` whenever the extended retype tracing hook runs.
- The suffix `pending`, `ok`, or `err(<code>)` mirrors the internal `RetypeStatus` enum from `apps/root-task/src/kernel.rs`.
- `retype status=…` lines now emit `raw.*` field names. These values are captured **before** the trace is sanitised and therefore show exactly what the root-task attempted to hand to the kernel.
- A subsequent `retype.init_cnode …` line dumps the expected root CNode capability, traversal index (the destination slot for the init thread), and guard depth derived from bootinfo. This line makes it trivial to compare the raw submission against the canonical init-thread parameters. Each kernel submission also logs `Retype → root=initCNode index=0 depth=<initBits> offset=0x<slot> …` immediately before issuing `seL4_Untyped_Retype`, confirming the canonical tuple was used.
- The syscall gateway now emits `Retype DEST(root=0x… idx=0 depth=<initBits> off=0x… obj=<ty> sz=<bits>)` for every init-root retype, mirroring the ABI order `(root, node_index, node_depth, node_offset)` and making type/size mismatches obvious when they occur.
- When sanitisation succeeds you will see `retype.sanitised …` with the values that were ultimately passed into `seL4_Untyped_Retype`. If sanitisation fails the log prints `retype.sanitise_error=…` describing the first mismatch (root capability, node index, guard depth, or slot bounds).
- The accompanying `retype.kind=` line reports the `RetypeKind` variant, which is `device_page` for MMIO mappings such as the PL011 UART.
- Device coverage output like `device coverage idx=16 […] state=free` confirms the root-task examined the manifest entry for the requested MMIO region before the failure.

## Implication for Current Panic
- Because the trace shows `retype status=err(3)` directly before the panic, the extended debug path **did** execute.
- The failure line now spells out the symbolic error: `map_device(0x09000000) failed with seL4_InvalidArgument (3)`. This confirms the kernel rejected the destination CNode/slot while decoding the untyped invocation rather than skipping our instrumentation. The kernel-side log `Untyped Retype: Invalid destination address.` mirrors this return code.
- Follow-up work should focus on why the PL011 physical address `0x09000000` cannot be retyped into a 4 KiB device page within the provided destination slot rather than on logging gaps.

## Recommended Investigation Path
- Use the `retype.sanitised` or `retype.sanitise_error` line to determine whether the trace was normalised successfully. A sanitisation error immediately pinpoints which parameter (root capability, node index, guard depth, or slot bounds) diverged from the bootinfo contract.
- Verify the destination capability path in `apps/root-task/src/kernel.rs` aligns with the manifest entry for the PL011 UART. The `retype.init_cnode` line will show the canonical values that sanitisation expects.
- Re-run `coh-rtc` to regenerate the device manifest if any physical address assignments changed; mismatches between compiled manifests and the boot image will also surface as lookup failures.
- Inspect the root-task CNode layout dump in the debug log to confirm the slot intended for the PL011 device page is free before the retype attempt.

## Resolution Summary
- The failure stemmed from issuing `node_depth = 0` tuples to `seL4_Untyped_Retype`. The kernel expects the init CNode to be addressed with guard depth equal to `initThreadCNodeSizeBits`; supplying zero causes `lookupTargetSlot` to reject the destination before any capability can be installed.
- Updating `KernelEnv::prepare_retype_trace` and its sanitiser to emit the canonical tuple (`root = initCNode`, `idx = 0`, `depth = initBits`, `off = slot`) ensures the kernel accepts the destination path while preserving the slot bounds derived from `initThreadCNodeSizeBits`.
- Retype helpers now log the ABI order explicitly: `seL4_Untyped_Retype(untyped, obj, size_bits, root, node_index, node_depth, node_offset, n)`. When targeting the init CNode, the offset varies while the guard depth remains `initThreadCNodeSizeBits`.
- Always emit `(root=bootinfo.initThreadCNode, node_index=0, node_depth=initThreadCNodeSizeBits, node_offset=slot)` when addressing the init thread CNode. This tuple is range-checked against `initThreadCNodeSizeBits` before the syscall to prevent slot clobbering.

## Message Dispatch Safety
- Bootstrap IPC payloads probed during early boot are staged until the event pump registers all handlers; delivery only begins once `handlers_ready()` is signalled.
- Opcode dispatch now uses the `BootstrapOp` enum and an explicit `match`, removing the former function-pointer table.
- All IPC buffer diagnostics obtain data through `IpcBufView::prefix`, guaranteeing dumps never exceed 4096 bytes.

## Net Bring-up Staging
- `net.stage=<stage>` is emitted once during network init to confirm the compile-time ladder selection.
- When `virtio_guard_queue` is enabled, virtio-net logs `virtio.guard_queue=1 base=<va> len=<bytes> guard=<va>` to identify the guarded queue region and the unmapped tripwire page.
