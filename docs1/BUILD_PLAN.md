# Cohesix Build Plan (ARM64, Pure Rust Userspace) — v0

**Host:** macOS 26 on Apple Silicon (M4)  
**Target:** QEMU aarch64 `virt` (GICv3)  
**Kernel:** seL4 (external)  
**Userspace:** Pure Rust crates (`root-task`, `nine-door`, `worker-heart`)

---

## Milestone 0 — Repo Skeleton & Toolchain (1–2 days)
**Deliverables**
- Cargo workspace with three crates: `root-task`, `nine-door`, `worker-heart`.
- `toolchain/setup_macos_arm64.sh` installs: rustup stable, qemu, llvm, cmake, ninja, python3.
- `scripts/qemu-run.sh` runs QEMU aarch64 `virt` with external `elfloader`, `kernel.elf`, `rootfs.cpio`.
- `ci/size_guard.sh` caps CPIO at 4MB.

**Checks**
- `cargo check` passes.
- `qemu-system-aarch64 --version` prints version.
- `ci/size_guard.sh out/rootfs.cpio` works on a dummy (tiny) CPIO.

---

## Milestone 1 — Boot + Timer + IPC (3–5 days)
**Deliverables**
- Root task prints banner on serial.
- Configure periodic timer in root task.
- Spawn a second user component; perform one IPC ping-pong.

**Checks**
- Serial shows banner and periodic tick.
- Serial shows `PING 1` / `PONG 1` once per boot.
- No panics; QEMU exits cleanly on Ctrl-C.

---

## Milestone 2 — Minimal 9P Server (“NineDoor”) (1–2 weeks)
**Deliverables**
- 9P2000.L codec + fid table (attach/walk/open/read/write/clunk).
- Synthetic namespace in memory:
  - `/proc/boot` (read-only text)
  - `/log/queen.log` (append-only)
  - `/queen/ctl` (append-only command sink)
- Transport: in-process channel or seL4 endpoint wrapper. (No TCP in v0.)

**Checks**
- A host or in-VM client can `attach`, `walk`, `read` `/proc/boot` and append to `/queen/ctl`.
- Attempting to write to `/proc/boot` fails with an error.

---

## Milestone 3 — Queen/Worker MVP (1 week)
**Deliverables**
- Command schema (JSON lines) for `/queen/ctl`: `{"spawn":"worker","args":{"ticks":100}}`, `{"kill":"<id>"}`.
- Worker-heart process emits `"heartbeat"` lines to `/worker/<id>/telemetry`.
- Budget enforcement (time or ops): worker terminated when exceeded.

**Checks**
- Writing spawn to `/queen/ctl` creates a worker and telemetry file appears.
- Writing kill to `/queen/ctl` removes worker and closes telemetry.
- Append-only guarantees: no overwrite allowed on telemetry/log nodes.

---

## Milestone 4 — Namespaces & Bind (1 week)
**Deliverables**
- Per-process mount tables.
- `bind(from, to)` and `mount(service, at)` operations, limited to single-path (no unions yet).

**Checks**
- A process can re-map `/queen` view via bind/mount; other processes unaffected.

---

## Milestone 5 — Hardening & Tests (ongoing)
**Deliverables**
- Unit tests for codec, fid lifecycle, error paths.
- Fuzz corpus for 9P decoder (malformed frames don’t panic).
- Minimal integration test: spawn → telemetry → kill.

**Checks**
- `cargo test` passes.
- Decoder fuzz suite runs N cases without crash.
