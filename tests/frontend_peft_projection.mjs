// Author: Lukas Bower
// Purpose: Keep candidate rejection and recovered incumbent views tied to recorded native phases.
// Copyright 2026 Lukas Bower
import assert from "node:assert/strict";
import test from "node:test";
import { peftProjection } from "../apps/swarmui/frontend/workbench/peft_projection.js";

test("PEFT projection preserves separate candidate and recovery outcomes", () => {
  const journal = {
    schema: "cohesix-peft-recipe-journal/v1",
    state: "recovered_failure",
    blocker: "canary_behavior_or_latency",
    comparison: { baseline: { generation: 4, adapter_sha256: "incumbent" },
                  candidate_sha256: "candidate" },
    phases: [
      { phase: "evaluate", result: { detail: {
        baseline: { samples: 16, metrics: { eval_loss: 4.0 } },
        candidate: { samples: 16, metrics: { eval_loss: 3.9 } },
      } } },
      { phase: "rollback", result: { succeeded: true,
        detail: { accepted: { generation: 4, adapter_sha256: "incumbent" } } } },
    ],
  };
  const view = peftProjection({ result: { native: journal } });
  assert.equal(view.state, "recovered_failure");
  assert.equal(view.candidate_loss, 3.9);
  assert.equal(view.baseline_loss, 4.0);
  assert.equal(view.served_generation, null);
  assert.equal(view.restored_generation, 4);
  assert.equal(view.restored_adapter, "incumbent");
  assert.equal(view.recovery_observed, true);
  assert.equal(peftProjection({ schema: "other" }), null);
});
