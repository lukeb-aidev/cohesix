// Author: Lukas Bower
// Purpose: Own transient UI session state and a bounded, credential-free action transcript.
// Copyright 2026 Lukas Bower
export const state = {
  connected: false,
  mode: "OFFLINE",
  view: "hive",
  roots: [],
  catalog: null,
  connection: null,
  hostBusy: false,
};
export const readControlValue = (id, fallback = "") =>
  String(document.getElementById(id)?.value ?? fallback).trim();
export const setStatus = (id, text) => {
  const node = document.getElementById(id);
  if (node) node.textContent = text;
};
export const readSession = () => ({
  role: state.connection?.role || readControlValue("session-role", "queen"),
  ticket: null,
});
const privateValues = new Set();
export const protect = (value) => {
  if (value) privateValues.add(value);
};
export function sanitize(text) {
  for (const value of privateValues)
    text = text.split(value).join("[redacted]");
  return text;
}
export function transcript(label, result) {
  const node = document.getElementById("action-transcript");
  const lines = result?.lines || [
    typeof result === "string" ? result : JSON.stringify(result, null, 2),
  ];
  const entry = document.createElement("section");
  const title = document.createElement("strong");
  title.textContent = label;
  const body = document.createElement("pre");
  body.textContent = sanitize(lines.join("\n")).slice(0, 32768);
  entry.append(title, body);
  node?.append(entry);
  while (node?.children.length > 32) node.firstElementChild.remove();
  setStatus("transcript-count", String(node?.children.length || 0));
}
export async function invoke(command, payload = {}) {
  const call =
    window.__TAURI__?.core?.invoke ||
    window.__TAURI__?.tauri?.invoke ||
    window.__TAURI__?.invoke;
  if (!call)
    return {
      ok: false,
      error: "Open the installed SwarmUI application to use a live connection.",
    };
  try {
    return { ok: true, result: await call(command, payload) };
  } catch (error) {
    return { ok: false, error: sanitize(String(error)) };
  }
}
export function setMode(mode) {
  state.mode = mode;
  setStatus("source-mode", mode);
  setStatus("presentation-mode", mode);
  document.body.dataset.mode = mode;
  setStatus(
    "proof-class",
    mode === "LIVE"
      ? "Live observation · proof per record"
      : "Retained observations · no live authority",
  );
  window.dispatchEvent(new CustomEvent("session-state", { detail: state }));
}
export function notice(text, bad = false) {
  const node = document.getElementById("notice");
  node.textContent = text;
  node.dataset.error = String(bad);
  node.hidden = false;
}
export function element(tag, text, className) {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = String(text);
  if (className) node.className = className;
  return node;
}
