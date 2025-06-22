// CLASSIFICATION: COMMUNITY
// Filename: README.md v0.16
// Author: Lukas Bower
// Date Modified: 2026-07-10


# Cohesix

Cohesix is a self‑contained, formally verified operating‑system and compiler suite designed for secure, scalable execution on edge and wearable devices.

Why Cohesix? seL4 proofs guarantee strong isolation, cold boot completes in under 200 ms with GPU offload latency below 5 ms, dynamic 9P namespaces expose services like `/sim/` and `/srv/cuda`, and the BusyBox userland keeps the toolchain familiar.

---

## 🔍 Overview

Cohesix combines a micro‑kernel architecture (seL4‑derived) with Plan 9‑style namespaces, a distributed compiler tool‑chain, and a cloud‑edge orchestration model. Built‑in telemetry, simulation via Rapier, and a role‑based trust model make it ideal for mission‑critical, privacy‑sensitive deployments.

### Key Features
- **Formally verified kernel** with provable isolation
- **9P namespace** for uniform resource access
- **Physics‑aware simulation** (Rapier) for Worker nodes
- **Queen–Worker protocol** for secure lifecycle modules (SLMs)
- **Multi‑language tool‑chain** (Rust, Go, Codex shell)
- **Modular boot & sandboxing** with trace validation
- **Joystick input** via SDL2 for interactive demos

  - **Trace-first validation** with CI-enforced snapshots and syscall replay

---

## 📚 Documentation

Community documents live in `docs/community/`, while private strategy files are under `docs/private/`.

| Path | Purpose |
|------|---------|
| `docs/community/MISSION_AND_ARCHITECTURE.md` | Philosophy and architecture overview |
| `docs/community/INSTRUCTION_BLOCK.md` | Canonical workflow rules |
| `PROJECT_MANIFEST.md` | Consolidated changelog, metadata, and OSS dependencies |
| `docs/private/COMMERCIAL_PLAN.md` | Market & investor messaging (restricted) |
| `docs/security/THREAT_MODEL.md` | Security assumptions and threat surfaces |
| `docs/security/SECURITY_POLICY.md` | Defense strategy, mitigations, secure boot |

| `docs/community/governance/LICENSES_AND_REUSE.md` | SPDX matrix and OSS reuse policy |
| `docs/community/governance/ROLE_POLICY.md` | Role manifest and execution policy |
| `docs/community/cli/README.md` | CLI and agent command index |

---

## 🚀 Getting Started

Clone, then hydrate missing artifacts.

Requires Rust **1.76** or newer (2024 edition).

```bash
git clone https://github.com/<user>/cohesix.git
cd cohesix
./scripts/run-smoke-tests.sh   # quick health check
make all                       # Go vet + C shims
cargo check --workspace        # Rust build
make go-test                  # Go unit tests (cd go && go test ./...)
./test_all_arch.sh             # run Rust, Go, and Python tests

```

To regenerate compiler/OS stubs:

```bash
./hydrate_cohcc_batch5.sh
```

All major commands emit validator-compatible logs and snapshots to `./log/trace/` and `./history/snapshots/`.

Or explore runtime scenarios with the Codex CLI tools:

``` 
cohbuild, cohrun, cohtrace, cohcap — see cli/README.md for usage by role
```

### Demo Scaffolds

Initial demo services are enabled:

* `/srv/webcam` and `/srv/gpuinfo` for workers
* `cohrun physics_demo` to run a Rapier simulation
* `cohtrace list` to view joined workers
* Optional Secure 9P server with TLS via `--features secure9p` (see `config/secure9p.toml`)
* Copy `etc/init.conf.example` to `/etc/init.conf` and adjust values to control startup behavior

### Running the GUI Orchestrator

Start the lightweight web UI to inspect orchestration state:

```bash
go run ./go/cmd/gui-orchestrator --port 8888 --bind 127.0.0.1
```
Example output:

```
GUI orchestrator listening on 127.0.0.1:8888
{"uptime":"1h","status":"ok","role":"Queen","workers":3}
```


## 🧪 Testing

Run unit tests before submitting pull requests:

```bash
cargo test --workspace
cd go && go test ./...
# or
GOWORK=$(pwd)/go/go.work go test ./go/...
```

Run `cohtrace diff` to compare validator snapshots between runs:
```bash
./target/debug/cohtrace diff --from last --to previous
```

## Environment Flags

The helper script `cohesix_fetch_build.sh` sets two variables after cloning:

* `COH_PLATFORM` – the host architecture from `uname -m`
* `COH_GPU` – `1` if an NVIDIA device is accessible, else `0`

CUDA tests and builds skip when `COH_GPU=0`.

### Building initfs.img

The initramfs provides early boot utilities. First build BusyBox:

```bash
./scripts/build_busybox.sh $(uname -m)
```

Copy `out/bin/busybox` and the scripts under `userland/miniroot/bin/` into a
staging directory. From that directory run:

```bash
find . | cpio -o -H newc | gzip > ../../initfs.img
```

Ensure the archive includes at minimum `busybox`, `init`, `rc`, `echo`, `ls` and
`help`. The `cpio` and `gzip` tools are required.

## Boot Testing

Confirm QEMU and EFI dependencies with:

```bash
./scripts/check-qemu-deps.sh
```

The script highlights missing packages so you can install them before running boot tests.
---

## 🧠 Learn More

* [Cohesix Project Philosophy](docs/community/MISSION_AND_ARCHITECTURE.md)
* [Technical Deep‑Dive](docs/community/MISSION_AND_ARCHITECTURE.md)
* [Canonical Workflows](docs/community/INSTRUCTION_BLOCK.md)
