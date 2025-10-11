# GPU Nodes (Out-of-VM CUDA/NVML Workers)

Why outside TCB? CUDA/NVML are large, dynamic, and OS-specific. Keeping the seL4 VM minimal preserves determinism and security.

## Architecture
- **GPU Worker (host/edge)**: Rust process with NVML bindings and CUDA runtime that exposes a **9P-facing bridge** to the VM.
- **Bridge Mapping**: Host worker mirrors files into VM namespace via NineDoor adapter:
  - `/gpu/<id>/info` ← NVML query
  - `/gpu/<id>/ctl` ← lease/release/priority
  - `/gpu/<id>/job` ← queue kernels (validated grid/block/bytes_hash)
  - `/gpu/<id>/status` ← job lifecycle updates

## Leases
```
struct GpuLease {
  gpu_id: String,
  mem_mb: u32,
  streams: u8,
  ttl_s: u32,
}
```
- Enforced by the host worker. On expiry, jobs drain and files become read-only.

## Security
- No device nodes inside VM.
- Capability tickets for `/gpu/*` paths bound to `WorkerGpu` only.
- Hash-validated payloads (`bytes_hash`) to prevent arbitrary code injection.

## Minimal Job Types (v1 candidate)
- `vadd` (vector add), `sgemm` (matrix multiply) — prove scheduling & telemetry.
- Async streams 0..N with back-pressure per lease.

## Dev on M4 Mac
- Implement GPU worker logic with feature flags; stub CUDA on macOS (no execution).
- CI still validates the control protocol and 9P mapping without GPUs present.
