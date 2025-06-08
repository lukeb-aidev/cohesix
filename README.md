// CLASSIFICATION: COMMUNITY
// Filename: README.md v0.6
// Author: Lukas Bower
// Date Modified: 2025-07-11


# Cohesix

Cohesix is a self‑contained, formally verified operating‑system and compiler suite designed for secure, scalable execution on edge and wearable devices.

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

---

## 📚 Documentation

Community documents live in `docs/community/`, while private strategy files are under `docs/private/`.

| Path | Purpose |
|------|---------|
| `docs/community/MISSION.md` | Project philosophy and goals |
| `docs/community/PROJECT_OVERVIEW.md` | Architecture & roadmap |
| `docs/community/INSTRUCTION_BLOCK.md` | Canonical workflow rules |
| `docs/private/COMMERCIAL_PLAN.md` | Market & investor messaging (restricted) |

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

Or explore runtime scenarios with the Codex CLI tools:

``` 
cohbuild, cohrun, cohtrace, cohcap
```

### Demo Scaffolds

Initial demo services are enabled:

* `/srv/webcam` and `/srv/gpuinfo` for workers
* `cohrun physics_demo` to run a Rapier simulation
* `cohtrace list` to view joined workers

## 🧪 Testing

Run unit tests before submitting pull requests:

```bash
cargo test --workspace
cd go && go test ./...
# or
GOWORK=$(pwd)/go/go.work go test ./go/...
```

---

## 🧠 Learn More

* [Cohesix Project Philosophy](docs/community/MISSION.md)
* [Technical Deep‑Dive](docs/community/PROJECT_OVERVIEW.md)
* [Canonical Workflows](docs/community/INSTRUCTION_BLOCK.md)
