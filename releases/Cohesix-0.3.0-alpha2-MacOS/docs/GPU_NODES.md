<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Describe host-side GPU bridge behaviour, namespaces, and telemetry/model lifecycle semantics. -->
<!-- Author: Lukas Bower -->
# GPU Nodes — Out-of-VM Acceleration Strategy

## 1. Rationale
CUDA/NVML stacks are large and platform-specific. Keeping them outside the seL4 guest (whether running on QEMU or physical UEFI hardware) preserves determinism and minimises the trusted computing base (TCB). The Cohesix instance interacts with GPUs exclusively through a capability-guarded 9P namespace mirrored by host workers.
GPU workers (`worker-gpu`) are another worker type under the hive’s Queen, not standalone services.

## 2. Model Lifecycle Surfaces (Milestone 6a)
- Namespace:
  - `/gpu/models/available/<model_id>/manifest.toml` (read-only)
  - `/gpu/models/active` (append-only pointer; host swaps atomically)
- Properties:
  - Manifests live on the **host filesystem**; Cohesix only sees TOML descriptors and the active pointer.
  - Activation is a host concern (reload/restart/hot-swap); no new verbs or control planes were added.
  - WorkerGpu reads `/gpu/models/active` and annotates telemetry with `model_id` / `lora_id` but cannot upload artefacts.
  - `/gpu/models` is published into the live VM by the host GPU bridge via `/gpu/bridge/ctl`; it is absent until the publish step completes.

## 3. Host GPU Worker Architecture
- **Process**: Rust binary running on macOS or a Linux edge node, outside the Cohesix instance, paired with the GPU bridge host.
- **Responsibilities**:
  - Discover GPUs using NVML (Linux) or Metal proxies (macOS, stubbed).
  - Enforce leases that cap memory (MiB), stream counts, and wall-clock TTL.
  - Mirror GPU state into the Cohesix instance by publishing bounded snapshots to `/gpu/bridge/ctl` over the TCP console (queen role); no CUDA/NVML components enter the VM profile or hardware deployment.
- **Safety**: Validate kernel binaries via SHA-256; ensure uploads match expected byte length before dispatch.

## 4. Cohesix Namespace Mapping
| Cohesix Path | Backing Action |
|---------|----------------|
| `/gpu/<id>/info` | Serialize GPU metadata (name, UUID, memory, SM count, driver/runtime versions). |
| `/gpu/<id>/ctl` | Accept textual commands (`LEASE`, `RELEASE`, `PRIORITY <n>`, `RESET`) and return status lines mediated by the bridge host. |
| `/gpu/<id>/lease` | Ticket/lease file gated by host policy; worker-gpu reads to learn active allocations and writes to request renewals. Append-only JSON lines use schema `gpu-lease/v1` (`state=ACTIVE|RELEASED`). |
| `/gpu/<id>/status` | Read-only view of utilisation and recent job summaries sourced from the host; append-only job lifecycle entries and `gpu-breadcrumb/v1` host-run breadcrumbs are included. |
| `/gpu/bridge/ctl` | Append-only publish channel for GPU bridge snapshots (`begin`/`b64:`/`end` lines). |
| `/gpu/bridge/status` | Read-only publish state (`state=idle|receiving|ok|err`). |
| `/gpu/models/*` | Host-mirrored model registry (available + active). |
| `/gpu/telemetry/schema.json` | Telemetry schema descriptor (read-only). |

Note:
- `/gpu/models` and `/gpu/telemetry/schema.json` appear only after a host GPU bridge publish; before that `ls /gpu/models` returns `ERR LS reason=policy detail=invalid-path`.

## 5. Lease Model
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
- `/gpu/<id>/lease` appends JSON lines with schema `gpu-lease/v1` and fields: `schema`, `state`, `gpu_id`, `worker_id`, `mem_mb`, `streams`, `ttl_s`, `priority`. The latest `state=ACTIVE` line indicates the current lease.
- The Queen uses `/queen/ctl` to create GPU workers and manage leases within the same hive orchestration model.

## 6. Job Descriptor Schema
```json
{
  "job": "jid-42",
  "kernel": "vadd",
  "grid": [128, 1, 1],
  "block": [256, 1, 1],
  "bytes_hash": "sha256:...",
  "inputs": ["/bundles/vadd.ptx"],
  "outputs": ["/shard/<label>/worker/<id>/result"],
  "timeout_ms": 5000,
  "payload_b64": "..."
}
```
- Host validates payload hash against staged artefacts before launch; when `payload_b64` is present the bridge decodes and hashes the inline bytes.
- `timeout_ms` triggers job cancellation; status stream records `ERR TIMEOUT` or includes the failure in `/gpu/<id>/status`.
- Successful submissions emit `QUEUED`, `RUNNING`, and `OK` entries in `/gpu/<id>/status` alongside worker telemetry updates.
GPU workers do not schedule hardware directly; they receive tickets and leases from the host over Secure9P, and all scheduling policy (queueing, eviction, throttling) runs on the host side of the bridge.

