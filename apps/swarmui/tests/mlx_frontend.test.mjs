// Author: Lukas Bower
// Purpose: Keep SwarmUI MLX and governed vMLX summaries tied to verified release graphs and original identity.
// Copyright 2026 Lukas Bower

import assert from "node:assert/strict";
import test from "node:test";
import { governedVmlxSummary, localMlxSummary, mlxSummary } from "../frontend/workbench/mlx.js";

test("local Metal result stays distinct from signed release evidence", () => {
  const text = localMlxSummary({schema:"cohesix-local-mlx/v1",
    proof_class:"local_metal_observation", operation:"infer",
    observation:{device_name:"Apple M4",model_sha256:"a".repeat(64),
      adapter_sha256:"b".repeat(64),peak_memory_bytes:1024,
      elapsed_ms:12,text:"Ready"}});
  assert.match(text, /Local Metal observation/);
  assert.match(text, /Response: Ready/);
  assert.doesNotMatch(text, /Promoted generation|Signed native release/);
  assert.match(localMlxSummary({schema:"cohesix-local-mlx/v1",
    proof_class:"local_refusal",reason:"mlx_model_changed"}), /refused: mlx_model_changed/);
  assert.equal(localMlxSummary({schema:"unknown"}), null);
});

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

test("governed vMLX requires both inspected signed graphs", () => {
  const release = "a".repeat(64), rollback = "b".repeat(64);
  const report = {schema:"cohesix-m28c1-live-report/v1",case:"m28c1-vmlx-live",
    release_graph_sha256:release,rollback_graph_sha256:rollback,
    source_sha256:"c".repeat(64),accepted_generation:1,
    changed_generation_refused:true,rollback_incumbent_observed:true,
    responses:Array.from({length:4}, () => ({prompt_sha256:"d".repeat(64),
      output_sha256:"e".repeat(64),model:"cohesix-g1-test",completion_tokens:8})),
    quality_resources:Array.from({length:4}, () => ({elapsed_ms:1200,rss_bytes:1048576}))};
  assert.equal(governedVmlxSummary(report, new Set([release])), null);
  assert.match(governedVmlxSummary(report, new Set([release, rollback])),
    /Accepted generation: 1/);
  assert.equal(governedVmlxSummary({...report, rollback_incumbent_observed:false},
    new Set([release, rollback])), null);
});
