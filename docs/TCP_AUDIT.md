<!-- Author: Lukas Bower -->
# Virtio-Net DMA + TCP Audit (BOOTINFO Snapshot Corruption)

## 1) DMA address correctness audit (RX/TX)

**Code references**
- `apps/root-task/src/sel4.rs`: `KernelEnv::alloc_dma_frame` (DMA frames mapped in DMA window, logged as UNCACHED). 
- `apps/root-task/src/drivers/virtio/net.rs`: `VirtioNet::new`, `initialise_queues`, `submit_tx`, `requeue_rx` (descriptor addresses sourced from `RamFrame::paddr`).
- `apps/root-task/src/drivers/virtio/net.rs`: `VirtQueue::new` (queue addresses programmed from `base_paddr`).

**Invariants observed**
- DMA buffers are mapped into the DMA window (vaddr >= `dma_window_base()`) and logged with `paddr` by `KernelEnv::alloc_dma_frame`.
- RX/TX descriptors use `buffer.paddr()` (not `buffer.ptr()`), and queue addresses are programmed from `base_paddr`.
- Queue backing frames are 4KiB and page-aligned.

**Additional debug checks added (this change)**
- Descriptor address validation ensures:
  - `desc.addr` not within virtio-mmio range.
  - `desc.addr` not within DMA vaddr range (heuristic for accidental vaddr use).
  - Page-aligned `desc.addr` for RX/TX single-buffer descriptors.
  - `desc.addr` does not overlap RX/TX virtqueue backing frames.
  - `desc.addr` is within the RX or TX DMA buffer pool.
- DMA pool overlap checks ensure RX/TX buffers do not overlap queue backing frames.

## 2) Virtqueue layout + bounds audit

**Code references**
- `apps/root-task/src/drivers/virtio/net.rs`: `VirtqLayout::compute_vq_layout`, `VirtQueue::new`, `VirtQueue::push_avail`, `VirtQueue::pop_used`.

**Evidence**
- Layout uses `desc_len = qsize * 16`, `avail = 4 + 2*qsize (+event)`, `used = 4 + 8*qsize (+event)` with alignment checks.
- `VirtQueue::new` verifies ring offsets, alignment, and total length within the 4KiB backing frame.
- `push_avail` uses `idx % qsize` (no power-of-two assumption).

**Status**
- Layout and bounds checks appear consistent with the virtio spec for split rings.
- No layout overlap or ring size violations observed in logs.

## 3) Queue programming order + status sequencing

**Code references**
- `apps/root-task/src/drivers/virtio/net.rs`: `VirtioNet::new`, `VirtQueue::new`, `VirtioRegs::*`.

**Evidence**
- Status sequence: reset → ACKNOWLEDGE → DRIVER → FEATURES_OK → queue setup → RX buffer provisioning → DRIVER_OK.
- Queues are configured and `queue_ready` is set before `DRIVER_OK`.
- RX provisioning notifies the device before `DRIVER_OK` (last RX buffer triggers notify).

**Risk note**
- Some virtio implementations may ignore queue kicks before `DRIVER_OK`, but if a device responds early, it can DMA before the final status bit is set. This is likely compliant but is worth tracking given the corruption window.

## 4) Memory ordering / barriers

**Code references**
- `apps/root-task/src/drivers/virtio/net.rs`: `VirtQueue::push_avail`, `VirtQueue::invalidate_used_header_for_cpu`, `VirtQueue::invalidate_used_elem_for_cpu`, `dma_barrier`.

**Evidence**
- Descriptor writes are followed by `sync_descriptor_table_for_device` and Release fences.
- Avail ring updates include Release fences before index updates.
- Notify uses `dma_barrier` (DSB/ISB on AArch64).
- Used ring reads invalidate headers/elements and apply SeqCst fences.

**Status**
- Ordering coverage is conservative; no obvious missing fence observed for split ring operation.

## 5) RX buffer lifecycle + smoltcp contract

**Code references**
- `apps/root-task/src/drivers/virtio/net.rs`: `VirtioRxToken::consume`, `requeue_rx`, `pop_rx`.

**Evidence**
- RX buffer is handed to smoltcp as a slice during `consume`, then requeued immediately after the closure returns.
- No reuse occurs while smoltcp holds the slice.

**Status**
- RX lifecycle appears consistent with smoltcp’s consume-then-release contract.

## 6) TX ownership, reclaim, and device-reported IDs

**Code references**
- `apps/root-task/src/drivers/virtio/net.rs`: `enqueue_tx_chain_checked`, `reclaim_tx`, `reclaim_tx_v2`, `VirtQueue::pop_used`.

**Evidence**
- Used-ring `id` validated against `qsize` before use; out-of-range IDs trigger forensic faults.
- TX ownership is tracked; double-submit is detected.

