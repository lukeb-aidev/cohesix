// Author: Lukas Bower
// Purpose: Guide session setup and retain only non-secret connection profiles.
// Copyright 2026 Lukas Bower
import {
  state,
  invoke,
  setMode,
  setStatus,
  protect,
  notice,
  transcript,
  readControlValue,
} from "./state.js";
const dialog = document.getElementById("connection-dialog");
const profileKey = "cohesix.connection-profiles.v1";
let connecting = false;
function profiles() {
  try {
    const values = JSON.parse(localStorage.getItem(profileKey) || "[]");
    return Array.isArray(values)
      ? values
          .slice(0, 16)
          .filter(
            (p) =>
              p &&
              typeof p.name === "string" &&
              p.name.length <= 64 &&
              typeof p.endpoint === "string" &&
              p.endpoint.length <= 2048 &&
              ["rest", "console"].includes(p.transport),
          )
      : [];
  } catch {
    return [];
  }
}
function renderProfiles() {
  const select = document.getElementById("connection-profile");
  select.replaceChildren(new Option("New connection", ""));
  profiles().forEach((p, index) =>
    select.add(new Option(p.name, String(index))),
  );
}
function updateHint() {
  const direct =
    document.querySelector('[name="transport"]:checked')?.value === "console";
  const role = document.getElementById("session-role");
  for (const option of role.options) option.disabled = !direct && option.value !== "queen";
  if (!direct) role.value = "queen";
  setStatus(
    "connection-hint",
    direct
      ? "The Queen console has one owner. Disconnect other direct clients or use a gateway to share it."
      : "Use your gateway’s base URL. Its delegated identity and policy still apply.",
  );
  document.getElementById("connection-endpoint").placeholder = direct
    ? "tcp://192.168.0.10:31337"
    : "https://gateway.example:8443";
}
export function openConnection() {
  renderProfiles();
  dialog.showModal();
  document.getElementById("connection-endpoint").focus();
}
export function reflectSession(info) {
  state.connection = info.connection || null;
  state.connected = Boolean(info.connection);
  const mode = info.mode || {};
  setMode(
    info.fixture
      ? "FIXTURE"
      : state.connected && !mode.offline
        ? "LIVE"
        : mode.trace_replay || mode.hive_replay
          ? "REPLAY"
          : "OFFLINE",
  );
  const disabled = Boolean(mode.offline);
  document
    .querySelector(".console-panel")
    .setAttribute("aria-disabled", String(disabled));
  for (const id of ["console-input", "console-send", "console-dump-log"])
    document.getElementById(id).disabled = disabled;
  if (disabled)
    setStatus(
      "console-output",
      "Console unavailable in offline snapshot replay mode.",
    );
  setStatus("connection-open", info.connection?.endpoint || "Connect a hive");
  setStatus("connection-role", info.connection ? `${info.connection.role}${info.connection.delegated ? " · delegated" : ""}` : "No active session");
  document
    .getElementById("connection-dot")
    .classList.toggle("connected", state.connected && !mode.offline);
  document.getElementById("welcome-strip").hidden =
    state.connected || state.mode === "REPLAY";
  if (!state.connected) {
    setStatus("metric-queen", "Not connected");
    setStatus("metric-workers", "—");
    setStatus("metric-pressure", "—");
  }
}
export function initializeSession() {
  for (const id of ["connection-open", "welcome-connect", "settings-connect"])
    document.getElementById(id).addEventListener("click", openConnection);
  document
    .querySelectorAll('[name="transport"]')
    .forEach((n) => n.addEventListener("change", updateHint));
  document
    .getElementById("connection-profile")
    .addEventListener("change", (e) => {
      if (e.target.value === "") return;
      const p = profiles()[Number(e.target.value)];
      if (!p) return;
      for (const [id, value] of [
        ["connection-endpoint", p.endpoint],
        ["connection-name", p.name],
        ["session-role", p.role],
      ])
        document.getElementById(id).value = value;
      document.querySelector(
        `[name="transport"][value="${p.transport}"]`,
      ).checked = true;
      updateHint();
    });
  const connect = async (e) => {
    e.preventDefault();
    if (connecting) return;
    const form = document.getElementById("connection-form");
    if (!form.reportValidity()) return;
    connecting = true;
    document.getElementById("connect").disabled = true;
    window.dispatchEvent(new Event("session-changing"));
    const request = {
      transport: document.querySelector('[name="transport"]:checked').value,
      endpoint: readControlValue("connection-endpoint"),
      credential: readControlValue("connection-credential"),
      role: readControlValue("session-role"),
      ticket: readControlValue("session-ticket") || null,
    };
    protect(request.credential);
    protect(request.ticket);
    setStatus("connection-result", "Connecting and checking your authority…");
    const response = await invoke("swarmui_session_open", { request });
    const info = await invoke("swarmui_workbench_info");
    if (info.ok) reflectSession(info.result);
    connecting = false;
    document.getElementById("connect").disabled = false;
    if (!response.ok || !response.result?.ok) {
      const message =
        response.error ||
        response.result?.lines?.join("\n") ||
        "Connection refused";
      setStatus("connection-result", message);
      transcript("Connection refused", message);
      return;
    }
    const name = readControlValue("connection-name");
    if (name) {
      const safe = {
        name,
        endpoint: request.endpoint,
        transport: request.transport,
        role: request.role,
      };
      try {
        localStorage.setItem(
          profileKey,
          JSON.stringify(
            [safe, ...profiles().filter((p) => p.name !== name)].slice(0, 16),
          ),
        );
      } catch {
        notice("Connected. This device could not save the non-secret profile.");
      }
    }
    document.getElementById("connection-credential").value = "";
    document.getElementById("session-ticket").value = "";
    setStatus("connection-result", "");
    transcript("Connection", response.result);
    dialog.close();
    notice("Connected. Choose an operation or start observing your hive.");
    window.dispatchEvent(new Event("session-connected"));
  };
  document
    .getElementById("connection-form")
    .addEventListener("submit", connect);
  document.getElementById("connect").addEventListener("click", connect);
  document.getElementById("disconnect").addEventListener("click", async () => {
    window.dispatchEvent(new Event("session-changing"));
    const res = await invoke("swarmui_session_close");
    if (!res.ok) {
      setStatus("connection-result", res.error);
      return;
    }
    reflectSession({ connection: null, mode: { offline: true } });
    dialog.close();
    notice("Disconnected. Open a retained artifact or connect another hive.");
  });
  document.getElementById("profile-forget").addEventListener("click", () => {
    localStorage.removeItem(profileKey);
    renderProfiles();
    notice("Saved connection profiles removed.");
  });
  document
    .getElementById("tool-directory-save")
    .addEventListener("click", async () => {
      const res = await invoke("swarmui_tool_directory", {
        path: readControlValue("tool-directory"),
      });
      notice(
        res.ok
          ? "Installed tools selected. Their schema is checked before every operation."
          : res.error,
        !res.ok,
      );
    });
  document.getElementById("mint-ticket").addEventListener("click", async () => {
    const response = await invoke("swarmui_mint_ticket", {
      role: readControlValue("session-role"),
      subject: readControlValue("session-subject") || null,
    });
    if (!response.ok) {
      setStatus("mint-status", response.error);
      return;
    }
    protect(response.result);
    document.getElementById("session-ticket").value = response.result;
    setStatus(
      "mint-status",
      "Ticket minted. Open Manage connection to attach with it.",
    );
  });
  renderProfiles();
  updateHint();
}
