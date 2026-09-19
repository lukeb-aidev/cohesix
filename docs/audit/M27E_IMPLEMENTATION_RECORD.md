<!-- Author: Lukas Bower -->
<!-- Purpose: Bind installation and CI completion to exact signed host packages, clean SDK installs and independently verified workflow outcomes. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Installation, adoption and CI implementation record

**Status: Complete — 20 September 2026.**

Title/ID: `m27e-packages-doctor-and-newcomer-path` and
`m27e-ci-reconciliation-and-verified-gate`.
Milestone: 27e — Installation, Adoption and CI.
Goal: Deliver the accepted CUDA/LoRA lifecycle through signed installable tools,
explicit deployment inputs and ordinary CI that succeeds only on verified outcomes.
Inputs: completed 27b–27d, package/source registries, retained independently enrolled
native CUDA and LoRA evidence, macOS ARM64 and Linux AArch64 production host builds.
The owner's 20 September request authorizes implementation, focused completion
checks and commit/push to main. The full suite and integrated release campaign are
outside this component claim.

## Changes and ownership

- The selected Mac controller, Linux ARM64 controller and Linux ARM64 CUDA profiles
  advance exact package membership to `1.1.0-beta`. Signed membership includes the
  Python sources, explicit wheel/sdist builder, adoption/evidence/recipe guides,
  native preparation examples and maintained CI example. The prior x86 profile
  remains unchanged. Package version owns this file-set change; the existing
  package format and VM manifest/ABI remain unchanged.
- `cohesix.journey` / `cohesix-journey` derives identity from workflow purpose,
  immutable workload semantics and explicit rerun intent. The original admission
  and existing controller journal live on operator-provisioned durable storage.
  Atomic binding/plan records and a process lock prevent changed admission or
  missing-state retries from silently starting another operation.
- Fixed lifecycle calls resolve fresh authority before continuation and reuse the
  existing Rust journals and signed verifier. Only requested verified completion
  returns zero. Submission, ambiguity, cancellation, failure, recovered failure,
  timeout and invalid evidence have distinct nonzero exits and evidence references.
- Doctor separates local controller/package, secret/expiry and planner/cache/evidence
  checks from remote target, CUDA executor, native runtime and resource observations.
  An unobserved remote boundary remains explicitly unobserved with remediation.
  It never declares remote readiness from local installation success.
- The public staging entrypoint copies only registered sources/binaries. Python
  wheels and sdists are checked derivatives of signed source packages, not falsely
  described as independently release-signed wheels. Optional provider libraries
  are unnecessary for the selected controller SDK.
- Native LoRA service enrollment selects the existing executor configuration and
  dedicated user's systemd bus; independent native/Worker custody and state-root
  confinement are required. No executor, authority path or VM listener was added.
- The maintained ordinary CI example isolates untrusted source validation from an
  explicitly protected enrolled runner. Production uses installed tools, persistent
  config/state and scoped secret references; it checks out no PR code.

## Focused checks and evidence

All local logs and component artifacts are retained under `out/m27e/`. The native
source and package records live under
`/mnt/nvme/cohesix-dev/m27e-host-source-01/out/` on the reference Linux AArch64 host;
collected copies accompany the local record. These are component package and
controller contracts, not a new public release or physical Pi qualification.

Commands and results:

- `cargo test --locked -p coh --lib package::tests::`: four signed-inventory,
  source/profile/schema/SBOM, native format, embedded-secret and filesystem-boundary
  tests pass (`package-rust-tests-01.log`).
- `python -m pytest -q tests/test_host_package_stage.py tests/test_host_services.py tests/test_python_package.py`:
  eleven exact-membership/archive, service privilege/device/PEFT enrollment and
  staging-confinement tests pass (`package-python-tests-03.log`).
- `PYTHONPATH=tools/cohesix-py python -m unittest discover -s tools/cohesix-py/tests -p test_journey.py`:
  eleven deterministic tests pass for immutable identity, changed-input/rerun identity,
  actual output requirements, ACK/rollback non-success, changed admission, lost
  journal, runner replacement after lost ACK, independent-verifier refusal,
  injected bounded time, private state, strict input, fresh authority and honest doctor boundaries
  (`journey-tests-final.log`). These mock only the fixed subprocess boundary; they
  do not claim native execution or target scheduling.
- `cargo test --locked -p coh --test recipe --test peft_release`: fifteen existing
  deterministic lifecycle tests pass (`lifecycle-tests-01.log`), preserving the
  underlying intent-before-dispatch, phase uncertainty, reuse, comparison,
  compensation and exact verification contracts.
- Exact macOS release build: `cargo build --locked --release -p coh -p cohsh
  -p hive-gateway -p host-ticket-agent -p host-sidecar-bridge -p sidecar-bus
  --features coh/fuse,sidecar-bus/live,sidecar-bus/modbus,sidecar-bus/dnp3`.
  Linux AArch64 adds `-p gpu-bridge-host` and `gpu-bridge-host/rest`.
  Final logs: `mac-build-final/build.log`, `native-build-final.log`. Both select the accepted
  production `release-a-epoch7-v127.toml` and retain generated overlays. The unchanged
  native CUDA helper retains its original qualified build identity.
- Signed package build/verify/install, duplicate-destination refusal, unexpected
  file, changed content, wrong source enrollment, package doctor and native service
  rendering pass for all three selected profiles. macOS plists pass `plutil -lint`;
  Linux units pass `systemd-analyze verify`. Services are not activated by installation.
