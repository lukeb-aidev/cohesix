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
  TOOLCHAIN_MAC_ARM64.md
  USERLAND_AND_CLI.md
/scripts/
  qemu-run.sh
  ci/
    size_guard.sh
/toolchain/
  setup_macos_arm64.sh
/apps/
  root-task/
    README.md            ← Event pump overview, testing commands, and feature flag notes
  nine-door/
  worker-heart/
  worker-gpu/            (future)
  gpu-bridge-host/       (host-only tools)
/tests/
  integration/
```

## Layout Principles
- **Docs-first**: Any new crate, script, or interface requires accompanying documentation under `/docs`.
- **Role-labelled crates**: Worker crates encode their role in the crate name to simplify CI filtering.
- **Host vs VM split**: Host-only tools live under `/apps/gpu-bridge-host` or `/tools/` and must never be packaged into the VM CPIO.
- **CI expectations**: `/tests/integration` houses black-box tests that launch QEMU using mock assets; unit tests live beside their crates.
