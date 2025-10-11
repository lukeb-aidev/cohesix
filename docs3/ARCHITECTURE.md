# Cohesix Architecture (Roles, Queen/Worker, GPU Roadmap)

## Components
- **Root Task (Rust)** — init, timer, spawn/kill endpoints, capability minting.
- **NineDoor (Rust 9P)** — 9P2000.L server exposing a synthetic namespace; role-scoped mounts.
- **Workers (Rust)** — processes with role-assigned tickets; communicate via 9P files.
- **(Optional) Host GPU Worker** — runs on macOS host or edge Linux box; managed via 9P-facing adapter (outside TCB).

## Roles
- `Queen` — can read `/proc/boot`, write `/queen/ctl`, read `/log/queen.log`.
- `Worker:Heartbeat` — can append to its own `/worker/<id>/telemetry` only.
- `Worker:GPU` (future) — can read `/gpu/<id>/info`, append `/gpu/<id>/job`, read `/gpu/<id>/status` for its lease.

## Data Structures (Rust-ish)
```rust
pub struct Ticket([u8;32]);           // capability token minted at spawn
pub type Fid = u32; pub type Qid = u64;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {{ Queen, WorkerHeartbeat, WorkerGpu }}

pub struct Session {{ pub ticket: Ticket, pub role: Role, pub msize: u32, pub next_fid: Fid, pub mounts: MountTable }}

pub enum OpenMode {{ ReadOnly, WriteOnlyAppend }}
pub enum NodeKind {{ Dir, RegReadOnly, RegAppendOnly }}

pub struct QidMeta {{ pub qid: Qid, pub kind: NodeKind, pub version: u32 }}

pub trait Node {{
  fn qid(&self) -> QidMeta;
  fn walk(&self, elem: &str) -> Result<&dyn Node, FsError>;
  fn open(&self, mode: OpenMode) -> Result<Box<dyn Handle>, FsError>;
}}

pub trait Handle {{
  fn read(&mut self, off:u64, buf:&mut[u8])->Result<usize, FsError>;
  fn write(&mut self, buf:&[u8])->Result<usize, FsError>; // append-only ignores offset
}}

// Role-scoped provider dispatch
pub trait Provider {{ fn root_for(role: Role) -> &'static dyn Node; }}
```

## Capability & Access
- Tickets are per-session; FIDs die on clunk; no reuse.
- Providers check {{role, open_mode, path}}; deny writes to read-only nodes; deny cross-role paths.

## GPU Worker (Design‑for‑later)
- Runs **outside** seL4 VM; uses NVML/CUDA to probe and run jobs.
- Exposes a **bridge** that maps host-side ops to VM 9P nodes:
  - `/gpu/<id>/info` — memory, SMs, driver/runtime versions
  - `/gpu/<id>/ctl` — `lease`, `release`, `priority`
  - `/gpu/<id>/job` — append JSON jobs `{{kernel, grid, block, bytes_hash, stream}}`
  - `/gpu/<id>/status` — job/stream state lines
- VM side remains pure Rust and capability-gated; no CUDA inside TCB.


**9P Security & Layering:** see `SECURE9P.md` for codec/core/transport split and capability hooks.
