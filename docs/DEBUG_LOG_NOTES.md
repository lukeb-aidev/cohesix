<!-- Author: Lukas Bower -->
# Root-task Debug Log Notes

## Retype Instrumentation Markers
- The root-task's `kernel` module emits lines beginning with `retype status=` whenever the extended retype tracing hook runs.
- The suffix `pending`, `ok`, or `err(<code>)` mirrors the internal `RetypeStatus` enum from `apps/root-task/src/kernel.rs`.
- Seeing a `retype status=err(6)` line means the trace captured the error path after `seL4_Untyped_Retype` failed and before the panic handler aborted the boot.
- The accompanying `retype.kind=` line reports the `RetypeKind` variant, which is `device_page` for MMIO mappings such as the PL011 UART.
- Device coverage output like `device coverage idx=16 [...] state=free` confirms the root-task examined the manifest entry for the requested MMIO region before the failure.

## Implication for Current Panic
- Because the trace shows `retype status=err(6)` directly before the panic, the extended debug path **did** execute.
- The seL4 error code `6` corresponds to `seL4_FailedLookup`, so the kernel rejected the destination CNode/slot while decoding the untyped invocation rather than skipping our instrumentation.
- Follow-up work should focus on why the PL011 physical address `0x09000000` cannot be retyped into a 4 kiB device page within the provided destination slot rather than on logging gaps.

## Recommended Investigation Path
- Verify the destination capability path in `apps/root-task/src/kernel.rs` aligns with the manifest entry for the PL011 UART. A stale depth or guard can trigger the `seL4_FailedLookup` reported by the tracing hook.
- Re-run `coh-rtc` to regenerate the device manifest if any physical address assignments changed; mismatches between compiled manifests and the boot image will also surface as lookup failures.
- Inspect the root-task CNode layout dump in the debug log to confirm the slot intended for the PL011 device page is free before the retype attempt.

## Resolution Summary
- The panic stemmed from supplying the root CNode's slot index to the `node_index`/`node_depth` parameters of `seL4_Untyped_Retype`. That forced the kernel to traverse a non-existent sub-CNode and fail with `seL4_FailedLookup` before inserting the PL011 frame.
- Updating `KernelEnv::prepare_retype_trace` and its sanitiser to leave the traversal path empty (both values zero) allows the kernel to consume the destination slot solely via `dest_offset`, restoring successful device retype and the boot flow into the root-task console.
