import { setupConsole } from "./components/console.js";
import { hydrateIcons } from "./components/icon.js";
import { createHiveController } from "./hive/index.js";

const output = (id, text) => {
  const node = document.getElementById(id);
  if (!node) {
    return;
  }
  node.textContent = text;
};

const resolveInvoke = () => {
  if (window.__TAURI__?.tauri?.invoke) {
    return window.__TAURI__.tauri.invoke.bind(window.__TAURI__.tauri);
  }
  if (window.__TAURI__?.invoke) {
    return window.__TAURI__.invoke.bind(window.__TAURI__);
  }
  if (window.__TAURI_INVOKE__) {
    return window.__TAURI_INVOKE__;
  }
  return null;
};

const invoke = async (cmd, payload) => {
  const invokeFn = resolveInvoke();
  if (!invokeFn) {
    return { ok: false, error: "Tauri API unavailable" };
  }
  try {
    const result = await invokeFn(cmd, payload);
    return { ok: true, result };
  } catch (err) {
    return { ok: false, error: String(err) };
  }
};

const readSession = () => {
  const role =
    document.getElementById("session-role")?.value?.trim() || "queen";
  const ticketRaw =
    document.getElementById("session-ticket")?.value?.trim() || "";
  return {
    role,
    ticket: ticketRaw.length ? ticketRaw : null,
  };
};

const readSubject = () => {
  const raw = document.getElementById("session-subject")?.value || "";
  const trimmed = raw.trim();
  return trimmed.length ? trimmed : null;
};

const readWorkerId = () => {
  const raw = document.getElementById("worker-id")?.value || "";
  const trimmed = raw.trim();
  return trimmed.length ? trimmed : "";
};

const renderTranscript = (id, transcript) => {
  if (!transcript || !Array.isArray(transcript.lines)) {
    output(id, "ERR UI malformed transcript");
    return;
  }
  output(id, transcript.lines.join("\n"));
};

const setStatus = (id, text) => {
  const node = document.getElementById(id);
  if (node) {
    node.textContent = text;
  }
};

hydrateIcons();

document.getElementById("connect")?.addEventListener("click", async () => {
  const session = readSession();
  const res = await invoke("swarmui_connect", session);
  if (!res.ok) {
    output("telemetry-output", `ERR CONNECT ${res.error}`);
    return;
  }
  renderTranscript("telemetry-output", res.result);
});

let offlineEnabled = false;
const offlineButton = document.getElementById("offline");
offlineButton?.addEventListener("click", async () => {
  offlineEnabled = !offlineEnabled;
  const res = await invoke("swarmui_offline", { offline: offlineEnabled });
  if (!res.ok) {
    offlineEnabled = !offlineEnabled;
    output("telemetry-output", `ERR OFFLINE ${res.error}`);
    return;
  }
  if (offlineButton) {
    offlineButton.textContent = offlineEnabled ? "Online mode" : "Offline mode";
  }
  output("telemetry-output", offlineEnabled ? "OK OFFLINE" : "OK ONLINE");
});

document.getElementById("mint-ticket")?.addEventListener("click", async () => {
  const session = readSession();
  const subject = readSubject();
  setStatus("mint-status", "Minting...");
  const res = await invoke("swarmui_mint_ticket", {
    role: session.role,
    subject,
  });
  if (!res.ok) {
    setStatus("mint-status", `Mint failed: ${res.error}`);
    return;
  }
  const ticket =
    typeof res.result === "string"
      ? res.result.trim()
      : String(res.result || "");
  if (!ticket) {
    setStatus("mint-status", "Mint failed: empty ticket");
    return;
  }
  const ticketInput = document.getElementById("session-ticket");
  if (ticketInput) {
    ticketInput.value = ticket;
  }
  setStatus("mint-status", "Ticket minted");
});

