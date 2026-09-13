<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document the canonical Cohesix repository layout and app roster. -->
<!-- Author: Lukas Bower -->
# Repository Layout — 1.0.0-beta

```
/AGENTS.md              ← Repo-wide working agreement
/docs/
  ARCHITECTURE.md
  BUILD_PLAN.md
  CODING_GUIDELINES.md
  GPU_NODES.md
  INTERFACES.md
  ROLES_AND_SCHEDULING.md
  SECURE9P.md
  TEST_PLAN.md
  TOOLCHAIN_MAC_ARM64.md
  USERLAND_AND_CLI.md
  snippets/
    cohsh_client.md
    cohsh_grammar.md
    cohsh_policy.md
    cohsh_ticket_policy.md
    root_task_manifest.md
    ticket_quotas.md
/configs/
  root_task.toml
  generated/
    root_task_resolved.json
    coh_policy.toml
    cohsh_policy.toml
/releases/              ← Current 1.0.0-beta bundles and release notes
/seL4/                  ← Required immutable prebuilt artifacts; upstream source is external
/out/                   ← Ignored disposable build, staging, log, and run output
/scripts/
  qemu-run.sh
  cohsh/
    cas_roundtrip.coh
    observe_watch.coh
  ci/
    size_guard.sh
/toolchain/
  setup_macos_arm64.sh
/tools/
  coh-rtc/
/crates/
  cohsh-core/
  host-cuda/
/apps/
  cohesix-proto/
  console-ack-wire/
  cohsh/
    src/
      client.rs
      queen.rs
  coh-status/             ← Library/replay surface; no standalone release CLI
  root-task/
    README.md            ← Event pump overview, testing commands, and feature flag notes
  nine-door/
    src/
      host/
        cbor.rs          ← Minimal CBOR writer for UI providers
        ui.rs            ← UI provider config + path matching
    tests/
      ui_providers.rs    ← UI provider bounds + audit tests
  nine-door-runtime/      ← Selected no_std target namespace-service child
  console-network-runtime/← Selected no_std target TCP console-network child
  worker-heart/
  worker-gpu/
  worker-lora/
  pi4-driver-runtime/
  gpu-bridge-host/       (host-only tools)
/tests/
  integration/
```

## Layout Principles
- **Docs-first**: Any new crate, script, or interface requires accompanying documentation under `/docs`.
- **Role-labelled crates**: Worker crates encode their role in the crate name to simplify CI filtering.
- **Host vs VM split**: Host-only tools live under `/apps/gpu-bridge-host` or `/tools/` and must never be packaged into the VM CPIO.
- **Evidence ownership**: The staged [Test Plan](TEST_PLAN.md) selects host, QEMU, and physical Pi checks. Fixtures and host tests do not establish target acceptance.

## Build and release ownership

[HOST_TOOLS.md](HOST_TOOLS.md#release-factory) describes the native Mac and Linux
builds and `scripts/release_bundle.sh`. [HARDWARE_BRINGUP.md](HARDWARE_BRINGUP.md)
owns Pi image composition and media installation. The selected manifests and
compiler-generated inventory determine each target and bundle's contents.

The current tree retains the 0.9.0-beta and 1.0.0-beta distributions. Earlier
distributions remain unchanged at their original Git tags. Retained
kernel artifacts, firmware, pinned dependencies, test fixtures, and audit
records still serve current build or evidence workflows; age alone does not
make them disposable. Put temporary probes and local reports under `out/`.
