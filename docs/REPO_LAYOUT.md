<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document the canonical Cohesix repository layout and app roster. -->
<!-- Author: Lukas Bower -->
# Repository Layout Blueprint

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
/configs/
  root_task.toml
/out/
  manifests/
    root_task_resolved.json
/scripts/
  qemu-run.sh
  cohsh/
    cas_roundtrip.coh
    observe_watch.coh
  regression/
    client_vs_console.sh
    transcript_compare.sh
    transcript_diff.sh
  ci/
    convergence_tests.sh
    size_guard.sh
/toolchain/
  setup_macos_arm64.sh
/tools/
  coh-rtc/
/crates/
  cohsh-core/
/apps/
  cohesix-proto/
  console-ack-wire/
  cohsh/
    src/
      client.rs
      queen.rs
  coh-status/
  root-task/
    README.md            ← Event pump overview, testing commands, and feature flag notes
  nine-door/
    src/
      host/
        cbor.rs          ← Minimal CBOR writer for UI providers
        ui.rs            ← UI provider config + path matching
    tests/
      ui_providers.rs    ← UI provider bounds + audit tests
  worker-heart/
  worker-gpu/
  gpu-bridge-host/       (host-only tools)
/tests/
  integration/
```

## Layout Principles
- **Docs-first**: Any new crate, script, or interface requires accompanying documentation under `/docs`.
- **Role-labelled crates**: Worker crates encode their role in the crate name to simplify CI filtering.
- **Host vs VM split**: Host-only tools live under `/apps/gpu-bridge-host` or `/tools/` and must never be packaged into the VM CPIO.
- **CI expectations**: `/tests/integration` houses black-box tests that launch QEMU using mock assets; unit tests live beside their crates.

## Milestone 7 Developer Workflow
- **Root task event pump**: Implementations replacing the legacy spin
  loop must update `apps/root-task/README.md` alongside code changes and
  document new handlers under `docs/ARCHITECTURE.md §10`. Tests live in
  `apps/root-task/tests/` and are executed with
  `cargo test -p root-task event_pump` and
  `cargo test -p root-task console_auth`.
- **Networking feature flag**: Networking remains behind
  `--features net`. When modifying `apps/root-task/src/net`, run
  `cargo check -p root-task --features net` and `cargo clippy -p
  root-task --features net --tests`; record the commands in commit and PR
  notes.
- **Console transports**: `apps/cohsh/src/transport` houses serial,
  mock, and TCP adapters. The TCP client introduced in Milestone 7c must
  stay feature gated; update `docs/USERLAND_AND_CLI.md` whenever verbs or
  flags change.
- **Integration harness**: `tests/integration/qemu_tcp_console.rs`
  exercises the Milestone 7 flow end-to-end. Use
  `scripts/qemu-run.sh --console serial --tcp-port <port>` while running
  the test to confirm QEMU boot logs advertise the expected
  `event-pump` activation lines and that the TCP transport remains
  responsive.
