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
  const raw = document.getElementById("worker-id")?.value || "worker-1";
  const trimmed = raw.trim();
  return trimmed.length ? trimmed : "worker-1";
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
    const res = await invoke("swarmui_tail_telemetry", {
      role: session.role,
      ticket: session.ticket,
      worker_id: workerId,
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
const hivePressure = document.getElementById("hive-pressure");
const hiveFallback = document.getElementById("hive-fallback");
let hiveController = null;
let hiveInitError = null;
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
    hiveController = createHiveController(hiveCanvas, hiveStatus);
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

const updateHivePressure = (batch) => {
  if (!hivePressure) {
    return;
  }
  const pressure = batch.pressure ?? 0;
  const backlog = batch.backlog ?? 0;
  const dropped = batch.dropped ?? 0;
  hivePressure.textContent = `Pressure ${(pressure * 100).toFixed(0)}% · backlog ${backlog} · dropped ${dropped}`;
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
  });
  hivePollInFlight = false;
  if (!res.ok) {
    setStatus("hive-status", `Hive halted (${res.error})`);
    hiveActive = false;
    stopHivePolling();
    return;
  }
  hiveController?.ingest(res.result);
  updateHivePressure(res.result);
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
