<!-- Author: Lukas Bower -->
<!-- Purpose: Document the canonical Cohesix repository layout and app roster. -->
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
  TOOLCHAIN_MAC_ARM64.md
  USERLAND_AND_CLI.md
  snippets/
    root_task_manifest.md
/configs/
  root_task.toml
/out/
  manifests/
    root_task_resolved.json
/scripts/
  qemu-run.sh
  ci/
    size_guard.sh
/toolchain/
  setup_macos_arm64.sh
/tools/
  coh-rtc/
/apps/
  cohesix-proto/
  console-ack-wire/
  cohsh/
  root-task/
    README.md            ← Event pump overview, testing commands, and feature flag notes
  nine-door/
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
