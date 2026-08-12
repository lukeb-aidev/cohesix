<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Explain how to propose, implement, validate, and review Cohesix contributions. -->
<!-- Author: Lukas Bower -->

# Contributing to Cohesix

Cohesix welcomes focused fixes, tests, documentation, and milestone-authorized
features. It is a research operating system with strict security, scope, and
evidence requirements; a change is complete only when code, generated
artifacts, tests, and public documentation describe the same as-built system.

Do not report a suspected vulnerability in a public issue. Follow the private
process in [Security](docs/SECURITY.md#reporting-a-vulnerability).

## 1. Establish scope before editing

Read these sources in order:

1. [`AGENTS.md`](AGENTS.md) — normative build charter and merge blockers.
2. [`docs/BUILD_PLAN.md`](docs/BUILD_PLAN.md) — active milestones and task
   authorization.
3. The contract document for the surface being changed, starting from the
   [README documentation map](README.md#documentation).
4. The selected source manifest, resolved manifest, and generated outputs when
   behavior is profile-controlled.

Every contribution must cite the exact active milestone or submilestone and
task title/ID that authorizes it. If no active task covers the change, update
and review the build plan first. Do not present cleanup, preparation, or future
work as authorization. A direct governance-repair task may reconcile canonical
documents without authorizing unrelated product implementation.

Use the task template in `AGENTS.md` without changing its fields. Keep the
change atomic: one stated goal, its applicable tests/evidence, any required
regeneration, and the matching documentation.

## 2. Preserve Cohesix invariants

Before proposing a design, confirm that it preserves the charter. In
particular:

- VM code remains Rust and `no_std`; do not add a POSIX or libc façade.
- The authenticated root-task console is the only permitted in-VM TCP
  listener. Host REST, UI, proxy, and bridge tools project existing authority;
  they do not create new target authority paths.
- Control-plane actions use documented console grammar or Secure9P namespace
  semantics with role- and ticket-scoped authority.
- Physical device authority, mapping, DMA, IRQ, and resource admission remain
  in HAL, with steady-state devices served by manifest-declared isolated driver
  runtimes.
- CUDA, NVML, model runtimes, training, and inference remain host-side.
- Memory, queues, retries, timeouts, and work are explicitly bounded.
- User-controlled frames, paths, JSON, tokens, and configuration are validated
  and fail with typed, deterministic errors.
- Secrets are supplied through deployment configuration or environment
  variables; examples must not normalize placeholder credentials.

The detailed limits and breaking-change rules live in `AGENTS.md`,
[Secure9P](docs/SECURE9P.md), and
[Userland and CLI](docs/USERLAND_AND_CLI.md).

## 3. Treat generated output as generated

The selected manifest and `coh-rtc` outputs define generated interfaces,
defaults, bounds, namespaces, and profile behavior. Never hand-edit a generated
file or generated block to make a check pass.

When a generated contract changes:

1. Change the compiler IR and validation.
2. Regenerate every output required by `scripts/check-generated.sh`.
3. Update source, tests, fixtures, and human-authored documentation together.
4. Run the drift guard and inspect the complete diff.

Host-only presentation or analysis may not need new IR, but it must remain
faithful to the existing protocol and authority model.

## 4. Implement and document the change

- Follow idiomatic Rust, Python PEP 8 with type hints, and the repository's
  existing patterns.
- Add or update tests only for distinct touched invariants under the Test
  Discipline in `AGENTS.md`, including relevant invalid and boundary inputs.
  Do not duplicate target behavior in host mocks merely because code changed.
- Keep `unsafe` use exceptional and document each block with a precise
  `SAFETY:` invariant. Do not increase risk indicators without the exception
  process defined in `AGENTS.md`.
- Preserve `ACK`/`ERR`/`END`, NineDoor errors, namespace layouts, and `/proc`
  formats unless the full breaking-change process is authorized.
- Update public docs in the same change as public behavior. Describe what the
  selected profiles build today, and keep planned work clearly labelled.
- Retain concise author, purpose, and current-year Lukas Bower copyright
  metadata in human-authored, comment-capable files. Do not make commentless
  formats invalid or invent metadata sidecars solely to satisfy this rule.
- Remove only artifacts made obsolete by the scoped change. Do not fold
  unrelated cleanup into the contribution.

## 5. Validate locally

During target development, use the non-claiming convergence workflow and
changed-path selection defined by `docs/TEST_PLAN.md` before broad closure.
Convergence evidence never replaces acceptance evidence.

Before merging AI-assisted Rust, run the repository baseline from the workspace
root:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo test --workspace
cargo audit
cargo deny check advisories
scripts/check-generated.sh
scripts/ci/check_test_plan.sh
scripts/ci/test_plan_run.sh --list
git diff --check
```

Documentation-only and non-Rust changes run their applicable documentation,
metadata, generated-consistency, link, and surface checks; do not run unrelated
Rust commands merely to satisfy a generic checklist.

Run the complete staged Test Plan with a unique evidence directory when the
active task or `docs/TEST_PLAN.md` requires acceptance, and before any milestone
or release claim:

```bash
scripts/ci/test_plan_run.sh --state-dir out/test-plan/<run-id>
```

Examples of additional evidence include QEMU transcripts for target behavior,
Pi 4 serial and packet captures for hardware claims, `.coh` fixtures for
console grammar, negative Secure9P tests for protocol changes, and release
checks for bundle changes. Repository-only tests are not Pi 4 hardware proof.

If a baseline command fails for a pre-existing reason, record the exact command
and failure separately. A new failure in the changed surface blocks review.

## 6. Submit a reviewable change

The pull request description should include:

- exact milestone/submilestone and task title/ID;
- goal and user-visible behavior;
- files and generated artifacts changed;
- authority, attack-surface, memory-bound, and determinism impact;
- commands run and durable evidence paths;
- known limitations or proof boundaries.

Keep commits intentional and do not include local build products, credentials,
or unrelated worktree changes. Release-bundle source changes must follow the
versioning rule in `AGENTS.md`.

Use GitHub Issues for reproducible, non-sensitive defects and scoped design
discussion. Include the smallest reproduction, selected profile, manifest
fingerprint where relevant, and evidence that another contributor can verify.
