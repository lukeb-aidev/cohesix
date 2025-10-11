<!-- Author: Lukas Bower -->
# GPU Nodes — Out-of-VM Acceleration Strategy

## 1. Rationale
CUDA/NVML stacks are large and platform-specific. Keeping them outside the seL4 VM preserves determinism and minimises the trusted computing base (TCB). The VM interacts with GPUs exclusively through a capability-guarded 9P namespace mirrored by host workers.

## 2. Host GPU Worker Architecture
- **Process**: Rust binary running on macOS or a Linux edge node.
- **Responsibilities**:
  - Discover GPUs using NVML (Linux) or Metal proxies (macOS, stubbed).
  - Enforce leases that cap memory (MiB), stream counts, and wall-clock TTL.
  - Mirror GPU state into the VM by exposing a 9P transport endpoint (`secure9p-transport::Tcp`) to NineDoor.
- **Safety**: Validate kernel binaries via SHA-256; ensure uploads match expected byte length before dispatch.

## 3. VM Namespace Mapping
| VM Path | Backing Action |
|---------|----------------|
| `/gpu/<id>/info` | Serialize GPU metadata (name, UUID, memory, SM count, driver/runtime versions). |
| `/gpu/<id>/ctl` | Accept textual commands (`LEASE`, `RELEASE`, `PRIORITY <n>`, `RESET`) and return status lines. |
| `/gpu/<id>/job` | Append JSON descriptors. Host worker allocates buffers, uploads code/data, and schedules kernels. |
| `/gpu/<id>/status` | Stream newline-delimited state transitions for submitted jobs. |

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
- `timeout_ms` triggers job cancellation; status stream records `ERR TIMEOUT`.
- Successful submissions emit `QUEUED`, `RUNNING`, and `OK` entries in `/gpu/<id>/status` alongside worker telemetry updates.

## 6. Simulation Path (for CI & macOS)
- `gpu-bridge-host --mock --list` emits deterministic namespace descriptors consumed by NineDoor via `install_gpu_nodes`.
- `info` returns synthetic GPU entries, `job` triggers precomputed status sequences.
- Enables continuous validation of control plane without real hardware.

## 7. Security Notes
- No GPU device nodes or drivers are shipped in the VM.
- Tickets for `/gpu/*` paths are issued only to `WorkerGpu` roles.
- All control traffic is logged to `/log/queen.log` with ticket IDs for audit.