document
  .getElementById("load-telemetry")
  ?.addEventListener("click", async () => {
    const session = readSession();
    const workerId = readWorkerId();
    if (!workerId) {
      output(
        "telemetry-output",
        "ERR TAIL missing worker id (run ls /worker to list active workers)",
      );
      return;
    }
    const res = await invoke("swarmui_tail_telemetry", {
      role: session.role,
      ticket: session.ticket,
      workerId,
    });
    if (!res.ok) {
      output("telemetry-output", `ERR TAIL ${res.error}`);
      return;
    }
    renderTranscript("telemetry-output", res.result);
  });

document.getElementById("load-fleet")?.addEventListener("click", async () => {
  const session = readSession();
  const res = await invoke("swarmui_fleet_snapshot", {
    role: session.role,
    ticket: session.ticket,
  });
  if (!res.ok) {
    output("fleet-output", `ERR FLEET ${res.error}`);
    return;
  }
  renderTranscript("fleet-output", res.result);
});

document
  .getElementById("load-namespace")
  ?.addEventListener("click", async () => {
    const session = readSession();
    const root = document.getElementById("namespace-root")?.value || "/proc";
    const res = await invoke("swarmui_list_namespace", {
      role: session.role,
      ticket: session.ticket,
      path: root,
    });
    if (!res.ok) {
      output("namespace-output", `ERR LS ${res.error}`);
      return;
    }
    renderTranscript("namespace-output", res.result);
  });

const hiveCanvas = document.getElementById("hive-canvas");
const hiveStatus = document.getElementById("hive-status");
const hiveRoot = document.getElementById("hive-root");
const hiveSessions = document.getElementById("hive-sessions");
const hivePressure = document.getElementById("hive-pressure");
const hivePressureStrip = document.getElementById("hive-pressure-strip");
const hiveErrorStrip = document.getElementById("hive-error-strip");
const hiveScheduleSummary = document.getElementById("hive-schedule-summary");
const hiveScheduleQueueSummary = document.getElementById(
  "hive-schedule-queue-summary"
);
const hiveScheduleQueue = document.getElementById("hive-schedule-queue");
const hiveLeaseSummary = document.getElementById("hive-lease-summary");
const hiveLeaseActive = document.getElementById("hive-lease-active");
const hiveLeasePreemptions = document.getElementById("hive-lease-preemptions");
const hiveFallback = document.getElementById("hive-fallback");
const hiveOverlays = document.getElementById("hive-overlays");
const hiveDetailTitle = document.getElementById("hive-detail-title");
const hiveDetailLines = document.getElementById("hive-detail-lines");
const hiveDetailClear = document.getElementById("hive-detail-clear");
let hiveController = null;
let hiveInitError = null;
let lastHiveBatch = null;

const selectChips = (root) => {
  const chips = {};
  if (!root) {
    return chips;
  }
  root.querySelectorAll("[data-kind]").forEach((node) => {
    if (node.dataset.kind) {
      chips[node.dataset.kind] = node;
    }
  });
  return chips;
};

const pressureChips = selectChips(hivePressureStrip);
const errorChips = selectChips(hiveErrorStrip);
const errorCounts = {
  busy: 0,
  quota: 0,
  cut: 0,
  policy: 0,
};
const setHiveFallback = (message) => {
  if (!hiveFallback) {
    return;
  }
  const trimmed = message ? String(message).trim() : "";
  if (trimmed) {
    hiveFallback.textContent = trimmed;
    hiveFallback.classList.add("active");
    return;
  }
  hiveFallback.textContent = "";
  hiveFallback.classList.remove("active");
};
if (hiveCanvas) {
  try {
    hiveController = createHiveController(hiveCanvas, hiveStatus, {
      onAgentSelect: (agentId) => {
        selectHiveAgent(agentId, true);
      },
    });
    setHiveFallback("");
  } catch (err) {
    hiveInitError = err;
    const message = `Hive renderer failed: ${err}`;
    setStatus("hive-status", message);
    setHiveFallback(message);
  }
}

