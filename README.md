<!-- Author: Lukas Bower -->
# Cohesix

Cohesix is a deterministic orchestration environment built on the seL4 microkernel for ARM64 virtual machines. The project delivers a pure Rust userspace that exposes all control and telemetry through a Secure9P-backed file namespace instead of traditional Unix facilities.

## Vision
- **Deterministic control plane**: Replace shell scripts and ad-hoc RPCs with append-only JSON command streams handled by the NineDoor 9P server.
- **Role-oriented governance**: Encode every interaction through capability tickets that enforce the roles and budgets defined in the scheduling policy.
- **Host/VM separation**: Keep heavyweight GPU and tooling stacks on the host while the VM focuses on minimal, auditable services.

## Key Components
- **Root Task (`apps/root-task`)** — boots the system, manages capabilities, and enforces worker budgets.
- **NineDoor (`apps/nine-door`)** — Secure9P server that publishes the queen, worker, and telemetry namespaces.
- **Worker Suites (`apps/worker-heart`, future `apps/worker-gpu`)** — role-specific agents launched via queen commands.
- **GPU Bridge (`apps/gpu-bridge-host`)** — host-side toolchain that mirrors GPU control surfaces into the VM through Secure9P.
- **Cohesix Shell (`cohsh`)** — operator CLI defined in `docs/USERLAND_AND_CLI.md`, translating shell-like commands into 9P operations.

## Getting Started
1. Review the architectural blueprint in `docs/ARCHITECTURE.md` and the repository expectations in `docs/REPO_LAYOUT.md`.
2. Follow `docs/BUILD_PLAN.md` to implement milestones sequentially, ensuring code and documentation advance together.
3. Use the tooling instructions in `docs/TOOLCHAIN_MAC_ARM64.md` to prepare the macOS ARM64 development environment.

## Build & Run (QEMU)
The `scripts/cohesix-build-run.sh` helper automates the full build pipeline for the
Rust workspace and packages the resulting binaries together with a pre-built seL4
kernel image. The script expects an existing seL4 build tree such as the one
described in `docs/TOOLCHAIN_MAC_ARM64.md`.

```bash
# Build every Cohesix component, assemble the payload CPIO, and boot QEMU
scripts/cohesix-build-run.sh \
  --sel4-build "$HOME/seL4/build" \
  --out-dir out/cohesix \
  --profile release \
  --cargo-target aarch64-unknown-none

# To inspect artefacts without starting QEMU
scripts/cohesix-build-run.sh --no-run
```

The script emits a manifest (`out/cohesix/staging/cohesix/manifest.json`) with
SHA-256 digests for every packaged binary and reuses `scripts/ci/size_guard.sh`
to enforce the 4 MiB CPIO size budget.

The helper builds host tooling (such as `cohsh` and `gpu-bridge-host`) for the
native Rust target while compiling seL4 payloads with the provided
`--cargo-target`. Supplying the seL4 triple is therefore mandatory whenever the
rootserver image is required.

## Documentation Map
- Interfaces and control schemas: `docs/INTERFACES.md`
- Roles and scheduling policy: `docs/ROLES_AND_SCHEDULING.md`
- Secure9P design and testing expectations: `docs/SECURE9P.md`
- GPU integration strategy: `docs/GPU_NODES.md`

Each milestone must maintain alignment with these documents to keep the Cohesix platform cohesive, auditable, and ready for future GPU-enabled workloads.
