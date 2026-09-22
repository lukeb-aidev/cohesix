- DD30's recurring acceptance requirement was permanently retired by Lukas
  Bower on 2026-09-14. [DD30 retirement decision](audit/AUDIT_REPORT_2026-09-13.md#dd30-retirement) records the
  repaired historical finding as P1 / `RETIRED_ACCEPTED_GAP`, with dynamic
  fault/wake still UNEXECUTED. Stage 05 checks its retirement record without
  milestone/release selection, whole-file hashes or expiry. `DD_MILESTONE_ID`
  is obsolete. No other finding, target evidence or staged prerequisite is
  waived; a demonstrated IPC regression follows the ordinary finding lifecycle.
- Technical Rust review remains required and may be agent-led, including
  independent review where required. Individual changes, commits, merges and
  component/milestone acceptance do not require human sign-off. Follow the
  [validation matrix and merge baseline](../CONTRIBUTING.md#5-validate-locally)
  and [Test Discipline](CODING_GUIDELINES.md#test-discipline); existing safety,
  scope, exception and evidence controls remain mandatory.
- Only an overall assembled Cohesix release requires explicit approval by a
  named human release owner before publication or promotion. Bind approval to
  the exact source/artifacts, evidence, limitations and residual risks. Agent
  review or a passing gate cannot supply release approval. Runtime capability,
  deployment and physical-operation authority checks remain unchanged.
- Historical [M27](audit/DD30_M27_APPROVAL.toml) and
  [M27a](audit/DD30_M27A_APPROVAL.toml) human approvals can be checked explicitly
  with `scripts/ci/due_diligence_gate.sh --check-rust-review 27|27a`. This binds
  only their original implementations and ignores historical DD30 expiry; it
  is not a recurring per-change or milestone gate. No new DD30 approval record
  is required. A finding preflight is neither code review nor release sign-off.
- For this M27a completion run, the owner's later instruction is: “Only run
  tests required to mark this milestone ‘Complete’, not the full suite”.
  [M27A_COMPLETION_EVIDENCE.md](audit/M27A_COMPLETION_EVIDENCE.md) maps the
  milestone's individual definition-of-done checks to exact retained evidence
  and focused validation. Earlier complete suites retain their source
  identities; the interrupted candidate-F common run is not a staged PASS.
  This run does not claim a complete final-source five-stage chain or create
  replacement markers. Required M27a contracts, target observations, compatibility
  regression outputs, the pre-27a gateway benchmark comparison and security checks
  remain required. The separate 2026-09-14
  [completion decision](audit/M27A_COMPLETION_EVIDENCE.md#completion-decision)
  accepts M26d without its missing status-baseline JSON and waives that comparison
  for M27a closure only. The comparison remains NOT_PERFORMED; this decision
  creates no performance-equivalence claim, target PASS or runner bypass.
