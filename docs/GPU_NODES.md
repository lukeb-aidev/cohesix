<!-- Author: Lukas Bower -->
# GPU Nodes — Out-of-VM Acceleration Strategy

## 1. Rationale
CUDA/NVML stacks are large and platform-specific. Keeping them outside the seL4 guest (whether running on QEMU or physical UEFI hardware) preserves determinism and minimises the trusted computing base (TCB). The Cohesix instance interacts with GPUs exclusively through a capability-guarded 9P namespace mirrored by host workers.
GPU workers (`worker-gpu`) are another worker type under the hive’s Queen, not standalone services.

## 2. Host GPU Worker Architecture
- **Process**: Rust binary running on macOS or a Linux edge node, outside the Cohesix instance, paired with the GPU bridge host.
- **Responsibilities**:
  - Discover GPUs using NVML (Linux) or Metal proxies (macOS, stubbed).
  - Enforce leases that cap memory (MiB), stream counts, and wall-clock TTL.
  - Mirror GPU state into the Cohesix instance by exposing a 9P transport endpoint (`secure9p-transport::Tcp`) to NineDoor and brokering `/gpu/` files; no CUDA/NVML components enter the VM profile or hardware deployment.
- **Safety**: Validate kernel binaries via SHA-256; ensure uploads match expected byte length before dispatch.

## 3. Cohesix Namespace Mapping
| Cohesix Path | Backing Action |
|---------|----------------|
| `/gpu/<id>/info` | Serialize GPU metadata (name, UUID, memory, SM count, driver/runtime versions). |
| `/gpu/<id>/ctl` | Accept textual commands (`LEASE`, `RELEASE`, `PRIORITY <n>`, `RESET`) and return status lines mediated by the bridge host. |
| `/gpu/<id>/lease` | Ticket/lease file gated by host policy; worker-gpu reads to learn active allocations and writes to request renewals. |
| `/gpu/<id>/stats` | Read-only view of utilisation and recent job summaries sourced from the host; replaces the earlier `/status` stream. |

## 4. Lease Model
```rust
pub struct GpuLease {
    pub gpu_id: String,
    pub mem_mb: u32,
    pub streams: u8,
    pub ttl_s: u32,
  pub priority: u8,
}
```
- Leases are tied to a worker ticket; revocation closes associated fids.
- Host worker enforces TTL via timers; once expired, queued jobs are drained and subsequent writes receive `Permission`.
- The Queen uses `/queen/ctl` to create GPU workers and manage leases within the same hive orchestration model.

## 5. Job Descriptor Schema
```json
{
  "job": "jid-42",
  "kernel": "vadd",
  "grid": [128, 1, 1],
  "block": [256, 1, 1],
  "bytes_hash": "sha256:...",
  "inputs": ["/bundles/vadd.ptx"],
  "outputs": ["/worker/jid-42/result"],
  "timeout_ms": 5000,
  "payload_b64": "..."
}
```
- Host validates payload hash against staged artefacts before launch; when `payload_b64` is present the bridge decodes and hashes the inline bytes.
- `timeout_ms` triggers job cancellation; status stream records `ERR TIMEOUT` or includes the failure in `/gpu/<id>/stats`.
- Successful submissions emit `QUEUED`, `RUNNING`, and `OK` entries in `/gpu/<id>/stats` alongside worker telemetry updates.
GPU workers do not schedule hardware directly; they receive tickets and leases from the host over Secure9P, and all scheduling policy (queueing, eviction, throttling) runs on the host side of the bridge.

## 6. Simulation Path (for CI & macOS)
- `gpu-bridge-host --mock --list` emits deterministic namespace descriptors consumed by NineDoor via `install_gpu_nodes`.
- `info` returns synthetic GPU entries, `job` triggers precomputed status sequences.
- Enables continuous validation of control plane without real hardware.
- CLI/GUI clients submit GPU jobs via the same verbs exposed through `cohsh` and Secure9P; no separate ad-hoc GPU control protocol exists inside the Cohesix instance.

## 7. Security Notes
- No GPU device nodes or drivers are shipped in the Cohesix instance (including the QEMU development image), and direct device access/virtio-gpu paths are explicitly out of scope; the bridge host terminates DMA and enforces isolation.
- Tickets for `/gpu/*` paths are issued only to `WorkerGpu` roles.
- All control traffic is logged to `/log/queen.log` with ticket IDs for audit.

Future work (per `BUILD_PLAN.md` milestones): ticket arbitration across multiple workers, lease renewal/expiry enforcement, GPU worker lifecycle hooks, and CI coverage of the `/gpu/<id>/` namespace surface.
