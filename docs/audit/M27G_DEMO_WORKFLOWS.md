<!-- Author: Lukas Bower -->
<!-- Purpose: Record the scoped 1.1.0 demo refresh and its host-only validation boundary. -->
<!-- Copyright 2026 Lukas Bower -->

# 1.1.0 demo refresh

```text
Title/ID: m27g-demo-workflows
Milestone: 27g / m27g-assembled-journeys-and-recovery — walkthrough/replay/showcase
Goal: Make the source demos comprehensive and runnable against current 1.1.0-beta interfaces.
Inputs: Current Queen grammar, selected generated policy, HOST_TOOLS, USERLAND_AND_CLI,
        PRIVATE_LORA_RELEASE, ADOPTION, CI_WORKFLOWS and SWARMUI.
Changes:
  - demo/README.md and *.coh — complete operator tour, preconditions, effects and recovery.
  - demo/prepare_worker.py — separate retained approval, strict intent and observation files.
  - demo/host_tools.sh — existing package, CUDA/LoRA, journey and evidence-inspection entrypoints.
  - demo fixtures, scripts/cohsh/run_demo.coh and TEST_PLAN — explicit synthetic proof limits.
  - configs/implementation_surfaces.toml and generated projections — classify source demos
    and fixtures without changing release versions, native contracts or acceptance thresholds.
  - apps/cohsh/tests/demo_runbooks.rs and tests/test_demo_tools.py — focused host contracts.
  - scripts/ci/check_implementation_surfaces.py — require demo commands and assets in the inventory.
Commands:
  cargo build -p cohsh -p coh -p coh-rtc
  cargo test -p cohsh --test demo_runbooks --test script_catalog
  python3 -m unittest discover -s tests -p test_demo_tools.py -v
  python3 scripts/ci/test_check_implementation_surfaces.py
  bash -n demo/host_tools.sh
  cargo fmt --all -- --check
  scripts/check-generated.sh
  scripts/ci/check_test_plan.sh
Checks: Every checked-in demo parses; seven scripts execute against explicit host contracts;
        fixture CLI commands remain runnable; helpers retain request bytes, reject invalid
        inputs/existing outputs, route documented arguments and propagate failures without retries.
Deliverables: Source demos, README, focused tests, regenerated classifications and this record.
```

## Compatibility review

- `cohsh`: existing grammar, 256-statement bound, selected policy, credential
  precedence and single-console ownership preserved. ACK assertions are separate
  from payload conditions; lifecycle refusal is checked by preserved DRAINING
  state and the exact retained lease, because host error prose omits the target's
  detailed refusal reason.
- `coh`: existing providers, package verifier, CUDA recipe, native PEFT release
  and evidence APIs only. No fabricated adapter, result, signature or host ticket.
- `hive-gateway`, `gpu-bridge-host`, `host-ticket-agent`,
  `host-sidecar-bridge` and `sidecar-bus`: deployment remains operator-owned;
  scripts use the existing gateway and enrolled executor. No service configuration
  or runtime change.
- SwarmUI: README uses the current Operations, Namespaces, Tickets & policy,
  Evidence, Run story, GPU flight deck and Replay views. No UI code change.
- `coh-status` and `cas-tool`: catalog reports actual installation availability;
  no control/status or CAS contract change.
- `tools/cohesix-py`: journey entrypoint and existing CUDA/LoRA preparation examples
  retained; only compiler-owned provider bindings regenerate. No SDK API change.
- Performance scripts and benchmark reports: no workloads, schemas, bounds or
  thresholds change. Demonstrations do not supply performance evidence.
- Existing release directories and their contents remain immutable; these are
  source demos for the reserved 1.1.0-beta line, not a release cut.

## Evidence limits

The focused host-model execution checks the real host namespace/parser contracts,
not seL4 scheduling, physical drivers, target Worker execution or native CUDA/HF.
The basic CLI mock omits host/policy/audit namespaces; the dedicated host test
enables those contracts explicitly without changing production defaults.

No fresh QEMU, Pi, native GPU/LoRA, full workspace, burn-in, staged release or
human-review acceptance is claimed. The M27g milestone remains In Progress.
Command logs are retained under `out/demo-1.1.0/` in the demo worktree.
