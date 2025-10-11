# Interfaces (Queen/Worker Roles, 9P, GPU Bridge)

## Queen Control (append-only)
Path: `/queen/ctl`
```json
{"spawn":"heartbeat","ticks":100}
{"kill":"<worker_id>"}
{"spawn":"gpu","lease":{"gpu_id":"GPU-0","mem_mb":4096,"streams":2,"ttl_s":120}}
```
- `spawn:"gpu"` is **future** and maps to a host GPU worker lease request.

## Telemetry
`/worker/<id>/telemetry` (append-only) — heartbeat lines, or GPU job status lines (role: WorkerGpu).

## GPU Bridge (future; outside TCB)
- `/gpu/<id>/info` (read-only text): vendor, model, memory, SMs, driver/runtime versions.
- `/gpu/<id>/ctl` (append-only): `LEASE`, `RELEASE`, `PRIORITY <n>`
- `/gpu/<id>/job` (append-only JSON):
```json
{"kernel":"vadd","grid":[128,1,1],"block":[256,1,1],"bytes_hash":"sha256:...","stream":0}
```
- `/gpu/<id>/status` (read-only append stream): `JOB <jid> QUEUED|RUNNING|OK|ERR <code>`

## 9P Limits
- msize ≤ 8192, max walk depth 8, append-only ignores offset, RO rejects write.

## IPC Traits
```rust
pub trait QueenCtl {
  fn spawn_heartbeat(&self, ticks:u32)->Result<String,CtlError>;
  fn kill(&self, id:&str)->Result<(),CtlError>;
  // Future:
  fn spawn_gpu(&self, lease:GpuLease)->Result<String,CtlError>;
}

pub struct GpuLease { pub gpu_id:String, pub mem_mb:u32, pub streams:u8, pub ttl_s:u32 }
```
