const output = (id, text) => {
  const node = document.getElementById(id);
  if (!node) {
    return;
  }
  node.textContent = text;
};

const invoke = async (cmd, payload) => {
  const tauri = window.__TAURI__ && window.__TAURI__.tauri;
  if (!tauri || !tauri.invoke) {
    return { ok: false, error: "Tauri API unavailable" };
  }
  try {
    const result = await tauri.invoke(cmd, payload);
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
