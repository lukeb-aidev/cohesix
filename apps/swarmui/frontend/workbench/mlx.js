// Author: Lukas Bower
// Purpose: Guide one local MLX release through reviewed Cohesix commands and display only verified journal observations.
// Copyright 2026 Lukas Bower

import { invoke, notice } from "./state.js";
import { selectOperation } from "./operations.js";
import { peftProjection } from "./peft_projection.js";

const summary = () => document.getElementById("mlx-release-summary");

function privatePath(field) {
  const path = document.getElementById(field).value.trim();
  if (!path.startsWith("/") || path.length > 4096 || /[\x00-\x1f\x7f]/.test(path)) {
    notice("Select an absolute, bounded local path.", true);
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

export function localMlxSummary(report) {
  if (report?.schema !== "cohesix-local-mlx/v1") return null;
  if (report.proof_class === "local_refusal")
    return `Local MLX refused: ${String(report.reason || "unknown").slice(0, 128)}`;
  if (report.proof_class !== "local_metal_observation" ||
      !["infer", "evaluate"].includes(report.operation) ||
      !report.observation || typeof report.observation !== "object" ||
      typeof report.observation.device_name !== "string" ||
      !/^[a-f0-9]{64}$/.test(report.observation.model_sha256)) return null;
  const observed = report.observation;
  const lines = ["Local Metal observation · no Cohesix admission or promotion",
    `Device: ${observed.device_name}`,
    `Model SHA-256: ${observed.model_sha256}`,
    `Adapter SHA-256: ${observed.adapter_sha256 || "base model"}`,
    `Peak Metal memory: ${observed.peak_memory_bytes} bytes`];
  if (report.operation === "infer") {
    if (typeof observed.text !== "string") return null;
    lines.push(`Elapsed: ${observed.elapsed_ms} ms`, `Response: ${observed.text}`);
  } else {
    if (typeof observed.eval_loss !== "number") return null;
    lines.push(`Held-out rows: ${observed.samples}`, `Held-out loss: ${observed.eval_loss}`);
  }
  return lines.join("\n");
}

export function initializeMlx(supportedHost) {
  if (!supportedHost) {
    document.querySelector('[data-view="mlx"]').hidden = true;
    summary().textContent = "Local MLX requires the selected macOS host. No native Metal result is available here.";
    document.querySelectorAll("[data-mlx-mode], [data-mlx-job], #mlx-submit, #mlx-infer, #mlx-evaluate")
      .forEach((button) => { button.disabled = true; });
    return;
  }
  for (const operation of ["infer", "evaluate"]) {
    document.getElementById(`mlx-${operation}`).addEventListener("click", async () => {
      const python = privatePath("mlx-python");
      const selection_path = privatePath("mlx-selection");
      if (!python || !selection_path) return;
      const result = document.getElementById("mlx-local-result");
      const prompt = operation === "infer" ?
        document.getElementById("mlx-prompt").value : "";
      const max_tokens = operation === "infer" ?
        Number(document.getElementById("mlx-tokens").value) : 0;
      if (operation === "infer" && (!prompt || new TextEncoder().encode(prompt).length > 2048 ||
          !Number.isInteger(max_tokens) || max_tokens < 1 || max_tokens > 64)) {
        notice("Enter a prompt up to 2048 bytes and a 1–64 token bound.", true);
        return;
      }
      result.textContent = "Running on the selected local Metal device…";
      document.querySelectorAll("#mlx-infer, #mlx-evaluate").forEach((button) => { button.disabled = true; });
      try {
        const response = await invoke("swarmui_local_mlx", { request: {
          python, selection_path, operation, prompt, max_tokens,
        } });
        result.textContent = response.ok ?
          (localMlxSummary(response.result) || "Unsupported local MLX response; no result is inferred.") :
          response.error;
      } finally {
        document.querySelectorAll("#mlx-infer, #mlx-evaluate").forEach((button) => { button.disabled = false; });
      }
    });
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
