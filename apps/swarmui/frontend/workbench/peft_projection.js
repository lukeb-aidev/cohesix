// Author: Lukas Bower
// Purpose: Project recorded PEFT comparison and recovery fields for the workbench without upgrading their proof class.
// Copyright 2026 Lukas Bower

export function peftProjection(source) {
  const journal = source?.result?.native || source;
  if (journal?.schema !== "cohesix-peft-recipe-journal/v1" ||
      !Array.isArray(journal.phases)) return null;
  const phases = Object.fromEntries(journal.phases
    .filter((row) => typeof row.phase === "string" && row.result)
    .map((row) => [row.phase, row.result]));
  const evaluation = phases.evaluate?.detail;
  const candidate = evaluation?.candidate;
  const baseline = evaluation?.baseline;
  const comparison = journal.comparison;
  return {
    state: journal.state,
    blocker: journal.blocker || null,
    observed_device: typeof phases.validate?.detail?.device === "string"
      ? phases.validate.detail.device : null,
    baseline_generation: comparison?.baseline?.generation ?? null,
    incumbent_adapter: comparison?.baseline?.adapter_sha256 ?? null,
    candidate_adapter: comparison?.candidate_sha256 ?? null,
    heldout_samples: candidate?.samples ?? null,
    baseline_loss: baseline?.metrics?.eval_loss ?? null,
    candidate_loss: candidate?.metrics?.eval_loss ?? null,
    served_generation: phases.promote?.detail?.accepted?.generation ?? null,
    served_adapter: phases.promote?.detail?.accepted?.adapter_sha256 ?? null,
    canary_latency_ms: phases.canary?.detail?.latency_ms ?? null,
    restored_generation: phases.rollback?.detail?.accepted?.generation ?? null,
    restored_adapter: phases.rollback?.detail?.accepted?.adapter_sha256 ?? null,
    recovery_observed: phases.rollback?.succeeded === true,
  };
}