## 7. Simulation Path (for CI & macOS)
- `gpu-bridge-host --mock --list` emits deterministic namespace descriptors consumed by NineDoor via `install_gpu_nodes`.
- In `dev-virt` QEMU runs without a host bridge, the root-task seeds mock `/gpu/<id>` entries (GPU-0/GPU-1) with `info`, `lease`, and `status` to satisfy CLI demos; `/gpu/models` and `/gpu/telemetry/schema.json` appear only after a host GPU bridge publish.
- `info` returns synthetic GPU entries, `job` triggers precomputed status sequences.
- Enables continuous validation of control plane without real hardware.
- CLI/GUI clients submit GPU jobs via the same verbs exposed through `cohsh` and Secure9P; no separate ad-hoc GPU control protocol exists inside the Cohesix instance.

## 8. Security Notes
- No GPU device nodes or drivers are shipped in the Cohesix instance (including the QEMU development image), and direct device access/virtio-gpu paths are explicitly out of scope; the bridge host terminates DMA and enforces isolation.
- Tickets for `/gpu/*` paths are issued only to `WorkerGpu` roles.
- All control traffic is logged to `/log/queen.log` with ticket IDs for audit.

## LoRA Feedback Loop Walkthrough  
**Jetson Nano → Cohesix Worker → Queen → PEFT/LoRA Farm → Queen → Worker → Jetson Nano**

This walkthrough describes a **pragmatic, end-to-end LoRA optimisation loop** using Cohesix as the **secure control plane**, while keeping CUDA, TensorRT, and training stacks **outside the VM and outside the TCB**.

The design assumes:
- Many **NVIDIA Jetson Nano** devices at the edge
- Each Jetson hosts a **Cohesix Worker VM**
- A single **Cohesix Queen** in the cloud
- An external **PEFT / LoRA training farm** (Kubernetes, Slurm, managed GPUs)

No new IPC mechanisms are introduced. Everything flows through **Secure9P namespaces and files**.

---

## 1. Runtime Inference on Jetson Nano (Outside Cohesix)

**Where inference runs**
- CUDA / TensorRT / PyTorch run on the **Jetson host OS**
- Cohesix never loads CUDA, NVML, or drivers

**Active model**
- Base model + LoRA adapter
- Loaded by the host inference process
- Selected by Cohesix via file pointers (not APIs)

**Why this matters**
- Keeps the Cohesix TCB small
- Allows native Jetson tooling and performance
- Avoids re-implementing ML runtimes

---

## 2. Telemetry Generation (Host → Worker telemetry)  

During inference, the host process emits **summarised telemetry**, not raw data or gradients.

Typical fields:
- Token counts
- Latency histograms
- Confidence / entropy
- Input class distribution
- Drift indicators
- Optional human feedback flags

The host GPU bridge publishes the telemetry schema at `/gpu/telemetry/schema.json` into the VM. Telemetry records themselves are emitted by host-side tooling and forwarded into `/queen/telemetry/*` or worker telemetry streams using Secure9P; no `/gpu/telemetry/*` record files exist inside the VM today.

Properties:
- Append-only, bounded records (CBOR or JSON) emitted by host tooling.
- Tagged with `model_id`, `lora_id`, `device_id`, `time_window`, and `schema_version` (`gpu-telemetry/v1`).
- No streaming, no sockets, no RPC.

### Telemetry Schema (Milestone 6a)
- Descriptor path: `/gpu/telemetry/schema.json` (read-only)
- Version: `gpu-telemetry/v1`
- Required fields:
  - `schema_version`, `device_id`, `model_id`, `time_window`, `token_count`, `latency_histogram`
- Optional fields:
  - `lora_id`, `confidence`, `entropy`, `drift`, `feedback_flags`
- Bounds:
- Max record size: 4096 bytes (enforced by host-side telemetry tooling)
- Append-only writes; emitters must clamp window sizes before writing
- Export:
  - Records may be forwarded to `/queen/telemetry/*` and `/queen/export/lora_jobs/*` by host tools; no in-VM ML stack is introduced.

