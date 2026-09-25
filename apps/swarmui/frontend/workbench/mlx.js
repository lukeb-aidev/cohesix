// Author: Lukas Bower
// Purpose: Guide one local MLX release through reviewed Cohesix commands and display only verified journal observations.
// Copyright 2026 Lukas Bower

import { notice } from "./state.js";
import { selectOperation } from "./operations.js";
import { peftProjection } from "./peft_projection.js";

const summary = () => document.getElementById("mlx-release-summary");

function privatePath(field) {
  const path = document.getElementById(field).value.trim();
  if (!path.startsWith("/") || path.length > 4096 || /[\x00-\x1f\x7f]/.test(path)) {
    notice("Select an absolute, bounded private deployment file.", true);
    return null;
  }
  return path;
}

function admissionId() {
  const id = document.getElementById("mlx-admission-id").value.trim();
  if (!id || id.length > 128 || /[\x00-\x20\x7f]/.test(id)) {
    notice("Enter the original bounded admission ID.", true);
    return null;
  }
  return id;
}

export function mlxSummary(report) {
  if (report?.schema !== "cohesix-peft-report/v1" ||
      typeof report.operation_id !== "string") return null;
  const lines = [
    `Operation: ${report.operation_id}`,
    `Request SHA-256: ${report.request_sha256 || "unavailable"}`,
    `Submitted: ${report.submitted === true ? "yes" : "no"}`,
    `Acknowledged: ${report.acknowledged === true ? "yes" : "no"}`,
  ];
  if (report.ambiguous === true) lines.push("Outcome: unresolved; follow the original identity.");
  const projected = peftProjection(report);
  if (!projected || !report.result?.graph_sha256) {
    lines.push("Signed native release evidence: unavailable.");
    return lines.join("\n");
  }
  lines.push(`Verified evidence graph: ${report.result.graph_sha256}`);
  lines.push(`Release state: ${projected.state}`);
  if (projected.blocker) lines.push(`Blocker: ${projected.blocker}`);
  if (projected.observed_device) lines.push(`Observed device: ${projected.observed_device}`);
  if (projected.heldout_samples !== null)
    lines.push(`Held-out samples: ${projected.heldout_samples}`);
  if (projected.baseline_loss !== null && projected.candidate_loss !== null)
    lines.push(`Held-out loss: ${projected.baseline_loss} → ${projected.candidate_loss}`);
  if (projected.canary_latency_ms !== null)
    lines.push(`Canary latency: ${projected.canary_latency_ms} ms`);
  if (projected.served_generation !== null)
    lines.push(`Promoted generation: ${projected.served_generation}`);
  if (projected.restored_generation !== null)
    lines.push(`Restored generation: ${projected.restored_generation}`);
  return lines.join("\n");
}

export function initializeMlx(supportedHost) {
  if (!supportedHost) {
    document.querySelector('[data-view="mlx"]').hidden = true;
    summary().textContent = "Local MLX requires the selected macOS host. No native Metal result is available here.";
    document.querySelectorAll("[data-mlx-mode], [data-mlx-job], #mlx-submit")
      .forEach((button) => { button.disabled = true; });
    return;
  }
  document.querySelectorAll("[data-mlx-mode]").forEach((button) => {
    button.addEventListener("click", () => {
      const deployment = privatePath("mlx-deployment");
      if (deployment) selectOperation("peft release", {
        mode: [button.dataset.mlxMode], deployment: [deployment],
      });
    });
  });
  document.getElementById("mlx-submit").addEventListener("click", () => {
    const input = privatePath("mlx-job-file");
    if (input) selectOperation("job submit", { input: [input] });
  });
  document.querySelectorAll("[data-mlx-job]").forEach((button) => {
    button.addEventListener("click", () => {
      const id = admissionId();
      if (id) selectOperation(`job ${button.dataset.mlxJob}`, { admission_id: [id] });
    });
  });
  window.addEventListener("host-result", (event) => {
    const action = event.detail?.operation?.join(" ");
    if (action !== "peft release" && action !== "job submit") return;
    const result = event.detail.result;
    if (!result?.success) {
      summary().textContent = "Command did not complete. Inspect the original journal and job before another action.";
      return;
    }
    try {
      const report = JSON.parse(result.stdout);
      if (action === "job submit") {
        const id = report?.record?.binding?.admission_id;
        if (typeof id === "string" && id.length > 0 && id.length <= 128) {
          document.getElementById("mlx-admission-id").value = id;
          summary().textContent = `Standing job: ${id}\nSubmission: ${report.submission || "unknown"}\nSigned native release evidence: pending; follow the original journal.`;
        } else summary().textContent = "Job response has no bounded admission ID; reconcile the original job file before retrying.";
      } else summary().textContent = mlxSummary(report) ||
        "Release report is unavailable or has an unsupported schema.";
    } catch {
      summary().textContent = "Command report is not valid JSON; no outcome is inferred.";
    }
  });
}
