// Author: Lukas Bower
// Purpose: Keep verified causal order, recorded native phases and independent proof axes visible together.
// Copyright 2026 Lukas Bower
import { element, state, setStatus } from "./state.js";
import { ownershipNodes, StoryCanvas } from "../hive/story.js";
let canvas = null,
  playback = null;
const human = (value) => String(value ?? "unknown").replaceAll("_", " ");
const stamp = (value) =>
  Number.isFinite(value) ? new Date(value).toISOString() : "Time unavailable";
export function detail(parent, label, value, open = false) {
  const node = element("details"),
    summary = element("summary", label),
    pre = element("pre", JSON.stringify(value, null, 2));
  node.open = open;
  node.append(summary, pre);
  parent.append(node);
  return node;
}
export function stopStory() {
  clearInterval(playback);
  playback = null;
}
window.addEventListener("session-changing", stopStory);
window.addEventListener("workbench-route", (event) => {
  if (event.detail !== "story") stopStory();
});
document.addEventListener("visibilitychange", () => {
  if (document.hidden) stopStory();
});

export function showStory(envelope, verified = false) {
  stopStory();
  canvas?.destroy();
  canvas = null;
  const root = document.getElementById("story-content");
  root.className = "story-document";
  root.closest("[data-desk]").classList.add("has-story");
  root.replaceChildren();
  const graph = verified ? envelope.graph : null;
  const reports = graph
    ? Object.entries(envelope.artifacts || {})
        .filter(([, a]) => a.status === "observed")
        .map(([sha256, a]) => ({
          sha256,
          report: a.value?.observation || a.value,
        }))
    : [{ sha256: envelope.sha256, report: envelope.report || envelope }];
  const native =
    reports.find((r) => Array.isArray(r.report?.phases)) ||
    reports.find((r) => r.report?.result?.native?.phases);
  const report =
    native?.report?.result?.native ||
    native?.report ||
    reports[0]?.report ||
    {};
  const primary = envelope.report || report;
  const head = element("header", undefined, "story-heading");
  const identity =
    graph?.binding?.ticket_id ||
    primary.operation_id ||
    report.operation_id ||
    "Retained observation";
  const stateLabel =
    report.state ||
    primary.result?.native?.state ||
    primary.blocker ||
    graph?.outcome ||
    "Outcome unverified";
  head.append(
    element(
      "span",
      `${state.mode} / ${graph ? "VERIFIED CAUSAL RECORDS" : "UNVERIFIED REPORT"}`,
      "section-kicker",
    ),
    element("h2", identity),
    element("p", human(stateLabel), "story-outcome"),
  );
  head.dataset.outcome = String(stateLabel);
  root.append(head);
  root.append(
    element(
      "p",
      graph
        ? `Signatures and CAS verified at ${stamp(graph.verified_at_unix_ms)}. This retained evidence does not establish current readiness.`
        : "This is a report observation. Its booleans and labels do not establish verified execution.",
      "field-help",
    ),
  );
  const blocker = report.blocker || primary.blocker;
  if (blocker)
    root.append(element("div", `Blocker: ${blocker}`, "story-blocker"));
  const layout = element("div", undefined, "story-layout"),
    left = element("div", undefined, "story-main"),
    inspector = element("aside", undefined, "story-inspector");
  layout.append(left, inspector);
  root.append(layout);
  const inspect = (label, value) => {
    inspector.replaceChildren(
      element("span", "SOURCE INSPECTOR", "section-kicker"),
      element("h3", label),
    );
    detail(inspector, "Exact record and references", value, true);
  };
  const nodes = ownershipNodes(graph);
  if (nodes.length) {
    const surface = element("div", undefined, "story-canvas");
    left.append(surface);
    canvas = new StoryCanvas(surface, nodes, (node) =>
      inspect(node.label, node.source),
    );
    const legend = element("div", undefined, "ownership-legend");
    nodes.forEach((node) => {
      const button = element("button", node.label);
      button.addEventListener("click", () => inspect(node.label, node.source));
      legend.append(button);
    });
    left.append(legend);
    const ribbon = element("div", undefined, "evidence-ribbon");
    // The verifier's topological record order is retained verbatim; animations never reorder evidence.
    for (const node of graph.records.slice(0, 64)) {
      const button = element("button");
      button.append(
        element("strong", human(node.kind)),
        element("small", human(node.outcome)),
      );
      button.dataset.outcome = node.outcome;
      button.addEventListener("click", () =>
        inspect(`${human(node.kind)} · ${stamp(node.observed_unix_ms)}`, node),
      );
      ribbon.append(button);
    }
    left.append(element("h3", "The evidence chain"), ribbon);
    const proof = element("div", undefined, "proof-axes");
    for (const [label, value] of [
      ["Declaration", "Exact manifest binding"],
      [
        "Worker receipt",
        graph.binding.worker
          ? "Bound identity; inspect receipt"
          : "Not part of this graph",
      ],
      [
        "External execution",
        graph.records.find((n) => n.kind === "execution")?.source ||
          "Unavailable",
      ],
      ["Target proof", "Inspect target and Worker artifacts"],
      ["Live readiness", "Not established by replay"],
    ]) {
      const row = element("div");
      row.append(element("span", label), element("strong", value));
      proof.append(row);
    }
    left.append(proof);
  }
  const rows =
    report.phases ||
    primary.result?.native?.phases ||
    primary.stages ||
    primary.attempts ||
    primary.phases ||
    primary.events ||
    [];
  const entries = Array.isArray(rows)
    ? rows.slice(0, 128)
    : Object.entries(rows)
        .slice(0, 128)
        .map(([id, value]) => ({ id, ...value }));
  if (entries.length) {
    left.append(element("h3", "Recorded native phases"));
    const toolbar = element("div", undefined, "story-player"),
      previous = element("button", "← Previous"),
      play = element("button", "Play walkthrough"),
      next = element("button", "Next →"),
      position = element("span");
    toolbar.append(previous, play, next, position);
    left.append(toolbar);
    const timeline = element("div", undefined, "phase-list");
    left.append(timeline);
    let selected = 0;
    const select = (index) => {
      selected = Math.max(0, Math.min(index, entries.length - 1));
      [...timeline.children].forEach((button, i) => {
        button.setAttribute("aria-pressed", String(i === selected));
      });
      const row = entries[selected],
        result = row.result || row.evidence || row;
      position.textContent = `${selected + 1} / ${entries.length}`;
      inspect(
        human(row.phase || row.stage || row.name || row.id || selected + 1),
        {
          proof: graph
            ? "Observation in verified CAS; independent from current state"
            : "Unverified report observation",
          artifact_sha256: native?.sha256 || envelope.sha256 || null,
          provider: result.native_identity || result.source || "unknown",
          worker: graph?.binding.worker || null,
          phase: row,
        },
      );
    };
    entries.forEach((row, index) => {
      const result = row.result || row.evidence || row,
        outcome =
          result.state ||
          result.status ||
          result.outcome ||
          (typeof result.succeeded === "boolean"
            ? result.succeeded
              ? "succeeded"
              : "failed"
            : "recorded");
      const button = element("button", undefined, "phase-button");
      button.append(
        element("span", String(index + 1).padStart(2, "0")),
        element(
          "strong",
          human(row.phase || row.stage || row.name || row.id || index + 1),
        ),
        element("small", human(outcome)),
      );
      button.dataset.outcome = outcome;
      button.addEventListener("click", () => select(index));
      timeline.append(button);
    });
    previous.addEventListener("click", () => {
      stopStory();
      select(selected - 1);
    });
    next.addEventListener("click", () => {
      stopStory();
      select(selected + 1);
    });
    play.addEventListener("click", () => {
      if (playback) {
        stopStory();
        play.textContent = "Play walkthrough";
        return;
      }
      if (
        matchMedia("(prefers-reduced-motion: reduce)").matches ||
        document.body.classList.contains("reduced-motion")
      ) {
        select(selected + 1);
        return;
      }
      play.textContent = "Pause walkthrough";
      select(0);
      playback = setInterval(() => {
        if (selected >= entries.length - 1) {
          stopStory();
          play.textContent = "Play walkthrough";
        } else select(selected + 1);
      }, 1800);
    });
    select(0);
  } else {
    left.append(
      element(
        "p",
        "No native phase list was captured. Inspect the causal records and their exact artifact observations.",
        "field-help",
      ),
    );
    inspect("Source identity", graph?.binding || envelope);
  }
  for (const item of reports)
    detail(root, `Artifact ${item.sha256 || "unverified report"}`, item.report);
  detail(root, "Complete source and verifier projection", envelope);
  document.getElementById("metric-proof").textContent = graph
    ? "Recorded proof"
    : "Unverified report";
  if (graph) updateFlight(graph, reports);
}

