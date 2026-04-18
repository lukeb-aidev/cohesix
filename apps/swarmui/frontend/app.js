// Author: Lukas Bower
// Purpose: SwarmUI frontend wiring for console actions, telemetry panels, and Live Hive polling.
// Copyright 2026 Lukas Bower

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

const readControlValue = (id, fallback = "") => {
  const node = document.getElementById(id);
  if (!node || !("value" in node)) {
    return fallback;
  }
  const raw = node.value ?? "";
  const text = typeof raw === "string" ? raw : String(raw);
  return text.trim();
};

const setControlValue = (id, value) => {
  const node = document.getElementById(id);
  if (!node || !("value" in node)) {
    return;
  }
  node.value = value;
  node.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
  node.dispatchEvent(new Event("change", { bubbles: true, composed: true }));
};

const setButtonLabel = (id, text) => {
  const node = document.getElementById(id);
  if (!node) {
    return;
  }
  const label = node.querySelector("[data-button-label]");
  if (label) {
    label.textContent = text;
    return;
  }
  node.textContent = text;
};

const resolveInvoke = () => {
  if (window.__TAURI__?.core?.invoke) {
    return window.__TAURI__.core.invoke.bind(window.__TAURI__.core);
  }
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

let swarmUiMode = null;

const readSwarmUiMode = async () => {
  if (swarmUiMode) {
    return swarmUiMode;
  }
  const res = await invoke("swarmui_mode");
  swarmUiMode = res.ok ? res.result || {} : {};
  return swarmUiMode;
};

const readSession = () => {
  const role = readControlValue("session-role", "queen") || "queen";
  const ticketRaw = readControlValue("session-ticket");
  return {
    role,
    ticket: ticketRaw.length ? ticketRaw : null,
  };
};

const readSubject = () => {
  const value = readControlValue("session-subject");
  return value.length ? value : null;
};

const readWorkerId = () => {
  return readControlValue("worker-id");
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

const setConsoleAvailability = (enabled, reason) => {
  const panel = document.querySelector(".console-panel");
  const outputNode = document.getElementById("console-output");
  const inputNode = document.getElementById("console-input");
  const sendNode = document.getElementById("console-send");
  const clearNode = document.getElementById("console-clear");
  const stopNode = document.getElementById("console-stop");

  panel?.classList.toggle("is-disabled", !enabled);
  panel?.setAttribute("aria-disabled", enabled ? "false" : "true");

  [inputNode, sendNode, clearNode].forEach((node) => {
    if (node) {
      node.disabled = !enabled;
    }
  });
  if (stopNode) {
    stopNode.disabled = true;
  }

  if (!enabled && outputNode) {
    outputNode.textContent = reason;
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
  swarmUiMode = {
    ...(swarmUiMode || {}),
    offline: offlineEnabled
  };
  const res = await invoke("swarmui_offline", { offline: offlineEnabled });
  if (!res.ok) {
    offlineEnabled = !offlineEnabled;
    swarmUiMode = {
      ...(swarmUiMode || {}),
      offline: offlineEnabled
    };
    output("telemetry-output", `ERR OFFLINE ${res.error}`);
    return;
  }
  if (offlineButton) {
    setButtonLabel("offline", offlineEnabled ? "Online mode" : "Offline mode");
  }
  setConsoleAvailability(
    !offlineEnabled,
    "Console unavailable while offline mode is enabled.",
  );
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
    setControlValue("session-ticket", ticket);
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
    const root = readControlValue("namespace-root", "/proc") || "/proc";
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
let hiveScrollTimer = null;
let hiveScrollActive = false;
let hiveRenderActive = false;
let hiveUiNeedsFlush = false;
let hiveCanvasVisible = true;
let hiveDocVisible = true;
let hiveInteractionTimer = null;
let hiveInteractionActive = false;
const applyHiveBatch = (batch) => {
  lastHiveBatch = batch;
  hiveController?.ingest(batch);
  updateHivePressure(batch);
  updateHiveRoot(batch);
  updateHiveSessions(batch);
  updateHivePressureCounters(batch);
  updateHiveErrors(batch);
  updateHiveSchedule(batch);
  updateHiveLease(batch);
  renderHiveOverlays(batch);
  renderHiveDetail(batch);
};
const setHiveRenderActive = (active) => {
  hiveRenderActive = Boolean(active);
  hiveController?.setRenderActive(hiveRenderActive);
  if (hiveRenderActive && hiveUiNeedsFlush && hiveActive && lastHiveBatch) {
    hiveUiNeedsFlush = false;
    applyHiveBatch(lastHiveBatch);
  }
};
const setHiveInteractionActive = (active) => {
  if (hiveInteractionTimer) {
    clearTimeout(hiveInteractionTimer);
    hiveInteractionTimer = null;
  }
  hiveInteractionActive = Boolean(active);
  hiveController?.setInteractionActive(hiveInteractionActive);
};
const bumpHiveInteraction = () => {
  if (!hiveActive) {
    return;
  }
  if (!hiveInteractionActive) {
    hiveInteractionActive = true;
    hiveController?.setInteractionActive(true);
  }
  if (hiveInteractionTimer) {
    clearTimeout(hiveInteractionTimer);
  }
  hiveInteractionTimer = setTimeout(() => {
    hiveInteractionTimer = null;
    hiveInteractionActive = false;
    hiveController?.setInteractionActive(false);
  }, 900);
};
const updateHiveRenderActive = () => {
  const shouldRender = hiveActive && hiveCanvasVisible && hiveDocVisible;
  setHiveRenderActive(shouldRender);
};
const setHiveScrollActive = (active) => {
  if (hiveScrollTimer) {
    clearTimeout(hiveScrollTimer);
    hiveScrollTimer = null;
  }
  hiveScrollActive = Boolean(active);
  if (hiveScrollActive) {
    setHiveInteractionActive(false);
  }
  if (document.body) {
    document.body.classList.toggle("hive-scroll-paused", hiveScrollActive);
  }
  hiveController?.setScrollActive(active);
  if (active) {
    hiveScrollTimer = setTimeout(() => {
      hiveScrollTimer = null;
      hiveScrollActive = false;
      if (document.body) {
        document.body.classList.remove("hive-scroll-paused");
      }
      hiveController?.setScrollActive(false);
      if (hiveUiNeedsFlush && hiveActive && hiveRenderActive && lastHiveBatch) {
        hiveUiNeedsFlush = false;
        applyHiveBatch(lastHiveBatch);
      }
    }, 180);
    return;
  }
  if (hiveUiNeedsFlush && hiveActive && hiveRenderActive && lastHiveBatch) {
    hiveUiNeedsFlush = false;
    applyHiveBatch(lastHiveBatch);
  }
};
if (hiveCanvas) {
  try {
    hiveController = createHiveController(hiveCanvas, hiveStatus, {
      onAgentSelect: (agentId) => {
        selectHiveAgent(agentId, true);
      },
    });
    const isHiveEventTarget = (event) => {
      if (!hiveCanvas || !event?.target) {
        return false;
      }
      if (!(event.target instanceof Node)) {
        return false;
      }
      return hiveCanvas.contains(event.target);
    };
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.length) {
          return;
        }
        hiveCanvasVisible = entries[0].isIntersecting;
        updateHiveRenderActive();
      },
      { root: null, threshold: 0.05 },
    );
    observer.observe(hiveCanvas);
    document.addEventListener("visibilitychange", () => {
      hiveDocVisible = document.visibilityState === "visible";
      updateHiveRenderActive();
    });
    window.addEventListener(
      "scroll",
      () => {
        if (hiveActive) {
          setHiveScrollActive(true);
        }
      },
      { passive: true },
    );
    window.addEventListener(
      "wheel",
      (event) => {
        if (!hiveActive) {
          return;
        }
        if (event.defaultPrevented) {
          return;
        }
        if (isHiveEventTarget(event)) {
          return;
        }
        setHiveScrollActive(true);
      },
      { passive: true },
    );
    window.addEventListener(
      "touchmove",
      (event) => {
        if (!hiveActive) {
          return;
        }
        if (isHiveEventTarget(event)) {
          return;
        }
        setHiveScrollActive(true);
      },
      { passive: true },
    );
    hiveCanvas.addEventListener("pointerdown", bumpHiveInteraction, { passive: true });
    hiveCanvas.addEventListener("pointermove", bumpHiveInteraction, { passive: true });
    hiveCanvas.addEventListener("wheel", bumpHiveInteraction, { passive: true });
    hiveCanvas.addEventListener("touchstart", bumpHiveInteraction, { passive: true });
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
let hivePollGeneration = 0;
let hiveDetailAgent = null;
let hiveScheduleSignature = "";
let hiveLeaseSignature = "";
let hiveOverlayOrder = [];
let hiveOverlayPlaceholder = null;
const hiveOverlayCards = new Map();
let hiveDetailSignature = "";

const resetHiveDetail = () => {
  hiveDetailAgent = null;
  hiveDetailSignature = "";
  hiveOverlayOrder = [];
  if (hiveOverlayPlaceholder) {
    hiveOverlayPlaceholder.remove();
  }
  hiveOverlayPlaceholder = null;
  for (const card of hiveOverlayCards.values()) {
    card.remove();
  }
  hiveOverlayCards.clear();
  if (hiveDetailTitle) {
    hiveDetailTitle.textContent = "Select worker";
  }
  if (hiveDetailLines) {
    renderPlaceholder(hiveDetailLines, "No telemetry loaded.");
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

const hashLines = (lines) => {
  let hash = 0x811c9dc5;
  for (const line of lines) {
    const text = String(line ?? "");
    for (let i = 0; i < text.length; i += 1) {
      hash ^= text.charCodeAt(i);
      hash = Math.imul(hash, 0x01000193);
    }
    hash ^= 0x9e3779b9;
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
};

const ensureOverlayPlaceholder = () => {
  if (!hiveOverlays) {
    return;
  }
  if (!hiveOverlayPlaceholder) {
    hiveOverlayPlaceholder = document.createElement("p");
    hiveOverlayPlaceholder.className = "placeholder";
    hiveOverlayPlaceholder.textContent = "Awaiting telemetry.";
  }
  if (!hiveOverlayPlaceholder.isConnected) {
    hiveOverlays.appendChild(hiveOverlayPlaceholder);
  }
};

const buildOverlayCard = (agentId) => {
  const card = document.createElement("button");
  card.type = "button";
  card.className = "hive-telemetry__card";
  card.dataset.agent = agentId;
  const header = document.createElement("div");
  header.className = "hive-telemetry__agent";
  header.textContent = agentId;
  const lines = document.createElement("div");
  lines.className = "hive-telemetry__lines";
  card.appendChild(header);
  card.appendChild(lines);
  card.addEventListener("click", () => {
    selectHiveAgent(agentId);
  });
  card.__cohHeader = header;
  card.__cohLines = lines;
  card.__cohHash = null;
  return card;
};

const syncOverlayOrder = (order) => {
  if (!hiveOverlays) {
    return;
  }
  let needsReorder = hiveOverlayOrder.length !== order.length;
  if (!needsReorder) {
    for (let i = 0; i < order.length; i += 1) {
      if (hiveOverlayOrder[i] !== order[i]) {
        needsReorder = true;
        break;
      }
    }
  }
  if (!needsReorder) {
    return;
  }
  const fragment = document.createDocumentFragment();
  order.forEach((agentId) => {
    const card = hiveOverlayCards.get(agentId);
    if (card) {
      fragment.appendChild(card);
    }
  });
  hiveOverlays.appendChild(fragment);
  hiveOverlayOrder = order.slice();
};

const renderHiveOverlays = (batch) => {
  if (!hiveOverlays) {
    return;
  }
  const overlays = Array.isArray(batch.overlays) ? batch.overlays : [];
  if (!overlays.length) {
    for (const card of hiveOverlayCards.values()) {
      card.remove();
    }
    hiveOverlayCards.clear();
    hiveOverlayOrder = [];
    ensureOverlayPlaceholder();
    return;
  }
  if (hiveOverlayPlaceholder) {
    hiveOverlayPlaceholder.remove();
  }
  const nextOrder = [];
  const seen = new Set();
  overlays.forEach((overlay, idx) => {
    const agentId = overlay.agent || `unknown-${idx}`;
    nextOrder.push(agentId);
    seen.add(agentId);
    let card = hiveOverlayCards.get(agentId);
    if (!card) {
      card = buildOverlayCard(agentId);
      hiveOverlayCards.set(agentId, card);
      hiveOverlays.appendChild(card);
    }
    card.classList.toggle("selected", agentId === hiveDetailAgent);
    const linesArray = Array.isArray(overlay.lines) ? overlay.lines : [];
    const nextHash = hashLines(linesArray);
    if (card.__cohHash !== nextHash) {
      card.__cohHash = nextHash;
      const linesText = linesArray.join("\n");
      card.__cohLines.textContent = linesText;
    }
  });
  for (const [agentId, card] of hiveOverlayCards.entries()) {
    if (!seen.has(agentId)) {
      card.remove();
      hiveOverlayCards.delete(agentId);
    }
  }
  syncOverlayOrder(nextOrder);
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
    renderPlaceholder(hiveDetailLines, "Select a worker to view details.");
  } else {
    renderPlaceholder(hiveDetailLines, "No telemetry yet.");
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

  const columns = [
    { label: "ID", value: (entry) => entry?.id ?? "?" },
    { label: "ROLE", value: (entry) => entry?.role ?? "?" },
    { label: "PRIORITY", value: (entry) => formatCount(entry?.priority) },
    { label: "TICKS", value: (entry) => formatCount(entry?.ticks) },
    { label: "BUDGET", value: (entry) => `${formatCount(entry?.budget_ms)}ms` },
    { label: "SEQ", value: (entry) => formatCount(entry?.seq) },
  ];

  const header = document.createElement("div");
  header.className = "hive-schedule__row header";
  columns.forEach(({ label }) => {
    const cell = document.createElement("div");
    cell.textContent = label;
    header.appendChild(cell);
  });
  hiveScheduleQueue.appendChild(header);

  queue.forEach((entry) => {
    const row = document.createElement("div");
    row.className = "hive-schedule__row";
    columns.forEach(({ label, value }) => {
      const cell = document.createElement("div");
      cell.className = "hive-schedule__cell";
      cell.dataset.label = label;
      cell.textContent = value(entry);
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

const pollHive = async (generation = hivePollGeneration) => {
  if (!hiveActive || hivePollInFlight || generation !== hivePollGeneration) {
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
  if (!hiveActive || generation !== hivePollGeneration) {
    return;
  }
  if (!res.ok) {
    setStatus("hive-status", `Hive halted (${res.error})`);
    hiveActive = false;
    hivePollGeneration += 1;
    stopHivePolling();
    updateHiveRenderActive();
    return;
  }
  lastHiveBatch = res.result;
  if (!hiveRenderActive) {
    hiveUiNeedsFlush = true;
  } else {
    applyHiveBatch(res.result);
  }
  if (res.result.done) {
    hiveActive = false;
    hivePollGeneration += 1;
    stopHivePolling();
    return;
  }
  hivePollTimer = setTimeout(() => {
    pollHive(generation);
  }, hivePollInterval);
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
  const snapshotKey = readControlValue("hive-snapshot-key", "demo") || "demo";
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
  hiveUiNeedsFlush = false;
  setHiveScrollActive(false);
  bumpHiveInteraction();
  resetHiveErrors();
  resetHiveDetail();
  resetHiveSchedule();
  resetHiveLease();
  renderHiveOverlays({ overlays: [] });
  lastHiveBatch = null;
  hiveActive = true;
  hivePollGeneration += 1;
  updateHiveRenderActive();
  const statusPollMs = res.result.hive?.status_poll_ms || 500;
  hivePollInterval = Math.max(250, Math.floor(statusPollMs));
  stopHivePolling();
  pollHive(hivePollGeneration);
};

const stopHive = async () => {
  if (!hiveController) {
    return;
  }
  hiveActive = false;
  hivePollGeneration += 1;
  stopHivePolling();
  hiveController.stop();
  setHiveScrollActive(false);
  setHiveInteractionActive(false);
  hiveUiNeedsFlush = false;
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
  updateHiveRenderActive();
};

document.getElementById("hive-start")?.addEventListener("click", startHive);
document.getElementById("hive-stop")?.addEventListener("click", stopHive);
document
  .getElementById("hive-reset-view")
  ?.addEventListener("click", () => hiveController?.resetView());

const initializeMode = async () => {
  const mode = await readSwarmUiMode();
  offlineEnabled = Boolean(mode.offline);
  if (offlineButton) {
    setButtonLabel("offline", offlineEnabled ? "Online mode" : "Offline mode");
  }
  setConsoleAvailability(
    !offlineEnabled,
    "Console unavailable in offline snapshot replay mode.",
  );
  if (mode.hive_replay) {
    setStatus("hive-status", "Hive replay booting...");
    await startHive();
  }
};

setupConsole(invoke);
initializeMode();
