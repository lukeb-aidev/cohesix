// Author: Lukas Bower
// Purpose: Present canonical artifact records without promoting reports to verified authority.
// Copyright 2026 Lukas Bower
import {
  state,
  invoke,
  element,
  notice,
  transcript,
  readControlValue,
  setStatus,
} from "./state.js";
import { reflectSession } from "./session.js";
import { navigate } from "./navigation.js";
import { resumeHive } from "./hive.js";
import { selectOperation } from "./operations.js";
import { showStory, detail as record } from "./story.js";
async function openArtifact(kind, path, target) {
  if (kind === "trace" || kind === "hive")
    window.dispatchEvent(new Event("session-changing"));
  const response = await invoke("swarmui_open_artifact", { kind, path });
  if (!response.ok) {
    notice(response.error, true);
    transcript("Artifact refused", response.error);
    return;
  }
  transcript(`Open ${kind}`, response.result);
  if (kind === "report") showStory(response.result);
  else if (kind === "pack") {
    target.replaceChildren(
      element("h2", "Evidence pack"),
      element(
        "p",
        `Source: ${response.result.source_class || "retained pack"}. Inspect violations and observation status before relying on a record.`,
      ),
    );
    const observations = response.result.observations || [];
    if (response.result.violations?.length)
      record(target, "Violations", response.result.violations);
    for (const observation of observations.slice(0, 256))
      record(
        target,
        `${observation.path || "Observation"} · ${observation.status || "unknown"}`,
        observation,
      );
    record(target, "Complete canonical inspection", response.result);
  } else {
    target.textContent = JSON.stringify(response.result, null, 2);
    const info = await invoke("swarmui_workbench_info");
    if (info.ok) reflectSession(info.result);
    if (kind === "hive") {
      navigate("hive");
      await resumeHive();
    }
  }
}
export function initializeArtifacts() {
  document
    .getElementById("story-open")
    .addEventListener("click", () =>
      openArtifact("report", readControlValue("story-path")),
    );
  document
    .getElementById("evidence-open")
    .addEventListener("click", () =>
      openArtifact(
        "pack",
        readControlValue("evidence-path"),
        document.getElementById("evidence-content"),
      ),
    );
  document
    .getElementById("replay-open")
    .addEventListener("click", () =>
      openArtifact(
        readControlValue("replay-kind"),
        readControlValue("replay-path"),
        document.getElementById("replay-output"),
      ),
    );
  document.getElementById("replay-hive").addEventListener("click", () => {
    navigate("hive");
    if (state.mode === "REPLAY") resumeHive();
  });
  document
    .getElementById("flight-refresh")
    .addEventListener("click", () => selectOperation("gpu list"));
  document.querySelectorAll("[data-reference]").forEach((button) =>
    button.addEventListener("click", async () => {
      window.dispatchEvent(new Event("session-changing"));
      button.disabled = true;
      const result = await invoke("swarmui_reference", {
        name: button.dataset.reference,
      });
      button.disabled = false;
      const info = await invoke("swarmui_workbench_info");
      if (info.ok) reflectSession(info.result);
      if (!result.ok) {
        notice(result.error, true);
        transcript("Reference refused", result.error);
        return;
      }
      notice("Verified retained evidence. Live session disconnected.");
      navigate("story");
      showStory(result.result, true);
      transcript("Verified reference replay", {
        reference: result.result.reference,
        graph_sha256: result.result.graph.graph_sha256,
        verified_at_unix_ms: result.result.graph.verified_at_unix_ms,
        mode: "REPLAY",
      });
    }),
  );
  window.addEventListener("host-result", (e) => {
    const { operation, result } = e.detail;
    if (!result) return;
    if (operation.join(" ") === "evidence story" && result.success) {
      try {
        navigate("story");
        showStory(JSON.parse(result.stdout), true);
      } catch {
        notice(
          "The verifier returned an unreadable story; inspect the exact transcript.",
          true,
        );
      }
    }
    if (operation[0] === "gpu")
      document.getElementById("flight-output").textContent =
        `${result.outcome}\n${result.stdout}\n${result.stderr}`;
    if (
      ["plan", "watch", "verify", "recover"].includes(operation[0]) ||
      operation.join(" ") === "peft release"
    ) {
      try {
        showStory({
          proof: "Host command report · inspect its verification fields",
          report: JSON.parse(result.stdout),
        });
      } catch {
        /* Non-JSON command failures remain in the exact transcript. */
      }
    }
  });
  window.addEventListener("hive-observation", (e) => {
    const batch = e.detail;
    setStatus(
      "metric-queen",
      batch.root?.reachable === true
        ? "Reachable"
        : batch.root?.reachable === false
          ? "Unreachable"
          : "Unknown",
    );
    setStatus("metric-workers", String(batch.sessions?.active ?? "—"));
    setStatus(
      "metric-pressure",
      Number.isFinite(batch.pressure)
        ? `${Math.round(batch.pressure * 100)}%`
        : "—",
    );
  });
}
