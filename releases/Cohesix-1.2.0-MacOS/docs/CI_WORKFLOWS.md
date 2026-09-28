<!-- Author: Lukas Bower -->
<!-- Purpose: Define ordinary CI retry identity, credential isolation and verified CUDA/LoRA outcome gates. -->
<!-- Copyright 2026 Lukas Bower -->

# CI with verified workflow outcomes

Install the signed tools using [Adoption](ADOPTION.md). The maintained
[workflow example](../packaging/ci/cohesix-journey.yml) uses ordinary runner jobs
and the same `cohesix-journey` Python/CLI interface as an operator. Copy it to
`.github/workflows/` in the adopting repository. Configure the protected
`cohesix-production` environment with required reviewers and main-only deployment
branches. Restrict the `cohesix-controller` runner group to that reviewed workflow;
untrusted PR jobs must never run there. Pin action revisions under your repository's
review policy before production use. The example is not installed or enabled automatically.

Provision an operator-owned configuration, state volume, package trust and evidence
CAS outside the runner workspace. `COH_JOURNEY_CONFIG` is a protected environment
variable naming that absolute configuration. Secret references resolve private
files or bounded environment values on the enrolled runner. They are never workflow
inputs or uploaded artifacts. Do not run `pull_request_target`, fetch PR code in the
protected job, execute unreviewed native artifacts or share production credential
mounts with validation jobs. The validation job has read-only repository permission,
no environment, no production credentials and no submission authority.

Derive the operation before admission using `cohesix-journey identity`. Identity
binds workflow kind, purpose, immutable workload semantics and explicit `rerun` intent.
For CUDA it includes each input's exact bytes, stage dependency/runtime, topology
and generated contract. For LoRA it includes the full requested input/profile,
model, baseline generation and evaluation policy. The operation ID itself is excluded
from its own hash. Never use run-attempt, current time or random identity on retry.
Use `rerun = "initial"` for ordinary retry; an owner-approved deliberate rerun gets a
new distinct string and fresh tickets. Changed input also produces a new operation.

After issuing the exact tickets, freeze the deployment. The private durable directory
contains an atomic `binding.json`, `planned.json`, the **existing** recipe/controller
journal and a sanitized `outcome.json`. An exclusive lock serializes retries across
runner processes. Changed deployment/admission is refused under the same identity.
A missing previously planned journal is refused; restore its exact retained state.
CI must not delete state after a timeout or recreate it on an ephemeral runner.
Native execution, Worker receipt and signed evidence remain the earlier owners.

```sh
cohesix-journey validate --config /srv/cohesix/journey.json
cohesix-journey run --config /srv/cohesix/journey.json --submit --wait-seconds 900
# A new runner with the same durable volume uses the identical command.
cohesix-journey run --config /srv/cohesix/journey.json --wait-seconds 60
```

`validate` checks configuration and immutable identity without credentials or submission.
`--submit` can advance only unsubmitted work through the existing lifecycle with freshly
resolved authority. An uncertain dispatch is queried; it is never blindly resubmitted.
A new CUDA stage still requires current scope, expiry, revocation and resource checks.
An expired LoRA operation requires the separately approved recovery-only workflow in
[Private LoRA release](PRIVATE_LORA_RELEASE.md), never a replacement candidate ticket
hidden inside a CI retry. Export/mirror independently signed graph/CAS objects to the
configured controller evidence locations as part of the enrolled evidence workflow.
A mere transport ACK cannot satisfy this gate.

| Exit | State | Meaning |
| --- | --- | --- |
| 0 | verified | Shared `coh verify` rechecked the requested CUDA output or exact successful LoRA release. |
| 10 | pending | Planned work has no submission. |
| 11 | running | Submission acknowledged; requested terminal evidence is absent. |
| 12 | ambiguous | Dispatch intent exists but ACK/terminal outcome is uncertain. |
| 13 | refused | Configuration, authority, bounds or a submission was refused. |
| 14 | cancelled | Verified cancellation; requested output was not completed. |
| 15 | recovered_failure | Failed candidate restored its baseline; candidate release still failed. |
| 16 | failed | Verified failure, including regression or failed rollback. |
| 17 | timeout | Bounded wait expired; retain state and evidence and reconcile later. |
| 18 | invalid_evidence | Missing, malformed, mismatched or rejected verification evidence/report. |

Wait is 0–3600 seconds, with at most two seconds between inspections. Each fixed
CLI call additionally has the existing 30-second/output bound; terminating a timed-out
controller does not cancel a native operation. Only exit 0 greens the CI job.
The final report carries operation identity, outcome, shared-report digest and evidence
references. Retain the actual protected graphs/CAS and trust separately; a report hash
or uploaded log alone is not verification authority. Never convert a refused,
insufficient-evidence, timeout or successfully rolled-back candidate into success.
