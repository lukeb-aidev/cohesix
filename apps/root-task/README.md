<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document the as-built root-task scope, target closures, and verification commands. -->
<!-- Author: Lukas Bower -->
# Root Task Crate

`root-task` is the initial pure-Rust seL4 task for the selected Cohesix QEMU
and Pi 4 profiles. It owns bootstrap and policy, admits manifest-declared
resources through HAL, launches restricted service and Worker children, and
projects their bounded results into the authenticated operator surfaces.
Physical steady-state device work stays in isolated driver runtimes.

The two operational feature closures are explicit and default features are
empty:

- `release-qemu`: four-core `aarch64/virt`, MCS, GICv3, exported virtual
  counter, PL011 emergency serial, VirtIO console networking, and the selected
  service/Worker images.
- `release-pi4`: four-core BCM2711 MCS, selected generated timer truth,
  emergency serial, and manifest-declared Pi driver/service/Worker images.

`dev-virt`, timer bypasses, mock seL4, early exits, and diagnostic driver
smokes are explicit non-production features. They cannot satisfy target,
integration, release, or use-case evidence.

## Production fail-closed behavior

An operational boot requires real generated timer frequency and an admitted
serial backend. Userland no longer substitutes a null serial driver. The
early console advertises only implemented pre-authentication diagnostics and
returns a typed authorization refusal for `reboot`; authenticated reboot is
handled only by the event pump. Target namespace state begins unavailable and
empty until a validated provider snapshot arrives. The unused IPC trace alias
and legacy PL011 bootstrap REPL are not selected.

## Build and checks

Use the repository build runner so seL4 profile validation, child-image
packaging, exact GIC selection, and system-CPIO construction stay aligned:

```sh
scripts/cohesix-build-run.sh --clean --no-run --cargo-target aarch64-unknown-none
```

Focused source checks:

```sh
cargo test -p root-task --test production_fallbacks
cargo test -p root-task --tests
SEL4_BUILD_DIR="$PWD/out/sel4/profile-v2/qemu-smp-production" \
  cargo check --locked --target aarch64-unknown-none \
  -p root-task --no-default-features --features release-qemu
```

QEMU source/build success is not Pi 4 hardware acceptance. Pi tests follow
only after the M26e QEMU closure is complete and frozen.

See [ARCHITECTURE.md](../../docs/ARCHITECTURE.md),
[ROLES_AND_SCHEDULING.md](../../docs/ROLES_AND_SCHEDULING.md), and
[TEST_PLAN.md](../../docs/TEST_PLAN.md) for the generated topology and evidence
requirements.
