// Author: Lukas Bower
// Purpose: Browse only generated namespace roots and show exact read/refusal transcripts.
// Copyright 2026 Lukas Bower
import {
  state,
  invoke,
  element,
  notice,
  transcript,
  readControlValue,
} from "./state.js";
const pathInput = document.getElementById("namespace-path");
const isProtocol = (line) => /^(OK |ERR |ACK |END$|coh>|cohesix>)/.test(line);
export async function readNamespace(verb, path, quiet = false) {
  const entries = document.getElementById("namespace-entries");
  entries.replaceChildren();
  const response = await invoke("swarmui_namespace", { verb, path });
  const result = response.ok
    ? response.result
    : { ok: false, lines: [response.error] };
  transcript(`${verb} ${path}`, result);
  document.getElementById("namespace-output").textContent = (
    result.lines || []
  ).join("\n");
  if (!result.ok) {
    document.getElementById("namespace-kind").textContent =
      "Unavailable or refused · inspect exact reason";
    if (!quiet) notice((result.lines || []).join("\n"), true);
    return result;
  }
  document.getElementById("namespace-kind").textContent =
    verb === "ls" ? "Directory · scoped read" : "File · scoped read";
  notice(`${verb === "ls" ? "Opened" : "Read"} ${path}`);
  if (verb === "ls") {
    for (const line of (result.lines || [])
      .filter((l) => l.trim() && !isProtocol(l))
      .slice(0, 256)) {
      const raw = line.trim();
      const name = raw.split(/\s+/)[0];
      if (name === "." || name === ".." || /[\x00-\x1f]/.test(name)) continue;
      const child = name.startsWith("/")
        ? name
        : `${path.replace(/\/$/, "")}/${name.replace(/\/$/, "")}`;
      const button = element("button");
      button.append(element("span", name), element("small", "Open →"));
      button.addEventListener("click", () => {
        pathInput.value = child;
        browse();
      });
      entries.append(button);
    }
    if (!entries.children.length)
      entries.append(
        element("p", "This directory has no visible entries.", "empty-copy"),
      );
  }
  return result;
}
async function browse() {
  const path = pathInput.value.trim();
  const kind =
    path === "/worker" || path.startsWith("/worker/")
      ? "Legacy alias · profile dependent"
      : path.endsWith("/ctl")
        ? "Control file · explicit append only"
        : path.startsWith("/policy")
          ? "Generated policy · scoped read"
          : /\/(queue|journal|decisions|log)$/.test(path)
            ? "Bounded journal · scoped read"
            : "Scoped · read only";
  document.getElementById("namespace-kind").textContent = kind;
  document.getElementById("namespace-title").textContent = path;
  const crumbs = document.getElementById("namespace-breadcrumbs");
  crumbs.replaceChildren();
  let current = "";
  for (const component of path.split("/").filter(Boolean)) {
    current += `/${component}`;
    const target = current;
    const button = element("button", `${component} /`);
    button.addEventListener("click", () => {
      pathInput.value = target;
      browse();
    });
    crumbs.append(button);
  }
  const result = await readNamespace("ls", path, true);
  if (!result.ok) {
    // Only a typed path-kind refusal permits a read-only file probe.
    // Authentication and capability refusals never trigger another operation.
    const lines = (result.lines || []).join("\n");
    if (/\b(?:detail=invalid-path|ENOTDIR|not-directory)\b/.test(lines) &&
        !/\b(?:AUTH|unauthorized|forbidden|scope|capability)\b/i.test(lines)) {
      await readNamespace("cat", path);
    } else {
      notice(lines, true);
    }
  }
}
export function initializeNamespace(roots) {
  state.roots = roots;
  const tree = document.getElementById("namespace-roots");
  for (const path of roots) {
    const button = element("button", `${path} →`);
    button.addEventListener("click", () => {
      pathInput.value = path;
      browse();
    });
    tree.append(button);
  }
  document.getElementById("namespace-go").addEventListener("click", browse);
  pathInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") browse();
  });
  document.getElementById("namespace-up").addEventListener("click", () => {
    const parts = pathInput.value.split("/").filter(Boolean);
    if (parts.length > 1) parts.pop();
    pathInput.value = `/${parts.join("/")}`;
    browse();
  });
  document
    .getElementById("namespace-read")
    .addEventListener("click", () =>
      readNamespace("cat", readControlValue("namespace-path")),
    );
  document
    .getElementById("namespace-tail")
    .addEventListener("click", () =>
      readNamespace("tail", readControlValue("namespace-path")),
    );
  document
    .getElementById("namespace-copy")
    .addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(pathInput.value);
        notice("Namespace path copied.");
      } catch {
        pathInput.select();
        notice("Select and copy the highlighted path.");
      }
    });
  document
    .getElementById("namespace-transcript")
    .addEventListener(
      "click",
      () => (document.getElementById("transcript-drawer").hidden = false),
    );
  document.querySelectorAll("[data-read-path]").forEach((n) =>
    n.addEventListener("click", async () => {
      const response = await invoke("swarmui_namespace", {
        verb: "cat",
        path: n.dataset.readPath,
      });
      const result = response.ok
        ? response.result
        : { ok: false, lines: [response.error] };
      document.getElementById("authority-output").textContent =
        result.lines.join("\n");
      transcript(n.dataset.readPath, result);
    }),
  );
}
