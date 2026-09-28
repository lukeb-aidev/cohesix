// Author: Lukas Bower
// Purpose: Route workbench desks, restore focus and keep the local action palette bounded.
// Copyright 2026 Lukas Bower
import { state, setStatus, element } from "./state.js";
export const desks = {
  hive: [
    "Mission control",
    "YOUR HIVE, IN VIEW",
    "A clear view of your fleet. A deliberate path to every action.",
  ],
  operations: [
    "Make the next move",
    "GUIDED OPERATIONS",
    "Find an operation. Set its inputs. Review the exact action.",
  ],
  mlx: [
    "Run local MLX",
    "METAL AND VERIFIED RELEASES",
    "Plan and follow one admitted local model release on the selected Mac.",
  ],
  namespaces: [
    "Explore the namespace",
    "FILES WITH AUTHORITY",
    "Browse the Queen’s published state, one scoped path at a time.",
  ],
  story: [
    "Follow the run",
    "EXECUTION, IN CONTEXT",
    "Every transition keeps its source, identity and evidence.",
  ],
  flight: [
    "GPU flight deck",
    "HOST EXECUTION",
    "See the external machines doing the work.",
  ],
  tickets: [
    "Authority, made visible",
    "TICKETS & POLICY",
    "Understand what is admitted, what is refused and why.",
  ],
  evidence: [
    "Show your work",
    "EVIDENCE DESK",
    "Collect, inspect and verify the records behind an outcome.",
  ],
  replay: [
    "Return to the moment",
    "REPLAY DESK",
    "Inspect retained observations with the network out of the picture.",
  ],
  console: [
    "The exact conversation",
    "EXPERT CONSOLE",
    "The familiar console, when you want it.",
  ],
  settings: [
    "Make yourself at home",
    "SETTINGS",
    "Connection, display and installed tools.",
  ],
};
export function navigate(view) {
  if (!desks[view]) return;
  state.view = view;
  window.scrollTo({ top: 0, behavior: "instant" });
  document
    .querySelectorAll("[data-desk]")
    .forEach((n) => (n.hidden = n.dataset.desk !== view));
  document.querySelectorAll(".dock [data-view]").forEach((n) => {
    if (n.dataset.view === view) n.setAttribute("aria-current", "page");
    else n.removeAttribute("aria-current");
    n.setAttribute("aria-label", n.textContent.replace(/\s+/g, " ").trim());
  });
  const [title, eyebrow, description] = desks[view];
  setStatus("view-title", `${title}.`);
  setStatus("view-eyebrow", eyebrow);
  setStatus("view-description", description);
  history.replaceState(null, "", `#${view}`);
  window.dispatchEvent(new CustomEvent("workbench-route", { detail: view }));
  document.getElementById("workspace").focus({ preventScroll: true });
}
export function initializeNavigation() {
  document.querySelector(".wordmark")?.addEventListener("click", (event) => { event.preventDefault(); navigate("hive"); });
  document
    .querySelectorAll("[data-view]")
    .forEach((n) =>
      n.addEventListener("click", () => navigate(n.dataset.view)),
    );
  document
    .querySelectorAll("[data-close-dialog]")
    .forEach((n) =>
      n.addEventListener("click", () => n.closest("dialog").close()),
    );
  const drawer = document.getElementById("transcript-drawer");
  document.getElementById("transcript-toggle").addEventListener("click", () => {
    drawer.hidden = !drawer.hidden;
  });
  document.getElementById("transcript-close").addEventListener("click", () => {
    drawer.hidden = true;
    document.getElementById("transcript-toggle").focus();
  });
  document.getElementById("showcase-toggle").addEventListener("click", () => {
    document.body.classList.toggle("showcase");
    setStatus(
      "showcase-toggle",
      document.body.classList.contains("showcase")
        ? "Exit presentation"
        : "Presentation view",
    );
    window.dispatchEvent(new Event("resize"));
  });
  document.getElementById("reduced-motion").checked = matchMedia(
    "(prefers-reduced-motion: reduce)",
  ).matches;
  document.getElementById("reduced-motion").addEventListener("change", (e) => {
    document.body.classList.toggle("reduced-motion", e.target.checked);
    window.dispatchEvent(
      new CustomEvent("motion-preference", { detail: e.target.checked }),
    );
  });
  const palette = document.getElementById("palette-dialog"),
    search = document.getElementById("palette-search"),
    results = document.getElementById("palette-results");
  let commands = [];
  window.addEventListener("operation-catalog", (event) => {
    commands = event.detail.slice(0, 256);
  });
  const render = () => {
    results.replaceChildren();
    const query = search.value.toLowerCase();
    Object.entries(desks)
      .filter(([id, row]) =>
        [id, ...row].join(" ").toLowerCase().includes(query),
      )
      .forEach(([id, row]) => {
        const button = element("button", `${row[0]} →`);
        button.addEventListener("click", () => {
          palette.close();
          navigate(id);
        });
        results.append(button);
      });
    commands
      .filter((c) => `${c.name} ${c.help}`.toLowerCase().includes(query))
      .slice(0, 30)
      .forEach((c) => {
        const button = element("button", `${c.name} →`);
        button.addEventListener("click", () => {
          palette.close();
          window.dispatchEvent(
            new CustomEvent("select-operation", { detail: c.path }),
          );
        });
        results.append(button);
      });
  };
  const open = () => {
    search.value = "";
    render();
    palette.showModal();
    search.focus();
  };
  document.getElementById("palette-open").addEventListener("click", open);
  search.addEventListener("input", render);
  palette.addEventListener("keydown", (e) => {
    const buttons = [...results.querySelectorAll("button")];
    const index = buttons.indexOf(document.activeElement);
    if (e.key === "ArrowDown") {
      e.preventDefault();
      buttons[Math.min(index + 1, buttons.length - 1)]?.focus();
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (index <= 0) search.focus();
      else buttons[index - 1]?.focus();
    }
    if (e.key === "Enter" && document.activeElement === search) {
      e.preventDefault();
      buttons[0]?.click();
    }
  });
  document.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      if (!palette.open) open();
    }
    if (e.key === "Escape" && document.body.classList.contains("showcase"))
      document.getElementById("showcase-toggle").click();
    if (e.altKey && ["1", "2", "3"].includes(e.key)) {
      e.preventDefault();
      navigate(["hive", "operations", "namespaces"][Number(e.key) - 1]);
    }
  });
  navigate(location.hash.slice(1) || "hive");
}

