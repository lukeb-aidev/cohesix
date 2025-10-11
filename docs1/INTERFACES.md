# Cohesix Interfaces

## Queen Control
**Path:** `/queen/ctl` (append-only)  
**JSON Lines:** 
```json
{"spawn":"worker","args":{"policy":"heartbeat","ticks":100}}
{"kill":"<id>"}
```

## Telemetry
`/worker/<id>/telemetry` append-only: `"heartbeat <tick>"` lines.

## 9P Limits
- `msize` ≤ 8192
- max walk depth 8
- append-only ignores offset

## IPC Traits
```rust
pub trait QueenCtl { fn spawn_heartbeat(&self, ticks: u32) -> Result<String, CtlError>; fn kill(&self, id: &str) -> Result<(), CtlError>; }
```