let hiveActive = false;
let hivePollTimer = null;
let hivePollInFlight = false;
let hivePollInterval = 300;
let hiveDetailAgent = null;
let hiveScheduleSignature = "";
let hiveLeaseSignature = "";
let hiveOverlaySignature = "";
let hiveDetailSignature = "";

const resetHiveDetail = () => {
  hiveDetailAgent = null;
  hiveDetailSignature = "";
  hiveOverlaySignature = "";
  if (hiveDetailTitle) {
    hiveDetailTitle.textContent = "Select worker";
  }
  if (hiveDetailLines) {
    hiveDetailLines.innerHTML = '<p class="placeholder">No telemetry loaded.</p>';
  }
};

const setDetailAgent = (agent) => {
  hiveDetailAgent = agent;
  if (hiveDetailTitle) {
    hiveDetailTitle.textContent = agent || "Select worker";
  }
};

function selectHiveAgent(agentId, fromHive = false) {
  if (!agentId) {
    return;
  }
  setDetailAgent(agentId);
  if (!fromHive) {
    hiveController?.selectAgent(agentId);
  }
  if (lastHiveBatch) {
    renderHiveOverlays(lastHiveBatch);
  }
  renderHiveDetail(lastHiveBatch || { detail: null });
  if (hiveActive && !hivePollInFlight) {
    stopHivePolling();
    pollHive();
  }
}

const renderHiveOverlays = (batch) => {
  if (!hiveOverlays) {
    return;
  }
  const overlays = Array.isArray(batch.overlays) ? batch.overlays : [];
  const overlaySignature =
    `selected:${hiveDetailAgent || ""}|` +
    overlays
      .map((overlay) => {
        const lines = Array.isArray(overlay.lines) ? overlay.lines.join("\n") : "";
        return `${overlay.agent ?? ""}:${lines}`;
      })
      .join("|");
  if (overlaySignature === hiveOverlaySignature) {
    return;
  }
  hiveOverlaySignature = overlaySignature;
  hiveOverlays.innerHTML = "";
  if (!overlays.length) {
    hiveOverlays.innerHTML = '<p class="placeholder">Awaiting telemetry.</p>';
    return;
  }
  overlays.forEach((overlay) => {
    const card = document.createElement("button");
    card.type = "button";
    card.className = "hive-telemetry__card";
    if (overlay.agent === hiveDetailAgent) {
      card.classList.add("selected");
    }
    card.dataset.agent = overlay.agent;
    const header = document.createElement("div");
    header.className = "hive-telemetry__agent";
    header.textContent = overlay.agent;
    const lines = document.createElement("div");
    lines.className = "hive-telemetry__lines";
    lines.textContent = Array.isArray(overlay.lines) ? overlay.lines.join("\n") : "";
    card.appendChild(header);
    card.appendChild(lines);
    card.addEventListener("click", () => {
      selectHiveAgent(overlay.agent);
    });
    hiveOverlays.appendChild(card);
  });
};

const renderHiveDetail = (batch) => {
  if (!hiveDetailLines) {
    return;
  }
  const detail = batch.detail;
  const detailLines = detail && Array.isArray(detail.lines) ? detail.lines.join("\n") : "";
  const detailSignature =
    `selected:${hiveDetailAgent || ""}|` +
    `detail:${detail?.agent || ""}:${detailLines}`;
  if (detailSignature === hiveDetailSignature) {
    return;
  }
  hiveDetailSignature = detailSignature;
  if (detail && Array.isArray(detail.lines) && detail.lines.length) {
    if (hiveDetailTitle) {
      hiveDetailTitle.textContent = detail.agent;
    }
    hiveDetailLines.textContent = detail.lines.join("\n");
    return;
  }
  if (hiveDetailAgent && Array.isArray(batch.overlays)) {
    const match = batch.overlays.find((overlay) => overlay.agent === hiveDetailAgent);
    if (match && Array.isArray(match.lines) && match.lines.length) {
      if (hiveDetailTitle) {
        hiveDetailTitle.textContent = match.agent;
      }
      hiveDetailLines.textContent = match.lines.join("\n");
      return;
    }
  }
  if (!hiveDetailAgent) {
    hiveDetailLines.innerHTML = '<p class="placeholder">Select a worker to view details.</p>';
  } else {
    hiveDetailLines.innerHTML = '<p class="placeholder">No telemetry yet.</p>';
  }
};

