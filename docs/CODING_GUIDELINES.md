<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Define Rust safety, contract, testing, and maintenance rules for Cohesix contributors. -->
<!-- Author: Lukas Bower -->
# Cohesix Coding Guidelines (Rust)

These are the detailed language and test rules required by [AGENTS](../AGENTS.md).
[TEST_PLAN](TEST_PLAN.md) owns test selection, execution and acceptance; the
[contribution baseline](../CONTRIBUTING.md#5-validate-locally) owns pre-merge commands.
Moving rules here does not weaken their authority or require new tooling.

## Rust conventions

1. **Toolchain**: Use the repository-pinned stable Rust toolchain with `rustfmt` and strict Clippy. Both are mandatory pre-merge checks; do not infer that the small GitHub `ci` job runs them. The actual CI coverage is documented in [TEST_PLAN](TEST_PLAN.md#github-actions-gate-mapping).
2. **Safety**: Use `#![forbid(unsafe_code)]` except in tightly scoped, reviewed seL4 syscall, MMIO/driver, allocator or IPC/bootstrap modules. Apply the safety and risk controls below; technical review may be agent-led.
3. **Crate structure**: Prefer small, role-oriented crates such as `secure9p-codec`, `secure9p-core`, `nine-door` and `worker-heart`. Public APIs need documentation and integration tests when behavior spans crates, under the Test Discipline below.
4. **Errors**: VM-facing crates use deterministic, exhaustively documented enums. Host tools may use `anyhow` but must preserve deterministic ACK/ERR semantics. No panics on user-controlled input or protocol frames.
5. **Logging**: Use plain-text append-only logs, not heavyweight frameworks. Include role and ticket identifiers for traceability, never secret capability material.
6. **Configuration**: No environment-variable magic inside the VM. Configuration flows through 9P control files or compile-time constants.
7. **Concurrency**: Use seL4 notifications and message queues, not OS threads inside the VM. Host async runtimes remain feature-gated.
8. **Dependencies**: Keep the dependency tree minimal, audit third-party crates and prefer `no_std`-friendly libraries. Never vendor seL4.
9. **Style**: Use idiomatic Rust naming and meaningful capability/ticket types or aliases. Abstractions must serve a concrete current need, not speculative extensibility.
10. **Documentation**: Update the owning reference when changing APIs, workflows or roles. Follow [CONTRIBUTING](../CONTRIBUTING.md#documentation-and-metadata); BUILD_PLAN must authorise new roles.
11. **Security**: Validate 9P lengths and UTF-8 before use; deny by default and grant permissions explicitly per role and path. Validate nested resource claims before allocating or performing work.

## Safety and risk controls

- Every `unsafe` block and unsafe trait implementation has a precise `SAFETY:`
  argument stating the applicable validity, lifetime, ownership, aliasing and
  synchronisation obligations and why they hold. Safe APIs must enforce their
  internal unsafe preconditions; otherwise expose an explicit unsafe caller
  contract. Comments alone do not establish safety.
- Every unsafe `Send`/`Sync` implementation also requires concurrency evidence.
  `transmute` requires documented ABI/layout equivalence.
- Non-test `unwrap()` is prohibited unless impossible by construction.
  Non-test `expect()` is limited to invariant boundaries with a precise message.
  User input returns typed errors; do not hide failures with lossy defaults.
- Never hold a lock across `.await`. Control-plane channels require bounded
  backpressure; spawned tasks define ownership, cancellation and shutdown.
- Non-test `unsafe`, `unwrap`, `expect` and `panic!` counts cannot increase
  without a finding in `docs/audit/findings.csv` and an approved exception in
  `docs/audit/EXCEPTIONS.md`. Counts are risk signals, not targets; do not hide
  operations, weaken checks or widen unsafe scopes to improve them.
- The exception process remains required. Technical review may be agent-led;
  mandatory human sign-off applies only to the overall release under AGENTS.

## Test Discipline

- Tests preserve distinct, independently known contracts. Do not add a test
  merely because code changed or optimise for test count.
- Prefer small deterministic tests for parsing, bounds, arithmetic, ABI/layout,
  serialisation, policy predicates, state machines and other pure behavior.
- For defect fixes, seek the smallest safe reproducer or independent
  counterexample before changing code. Exercise relevant malformed and boundary
  inputs; resource-boundary tests must check rejection before excessive
  allocation or work. Record when reproduction is infeasible, and distinguish
  source-review findings from reproduced failures and verified fixes.
- Match authority to execution: pure contracts use deterministic unit tests;
  host-component contracts use focused host tests; QEMU-target seL4 behavior
  requires QEMU evidence; physical Pi behavior requires fresh Pi evidence.
  Each layer proves only what it exercises.
- Do not model more target scheduling, IPC, capability, IRQ, DMA/cache or
  driver behavior in a host test than a genuine host-testable contract needs.
  A green host simulation is not target acceptance, and target evidence does
  not replace an unexercised pure contract test.
- If host simulation and target evidence disagree, investigate the simulation
  before changing target code merely to satisfy it.
- After a target-discovered defect, add a host regression only when the cause is
  a useful deterministic host-testable invariant. Preserve the smallest
  independently understood invariant; do not recreate the full target scenario
  in mocks or require a host test for every target defect.
- Tests cannot depend on uncontrolled wall-clock time, sleeps, randomness,
  execution order, external networks or shared mutable state/environment
  unless that behavior is the contract under test. Prefer fixed or injected
  inputs; controlled time, randomness, network, filesystem and mocks remain
  legitimate.
- Prefer exact assertions for exact contracts. Do not loosen assertions, widen
  accepted outcomes, increase arbitrary retries/polling or change expected
  values merely to obtain PASS. Predicates, ranges and set membership remain
  valid when they are the contract.
- Production constants, tables or implementation logic are not independent
  test oracles. Expected truth comes from an independent specification,
  generated contract, ABI, fixture, protocol or other authoritative source.
- A directly affected test may be simplified, consolidated, replaced or
  removed only when its protection is demonstrably redundant,
  implementation-coupled, misleading, obsolete or superseded by stronger
  evidence. Canonical protocol/as-built fixtures remain authoritative unless
  their governing contract intentionally changes.

## Review rule maintenance

Keep [Code Review Rules](../AGENTS.md#code-review-rules) focused on consequential,
recurring mistakes. When materially changing a rule, check a real violation,
a safe counterexample and an unrelated change. This is a focused rule review,
not a new mandatory repository-wide test suite or a substitute for code evidence.
