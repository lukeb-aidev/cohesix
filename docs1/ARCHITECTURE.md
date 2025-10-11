# Cohesix Architecture (Pure Rust Userspace on seL4)

## High-Level
- **Kernel (external)**: seL4 provides capabilities, endpoints, timers, memory.
- **Root Task (Rust)**: boots, sets timer, spawns services, exposes capability-limited endpoints.
- **NineDoor (Rust 9P server)**: capability-aware virtual filesystem surface.
- **Workers (Rust)**: sandboxed processes interacting only through 9P files.

## Crates
- `coh_root_task`: entry process; init + process manager.
- `coh_nine_door`: 9P codec, fid table, node providers, transport adapters.
- `coh_worker_heart`: example worker with tick/telemetry.

## Data Structures (Rust-like)
```rust
pub struct Ticket([u8; 32]);
pub type Fid = u32;
pub type Qid = u64;

#[derive(Clone, Copy)]
pub enum OpenMode { ReadOnly, WriteOnlyAppend }

pub enum NodeKind { Dir, RegReadOnly, RegAppendOnly }

pub struct QidMeta { pub qid: Qid, pub kind: NodeKind, pub version: u32 }

pub trait Node {
    fn qid(&self) -> QidMeta;
    fn walk(&self, elem: &str) -> Result<&dyn Node, FsError>;
    fn open(&self, mode: OpenMode) -> Result<Box<dyn Handle>, FsError>;
}

pub trait Handle {
    fn read(&mut self, off: u64, buf: &mut [u8]) -> Result<usize, FsError>;
    fn write(&mut self, buf: &[u8]) -> Result<usize, FsError>; // append-only ignores off
}

pub struct Session { pub ticket: Ticket, pub msize: u32, pub next_fid: Fid, pub mounts: MountTable }
pub struct MountTable { /* virtual path -> provider root */ }
```

## 9P Codec (2000.L)
- Implement: `Tversion/Rversion`, `Tattach/Rattach`, `Twalk/Rwalk`, `Topen/Ropen`, `Tread/Rread`, `Twrite/Rwrite`, `Tclunk/Rclunk`.
- Enforce `msize` cap; early reject oversize frames.
- FIDs are per-session; clunked FIDs cannot be reused.

## Providers
- `/proc/boot` (read-only text), `/queen/ctl` (append-only), `/log/queen.log` (append-only), `/worker/<id>/telemetry`.

## IPC
- Root exposes `spawn(args)->WorkerId` and `kill(id)->bool` endpoints; NineDoor maps `/queen/ctl` appends to these calls.

## Error Model
- `NotFound`, `Permission`, `Busy`, `Invalid`, `TooBig`, `Closed` → 9P error strings.

## Transport
```rust
pub trait Transport { fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), IoError>; fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError>; }
```
Implement: InProc ring; seL4 endpoint wrapper; optional host TCP (outside TCB).