if (hiveDetailClear) {
  hiveDetailClear.addEventListener("click", () => {
    resetHiveDetail();
  });
}

const updateHivePressure = (batch) => {
  if (!hivePressure) {
    return;
  }
  const pressure = batch.pressure ?? 0;
  const backlog = batch.backlog ?? 0;
  const dropped = batch.dropped ?? 0;
  hivePressure.textContent = `Pressure ${(pressure * 100).toFixed(0)}% · backlog ${backlog} · dropped ${dropped}`;
};

const updateHiveRoot = (batch) => {
  if (!hiveRoot) {
    return;
  }
  const root = batch.root;
  hiveRoot.classList.remove("ok", "cut", "unknown");
  if (!root) {
    hiveRoot.textContent = "ROOT ?";
    hiveRoot.classList.add("unknown");
    return;
  }
  if (root.reachable) {
    hiveRoot.textContent = "ROOT OK";
    hiveRoot.classList.add("ok");
    hiveRoot.title = "Root reachable";
  } else {
    const reason = root.cut_reason || "unknown";
    hiveRoot.textContent = `CUT ${reason}`;
    hiveRoot.classList.add("cut");
    hiveRoot.title = `Root cut: ${reason}`;
  }
};

const updateHiveSessions = (batch) => {
  if (!hiveSessions) {
    return;
  }
  const sessions = batch.sessions;
  hiveSessions.classList.remove("draining");
  if (!sessions) {
    hiveSessions.textContent = "Sessions ?";
    return;
  }
  const active = sessions.active ?? 0;
  const draining = sessions.draining ?? 0;
  hiveSessions.textContent = `Sessions ${active} · draining ${draining}`;
  if (draining > 0) {
    hiveSessions.classList.add("draining");
  }
};

const renderStripCounts = (chips, counts) => {
  Object.entries(chips).forEach(([key, node]) => {
    const value = counts[key] ?? 0;
    node.textContent = `${key} ${value}`;
    node.classList.toggle("active", value > 0);
  });
};

const updateHivePressureCounters = (batch) => {
  const counters = batch.pressure_counters;
  if (!counters || Object.keys(pressureChips).length === 0) {
    return;
  }
  renderStripCounts(pressureChips, {
    busy: counters.busy ?? 0,
    quota: counters.quota ?? 0,
    cut: counters.cut ?? 0,
    policy: counters.policy ?? 0,
  });
};

const resetHiveErrors = () => {
  errorCounts.busy = 0;
  errorCounts.quota = 0;
  errorCounts.cut = 0;
  errorCounts.policy = 0;
  if (Object.keys(errorChips).length > 0) {
    renderStripCounts(errorChips, errorCounts);
  }
};

const clearNode = (node) => {
  if (!node) {
    return;
  }
  while (node.firstChild) {
    node.removeChild(node.firstChild);
  }
};

const renderPlaceholder = (node, text) => {
  if (!node) {
    return;
  }
  clearNode(node);
  const placeholder = document.createElement("p");
  placeholder.className = "placeholder";
  placeholder.textContent = text;
  node.appendChild(placeholder);
};

const formatCount = (value) => {
  const num = Number(value);
  if (!Number.isFinite(num)) {
    return "0";
  }
  return Math.max(0, Math.floor(num)).toString();
};