function updateFlight(graph, reports) {
  const native = reports.find((r) => r.report?.phases)?.report;
  const phase = native?.phases?.findLast((r) => r.result?.detail)?.result;
  const detailValue = phase?.detail || {};
  const execution = graph.records.find((n) => n.kind === "execution");
  const cards = [
    [
      "PROVIDER / SOURCE",
      execution?.source || "Unknown",
      "External execution custodian",
    ],
    [
      "TARGET PROFILE",
      graph.binding.target_manifest_sha256.slice(0, 16),
      "Historical manifest binding",
    ],
    [
      "RUNTIME",
      execution?.native_identity || "Unknown",
      "Original native identity",
    ],
    [
      "MODEL / ADAPTER",
      detailValue.accepted?.adapter_sha256 ||
        native?.comparison?.candidate_sha256 ||
        "Unknown",
      "Exact artifact identity",
    ],
    [
      "HOST MEMORY",
      detailValue.resources?.process_peak_host_rss_bytes
        ? `${(detailValue.resources.process_peak_host_rss_bytes / 1048576).toFixed(1)} MiB`
        : "Unknown",
      "Recorded process peak RSS",
    ],
    [
      "GPU UTILIZATION",
      "Unavailable",
      "No utilization sensor in this evidence",
    ],
    ["TEMPERATURE / POWER", "Unavailable", "No thermal or power observation"],
    [
      "QUEUE / TTFT / DECODE",
      "Unavailable",
      "No equivalent latency metrics captured",
    ],
    [
      "ARTIFACT STORE",
      `${Object.keys(reports).length} observations`,
      "CAS digests verified; storage health unknown",
    ],
  ];
  const root = document.getElementById("flight-content");
  root.replaceChildren();
  for (const [label, value, note] of cards) {
    const card = element("article");
    card.append(
      element("span", label),
      element("strong", value),
      element("small", note),
    );
    root.append(card);
  }
  setStatus(
    "flight-output",
    `Retained evidence at ${stamp(graph.verified_at_unix_ms)}. This is not live sensor telemetry.\n${JSON.stringify({ binding: graph.binding, execution }, null, 2)}`,
  );
}