const screenHelp = {
  hive: "Connect to your controller, then choose Start observing. Select a task worker to see its activity. Connected does not mean work has finished.",
  operations: "Find a task, fill in the form, then review it before running. Results come from the Cohesix program installed on this computer.",
  mlx: "Select the private deployment file, plan the release, then review its original ticket before starting. Follow its journal and signed result here. A model reply alone does not verify a canary or promotion.",
  namespaces: "Browse files published by the controller. Open a file to read it, or choose Recent activity for its latest lines. Your access permissions still apply.",
  story: "Open a report to follow a past run. Select a step to see the records behind it. Past results do not show what is running now.",
  flight: "See the computer and graphics processor used by a run. Unavailable means no measurement was recorded.",
  tickets: "Check your access permissions on the left. On the right, choose an action and review it before sending. The controller may refuse actions you are not allowed to perform.",
  evidence: "Open saved records to inspect what happened, check that they have not been altered, or export them. Checking records does not start new work.",
  replay: "Explore past recorded activity without changing the running system. Choose a sample run or open a saved recording. This closes your live connection.",
  console: "Prefer Operations for guided tasks. This optional screen accepts typed commands. Enter help for a list, or man followed by a command for instructions.",
  settings: "Set up your connection, reduce animation, or choose the installed Cohesix programs. Saved connection names never include passwords or access tokens.",
};

export function initializeHelp() {
  const tooltip = document.createElement("div");
  tooltip.id = "workbench-tooltip";
  tooltip.className = "workbench-tooltip";
  tooltip.setAttribute("role", "tooltip");
  tooltip.hidden = true;
  document.querySelector("sp-theme").append(tooltip);
  let active = null;
  let timer;
  const hide = () => {
    clearTimeout(timer);
    tooltip.hidden = true;
    active?.removeAttribute("aria-describedby");
    active = null;
  };
  const show = (target, text) => {
    hide();
    active = target;
    tooltip.textContent = typeof text === "function" ? text() : text;
    target.setAttribute("aria-describedby", tooltip.id);
    tooltip.hidden = false;
    const rect = target.getBoundingClientRect();
    const below = rect.bottom + tooltip.offsetHeight + 16 < innerHeight;
    tooltip.style.top = `${below ? rect.bottom + 8 : Math.max(8, rect.top - tooltip.offsetHeight - 8)}px`;
    tooltip.style.left = `${Math.max(8, Math.min(rect.left, innerWidth - tooltip.offsetWidth - 8))}px`;
  };
  const hints = {
    "context-help": () => screenHelp[state.view] || screenHelp.hive,
    "showcase-toggle": "Give this screen more space for presenting. Labels still show whether you are viewing current or past activity.",
    "transcript-toggle": "See what the app asked the controller or local program to do, and its exact reply. Useful when an action fails.",
    "namespace-tail": "Show the latest lines of this file. Access permissions and limits still apply.",
    "hive-snapshot-key": "A name for saving and reopening a recorded view of the system on this computer. It is not a password.",
  };
  for (const [id, text] of Object.entries(hints)) {
    const target = document.getElementById(id);
    target.addEventListener("pointerenter", () => {
      clearTimeout(timer);
      timer = setTimeout(() => show(target, text), 500);
    });
    target.addEventListener("pointerleave", () => {
      clearTimeout(timer);
      if (!target.matches(":focus-within")) timer = setTimeout(hide, 150);
    });
    target.addEventListener("focusin", () => show(target, text));
    target.addEventListener("focusout", hide);
    if (id === "context-help") target.addEventListener("click", () => show(target, text));
  }
  tooltip.addEventListener("pointerenter", () => clearTimeout(timer));
  tooltip.addEventListener("pointerleave", hide);
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") hide();
  });
  document.addEventListener("pointerdown", (event) => {
    if (active && !event.composedPath().includes(active)) hide();
  });
  window.addEventListener("workbench-route", hide);
  window.addEventListener("resize", hide);
  document.addEventListener("scroll", hide, true);
}