const updateHiveSchedule = (batch) => {
  if (!hiveScheduleSummary || !hiveScheduleQueueSummary || !hiveScheduleQueue) {
    return;
  }
  const schedule = batch.schedule;
  if (!schedule) {
    resetHiveSchedule();
    return;
  }
  const summary = schedule.summary;
  const queue = Array.isArray(schedule.queue) ? schedule.queue : [];
  const queueCount = summary?.queue ?? queue.length;
  const maxEntries = summary?.max_entries ?? 0;
  const dequeued = summary?.dequeued ?? 0;
  const dropped = summary?.dropped ?? 0;
  const scheduleSignature =
    `${queueCount}:${maxEntries}:${dequeued}:${dropped}|` +
    queue
      .map(
        (entry) =>
          `${entry?.id ?? ""}:${entry?.role ?? ""}:${entry?.priority ?? ""}:${entry?.ticks ?? ""}:${entry?.budget_ms ?? ""}:${entry?.seq ?? ""}`
      )
      .join("|");
  if (scheduleSignature === hiveScheduleSignature) {
    return;
  }
  hiveScheduleSignature = scheduleSignature;

  if (summary) {
    hiveScheduleSummary.textContent = `Queue ${formatCount(queueCount)}/${formatCount(maxEntries)} · dequeued ${formatCount(dequeued)} · dropped ${formatCount(dropped)}`;
    hiveScheduleQueueSummary.textContent = `Queue ${formatCount(queueCount)}/${formatCount(maxEntries)}`;
  } else {
    hiveScheduleSummary.textContent = `Queue ${formatCount(queueCount)}`;
    hiveScheduleQueueSummary.textContent = `Queue ${formatCount(queueCount)}`;
  }

  clearNode(hiveScheduleQueue);
  if (queue.length === 0) {
    renderPlaceholder(hiveScheduleQueue, "No scheduled entries.");
    return;
  }

  const header = document.createElement("div");
  header.className = "hive-schedule__row header";
  ["ID", "ROLE", "PRIORITY", "TICKS", "BUDGET", "SEQ"].forEach((label) => {
    const cell = document.createElement("div");
    cell.textContent = label;
    header.appendChild(cell);
  });
  hiveScheduleQueue.appendChild(header);

  queue.forEach((entry) => {
    const row = document.createElement("div");
    row.className = "hive-schedule__row";
    const cells = [
      entry?.id ?? "?",
      entry?.role ?? "?",
      formatCount(entry?.priority),
      formatCount(entry?.ticks),
      `${formatCount(entry?.budget_ms)}ms`,
      formatCount(entry?.seq)
    ];
    cells.forEach((value) => {
      const cell = document.createElement("div");
      cell.textContent = value;
      row.appendChild(cell);
    });
    hiveScheduleQueue.appendChild(row);
  });
};