- Fresh installed-source wheel/sdist builds and clean venv wheel installs pass on
  both hosts. Isolated `python -I` imports resolve only the installed SDK. The
  installed `cohesix-journey --help` and packaged `coh plan cuda-reference --recipe`
  work outside the repository (`mac-clean-final`, `native-python-clean-final.log`).
- The fresh installed SDK invokes the packaged Rust verifier on the unchanged
  accepted M27c CUDA and M27d native-import evidence. Both return `verified`.
  The retained M27d forced-canary case returns `recovered_failure` and exit 15.
  Original operation, ticket, native source, Worker, graph and trust identities
  are preserved (`mac-clean-final/workflow-results.json`). No evidence was fabricated,
  re-signed or reclassified as new execution. This reuses the accepted native owners
  while testing their installed CLI/Python consumption. Full assembled fresh
  workflows and release qualification remain 27g.
- Generated consistency and Test Plan integrity pass. No root/Worker/driver runtime
  change requires new QEMU/Pi execution: the changed Rust registry string is under
  `cfg(feature = "std")`; the no_std compatibility vocabulary is unchanged.

## Clean-environment walkthrough and first-use findings

The walkthrough uses fresh package/install directories, fresh Python environments,
shipped source manifests/guides/entrypoints and explicit enrolled workload/evidence
inputs. It is not an external-user study. The model, native CUDA runtime, target
and accepted evidence are provisioned dependencies, not artifacts claimed to be
installed by the controller package. The clean controller verifies their original
outcomes without running repository Python code or patching shipped sources.

The walkthrough found and resolved missing LoRA helper/example package membership,
missing selected workflow/CI instructions, missing PEFT service selection, the confined user-manager socket mounts, and the
placement of the global `coh --policy` flag in the new instructions. The local
system Python lacked pytest; a private build/test venv supplies it and the wheel
build prerequisites without changing system Python. An initial packaging diagnostic
passed the source-digest tool's `sha256:` prefix where the CLI requires lowercase
hex; the builder correctly refused it. Failed setup logs remain retained.

Doctor deliberately reports remote health as `not_observed` until the operator
performs the published target/native checks. The signed package and local CLI do
not grant host privileges, issue approval/tickets, install a GPU driver/model,
activate services, or establish production device attestation.

## Compatibility review

- `coh`, `cohsh`, gateway, host-ticket agent, GPU and sidecar bridges: selected
  production builds and exact package policies/registries remain aligned. Existing
  executable paths and on-wire behavior are unchanged. The service renderer adds
  explicit optional PEFT enrollment to the existing ticket agent.
- Python: new fixed journey entrypoint calls the same Rust lifecycle/verifier;
  the explicit distribution now contains the native helper and selected examples.
  Optional provider imports remain lazy; clean base SDK install has no dependencies.
- Root/cohsh console help, shared manual and SwarmUI help were reviewed. Existing
  manuals already route host CUDA/LoRA commands to their updated host guides.
  New Python CLI help documents its own host-only commands; there is no new VM or
  SwarmUI console verb or native UI claim. USERLAND and Quickstart link adoption.
- CAS, evidence/attestation, audit/identity, fleet, exporters, other host utilities
  and `tools/cohesix-py` evidence parsers retain their schemas and authority. Package
  trust remains distinct from independently enrolled native/Worker evidence trust.
- Compiler outputs are regenerated from the two source registries. Root manifest,
  target build outputs and no_std policy vocabulary are unchanged.
- Benchmark scripts, workload parameters, raw TCP authority, report schemas and
  acceptance thresholds require no change. Installation/CI checks are not
  performance qualification. No immutable `releases/` file is modified.

No full workspace suite, new physical Pi evidence, newly trained model, native UI,
production Worker bundle binding, or complete 1.1 release acceptance is claimed.

## Final artifact identity

The final selected host registry SHA-256 is
`c90c29cea31819359e9c9d46fcaa48ccedcd68c9a8c0e54d3c6b9caa7b9afec4`.
Adding this record changed the generated documentation inventory, so both native
host builds were repeated against that final graph before sealing. Final package
manifests are:

| Selected component profile | Signed manifest SHA-256 |
| --- | --- |
| macos-controller | `a38f6d2f568033ecbb123e98806d200013e02799242d4bb66d4f6d23dac68963` |
| linux-aarch64-controller | `03782bbcabfa9026b5c2cec93778543f97cfeeeb701d04c4cce0cb2e5b29c91b` |
| linux-aarch64-cuda | `badf5fb89eb6536e255dea7370a26f9a395d244d1ede8f757112d38f848d0cdd` |

Archives are under `out/m27e/mac-package-final/` and
`out/m27e/native-packages/`; their summaries retain archive hashes, source enrollment,
exact commands and independent qualification public keys. Ephemeral signing seeds
were removed. These local component signers are not public release endorsements.

The final native PEFT service confinement probe passes on systemd 255.4: only the
dedicated user's session/manager sockets are exposed through `ProtectHome=tmpfs`
and read-only bind mounts, while the home remains hidden. It reads the real user
manager without starting training, serving or a product service; its transient unit
is collected (`native-service-bus-01.json`). The focused seven service contract tests
pass after this repair (`service-tests-final.log`). Non-PEFT confinement stays intact.