**Status**
- TX reclaim path has index validation and logs anomalies.

## 7) BootInfo snapshot corruption correlation

**Code references**
- `apps/root-task/src/bootstrap/bootinfo_snapshot.rs`: canary checks and `debug_peek_canary`.
- `apps/root-task/src/net/stack.rs`: `log_bootinfo_mark` and `BootInfoState::verify`.
- `apps/root-task/src/drivers/virtio/net.rs`: debug canary peeks around RX provisioning and DRIVER_OK.

**Evidence from QEMU run**
- BootInfo snapshot region (vaddr) logged as `[0x000000000007ba30..0x000000000007e769)`.
- DMA buffers and virtqueue backing were allocated at paddr `0x4033f000..0x4035?000` (UNCACHED).
- Corruption is detected immediately after `DRIVER_OK` during `net.init.device`:
  - `BOOTINFO_SNAPSHOT_CORRUPTED phase=net.init last_mark=net.init.device ... post=0xf0000000000001c9`.

**Likely root cause (current evidence)**
- **DMA scribble** originating from virtio-net after queues are configured and RX buffers are posted. The timing is tightly coupled to `DRIVER_OK`, and the post-canary value looks like a DMA write pattern rather than a software overwrite.

**Why this is not yet conclusive**
- Descriptor addresses logged in the run are physical (e.g., `0x40342000`), not obvious vaddrs.
- No direct overlap is visible between BootInfo snapshot vaddr range and DMA paddr ranges.

## 8) Minimal patches applied

- `apps/root-task/src/drivers/virtio/net.rs`: added debug-only DMA range assertions to catch:
  - descriptor addresses that look like vaddrs,
  - descriptor addresses inside virtio-mmio space,
  - overlaps with virtqueue backing frames,
  - descriptor addresses outside the RX/TX DMA pools.
- `apps/root-task/src/drivers/virtio/net.rs`: added debug-only overlap checks between DMA buffers and queue backing frames.
- `apps/root-task/src/drivers/virtio/net.rs`: added HAL DMA window snapshot logging before queue allocation.

These changes are fail-fast diagnostics only; no refactors or interface changes.

## 9) Remaining risks / follow-ups

- Verify that the MMIO device expects physical addresses (not IOVA/bus offsets). If an IOMMU/bus translation is required in this environment, descriptors and queue addresses may need translation.
- Confirm that early queue kicks (before `DRIVER_OK`) are acceptable for the virtio-net implementation used by QEMU; if not, move the RX notify to after `DRIVER_OK` as a targeted experiment.
- Consider a temporary guard-page allocation adjacent to queue backing frames to detect DMA overflow (behind a debug feature).

## Repro steps + logs

**QEMU launch command used**
```
scripts/cohesix-build-run.sh --sel4-build /Users/lukasbower/GitHub/cohesix/seL4/build --cargo-target aarch64-unknown-none --profile release --root-task-features dev-virt --raw-qemu
```

**Log tail (virtio init → DRIVER_OK → BOOTINFO corruption)**
```
[INFO net-console] [net-console] allocating virtqueue backing memory
[INFO hal] [hal] dma frame mapped vaddr=0xb0000000 paddr=0x4033f000 attr=UNCACHED
[INFO hal] [hal] dma frame mapped vaddr=0xb0001000 paddr=0x40341000 attr=UNCACHED
[INFO net-console] [virtio-net] queue 0 configured: size=16 pfn=0x4033f mode=Modern
[INFO net-console] [virtio-net] queue 1 configured: size=16 pfn=0x40341 mode=Modern
[INFO virtio-net] [virtio-net][rx-arm] start buffers=16 hdr_len=12 payload_cap=1536 frame_cap=1548
[INFO net-console] [virtio-net] RX queue initialised: size=16 buffers=16 avail.idx=16 used.idx=0 first_paddr=0x40342000 last_paddr=0x40351000
[INFO net-console] [virtio-net] TX queue initialised: size=16 buffers=16 free_entries=16
[INFO net-console] [virtio-net] post-setup: queue0_pfn=0x4033f, queue1_pfn=0x40341, status=0x0b
[INFO virtio-net] [virtio-net] DRIVER_OK about to set
[INFO net-console] [virtio-net] driver status set to DRIVER_OK (status=0x0f)
[INFO root_task::net::stack] [bootinfo:net] attempt_id=0x0000000100000001 mark=net.init.device region=[0x000000000007ba30..0x000000000007e769) len=0x00002d39 pre=0x0b0f1ce5ca4ecafe post=0x...
BOOTINFO_SNAPSHOT_CORRUPTED phase=net.init last_mark=net.init.device pre=0x0b0f1ce5ca4ecafe post=0xf0000000000001c9 expected_pre=0x0b0f1ce5ca4ecafe expected_post=0x9ddf1ce5f00dbeef
```