const updateHiveLease = (batch) => {
  if (!hiveLeaseSummary || !hiveLeaseActive || !hiveLeasePreemptions) {
    return;
  }
  const lease = batch.lease;
  if (!lease) {
    resetHiveLease();
    return;
  }

  const summary = lease.summary;
  const active = Array.isArray(lease.active) ? lease.active : [];
  const preemptions = Array.isArray(lease.preemptions) ? lease.preemptions : [];
  const activeCount = summary?.active ?? active.length;
  const preemptCount = summary?.preemptions ?? preemptions.length;
  const quotaCount = summary?.quotas ?? 0;
  const maxActive = summary?.max_active ?? 0;
  const leaseSignature =
    `${activeCount}:${preemptCount}:${quotaCount}:${maxActive}|` +
    active
      .map(
        (entry) =>
          `${entry?.id ?? ""}:${entry?.subject ?? ""}:${entry?.resource ?? ""}:${entry?.ttl_s ?? ""}:${entry?.priority ?? ""}:${entry?.state ?? ""}:${entry?.seq ?? ""}`
      )
      .join("|") +
    "|" +
    preemptions
      .map(
        (entry) =>
          `${entry?.id ?? ""}:${entry?.subject ?? ""}:${entry?.resource ?? ""}:${entry?.reason ?? ""}:${entry?.seq ?? ""}`
      )
      .join("|");
  if (leaseSignature === hiveLeaseSignature) {
    return;
  }
  hiveLeaseSignature = leaseSignature;

  if (summary) {
    hiveLeaseSummary.textContent = `Active ${formatCount(activeCount)}/${formatCount(maxActive)} · preemptions ${formatCount(preemptCount)} · quotas ${formatCount(quotaCount)}`;
  } else {
    hiveLeaseSummary.textContent = `Active ${formatCount(activeCount)} · preemptions ${formatCount(preemptCount)}`;
  }

  clearNode(hiveLeaseActive);
  if (active.length === 0) {
    renderPlaceholder(hiveLeaseActive, "No active leases.");
  } else {
    active.forEach((entry) => {
      const item = document.createElement("div");
      item.className = "hive-lease__item";
      const header = document.createElement("div");
      header.className = "hive-lease__item-header";
      const title = document.createElement("span");
      title.textContent = entry?.id ?? "?";
      const state = document.createElement("span");
      state.textContent = entry?.state ?? "unknown";
      header.appendChild(title);
      header.appendChild(state);
      const meta = document.createElement("div");
      meta.className = "hive-lease__item-meta";
      [
        `subject ${entry?.subject ?? "?"}`,
        `resource ${entry?.resource ?? "?"}`,
        `ttl ${formatCount(entry?.ttl_s)}s`,
        `priority ${formatCount(entry?.priority)}`,
        `seq ${formatCount(entry?.seq)}`
      ].forEach((text) => {
        const span = document.createElement("span");
        span.textContent = text;
        meta.appendChild(span);
      });
      item.appendChild(header);
      item.appendChild(meta);
      hiveLeaseActive.appendChild(item);
    });
  }

  clearNode(hiveLeasePreemptions);
  if (preemptions.length === 0) {
    renderPlaceholder(hiveLeasePreemptions, "No preemptions yet.");
  } else {
    preemptions.forEach((entry) => {
      const item = document.createElement("div");
      item.className = "hive-lease__item";
      const header = document.createElement("div");
      header.className = "hive-lease__item-header";
      const title = document.createElement("span");
      title.textContent = entry?.id ?? "?";
      const reason = document.createElement("span");
      reason.textContent = entry?.reason ?? "unknown";
      header.appendChild(title);
      header.appendChild(reason);
      const meta = document.createElement("div");
      meta.className = "hive-lease__item-meta";
      [
        `subject ${entry?.subject ?? "?"}`,
        `resource ${entry?.resource ?? "?"}`,
        `seq ${formatCount(entry?.seq)}`
      ].forEach((text) => {
        const span = document.createElement("span");
        span.textContent = text;
        meta.appendChild(span);
      });
      item.appendChild(header);
      item.appendChild(meta);
      hiveLeasePreemptions.appendChild(item);
    });
  }

};

const resetHiveSchedule = () => {
  hiveScheduleSignature = "";
  if (hiveScheduleSummary) {
    hiveScheduleSummary.textContent = "Scheduler idle";
  }
  if (hiveScheduleQueueSummary) {
    hiveScheduleQueueSummary.textContent = "Queue empty";
  }
  if (hiveScheduleQueue) {
    renderPlaceholder(hiveScheduleQueue, "Awaiting schedule queue.");
  }
};

const resetHiveLease = () => {
  hiveLeaseSignature = "";
  if (hiveLeaseSummary) {
    hiveLeaseSummary.textContent = "Leases idle";
  }
  if (hiveLeaseActive) {
    renderPlaceholder(hiveLeaseActive, "No active leases.");
  }
  if (hiveLeasePreemptions) {
    renderPlaceholder(hiveLeasePreemptions, "No preemptions yet.");
  }
};

