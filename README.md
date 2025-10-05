// CLASSIFICATION: COMMUNITY
// Filename: README.md v0.26
// Author: Lukas Bower
// Date Modified: 2029-02-22


# Cohesix

Cohesix is a self‑contained, formally verified operating‑system and compiler suite designed for secure, scalable execution on edge and wearable devices.

Why Cohesix? seL4 proofs guarantee strong isolation, cold boot completes in under 200 ms, dynamic 9P namespaces expose services like `/sim/` and `/srv/cuda`, and the BusyBox userland keeps the toolchain familiar. Cohesix now positions itself as the verifiable control plane that orchestrates external CUDA farms through tamper-evident Secure9P workflows instead of promising on-device GPU execution.

---

## 🔍 Overview

Cohesix combines a micro‑kernel architecture (seL4‑derived) with Plan 9‑style namespaces, a distributed compiler tool‑chain, and a cloud‑edge orchestration model. Built‑in telemetry, simulation via Rapier, and a role‑based trust model make it ideal for mission‑critical, privacy‑sensitive deployments. The operating system now acts as a zero-trust GPU control plane that fronts dedicated Linux CUDA microservers, ensuring every remote GPU job is scheduled, validated, and archived through trace-first governance.

### Key Features
- **Formally verified kernel** with provable isolation
- **9P namespace** for uniform resource access
- **Remote CUDA governance** via Cohesix CUDA Servers managed as a zero-trust annex
- **Physics‑aware simulation** (Rapier) for Worker nodes
- **Queen–Worker protocol** for secure lifecycle modules (SLMs)
- **Multi‑language tool‑chain** (Rust, Go, Codex shell)
- **Modular boot & sandboxing** with trace validation and replayable audit trails

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
cargo build --release \
  # CUDA workloads run remotely via Secure9P; ensure /srv/cuda points at a Cohesix-managed microserver
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

* `/srv/camera` (9P stream) and `/srv/gpuinfo` sourced from Cohesix CUDA Servers
* `cohrun physics_demo` to run a Rapier simulation
* `cohtrace list` to view joined workers
* Optional Secure 9P server with TLS via `--features secure9p` (see `config/secure9p.toml`).
  TLS certificates are pinned and client auth can be enforced across the Queen-to-microserver boundary.
* Copy `etc/init.conf.example` to `/etc/init.conf` and adjust values to control startup behavior

### Plan9 Physics Server

Use `scripts/mount_secure9p.rc` to mount the job directory with mutual TLS. Then run:

```rc
physics-server &
```

Drop `physics_job_*.json` files into `/mnt/physics_jobs` and inspect `/sim/world.json` for results. `/srv/physics/status` reports server progress.

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
./tools/setup_test_targets.sh    # install cross targets if missing
```

Run `cohtrace diff` to compare the two most recent validator snapshots:
```bash
./target/debug/cohtrace diff
```

Inspect orchestration state and worker health with `cohtrace cloud`:
```bash
./target/debug/cohtrace cloud
```

## Environment Flags

The helper script `cohesix_fetch_build.sh` sets two variables after cloning:

* `COH_PLATFORM` – the host architecture from `uname -m`
* `/srv/cuda` – contains the remote Secure9P address for CUDA jobs

All CUDA execution occurs on the remote server referenced by `/srv/cuda`.

### Preparing dependencies for `cohesix_fetch_build.sh`

`cohesix_fetch_build.sh` now assumes the seL4 artefacts and Git submodules are
populated before execution. This avoids network fetches on macOS and other
restricted environments. Run the following commands from the repository root
after cloning:

```bash
./third_party/seL4/fetch_sel4.sh --non-interactive
git submodule update --init --recursive  # if .gitmodules exists
```

Verify that `third_party/seL4/lib/libsel4.a` and the header directory under
`third_party/seL4/include/` exist before invoking the build script. When these
artefacts are absent the script will exit with guidance so you can rerun the
preparation steps.

### UEFI SSE Requirements

UEFI builds for the `x86_64-unknown-uefi` target rely on SSE and SSE2
instructions. These features are explicitly enabled in `.cargo/config.toml`
to satisfy compile-time checks in crates like `ring` and to prevent
runtime crashes on processors where the instructions are available.

### Building the seL4 entry binary

The optional `sel4_entry_bin` feature compiles `src/bootstrap/sel4_entry.rs` into
an ELF used for kernel-level boot testing. Regular workspace builds exclude this
binary:

```bash
cargo build --workspace --all-targets --all-features
```

To produce the seL4 entry binary, enable the feature explicitly:

```bash
cargo build --workspace --all-targets --all-features --features sel4_entry_bin
```

### Cross-building the root task

Build the sel4-sys crate and root ELF from the workspace root:

```bash
cd ~/cohesix/workspace
cargo clean
SEL4_INCLUDE=$(realpath ../third_party/seL4/include) SEL4_ARCH=aarch64 \
  cargo +nightly build -p sel4-sys --release \
    --target=cohesix_root/sel4-aarch64.json

cd ~/cohesix/workspace
cargo +nightly build -p cohesix_root --release \
  --target=cohesix_root/sel4-aarch64.json
```
The resulting binary appears under `target/sel4-aarch64/release/`.

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

Confirm QEMU dependencies with:

```bash
./scripts/check-qemu-deps.sh
```

The script highlights missing packages so you can install them before running boot tests.

Build the GRUB-based ISO:

```bash
./tools/make_iso.sh
```

Run the QEMU boot check to verify the GRUB → seL4 → Cohesix flow:

```bash
ci/qemu_boot_check.sh
```
## 📦 Release Process

Use the cargo-release tool to tag and publish new versions:

```bash
cargo release patch
```

This command updates crate versions, the changelog, and creates the Git tag `v<version>`.

---

## 🧠 Learn More

* [Cohesix Project Philosophy](docs/community/MISSION_AND_ARCHITECTURE.md)
* [Technical Deep‑Dive](docs/community/MISSION_AND_ARCHITECTURE.md)
* [Canonical Workflows](docs/community/INSTRUCTION_BLOCK.md)
