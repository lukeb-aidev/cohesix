<!-- Author: Lukas Bower -->
<!-- Purpose: Define common production capacity and justified target-specific kernel and manifest allocations. -->
<!-- Copyright 2026 Lukas Bower -->

# Production profiles and capacity

The selected inputs are `configs/root_task.toml` with `qemu_smp_production`
and `configs/root_task_pi4_uboot_aarch64.toml` with `pi4_production`.
The Pi image builder defaults to the immutable `seL4/build_UBOOT` production
kernel. Both use release seL4 kernels, four cores, MCS, IPC fastpath, exported
virtual counters, and disabled kernel debugging, printing, and benchmarking.
Other repository prebuilt kernels retain their existing identities.

Both manifests enable the implemented production features: isolated services
and Workers, CAS/signature verification, host tickets/federation, policy,
audit/replay, models, telemetry and the host Modbus sidecar. Modbus describes
a host adapter; enabling its namespace does not add an in-VM bus driver.
Unimplemented WorkerBus and DNP3 execution remain disabled on both targets.

## Capacity means admitted execution

Both profiles expose 256 executable Worker slots (one heartbeat, 127 GPU,
128 LoRA) and the maximum compiler-supported 8-bit/256-shard namespace.
This is the current supported execution envelope, not a claim that every
theoretical object limit has been exercised. Larger host-model namespaces do
not establish additional executable Worker capacity. Increasing this envelope
requires coherent role, badge, queue, object and memory inventories plus new
target construction and pressure evidence.

Common limits retain 512 TCBs/CNodes/VSpaces/ASIDs/endpoints/Replies/SCs,
4096 page tables, 8192 frames, 128 notifications and 256 MiB of admitted
untyped memory. These are admission ceilings, not eagerly allocated objects.
Both preserve the complete post-construction reserve, including 512 frames,
2048 CSpace slots and 32 MiB untyped memory. Do not convert that reserve into
Worker capacity. Pi consumes 7666 admitted frames including reserves, leaving
526; its 65536-slot root CSpace accounts for 19516 slots including reserves.
QEMU's 16384-slot CSpace accounts for 14620 slots including reserves.

Protocol and queue bounds remain common: 8192-byte Secure9P msize, 16 tags,
one control in flight, eight console commands/packets per wake, 1024-byte
Worker telemetry rings, and 32 KiB TCP receive/send buffers. Raising all
queue limits to their numeric maxima would increase memory and latency
without proving more useful concurrency. These bounds require complete-path
evidence before expansion.

## Required target differences

| Difference | Reason and bound |
| --- | --- |
| Platform, GICv3/v2, 24/54 MHz counter, network configuration | QEMU VirtIO and Pi BCM2711 hardware contracts. |
| Physical devices, IRQs, DMA and local-seat/attestation declarations | Pi owns seven isolated physical drivers, including USB/HDMI and linked WiFi/SDIO; QEMU does not construct these drivers. |
| Console frame/slot inventory | Compiler-derived VirtIO mappings versus direct GENET mappings; application TCP buffers are identical. |
| Fixed objects, fault/badge inventories, supervisor caps | Pi's seven additional driver TCBs require their own objects and authority. |
| Root CNode 14/16 bits and physical memory map | Exact paired-kernel allocations; Pi needs more slots for physical runtime and framebuffer mappings. Pi's kernel uses the supported 2 GiB device/RAM map. |
| Initial root SC 128/256 bytes, 2/8 refills | Clean upstream QEMU allocates 128 bytes at boot; the authenticated Pi overlay allocates 256. A manifest cannot enlarge this pre-existing object. Both sizes are checked against the selected kernel contract. |
| Root/fault/emergency cores and root budget 9000/5500 us per 10000 us | QEMU can dedicate core 0 to root. Pi also admits 3000 us fault and 250 us emergency reservations on core 0. Copying QEMU's root budget would exceed admission. |
| Supervisors and LoRA executor budgets | Pi shares core 1 with GENET/serial/USB/HDMI and core 3 with WiFi/SDIO. QEMU can devote more of those cores to supervisors/LoRA. |
| Worker-supervisor and GPU-executor SC refills | QEMU declares 10 refills for each; Pi retains 2 for its Worker supervisor and 8 for its GPU executor. Both use 256-byte SC objects. These target-specific replenishment allocations preserve the selected scheduling envelopes; parity does not imply identical refill histories. |
| WCET, response bounds and provenance | Target-specific execution and interference envelopes; equalizing numbers would erase their hardware basis. |
| Root serial-I/O allowance | QEMU root services the virtual UART; Pi's declared isolated serial owner owns physical I/O. |

The common admission window is 10000 us with a 1000 us reserve on every core.
Manifest active reservations consume QEMU 90/90/80/82.5% and Pi
87.5/82.5/84/80% on cores 0–3. These are reserved shares, not measured CPU
utilization. Spare capacity protects bounded interference; it is not evidence
that arbitrary budget increases are safe. Both console services receive
3000/10000 us, priority 200 and eight refills. Both use the same bounded
NineDoor timeout policy and Worker bootstrap envelope (400/10000 us).

## Validation and comparison

`tests/test_production_manifest_parity.py` guards common feature, policy and
capacity parity and allows only the target differences above. `coh-rtc`
remains the authority for full admission, SC layout, namespace, and ABI checks.
The host-tool suite, `tools/cohesix-py`, gateway, `cohsh`, `coh`, SwarmUI,
generated profiles and benchmark consumers must use regenerated profile
contracts. Shard labels change for QEMU; retained evidence keeps its original
manifest and may not be relabelled with this profile.

Use `scripts/rest_perf_harness.py` for all benchmarks. Its concurrent HTTP
TAIL requests still use one serialized target stream; TAIL batch means are
not per-request p95. Compare exact release-kernel images and matching workload
receipts. Build/admission checks and QEMU execution do not establish fresh Pi
boot, physical performance, repeatability or full-system release acceptance.