const updateHiveErrors = (batch) => {
  if (!batch.events || Object.keys(errorChips).length === 0) {
    return;
  }
  for (const event of batch.events) {
    if (event.kind !== "error") {
      continue;
    }
    const reason = String(event.reason || "").toLowerCase();
    if (reason in errorCounts) {
      errorCounts[reason] += 1;
    }
  }
  renderStripCounts(errorChips, errorCounts);
};

const stopHivePolling = () => {
  if (hivePollTimer) {
    clearTimeout(hivePollTimer);
    hivePollTimer = null;
  }
};

const pollHive = async () => {
  if (!hiveActive || hivePollInFlight) {
    return;
  }
  hivePollInFlight = true;
  const session = readSession();
  const res = await invoke("swarmui_hive_poll", {
    role: session.role,
    ticket: session.ticket,
    detail_agent: hiveDetailAgent,
  });
  hivePollInFlight = false;
  if (!res.ok) {
    setStatus("hive-status", `Hive halted (${res.error})`);
    hiveActive = false;
    stopHivePolling();
    return;
  }
  lastHiveBatch = res.result;
  hiveController?.ingest(res.result);
  updateHivePressure(res.result);
  updateHiveRoot(res.result);
  updateHiveSessions(res.result);
  updateHivePressureCounters(res.result);
  updateHiveErrors(res.result);
  updateHiveSchedule(res.result);
  updateHiveLease(res.result);
  renderHiveOverlays(res.result);
  renderHiveDetail(res.result);
  if (res.result.done) {
    hiveActive = false;
    stopHivePolling();
    return;
  }
  hivePollTimer = setTimeout(pollHive, hivePollInterval);
};

const startHive = async () => {
  if (!hiveController) {
    if (hiveInitError) {
      const message = `Hive renderer failed: ${hiveInitError}`;
      setStatus("hive-status", message);
      setHiveFallback(message);
    }
    return;
  }
  setHiveFallback("");
  const session = readSession();
  const snapshotKey =
    document.getElementById("hive-snapshot-key")?.value?.trim() || "demo";
  const res = await invoke("swarmui_hive_bootstrap", {
    role: session.role,
    ticket: session.ticket,
    snapshot_key: snapshotKey,
  });
  if (!res.ok) {
    setStatus("hive-status", `Hive blocked (${res.error})`);
    return;
  }
  hiveController.bootstrap(res.result);
  hiveController.start();
  resetHiveErrors();
  resetHiveDetail();
  resetHiveSchedule();
  resetHiveLease();
  renderHiveOverlays({ overlays: [] });
  lastHiveBatch = null;
  hiveActive = true;
  hivePollInterval = Math.max(
    120,
    Math.floor(1000 / (res.result.hive?.frame_cap_fps || 60))
  );
  stopHivePolling();
  pollHive();
};

const stopHive = async () => {
  if (!hiveController) {
    return;
  }
  hiveActive = false;
  stopHivePolling();
  hiveController.stop();
  resetHiveErrors();
  resetHiveDetail();
  resetHiveSchedule();
  resetHiveLease();
  renderHiveOverlays({ overlays: [] });
  const session = readSession();
  await invoke("swarmui_hive_reset", {
    role: session.role,
    ticket: session.ticket,
  });
  setStatus("hive-status", "Hive idle");
};

document.getElementById("hive-start")?.addEventListener("click", startHive);
document.getElementById("hive-stop")?.addEventListener("click", stopHive);
document
  .getElementById("hive-reset-view")
  ?.addEventListener("click", () => hiveController?.resetView());

const autoStartHiveReplay = async () => {
  const res = await invoke("swarmui_mode");
  if (!res.ok) {
    return;
  }
  if (res.result?.hive_replay) {
    setStatus("hive-status", "Hive replay booting...");
    await startHive();
  }
};

autoStartHiveReplay();
setupConsole(invoke);