---

## 3. Worker Collection & Thinning  

Each Jetson runs a **Cohesix Worker** with a role-scoped ticket.

The worker:
- Emits bounded telemetry upstream into `/worker/<id>/telemetry`
- Propagates `model_id` / `lora_id` from `/gpu/models/active` into every forwarded record
- Applies optional thinning / aggregation before forwarding to `/queen/telemetry/*`

/shard/<label>/worker/<id>/telemetry/
window_2025-01-08.cbor

Legacy aliases at `/worker/<id>/telemetry` are available only when `sharding.legacy_worker_alias = true`.

This step is important on Jetson:
- Bandwidth-aware
- Offline-tolerant
- Deterministic memory use

---

## 4. Secure Uplink to the Queen  

The Worker writes telemetry into the Queen namespace via Secure9P using the OS-named ingest surface:

/queen/telemetry/jetson-42/
ctl
seg/seg-000001
latest

Transport characteristics:
- Secure9P over authenticated transport
- msize-bounded frames
- Rate-limited
- Fully auditable (append-only)

If the link drops:
- Telemetry spools locally
- Resumes when connectivity returns

---

## 5. Queen Aggregation & Policy Gating  

The Queen:
- Aggregates telemetry from many workers
- Applies policy:
  - Minimum sample size
  - Drift thresholds
  - Time windows
  - Manual approval (optional)

When criteria are met, the Queen **exports a LoRA training job**:

/queen/export/lora_jobs/job_8932/
telemetry.cbor
base_model.ref
policy.toml

This directory is the **contract boundary** between Cohesix and ML tooling.
Host operators pull this bundle via `coh peft export` before handing it to external training.

---

## 6. External PEFT / LoRA Training (Out of Band)

A LoRA farm watches `/queen/export/lora_jobs/`.

This can be:
- HuggingFace PEFT
- QLoRA
- Axolotl
- Lightning / Accelerate
- Running on Kubernetes, Slurm, or managed GPUs

Cohesix does **not**:
- Run training
- Schedule GPUs
- Manage ML frameworks

It only:
- Supplies telemetry
- Tracks provenance
- Enforces policy

---

## 7. LoRA Artifact Import (Farm → Host Registry)

The training job produces:
- `adapter.safetensors`
- `lora.json` (rank, alpha, target layers)
- Validation metrics

These are staged on the **host filesystem** and surfaced through the GPU model lifecycle view:

/gpu/models/available/llama3-edge-v7/manifest.toml

The manifest records:
- Parent model hash
- Telemetry window used
- Training job ID
- Approval status

---

## 8. Model Distribution to Workers  

Workers observe the active model pointer:

/gpu/models/active -> llama3-edge-v7

---

## 9. Jetson Hot-Swap or Restart  

The host inference process:
- Detects the model pointer change
- Reloads the LoRA adapter (hot-swap or restart)
- Continues inference with the new adapter

Post-deployment telemetry flows immediately, closing the loop.

---

## 10. What Cohesix Provides (and What It Doesn’t)

**Cohesix provides**
- Secure telemetry paths
- Deterministic control plane
- Policy enforcement
- Provenance & audit
- Safe model distribution

**Cohesix deliberately does not**
- Run CUDA
- Train models
- Stream tensors
- Replace ML ecosystems

---

## 11. Minimal Glue Required for Adoption  

To deploy this at scale, only a few thin adapters are needed:

### Host-side
- `gpu-bridge-host`
  - Publishes `/gpu/<id>/*`, `/gpu/models/*`, and `/gpu/telemetry/schema.json` via `/gpu/bridge/ctl`
  - Watches `/gpu/models` (host registry) for active pointer changes
  - Loads LoRA adapters via TensorRT / PyTorch

### Cloud-side
- `coh peft` (host tool)
  - `coh peft export` pulls `/queen/export/lora_jobs/*` into host storage
  - External PEFT job runs out-of-band (HF/Slurm/K8s)
  - `coh peft import` stages adapters into the registry backing `/gpu/models/available/*`

Everything else already exists in the protocol.

---

## 12. Bottom Line

- The Secure9P + namespace model is sufficient
- No protocol changes are required
- The loop scales from 1 Jetson to thousands
- ML teams keep their existing tools
- Cohesix stays small, auditable, and boring — on purpose

That’s exactly what you want for real deployment.

Future work includes ticket arbitration across multiple workers, lease renewal/expiry enforcement, GPU worker lifecycle hooks, and CI coverage of the `/gpu/<id>/` namespace surface.
