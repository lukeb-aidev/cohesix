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

See the [Glossary](docs/GLOSSARY.md) for Cohesix-specific role, interface, and
evidence terms.

## 1. Establish scope before editing

Read these sources in order:

1. [`AGENTS.md`](AGENTS.md) — normative build charter and task routing.
2. [`docs/BUILD_PLAN.md`](docs/BUILD_PLAN.md) — active milestones and task
   authorization.
3. The contract document for the surface being changed, starting from the
   charter's task routes or [README documentation map](README.md#documentation).
4. The selected source manifest, resolved manifest, and generated outputs when
   behavior is profile-controlled.

Confirm the requested branch, worktree changes and exact execution profile before
editing. Preserve unrelated changes and retained evidence. Read applicable local
instructions and use an existing relevant skill or maintained workflow; neither
can waive the charter or substitute for actual verification.

Every contribution must cite the exact active milestone or submilestone and
task title/ID that authorizes it. If no active task covers the change, update
and review the build plan first. Do not present cleanup, preparation, or future
work as authorization. A direct governance-repair task may reconcile canonical
documents without authorizing unrelated product implementation.

Use the [task record](#task-record) without changing its fields. Keep the change
atomic: one stated goal, its applicable tests/evidence, any required regeneration,
and matching documentation. Explain the changed invariants and rationale, not
just the files. Record material AI assistance in the task/review record, not
source headers, and preserve licensing and provenance.

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
  before their resource use and fail with typed, deterministic errors.
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
  existing patterns. Apply [Coding Guidelines](docs/CODING_GUIDELINES.md).
- Add or update tests only for distinct touched invariants under
  [Test Discipline](docs/CODING_GUIDELINES.md#test-discipline), including relevant
  invalid and boundary inputs. Do not duplicate target behavior in host mocks.
- Keep `unsafe` exceptional and apply the complete
  [safety and risk controls](docs/CODING_GUIDELINES.md#safety-and-risk-controls).
  The findings/exception ratchet is unchanged; counts are not optimisation targets.
- Preserve `ACK`/`ERR`/`END`, NineDoor errors, namespace layouts, and `/proc`
  formats unless the full breaking-change process is authorized.
- Any material change requires the charter's complete host-tool, Python and
  benchmark compatibility review. Record reviewed surfaces needing no change;
  update every affected implementation, contract, test/fixture and reference.
- Remove only artifacts made obsolete by the scoped change. Do not fold
  unrelated cleanup into the contribution.

### Documentation and metadata

Write each reference for its purpose: architecture explains components and trust
boundaries; guides explain operations; help indexes commands; manuals explain full
usage, realistic examples and recovery. Integrate changed behavior into its topic
and replace obsolete explanations. Keep milestone chronology and qualification
history in build, audit or release records, not appended to user references.

Update affected root-shell/cohsh help, manuals, SwarmUI help, host CLI help and
public guides in the same change as public behavior. Review examples, arguments,
defaults, authority, errors and feature/transport restrictions. Shared manual
content has one source; surface-specific text differs only for real capabilities.

Human-authored, comment-capable files retain concise Author, Purpose and
current-year Lukas Bower copyright metadata. Keep Purpose informative, not a
restatement of the filename. Do not add invalid comments, invented fields or
metadata sidecars to commentless formats; use existing package metadata or the
owning documentation. Generated, vendored and immutable release files retain
their authoritative format. Preserve useful API and safety documentation;
explain contracts, constraints and non-obvious choices rather than narrating
syntax or inventing design history. Do not credit tools in file headers.

### Script lifecycle

Tracked `scripts/` must implement a documented community/developer workflow,
a canonical CI/test/evidence gate, or support invoked by a tracked build,
release or operator entry point. Temporary probes, one-off reproducers, scratch
generators and ad-hoc wrappers belong in ignored `out/scripts/` or an OS temporary
directory. Promote a script only with its owning call site or documentation,
focused tests where its logic merits them, and removal of the superseded path
in the same change.

## 5. Validate locally

The matrix below owns contribution-level validation. TEST_PLAN's
[action catalog](docs/TEST_PLAN.md#canonical-action-catalog) owns staged commands,
claim selection and evidence; it is not replaced by a second command inventory.

| Stage or change | Required work | What it cannot establish |
| --- | --- | --- |
| Development iteration | Select the smallest meaningful checks for every affected contract/profile. For target work, follow non-claiming target-first convergence and its stop-at-first-failure rule. | Merge, milestone or release acceptance by itself. |
| Rust merge acceptance | Code review, the complete baseline below, exact affected host/target compilation and required changed-surface evidence. | Target or release claims not exercised by that evidence. |
| Documentation/policy-only or non-Rust change | Applicable documentation, metadata, generated-consistency, link and surface checks. No unrelated Rust compilation requirement. | Runtime verification or a waiver of applicable checks. |
| Component/milestone acceptance | Every BUILD_PLAN definition-of-done check and the applicable TEST_PLAN evidence at exact source/profile identity. | Acceptance of untested targets or of the assembled release. |
| Overall release | Complete applicable staged, conditional, hardware, pressure/repeatability, due-diligence and bundle/promotion evidence, then named human release-owner sign-off. | Approval of unsupported claims or fabricated/relabeled evidence. |

Before merging AI-assisted Rust, run the repository baseline from the workspace
root. The charter's relocation does not remove or change these gates:

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

`scripts/check-generated.sh` invokes the Test Plan consistency check; record that
invocation rather than rerunning an identical check only to satisfy both listings.
Any evidence reuse must follow TEST_PLAN's immutable source/profile/resume rules.
A subset used during development does not waive final merge obligations.
For release packaging, [Conditional G](docs/TEST_PLAN.md#conditional-g--release-bundle-validation-macos-linux-and-pi4)
selects repeat checks by material change. A changed archive or image hash alone
requires an integrity/equivalence decision, not an automatic staged rerun; the
original target result retains its exact source, image and boot identity.

The current GitHub `ci` workflow is a smaller health check, not execution of this
entire baseline. Its weekly/manual dependency audit does not silently waive the
pre-merge audit requirement. Complete and record the required checks not performed
by CI; do not describe unexecuted checks as passing. The actual division is in
[GitHub Actions gate mapping](docs/TEST_PLAN.md#github-actions-gate-mapping).

Establish the complete staged Test Plan with a unique evidence directory when
the active task or TEST_PLAN requires staged acceptance. Before a release claim,
use the retained baseline at its original source/profile identity and apply
Conditional G's material-change decision; repeat only the affected checks when
materially changed. The initial staged commands are:

```bash
scripts/ci/test_plan_run.sh --list
scripts/ci/test_plan_run.sh --target qemu --state-dir out/test-plan/<run-id>
scripts/ci/test_plan_run.sh --target pi4 --state-dir out/test-plan/<run-id>
```

Select the applicable targets; these examples do not turn a host-only change into
a physical-Pi claim. All targets required for an overall release remain mandatory.
Examples of additional evidence include QEMU transcripts, fresh Pi serial/packet
captures, `.coh` fixtures, negative Secure9P tests and bundle checks. Repository-only
tests are not Pi proof. Preserve exact source/image/target provenance and original
failed attempts.

If a baseline fails for a pre-existing reason, record its exact command and failure
separately. A new failure in the changed surface blocks review. Missing hardware,
credentials or tooling leaves the corresponding check blocked/unexecuted; disclose
it rather than weakening assertions or claiming acceptance.

## 6. Submit a reviewable change

The pull request description should include:

- exact milestone/submilestone and task title/ID;
- goal, changed invariants, rationale and user-visible behavior;
- files and generated artifacts changed;
- authority, attack-surface, memory-bound and determinism impact;
- commands run, actual results and durable evidence paths;
- known limitations, proof boundaries and material AI assistance.

Individual changes, commits, merges and component/milestone acceptance do not
require human sign-off. Technical review may be agent-led, including an independent
review where required. Safety arguments, test evidence, scope and exception controls
remain mandatory. No agent may invent an approval or mark its own output as human-reviewed.

Only the overall assembled Cohesix release needs explicit approval by a named human
release owner before publication or promotion. Bind that approval to the exact
source/artifact identities, evidence, limitations and residual risks. No separate
human approval of each constituent change is required. Historical approvals remain
valid only as records of their original scope. This policy does not remove runtime
capability, deployment-credential or physical-operation authority checks.

Keep commits intentional and do not include local build products, credentials or
unrelated worktree changes. Code changes under `releases/` increment the minor
version and update release directory/tarball names under AGENTS.

Use GitHub Issues for reproducible, non-sensitive defects and scoped design
discussion. Include the smallest reproduction, selected profile, manifest
fingerprint where relevant, and evidence that another contributor can verify.

### Task record

Use these unchanged fields, formerly printed in the root charter. Record rationale,
AI assistance and proof limits within the appropriate fields or review description.
Planner, Builder and Auditor are contribution roles; Queen and Workers are system
roles. BUILD_PLAN must explicitly introduce any additional role.

```text
Title/ID: <slug>
Milestone: <exact milestone/submilestone and task title/ID>
Goal: <one sentence>
Inputs: <artifacts, versions, paths>
Changes:
  - <file> — <summary>
Commands: <exact shell commands for the scoped host/target>
Checks: <deterministic success criteria>
Deliverables: <files, logs, doc updates>
```
