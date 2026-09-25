// Author: Lukas Bower
// Purpose: Keep SwarmUI MLX summaries tied to verified release evidence and original identity.
// Copyright 2026 Lukas Bower

import assert from "node:assert/strict";
import test from "node:test";
import { mlxSummary } from "../frontend/workbench/mlx.js";

test("unverified release report cannot claim a canary or promotion", () => {
  const text = mlxSummary({
    schema: "cohesix-peft-report/v1",
    operation_id: "local-mlx-1",
    request_sha256: "a".repeat(64),
    submitted: true,
    acknowledged: false,
    ambiguous: true,
    result: null,
  });
  assert.match(text, /Operation: local-mlx-1/);
  assert.match(text, /Outcome: unresolved/);
  assert.match(text, /Signed native release evidence: unavailable/);
  assert.doesNotMatch(text, /Promoted generation:/);
  assert.equal(mlxSummary({ schema: "other" }), null);
});

test("verified journal projects Metal, comparison, canary and rollback separately", () => {
  const text = mlxSummary({
    schema: "cohesix-peft-report/v1",
    operation_id: "local-mlx-1",
    request_sha256: "a".repeat(64),
    submitted: true,
    acknowledged: true,
    result: {
      graph_sha256: "b".repeat(64),
      native: {
        schema: "cohesix-peft-recipe-journal/v1",
        state: "recovered_failure",
        phases: [
          { phase: "validate", result: { detail: { device: "Apple M4 Metal" } } },
          { phase: "evaluate", result: { detail: {
            baseline: { metrics: { eval_loss: 2 } },
            candidate: { metrics: { eval_loss: 1 }, samples: 16 },
          } } },
          { phase: "canary", result: { detail: { latency_ms: 200 } } },
          { phase: "rollback", result: { succeeded: true,
            detail: { accepted: { generation: 0 } } } },
        ],
      },
    },
  });
  assert.match(text, /Observed device: Apple M4 Metal/);
  assert.match(text, /Held-out loss: 2 → 1/);
  assert.match(text, /Canary latency: 200 ms/);
  assert.match(text, /Restored generation: 0/);
  assert.doesNotMatch(text, /Promoted generation:/);
});
