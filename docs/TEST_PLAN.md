- DD30's recurring acceptance requirement was permanently retired by Lukas
  Bower on 2026-09-14. [DD30 retirement decision](audit/AUDIT_REPORT_2026-09-13.md#dd30-retirement) records the
  repaired historical finding as P1 / `RETIRED_ACCEPTED_GAP`, with dynamic
  fault/wake still UNEXECUTED. Stage 05 checks its retirement record without
  milestone/release selection, whole-file hashes or expiry. `DD_MILESTONE_ID`
  is obsolete. No other finding, target evidence or staged prerequisite is
  waived; a demonstrated IPC regression follows the ordinary finding lifecycle.
- Human Rust review remains separate. The historical [M27](audit/DD30_M27_APPROVAL.toml)
  and [M27a](audit/DD30_M27A_APPROVAL.toml) approvals can be checked explicitly
  with `scripts/ci/due_diligence_gate.sh --check-rust-review 27|27a`. The check
  binds review to the original implementation and ignores historical DD30
  expiry. Future implementation requires its own normal review, with no new
  DD30 approval record. A finding preflight alone establishes no Rust sign-off.
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
